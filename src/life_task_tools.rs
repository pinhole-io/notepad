//! Agent-facing life task registry tools.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use superglue::tools::{Tool, ToolContextPolicy, ToolInvokeError, ToolRegistry, ToolSpec};
use time::OffsetDateTime;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::life_task::{LifeTaskCreate, LifeTaskId, LifeTaskStatus};
use crate::life_task_registry::{LifeTaskFilter, LifeTaskRegistry};

pub const LIFE_TASK_TOOL_NAME: &str = "task";

/// Shared state for life task agent tools.
pub struct LifeTaskToolState {
    registry: Arc<RwLock<LifeTaskRegistry>>,
    persist_path: PathBuf,
}

impl LifeTaskToolState {
    async fn reload_from_disk(&self) -> Result<(), ToolInvokeError> {
        let fresh = LifeTaskRegistry::load(&self.persist_path).map_err(|e| {
            ToolInvokeError::handler(format!("failed to load life task registry: {e}"), None)
        })?;
        *self.registry.write().await = fresh;
        Ok(())
    }

    async fn save(&self) -> Result<(), ToolInvokeError> {
        let reg = self.registry.read().await;
        reg.save(&self.persist_path).map_err(|e| {
            ToolInvokeError::handler(format!("failed to save life task registry: {e}"), None)
        })
    }
}

/// Build shared life task tool state.
#[must_use]
pub fn life_task_registry_state(
    registry: Arc<RwLock<LifeTaskRegistry>>,
    persist_path: PathBuf,
) -> Arc<LifeTaskToolState> {
    Arc::new(LifeTaskToolState {
        registry,
        persist_path,
    })
}

/// Register the life task tool (all actions available in every mode).
pub async fn register_life_task_tools(
    registry: &ToolRegistry,
    state: Arc<LifeTaskToolState>,
) -> Result<(), ToolInvokeError> {
    registry.register(Arc::new(LifeTaskTool { state })).await
}

pub struct LifeTaskTool {
    state: Arc<LifeTaskToolState>,
}

fn tool_error(msg: impl Into<String>) -> ToolInvokeError {
    ToolInvokeError::handler(msg, Some(LIFE_TASK_TOOL_NAME.to_string()))
}

fn parse_task_id(raw: &str) -> Result<LifeTaskId, ToolInvokeError> {
    let uuid: Uuid = raw
        .parse()
        .map_err(|_| tool_error(format!("invalid task id: {raw}")))?;
    Ok(LifeTaskId(uuid))
}

fn parse_status(raw: &str) -> Result<LifeTaskStatus, ToolInvokeError> {
    match raw {
        "open" => Ok(LifeTaskStatus::Open),
        "in_progress" | "active" => Ok(LifeTaskStatus::InProgress),
        "done" | "completed" => Ok(LifeTaskStatus::Done),
        "cancelled" | "canceled" => Ok(LifeTaskStatus::Cancelled),
        other => Err(tool_error(format!(
            "invalid task status: {other} (use open, in_progress, done, or cancelled)"
        ))),
    }
}

fn optional_status(arguments: &Value) -> Result<Option<LifeTaskStatus>, ToolInvokeError> {
    match arguments.get("status").and_then(Value::as_str) {
        None | Some("") => Ok(None),
        Some(raw) => parse_status(raw).map(Some),
    }
}

fn parse_due_at(raw: &str) -> Result<OffsetDateTime, ToolInvokeError> {
    OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339).map_err(|_| {
        tool_error(format!(
            "invalid due_at: {raw} (use RFC3339, e.g. 2026-07-25T17:00:00Z)"
        ))
    })
}

fn optional_due_at(
    arguments: &Value,
    key: &str,
) -> Result<Option<OffsetDateTime>, ToolInvokeError> {
    match arguments.get(key).and_then(Value::as_str) {
        None => Ok(None),
        Some("") => Ok(None),
        Some(raw) => parse_due_at(raw).map(Some),
    }
}

fn optional_due_at_update(
    arguments: &Value,
) -> Result<Option<Option<OffsetDateTime>>, ToolInvokeError> {
    if !arguments.get("due_at").is_some() {
        return Ok(None);
    }
    match arguments.get("due_at").and_then(Value::as_str) {
        Some("") => Ok(Some(None)),
        Some(raw) => parse_due_at(raw).map(|d| Some(Some(d))),
        None => Ok(Some(None)),
    }
}

fn optional_due_before(arguments: &Value) -> Result<Option<OffsetDateTime>, ToolInvokeError> {
    match arguments.get("due_before").and_then(Value::as_str) {
        None | Some("") => Ok(None),
        Some(raw) => parse_due_at(raw).map(Some),
    }
}

fn task_summary_json(task: &crate::life_task::LifeTask) -> Value {
    json!({
        "id": task.id.to_string(),
        "title": task.title,
        "status": task.status,
        "tags": task.tags,
        "due_at": task.due_at.map(|d| d.to_string()),
        "updated_at": task.updated_at.to_string(),
        "deleted": task.is_deleted(),
    })
}

