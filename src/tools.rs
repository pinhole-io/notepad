//! Agent-facing scratchpad tools — the model decides what to store and when to read back.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use superglue::tools::{
    OnToolError, Tool, ToolContextPolicy, ToolInvokeError, ToolRegistry, ToolRetryPolicy, ToolSpec,
};
use tokio::sync::RwLock;

use crate::entry::{EntryId, StoredPayload};
use crate::notepad::Notepad;

pub const NOTEPAD_TOOL_NAME: &str = "notepad";
const BATCH_INPUT_LIMIT: usize = 8;

/// Scratchpad tools return errors to the model instead of aborting the chat loop.
fn notepad_tool_policy() -> ToolRetryPolicy {
    ToolRetryPolicy {
        on_error: OnToolError::Skip,
    }
}

async fn register_notepad_tool(
    registry: &ToolRegistry,
    tool: Arc<dyn Tool>,
) -> Result<(), ToolInvokeError> {
    registry
        .register_with_policy(tool, notepad_tool_policy())
        .await
}

/// Register the consolidated scratchpad tool on a superglue [`ToolRegistry`].
pub async fn register_notepad_tools(
    registry: &ToolRegistry,
    notepad: Arc<RwLock<Notepad>>,
) -> Result<(), ToolInvokeError> {
    register_notepad_tool(registry, Arc::new(NotepadTool { notepad })).await
}

pub struct NotepadTool {
    notepad: Arc<RwLock<Notepad>>,
}

fn tool_error(name: &str, msg: impl Into<String>) -> ToolInvokeError {
    ToolInvokeError::handler(msg, Some(name.to_string()))
}

fn parse_id(name: &str, raw: &str) -> Result<EntryId, ToolInvokeError> {
    let uuid: uuid::Uuid = raw
        .parse()
        .map_err(|_| tool_error(name, format!("invalid entry id: {raw}")))?;
    Ok(EntryId(uuid))
}

fn preview(content: &str, max: usize) -> String {
    let flat: String = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max).collect::<String>())
    }
}

fn singular_entry_id(arguments: &Value) -> Option<&str> {
    for key in ["id", "notepad_ref"] {
        if let Some(s) = arguments.get(key).and_then(Value::as_str) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

/// Resolve entry ids for read. Empty `ids: []` (common when models fill unused
/// optional fields) falls back to `id` / `notepad_ref` instead of failing.
fn parse_id_batch(name: &str, arguments: &Value) -> Result<Vec<String>, ToolInvokeError> {
    if let Some(arr) = arguments.get("ids") {
        let items = arr
            .as_array()
            .ok_or_else(|| tool_error(name, "`ids` must be an array"))?;
        if !items.is_empty() {
            if items.len() > BATCH_INPUT_LIMIT {
                return Err(tool_error(
                    name,
                    format!("`ids` supports at most {BATCH_INPUT_LIMIT} entries"),
                ));
            }
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                let s = item
                    .as_str()
                    .ok_or_else(|| tool_error(name, format!("`ids[{i}]` must be a string")))?;
                if s.is_empty() {
                    return Err(tool_error(name, format!("`ids[{i}]` must not be empty")));
                }
                out.push(s.to_string());
            }
            return Ok(out);
        }
        // Empty array: fall through to singular id / notepad_ref.
    }
    if let Some(s) = singular_entry_id(arguments) {
        return Ok(vec![s.to_string()]);
    }
    Err(tool_error(name, "missing `id`, `notepad_ref`, or `ids`"))
}

fn notepad_parameters_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["store", "list", "read", "remove"],
                "description": "store: save a note; list: list entries; read: read by id; remove: delete by id"
            },
            "content": {
                "type": "string",
                "description": "Text or JSON string to remember (store)"
            },
            "label": {
                "type": "string",
                "description": "Short label for store (e.g. page title or topic)"
            },
            "tags": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional tags for filtering (store)"
            },
            "tag": {
                "type": "string",
                "description": "Filter entries by tag (list)"
            },
            "id": {
                "type": "string",
                "description": "Entry id from list/store, or notepad_ref from an offload stub (read, remove)"
            },
            "notepad_ref": {
                "type": "string",
                "description": "Alias for id — copy from an offload stub's notepad_ref field (read)"
            },
            "ids": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Up to 8 entry ids to read in one call (read). Omit or leave unset when using id; do not pass []."
            }
        },
        "required": ["action"]
    })
}

