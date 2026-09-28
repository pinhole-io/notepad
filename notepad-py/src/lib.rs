//! Python extension: notepad

mod task_registry;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use notepad::{
    Entry, EntryId, Notepad, NotepadSynthesizer, OffloadConfig, RenderFormat, RenderOpts,
    StoredPayload, SynthesisConfig, SynthesisOutcome, is_notepad_stub as notepad_is_stub,
    offload_config_from_json, synthesis_user_prompt as build_synthesis_prompt,
};
use tokio::sync::RwLock as AsyncRwLock;

#[link(name = "notepad_bridge")]
unsafe extern "C" {
    fn notepad_bridge_register_arc(np_arc: *const AsyncRwLock<Notepad>) -> u64;
    fn notepad_bridge_release(handle: u64);
    fn notepad_bridge_attach_tools(registry: *const std::ffi::c_void, handle: u64) -> i32;
    fn notepad_bridge_attach_offload_hook(
        hooks: *const std::ffi::c_void,
        handle: u64,
        config_json: *const i8,
    ) -> i32;
    fn notepad_bridge_compact_tool_messages(
        messages_json: *const i8,
        out_buf: *mut i8,
        out_len: usize,
        handle: u64,
        config_json: *const i8,
    ) -> i32;
}
use pyo3::IntoPyObjectExt;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use serde_json::Value;
use superglue::client::{BindingBootstrapConfig, ClientBuildError, bootstrap_from_parts};
use superglue::http::HttpClient;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().expect("tokio runtime"))
}

fn parse_entry_id(id: &str) -> PyResult<EntryId> {
    let uuid: uuid::Uuid = id
        .parse()
        .map_err(|_| PyValueError::new_err(format!("invalid entry id: {id}")))?;
    Ok(EntryId(uuid))
}

fn py_to_json_value(py: Python<'_>, obj: Bound<'_, PyAny>) -> PyResult<Value> {
    let json_mod = py.import("json")?;
    let s: String = json_mod.call_method1("dumps", (obj,))?.extract()?;
    serde_json::from_str(&s).map_err(|e| PyValueError::new_err(format!("json: {e}")))
}

fn payload_from_py(py: Python<'_>, content: Bound<'_, PyAny>) -> PyResult<StoredPayload> {
    if content.is_instance_of::<pyo3::types::PyString>() {
        return Ok(StoredPayload::Text(content.extract()?));
    }
    if content.is_instance_of::<PyDict>() {
        return Ok(StoredPayload::Json(py_to_json_value(py, content)?));
    }
    Err(PyValueError::new_err(
        "content must be a str or dict (JSON object)",
    ))
}

fn entry_to_dict(py: Python<'_>, entry: &Entry) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("id", entry.id.to_string())?;
    dict.set_item("label", entry.label.clone())?;
    dict.set_item("tags", entry.tags.clone())?;
    dict.set_item(
        "payload",
        match &entry.payload {
            StoredPayload::Text(s) => s.clone().into_py_any(py)?,
            StoredPayload::Json(v) => {
                let json_mod = py.import("json")?;
                json_mod
                    .call_method1("loads", (v.to_string(),))?
                    .into_py_any(py)?
            }
        },
    )?;
    dict.set_item(
        "metadata",
        match serde_json::to_string(&entry.metadata) {
            Ok(s) => {
                let json_mod = py.import("json")?;
                json_mod.call_method1("loads", (s,))?
            }
            Err(_) => PyDict::new(py).into_any(),
        },
    )?;
    dict.set_item("created_at", entry.created_at.to_string())?;
    Ok(dict.into())
}

fn render_format_from_str(s: &str) -> PyResult<RenderFormat> {
    match s {
        "markdown" => Ok(RenderFormat::Markdown),
        "json" => Ok(RenderFormat::Json),
        other => Err(PyValueError::new_err(format!(
            "format must be 'markdown' or 'json', got {other:?}"
        ))),
    }
}

fn render_opts_from_py(
    format: Option<&str>,
    include_metadata: Option<bool>,
    max_chars: Option<usize>,
) -> PyResult<RenderOpts> {
    Ok(RenderOpts {
        format: format
            .map(render_format_from_str)
            .transpose()?
            .unwrap_or_default(),
        include_metadata: include_metadata.unwrap_or(true),
        max_chars,
    })
}

fn lock_err(e: impl std::fmt::Display) -> PyErr {
    PyRuntimeError::new_err(format!("lock: {e}"))
}

#[pyclass(name = "OffloadConfig")]
#[derive(Clone)]
struct PyOffloadConfig {
    inner: OffloadConfig,
}

