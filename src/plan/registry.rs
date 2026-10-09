use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use super::plan_ref::{public_id_for_plan_id, public_id_for_plan_id_with_attempt};
use crate::error::PlanError;
use crate::plan::{
    NoteId, Plan, PlanBodyUpdate, PlanDraft, PlanDraftTask, PlanId, PlanNote, PlanStatus,
    PlanTask, PlanTaskUpdate, TaskId, TaskStatus,
};

const REGISTRY_VERSION: u32 = 1;

/// Filter options for listing plans.
#[derive(Debug, Clone, Default)]
pub struct PlanFilter {
    pub status: Option<PlanStatus>,
    pub since: Option<OffsetDateTime>,
    pub until: Option<OffsetDateTime>,
    pub limit: Option<usize>,
}

/// On-disk collection of persisted plans.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RegistryFile {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    plans: Vec<Plan>,
}

fn default_version() -> u32 {
    REGISTRY_VERSION
}

/// Queryable store of structured implementation plans.
#[derive(Debug, Clone, Default)]
pub struct PlanRegistry {
    plans: Vec<Plan>,
}

impl PlanRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self { plans: Vec::new() }
    }

    pub fn load(path: &Path) -> Result<Self, PlanError> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let content = fs::read_to_string(path).map_err(|e| PlanError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        if content.trim().is_empty() {
            return Ok(Self::new());
        }
        let file: RegistryFile = serde_json::from_str(&content).map_err(|e| PlanError::Parse {
            message: e.to_string(),
        })?;
        let mut registry = Self { plans: file.plans };
        if registry.ensure_public_ids() {
            registry.save(path)?;
        }
        Ok(registry)
    }

    pub fn save(&self, path: &Path) -> Result<(), PlanError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| PlanError::Io {
                path: parent.display().to_string(),
                message: e.to_string(),
            })?;
        }
        let file = RegistryFile {
            version: REGISTRY_VERSION,
            plans: self.plans.clone(),
        };
        let json = serde_json::to_string_pretty(&file).map_err(|e| PlanError::Parse {
            message: e.to_string(),
        })?;
        fs::write(path, json).map_err(|e| PlanError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })
    }

    /// Replace in-memory state from disk (e.g. before serving list/get).
    pub fn reload_from_disk(&mut self, path: &Path) -> Result<(), PlanError> {
        *self = Self::load(path)?;
        Ok(())
    }

    /// Commit a new plan from a draft; returns the assigned plan id.
    ///
    /// When a plan with the content-addressed id already exists and its body still
    /// matches the draft (ignoring task status and notes), return that id.
    /// When the id exists but the body was rewritten in place, store a new plan.
    pub fn commit(&mut self, draft: PlanDraft, session_id: Option<Uuid>) -> PlanId {
        let content_id = draft.content_id();
        if let Some(existing) = self.get(content_id) {
            if plan_body_matches_draft(existing, &draft) {
                return content_id;
            }
            let plan = draft.into_plan_with_id(session_id, PlanId::new_v4());
            let id = plan.id;
            self.plans.push(plan);
            self.ensure_public_ids();
            return id;
        }
        let plan = draft.into_plan(session_id);
        let id = plan.id;
        self.plans.push(plan);
        self.ensure_public_ids();
        id
    }

    /// Rewrite selected body fields on an existing plan. Keeps [`PlanId`] and `public_id`.
    pub fn update_body(&mut self, id: PlanId, update: PlanBodyUpdate) -> Result<(), PlanError> {
        if update.is_empty() {
            return Err(PlanError::InvalidDraft(
                "update requires at least one of title, overview, key_files, context, tasks, verification, published_output"
                    .into(),
            ));
        }
        let plan = self
            .get_mut(id)
            .ok_or_else(|| PlanError::PlanNotFound(id.to_string()))?;

        if let Some(title) = update.title {
            let title = title.trim().to_string();
            if title.is_empty() {
                return Err(PlanError::InvalidDraft("title must not be empty".into()));
            }
            plan.title = title;
        }
        if let Some(overview) = update.overview {
            plan.overview = overview;
        }
        if let Some(key_files) = update.key_files {
            plan.key_files = key_files;
        }
        if let Some(context) = update.context {
            plan.context = context;
        }
        if let Some(verification) = update.verification {
            plan.verification = verification;
        }
        if let Some(published_output) = update.published_output {
            plan.published_output = published_output;
        }
        if let Some(tasks) = update.tasks {
            apply_task_updates(plan, tasks)?;
        }
        plan.updated_at = OffsetDateTime::now_utc();
        Ok(())
    }

    #[must_use]
    pub fn get(&self, id: PlanId) -> Option<&Plan> {
        self.plans.iter().find(|p| p.id == id)
    }

    #[must_use]
    pub fn get_mut(&mut self, id: PlanId) -> Option<&mut Plan> {
        self.plans.iter_mut().find(|p| p.id == id)
    }

    #[must_use]
    pub fn plans(&self) -> &[Plan] {
        &self.plans
    }

    fn ensure_public_ids(&mut self) -> bool {
        let mut changed = false;
        let mut used = HashSet::new();

        for plan in &mut self.plans {
            let current = plan.public_id.trim().to_ascii_uppercase();
            if !current.is_empty() && used.insert(current.clone()) {
                if plan.public_id != current {
                    plan.public_id = current;
                    changed = true;
                }
                continue;
            }

            let mut attempt = 0;
            loop {
                let candidate = if attempt == 0 {
                    public_id_for_plan_id(plan.id)
                } else {
                    public_id_for_plan_id_with_attempt(plan.id, attempt)
                };
                if used.insert(candidate.clone()) {
                    plan.public_id = candidate;
                    changed = true;
                    break;
                }
                attempt += 1;
            }
        }

        changed
    }

    pub fn remove(&mut self, id: PlanId) -> Result<Plan, PlanError> {
        let index = self
            .plans
            .iter()
            .position(|plan| plan.id == id)
            .ok_or_else(|| PlanError::PlanNotFound(id.to_string()))?;
        Ok(self.plans.remove(index))
    }

    #[must_use]
    pub fn list(&self, filter: &PlanFilter) -> Vec<&Plan> {
        let mut out: Vec<&Plan> = self
            .plans
            .iter()
            .filter(|p| filter.status.is_none_or(|s| p.status == s))
            .filter(|p| filter.since.is_none_or(|since| p.created_at >= since))
            .filter(|p| filter.until.is_none_or(|until| p.created_at <= until))
            .collect();
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        if let Some(limit) = filter.limit {
            out.truncate(limit);
        }
        out
    }

    pub fn set_plan_status(&mut self, id: PlanId, status: PlanStatus) -> Result<(), PlanError> {
        let plan = self
            .get_mut(id)
            .ok_or_else(|| PlanError::PlanNotFound(id.to_string()))?;
        plan.status = status;
        plan.updated_at = OffsetDateTime::now_utc();
        if status == PlanStatus::Completed && plan.completed_at.is_none() {
            plan.completed_at = Some(OffsetDateTime::now_utc());
        }
        Ok(())
    }

    pub fn activate_plan(&mut self, id: PlanId) -> Result<(), PlanError> {
        self.set_plan_status(id, PlanStatus::Active)
    }

    pub fn update_task_status(
        &mut self,
        plan_id: PlanId,
        task_id: TaskId,
        status: TaskStatus,
    ) -> Result<(), PlanError> {
        let plan = self
            .get_mut(plan_id)
            .ok_or_else(|| PlanError::PlanNotFound(plan_id.to_string()))?;
        let task = plan
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| PlanError::TaskNotFound(task_id.to_string()))?;
        task.status = status;
        task.completed_at = if status == TaskStatus::Completed {
            Some(OffsetDateTime::now_utc())
        } else {
            None
        };
        plan.updated_at = OffsetDateTime::now_utc();
        Ok(())
    }

    pub fn append_overview(&mut self, plan_id: PlanId, content: String) -> Result<(), PlanError> {
        let plan = self
            .get_mut(plan_id)
            .ok_or_else(|| PlanError::PlanNotFound(plan_id.to_string()))?;
        let addition = content.trim();
        if !addition.is_empty() {
            if !plan.overview.is_empty() {
                plan.overview.push_str("\n\n");
            }
            plan.overview.push_str(addition);
        }
        plan.updated_at = OffsetDateTime::now_utc();
        Ok(())
    }

    /// Replace the first occurrence of `old_text` in the overview with `new_text`.
    /// Falls back to whitespace-normalized matching when an exact match fails
    /// (selections from rendered markdown often differ only by wrapping/space).
    pub fn replace_excerpt(
        &mut self,
        plan_id: PlanId,
        old_text: &str,
        new_text: &str,
    ) -> Result<(), PlanError> {
        let plan = self
            .get_mut(plan_id)
            .ok_or_else(|| PlanError::PlanNotFound(plan_id.to_string()))?;
        let old = old_text.trim();
        if old.is_empty() {
            return Err(PlanError::Parse {
                message: "old_text must not be empty".into(),
            });
        }
        let replacement = new_text.trim();
        if let Some(idx) = plan.overview.find(old) {
            plan.overview
                .replace_range(idx..idx + old.len(), replacement);
            plan.updated_at = OffsetDateTime::now_utc();
            return Ok(());
        }
        if let Some((start, end)) = find_normalized_span(&plan.overview, old) {
            plan.overview.replace_range(start..end, replacement);
            plan.updated_at = OffsetDateTime::now_utc();
            return Ok(());
        }
        Err(PlanError::Parse {
            message: "old_text not found in plan overview".into(),
        })
    }

    /// Replace the entire plan overview with new text.
    pub fn replace_overview(&mut self, plan_id: PlanId, overview: String) -> Result<(), PlanError> {
        let plan = self
            .get_mut(plan_id)
            .ok_or_else(|| PlanError::PlanNotFound(plan_id.to_string()))?;
        plan.overview = overview;
        plan.updated_at = OffsetDateTime::now_utc();
        Ok(())
    }

    pub fn add_note(
        &mut self,
        plan_id: PlanId,
        content: String,
        task_id: Option<TaskId>,
    ) -> Result<NoteId, PlanError> {
        if let Some(tid) = task_id {
            let plan = self
                .get(plan_id)
                .ok_or_else(|| PlanError::PlanNotFound(plan_id.to_string()))?;
            if !plan.tasks.iter().any(|t| t.id == tid) {
                return Err(PlanError::TaskNotFound(tid.to_string()));
            }
        }
        let note_index = self
            .get(plan_id)
            .map(|plan| plan.notes.len())
            .ok_or_else(|| PlanError::PlanNotFound(plan_id.to_string()))?;
        let note_id = NoteId(Uuid::new_v5(
            &Uuid::from_u128(0x8d4c2f4a_5b68_4c63_9f72_6c3e1f4a2b90),
            format!("{plan_id}:note:{note_index}:{content}").as_bytes(),
        ));
        let note = PlanNote {
            id: note_id,
            content,
            created_at: OffsetDateTime::now_utc(),
            task_id,
        };
        let plan = self
            .get_mut(plan_id)
            .ok_or_else(|| PlanError::PlanNotFound(plan_id.to_string()))?;
        plan.notes.push(note);
        plan.updated_at = OffsetDateTime::now_utc();
        Ok(note_id)
    }

    pub fn complete_plan(&mut self, id: PlanId) -> Result<(), PlanError> {
        self.set_plan_status(id, PlanStatus::Completed)
    }

    /// Most recently updated non-completed plan, if any.
    #[must_use]
    pub fn latest_open_plan(&self) -> Option<&Plan> {
        self.plans
            .iter()
            .filter(|p| p.status != PlanStatus::Completed && p.status != PlanStatus::Abandoned)
            .max_by_key(|p| p.updated_at)
    }

    /// Most recently updated active plan.
    #[must_use]
    pub fn latest_active_plan(&self) -> Option<&Plan> {
        self.plans
            .iter()
            .filter(|p| p.status == PlanStatus::Active)
            .max_by_key(|p| p.updated_at)
    }
}

