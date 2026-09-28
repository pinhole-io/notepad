//! superglue integration: offload tool output and compact conversation history.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use superglue::hooks::{
    HookConfig, HookContext, HookError, HookErrorStrategy, HookHandler, HookRegistry, HookStage,
};
use superglue::openai::{ChatMessage, MessageContent};
use superglue::tools::ToolContextPolicy;
use tokio::sync::RwLock;

use crate::content_quality::is_low_signal_web_text;
use crate::entry::StoredPayload;
use crate::notepad::Notepad;

/// When to replace a tool result with a notepad stub (automatic offload mode).
#[derive(Debug, Clone)]
pub struct OffloadConfig {
    /// When false, tool results stay in context; use [`register_notepad_tools`] instead.
    pub auto_offload: bool,
    /// Offload when tool result length (chars) is >= this threshold (only if `auto_offload`).
    pub min_chars: usize,
    /// Per-tool char thresholds; overrides [`Self::min_chars`] when present.
    pub min_chars_by_tool: HashMap<String, usize>,
    /// Always offload these tool names regardless of size (only if `auto_offload`).
    pub always_offload_tools: Vec<String>,
    /// Never offload these tool names (in addition to [`SCRATCHPAD_TOOLS`]).
    pub never_offload_tools: Vec<String>,
    /// Preview length embedded in the stub JSON.
    pub preview_chars: usize,
}

/// Tools whose output *is* the state the model asked to see; offloading them would
/// hand back a stub for content the model can then only re-request, looping forever.
pub const SCRATCHPAD_TOOLS: &[&str] = &[
    crate::tools::NOTEPAD_TOOL_NAME,
    crate::artifact_tools::ARTIFACT_TOOL_NAME,
    crate::plan_tools::PLAN_TOOL_NAME,
    crate::life_task_tools::LIFE_TASK_TOOL_NAME,
    "todo",
];

/// Filesystem navigation tools — results must stay inline. Offloading a directory
/// listing (e.g. `~/.harn` at depth 2) replaces it with a notepad stub; the UI then
/// shows "Scanned 0 entries" and the model loops on empty `notepad` reads.
pub const NAVIGATION_TOOLS: &[&str] = &["ls", "list_dir", "glob", "grep"];

/// Lookup tools return the evidence the model needs immediately. Keep their rows inline
/// so a search result cannot be replaced by a pointer that triggers another search.
pub const INLINE_LOOKUP_TOOLS: &[&str] = &["web_search"];

pub fn default_never_offload_tools() -> Vec<String> {
    SCRATCHPAD_TOOLS
        .iter()
        .chain(NAVIGATION_TOOLS.iter())
        .chain(INLINE_LOOKUP_TOOLS.iter())
        .map(|t| (*t).to_string())
        .collect()
}

impl Default for OffloadConfig {
    fn default() -> Self {
        Self {
            auto_offload: true,
            min_chars: 2048,
            min_chars_by_tool: HashMap::from([("read".into(), 16_000)]),
            always_offload_tools: vec!["charlotte_site_crawl_chunks".into()],
            never_offload_tools: default_never_offload_tools(),
            preview_chars: 200,
        }
    }
}

/// Parsed body ready to store in the notepad during auto-offload.
struct OffloadBody {
    payload: StoredPayload,
    preview_source: String,
    meta: HashMap<String, Value>,
}

struct StoredRef {
    id: crate::entry::EntryId,
    url: Option<String>,
    path: Option<String>,
    stored_chars: usize,
    preview: String,
    identity: HashMap<String, Value>,
}

/// PostTool hook: store large tool payloads in the notepad, return a stub reference.
pub struct OffloadHook {
    notepad: Arc<RwLock<Notepad>>,
    config: OffloadConfig,
}

impl OffloadHook {
    pub fn new(notepad: Arc<RwLock<Notepad>>, config: OffloadConfig) -> Self {
        Self { notepad, config }
    }

    pub fn should_offload(
        &self,
        tool_name: &str,
        content: &str,
        context_policy: Option<ToolContextPolicy>,
    ) -> bool {
        should_offload(&self.config, tool_name, content, context_policy)
    }

    pub fn make_stub(
        &self,
        entry_id: crate::entry::EntryId,
        tool_name: &str,
        stored_chars: usize,
        preview: &str,
        identity: &HashMap<String, Value>,
    ) -> String {
        make_stub(entry_id, tool_name, stored_chars, preview, identity)
    }