async fn notepad_store(
    notepad: &Arc<RwLock<Notepad>>,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let content = arguments
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(NOTEPAD_TOOL_NAME, "missing content"))?;
    let label = arguments.get("label").and_then(Value::as_str);
    let tags: Vec<&str> = arguments
        .get("tags")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let payload = if serde_json::from_str::<Value>(content).is_ok() {
        StoredPayload::Json(serde_json::from_str(content).map_err(|e| {
            tool_error(NOTEPAD_TOOL_NAME, format!("content is not valid JSON: {e}"))
        })?)
    } else {
        StoredPayload::Text(content.to_string())
    };

    let stored_chars = payload.char_len();
    let id = {
        let mut np = notepad.write().await;
        np.store_tagged(&tags, label, payload, Default::default())
    };

    Ok(json!({
        "id": id.to_string(),
        "stored_chars": stored_chars,
        "message": "stored in notepad scratchpad"
    }))
}

async fn notepad_list(
    notepad: &Arc<RwLock<Notepad>>,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let tag_filter = arguments.get("tag").and_then(Value::as_str);
    let np = notepad.read().await;
    let entries: Vec<Value> = np
        .entries()
        .iter()
        .filter(|e| tag_filter.is_none_or(|t| e.tags.iter().any(|tag| tag == t)))
        .map(|e| {
            json!({
                "id": e.id.to_string(),
                "label": e.label,
                "tags": e.tags,
                "stored_chars": e.char_len(),
                "preview": preview(&e.payload.as_render_body(), 240),
            })
        })
        .collect();

    Ok(json!({
        "count": entries.len(),
        "entries": entries,
    }))
}

async fn notepad_read(
    notepad: &Arc<RwLock<Notepad>>,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let ids = match parse_id_batch(NOTEPAD_TOOL_NAME, arguments) {
        Ok(ids) => ids,
        Err(_) => {
            // Empty `ids: []` with no id — common model mistake after offload stubs.
            // Return the current index so the next call can succeed without looping.
            let listed = notepad_list(notepad, arguments).await?;
            return Ok(json!({
                "ok": false,
                "error": "read needs a non-empty `id` (or `notepad_ref` from an offload stub). \
                    `ids: []` is invalid. Listing current scratchpad entries — pick an id and call \
                    action=read with that id.",
                "count": listed.get("count").cloned().unwrap_or(json!(0)),
                "entries": listed.get("entries").cloned().unwrap_or(json!([])),
            }));
        }
    };
    let np = notepad.read().await;

    if ids.len() == 1 {
        let raw = &ids[0];
        let id = parse_id(NOTEPAD_TOOL_NAME, raw)?;
        let entry = np
            .get(id)
            .ok_or_else(|| tool_error(NOTEPAD_TOOL_NAME, format!("entry not found: {raw}")))?;
        return Ok(json!({
            "id": entry.id.to_string(),
            "label": entry.label,
            "tags": entry.tags,
            "content": entry.payload.as_render_body(),
        }));
    }

    let entries: Vec<Value> = ids
        .iter()
        .map(|raw| match parse_id(NOTEPAD_TOOL_NAME, raw) {
            Ok(id) => match np.get(id) {
                Some(entry) => json!({
                    "id": entry.id.to_string(),
                    "label": entry.label,
                    "tags": entry.tags,
                    "content": entry.payload.as_render_body(),
                }),
                None => json!({
                    "id": raw,
                    "error": format!("entry not found: {raw}"),
                }),
            },
            Err(e) => json!({
                "id": raw,
                "error": e.to_string(),
            }),
        })
        .collect();

    Ok(json!({ "entries": entries }))
}

async fn notepad_remove(
    notepad: &Arc<RwLock<Notepad>>,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let raw = singular_entry_id(arguments)
        .ok_or_else(|| tool_error(NOTEPAD_TOOL_NAME, "missing id or notepad_ref"))?;
    let id = parse_id(NOTEPAD_TOOL_NAME, raw)?;
    let removed = {
        let mut np = notepad.write().await;
        np.remove(id)
    };
    match removed {
        Some(entry) => Ok(json!({
            "removed": entry.id.to_string(),
            "label": entry.label,
        })),
        None => Err(tool_error(
            NOTEPAD_TOOL_NAME,
            format!("entry not found: {raw}"),
        )),
    }
}

