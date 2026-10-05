//! Agent-facing plan registry tools.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use superglue::tools::{
    OnToolError, Tool, ToolContextPolicy, ToolInvokeError, ToolRegistry, ToolRetryPolicy, ToolSpec,
};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use super::note_ref::note_ref_for_id;
use super::plan_ref::resolve_plan_id;
use super::registry::{PlanFilter, PlanRegistry};
use super::task_ref::{resolve_task_id, task_ref_for_id, task_ref_for_task};
use crate::plan::{
    KeyFile, PlanDraft, PlanDraftTask, PlanId, PlanStatus, TaskAnchor, TaskId, TaskStatus,
};

pub const PLAN_TOOL_NAME: &str = "plan";

/// Plan tools return errors to the model instead of aborting the chat loop.
fn plan_tool_policy() -> ToolRetryPolicy {
    ToolRetryPolicy {
        on_error: OnToolError::Skip,
    }
}

async fn register_plan_tool(
    registry: &ToolRegistry,
    tool: Arc<dyn Tool>,
) -> Result<(), ToolInvokeError> {
    registry
        .register_with_policy(tool, plan_tool_policy())
        .await
}

/// Callback invoked after a plan is committed (e.g. to emit UI events).
pub type PlanCommitHook = Arc<dyn Fn(PlanId) + Send + Sync>;

/// Async gate for mutating plan text (approve/deny before apply).
pub type PlanMutateApproveFn = Arc<
    dyn Fn(String, String) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>
        + Send
        + Sync,
>;

/// Shared state for plan registry agent tools.
pub struct PlanToolState {
    registry: Arc<RwLock<PlanRegistry>>,
    persist_path: PathBuf,
    session_id: Option<Uuid>,
    on_commit: Option<PlanCommitHook>,
    on_update: Option<PlanCommitHook>,
    /// When set, `replace_excerpt` asks this gate before mutating.
    on_mutate_approve: Option<PlanMutateApproveFn>,
    /// Set when `plan` commit succeeds during the current turn (cleared by host).
    pub committed_plan: Arc<RwLock<Option<PlanId>>>,
    get_cache: Arc<Mutex<HashMap<PlanId, (String, Value, u32)>>>,
}

impl PlanToolState {
    async fn reload_from_disk(&self) -> Result<(), ToolInvokeError> {
        let fresh = PlanRegistry::load(&self.persist_path).map_err(|e| {
            ToolInvokeError::handler(format!("failed to load plan registry: {e}"), None)
        })?;
        *self.registry.write().await = fresh;
        Ok(())
    }

    async fn save(&self) -> Result<(), ToolInvokeError> {
        let reg = self.registry.read().await;
        reg.save(&self.persist_path).map_err(|e| {
            ToolInvokeError::handler(format!("failed to save plan registry: {e}"), None)
        })
    }