#[pymethods]
impl PyOffloadConfig {
    #[new]
    #[pyo3(signature = (
        auto_offload = true,
        min_chars = 2048,
        always_offload_tools = None,
        never_offload_tools = None,
        preview_chars = 200,
    ))]
    fn new(
        auto_offload: bool,
        min_chars: usize,
        always_offload_tools: Option<Vec<String>>,
        never_offload_tools: Option<Vec<String>>,
        preview_chars: usize,
    ) -> Self {
        Self {
            inner: OffloadConfig {
                auto_offload,
                min_chars,
                always_offload_tools: always_offload_tools
                    .unwrap_or_else(|| vec!["charlotte_site_crawl_chunks".into()]),
                never_offload_tools: never_offload_tools.unwrap_or_default(),
                preview_chars,
            },
        }
    }

    #[getter]
    fn auto_offload(&self) -> bool {
        self.inner.auto_offload
    }

    #[getter]
    fn min_chars(&self) -> usize {
        self.inner.min_chars
    }

    #[getter]
    fn always_offload_tools(&self) -> Vec<String> {
        self.inner.always_offload_tools.clone()
    }

    #[getter]
    fn never_offload_tools(&self) -> Vec<String> {
        self.inner.never_offload_tools.clone()
    }

    #[getter]
    fn preview_chars(&self) -> usize {
        self.inner.preview_chars
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("autoOffload", self.inner.auto_offload)?;
        dict.set_item("minChars", self.inner.min_chars)?;
        dict.set_item("alwaysOffloadTools", &self.inner.always_offload_tools)?;
        dict.set_item("neverOffloadTools", &self.inner.never_offload_tools)?;
        dict.set_item("previewChars", self.inner.preview_chars)?;
        Ok(dict.into())
    }

    fn __repr__(&self) -> String {
        format!("OffloadConfig(auto_offload={})", self.inner.auto_offload)
    }
}

#[pyclass(name = "Notepad")]
struct PyNotepad {
    inner: Arc<RwLock<Notepad>>,
    bridge_handle: u64,
}

impl Drop for PyNotepad {
    fn drop(&mut self) {
        unsafe { notepad_bridge_release(self.bridge_handle) };
    }
}

#[pymethods]
impl PyNotepad {
    #[new]
    fn new() -> Self {
        let inner = Arc::new(RwLock::new(Notepad::new()));
        let bridge_handle = unsafe {
            let raw = Arc::into_raw(Arc::clone(&inner));
            notepad_bridge_register_arc(raw)
        };
        Self {
            inner,
            bridge_handle,
        }
    }

    fn native_handle(&self) -> u64 {
        self.bridge_handle
    }

    #[getter]
    fn session_id(&self) -> PyResult<String> {
        let np = self.inner.blocking_read();
        Ok(np.session_id().to_string())
    }

    fn store(&self, py: Python<'_>, content: Bound<'_, PyAny>) -> PyResult<String> {
        let payload = payload_from_py(py, content)?;
        let mut np = self.inner.blocking_write();
        Ok(np.store(payload).to_string())
    }

    #[pyo3(signature = (content, tags=None, label=None, metadata=None))]
    fn store_tagged(
        &self,
        py: Python<'_>,
        content: Bound<'_, PyAny>,
        tags: Option<Vec<String>>,
        label: Option<String>,
        metadata: Option<Bound<'_, PyDict>>,
    ) -> PyResult<String> {
        let payload = payload_from_py(py, content)?;
        let tags_vec = tags.unwrap_or_default();
        let tag_refs: Vec<&str> = tags_vec.iter().map(String::as_str).collect();
        let meta: HashMap<String, Value> = if let Some(d) = metadata {
            match py_to_json_value(py, d.into_any())? {
                Value::Object(m) => m.into_iter().collect(),
                _ => HashMap::new(),
            }
        } else {
            HashMap::new()
        };
        let mut np = self.inner.blocking_write();
        Ok(np
            .store_tagged(&tag_refs, label.as_deref(), payload, meta)
            .to_string())
    }

    fn get(&self, py: Python<'_>, entry_id: &str) -> PyResult<Option<Py<PyDict>>> {
        let id = parse_entry_id(entry_id)?;
        let np = self.inner.blocking_read();
        Ok(np.get(id).map(|e| entry_to_dict(py, e)).transpose()?)
    }

    fn remove(&self, py: Python<'_>, entry_id: &str) -> PyResult<Option<Py<PyDict>>> {
        let id = parse_entry_id(entry_id)?;
        let mut np = self.inner.blocking_write();
        Ok(np.remove(id).map(|e| entry_to_dict(py, &e)).transpose()?)
    }

