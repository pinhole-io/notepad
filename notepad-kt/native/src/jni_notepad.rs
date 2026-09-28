use std::collections::HashMap;
use std::sync::Arc;

use jni::JNIEnv;
use jni::objects::JClass;
use jni::sys::{jint, jlong, jstring};
use notepad::{
    Entry, EntryId, Notepad, NotepadSynthesizer, RenderFormat, RenderOpts, StoredPayload,
    SynthesisConfig, is_notepad_stub, offload_config_from_json, offload_config_to_json,
    synthesis_user_prompt,
};
use serde::Deserialize;
use serde_json::{Value, json};
use superglue::client::{BindingBootstrapConfig, bootstrap_from_parts};
use superglue::hooks::HookRegistry;
use superglue::tools::ToolRegistry;
use tokio::sync::RwLock;

use crate::handle::{
    KHandle, NotepadState, SynthesizerState, alloc, get_notepad, get_notepad_bridge_handle,
    get_synthesizer, remove,
};
use crate::jni_base::{ensure_jvm, jstr_from_str, jstr_to_string, jthrow, runtime};

fn parse_entry_id(id: &str) -> Result<EntryId, String> {
    let uuid: uuid::Uuid = id.parse().map_err(|_| format!("invalid entry id: {id}"))?;
    Ok(EntryId(uuid))
}

fn payload_from_value(v: Value) -> Result<StoredPayload, String> {
    match v {
        Value::String(s) => Ok(StoredPayload::Text(s)),
        other => Ok(StoredPayload::Json(other)),
    }
}