    pub fn preview(content: &str, max_chars: usize) -> String {
        let s: String = content.chars().take(max_chars).collect();
        if content.chars().count() > max_chars {
            format!("{s}…")
        } else {
            s
        }
    }
}

#[async_trait]
impl HookHandler for OffloadHook {
    async fn execute(&self, ctx: HookContext) -> Result<HookContext, HookError> {
        if ctx.stage != HookStage::PostTool {
            return Ok(ctx);
        }

        let tool_name = ctx
            .metadata
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        let context_policy = context_policy_from_metadata(&ctx.metadata);

        if is_notepad_stub(&ctx.content) {
            return Ok(ctx);
        }
        if tool_name == "read" && read_result_is_cached(&ctx.content) {
            return Ok(ctx);
        }

        if !self.should_offload(&tool_name, &ctx.content, context_policy) {
            return Ok(ctx);
        }

        let bodies = extract_offload_bodies(&ctx.content);
        if bodies.is_empty() {
            return Ok(ctx);
        }
        if tool_name == "read" && !read_has_identity(&bodies, &ctx.metadata) {
            return Ok(ctx);
        }

        let mut np = self.notepad.write().await;
        if bodies.len() == 1 {
            let body = bodies.into_iter().next().expect("one body");
            let stored_chars = body.payload.char_len();
            let preview = Self::preview(&body.preview_source, self.config.preview_chars);
            let mut identity = identity_fields_from_meta(&body.meta);
            add_read_identity(&mut identity, &ctx.metadata, &tool_name);
            let mut meta = body.meta;
            for (key, value) in &identity {
                meta.entry(key.clone()).or_insert_with(|| value.clone());
            }
            meta.insert("tool".into(), json!(tool_name));
            meta.insert("stored_chars".into(), json!(stored_chars));
            let id = np.store_tagged(&["tool_result"], Some(&tool_name), body.payload, meta);
            let stub = self.make_stub(id, &tool_name, stored_chars, &preview, &identity);
            return Ok(HookContext {
                content: stub,
                ..ctx
            });
        }

        let mut refs = Vec::with_capacity(bodies.len());
        for body in bodies {
            let stored_chars = body.payload.char_len();
            let preview = Self::preview(&body.preview_source, self.config.preview_chars);
            let identity = identity_fields_from_meta(&body.meta);
            let url = body
                .meta
                .get("url")
                .and_then(Value::as_str)
                .map(str::to_string);
            let path = body
                .meta
                .get("path")
                .or_else(|| body.meta.get("rel_path"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let mut meta = body.meta;
            meta.insert("tool".into(), json!(tool_name));
            meta.insert("stored_chars".into(), json!(stored_chars));
            let id = np.store_tagged(&["tool_result"], Some(&tool_name), body.payload, meta);
            refs.push(StoredRef {
                id,
                url,
                path,
                stored_chars,
                preview,
                identity,
            });
        }
        drop(np);

        let stub = make_multi_stub(&refs, &tool_name);
        Ok(HookContext {
            content: stub,
            ..ctx
        })
    }
}

/// Register the offload hook on a superglue [`HookRegistry`].
pub async fn register_offload_hook(
    hooks: &HookRegistry,
    notepad: Arc<RwLock<Notepad>>,
    config: OffloadConfig,
) {
    hooks
        .add(
            HookStage::PostTool,
            HookConfig {
                name: "notepad_offload".into(),
                error_strategy: HookErrorStrategy::Abort,
                handler: Arc::new(OffloadHook::new(notepad, config)),
            },
        )
        .await;
}

fn context_policy_from_metadata(metadata: &HashMap<String, Value>) -> Option<ToolContextPolicy> {
    metadata
        .get("context_policy")
        .and_then(Value::as_str)
        .and_then(ToolContextPolicy::parse)
}

fn identity_fields_from_meta(meta: &HashMap<String, Value>) -> HashMap<String, Value> {
    read_identity_keys()
        .iter()
        .filter_map(|key| {
            meta.get(*key)
                .filter(|value| !value.is_null())
                .map(|value| ((*key).to_string(), value.clone()))
        })
        .collect()
}

fn read_path_from_arguments(metadata: &HashMap<String, Value>) -> Option<String> {
    metadata
        .get("arguments")
        .and_then(Value::as_str)
        .and_then(|args| serde_json::from_str::<Value>(args).ok())
        .and_then(|args| {
            let path = args
                .get("path")
                .or_else(|| args.get("rel_path"))
                .and_then(Value::as_str)?;
            (!path.trim().is_empty()).then(|| path.to_string())
        })
}

fn add_read_identity(
    identity: &mut HashMap<String, Value>,
    metadata: &HashMap<String, Value>,
    tool_name: &str,
) {
    if tool_name == "read"
        && !identity.contains_key("path")
        && !identity.contains_key("rel_path")
        && let Some(path) = read_path_from_arguments(metadata)
    {
        identity.insert("path".into(), json!(path));
    }
}

fn read_has_identity(bodies: &[OffloadBody], metadata: &HashMap<String, Value>) -> bool {
    bodies.iter().any(|body| {
        body.meta
            .get("path")
            .or_else(|| body.meta.get("rel_path"))
            .and_then(Value::as_str)
            .is_some_and(|path| !path.trim().is_empty())
    }) || read_path_from_arguments(metadata).is_some()
}

fn read_result_is_cached(content: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return false;
    };
    if value.get("cached").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    value
        .get("files")
        .and_then(Value::as_array)
        .is_some_and(|files| {
            !files.is_empty()
                && files
                    .iter()
                    .all(|file| file.get("cached").and_then(Value::as_bool) == Some(true))
        })
}

fn should_offload(
    config: &OffloadConfig,
    tool_name: &str,
    content: &str,
    context_policy: Option<ToolContextPolicy>,
) -> bool {
    if !config.auto_offload {
        return false;
    }
    if context_policy == Some(ToolContextPolicy::Inline) {
        return false;
    }
    if context_policy == Some(ToolContextPolicy::AlwaysOffload) {
        return !extract_offload_bodies(content).is_empty();
    }
    if SCRATCHPAD_TOOLS.contains(&tool_name) {
        return false;
    }
    // Hard-exclude even when an older config omitted these from never_offload_tools.
    if NAVIGATION_TOOLS.contains(&tool_name) {
        return false;
    }
    // Search output is the evidence requested by the model. Offloading it replaces
    // usable title/url/snippet rows with a notepad pointer and encourages query spam.
    if INLINE_LOOKUP_TOOLS.contains(&tool_name) {
        return false;
    }
    if config.never_offload_tools.iter().any(|t| t == tool_name) {
        return false;
    }
    if config.always_offload_tools.iter().any(|t| t == tool_name) {
        return !extract_offload_bodies(content).is_empty();
    }
    let bodies = extract_offload_bodies(content);
    if tool_name == "read" && !read_has_identity(&bodies, &HashMap::new()) {
        return false;
    }
    let threshold = config
        .min_chars_by_tool
        .get(tool_name)
        .copied()
        .unwrap_or(config.min_chars);
    content.chars().count() >= threshold && !bodies.is_empty()
}

fn is_soft_fail_tool_result(value: &Value) -> bool {
    if value.get("ok") == Some(&Value::Bool(false)) {
        return true;
    }
    if value.get("status").and_then(Value::as_str) == Some("denied_by_user") {
        return true;
    }
    if value.get("error").and_then(Value::as_str).is_some()
        && !value
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    {
        return true;
    }
    false
}

/// Decide what to store in the notepad. Empty vec means the tool result should stay inline.
fn extract_offload_bodies(raw: &str) -> Vec<OffloadBody> {
    if is_notepad_stub(raw) {
        return Vec::new();
    }

    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        let trimmed = raw.trim();
        if trimmed.is_empty() || is_low_signal_web_text(trimmed) {
            return Vec::new();
        }
        return vec![OffloadBody {
            payload: StoredPayload::Text(trimmed.to_string()),
            preview_source: trimmed.to_string(),
            meta: HashMap::new(),
        }];
    };