    fn entries(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        let np = self.inner.blocking_read();
        let list = PyList::empty(py);
        for entry in np.entries() {
            list.append(entry_to_dict(py, entry)?)?;
        }
        Ok(list.into())
    }

    fn __len__(&self) -> PyResult<usize> {
        let np = self.inner.blocking_read();
        Ok(np.len())
    }

    fn is_empty(&self) -> PyResult<bool> {
        let np = self.inner.blocking_read();
        Ok(np.is_empty())
    }

    fn total_chars(&self) -> PyResult<usize> {
        let np = self.inner.blocking_read();
        Ok(np.total_chars())
    }

    #[pyo3(signature = (format=None, include_metadata=None, max_chars=None))]
    fn render(
        &self,
        format: Option<&str>,
        include_metadata: Option<bool>,
        max_chars: Option<usize>,
    ) -> PyResult<String> {
        let opts = render_opts_from_py(format, include_metadata, max_chars)?;
        let np = self.inner.blocking_read();
        Ok(np.render(&opts))
    }

    fn partition(&self, max_chars: usize) -> PyResult<Vec<Vec<String>>> {
        let np = self.inner.blocking_read();
        Ok(np
            .partition(max_chars)
            .into_iter()
            .map(|batch| batch.into_iter().map(|id| id.to_string()).collect())
            .collect())
    }

    fn __repr__(&self) -> PyResult<String> {
        let np = self.inner.blocking_read();
        Ok(format!(
            "Notepad(session_id={:?}, len={})",
            np.session_id(),
            np.len()
        ))
    }
}

#[pyclass(name = "SynthesisOutcome")]
struct PySynthesisOutcome {
    #[pyo3(get)]
    answer: String,
    #[pyo3(get)]
    passes: u32,
}

#[pyclass(name = "Synthesizer")]
struct PySynthesizer {
    notepad: Py<PyNotepad>,
    http: HttpClient,
    chat_opts: superglue::chat::ChatOptions,
    config: SynthesisConfig,
}

#[pymethods]
impl PySynthesizer {
    #[new]
    #[pyo3(signature = (
        notepad,
        api_key,
        model = "gpt-5.4-nano-2026-03-17-mini",
        base_url = "https://api.openai.com",
        system_prompt = None,
        max_context_chars = 120_000,
        synthesis_budget_ratio = 0.8,
        extraction_budget_ratio = 0.6,
        max_extraction_rounds = 8,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        notepad: Py<PyNotepad>,
        api_key: String,
        model: &str,
        base_url: &str,
        system_prompt: Option<String>,
        max_context_chars: usize,
        synthesis_budget_ratio: f32,
        extraction_budget_ratio: f32,
        max_extraction_rounds: u32,
    ) -> PyResult<Self> {
        let bootstrap = bootstrap_from_parts(BindingBootstrapConfig {
            api_key,
            base_url: base_url.to_string(),
            model: model.to_string(),
            system_prompt,
            ..Default::default()
        })
        .map_err(client_build_err)?;
        Ok(Self {
            notepad,
            http: bootstrap.http,
            chat_opts: bootstrap.options,
            config: SynthesisConfig {
                max_context_chars,
                synthesis_budget_ratio,
                extraction_budget_ratio,
                max_extraction_rounds,
            },
        })
    }

    fn synthesize(&self, py: Python<'_>, question: &str) -> PyResult<PySynthesisOutcome> {
        let np = Arc::clone(&self.notepad.bind(py).borrow().inner);
        let http = self.http.clone();
        let chat_opts = self.chat_opts.clone();
        let config = self.config.clone();
        let q = question.to_string();
        let outcome = py.detach(move || -> PyResult<SynthesisOutcome> {
            runtime().block_on(async move {
                let np = np.read().await;
                let synth = NotepadSynthesizer::new(&np, &http, chat_opts).with_config(config);
                synth
                    .synthesize(&q)
                    .await
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))
            })
        })?;
        Ok(PySynthesisOutcome {
            answer: outcome.answer,
            passes: outcome.passes,
        })
    }
}

fn client_build_err(e: ClientBuildError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pyfunction]
fn is_notepad_stub(content: &str) -> bool {
    notepad_is_stub(content)
}

#[pyfunction]
fn synthesis_user_prompt(question: &str, material: &str) -> String {
    build_synthesis_prompt(question, material)
}

#[pyfunction]
#[pyo3(signature = (obj))]
fn offload_config_from_dict(py: Python<'_>, obj: Bound<'_, PyAny>) -> PyResult<PyOffloadConfig> {
    let value = py_to_json_value(py, obj)?;
    let inner = offload_config_from_json(&value).map_err(PyValueError::new_err)?;
    Ok(PyOffloadConfig { inner })
}

