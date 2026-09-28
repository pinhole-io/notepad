//! Agent-facing session artifact tools.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use superglue::tools::{
    OnToolError, Tool, ToolContextPolicy, ToolInvokeError, ToolRegistry, ToolRetryPolicy, ToolSpec,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::artifact::{Artifact, ArtifactId};
use crate::artifact_store::ArtifactStore;

pub const ARTIFACT_TOOL_NAME: &str = "artifact";

fn artifact_tool_policy() -> ToolRetryPolicy {
    ToolRetryPolicy {
        on_error: OnToolError::Skip,
    }
}

async fn register_artifact_tool(
    registry: &ToolRegistry,
    tool: Arc<dyn Tool>,
) -> Result<(), ToolInvokeError> {
    registry
        .register_with_policy(tool, artifact_tool_policy())
        .await
}

/// Callback invoked after an artifact is created or updated (e.g. to emit UI events).
pub type ArtifactHook = Arc<dyn Fn(Artifact) + Send + Sync>;

/// Shared state for artifact agent tools.
pub struct ArtifactToolState {
    store: Arc<RwLock<ArtifactStore>>,
    on_create: Option<ArtifactHook>,
    on_update: Option<ArtifactHook>,
}

impl ArtifactToolState {
    fn notify_create(&self, artifact: &Artifact) {
        if let Some(hook) = &self.on_create {
            hook(artifact.clone());
        }
    }

    fn notify_update(&self, artifact: &Artifact) {
        if let Some(hook) = &self.on_update {
            hook(artifact.clone());
        }
    }
}

/// Build shared artifact tool state.
#[must_use]
pub fn artifact_tool_state(
    store: Arc<RwLock<ArtifactStore>>,
    on_create: Option<ArtifactHook>,
    on_update: Option<ArtifactHook>,
) -> Arc<ArtifactToolState> {
    Arc::new(ArtifactToolState {
        store,
        on_create,
        on_update,
    })
}

/// Register the consolidated artifact tool on a superglue [`ToolRegistry`].
pub async fn register_artifact_tools(
    registry: &ToolRegistry,
    state: Arc<ArtifactToolState>,
) -> Result<(), ToolInvokeError> {
    register_artifact_tool(registry, Arc::new(ArtifactTool { state })).await
}

pub struct ArtifactTool {
    state: Arc<ArtifactToolState>,
}

fn tool_error(name: &str, msg: impl Into<String>) -> ToolInvokeError {
    ToolInvokeError::handler(msg, Some(name.to_string()))
}

fn parse_id(name: &str, raw: &str) -> Result<ArtifactId, ToolInvokeError> {
    let uuid: Uuid = raw
        .parse()
        .map_err(|_| tool_error(name, format!("invalid artifact id: {raw}")))?;
    Ok(ArtifactId(uuid))
}

fn preview(content: &str, max: usize) -> String {
    let flat: String = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max).collect::<String>())
    }
}

fn artifact_parameters_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["create", "update", "get", "list", "delete"],
                "description": "create: new markdown artifact; update: replace content; get: read by id; list: summaries; delete: remove"
            },
            "title": {
                "type": "string",
                "description": "Human-readable title (create, optional on update)"
            },
            "content": {
                "type": "string",
                "description": "Full markdown body (create, update)"
            },
            "description": {
                "type": "string",
                "description": "Optional one-line summary shown on the inline artifact card (create, update)"
            },
            "id": {
                "type": "string",
                "description": "Artifact id from create/list (get, update, delete)"
            }
        },
        "required": ["action"]
    })
}

async fn artifact_create(
    state: &Arc<ArtifactToolState>,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let title = arguments
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| tool_error(ARTIFACT_TOOL_NAME, "missing title"))?;
    let content = arguments
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(ARTIFACT_TOOL_NAME, "missing content"))?;
    let description = arguments
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let artifact = {
        let mut store = state.store.write().await;
        store.create(title.to_string(), content.to_string(), description)
    };
    state.notify_create(&artifact);

    Ok(json!({
        "id": artifact.id.to_string(),
        "title": artifact.title,
        "description": artifact.description,
        "version": artifact.version,
        "preview": preview(&artifact.content, 240),
        "message": "artifact created"
    }))
}

async fn artifact_update(
    state: &Arc<ArtifactToolState>,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let raw_id = arguments
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(ARTIFACT_TOOL_NAME, "missing id"))?;
    let id = parse_id(ARTIFACT_TOOL_NAME, raw_id)?;
    let content = arguments
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(ARTIFACT_TOOL_NAME, "missing content"))?;
    let title = arguments
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let description = arguments
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let artifact = {
        let mut store = state.store.write().await;
        store
            .update(id, title, content.to_string(), description)
            .map_err(|e| tool_error(ARTIFACT_TOOL_NAME, e.to_string()))?
    };
    state.notify_update(&artifact);

    Ok(json!({
        "id": artifact.id.to_string(),
        "title": artifact.title,
        "description": artifact.description,
        "version": artifact.version,
        "preview": preview(&artifact.content, 240),
        "message": "artifact updated"
    }))
}