    async fn notify_update(&self, plan_id: PlanId) {
        if let Some(hook) = &self.on_update {
            hook(plan_id);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanToolMode {
    /// Authoring surface from the old Plan/Ask modes. Same actions as [`Self::Agent`].
    ReadonlyCommit,
    /// Implementation surface. Same actions as [`Self::ReadonlyCommit`].
    Agent,
}

/// Register the full plan tool (list, get, commit, and mutations).
pub async fn register_plan_readonly_tools(
    registry: &ToolRegistry,
    state: Arc<PlanToolState>,
) -> Result<(), ToolInvokeError> {
    register_plan_tool(
        registry,
        Arc::new(PlanTool {
            state,
            mode: PlanToolMode::ReadonlyCommit,
        }),
    )
    .await
}

/// Register the full plan tool (same actions as [`register_plan_readonly_tools`]).
pub async fn register_plan_mutation_tools(
    registry: &ToolRegistry,
    state: Arc<PlanToolState>,
) -> Result<(), ToolInvokeError> {
    register_plan_tool(registry, plan_agent_tool(state)).await
}

/// Build the `plan` tool for a host that registers SuperGlue tools itself.
#[must_use]
pub fn plan_agent_tool(state: Arc<PlanToolState>) -> Arc<dyn Tool> {
    Arc::new(PlanTool {
        state,
        mode: PlanToolMode::Agent,
    })
}

/// Skip a failed `plan` call and continue the turn.
#[must_use]
pub fn plan_tool_retry_policy() -> ToolRetryPolicy {
    plan_tool_policy()
}

/// Build shared plan tool state.
#[must_use]
pub fn plan_registry_state(
    registry: Arc<RwLock<PlanRegistry>>,
    persist_path: PathBuf,
    session_id: Option<Uuid>,
    on_commit: Option<PlanCommitHook>,
    on_update: Option<PlanCommitHook>,
) -> Arc<PlanToolState> {
    plan_registry_state_with_approve(
        registry,
        persist_path,
        session_id,
        on_commit,
        on_update,
        None,
    )
}

/// Build shared plan tool state with an optional mutate-approval gate.
#[must_use]
pub fn plan_registry_state_with_approve(
    registry: Arc<RwLock<PlanRegistry>>,
    persist_path: PathBuf,
    session_id: Option<Uuid>,
    on_commit: Option<PlanCommitHook>,
    on_update: Option<PlanCommitHook>,
    on_mutate_approve: Option<PlanMutateApproveFn>,
) -> Arc<PlanToolState> {
    Arc::new(PlanToolState {
        registry,
        persist_path,
        session_id,
        on_commit,
        on_update,
        on_mutate_approve,
        committed_plan: Arc::new(RwLock::new(None)),
        get_cache: Arc::new(Mutex::new(HashMap::new())),
    })
}

pub struct PlanTool {
    state: Arc<PlanToolState>,
    mode: PlanToolMode,
}

fn tool_error(name: &str, msg: impl Into<String>) -> ToolInvokeError {
    ToolInvokeError::handler(msg, Some(name.to_string()))
}

fn parse_plan_id(
    name: &str,
    raw: &str,
    registry: &PlanRegistry,
) -> Result<PlanId, ToolInvokeError> {
    resolve_plan_id(registry, raw).map_err(|message| tool_error(name, message))
}

async fn plan_id_from_args(
    state: &PlanToolState,
    arguments: &Value,
) -> Result<PlanId, ToolInvokeError> {
    let raw = arguments
        .get("plan_id")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing plan_id"))?;
    let registry = state.registry.read().await;
    parse_plan_id(PLAN_TOOL_NAME, raw, &registry)
}

fn resolve_task_id_for_plan(
    name: &str,
    plan: &crate::plan::Plan,
    raw: &str,
) -> Result<TaskId, ToolInvokeError> {
    resolve_task_id(plan, raw).map_err(|e| tool_error(name, e))
}

fn parse_plan_status(raw: &str) -> Result<PlanStatus, ToolInvokeError> {
    match raw {
        "draft" => Ok(PlanStatus::Draft),
        "active" => Ok(PlanStatus::Active),
        "completed" => Ok(PlanStatus::Completed),
        "paused" => Ok(PlanStatus::Paused),
        "abandoned" => Ok(PlanStatus::Abandoned),
        other => Err(tool_error(
            PLAN_TOOL_NAME,
            format!("invalid plan status: {other}"),
        )),
    }
}

fn parse_task_status(raw: &str) -> Result<TaskStatus, ToolInvokeError> {
    match raw {
        "pending" => Ok(TaskStatus::Pending),
        // Models often confuse plan-level `active` with task `in_progress`.
        "in_progress" | "active" => Ok(TaskStatus::InProgress),
        "completed" => Ok(TaskStatus::Completed),
        "skipped" => Ok(TaskStatus::Skipped),
        other => Err(tool_error(
            PLAN_TOOL_NAME,
            format!(
                "invalid task status: {other} (use pending, in_progress, completed, or skipped)"
            ),
        )),
    }
}

fn parse_draft(arguments: &Value) -> Result<PlanDraft, ToolInvokeError> {
    let title = arguments
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing title"))?
        .trim()
        .to_string();
    if title.is_empty() {
        return Err(tool_error(PLAN_TOOL_NAME, "title must not be empty"));
    }
    let overview = arguments
        .get("overview")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing overview"))?
        .trim()
        .to_string();
    let key_files: Vec<KeyFile> = arguments
        .get("key_files")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    Some(KeyFile {
                        path: v.get("path")?.as_str()?.to_string(),
                        description: v.get("description")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let context: Vec<String> = arguments
        .get("context")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::trim).filter(|s| !s.is_empty()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let tasks: Vec<PlanDraftTask> = arguments
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing tasks array"))?
        .iter()
        .filter_map(|v| {
            let title = v.get("title")?.as_str()?.trim();
            if title.is_empty() {
                return None;
            }
            Some(PlanDraftTask {
                title: title.to_string(),
                description: v
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                files: v
                    .get("files")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                Some(TaskAnchor {
                                    path: item.get("path")?.as_str()?.to_string(),
                                    symbol_or_lines: item
                                        .get("symbol_or_lines")?
                                        .as_str()?
                                        .to_string(),
                                    change: item.get("change")?.as_str()?.to_string(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();
    if tasks.is_empty() {
        return Err(tool_error(
            PLAN_TOOL_NAME,
            "tasks must contain at least one item",
        ));
    }
    let verification: Vec<String> = arguments
        .get("verification")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let notes: Vec<String> = arguments
        .get("notes")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let published_output = arguments
        .get("published_output")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    Ok(PlanDraft {
        title,
        overview,
        key_files,
        context,
        tasks,
        verification,
        published_output,
        notes,
    })
}

fn plan_summary_json(plan: &crate::plan::Plan) -> Value {
    let p = plan.progress();
    json!({
        "id": plan.public_id,
        "public_id": plan.public_id,
        "title": plan.title,
        "status": plan.status,
        "progress": {
            "completed": p.completed,
            "total": p.total,
            "in_progress": p.in_progress,
        },
        "updated_at": format_time(plan.updated_at),
    })
}

fn plan_full_json(plan: &crate::plan::Plan) -> Value {
    json!({
        "id": plan.public_id,
        "public_id": plan.public_id,
        "title": plan.title,
        "overview": plan.overview,
        "status": plan.status,
        "key_files": plan.key_files,
        "context": plan.context,
        "tasks": plan.tasks.iter().map(|t| json!({
            "id": task_ref_for_task(t),
            "title": t.title,
            "description": t.description,
            "files": t.files,
            "status": t.status,
            "order": t.order,
            "completed_at": t.completed_at.map(format_time),
        })).collect::<Vec<_>>(),
        "verification": plan.verification,
        "published_output": plan.published_output,
        "notes": plan.notes.iter().map(|n| json!({
            "id": note_ref_for_id(plan, n.id),
            "content": n.content,
            "created_at": format_time(n.created_at),
            "task_id": n.task_id.and_then(|tid| task_ref_for_id(plan, tid)),
        })).collect::<Vec<_>>(),
        "created_at": format_time(plan.created_at),
        "updated_at": format_time(plan.updated_at),
        "completed_at": plan.completed_at.map(format_time),
    })
}

fn format_time(value: time::OffsetDateTime) -> String {
    value
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| value.to_string())
}

async fn plan_public_id(state: &PlanToolState, plan_id: PlanId) -> String {
    state
        .registry
        .read()
        .await
        .get(plan_id)
        .map(|plan| plan.public_id.clone())
        .unwrap_or_else(|| "unknown".to_string())
}

const PLAN_ACTIONS: &[&str] = &[
    "list",
    "get",
    "commit",
    "update_task",
    "add_note",
    "set_status",
    "append_overview",
    "replace_excerpt",
    "replace_overview",
];

fn plan_parameters_schema(_mode: PlanToolMode) -> Value {
    let actions = PLAN_ACTIONS;
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": actions,
                "description": "Plan registry action"
            },
            "title": { "type": "string", "description": "Short plan title (commit)" },
            "overview": { "type": "string", "description": "2-4 sentences on goal and approach (commit)" },
            "key_files": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "description": { "type": "string" }
                    },
                    "required": ["path", "description"]
                }
            },
            "context": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Cross-cutting investigation findings (invariants, gotchas, patterns) not tied to one task (commit)"
            },
            "tasks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "description": { "type": "string" },
                        "files": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string" },
                                    "symbol_or_lines": { "type": "string" },
                                    "change": { "type": "string" }
                                },
                                "required": ["path", "symbol_or_lines", "change"]
                            },
                            "description": "Concrete file/symbol anchors for this task (commit)"
                        }
                    },
                    "required": ["title"]
                },
                "description": "Ordered executable steps (commit)"
            },
            "verification": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Concrete checks to confirm success (commit)"
            },
            "published_output": {
                "type": "string",
                "description": "Exact form of the user-facing deliverable: units, scale, rounding, formatting, and expected value if known. Separate from working/intermediate math."
            },
            "notes": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional trade-offs or open questions (commit)"
            },
            "status": {
                "type": "string",
                "description": "Depends on action: \
                     set_status/list → plan status: draft|active|completed|paused|abandoned \
                     (omit or leave empty on list for all plans); \
                     update_task → task status: pending|in_progress|completed|skipped \
                     (do not use plan statuses like active for tasks)"
            },
            "limit": { "type": "integer", "description": "Max results (list)" },
            "plan_id": { "type": "string", "description": "Public plan id from plan list, for example AB12CD34. Legacy UUIDs are accepted." },
            "task_id": { "type": "string", "description": "Task ref from plan get, e.g. #TASK:ID:1#. Required for update_task; optional for add_note (omit for plan-wide notes)." },
            "note_id": { "type": "string", "description": "Note ref from plan get, e.g. #NOTE:ID:1#." },
            "content": { "type": "string", "description": "Note content (add_note) or overview text to append (append_overview)" },
            "old_text": { "type": "string", "description": "Exact excerpt to find in the overview (replace_excerpt)" },
            "new_text": { "type": "string", "description": "Replacement text for the excerpt (replace_excerpt)" },
            "overview": { "type": "string", "description": "Full replacement overview text (replace_overview)" }
        },
        "required": ["action"]
    })
}