fn entry_to_json(entry: &Entry) -> Value {
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

#[derive(Deserialize)]
struct StoreTaggedReq {
    content: Value,
    #[serde(default)]
    tags: Vec<String>,
    label: Option<String>,
    #[serde(default)]
    metadata: HashMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderReq {
    #[serde(default = "default_format")]
    format: String,
    #[serde(default = "default_true")]
    include_metadata: bool,
    max_chars: Option<usize>,
}

fn default_format() -> String {
    "markdown".to_string()
}

fn default_true() -> bool {
    true
}

fn render_format(s: &str) -> Result<RenderFormat, String> {
    match s {
        "markdown" => Ok(RenderFormat::Markdown),
        "json" => Ok(RenderFormat::Json),
        other => Err(format!(
            "format must be 'markdown' or 'json', got {other:?}"
        )),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SynthesizerConfig {
    api_key: String,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default = "default_base_url")]
    base_url: String,
    system_prompt: Option<String>,
    #[serde(default = "default_max_context")]
    max_context_chars: usize,
    #[serde(default = "default_synth_ratio")]
    synthesis_budget_ratio: f32,
    #[serde(default = "default_extract_ratio")]
    extraction_budget_ratio: f32,
    #[serde(default = "default_max_rounds")]
    max_extraction_rounds: u32,
}

fn default_model() -> String {
    "gpt-5.4-nano-2026-03-17-mini".to_string()
}

fn default_base_url() -> String {
    "https://api.openai.com".to_string()
}

fn default_max_context() -> usize {
    120_000
}

fn default_synth_ratio() -> f32 {
    0.8
}

fn default_extract_ratio() -> f32 {
    0.6
}

fn default_max_rounds() -> u32 {
    8
}

fn json_err(env: &mut JNIEnv, msg: impl AsRef<str>) -> jstring {
    jthrow(env, msg.as_ref());
    std::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadCreate(
    mut env: JNIEnv,
    _cl: JClass,
) -> jlong {
    let _jvm = ensure_jvm(&mut env);
    let inner = Arc::new(RwLock::new(Notepad::new()));
    let bridge_handle = notepad_bridge::alloc_notepad(Arc::clone(&inner));
    alloc(KHandle::Notepad(NotepadState {
        inner,
        bridge_handle,
    })) as jlong
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadDestroy(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
) {
    let _jvm = ensure_jvm(&mut env);
    remove(handle as u64);
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadSessionId(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    match get_notepad(handle as u64) {
        Ok(np) => {
            let guard = np.blocking_read();
            jstr_from_str(&mut env, &guard.session_id().to_string())
        }
        Err(e) => json_err(&mut env, e),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadStore(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
    content_json: jstring,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    let raw = match jstr_to_string(&mut env, content_json) {
        Ok(s) => s,
        Err(e) => return json_err(&mut env, e),
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return json_err(&mut env, format!("content json: {e}")),
    };
    let payload = match payload_from_value(value) {
        Ok(p) => p,
        Err(e) => return json_err(&mut env, e),
    };
    match get_notepad(handle as u64) {
        Ok(np) => {
            let mut guard = np.blocking_write();
            jstr_from_str(&mut env, &guard.store(payload).to_string())
        }
        Err(e) => json_err(&mut env, e),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadStoreTagged(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
    req_json: jstring,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    let raw = match jstr_to_string(&mut env, req_json) {
        Ok(s) => s,
        Err(e) => return json_err(&mut env, e),
    };
    let req: StoreTaggedReq = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return json_err(&mut env, format!("request json: {e}")),
    };
    let payload = match payload_from_value(req.content) {
        Ok(p) => p,
        Err(e) => return json_err(&mut env, e),
    };
    let tag_refs: Vec<&str> = req.tags.iter().map(String::as_str).collect();
    match get_notepad(handle as u64) {
        Ok(np) => {
            let mut guard = np.blocking_write();
            jstr_from_str(
                &mut env,
                &guard
                    .store_tagged(&tag_refs, req.label.as_deref(), payload, req.metadata)
                    .to_string(),
            )
        }
        Err(e) => json_err(&mut env, e),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadGet(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
    entry_id: jstring,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    let id_str = match jstr_to_string(&mut env, entry_id) {
        Ok(s) => s,
        Err(e) => return json_err(&mut env, e),
    };
    let id = match parse_entry_id(&id_str) {
        Ok(v) => v,
        Err(e) => return json_err(&mut env, e),
    };
    match get_notepad(handle as u64) {
        Ok(np) => {
            let guard = np.blocking_read();
            let out = guard.get(id).map(entry_to_json);
            match serde_json::to_string(&out) {
                Ok(s) => jstr_from_str(&mut env, &s),
                Err(e) => json_err(&mut env, format!("encode: {e}")),
            }
        }
        Err(e) => json_err(&mut env, e),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadRemove(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
    entry_id: jstring,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    let id_str = match jstr_to_string(&mut env, entry_id) {
        Ok(s) => s,
        Err(e) => return json_err(&mut env, e),
    };
    let id = match parse_entry_id(&id_str) {
        Ok(v) => v,
        Err(e) => return json_err(&mut env, e),
    };
    match get_notepad(handle as u64) {
        Ok(np) => {
            let mut guard = np.blocking_write();
            let out = guard.remove(id).map(|e| entry_to_json(&e));
            match serde_json::to_string(&out) {
                Ok(s) => jstr_from_str(&mut env, &s),
                Err(e) => json_err(&mut env, format!("encode: {e}")),
            }
        }
        Err(e) => json_err(&mut env, e),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadEntries(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    match get_notepad(handle as u64) {
        Ok(np) => {
            let guard = np.blocking_read();
            let entries: Vec<Value> = guard.entries().iter().map(entry_to_json).collect();
            match serde_json::to_string(&entries) {
                Ok(s) => jstr_from_str(&mut env, &s),
                Err(e) => json_err(&mut env, format!("encode: {e}")),
            }
        }
        Err(e) => json_err(&mut env, e),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadLen(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
) -> jint {
    let _jvm = ensure_jvm(&mut env);
    match get_notepad(handle as u64) {
        Ok(np) => {
            let guard = np.blocking_read();
            guard.len() as jint
        }
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadTotalChars(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
) -> jint {
    let _jvm = ensure_jvm(&mut env);
    match get_notepad(handle as u64) {
        Ok(np) => {
            let guard = np.blocking_read();
            guard.total_chars() as jint
        }
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadRender(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
    opts_json: jstring,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    let raw = match jstr_to_string(&mut env, opts_json) {
        Ok(s) => s,
        Err(e) => return json_err(&mut env, e),
    };
    let req: RenderReq = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return json_err(&mut env, format!("opts json: {e}")),
    };
    let format = match render_format(&req.format) {
        Ok(f) => f,
        Err(e) => return json_err(&mut env, e),
    };
    let opts = RenderOpts {
        format,
        include_metadata: req.include_metadata,
        max_chars: req.max_chars,
    };
    match get_notepad(handle as u64) {
        Ok(np) => {
            let guard = np.blocking_read();
            jstr_from_str(&mut env, &guard.render(&opts))
        }
        Err(e) => json_err(&mut env, e),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadPartition(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
    max_chars: jint,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    match get_notepad(handle as u64) {
        Ok(np) => {
            let guard = np.blocking_read();
            let batches: Vec<Vec<String>> = guard
                .partition(max_chars.max(0) as usize)
                .into_iter()
                .map(|batch| batch.into_iter().map(|id| id.to_string()).collect())
                .collect();
            match serde_json::to_string(&batches) {
                Ok(s) => jstr_from_str(&mut env, &s),
                Err(e) => json_err(&mut env, format!("encode: {e}")),
            }
        }
        Err(e) => json_err(&mut env, e),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_synthesizerCreate(
    mut env: JNIEnv,
    _cl: JClass,
    notepad_handle: jlong,
    config_json: jstring,
) -> jlong {
    let _jvm = ensure_jvm(&mut env);
    let raw = match jstr_to_string(&mut env, config_json) {
        Ok(s) => s,
        Err(e) => {
            jthrow(&mut env, &e);
            return 0;
        }
    };
    let cfg: SynthesizerConfig = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            jthrow(&mut env, &format!("config json: {e}"));
            return 0;
        }
    };
    let notepad = match get_notepad(notepad_handle as u64) {
        Ok(np) => np,
        Err(e) => {
            jthrow(&mut env, &e);
            return 0;
        }
    };
    let bootstrap = match bootstrap_from_parts(BindingBootstrapConfig {
        api_key: cfg.api_key,
        base_url: cfg.base_url,
        model: cfg.model,
        system_prompt: cfg.system_prompt,
        ..Default::default()
    }) {
        Ok(b) => b,
        Err(e) => {
            jthrow(&mut env, &e.to_string());
            return 0;
        }
    };
    alloc(KHandle::Synthesizer(SynthesizerState {
        notepad,
        http: bootstrap.http,
        chat_opts: bootstrap.options,
        config: SynthesisConfig {
            max_context_chars: cfg.max_context_chars,
            synthesis_budget_ratio: cfg.synthesis_budget_ratio,
            extraction_budget_ratio: cfg.extraction_budget_ratio,
            max_extraction_rounds: cfg.max_extraction_rounds,
        },
    })) as jlong
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_synthesizerDestroy(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
) {
    let _jvm = ensure_jvm(&mut env);
    remove(handle as u64);
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_synthesizerSynthesize(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
    question: jstring,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    let q = match jstr_to_string(&mut env, question) {
        Ok(s) => s,
        Err(e) => return json_err(&mut env, e),
    };
    let state = match get_synthesizer(handle as u64) {
        Ok(s) => s,
        Err(e) => return json_err(&mut env, e),
    };
    let outcome = runtime().block_on(async {
        let np = state.notepad.read().await;
        let synth = NotepadSynthesizer::new(&np, &state.http, state.chat_opts.clone())
            .with_config(state.config.clone());
        synth.synthesize(&q).await
    });
    match outcome {
        Ok(o) => {
            let v = json!({ "answer": o.answer, "passes": o.passes });
            match serde_json::to_string(&v) {
                Ok(s) => jstr_from_str(&mut env, &s),
                Err(e) => json_err(&mut env, format!("encode: {e}")),
            }
        }
        Err(e) => json_err(&mut env, e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_notepadNativeHandle(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
) -> jlong {
    let _jvm = ensure_jvm(&mut env);
    match get_notepad_bridge_handle(handle as u64) {
        Ok(h) => h as jlong,
        Err(e) => {
            jthrow(&mut env, &e);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_isNotepadStub(
    mut env: JNIEnv,
    _cl: JClass,
    content: jstring,
) -> jni::sys::jboolean {
    let _jvm = ensure_jvm(&mut env);
    match jstr_to_string(&mut env, content) {
        Ok(s) => is_notepad_stub(&s).into(),
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_synthesisUserPrompt(
    mut env: JNIEnv,
    _cl: JClass,
    question: jstring,
    material: jstring,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    let q = match jstr_to_string(&mut env, question) {
        Ok(s) => s,
        Err(e) => return json_err(&mut env, e),
    };
    let m = match jstr_to_string(&mut env, material) {
        Ok(s) => s,
        Err(e) => return json_err(&mut env, e),
    };
    jstr_from_str(&mut env, &synthesis_user_prompt(&q, &m))
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_offloadConfigToJson(
    mut env: JNIEnv,
    _cl: JClass,
    config_json: jstring,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    let raw = match jstr_to_string(&mut env, config_json) {
        Ok(s) => s,
        Err(e) => return json_err(&mut env, e),
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return json_err(&mut env, format!("config json: {e}")),
    };
    let cfg = match offload_config_from_json(&value) {
        Ok(c) => c,
        Err(e) => return json_err(&mut env, e),
    };
    match serde_json::to_string(&offload_config_to_json(&cfg)) {
        Ok(s) => jstr_from_str(&mut env, &s),
        Err(e) => json_err(&mut env, format!("encode: {e}")),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_bridgeRegisterTools(
    mut env: JNIEnv,
    _cl: JClass,
    registry_ptr: jlong,
    notepad_handle: jlong,
) {
    let _jvm = ensure_jvm(&mut env);
    if registry_ptr == 0 || notepad_handle == 0 {
        jthrow(&mut env, "registry_ptr and notepad_handle must be non-zero");
        return;
    }
    let registry = &*(registry_ptr as *const ToolRegistry);
    if let Err(e) = runtime().block_on(notepad_bridge::attach_tools(
        registry,
        notepad_handle as u64,
    )) {
        jthrow(&mut env, &e.to_string());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_bridgeRegisterOffloadHook(
    mut env: JNIEnv,
    _cl: JClass,
    hooks_ptr: jlong,
    notepad_handle: jlong,
    config_json: jstring,
) {
    let _jvm = ensure_jvm(&mut env);
    if hooks_ptr == 0 || notepad_handle == 0 {
        jthrow(&mut env, "hooks_ptr and notepad_handle must be non-zero");
        return;
    }
    let hooks = &*(hooks_ptr as *const HookRegistry);
    let cfg = if config_json.is_null() {
        notepad::OffloadConfig::default()
    } else {
        let raw = match jstr_to_string(&mut env, config_json) {
            Ok(s) => s,
            Err(e) => {
                jthrow(&mut env, &e);
                return;
            }
        };
        if raw.is_empty() {
            notepad::OffloadConfig::default()
        } else {
            let value: Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(e) => {
                    jthrow(&mut env, &format!("config json: {e}"));
                    return;
                }
            };
            match offload_config_from_json(&value) {
                Ok(c) => c,
                Err(e) => {
                    jthrow(&mut env, &e);
                    return;
                }
            }
        }
    };
    if let Err(e) = runtime().block_on(notepad_bridge::attach_offload_hook(
        hooks,
        notepad_handle as u64,
        cfg,
    )) {
        jthrow(&mut env, &e);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_bridgeCompactToolMessages(
    mut env: JNIEnv,
    _cl: JClass,
    messages_json: jstring,
    notepad_handle: jlong,
    config_json: jstring,
) -> jstring {
    let _jvm = ensure_jvm(&mut env);
    if notepad_handle == 0 {
        jthrow(&mut env, "notepad_handle must be non-zero");
        return std::ptr::null_mut();
    }
    let raw = match jstr_to_string(&mut env, messages_json) {
        Ok(s) => s,
        Err(e) => return json_err(&mut env, e),
    };
    let mut messages: Vec<superglue::openai::ChatMessage> = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return json_err(&mut env, format!("messages json: {e}")),
    };
    let cfg = if config_json.is_null() {
        notepad::OffloadConfig::default()
    } else {
        let cfg_raw = match jstr_to_string(&mut env, config_json) {
            Ok(s) => s,
            Err(e) => return json_err(&mut env, e),
        };
        if cfg_raw.is_empty() {
            notepad::OffloadConfig::default()
        } else {
            let value: Value = match serde_json::from_str(&cfg_raw) {
                Ok(v) => v,
                Err(e) => return json_err(&mut env, format!("config json: {e}")),
            };
            match offload_config_from_json(&value) {
                Ok(c) => c,
                Err(e) => return json_err(&mut env, e),
            }
        }
    };
    if let Err(e) = runtime().block_on(notepad_bridge::compact_tool_messages_handle(
        &mut messages,
        notepad_handle as u64,
        &cfg,
    )) {
        return json_err(&mut env, e);
    }
    match serde_json::to_string(&messages) {
        Ok(s) => jstr_from_str(&mut env, &s),
        Err(e) => json_err(&mut env, format!("encode: {e}")),
    }
}