fn task_full_json(task: &crate::life_task::LifeTask) -> Value {
    json!({
        "id": task.id.to_string(),
        "title": task.title,
        "description": task.description,
        "status": task.status,
        "tags": task.tags,
        "notes": task.notes.iter().map(|n| json!({
            "id": n.id.to_string(),
            "content": n.content,
            "created_at": n.created_at.to_string(),
        })).collect::<Vec<_>>(),
        "created_at": task.created_at.to_string(),
        "updated_at": task.updated_at.to_string(),
        "completed_at": task.completed_at.map(|d| d.to_string()),
        "due_at": task.due_at.map(|d| d.to_string()),
        "deleted_at": task.deleted_at.map(|d| d.to_string()),
        "revision": task.revision,
    })
}

fn task_parameters_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["list", "get", "create", "update", "set_status", "add_note", "delete"],
                "description": "Life task registry action"
            },
            "task_id": { "type": "string", "description": "Task id" },
            "title": { "type": "string", "description": "Task title (create/update)" },
            "description": { "type": "string", "description": "Task description (create/update)" },
            "tags": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Tags for grouping tasks (create/update)"
            },
            "status": {
                "type": "string",
                "description": "Task status: open|in_progress|done|cancelled (create/set_status/list filter)"
            },
            "tag": { "type": "string", "description": "Filter by tag (list)" },
            "due_at": { "type": "string", "description": "Due date/time in RFC3339 UTC (create/update); empty string on update clears" },
            "due_before": { "type": "string", "description": "List tasks with due_at on or before this RFC3339 instant" },
            "limit": { "type": "integer", "description": "Max results (list)" },
            "content": { "type": "string", "description": "Note content (add_note)" }
        },
        "required": ["action"]
    })
}

async fn task_list(state: &LifeTaskToolState, arguments: &Value) -> Result<Value, ToolInvokeError> {
    state.reload_from_disk().await?;
    let filter = LifeTaskFilter {
        status: optional_status(arguments)?,
        tag: arguments
            .get("tag")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        due_before: optional_due_before(arguments)?,
        limit: arguments
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n as usize),
        include_deleted: false,
    };
    let reg = state.registry.read().await;
    let tasks: Vec<Value> = reg
        .list(&filter)
        .iter()
        .map(|t| task_summary_json(t))
        .collect();
    Ok(json!({
        "tasks": tasks,
        "count": tasks.len(),
    }))
}

async fn task_get(state: &LifeTaskToolState, arguments: &Value) -> Result<Value, ToolInvokeError> {
    state.reload_from_disk().await?;
    let raw = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error("missing task_id"))?;
    let task_id = parse_task_id(raw)?;
    let reg = state.registry.read().await;
    let task = reg
        .get(task_id)
        .ok_or_else(|| tool_error(format!("task not found: {raw}")))?;
    Ok(task_full_json(task))
}

async fn task_create(
    state: &LifeTaskToolState,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let title = arguments
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error("missing title"))?;
    let description = arguments
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let tags: Vec<String> = arguments
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    let input = LifeTaskCreate {
        title: title.to_string(),
        description,
        tags,
        status: optional_status(arguments)?,
        due_at: optional_due_at(arguments, "due_at")?,
    };
    let task_id = {
        let mut reg = state.registry.write().await;
        reg.create(input).map_err(|e| tool_error(e.to_string()))?
    };
    state.save().await?;
    Ok(json!({
        "task_id": task_id.to_string(),
        "message": "task created",
    }))
}

async fn task_update(
    state: &LifeTaskToolState,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let raw = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error("missing task_id"))?;
    let task_id = parse_task_id(raw)?;
    let title = arguments
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_string);
    let description = if arguments.get("description").is_some() {
        Some(
            arguments
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        )
    } else {
        None
    };
    let tags = arguments.get("tags").and_then(|v| {
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    item.as_str()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                })
                .collect::<Vec<_>>()
        })
    });
    let due_at = optional_due_at_update(arguments)?;
    {
        let mut reg = state.registry.write().await;
        reg.update(task_id, title, description, tags, due_at)
            .map_err(|e| tool_error(e.to_string()))?;
    }
    state.save().await?;
    Ok(json!({
        "task_id": task_id.to_string(),
        "message": "task updated",
    }))
}

async fn task_set_status(
    state: &LifeTaskToolState,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let raw = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error("missing task_id"))?;
    let task_id = parse_task_id(raw)?;
    let status = arguments
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error("missing status"))?;
    let status = parse_status(status)?;
    {
        let mut reg = state.registry.write().await;
        reg.set_status(task_id, status)
            .map_err(|e| tool_error(e.to_string()))?;
    }
    state.save().await?;
    Ok(json!({
        "task_id": task_id.to_string(),
        "status": status,
        "message": "task status updated",
    }))
}