fn plan_description(_mode: PlanToolMode) -> String {
    "Manage saved implementation plans. Actions: list, get, commit, update_task, add_note, set_status, append_overview, replace_excerpt, replace_overview. \
     Use the short public plan_id returned by list; do not invent or expose canonical UUIDs. \
     Call commit after investigation with title, overview, key_files, context, tasks (each task should include files anchors: path, symbol_or_lines, change), verification, and published_output \
     (required when the user specifies output format or units). \
     Task refs in plan get are short refs like #TASK:ID:1# — pass those as task_id to update_task and add_note. Note refs use #NOTE:ID:1#. \
     Use update_task to mark tasks completed when done — batch multiple completions in one call round when possible; \
     use in_progress only for long-running tasks. Task statuses: pending|in_progress|completed|skipped (never plan status active). \
     Use set_status for plan-level draft|active|completed|paused|abandoned. \
     For full overview rewrites use replace_overview (overview=<entire new overview>). It prompts the user with a diff to approve. \
     For spot edits to existing overview text use replace_excerpt (old_text + new_text). It also prompts for approval. \
     Never use add_note or append_overview as a substitute for rewriting existing overview text. \
     Use append_overview only to add brand-new overview material at the end. \
     Use add_note for supplementary plan-linked notes."
        .to_string()
}

