use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::LifeTaskError;
use crate::life_task::{
    LifeTask, LifeTaskCreate, LifeTaskId, LifeTaskNote, LifeTaskNoteId, LifeTaskStatus,
};

const REGISTRY_VERSION: u32 = 1;

/// Filter options for listing life tasks.
#[derive(Debug, Clone, Default)]
pub struct LifeTaskFilter {
    pub status: Option<LifeTaskStatus>,
    pub tag: Option<String>,
    pub due_before: Option<OffsetDateTime>,
    pub limit: Option<usize>,
    pub include_deleted: bool,
}

/// On-disk collection of persisted life tasks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RegistryFile {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    tasks: Vec<LifeTask>,
}

fn default_version() -> u32 {
    REGISTRY_VERSION
}

/// Queryable store of life-wide personal tasks.
#[derive(Debug, Clone, Default)]
pub struct LifeTaskRegistry {
    tasks: Vec<LifeTask>,
}

impl LifeTaskRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    pub fn load(path: &Path) -> Result<Self, LifeTaskError> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let content = fs::read_to_string(path).map_err(|e| LifeTaskError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        if content.trim().is_empty() {
            return Ok(Self::new());
        }
        let file: RegistryFile =
            serde_json::from_str(&content).map_err(|e| LifeTaskError::Parse {
                message: e.to_string(),
            })?;
        Ok(Self { tasks: file.tasks })
    }

    pub fn save(&self, path: &Path) -> Result<(), LifeTaskError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| LifeTaskError::Io {
                path: parent.display().to_string(),
                message: e.to_string(),
            })?;
        }
        let file = RegistryFile {
            version: REGISTRY_VERSION,
            tasks: self.tasks.clone(),
        };
        let json = serde_json::to_string_pretty(&file).map_err(|e| LifeTaskError::Parse {
            message: e.to_string(),
        })?;
        fs::write(path, json).map_err(|e| LifeTaskError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })
    }

    /// Replace in-memory state from disk (e.g. before serving list/get).
    pub fn reload_from_disk(&mut self, path: &Path) -> Result<(), LifeTaskError> {
        *self = Self::load(path)?;
        Ok(())
    }

    pub fn create(&mut self, input: LifeTaskCreate) -> Result<LifeTaskId, LifeTaskError> {
        let title = input.title.trim();
        if title.is_empty() {
            return Err(LifeTaskError::InvalidInput(
                "title must not be empty".into(),
            ));
        }
        let task = LifeTask::from_create(LifeTaskCreate {
            title: title.to_string(),
            ..input
        });
        let id = task.id;
        self.tasks.push(task);
        Ok(id)
    }

    #[must_use]
    pub fn get(&self, id: LifeTaskId) -> Option<&LifeTask> {
        self.tasks.iter().find(|t| t.id == id)
    }

    #[must_use]
    pub fn get_mut(&mut self, id: LifeTaskId) -> Option<&mut LifeTask> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    #[must_use]
    pub fn tasks(&self) -> &[LifeTask] {
        &self.tasks
    }

    #[must_use]
    pub fn list(&self, filter: &LifeTaskFilter) -> Vec<&LifeTask> {
        let mut out: Vec<&LifeTask> = self
            .tasks
            .iter()
            .filter(|t| filter.include_deleted || !t.is_deleted())
            .filter(|t| filter.status.is_none_or(|s| t.status == s))
            .filter(|t| {
                filter
                    .tag
                    .as_ref()
                    .is_none_or(|tag| t.tags.iter().any(|existing| existing == tag))
            })
            .filter(|t| {
                filter
                    .due_before
                    .is_none_or(|before| t.due_at.is_some_and(|due| due <= before))
            })
            .collect();
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        if let Some(limit) = filter.limit {
            out.truncate(limit);
        }
        out
    }

    pub fn update(
        &mut self,
        id: LifeTaskId,
        title: Option<String>,
        description: Option<Option<String>>,
        tags: Option<Vec<String>>,
        due_at: Option<Option<OffsetDateTime>>,
    ) -> Result<(), LifeTaskError> {
        let task = self
            .get_mut(id)
            .ok_or_else(|| LifeTaskError::TaskNotFound(id.to_string()))?;
        if task.is_deleted() {
            return Err(LifeTaskError::TaskDeleted(id.to_string()));
        }
        if let Some(title) = title {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                return Err(LifeTaskError::InvalidInput(
                    "title must not be empty".into(),
                ));
            }
            task.title = trimmed.to_string();
        }
        if let Some(description) = description {
            task.description = description;
        }
        if let Some(tags) = tags {
            task.tags = tags;
        }
        if let Some(due_at) = due_at {
            task.due_at = due_at;
        }
        task.bump_revision();
        Ok(())
    }

    pub fn set_status(
        &mut self,
        id: LifeTaskId,
        status: LifeTaskStatus,
    ) -> Result<(), LifeTaskError> {
        let task = self
            .get_mut(id)
            .ok_or_else(|| LifeTaskError::TaskNotFound(id.to_string()))?;
        if task.is_deleted() {
            return Err(LifeTaskError::TaskDeleted(id.to_string()));
        }
        task.status = status;
        task.completed_at = if status == LifeTaskStatus::Done {
            Some(OffsetDateTime::now_utc())
        } else {
            None
        };
        task.bump_revision();
        Ok(())
    }

    pub fn add_note(
        &mut self,
        id: LifeTaskId,
        content: String,
    ) -> Result<LifeTaskNoteId, LifeTaskError> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(LifeTaskError::InvalidInput(
                "note content must not be empty".into(),
            ));
        }
        let note_id = LifeTaskNoteId::new_v4();
        let note = LifeTaskNote {
            id: note_id,
            content: trimmed.to_string(),
            created_at: OffsetDateTime::now_utc(),
        };
        let task = self
            .get_mut(id)
            .ok_or_else(|| LifeTaskError::TaskNotFound(id.to_string()))?;
        if task.is_deleted() {
            return Err(LifeTaskError::TaskDeleted(id.to_string()));
        }
        task.notes.push(note);
        task.bump_revision();
        Ok(note_id)
    }

    pub fn delete(&mut self, id: LifeTaskId) -> Result<(), LifeTaskError> {
        let task = self
            .get_mut(id)
            .ok_or_else(|| LifeTaskError::TaskNotFound(id.to_string()))?;
        if task.is_deleted() {
            return Ok(());
        }
        task.deleted_at = Some(OffsetDateTime::now_utc());
        task.bump_revision();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_create(title: &str) -> LifeTaskCreate {
        LifeTaskCreate {
            title: title.into(),
            description: None,
            tags: vec!["home".into()],
            status: None,
            due_at: None,
        }
    }

    #[test]
    fn create_and_get() {
        let mut reg = LifeTaskRegistry::new();
        let id = reg.create(sample_create("Buy milk")).unwrap();
        let task = reg.get(id).unwrap();
        assert_eq!(task.title, "Buy milk");
        assert_eq!(task.revision, 1);
    }

    #[test]
    fn save_load_round_trip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let mut reg = LifeTaskRegistry::new();
        let id = reg.create(sample_create("Saved")).unwrap();
        reg.save(&path).unwrap();
        let loaded = LifeTaskRegistry::load(&path).unwrap();
        assert_eq!(loaded.get(id).unwrap().title, "Saved");
    }

    #[test]
    fn revision_bumps_on_update() {
        let mut reg = LifeTaskRegistry::new();
        let id = reg.create(sample_create("Task")).unwrap();
        reg.update(id, Some("Updated".into()), None, None, None)
            .unwrap();
        assert_eq!(reg.get(id).unwrap().revision, 2);
        assert_eq!(reg.get(id).unwrap().title, "Updated");
    }

    #[test]
    fn soft_delete_hides_from_list() {
        let mut reg = LifeTaskRegistry::new();
        let id = reg.create(sample_create("Gone")).unwrap();
        reg.delete(id).unwrap();
        assert!(reg.list(&LifeTaskFilter::default()).is_empty());
        let task = reg.get(id).unwrap();
        assert!(task.is_deleted());
        assert!(
            reg.list(&LifeTaskFilter {
                include_deleted: true,
                ..Default::default()
            })
            .iter()
            .any(|t| t.id == id)
        );
    }

    #[test]
    fn set_status_sets_completed_at() {
        let mut reg = LifeTaskRegistry::new();
        let id = reg.create(sample_create("Done task")).unwrap();
        reg.set_status(id, LifeTaskStatus::Done).unwrap();
        let task = reg.get(id).unwrap();
        assert_eq!(task.status, LifeTaskStatus::Done);
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn add_note() {
        let mut reg = LifeTaskRegistry::new();
        let id = reg.create(sample_create("Notes")).unwrap();
        reg.add_note(id, "remember aisle 3".into()).unwrap();
        assert_eq!(reg.get(id).unwrap().notes.len(), 1);
    }

    #[test]
    fn list_filter_by_tag() {
        let mut reg = LifeTaskRegistry::new();
        reg.create(sample_create("Home task")).unwrap();
        reg.create(LifeTaskCreate {
            title: "Work task".into(),
            description: None,
            tags: vec!["work".into()],
            status: None,
            due_at: None,
        })
        .unwrap();
        let home = reg.list(&LifeTaskFilter {
            tag: Some("home".into()),
            ..Default::default()
        });
        assert_eq!(home.len(), 1);
        assert_eq!(home[0].title, "Home task");
    }

    #[test]
    fn due_at_round_trip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let due = OffsetDateTime::now_utc() + time::Duration::days(7);
        let mut reg = LifeTaskRegistry::new();
        let id = reg
            .create(LifeTaskCreate {
                title: "Due task".into(),
                description: None,
                tags: vec![],
                status: None,
                due_at: Some(due),
            })
            .unwrap();
        reg.save(&path).unwrap();
        let loaded = LifeTaskRegistry::load(&path).unwrap();
        let task = loaded.get(id).unwrap();
        assert_eq!(task.due_at, Some(due));
    }

    #[test]
    fn update_sets_and_clears_due_at() {
        let mut reg = LifeTaskRegistry::new();
        let due = OffsetDateTime::now_utc() + time::Duration::days(3);
        let id = reg.create(sample_create("Task")).unwrap();
        reg.update(id, None, None, None, Some(Some(due))).unwrap();
        assert_eq!(reg.get(id).unwrap().due_at, Some(due));
        assert_eq!(reg.get(id).unwrap().revision, 2);
        reg.update(id, None, None, None, Some(None)).unwrap();
        assert!(reg.get(id).unwrap().due_at.is_none());
        assert_eq!(reg.get(id).unwrap().revision, 3);
    }

    #[test]
    fn list_due_before_filter() {
        let mut reg = LifeTaskRegistry::new();
        let soon = OffsetDateTime::now_utc() + time::Duration::days(1);
        let later = OffsetDateTime::now_utc() + time::Duration::days(10);
        let cutoff = OffsetDateTime::now_utc() + time::Duration::days(5);
        reg.create(LifeTaskCreate {
            title: "Soon".into(),
            description: None,
            tags: vec![],
            status: None,
            due_at: Some(soon),
        })
        .unwrap();
        reg.create(LifeTaskCreate {
            title: "Later".into(),
            description: None,
            tags: vec![],
            status: None,
            due_at: Some(later),
        })
        .unwrap();
        reg.create(sample_create("No due")).unwrap();
        let due_soon = reg.list(&LifeTaskFilter {
            due_before: Some(cutoff),
            ..Default::default()
        });
        assert_eq!(due_soon.len(), 1);
        assert_eq!(due_soon[0].title, "Soon");
    }
}
