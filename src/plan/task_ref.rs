//! LLM-friendly short task refs (`#TASK:ID:N#`) mapped to internal UUIDs.

use std::collections::HashMap;

use uuid::Uuid;

use crate::plan::{Plan, PlanTask, TaskId};

const TASK_REF_PREFIX: &str = "#TASK:ID:";
const TASK_REF_SUFFIX: &str = "#";

/// Format a 1-based task index as `#TASK:ID:N#`.
#[must_use]
pub fn task_ref(order: usize) -> String {
    format!("{TASK_REF_PREFIX}{}{TASK_REF_SUFFIX}", order + 1)
}

/// Parse `#TASK:ID:N#` into 1-based index `N`.
#[must_use]
pub fn parse_task_ref(raw: &str) -> Option<usize> {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix(TASK_REF_PREFIX)?
        .strip_suffix(TASK_REF_SUFFIX)?;
    inner.parse().ok().filter(|n: &usize| *n >= 1)
}

/// Build `N -> TaskId` from plan tasks keyed by `order + 1`.
#[must_use]
pub fn task_ref_map(plan: &Plan) -> HashMap<usize, TaskId> {
    plan.tasks.iter().map(|t| (t.order + 1, t.id)).collect()
}

/// Short ref for a task on the plan, if present.
#[must_use]
pub fn task_ref_for_task(task: &PlanTask) -> String {
    task_ref(task.order)
}

/// Short ref for a task id on the plan, if present.
#[must_use]
pub fn task_ref_for_id(plan: &Plan, task_id: TaskId) -> Option<String> {
    plan.tasks
        .iter()
        .find(|t| t.id == task_id)
        .map(task_ref_for_task)
}

/// List every task as `#TASK:ID:N# — title` for error recovery.
#[must_use]
pub fn format_task_ref_list(plan: &Plan) -> String {
    plan.tasks
        .iter()
        .map(|t| format!("{} {}", task_ref_for_task(t), t.title))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Resolve a task id from short ref or UUID string against a plan.
pub fn resolve_task_id(plan: &Plan, raw: &str) -> Result<TaskId, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("missing task_id".into());
    }

    if let Some(index) = parse_task_ref(trimmed) {
        let map = task_ref_map(plan);
        return map.get(&index).copied().ok_or_else(|| {
            format!(
                "unknown task ref {trimmed}; use one of: {}",
                format_task_ref_list(plan)
            )
        });
    }

    if let Ok(uuid) = trimmed.parse::<Uuid>() {
        let task_id = TaskId(uuid);
        if plan.tasks.iter().any(|t| t.id == task_id) {
            return Ok(task_id);
        }
        return Err(format!(
            "task not found on plan: {trimmed}; use one of: {}",
            format_task_ref_list(plan)
        ));
    }

    Err(format!(
        "invalid task id: {trimmed}; use one of: {}",
        format_task_ref_list(plan)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{PlanDraft, PlanDraftTask};

    fn sample_plan() -> Plan {
        PlanDraft {
            title: "T".into(),
            overview: "O".into(),
            key_files: vec![],
            context: vec![],
            tasks: vec![
                PlanDraftTask {
                    title: "First".into(),
                    description: None,
                    files: vec![],
                },
                PlanDraftTask {
                    title: "Second".into(),
                    description: None,
                    files: vec![],
                },
            ],
            verification: vec![],
            published_output: String::new(),
            notes: vec![],
        }
        .into_plan(None)
    }

    #[test]
    fn task_ref_roundtrip() {
        assert_eq!(task_ref(0), "#TASK:ID:1#");
        assert_eq!(parse_task_ref("#TASK:ID:2#"), Some(2));
        assert_eq!(parse_task_ref("  #TASK:ID:1#  "), Some(1));
        assert!(parse_task_ref("0d?").is_none());
    }

    #[test]
    fn resolve_short_ref_and_uuid() {
        let plan = sample_plan();
        let first = resolve_task_id(&plan, "#TASK:ID:1#").unwrap();
        assert_eq!(first, plan.tasks[0].id);
        assert_eq!(
            resolve_task_id(&plan, &plan.tasks[1].id.to_string()).unwrap(),
            plan.tasks[1].id
        );
    }

    #[test]
    fn resolve_invalid_lists_refs() {
        let plan = sample_plan();
        let err = resolve_task_id(&plan, "0d?").unwrap_err();
        assert!(err.contains("#TASK:ID:1# First"));
        assert!(err.contains("#TASK:ID:2# Second"));
    }
}