async fn plan_commit(state: &PlanToolState, arguments: &Value) -> Result<Value, ToolInvokeError> {
    let draft = parse_draft(arguments)?;
    let plan_id = {
        let mut reg = state.registry.write().await;
        reg.commit(draft, state.session_id)
    };
    state.save().await?;
    *state.committed_plan.write().await = Some(plan_id);
    if let Some(hook) = &state.on_commit {
        hook(plan_id);
    }
    Ok(json!({
        "plan_id": plan_public_id(state, plan_id).await,
        "message": "plan committed to registry",
    }))
}

async fn plan_list(state: &PlanToolState, arguments: &Value) -> Result<Value, ToolInvokeError> {
    state.reload_from_disk().await?;
    // Models often pass `"status": ""` on list; treat blank as no filter.
    let status = match arguments
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
    {
        None | Some("") => None,
        Some(raw) => Some(parse_plan_status(raw)?),
    };
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    let filter = PlanFilter {
        status,
        limit,
        ..Default::default()
    };
    let reg = state.registry.read().await;
    let plans: Vec<Value> = reg
        .list(&filter)
        .iter()
        .map(|p| plan_summary_json(p))
        .collect();
    Ok(json!({ "count": plans.len(), "plans": plans }))
}

async fn plan_get(state: &PlanToolState, arguments: &Value) -> Result<Value, ToolInvokeError> {
    state.reload_from_disk().await?;
    let raw = arguments
        .get("plan_id")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing plan_id"))?;
    let plan_id = {
        let registry = state.registry.read().await;
        parse_plan_id(PLAN_TOOL_NAME, raw, &registry)?
    };
    let reg = state.registry.read().await;
    let plan = reg
        .get(plan_id)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, format!("plan not found: {raw}")))?;
    let updated_at = format_time(plan.updated_at);
    let mut cache = state.get_cache.lock().await;
    if let Some((cached_updated_at, _, get_count)) = cache.get_mut(&plan_id)
        && cached_updated_at == &updated_at
    {
        *get_count += 1;
        if *get_count >= 3 {
            return Ok(json!({
                "plan_id": plan.public_id,
                "unchanged": true,
                "skipped": true,
                "updated_at": updated_at,
                "message": "plan is already in context; implement the next pending task",
            }));
        }
        return Ok(json!({
            "plan_id": plan.public_id,
            "unchanged": true,
            "updated_at": updated_at,
            "message": "plan unchanged since the previous get; use the previous plan content",
        }));
    }
    let full = plan_full_json(plan);
    cache.insert(plan_id, (updated_at, full.clone(), 1));
    Ok(full)
}

async fn plan_update_task(
    state: &PlanToolState,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let plan_id = plan_id_from_args(state, arguments).await?;
    let raw_task_id = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing task_id"))?;
    let status = parse_task_status(
        arguments
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing status"))?,
    )?;
    let task_id = {
        let reg = state.registry.read().await;
        let plan = reg
            .get(plan_id)
            .ok_or_else(|| tool_error(PLAN_TOOL_NAME, format!("plan not found: {plan_id}")))?;
        resolve_task_id_for_plan(PLAN_TOOL_NAME, plan, raw_task_id)?
    };
    let task_ref = {
        let reg = state.registry.read().await;
        task_ref_for_id(reg.get(plan_id).expect("plan"), task_id)
            .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "task not found after update"))?
    };
    {
        let mut reg = state.registry.write().await;
        reg.update_task_status(plan_id, task_id, status)
            .map_err(|e| tool_error(PLAN_TOOL_NAME, e.to_string()))?;
    }
    state.save().await?;
    state.notify_update(plan_id).await;
    Ok(json!({
        "plan_id": plan_public_id(state, plan_id).await,
        "task_id": task_ref,
        "status": status,
        "message": "task updated",
    }))
}

async fn plan_append_overview(
    state: &PlanToolState,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let plan_id = plan_id_from_args(state, arguments).await?;
    let content = arguments
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing content"))?
        .to_string();
    {
        let mut reg = state.registry.write().await;
        reg.append_overview(plan_id, content)
            .map_err(|e| tool_error(PLAN_TOOL_NAME, e.to_string()))?;
    }
    state.save().await?;
    state.notify_update(plan_id).await;
    Ok(json!({
        "plan_id": plan_public_id(state, plan_id).await,
        "message": "overview appended",
    }))
}

