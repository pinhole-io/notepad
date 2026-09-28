#![deny(clippy::all)]

mod task_registry;

use std::collections::HashMap;
use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use notepad::{
    Entry, EntryId, Notepad, NotepadSynthesizer, OffloadConfig, RenderFormat, RenderOpts,
    StoredPayload, SynthesisConfig, is_notepad_stub as notepad_is_stub, offload_config_from_json,
    offload_config_to_json, synthesis_user_prompt as build_synthesis_prompt,
};
use serde_json::{Value, json};
use superglue::client::{BindingBootstrapConfig, bootstrap_from_parts};
pub use task_registry::TaskRegistryJs;
pub use task_registry::TaskRegistryJs as TaskRegistry;
use tokio::sync::RwLock;

#[link(name = "notepad_bridge")]
unsafe extern "C" {
    fn notepad_bridge_register_arc(np_arc: *const RwLock<Notepad>) -> u64;
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

fn parse_entry_id(id: &str) -> Result<EntryId> {
    let uuid: uuid::Uuid = id
        .parse()
        .map_err(|_| Error::from_reason(format!("invalid entry id: {id}")))?;
    Ok(EntryId(uuid))
}

fn payload_from_value(v: Value) -> Result<StoredPayload> {
    match v {
        Value::String(s) => Ok(StoredPayload::Text(s)),
        Value::Object(_) | Value::Array(_) | Value::Number(_) | Value::Bool(_) | Value::Null => {
            Ok(StoredPayload::Json(v))
        }
    }
}

fn entry_to_value(entry: &Entry) -> Value {
    let payload = match &entry.payload {
        StoredPayload::Text(s) => json!(s),
        StoredPayload::Json(v) => v.clone(),
    };
    json!({
        "id": entry.id.to_string(),
        "label": entry.label,
        "tags": entry.tags,
        "payload": payload,
        "metadata": entry.metadata,
        "createdAt": entry.created_at.to_string(),
    })
}

fn render_format_from_str(s: &str) -> Result<RenderFormat> {
    match s {
        "markdown" => Ok(RenderFormat::Markdown),
        "json" => Ok(RenderFormat::Json),
        other => Err(Error::from_reason(format!(
            "format must be 'markdown' or 'json', got {other:?}"
        ))),
    }
}

#[napi(object, js_name = "OffloadConfig")]
pub struct OffloadConfigJs {
    #[napi(js_name = "autoOffload")]
    pub auto_offload: Option<bool>,
    #[napi(js_name = "minChars")]
    pub min_chars: Option<u32>,
    #[napi(js_name = "alwaysOffloadTools")]
    pub always_offload_tools: Option<Vec<String>>,
    #[napi(js_name = "neverOffloadTools")]
    pub never_offload_tools: Option<Vec<String>>,
    #[napi(js_name = "previewChars")]
    pub preview_chars: Option<u32>,
}

impl OffloadConfigJs {
    fn to_inner(&self) -> OffloadConfig {
        OffloadConfig {
            auto_offload: self.auto_offload.unwrap_or(true),
            min_chars: self.min_chars.unwrap_or(2048) as usize,
            always_offload_tools: self
                .always_offload_tools
                .clone()
                .unwrap_or_else(|| vec!["charlotte_site_crawl_chunks".into()]),
            never_offload_tools: self.never_offload_tools.clone().unwrap_or_default(),
            preview_chars: self.preview_chars.unwrap_or(200) as usize,
        }
    }
}

#[napi(js_name = "Notepad")]
pub struct NotepadJs {
    inner: Arc<RwLock<Notepad>>,
    bridge_handle: u64,
}

impl Drop for NotepadJs {
    fn drop(&mut self) {
        unsafe {
            notepad_bridge_release(self.bridge_handle);
        }
    }
}

#[napi]
impl NotepadJs {
    #[napi(constructor)]
    pub fn new() -> Self {
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

    #[napi(js_name = "nativeHandle")]
    pub fn native_handle(&self) -> u32 {
        self.bridge_handle as u32
    }

    #[napi(getter)]
    pub fn session_id(&self) -> Result<String> {
        let np = self.inner.blocking_read();
        Ok(np.session_id().to_string())
    }

    #[napi]
    pub fn store(&self, content: Value) -> Result<String> {
        let payload = payload_from_value(content)?;
        let mut np = self.inner.blocking_write();
        Ok(np.store(payload).to_string())
    }

    #[napi]
    pub fn store_tagged(
        &self,
        content: Value,
        tags: Option<Vec<String>>,
        label: Option<String>,
        metadata: Option<HashMap<String, Value>>,
    ) -> Result<String> {
        let payload = payload_from_value(content)?;
        let tags_vec = tags.unwrap_or_default();
        let tag_refs: Vec<&str> = tags_vec.iter().map(String::as_str).collect();
        let mut np = self.inner.blocking_write();
        Ok(np
            .store_tagged(
                &tag_refs,
                label.as_deref(),
                payload,
                metadata.unwrap_or_default(),
            )
            .to_string())
    }

    #[napi]
    pub fn get(&self, entry_id: String) -> Result<Option<Value>> {
        let id = parse_entry_id(&entry_id)?;
        let np = self.inner.blocking_read();
        Ok(np.get(id).map(entry_to_value))
    }

    #[napi]
    pub fn remove(&self, entry_id: String) -> Result<Option<Value>> {
        let id = parse_entry_id(&entry_id)?;
        let mut np = self.inner.blocking_write();
        Ok(np.remove(id).map(|e| entry_to_value(&e)))
    }

    #[napi]
    pub fn entries(&self) -> Result<Vec<Value>> {
        let np = self.inner.blocking_read();
        Ok(np.entries().iter().map(entry_to_value).collect())
    }

    #[napi]
    pub fn len(&self) -> Result<u32> {
        let np = self.inner.blocking_read();
        Ok(np.len() as u32)
    }

    #[napi]
    pub fn is_empty(&self) -> Result<bool> {
        let np = self.inner.blocking_read();
        Ok(np.is_empty())
    }

    #[napi]
    pub fn total_chars(&self) -> Result<u32> {
        let np = self.inner.blocking_read();
        Ok(np.total_chars() as u32)
    }

    #[napi]
    pub fn render(
        &self,
        format: Option<String>,
        include_metadata: Option<bool>,
        max_chars: Option<u32>,
    ) -> Result<String> {
        let opts = RenderOpts {
            format: format
                .as_deref()
                .map(render_format_from_str)
                .transpose()?
                .unwrap_or_default(),
            include_metadata: include_metadata.unwrap_or(true),
            max_chars: max_chars.map(|n| n as usize),
        };
        let np = self.inner.blocking_read();
        Ok(np.render(&opts))
    }

    #[napi]
    pub fn partition(&self, max_chars: u32) -> Result<Vec<Vec<String>>> {
        let np = self.inner.blocking_read();
        Ok(np
            .partition(max_chars as usize)
            .into_iter()
            .map(|batch| batch.into_iter().map(|id| id.to_string()).collect())
            .collect())
    }
}

#[napi(object, js_name = "SynthesisOutcome")]
pub struct SynthesisOutcomeJs {
    pub answer: String,
    pub passes: u32,
}

#[napi(js_name = "Synthesizer")]
pub struct SynthesizerJs {
    notepad: Arc<RwLock<Notepad>>,
    http: superglue::http::HttpClient,
    chat_opts: superglue::chat::ChatOptions,
    config: SynthesisConfig,
}

#[napi]
impl SynthesizerJs {
    #[napi(constructor)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        notepad: &NotepadJs,
        api_key: String,
        model: Option<String>,
        base_url: Option<String>,
        system_prompt: Option<String>,
        max_context_chars: Option<u32>,
        synthesis_budget_ratio: Option<f64>,
        extraction_budget_ratio: Option<f64>,
        max_extraction_rounds: Option<u32>,
    ) -> Result<Self> {
        let bootstrap = bootstrap_from_parts(BindingBootstrapConfig {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com".to_string()),
            model: model.unwrap_or_else(|| "gpt-5.4-nano-2026-03-17-mini".to_string()),
            system_prompt,
            ..Default::default()
        })
        .map_err(|e| Error::from_reason(e.to_string()))?;
        Ok(Self {
            notepad: Arc::clone(&notepad.inner),
            http: bootstrap.http,
            chat_opts: bootstrap.options,
            config: SynthesisConfig {
                max_context_chars: max_context_chars.unwrap_or(120_000) as usize,
                synthesis_budget_ratio: synthesis_budget_ratio.unwrap_or(0.8) as f32,
                extraction_budget_ratio: extraction_budget_ratio.unwrap_or(0.6) as f32,
                max_extraction_rounds: max_extraction_rounds.unwrap_or(8),
            },
        })
    }

    #[napi]
    pub async fn synthesize(&self, question: String) -> Result<SynthesisOutcomeJs> {
        let np = Arc::clone(&self.notepad);
        let http = self.http.clone();
        let chat_opts = self.chat_opts.clone();
        let config = self.config.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            napi::bindgen_prelude::block_on(async move {
                let np = np.read().await;
                let synth = NotepadSynthesizer::new(&np, &http, chat_opts).with_config(config);
                synth
                    .synthesize(&question)
                    .await
                    .map_err(|e| Error::from_reason(e.to_string()))
            })
        })
        .await
        .map_err(|e| Error::from_reason(format!("task join: {e}")))??;
        Ok(SynthesisOutcomeJs {
            answer: outcome.answer,
            passes: outcome.passes,
        })
    }
}