    if is_soft_fail_tool_result(&value) {
        return Vec::new();
    }

    if let Some(results) = value.get("results").and_then(Value::as_array) {
        // Search rows are usually {title,url,snippet}; keep that envelope together.
        // Only fan out rows that contain fetched page bodies.
        if results.iter().any(result_has_fetched_body) {
            return results
                .iter()
                .filter(|result| result_has_fetched_body(result))
                .filter_map(extract_body_from_element)
                .collect();
        }
    }
    if let Some(files) = value.get("files").and_then(Value::as_array) {
        return files.iter().filter_map(extract_body_from_element).collect();
    }

    extract_body_from_element(&value).into_iter().collect()
}

fn result_has_fetched_body(value: &Value) -> bool {
    ["content", "markdown"]
        .iter()
        .any(|key| value.get(*key).and_then(Value::as_str).is_some())
}

fn extract_body_from_element(value: &Value) -> Option<OffloadBody> {
    if is_soft_fail_tool_result(value) {
        return None;
    }

    if let Some(content) = value.get("content").and_then(Value::as_str) {
        if content.trim().is_empty() || is_low_signal_web_text(content) {
            return None;
        }
        let meta = identity_fields_from_value(value);
        return Some(OffloadBody {
            payload: StoredPayload::Text(content.to_string()),
            preview_source: content.to_string(),
            meta,
        });
    }

    let serialized = serde_json::to_string(value).ok()?;
    if serialized.trim().is_empty() {
        return None;
    }

    Some(OffloadBody {
        payload: StoredPayload::Json(value.clone()),
        preview_source: serialized,
        meta: identity_fields_from_value(value),
    })
}