#[async_trait]
impl Tool for NotepadTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: NOTEPAD_TOOL_NAME.to_string(),
            description: Some(
                "Session scratchpad memory. Actions: store (save distilled note), list, read, remove. \
                 For read, pass `id` (or `notepad_ref` from an offload stub) for one entry, or `ids` \
                 (up to 8, non-empty) to batch-read. Never pass `ids: []` — omit ids when using id. \
                 Store facts, quotes, or summaries you will need later — not raw tool dumps or Gmail message ids."
                    .to_string(),
            ),
            static_tool: false,
            parameters_schema: notepad_parameters_schema(),
        }
    }

    fn context_policy(&self) -> ToolContextPolicy {
        ToolContextPolicy::Inline
    }

    async fn call(&self, arguments: Value) -> Result<Value, ToolInvokeError> {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error(NOTEPAD_TOOL_NAME, "missing action"))?;
        match action {
            "store" => notepad_store(&self.notepad, &arguments).await,
            "list" => notepad_list(&self.notepad, &arguments).await,
            "read" => notepad_read(&self.notepad, &arguments).await,
            "remove" => notepad_remove(&self.notepad, &arguments).await,
            other => Err(tool_error(
                NOTEPAD_TOOL_NAME,
                format!("unknown action: {other}"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use superglue::tools::Tool;

    #[tokio::test]
    async fn store_and_list() {
        let np = Arc::new(RwLock::new(Notepad::new()));
        let tool = NotepadTool { notepad: np };

        let out = tool
            .call(json!({
                "action": "store",
                "content": "Rust 1.88 stable",
                "label": "version"
            }))
            .await
            .unwrap();
        let id = out.get("id").unwrap().as_str().unwrap();

        let listed = tool.call(json!({ "action": "list" })).await.unwrap();
        assert_eq!(listed.get("count").and_then(|v| v.as_u64()), Some(1));
        let entries = listed.get("entries").unwrap().as_array().unwrap();
        assert_eq!(entries[0].get("id").and_then(|v| v.as_str()), Some(id));
    }

    #[tokio::test]
    async fn batch_read_partial_failure() {
        let np = Arc::new(RwLock::new(Notepad::new()));
        let tool = NotepadTool { notepad: np };

        let out = tool
            .call(json!({
                "action": "store",
                "content": "note one",
            }))
            .await
            .unwrap();
        let id = out.get("id").unwrap().as_str().unwrap();

        let read = tool
            .call(json!({
                "action": "read",
                "ids": [id, "00000000-0000-0000-0000-000000000000"],
            }))
            .await
            .unwrap();
        let entries = read.get("entries").unwrap().as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].get("content").is_some());
        assert!(entries[1].get("error").is_some());
    }

    #[tokio::test]
    async fn read_empty_ids_lists_entries_for_recovery() {
        let np = Arc::new(RwLock::new(Notepad::new()));
        let tool = NotepadTool {
            notepad: Arc::clone(&np),
        };
        let stored = tool
            .call(json!({
                "action": "store",
                "content": "recover me",
                "label": "note",
            }))
            .await
            .unwrap();
        let id = stored
            .get("id")
            .and_then(Value::as_str)
            .unwrap()
            .to_string();

        let out = tool
            .call(json!({ "action": "read", "ids": [] }))
            .await
            .unwrap();
        assert_eq!(out.get("ok"), Some(&json!(false)));
        assert_eq!(out.get("count").and_then(Value::as_u64), Some(1));
        let entries = out.get("entries").and_then(Value::as_array).unwrap();
        assert_eq!(
            entries[0].get("id").and_then(Value::as_str),
            Some(id.as_str())
        );
    }

    #[tokio::test]
    async fn read_empty_ids_falls_back_to_id() {
        let np = Arc::new(RwLock::new(Notepad::new()));
        let tool = NotepadTool { notepad: np };
        let stored = tool
            .call(json!({
                "action": "store",
                "content": "via id fallback",
            }))
            .await
            .unwrap();
        let id = stored.get("id").and_then(Value::as_str).unwrap();

        let out = tool
            .call(json!({ "action": "read", "ids": [], "id": id }))
            .await
            .unwrap();
        assert_eq!(
            out.get("content").and_then(Value::as_str),
            Some("via id fallback")
        );
    }

    #[tokio::test]
    async fn read_accepts_notepad_ref_alias() {
        let np = Arc::new(RwLock::new(Notepad::new()));
        let tool = NotepadTool { notepad: np };
        let stored = tool
            .call(json!({
                "action": "store",
                "content": "from stub field",
            }))
            .await
            .unwrap();
        let id = stored.get("id").and_then(Value::as_str).unwrap();

        let out = tool
            .call(json!({ "action": "read", "notepad_ref": id }))
            .await
            .unwrap();
        assert_eq!(
            out.get("content").and_then(Value::as_str),
            Some("from stub field")
        );
    }
}
