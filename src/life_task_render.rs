use crate::life_task::{LifeTask, LifeTaskStatus};

/// Plain-text layout for chat and logging.
#[must_use]
pub fn render_life_task_text(task: &LifeTask) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "── {} ── [{}] ──\n",
        task.title,
        task.status.label()
    ));
    if let Some(desc) = &task.description {
        out.push_str("\nDESCRIPTION\n");
        for line in desc.lines() {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
    }
    if !task.tags.is_empty() {
        out.push_str("\nTAGS\n");
        out.push_str(&format!("  {}\n", task.tags.join(", ")));
    }
    if let Some(due) = task.due_at {
        let ts = due
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| due.to_string());
        out.push_str("\nDUE\n");
        out.push_str(&format!("  {ts}\n"));
    }
    if !task.notes.is_empty() {
        out.push_str("\nNOTES\n");
        for note in &task.notes {
            let ts = note
                .created_at
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| note.created_at.to_string());
            out.push_str(&format!("  {ts} — {}\n", note.content));
        }
    }
    if task.is_deleted() {
        out.push_str("\n[deleted]\n");
    }
    out
}

#[must_use]
pub fn life_task_status_icon(status: LifeTaskStatus) -> &'static str {
    match status {
        LifeTaskStatus::Open => "[ ]",
        LifeTaskStatus::InProgress => "[●]",
        LifeTaskStatus::Done => "[✓]",
        LifeTaskStatus::Cancelled => "[–]",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::life_task::{LifeTaskCreate, LifeTaskStatus};
    use time::OffsetDateTime;

    #[test]
    fn render_includes_title_and_status() {
        let task = LifeTask::from_create(LifeTaskCreate {
            title: "Call dentist".into(),
            description: Some("Tuesday morning".into()),
            tags: vec!["health".into()],
            status: Some(LifeTaskStatus::Open),
            due_at: Some(OffsetDateTime::now_utc() + time::Duration::days(2)),
        });
        let text = render_life_task_text(&task);
        assert!(text.contains("Call dentist"));
        assert!(text.contains("OPEN"));
        assert!(text.contains("Tuesday morning"));
        assert!(text.contains("health"));
        assert!(text.contains("DUE"));
    }
}