fn identity_fields_from_value(value: &Value) -> HashMap<String, Value> {
    read_identity_keys()
        .iter()
        .filter_map(|key| {
            value
                .get(*key)
                .filter(|value| !value.is_null())
                .map(|v| ((*key).to_string(), v.clone()))
        })
        .collect()
}

fn read_identity_keys() -> &'static [&'static str] {
    &[
        "path",
        "rel_path",
        "url",
        "has_more",
        "total_lines",
        "start_line",
        "end_line",
        "file_hash",
    ]
}

fn make_multi_stub(refs: &[StoredRef], tool_name: &str) -> String {
    let notepad_refs: Vec<Value> = refs
        .iter()
        .map(|r| {
            let id = r.id.to_string();
            let mut obj = json!({
                "notepad_ref": id.clone(),
                "id": id.clone(),
                "stored_chars": r.stored_chars,
                "preview": r.preview,
                "retrieve": {
                    "tool": "notepad",
                    "action": "read",
                    "id": id,
                },
            });
            if let Some(url) = &r.url {
                obj["url"] = json!(url);
            }
            if let Some(path) = &r.path {
                obj["path"] = json!(path);
            }
            for (key, value) in &r.identity {
                obj[key] = value.clone();
            }
            obj
        })
        .collect();
    json!({
        "notepad_refs": notepad_refs,
        "tool": tool_name,
        "count": refs.len(),
        "note": "The preview is truncated. Call `notepad` with `action=read` and each `retrieve.id` (or `notepad_ref`) to retrieve the full content. Do not re-run the original tool.",
    })
    .to_string()
}

fn make_stub(
    entry_id: crate::entry::EntryId,
    tool_name: &str,
    stored_chars: usize,
    preview: &str,
    identity: &HashMap<String, Value>,
) -> String {
    let id = entry_id.to_string();
    let mut obj = json!({
        "notepad_ref": id.clone(),
        "id": id.clone(),
        "stored_chars": stored_chars,
        "tool": tool_name,
        "preview": preview,
        "retrieve": {
            "tool": "notepad",
            "action": "read",
            "id": id,
        },
        "note": "The preview is truncated. Call `notepad` with `action=read` and `retrieve.id` (or `notepad_ref`) to retrieve the full content. Do not re-run the original tool.",
    });
    for (key, value) in identity {
        obj[key] = value.clone();
    }
    obj.to_string()
}