async fn task_add_note(
    state: &LifeTaskToolState,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let raw = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error("missing task_id"))?;
    let task_id = parse_task_id(raw)?;
    let content = arguments
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error("missing content"))?;
    let note_id = {
        let mut reg = state.registry.write().await;
        reg.add_note(task_id, content.to_string())
            .map_err(|e| tool_error(e.to_string()))?
    };
    state.save().await?;
    Ok(json!({
        "task_id": task_id.to_string(),
        "note_id": note_id.to_string(),
        "message": "note added",
    }))
}

async fn task_delete(
    state: &LifeTaskToolState,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let raw = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error("missing task_id"))?;
    let task_id = parse_task_id(raw)?;
    {
        let mut reg = state.registry.write().await;
        reg.delete(task_id).map_err(|e| tool_error(e.to_string()))?;
    }
    state.save().await?;
    Ok(json!({
        "task_id": task_id.to_string(),
        "message": "task deleted",
    }))
}

#[async_trait]
impl Tool for LifeTaskTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: LIFE_TASK_TOOL_NAME.to_string(),
            description: Some(
                "Manage durable life-wide personal tasks (distinct from coding `plan` and session `todo`). \
                 Actions: list, get, create, update, set_status, add_note, delete."
                    .to_string(),
            ),
            static_tool: false,
            parameters_schema: task_parameters_schema(),
        }
    }

    fn context_policy(&self) -> ToolContextPolicy {
        ToolContextPolicy::Inline
    }

    async fn call(&self, arguments: Value) -> Result<Value, ToolInvokeError> {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("missing action"))?;
        match action {
            "list" => task_list(&self.state, &arguments).await,
            "get" => task_get(&self.state, &arguments).await,
            "create" => task_create(&self.state, &arguments).await,
            "update" => task_update(&self.state, &arguments).await,
            "set_status" => task_set_status(&self.state, &arguments).await,
            "add_note" => task_add_note(&self.state, &arguments).await,
            "delete" => task_delete(&self.state, &arguments).await,
            other => Err(tool_error(format!("unknown action: {other}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use superglue::tools::Tool;
    use tempfile::TempDir;

    fn test_state(tmp: &TempDir) -> Arc<LifeTaskToolState> {
        let path = tmp.path().join("registry.json");
        life_task_registry_state(Arc::new(RwLock::new(LifeTaskRegistry::new())), path)
    }

    #[tokio::test]
    async fn create_list_get_flow() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = LifeTaskTool {
            state: Arc::clone(&state),
        };
        let out = tool
            .call(json!({
                "action": "create",
                "title": "Buy milk",
                "tags": ["errands"]
            }))
            .await
            .unwrap();
        let task_id = out.get("task_id").unwrap().as_str().unwrap();
        let list = tool.call(json!({ "action": "list" })).await.unwrap();
        assert_eq!(list["count"], 1);
        let full = tool
            .call(json!({ "action": "get", "task_id": task_id }))
            .await
            .unwrap();
        assert_eq!(full.get("title").and_then(|v| v.as_str()), Some("Buy milk"));
    }

    #[tokio::test]
    async fn note_status_delete_flow() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = LifeTaskTool {
            state: Arc::clone(&state),
        };
        let out = tool
            .call(json!({
                "action": "create",
                "title": "Call dentist"
            }))
            .await
            .unwrap();
        let task_id = out["task_id"].as_str().unwrap();
        tool.call(json!({
            "action": "add_note",
            "task_id": task_id,
            "content": "Tuesday 9am"
        }))
        .await
        .unwrap();
        tool.call(json!({
            "action": "set_status",
            "task_id": task_id,
            "status": "done"
        }))
        .await
        .unwrap();
        tool.call(json!({
            "action": "delete",
            "task_id": task_id
        }))
        .await
        .unwrap();
        let list = tool.call(json!({ "action": "list" })).await.unwrap();
        assert_eq!(list["count"], 0);
        let full = tool
            .call(json!({ "action": "get", "task_id": task_id }))
            .await
            .unwrap();
        assert!(full.get("deleted_at").and_then(|v| v.as_str()).is_some());
    }

    #[tokio::test]
    async fn due_at_create_get_update_clear_flow() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = LifeTaskTool {
            state: Arc::clone(&state),
        };
        let due = "2026-12-25T09:00:00Z";
        let out = tool
            .call(json!({
                "action": "create",
                "title": "Christmas prep",
                "due_at": due
            }))
            .await
            .unwrap();
        let task_id = out["task_id"].as_str().unwrap();
        let full = tool
            .call(json!({ "action": "get", "task_id": task_id }))
            .await
            .unwrap();
        assert!(full.get("due_at").is_some_and(|v| !v.is_null()));
        tool.call(json!({
            "action": "update",
            "task_id": task_id,
            "due_at": ""
        }))
        .await
        .unwrap();
        let cleared = tool
            .call(json!({ "action": "get", "task_id": task_id }))
            .await
            .unwrap();
        assert!(cleared.get("due_at").is_none() || cleared["due_at"].is_null());
    }
}