/// Compare draft identity fields to a stored plan. Task status and notes are ignored.
fn plan_body_matches_draft(plan: &Plan, draft: &PlanDraft) -> bool {
    plan.title == draft.title
        && plan.overview == draft.overview
        && plan.key_files == draft.key_files
        && plan.context == draft.context
        && plan.verification == draft.verification
        && plan.published_output == draft.published_output
        && plan.tasks.len() == draft.tasks.len()
        && plan.tasks.iter().zip(draft.tasks.iter()).all(|(task, draft_task)| {
            task_matches_draft(task, draft_task)
        })
}

fn task_matches_draft(task: &PlanTask, draft: &PlanDraftTask) -> bool {
    task.title == draft.title && task.description == draft.description && task.files == draft.files
}

fn apply_task_updates(plan: &mut Plan, updates: Vec<PlanTaskUpdate>) -> Result<(), PlanError> {
    if updates.is_empty() {
        return Err(PlanError::InvalidDraft(
            "tasks must contain at least one item".into(),
        ));
    }

    let mut seen = HashSet::new();
    let mut next_tasks = Vec::with_capacity(updates.len());
    let mut kept_ids = HashSet::new();

    for (order, update) in updates.into_iter().enumerate() {
        let title = update.title.trim().to_string();
        if title.is_empty() {
            return Err(PlanError::InvalidDraft(
                "task title must not be empty".into(),
            ));
        }
        let task = match update.task_id {
            Some(task_id) => {
                if !seen.insert(task_id) {
                    return Err(PlanError::InvalidDraft(format!(
                        "duplicate task_id in update: {task_id}"
                    )));
                }
                let existing = plan
                    .tasks
                    .iter()
                    .find(|task| task.id == task_id)
                    .ok_or_else(|| PlanError::TaskNotFound(task_id.to_string()))?;
                kept_ids.insert(task_id);
                let status = update.status.unwrap_or(existing.status);
                let completed_at = match status {
                    TaskStatus::Completed => existing
                        .completed_at
                        .or_else(|| Some(OffsetDateTime::now_utc())),
                    _ => None,
                };
                PlanTask {
                    id: task_id,
                    title,
                    description: update.description,
                    files: update.files,
                    status,
                    order,
                    completed_at,
                }
            }
            None => {
                if update.status.is_some() {
                    return Err(PlanError::InvalidDraft(
                        "status is only valid with task_id on an existing task".into(),
                    ));
                }
                PlanTask {
                    id: TaskId::new_v4(),
                    title,
                    description: update.description,
                    files: update.files,
                    status: TaskStatus::Pending,
                    order,
                    completed_at: None,
                }
            }
        };
        next_tasks.push(task);
    }

    for note in &mut plan.notes {
        if let Some(task_id) = note.task_id
            && !kept_ids.contains(&task_id)
        {
            note.task_id = None;
        }
    }
    plan.tasks = next_tasks;
    Ok(())
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Find a byte span in `haystack` whose whitespace-normalized form equals `needle`.
fn find_normalized_span(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    let target = normalize_ws(needle);
    if target.is_empty() {
        return None;
    }
    let chars: Vec<(usize, char)> = haystack.char_indices().collect();
    if chars.is_empty() {
        return None;
    }
    for start_idx in 0..chars.len() {
        let mut built = String::new();
        for &(byte_idx, ch) in &chars[start_idx..] {
            if ch.is_whitespace() {
                if built.ends_with(' ') || built.is_empty() {
                    continue;
                }
                built.push(' ');
            } else {
                built.push(ch);
            }
            let end_byte = byte_idx + ch.len_utf8();
            if built == target {
                return Some((chars[start_idx].0, end_byte));
            }
            if !target.starts_with(&built) {
                break;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{KeyFile, PlanBodyUpdate, PlanDraftTask, PlanTaskUpdate, TaskStatus};
    use tempfile::TempDir;

    fn sample_draft(title: &str) -> PlanDraft {
        PlanDraft {
            title: title.into(),
            overview: "Overview text".into(),
            key_files: vec![KeyFile {
                path: "a.rs".into(),
                description: "main".into(),
            }],
            tasks: vec![PlanDraftTask {
                title: "Do work".into(),
                description: None,
                files: vec![],
            }],
            verification: vec!["test".into()],
            published_output: String::new(),
            notes: vec![],
            context: vec![],
        }
    }

    #[test]
    fn replace_overview_sets_full_overview() {
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("T"), None);
        reg.replace_overview(id, "New full overview.\n\nSecond paragraph.".into())
            .unwrap();
        assert_eq!(
            reg.get(id).unwrap().overview,
            "New full overview.\n\nSecond paragraph."
        );
    }

    #[test]
    fn replace_excerpt_rewrites_overview_in_place() {
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("T"), None);
        reg.replace_excerpt(id, "Overview text", "Paragraph one.\n\nParagraph two.")
            .unwrap();
        assert_eq!(
            reg.get(id).unwrap().overview,
            "Paragraph one.\n\nParagraph two."
        );
    }

    #[test]
    fn replace_excerpt_matches_whitespace_normalized_selection() {
        let mut reg = PlanRegistry::new();
        let id = reg.commit(
            PlanDraft {
                title: "T".into(),
                overview: "Make separate terminal launches attach to the same session.".into(),
                key_files: vec![],
                tasks: vec![PlanDraftTask {
                    title: "Do work".into(),
                    description: None,
                    files: vec![],
                }],
                verification: vec![],
                published_output: String::new(),
                notes: vec![],
                context: vec![],
            },
            None,
        );
        reg.replace_excerpt(
            id,
            "Make separate   terminal launches\nattach to the same session.",
            "Shared session attach across terminals.",
        )
        .unwrap();
        assert_eq!(
            reg.get(id).unwrap().overview,
            "Shared session attach across terminals."
        );
    }

    #[test]
    fn replace_excerpt_errors_when_old_text_missing() {
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("T"), None);
        let err = reg.replace_excerpt(id, "not present", "x").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn commit_and_get() {
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("One"), None);
        let plan = reg.get(id).unwrap();
        assert_eq!(plan.title, "One");
    }

    #[test]
    fn commit_is_idempotent_for_the_same_draft() {
        let mut reg = PlanRegistry::new();
        let draft = sample_draft("Same");
        let first = reg.commit(draft.clone(), None);
        let second = reg.commit(draft, None);

        assert_eq!(first, second);
        assert_eq!(reg.plans().len(), 1);
    }

    #[test]
    fn commit_after_body_edit_creates_second_plan() {
        let mut reg = PlanRegistry::new();
        let draft = sample_draft("Original");
        let first = reg.commit(draft.clone(), None);
        reg.update_body(
            first,
            PlanBodyUpdate {
                title: Some("Rewritten".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let second = reg.commit(draft, None);
        assert_ne!(first, second);
        assert_eq!(reg.plans().len(), 2);
        assert_eq!(reg.get(first).unwrap().title, "Rewritten");
        assert_eq!(reg.get(second).unwrap().title, "Original");
    }

    #[test]
    fn commit_after_task_status_change_stays_idempotent() {
        let mut reg = PlanRegistry::new();
        let draft = sample_draft("Status only");
        let id = reg.commit(draft.clone(), None);
        let task_id = reg.get(id).unwrap().tasks[0].id;
        reg.update_task_status(id, task_id, TaskStatus::Completed)
            .unwrap();

        let again = reg.commit(draft, None);
        assert_eq!(again, id);
        assert_eq!(reg.plans().len(), 1);
    }

    #[test]
    fn update_body_rewrites_in_place() {
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("Keep id"), None);
        let public_id = reg.get(id).unwrap().public_id.clone();
        let first_task = reg.get(id).unwrap().tasks[0].id;
        reg.update_task_status(id, first_task, TaskStatus::Completed)
            .unwrap();
        reg.add_note(id, "task note".into(), Some(first_task))
            .unwrap();

        reg.update_body(
            id,
            PlanBodyUpdate {
                title: Some("New title".into()),
                tasks: Some(vec![
                    PlanTaskUpdate {
                        task_id: Some(first_task),
                        title: "Kept task".into(),
                        description: Some("updated".into()),
                        files: vec![],
                        status: None,
                    },
                    PlanTaskUpdate {
                        task_id: None,
                        title: "New task".into(),
                        description: None,
                        files: vec![],
                        status: None,
                    },
                ]),
                ..Default::default()
            },
        )
        .unwrap();

        let plan = reg.get(id).unwrap();
        assert_eq!(plan.id, id);
        assert_eq!(plan.public_id, public_id);
        assert_eq!(plan.title, "New title");
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].id, first_task);
        assert_eq!(plan.tasks[0].status, TaskStatus::Completed);
        assert_eq!(plan.tasks[0].title, "Kept task");
        assert_eq!(plan.tasks[1].title, "New task");
        assert_eq!(plan.tasks[1].status, TaskStatus::Pending);
        assert_eq!(plan.notes[0].task_id, Some(first_task));
    }

    #[test]
    fn update_body_detaches_notes_for_removed_tasks() {
        let mut reg = PlanRegistry::new();
        let draft = PlanDraft {
            tasks: vec![
                PlanDraftTask {
                    title: "Keep".into(),
                    description: None,
                    files: vec![],
                },
                PlanDraftTask {
                    title: "Drop".into(),
                    description: None,
                    files: vec![],
                },
            ],
            ..sample_draft("Notes")
        };
        let id = reg.commit(draft, None);
        let keep = reg.get(id).unwrap().tasks[0].id;
        let drop = reg.get(id).unwrap().tasks[1].id;
        reg.add_note(id, "on drop".into(), Some(drop)).unwrap();

        reg.update_body(
            id,
            PlanBodyUpdate {
                tasks: Some(vec![PlanTaskUpdate {
                    task_id: Some(keep),
                    title: "Keep".into(),
                    description: None,
                    files: vec![],
                    status: None,
                }]),
                ..Default::default()
            },
        )
        .unwrap();

        let plan = reg.get(id).unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert!(plan.notes[0].task_id.is_none());
    }

    #[test]
    fn remove_deletes_plan() {
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("Delete me"), None);

        let removed = reg.remove(id).unwrap();

        assert_eq!(removed.title, "Delete me");
        assert!(reg.get(id).is_none());
        assert!(reg.plans().is_empty());
    }

    #[test]
    fn remove_errors_for_unknown_plan() {
        let mut reg = PlanRegistry::new();
        let missing = PlanId::new_v4();

        let err = reg.remove(missing).unwrap_err();

        assert!(err.to_string().contains(&missing.to_string()));
    }

    #[test]
    fn save_load_round_trip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("Saved"), None);
        reg.save(&path).unwrap();
        let loaded = PlanRegistry::load(&path).unwrap();
        assert_eq!(loaded.get(id).unwrap().title, "Saved");
    }

    #[test]
    fn load_backfills_missing_public_id_and_persists_it() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("Legacy"), None);
        reg.save(&path).unwrap();

        let mut json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        json["plans"][0]
            .as_object_mut()
            .unwrap()
            .remove("public_id");
        fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let loaded = PlanRegistry::load(&path).unwrap();
        let public_id = loaded.get(id).unwrap().public_id.clone();
        assert_eq!(public_id.len(), crate::plan::PUBLIC_ID_LENGTH);
        assert!(
            serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path).unwrap()).unwrap()
                ["plans"][0]["public_id"]
                .as_str()
                .is_some()
        );
    }

    #[test]
    fn load_accepts_legacy_tuple_timestamps() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("Legacy time"), None);
        reg.save(&path).unwrap();

        let mut json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let legacy_time = serde_json::json!([2026, 227, 4, 31, 36, 25_829_851, 0, 0, 0]);
        json["plans"][0]["created_at"] = legacy_time.clone();
        json["plans"][0]["updated_at"] = legacy_time;
        fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let loaded = PlanRegistry::load(&path).unwrap();
        assert_eq!(loaded.get(id).unwrap().title, "Legacy time");
    }

    #[test]
    fn update_task_and_complete() {
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("Tasks"), None);
        let task_id = reg.get(id).unwrap().tasks[0].id;
        reg.activate_plan(id).unwrap();
        reg.update_task_status(id, task_id, TaskStatus::Completed)
            .unwrap();
        reg.complete_plan(id).unwrap();
        let plan = reg.get(id).unwrap();
        assert_eq!(plan.status, PlanStatus::Completed);
        assert_eq!(plan.tasks[0].status, TaskStatus::Completed);
    }

    #[test]
    fn list_filter_by_status() {
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("Active one"), None);
        reg.activate_plan(id).unwrap();
        reg.commit(sample_draft("Draft one"), None);
        let active = reg.list(&PlanFilter {
            status: Some(PlanStatus::Active),
            ..Default::default()
        });
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn reload_from_disk() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("On disk"), None);
        reg.save(&path).unwrap();
        let mut empty = PlanRegistry::new();
        empty.reload_from_disk(&path).unwrap();
        assert_eq!(empty.get(id).unwrap().title, "On disk");
    }

    #[test]
    fn add_note() {
        let mut reg = PlanRegistry::new();
        let id = reg.commit(sample_draft("Notes"), None);
        reg.add_note(id, "blocker".into(), None).unwrap();
        assert_eq!(reg.get(id).unwrap().notes.len(), 1);
    }
}