#[napi]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[napi(js_name = "isNotepadStub")]
pub fn is_notepad_stub(content: String) -> bool {
    notepad_is_stub(&content)
}

#[napi(js_name = "synthesisUserPrompt")]
pub fn synthesis_user_prompt(question: String, material: String) -> String {
    build_synthesis_prompt(&question, &material)
}

#[napi(js_name = "offloadConfigFromObject")]
pub fn offload_config_from_object(obj: Value) -> Result<OffloadConfigJs> {
    let inner = offload_config_from_json(&obj).map_err(Error::from_reason)?;
    Ok(OffloadConfigJs {
        auto_offload: Some(inner.auto_offload),
        min_chars: Some(inner.min_chars as u32),
        always_offload_tools: Some(inner.always_offload_tools),
        never_offload_tools: Some(inner.never_offload_tools),
        preview_chars: Some(inner.preview_chars as u32),
    })
}

#[napi(js_name = "offloadConfigToObject")]
pub fn offload_config_to_object(config: OffloadConfigJs) -> Result<Value> {
    Ok(offload_config_to_json(&config.to_inner()))
}

#[napi(js_name = "bridgeRegisterTools")]
pub fn bridge_register_tools(registry_ptr: i64, notepad_handle: i64) -> Result<()> {
    if registry_ptr == 0 || notepad_handle == 0 {
        return Err(Error::from_reason(
            "registry_ptr and notepad_handle must be non-zero".to_string(),
        ));
    }
    let rc = unsafe {
        notepad_bridge_attach_tools(
            registry_ptr as *const std::ffi::c_void,
            notepad_handle as u64,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(Error::from_reason(format!(
            "notepad_bridge_attach_tools failed ({rc})"
        )))
    }
}

#[napi(js_name = "bridgeRegisterOffloadHook")]
pub fn bridge_register_offload_hook(
    hooks_ptr: i64,
    notepad_handle: i64,
    config: Option<Value>,
) -> Result<()> {
    if hooks_ptr == 0 || notepad_handle == 0 {
        return Err(Error::from_reason(
            "hooks_ptr and notepad_handle must be non-zero".to_string(),
        ));
    }
    let config_json = config
        .map(|v| serde_json::to_string(&v).map_err(|e| Error::from_reason(e.to_string())))
        .transpose()?;
    let cfg_c = config_json
        .as_ref()
        .map(|s| std::ffi::CString::new(s.as_str()).map_err(|e| Error::from_reason(e.to_string())))
        .transpose()?;
    let cfg_ptr = cfg_c
        .as_ref()
        .map(|c| c.as_ptr())
        .unwrap_or(std::ptr::null());
    let rc = unsafe {
        notepad_bridge_attach_offload_hook(
            hooks_ptr as *const std::ffi::c_void,
            notepad_handle as u64,
            cfg_ptr,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(Error::from_reason(format!(
            "notepad_bridge_attach_offload_hook failed ({rc})"
        )))
    }
}

#[napi(js_name = "bridgeCompactToolMessages")]
pub fn bridge_compact_tool_messages(
    messages: Value,
    notepad_handle: i64,
    config: Option<Value>,
) -> Result<Value> {
    if notepad_handle == 0 {
        return Err(Error::from_reason(
            "notepad_handle must be non-zero".to_string(),
        ));
    }
    let messages_json =
        serde_json::to_string(&messages).map_err(|e| Error::from_reason(e.to_string()))?;
    let config_json = config
        .map(|v| serde_json::to_string(&v).map_err(|e| Error::from_reason(e.to_string())))
        .transpose()?;
    let mut buf = vec![0u8; messages_json.len() + 65536];
    let cfg_c = config_json
        .as_ref()
        .map(|s| std::ffi::CString::new(s.as_str()).map_err(|e| Error::from_reason(e.to_string())))
        .transpose()?;
    let cfg_ptr = cfg_c
        .as_ref()
        .map(|c| c.as_ptr())
        .unwrap_or(std::ptr::null());
    let msg_c =
        std::ffi::CString::new(messages_json).map_err(|e| Error::from_reason(e.to_string()))?;
    let rc = unsafe {
        notepad_bridge_compact_tool_messages(
            msg_c.as_ptr(),
            buf.as_mut_ptr() as *mut i8,
            buf.len(),
            notepad_handle as u64,
            cfg_ptr,
        )
    };
    if rc != 0 {
        return Err(Error::from_reason(format!(
            "notepad_bridge_compact_tool_messages failed ({rc})"
        )));
    }
    let out = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr() as *const i8) };
    let out_str = out
        .to_str()
        .map_err(|e| Error::from_reason(e.to_string()))?;
    serde_json::from_str(out_str).map_err(|e| Error::from_reason(e.to_string()))
}