#[pyfunction]
fn bridge_register_tools(registry_ptr: u64, notepad_handle: u64) -> PyResult<()> {
    if registry_ptr == 0 || notepad_handle == 0 {
        return Err(PyValueError::new_err(
            "registry_ptr and notepad_handle must be non-zero",
        ));
    }
    let rc = unsafe {
        notepad_bridge_attach_tools(registry_ptr as *const std::ffi::c_void, notepad_handle)
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(PyRuntimeError::new_err(format!(
            "notepad_bridge_attach_tools failed ({rc})"
        )))
    }
}

#[pyfunction]
#[pyo3(signature = (hooks_ptr, notepad_handle, config=None))]
fn bridge_register_offload_hook(
    py: Python<'_>,
    hooks_ptr: u64,
    notepad_handle: u64,
    config: Option<Bound<'_, PyAny>>,
) -> PyResult<()> {
    if hooks_ptr == 0 || notepad_handle == 0 {
        return Err(PyValueError::new_err(
            "hooks_ptr and notepad_handle must be non-zero",
        ));
    }
    let config_json = if let Some(obj) = config {
        let json_mod = py.import("json")?;
        Some(
            json_mod
                .call_method1("dumps", (obj,))?
                .extract::<String>()?,
        )
    } else {
        None
    };
    let cfg_c = config_json
        .as_ref()
        .map(|s| {
            std::ffi::CString::new(s.as_str()).map_err(|e| PyValueError::new_err(e.to_string()))
        })
        .transpose()?;
    let cfg_ptr = cfg_c
        .as_ref()
        .map(|c| c.as_ptr())
        .unwrap_or(std::ptr::null());
    let rc = unsafe {
        notepad_bridge_attach_offload_hook(
            hooks_ptr as *const std::ffi::c_void,
            notepad_handle,
            cfg_ptr,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(PyRuntimeError::new_err(format!(
            "notepad_bridge_attach_offload_hook failed ({rc})"
        )))
    }
}

#[pyfunction]
#[pyo3(signature = (messages, notepad_handle, config=None))]
fn bridge_compact_tool_messages(
    py: Python<'_>,
    messages: Bound<'_, PyAny>,
    notepad_handle: u64,
    config: Option<Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    if notepad_handle == 0 {
        return Err(PyValueError::new_err("notepad_handle must be non-zero"));
    }
    let json_mod = py.import("json")?;
    let messages_json: String = json_mod.call_method1("dumps", (messages,))?.extract()?;
    let config_json: Option<String> = if let Some(obj) = config {
        Some(json_mod.call_method1("dumps", (obj,))?.extract()?)
    } else {
        None
    };
    let mut buf = vec![0u8; messages_json.len() + 65536];
    let cfg_c = config_json
        .as_ref()
        .map(|s| {
            std::ffi::CString::new(s.as_str()).map_err(|e| PyValueError::new_err(e.to_string()))
        })
        .transpose()?;
    let cfg_ptr = cfg_c
        .as_ref()
        .map(|c| c.as_ptr())
        .unwrap_or(std::ptr::null());
    let msg_c =
        std::ffi::CString::new(messages_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let rc = unsafe {
        notepad_bridge_compact_tool_messages(
            msg_c.as_ptr(),
            buf.as_mut_ptr() as *mut i8,
            buf.len(),
            notepad_handle,
            cfg_ptr,
        )
    };
    if rc != 0 {
        return Err(PyRuntimeError::new_err(format!(
            "notepad_bridge_compact_tool_messages failed ({rc})"
        )));
    }
    let out = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr() as *const i8) };
    let out_str = out
        .to_str()
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    Ok(json_mod.call_method1("loads", (out_str,))?.unbind())
}

#[pymodule(gil_used = false)]
fn _notepad(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyNotepad>()?;
    m.add_class::<PySynthesizer>()?;
    m.add_class::<PySynthesisOutcome>()?;
    m.add_class::<PyOffloadConfig>()?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(is_notepad_stub, m)?)?;
    m.add_function(wrap_pyfunction!(synthesis_user_prompt, m)?)?;
    m.add_function(wrap_pyfunction!(offload_config_from_dict, m)?)?;
    m.add_function(wrap_pyfunction!(bridge_register_tools, m)?)?;
    m.add_function(wrap_pyfunction!(bridge_register_offload_hook, m)?)?;
    m.add_function(wrap_pyfunction!(bridge_compact_tool_messages, m)?)?;
    task_registry::register_task_registry(m)?;
    Ok(())
}