fn format_excerpt_diff(old_text: &str, new_text: &str) -> String {
    let mut out = String::from("--- excerpt\n");
    for line in old_text.lines() {
        out.push('-');
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("+++ replacement\n");
    for line in new_text.lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn format_overview_diff(old_text: &str, new_text: &str) -> String {
    let mut out = String::from("--- overview\n");
    for line in old_text.lines() {
        out.push('-');
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("+++ overview\n");
    for line in new_text.lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

async fn plan_replace_excerpt(
    state: &PlanToolState,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let plan_id = plan_id_from_args(state, arguments).await?;
    let old_text = arguments
        .get("old_text")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing old_text"))?
        .to_string();
    let new_text = arguments
        .get("new_text")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing new_text"))?
        .to_string();

    if let Some(gate) = &state.on_mutate_approve {
        let preview = format_excerpt_diff(&old_text, &new_text);
        let reason = format!("Replace excerpt in plan {plan_id}");
        if !gate(preview, reason).await {
            return Ok(json!({
                "ok": false,
                "status": "denied",
                "plan_id": plan_public_id(state, plan_id).await,
                "message": "replace_excerpt denied by user",
            }));
        }
    }

    {
        let mut reg = state.registry.write().await;
        reg.replace_excerpt(plan_id, &old_text, &new_text)
            .map_err(|e| tool_error(PLAN_TOOL_NAME, e.to_string()))?;
    }
    state.save().await?;
    state.notify_update(plan_id).await;
    Ok(json!({
        "ok": true,
        "plan_id": plan_public_id(state, plan_id).await,
        "message": "excerpt replaced",
    }))
}

async fn plan_replace_overview(
    state: &PlanToolState,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let plan_id = plan_id_from_args(state, arguments).await?;
    let new_overview = arguments
        .get("overview")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing overview"))?
        .to_string();

    let old_overview = {
        let reg = state.registry.read().await;
        reg.get(plan_id)
            .map(|p| p.overview.clone())
            .ok_or_else(|| tool_error(PLAN_TOOL_NAME, format!("plan not found: {plan_id}")))?
    };

    if let Some(gate) = &state.on_mutate_approve {
        let preview = format_overview_diff(&old_overview, &new_overview);
        let reason = format!("Replace overview in plan {plan_id}");
        if !gate(preview, reason).await {
            return Ok(json!({
                "ok": false,
                "status": "denied",
                "plan_id": plan_public_id(state, plan_id).await,
                "message": "replace_overview denied by user",
            }));
        }
    }

    {
        let mut reg = state.registry.write().await;
        reg.replace_overview(plan_id, new_overview)
            .map_err(|e| tool_error(PLAN_TOOL_NAME, e.to_string()))?;
    }
    state.save().await?;
    state.notify_update(plan_id).await;
    Ok(json!({
        "ok": true,
        "plan_id": plan_public_id(state, plan_id).await,
        "message": "overview replaced",
    }))
}

async fn plan_add_note(state: &PlanToolState, arguments: &Value) -> Result<Value, ToolInvokeError> {
    let plan_id = plan_id_from_args(state, arguments).await?;
    let content = arguments
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing content"))?
        .to_string();
    let task_id = match arguments
        .get("task_id")
        .and_then(Value::as_str)
        .map(str::trim)
    {
        None | Some("") => None,
        Some(raw) => {
            let reg = state.registry.read().await;
            let plan = reg
                .get(plan_id)
                .ok_or_else(|| tool_error(PLAN_TOOL_NAME, format!("plan not found: {plan_id}")))?;
            Some(resolve_task_id_for_plan(PLAN_TOOL_NAME, plan, raw)?)
        }
    };
    let note_id = {
        let mut reg = state.registry.write().await;
        reg.add_note(plan_id, content, task_id)
            .map_err(|e| tool_error(PLAN_TOOL_NAME, e.to_string()))?
    };
    state.save().await?;
    state.notify_update(plan_id).await;
    let note_ref = {
        let registry = state.registry.read().await;
        registry
            .get(plan_id)
            .and_then(|plan| note_ref_for_id(plan, note_id))
            .unwrap_or_else(|| "unknown".to_string())
    };
    Ok(json!({
        "plan_id": plan_public_id(state, plan_id).await,
        "note_id": note_ref,
        "message": "note added",
    }))
}

async fn plan_set_status(
    state: &PlanToolState,
    arguments: &Value,
) -> Result<Value, ToolInvokeError> {
    let plan_id = plan_id_from_args(state, arguments).await?;
    let status = parse_plan_status(
        arguments
            .get("status")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing status"))?,
    )?;
    {
        let mut reg = state.registry.write().await;
        reg.set_plan_status(plan_id, status)
            .map_err(|e| tool_error(PLAN_TOOL_NAME, e.to_string()))?;
    }
    state.save().await?;
    state.notify_update(plan_id).await;
    Ok(json!({
        "plan_id": plan_public_id(state, plan_id).await,
        "status": status,
        "message": "plan status updated",
    }))
}

#[async_trait]
impl Tool for PlanTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: PLAN_TOOL_NAME.to_string(),
            description: Some(plan_description(self.mode)),
            static_tool: false,
            parameters_schema: plan_parameters_schema(self.mode),
        }
    }

    fn context_policy(&self) -> ToolContextPolicy {
        ToolContextPolicy::Inline
    }

    async fn call(&self, arguments: Value) -> Result<Value, ToolInvokeError> {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error(PLAN_TOOL_NAME, "missing action"))?;
        match action {
            "list" => plan_list(&self.state, &arguments).await,
            "get" => plan_get(&self.state, &arguments).await,
            "commit" => plan_commit(&self.state, &arguments).await,
            "add_note" => plan_add_note(&self.state, &arguments).await,
            "append_overview" => plan_append_overview(&self.state, &arguments).await,
            "replace_excerpt" => plan_replace_excerpt(&self.state, &arguments).await,
            "replace_overview" => plan_replace_overview(&self.state, &arguments).await,
            "update_task" => plan_update_task(&self.state, &arguments).await,
            "set_status" => plan_set_status(&self.state, &arguments).await,
            other => Err(tool_error(
                PLAN_TOOL_NAME,
                format!("unknown action '{other}'"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use superglue::tools::{OnToolError, Tool, ToolRegistry};
    use tempfile::TempDir;

    fn test_state(tmp: &TempDir) -> Arc<PlanToolState> {
        let path = tmp.path().join("registry.json");
        plan_registry_state(
            Arc::new(RwLock::new(PlanRegistry::new())),
            path,
            None,
            None,
            None,
        )
    }

    fn plan_tool(state: Arc<PlanToolState>, mode: PlanToolMode) -> PlanTool {
        PlanTool { state, mode }
    }

    #[tokio::test]
    async fn plan_agent_tool_uses_plan_name() {
        let tmp = TempDir::new().unwrap();
        let tool = plan_agent_tool(test_state(&tmp));
        assert_eq!(tool.spec().name, PLAN_TOOL_NAME);
    }

    #[tokio::test]
    async fn agent_mode_can_commit() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = plan_tool(Arc::clone(&state), PlanToolMode::Agent);
        let out = tool
            .call(json!({
                "action": "commit",
                "title": "Agent authored",
                "overview": "Commit from the unified tool surface",
                "tasks": [{ "title": "Step 1" }],
                "verification": ["ok"],
                "published_output": "Standard code deliverable"
            }))
            .await
            .unwrap();
        let plan_id = out.get("plan_id").and_then(|v| v.as_str()).unwrap();
        let full = tool
            .call(json!({ "action": "get", "plan_id": plan_id }))
            .await
            .unwrap();
        assert_eq!(
            full.get("title").and_then(|v| v.as_str()),
            Some("Agent authored")
        );
    }

    #[tokio::test]
    async fn readonly_mode_can_update_task() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let out = tool
            .call(json!({
                "action": "commit",
                "title": "A",
                "overview": "O",
                "tasks": [{ "title": "Step" }],
                "verification": ["ok"]
            }))
            .await
            .unwrap();
        let plan_id = out["plan_id"].as_str().unwrap();
        let updated = tool
            .call(json!({
                "action": "update_task",
                "plan_id": plan_id,
                "task_id": "#TASK:ID:1#",
                "status": "completed"
            }))
            .await
            .unwrap();
        assert_eq!(updated["status"], "completed");
    }

    #[tokio::test]
    async fn commit_and_get_round_trip() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let out = tool
            .call(json!({
                "action": "commit",
                "title": "My plan",
                "overview": "Do stuff",
                "tasks": [{ "title": "Step 1" }],
                "verification": ["cargo test"],
                "published_output": "Standard code deliverable"
            }))
            .await
            .unwrap();
        let plan_id = out.get("plan_id").unwrap().as_str().unwrap();
        let full = tool
            .call(json!({ "action": "get", "plan_id": plan_id }))
            .await
            .unwrap();
        assert_eq!(full.get("title").and_then(|v| v.as_str()), Some("My plan"));
        assert_eq!(
            full.get("published_output").and_then(|v| v.as_str()),
            Some("Standard code deliverable")
        );
    }

    #[tokio::test]
    async fn full_commit_registry_render_checkoff_complete_flow() {
        use crate::render_plan_text;

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let state = plan_registry_state(
            Arc::new(RwLock::new(PlanRegistry::new())),
            path.clone(),
            None,
            None,
            None,
        );
        let commit_tool = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let agent_tool = plan_tool(Arc::clone(&state), PlanToolMode::Agent);

        let out = commit_tool
            .call(json!({
                "action": "commit",
                "title": "Integration plan",
                "overview": "End-to-end test",
                "key_files": [{ "path": "src/lib.rs", "description": "entry" }],
                "tasks": [
                    { "title": "Step one", "description": "first" },
                    { "title": "Step two" }
                ],
                "verification": ["cargo test"],
                "notes": ["trade-off noted at commit"]
            }))
            .await
            .unwrap();
        let plan_id_str = out.get("plan_id").unwrap().as_str().unwrap();

        let loaded = PlanRegistry::load(&path).unwrap();
        let plan_id = crate::resolve_plan_id(&loaded, plan_id_str).unwrap();
        let plan = loaded.get(plan_id).unwrap();
        let pending = render_plan_text(plan);
        assert!(pending.contains("Integration plan"));
        assert!(pending.contains("- [ ] `#TASK:ID:1#` **Step one**"));
        assert!(pending.contains("## Key files"));

        let full = agent_tool
            .call(json!({ "action": "get", "plan_id": plan_id_str }))
            .await
            .unwrap();
        let task_id = full["tasks"][0]["id"].as_str().unwrap();

        agent_tool
            .call(json!({
                "action": "update_task",
                "plan_id": plan_id_str,
                "task_id": task_id,
                "status": "in_progress"
            }))
            .await
            .unwrap();
        // Plan-level `active` is a common model mix-up; accept as in_progress.
        let alias = agent_tool
            .call(json!({
                "action": "update_task",
                "plan_id": plan_id_str,
                "task_id": full["tasks"][1]["id"].as_str().unwrap(),
                "status": "active"
            }))
            .await
            .unwrap();
        assert_eq!(alias["status"], "in_progress");
        agent_tool
            .call(json!({
                "action": "update_task",
                "plan_id": plan_id_str,
                "task_id": task_id,
                "status": "completed"
            }))
            .await
            .unwrap();
        agent_tool
            .call(json!({
                "action": "add_note",
                "plan_id": plan_id_str,
                "content": "step one done",
                "task_id": task_id
            }))
            .await
            .unwrap();

        let reg = state.registry.read().await;
        let mid = render_plan_text(reg.get(plan_id).unwrap());
        assert!(mid.contains("- [x] `#TASK:ID:1#` **Step one**"));
        assert!(mid.contains("- [ ] `#TASK:ID:2#` **Step two** **(in progress)**"));
        assert!(mid.contains("step one done"));
        drop(reg);

        agent_tool
            .call(json!({
                "action": "set_status",
                "plan_id": plan_id_str,
                "status": "completed"
            }))
            .await
            .unwrap();

        let list_out = agent_tool
            .call(json!({ "action": "list", "status": "completed" }))
            .await
            .unwrap();
        let plans = list_out.get("plans").and_then(|v| v.as_array()).unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].get("title").and_then(|v| v.as_str()),
            Some("Integration plan")
        );
    }

    #[tokio::test]
    async fn list_treats_empty_status_as_no_filter() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        tool.call(json!({
            "action": "commit",
            "title": "A",
            "overview": "O",
            "tasks": [{ "title": "T" }],
            "verification": ["ok"]
        }))
        .await
        .unwrap();

        let out = tool
            .call(json!({ "action": "list", "status": "" }))
            .await
            .expect("empty status must not fail list");
        assert_eq!(out["count"], 1);
    }

    #[tokio::test]
    async fn readonly_mode_can_add_note() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let out = tool
            .call(json!({
                "action": "commit",
                "title": "A",
                "overview": "O",
                "tasks": [{ "title": "T" }],
                "verification": ["ok"]
            }))
            .await
            .unwrap();
        let plan_id = out["plan_id"].as_str().unwrap();
        let note = tool
            .call(json!({
                "action": "add_note",
                "plan_id": plan_id,
                "content": "more instructions"
            }))
            .await
            .unwrap();
        assert!(note["note_id"].as_str().is_some());
    }

    #[tokio::test]
    async fn add_note_treats_empty_task_id_as_omitted() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let out = tool
            .call(json!({
                "action": "commit",
                "title": "A",
                "overview": "O",
                "tasks": [{ "title": "T" }],
                "verification": ["ok"]
            }))
            .await
            .unwrap();
        let plan_id = out["plan_id"].as_str().unwrap();
        let note = tool
            .call(json!({
                "action": "add_note",
                "plan_id": plan_id,
                "task_id": "",
                "content": "plan-wide correction"
            }))
            .await
            .expect("empty task_id must not fail add_note");
        assert!(note["note_id"].as_str().is_some());
    }

    #[tokio::test]
    async fn get_includes_task_ref() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let out = tool
            .call(json!({
                "action": "commit",
                "title": "A",
                "overview": "O",
                "tasks": [{ "title": "Step" }],
                "verification": ["ok"]
            }))
            .await
            .unwrap();
        let plan_id = out["plan_id"].as_str().unwrap();
        let full = tool
            .call(json!({ "action": "get", "plan_id": plan_id }))
            .await
            .unwrap();
        assert_eq!(full["tasks"][0]["id"], "#TASK:ID:1#");
        assert!(full["tasks"][0].get("ref").is_none());
    }

    #[tokio::test]
    async fn repeated_get_is_bounded() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let out = tool
            .call(json!({
                "action": "commit",
                "title": "A",
                "overview": "O",
                "tasks": [{ "title": "Step" }],
                "notes": ["A note"]
            }))
            .await
            .unwrap();
        let plan_id = out["plan_id"].as_str().unwrap();

        let first = tool
            .call(json!({ "action": "get", "plan_id": plan_id }))
            .await
            .unwrap();
        assert!(first.get("tasks").is_some());
        assert_eq!(first["notes"][0]["id"], "#NOTE:ID:1#");

        let second = tool
            .call(json!({ "action": "get", "plan_id": plan_id }))
            .await
            .unwrap();
        assert_eq!(second["unchanged"], true);
        assert_eq!(second["skipped"], Value::Null);

        let third = tool
            .call(json!({ "action": "get", "plan_id": plan_id }))
            .await
            .unwrap();
        assert_eq!(third["skipped"], true);
        assert!(third.get("tasks").is_none());
    }

    #[tokio::test]
    async fn update_task_accepts_short_ref() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let commit = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let agent = plan_tool(Arc::clone(&state), PlanToolMode::Agent);
        let out = commit
            .call(json!({
                "action": "commit",
                "title": "A",
                "overview": "O",
                "tasks": [{ "title": "Step" }],
                "verification": ["ok"]
            }))
            .await
            .unwrap();
        let plan_id = out["plan_id"].as_str().unwrap();
        let updated = agent
            .call(json!({
                "action": "update_task",
                "plan_id": plan_id,
                "task_id": "#TASK:ID:1#",
                "status": "completed"
            }))
            .await
            .unwrap();
        assert_eq!(updated["task_id"], "#TASK:ID:1#");
        assert_eq!(updated["status"], "completed");
    }

    #[tokio::test]
    async fn invalid_task_id_lists_short_refs() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let commit = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let agent = plan_tool(Arc::clone(&state), PlanToolMode::Agent);
        let out = commit
            .call(json!({
                "action": "commit",
                "title": "A",
                "overview": "O",
                "tasks": [{ "title": "Step one" }, { "title": "Step two" }],
                "verification": ["ok"]
            }))
            .await
            .unwrap();
        let plan_id = out["plan_id"].as_str().unwrap();
        let err = agent
            .call(json!({
                "action": "update_task",
                "plan_id": plan_id,
                "task_id": "0d?",
                "status": "completed"
            }))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid task id: 0d?"));
        assert!(err.contains("#TASK:ID:1# Step one"));
        assert!(err.contains("#TASK:ID:2# Step two"));
    }

    #[tokio::test]
    async fn replace_overview_rewrites_full_overview() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let tool = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let out = tool
            .call(json!({
                "action": "commit",
                "title": "A",
                "overview": "Original overview",
                "tasks": [{ "title": "T" }],
                "verification": ["ok"]
            }))
            .await
            .unwrap();
        let plan_id = out["plan_id"].as_str().unwrap();
        let replaced = tool
            .call(json!({
                "action": "replace_overview",
                "plan_id": plan_id,
                "overview": "Brand new overview.\n\nSecond paragraph."
            }))
            .await
            .unwrap();
        assert_eq!(replaced["ok"], true);
        let full = tool
            .call(json!({ "action": "get", "plan_id": plan_id }))
            .await
            .unwrap();
        assert_eq!(
            full["overview"].as_str(),
            Some("Brand new overview.\n\nSecond paragraph.")
        );
    }

    #[tokio::test]
    async fn replace_overview_denied_when_gate_rejects() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let state = plan_registry_state_with_approve(
            Arc::new(RwLock::new(PlanRegistry::new())),
            path,
            None,
            None,
            None,
            Some(Arc::new(|preview, _reason| {
                assert!(preview.contains("--- overview"));
                assert!(preview.contains("+++ overview"));
                Box::pin(async move { false })
            })),
        );
        let tool = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let out = tool
            .call(json!({
                "action": "commit",
                "title": "A",
                "overview": "Keep me",
                "tasks": [{ "title": "T" }],
                "verification": ["ok"]
            }))
            .await
            .unwrap();
        let plan_id = out["plan_id"].as_str().unwrap();
        let denied = tool
            .call(json!({
                "action": "replace_overview",
                "plan_id": plan_id,
                "overview": "Should not apply"
            }))
            .await
            .unwrap();
        assert_eq!(denied["status"], "denied");
        let full = tool
            .call(json!({ "action": "get", "plan_id": plan_id }))
            .await
            .unwrap();
        assert_eq!(full["overview"].as_str(), Some("Keep me"));
    }

    #[tokio::test]
    async fn plan_tool_registers_skip_policy() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let registry = ToolRegistry::new();
        register_plan_mutation_tools(&registry, state)
            .await
            .unwrap();
        let policy = registry.policy_for(PLAN_TOOL_NAME).await;
        assert_eq!(policy.on_error, OnToolError::Skip);
    }

    #[tokio::test]
    async fn skip_policy_softens_invalid_task_id() {
        let tmp = TempDir::new().unwrap();
        let state = test_state(&tmp);
        let commit = plan_tool(Arc::clone(&state), PlanToolMode::ReadonlyCommit);
        let registry = ToolRegistry::new();
        register_plan_mutation_tools(&registry, Arc::clone(&state))
            .await
            .unwrap();
        let out = commit
            .call(json!({
                "action": "commit",
                "title": "A",
                "overview": "O",
                "tasks": [{ "title": "Step one" }],
                "verification": ["ok"]
            }))
            .await
            .unwrap();
        let plan_id = out["plan_id"].as_str().unwrap();
        let result = registry
            .invoke(
                PLAN_TOOL_NAME,
                json!({
                    "action": "update_task",
                    "plan_id": plan_id,
                    "task_id": "0d?",
                    "status": "completed"
                }),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(result.contains("invalid task id: 0d?"));

        let (tool, policy) = registry.resolve_invocation(PLAN_TOOL_NAME).await.unwrap();
        let handler_err = tool
            .call(json!({
                "action": "update_task",
                "plan_id": plan_id,
                "task_id": "0d?",
                "status": "completed"
            }))
            .await
            .unwrap_err();
        let ToolInvokeError::HandlerFailed { message, .. } = handler_err else {
            panic!("expected handler failure");
        };
        assert!(message.contains("#TASK:ID:1#"));
        assert!(matches!(policy.on_error, OnToolError::Skip));
    }
}
