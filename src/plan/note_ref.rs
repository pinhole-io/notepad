//! LLM-friendly short note refs (`#NOTE:ID:N#`) mapped to internal UUIDs.

use std::collections::HashMap;

use uuid::Uuid;

use crate::plan::{NoteId, Plan};

const NOTE_REF_PREFIX: &str = "#NOTE:ID:";
const NOTE_REF_SUFFIX: &str = "#";

/// Format a 1-based note index as `#NOTE:ID:N#`.
#[must_use]
pub fn note_ref(index: usize) -> String {
    format!("{NOTE_REF_PREFIX}{}{NOTE_REF_SUFFIX}", index + 1)
}

/// Parse `#NOTE:ID:N#` into a 1-based index.
#[must_use]
pub fn parse_note_ref(raw: &str) -> Option<usize> {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix(NOTE_REF_PREFIX)?
        .strip_suffix(NOTE_REF_SUFFIX)?;
    inner.parse().ok().filter(|n: &usize| *n >= 1)
}

/// Build a 1-based note index to internal id map.
#[must_use]
pub fn note_ref_map(plan: &Plan) -> HashMap<usize, NoteId> {
    plan.notes
        .iter()
        .enumerate()
        .map(|(index, note)| (index + 1, note.id))
        .collect()
}

/// Return the short ref for a note id on a plan.
#[must_use]
pub fn note_ref_for_id(plan: &Plan, note_id: NoteId) -> Option<String> {
    plan.notes
        .iter()
        .position(|note| note.id == note_id)
        .map(note_ref)
}

/// Resolve a short ref or legacy UUID against a plan.
pub fn resolve_note_id(plan: &Plan, raw: &str) -> Result<NoteId, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("missing note_id".into());
    }

    if let Some(index) = parse_note_ref(trimmed) {
        return note_ref_map(plan).get(&index).copied().ok_or_else(|| {
            format!(
                "unknown note ref {trimmed}; use one of: {}",
                format_note_ref_list(plan)
            )
        });
    }

    if let Ok(uuid) = trimmed.parse::<Uuid>() {
        let note_id = NoteId(uuid);
        if plan.notes.iter().any(|note| note.id == note_id) {
            return Ok(note_id);
        }
        return Err(format!(
            "note not found on plan: {trimmed}; use one of: {}",
            format_note_ref_list(plan)
        ));
    }

    Err(format!(
        "invalid note id: {trimmed}; use one of: {}",
        format_note_ref_list(plan)
    ))
}

fn format_note_ref_list(plan: &Plan) -> String {
    plan.notes
        .iter()
        .enumerate()
        .map(|(index, note)| format!("{} {}", note_ref(index), note.content))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{PlanDraft, PlanDraftTask};

    fn sample_plan() -> Plan {
        PlanDraft {
            title: "Plan".into(),
            overview: "Overview".into(),
            key_files: vec![],
            context: vec![],
            tasks: vec![PlanDraftTask {
                title: "Task".into(),
                description: None,
                files: vec![],
            }],
            verification: vec![],
            published_output: String::new(),
            notes: vec!["First note".into(), "Second note".into()],
        }
        .into_plan(None)
    }

    #[test]
    fn note_ref_roundtrip() {
        let plan = sample_plan();
        assert_eq!(note_ref(0), "#NOTE:ID:1#");
        assert_eq!(parse_note_ref("#NOTE:ID:2#"), Some(2));
        assert_eq!(
            resolve_note_id(&plan, "#NOTE:ID:1#").unwrap(),
            plan.notes[0].id
        );
        assert_eq!(
            note_ref_for_id(&plan, plan.notes[1].id).as_deref(),
            Some("#NOTE:ID:2#")
        );
    }
}