async fn artifact_get(
    state: &Arc<ArtifactToolState>,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let raw_id = arguments
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(ARTIFACT_TOOL_NAME, "missing id"))?;
    let id = parse_id(ARTIFACT_TOOL_NAME, raw_id)?;
    let store = state.store.read().await;
    let artifact = store
        .get(id)
        .ok_or_else(|| tool_error(ARTIFACT_TOOL_NAME, format!("artifact not found: {raw_id}")))?;

    Ok(json!({
        "id": artifact.id.to_string(),
        "title": artifact.title,
        "description": artifact.description,
        "content": artifact.content,
        "version": artifact.version,
        "updated_at": artifact.updated_at.to_string(),
    }))
}

async fn artifact_list(state: &Arc<ArtifactToolState>) -> Result<Value, ToolInvokeError> {
    let store = state.store.read().await;
    let artifacts: Vec<Value> = store
        .list()
        .iter()
        .map(|a| {
            json!({
                "id": a.id.to_string(),
                "title": a.title,
                "description": a.description,
                "version": a.version,
                "updated_at": a.updated_at.to_string(),
                "preview": preview(&a.content, 240),
            })
        })
        .collect();

    Ok(json!({
        "count": artifacts.len(),
        "artifacts": artifacts,
    }))
}

async fn artifact_delete(
    state: &Arc<ArtifactToolState>,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let raw_id = arguments
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(ARTIFACT_TOOL_NAME, "missing id"))?;
    let id = parse_id(ARTIFACT_TOOL_NAME, raw_id)?;
    let removed = {
        let mut store = state.store.write().await;
        store
            .remove(id)
            .map_err(|e| tool_error(ARTIFACT_TOOL_NAME, e.to_string()))?
    };

    Ok(json!({
        "removed": removed.id.to_string(),
        "title": removed.title,
    }))
}

#[async_trait]
impl Tool for ArtifactTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: ARTIFACT_TOOL_NAME.to_string(),
            description: Some(
                "Session markdown artifacts for user-facing deliverables. Use for documents, writeups, and specs the user should read — not private scratchpad notes (`notepad`) or implementation checklists (`plan`).

The chat UI shows inline preview cards with the description; do not link to artifacts in assistant text.

- `create` — title + markdown content, optional one-line description
- `update` — id + content, optional title and description
- `get`, `list`, `delete`"
                    .to_string(),
            ),
            static_tool: false,
            parameters_schema: artifact_parameters_schema(),
        }
    }

    fn context_policy(&self) -> ToolContextPolicy {
        ToolContextPolicy::Inline
    }

    async fn call(&self, arguments: Value) -> Result<Value, ToolInvokeError> {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error(ARTIFACT_TOOL_NAME, "missing action"))?;
        match action {
            "create" => artifact_create(&self.state, &arguments).await,
            "update" => artifact_update(&self.state, &arguments).await,
            "get" => artifact_get(&self.state, &arguments).await,
            "list" => artifact_list(&self.state).await,
            "delete" => artifact_delete(&self.state, &arguments).await,
            other => Err(tool_error(
                ARTIFACT_TOOL_NAME,
                format!("unknown action: {other}"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use superglue::tools::Tool;

    fn test_state() -> Arc<ArtifactToolState> {
        artifact_tool_state(Arc::new(RwLock::new(ArtifactStore::new())), None, None)
    }

    #[tokio::test]
    async fn create_list_get_update_delete() {
        let state = test_state();
        let tool = ArtifactTool {
            state: Arc::clone(&state),
        };

        let created = tool
            .call(json!({
                "action": "create",
                "title": "Spec",
                "content": "# Overview\n\nDetails here."
            }))
            .await
            .unwrap();
        let id = created.get("id").unwrap().as_str().unwrap();

        let listed = tool.call(json!({ "action": "list" })).await.unwrap();
        assert_eq!(listed.get("count").and_then(|v| v.as_u64()), Some(1));

        let got = tool
            .call(json!({ "action": "get", "id": id }))
            .await
            .unwrap();
        assert!(
            got.get("content")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("Overview")
        );

        let updated = tool
            .call(json!({
                "action": "update",
                "id": id,
                "content": "# Overview\n\nUpdated."
            }))
            .await
            .unwrap();
        assert_eq!(updated.get("version").and_then(|v| v.as_u64()), Some(2));

        let deleted = tool
            .call(json!({ "action": "delete", "id": id }))
            .await
            .unwrap();
        assert_eq!(deleted.get("removed").and_then(|v| v.as_str()), Some(id));
    }
}