/// Retroactively stub inline tool messages (multi-turn REPL compaction).
pub fn compact_tool_messages(
    messages: &mut [ChatMessage],
    notepad: &mut Notepad,
    config: &OffloadConfig,
) {
    for msg in messages.iter_mut() {
        if msg.role != "tool" {
            continue;
        }
        let Some(content) = msg
            .content
            .as_ref()
            .and_then(MessageContent::as_text)
            .map(str::to_string)
        else {
            continue;
        };

        if is_notepad_stub(&content) {
            continue;
        }

        let tool_name = msg.name.clone().unwrap_or_else(|| "unknown".into());
        if !should_offload(config, &tool_name, &content, None) {
            continue;
        }

        let bodies = extract_offload_bodies(&content);
        if bodies.is_empty() {
            continue;
        }

        if bodies.len() == 1 {
            let body = bodies.into_iter().next().expect("one body");
            let stored_chars = body.payload.char_len();
            let preview = OffloadHook::preview(&body.preview_source, config.preview_chars);
            let identity = identity_fields_from_meta(&body.meta);
            let mut meta = body.meta;
            meta.insert("tool".into(), json!(tool_name.clone()));
            meta.insert("stored_chars".into(), json!(stored_chars));
            let id = notepad.store_tagged(&["tool_result"], Some(&tool_name), body.payload, meta);
            let stub = make_stub(id, &tool_name, stored_chars, &preview, &identity);
            msg.content = Some(MessageContent::Text(stub));
            continue;
        }

        let mut refs = Vec::with_capacity(bodies.len());
        for body in bodies {
            let stored_chars = body.payload.char_len();
            let preview = OffloadHook::preview(&body.preview_source, config.preview_chars);
            let identity = identity_fields_from_meta(&body.meta);
            let url = body
                .meta
                .get("url")
                .and_then(Value::as_str)
                .map(str::to_string);
            let path = body
                .meta
                .get("path")
                .or_else(|| body.meta.get("rel_path"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let mut meta = body.meta;
            meta.insert("tool".into(), json!(tool_name.clone()));
            meta.insert("stored_chars".into(), json!(stored_chars));
            let id = notepad.store_tagged(&["tool_result"], Some(&tool_name), body.payload, meta);
            refs.push(StoredRef {
                id,
                url,
                path,
                stored_chars,
                preview,
                identity,
            });
        }
        let stub = make_multi_stub(&refs, &tool_name);
        msg.content = Some(MessageContent::Text(stub));
    }
}

#[must_use]
pub fn is_notepad_stub(content: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return false;
    };
    if value.get("notepad_ref").and_then(Value::as_str).is_some() {
        return true;
    }
    value
        .get("notepad_refs")
        .and_then(Value::as_array)
        .is_some_and(|arr| !arr.is_empty())
}

/// Build a user message for a synthesis pass over the notepad.
#[must_use]
pub fn synthesis_user_prompt(question: &str, material: &str) -> String {
    format!(
        "## Collected material\n\n{material}\n\n## Question\n\n{question}\n\n\
         Answer using only the collected material."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{EntryId, StoredPayload};

    fn long_article() -> String {
        "Butter Chicken Recipe\n\n".to_string()
            + &"Tender chicken in a rich tomato-cream sauce with garam masala and ginger. "
                .repeat(80)
    }

    fn fetch_success_json(content: &str) -> String {
        serde_json::json!({
            "url": "https://example.com/recipe",
            "content": content,
            "truncated": false,
            "content_type": "text/html",
        })
        .to_string()
    }

    #[test]
    fn stub_detection() {
        let stub = r#"{"notepad_ref":"550e8400-e29b-41d4-a716-446655440000","stored_chars":100}"#;
        assert!(is_notepad_stub(stub));
        let multi = r#"{"notepad_refs":[{"notepad_ref":"550e8400-e29b-41d4-a716-446655440000"}],"count":1}"#;
        assert!(is_notepad_stub(multi));
        assert!(!is_notepad_stub(r#"{"url":"https://example.com"}"#));
    }

    #[test]
    fn cached_read_results_stay_inline() {
        assert!(read_result_is_cached(
            r#"{"path":"a.rs","cached":true,"content":"1| fn main() {}"}"#
        ));
        assert!(read_result_is_cached(
            r#"{"files":[{"path":"a.rs","cached":true},{"path":"b.rs","cached":true}]}"#
        ));
        assert!(!read_result_is_cached(
            r#"{"files":[{"path":"a.rs","cached":true},{"path":"b.rs"}]}"#
        ));
    }

    #[test]
    fn auto_offload_enabled_by_default() {
        let hook = OffloadHook::new(
            Arc::new(RwLock::new(Notepad::new())),
            OffloadConfig::default(),
        );
        let body = long_article();
        assert!(hook.should_offload("charlotte_site_crawl_chunks", &body, None));
    }

    #[test]
    fn fetch_success_offloads_unwrapped_text() {
        let article = long_article();
        let raw = fetch_success_json(&article);
        let body = extract_offload_bodies(&raw)
            .into_iter()
            .next()
            .expect("should offload");
        match body.payload {
            StoredPayload::Text(text) => {
                assert!(text.starts_with("Butter Chicken"));
                assert!(text.contains("garam masala"));
                assert!(!text.contains("\"url\""));
            }
            StoredPayload::Json(_) => panic!("expected unwrapped text payload"),
        }
        assert!(body.preview_source.starts_with("Butter Chicken"));
    }

    #[test]
    fn soft_fail_not_offloaded() {
        let raw = r#"{"ok":false,"url":"https://example.com","error":"fetch failed"}"#;
        assert!(extract_offload_bodies(raw).is_empty());
        let hook = OffloadHook::new(
            Arc::new(RwLock::new(Notepad::new())),
            OffloadConfig::default(),
        );
        assert!(!hook.should_offload("fetch", raw, None));
    }

    #[test]
    fn region_picker_not_offloaded() {
        let junk = "All Regions Argentina Australia Austria Belgium (fr) Belgium (nl) Brazil Bulgaria Canada "
            .repeat(80);
        let raw = fetch_success_json(&junk);
        assert!(extract_offload_bodies(&raw).is_empty());
    }

    #[test]
    fn good_long_content_still_offloads() {
        let raw = fetch_success_json(&long_article());
        assert!(!extract_offload_bodies(&raw).is_empty());
    }

    #[tokio::test]
    async fn hook_stores_text_not_envelope() {
        let notepad = Arc::new(RwLock::new(Notepad::new()));
        let hook = OffloadHook::new(
            Arc::clone(&notepad),
            OffloadConfig {
                auto_offload: true,
                min_chars: 100,
                ..Default::default()
            },
        );
        let article = long_article();
        let raw = fetch_success_json(&article);
        let ctx = HookContext {
            stage: HookStage::PostTool,
            content: raw,
            metadata: [("tool_name".into(), json!("fetch"))].into(),
        };
        let out = hook.execute(ctx).await.expect("hook");
        assert!(is_notepad_stub(&out.content));
        let np = notepad.read().await;
        let entry = np.entries().first().expect("stored");
        match &entry.payload {
            StoredPayload::Text(text) => {
                assert!(text.starts_with("Butter Chicken"));
                assert!(!text.contains("\"content_type\""));
            }
            StoredPayload::Json(_) => panic!("expected text payload"),
        }
    }

    #[test]
    fn scratchpad_reads_never_offloaded() {
        let hook = OffloadHook::new(
            Arc::new(RwLock::new(Notepad::new())),
            OffloadConfig {
                min_chars: 10,
                // An override must not be able to re-enable the read/offload loop.
                never_offload_tools: Vec::new(),
                always_offload_tools: SCRATCHPAD_TOOLS.iter().map(|t| (*t).to_string()).collect(),
                ..Default::default()
            },
        );
        let read_result = serde_json::json!({
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "content": long_article(),
        })
        .to_string();
        for tool in SCRATCHPAD_TOOLS {
            assert!(
                !hook.should_offload(tool, &read_result, None),
                "{tool} output must stay in context"
            );
        }
    }

    #[test]
    fn navigation_tools_never_offloaded() {
        let hook = OffloadHook::new(
            Arc::new(RwLock::new(Notepad::new())),
            OffloadConfig {
                min_chars: 10,
                never_offload_tools: Vec::new(),
                always_offload_tools: NAVIGATION_TOOLS.iter().map(|t| (*t).to_string()).collect(),
                ..Default::default()
            },
        );
        let ls_result = serde_json::json!({
            "path": "/home/user/.harn",
            "entries": (0..80)
                .map(|i| json!({"path": format!(".harn/file{i}.txt"), "kind": "file"}))
                .collect::<Vec<_>>(),
            "truncated": false,
        })
        .to_string();
        assert!(ls_result.chars().count() > 10);
        for tool in NAVIGATION_TOOLS {
            assert!(
                !hook.should_offload(tool, &ls_result, None),
                "{tool} listings must stay inline"
            );
        }
    }

    #[tokio::test]
    async fn web_search_results_stay_inline() {
        let notepad = Arc::new(RwLock::new(Notepad::new()));
        let hook = OffloadHook::new(
            Arc::clone(&notepad),
            OffloadConfig {
                min_chars: 10,
                never_offload_tools: Vec::new(),
                ..Default::default()
            },
        );
        let raw = serde_json::json!({
            "backend": "searxng",
            "results": (0..80)
                .map(|i| json!({
                    "title": format!("LLM SDK {i}"),
                    "url": format!("https://example.com/sdk/{i}"),
                    "snippet": "A developer SDK for building LLM applications."
                }))
                .collect::<Vec<_>>(),
        })
        .to_string();
        assert!(raw.chars().count() > 10);
        assert!(!hook.should_offload("web_search", &raw, None));

        let ctx = HookContext {
            stage: HookStage::PostTool,
            content: raw.clone(),
            metadata: [("tool_name".into(), json!("web_search"))].into(),
        };
        let out = hook.execute(ctx).await.expect("hook");
        assert_eq!(out.content, raw);
        assert!(notepad.read().await.entries().is_empty());
    }

    #[test]
    fn search_results_are_kept_as_one_envelope() {
        let raw = serde_json::json!({
            "results": [
                {
                    "title": "LLM SDK",
                    "url": "https://example.com/sdk",
                    "snippet": "Developer SDK documentation."
                },
                {
                    "title": "Another SDK",
                    "url": "https://example.com/another",
                    "snippet": "More documentation."
                }
            ]
        })
        .to_string();
        let bodies = extract_offload_bodies(&raw);
        assert_eq!(bodies.len(), 1);
        assert!(matches!(bodies[0].payload, StoredPayload::Json(_)));
    }

    #[test]
    fn inline_context_policy_blocks_offload() {
        let hook = OffloadHook::new(
            Arc::new(RwLock::new(Notepad::new())),
            OffloadConfig::default(),
        );
        let read_result = serde_json::json!({
            "path": "src/foo.rs",
            "content": long_article(),
        })
        .to_string();
        assert!(!hook.should_offload("read", &read_result, Some(ToolContextPolicy::Inline)));
    }

    #[tokio::test]
    async fn large_read_offloads_with_retrievable_path_and_id() {
        let notepad = Arc::new(RwLock::new(Notepad::new()));
        let hook = OffloadHook::new(
            Arc::clone(&notepad),
            OffloadConfig {
                min_chars: 100,
                min_chars_by_tool: HashMap::from([("read".into(), 100)]),
                never_offload_tools: Vec::new(),
                ..Default::default()
            },
        );
        let raw = serde_json::json!({
            "path": "src/foo.rs",
            "content": long_article(),
            "start_line": 1,
            "end_line": 200,
        })
        .to_string();
        let ctx = HookContext {
            stage: HookStage::PostTool,
            content: raw,
            metadata: [
                ("tool_name".into(), json!("read")),
                ("arguments".into(), json!(r#"{"path":"src/foo.rs"}"#)),
            ]
            .into(),
        };

        let out = hook.execute(ctx).await.expect("hook");
        let parsed: Value = serde_json::from_str(&out.content).expect("stub JSON");
        let id = parsed
            .get("notepad_ref")
            .and_then(Value::as_str)
            .expect("non-empty notepad id");
        assert!(!id.is_empty());
        assert_eq!(
            parsed.get("path").and_then(Value::as_str),
            Some("src/foo.rs")
        );
        assert!(
            parsed
                .get("note")
                .and_then(Value::as_str)
                .is_some_and(|note| {
                    note.contains("notepad")
                        && note.contains("action=read")
                        && note.contains("Do not re-run")
                })
        );
        let np = notepad.read().await;
        let entry = np.entries().first().expect("stored read");
        assert_eq!(
            entry.metadata.get("path").and_then(Value::as_str),
            Some("src/foo.rs")
        );
        match &entry.payload {
            StoredPayload::Text(text) => assert!(text.contains("garam masala")),
            StoredPayload::Json(_) => panic!("expected exact read text payload"),
        }
    }

    #[tokio::test]
    async fn read_without_path_stays_inline() {
        let notepad = Arc::new(RwLock::new(Notepad::new()));
        let hook = OffloadHook::new(
            Arc::clone(&notepad),
            OffloadConfig {
                min_chars: 10,
                never_offload_tools: Vec::new(),
                ..Default::default()
            },
        );
        let raw = serde_json::json!({ "content": long_article() }).to_string();
        let ctx = HookContext {
            stage: HookStage::PostTool,
            content: raw.clone(),
            metadata: [("tool_name".into(), json!("read"))].into(),
        };

        let out = hook.execute(ctx).await.expect("hook");
        assert_eq!(out.content, raw);
        assert!(notepad.read().await.entries().is_empty());
    }

    #[test]
    fn stub_includes_read_pagination_identity() {
        let stub = make_stub(
            EntryId::new_v4(),
            "read",
            1200,
            "preview",
            &HashMap::from([
                ("path".into(), json!("src/foo.rs")),
                ("start_line".into(), json!(1)),
                ("end_line".into(), json!(200)),
                ("total_lines".into(), json!(1500)),
                ("has_more".into(), json!(true)),
            ]),
        );
        let parsed: Value = serde_json::from_str(&stub).unwrap();
        assert_eq!(parsed.get("has_more").and_then(Value::as_bool), Some(true));
        assert_eq!(
            parsed.get("total_lines").and_then(Value::as_u64),
            Some(1500)
        );
        assert_eq!(parsed.get("start_line").and_then(Value::as_u64), Some(1));
        assert_eq!(parsed.get("end_line").and_then(Value::as_u64), Some(200));
    }

    #[test]
    fn multi_stub_includes_read_pagination_identity() {
        let stub = make_multi_stub(
            &[StoredRef {
                id: EntryId::new_v4(),
                url: None,
                path: Some("src/foo.rs".into()),
                stored_chars: 1200,
                preview: "preview".into(),
                identity: HashMap::from([
                    ("start_line".into(), json!(201)),
                    ("end_line".into(), json!(400)),
                    ("total_lines".into(), json!(1500)),
                    ("has_more".into(), json!(true)),
                ]),
            }],
            "read",
        );
        let parsed: Value = serde_json::from_str(&stub).unwrap();
        let entry = &parsed["notepad_refs"][0];
        assert_eq!(entry["path"], json!("src/foo.rs"));
        assert_eq!(entry["start_line"], json!(201));
        assert_eq!(entry["end_line"], json!(400));
        assert_eq!(entry["total_lines"], json!(1500));
        assert_eq!(entry["has_more"], json!(true));
    }

    #[test]
    fn read_uses_per_tool_threshold() {
        let config = OffloadConfig {
            min_chars: 100,
            min_chars_by_tool: HashMap::from([("read".into(), 16_000)]),
            never_offload_tools: Vec::new(),
            ..Default::default()
        };
        let medium = "x".repeat(5000);
        let read_result = serde_json::json!({
            "path": "src/foo.rs",
            "content": medium,
            "start_line": 1,
            "end_line": 200,
            "total_lines": 1500,
            "has_more": true,
        })
        .to_string();
        assert!(!should_offload(&config, "read", &read_result, None));
        assert!(should_offload(
            &config,
            "fetch",
            &serde_json::json!({"content": medium}).to_string(),
            None
        ));
    }

    #[test]
    fn stub_includes_path_identity() {
        let stub = make_stub(
            EntryId::new_v4(),
            "read",
            1200,
            "preview",
            &HashMap::from([("path".into(), json!("src/foo.rs"))]),
        );
        let parsed: Value = serde_json::from_str(&stub).unwrap();
        assert_eq!(
            parsed.get("path").and_then(Value::as_str),
            Some("src/foo.rs")
        );
    }

    #[tokio::test]
    async fn notepad_read_passes_through_unchanged() {
        let notepad = Arc::new(RwLock::new(Notepad::new()));
        let hook = OffloadHook::new(
            Arc::clone(&notepad),
            OffloadConfig {
                min_chars: 100,
                ..Default::default()
            },
        );
        let read_result = serde_json::json!({ "content": long_article() }).to_string();
        let ctx = HookContext {
            stage: HookStage::PostTool,
            content: read_result.clone(),
            metadata: [("tool_name".into(), json!(crate::tools::NOTEPAD_TOOL_NAME))].into(),
        };
        let out = hook.execute(ctx).await.expect("hook");
        assert_eq!(out.content, read_result);
        assert!(notepad.read().await.entries().is_empty());
    }

    #[tokio::test]
    async fn batch_fetch_stores_multiple_entries() {
        let notepad = Arc::new(RwLock::new(Notepad::new()));
        let hook = OffloadHook::new(
            Arc::clone(&notepad),
            OffloadConfig {
                auto_offload: true,
                min_chars: 100,
                ..Default::default()
            },
        );
        let article_a = long_article();
        let mut article_b = long_article();
        article_b.push_str("\nExtra paragraph for page b.");
        let raw = serde_json::json!({
            "results": [
                {
                    "url": "https://example.com/a",
                    "content": article_a,
                    "truncated": false,
                    "content_type": "text/html",
                },
                {
                    "ok": false,
                    "url": "https://example.com/blocked",
                    "error": "page looks blocked or is not usable content",
                },
                {
                    "url": "https://example.com/b",
                    "content": article_b,
                    "truncated": false,
                    "content_type": "text/html",
                },
            ]
        })
        .to_string();
        let ctx = HookContext {
            stage: HookStage::PostTool,
            content: raw,
            metadata: [("tool_name".into(), json!("fetch"))].into(),
        };
        let out = hook.execute(ctx).await.expect("hook");
        assert!(is_notepad_stub(&out.content));
        let parsed: Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(parsed.get("count").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(notepad.read().await.entries().len(), 2);
    }
}
