use crate::plan::{Plan, TaskStatus};
use crate::plan_task_ref::{task_ref_for_id, task_ref_for_task};

/// Markdown layout for chat, logging, and the plan browser.
#[must_use]
pub fn render_plan_text(plan: &Plan) -> String {
    let progress = plan.progress();
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", plan.title));
    out.push_str(&format!(
        "**Status:** `{}` · **Progress:** {}/{} tasks\n\n",
        plan.status.label(),
        progress.completed,
        progress.total
    ));
    out.push_str("## Overview\n\n");
    for line in plan.overview.lines() {
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    if !plan.published_output.is_empty() {
        out.push_str("## Published output\n\n");
        for line in plan.published_output.lines() {
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    if !plan.key_files.is_empty() {
        out.push_str("## Key files\n\n");
        for kf in &plan.key_files {
            out.push_str(&format!("- `{}` — {}\n", kf.path, kf.description));
        }
        out.push('\n');
    }
    if !plan.context.is_empty() {
        out.push_str("## Context\n\n");
        for item in &plan.context {
            out.push_str(&format!("- {item}\n"));
        }
        out.push('\n');
    }
    if !plan.tasks.is_empty() {
        out.push_str("## Tasks\n\n");
        for task in &plan.tasks {
            let checkbox = if matches!(task.status, TaskStatus::Completed | TaskStatus::Skipped) {
                "x"
            } else {
                " "
            };
            let status = match task.status {
                TaskStatus::InProgress => " **(in progress)**",
                TaskStatus::Skipped => " **(skipped)**",
                _ => "",
            };
            out.push_str(&format!(
                "- [{checkbox}] `{}` **{}**{status}",
                task_ref_for_task(task),
                task.title
            ));
            if let Some(desc) = &task.description {
                out.push_str(&format!(" — {desc}"));
            }
            out.push('\n');
            for anchor in &task.files {
                out.push_str(&format!(
                    "  - `{}` · `{}` — {}\n",
                    anchor.path, anchor.symbol_or_lines, anchor.change
                ));
            }
        }
        out.push('\n');
    }
    if !plan.verification.is_empty() {
        out.push_str("## Verification\n\n");
        for v in &plan.verification {
            out.push_str(&format!("- {v}\n"));
        }
        out.push('\n');
    }
    if !plan.notes.is_empty() {
        out.push_str("## Notes\n\n");
        for note in &plan.notes {
            let ts = note
                .created_at
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| note.created_at.to_string());
            out.push_str(&format!("- `{ts}` — {}", note.content));
            if let Some(tid) = note.task_id {
                if let Some(task_ref) = task_ref_for_id(plan, tid) {
                    out.push_str(&format!(" (`{task_ref}`)"));
                }
            }
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

#[must_use]
pub fn task_status_icon(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Completed => "[✓]",
        TaskStatus::InProgress => "[●]",
        TaskStatus::Pending => "[ ]",
        TaskStatus::Skipped => "[–]",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{PlanDraft, PlanDraftTask, TaskAnchor};

    #[test]
    fn render_includes_sections() {
        let plan = PlanDraft {
            title: "Site plan".into(),
            overview: "Build it".into(),
            key_files: vec![],
            context: vec![],
            tasks: vec![PlanDraftTask {
                title: "Add HTML".into(),
                description: None,
                files: vec![],
            }],
            verification: vec!["open browser".into()],
            published_output: String::new(),
            notes: vec![],
        }
        .into_plan(None);
        let text = render_plan_text(&plan);
        assert!(text.contains("Site plan"));
        assert!(text.contains("## Overview"));
        assert!(text.contains("## Tasks"));
        assert!(text.contains("- [ ] `#TASK:ID:1#` **Add HTML**"));
        assert!(text.contains("## Verification"));
        assert!(!text.contains("## Published output"));
    }

    #[test]
    fn render_includes_task_anchors_and_context() {
        let plan = PlanDraft {
            title: "Site plan".into(),
            overview: "Build it".into(),
            key_files: vec![],
            context: vec!["Follow existing layout patterns".into()],
            tasks: vec![PlanDraftTask {
                title: "Add HTML".into(),
                description: None,
                files: vec![TaskAnchor {
                    path: "site/index.html".into(),
                    symbol_or_lines: "new file".into(),
                    change: "create landing page".into(),
                }],
            }],
            verification: vec!["open browser".into()],
            published_output: String::new(),
            notes: vec![],
        }
        .into_plan(None);
        let text = render_plan_text(&plan);
        assert!(text.contains("## Context"));
        assert!(text.contains("Follow existing layout patterns"));
        assert!(text.contains("`site/index.html` · `new file` — create landing page"));
    }

    #[test]
    fn render_includes_published_output() {
        let plan = PlanDraft {
            title: "Calc plan".into(),
            overview: "Compute value".into(),
            key_files: vec![],
            context: vec![],
            tasks: vec![PlanDraftTask {
                title: "Compute".into(),
                description: None,
                files: vec![],
            }],
            verification: vec!["unrounded ~17053 hours".into()],
            published_output: "17 — thousand hours (nearest 1000 hours; no commas)".into(),
            notes: vec![],
        }
        .into_plan(None);
        let text = render_plan_text(&plan);
        assert!(text.contains("## Published output"));
        assert!(text.contains("17 — thousand hours"));
    }
}
