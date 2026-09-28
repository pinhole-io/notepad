//! napi bindings for life-wide task registry.

use std::path::PathBuf;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use notepad::{
    LifeTask, LifeTaskCreate, LifeTaskFilter, LifeTaskId, LifeTaskRegistry, LifeTaskStatus,
};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

fn parse_task_id(task_id: &str) -> Result<LifeTaskId> {
    let uuid: Uuid = task_id
        .parse()
        .map_err(|_| Error::from_reason(format!("invalid task id: {task_id}")))?;
    Ok(LifeTaskId(uuid))
}

fn parse_status(raw: Option<String>) -> Result<Option<LifeTaskStatus>> {
    match raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some("open") => Ok(Some(LifeTaskStatus::Open)),
        Some("in_progress" | "active") => Ok(Some(LifeTaskStatus::InProgress)),
        Some("done" | "completed") => Ok(Some(LifeTaskStatus::Done)),
        Some("cancelled" | "canceled") => Ok(Some(LifeTaskStatus::Cancelled)),
        Some(other) => Err(Error::from_reason(format!("invalid task status: {other}"))),
    }
}

fn parse_due_at(raw: Option<String>) -> Result<Option<OffsetDateTime>> {
    match raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(value) => OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .map(Some)
            .map_err(|_| {
                Error::from_reason(format!(
                    "invalid due_at: {value} (use RFC3339, e.g. 2026-07-25T17:00:00Z)"
                ))
            }),
    }
}

fn task_to_value(task: &LifeTask) -> Result<Value> {
    serde_json::to_value(task).map_err(|e| Error::from_reason(e.to_string()))
}

#[napi(js_name = "TaskRegistry")]
pub struct TaskRegistryJs {
    inner: LifeTaskRegistry,
    path: PathBuf,
}

#[napi]
impl TaskRegistryJs {
    #[napi(constructor)]
    pub fn new(path: String) -> Result<Self> {
        let path = PathBuf::from(path);
        let inner = LifeTaskRegistry::load(&path).map_err(|e| Error::from_reason(e.to_string()))?;
        Ok(Self { inner, path })
    }

    #[napi(getter)]
    pub fn path(&self) -> String {
        self.path.display().to_string()
    }

    #[napi]
    pub fn save(&self) -> Result<()> {
        self.inner
            .save(&self.path)
            .map_err(|e| Error::from_reason(e.to_string()))
    }

    #[napi]
    pub fn reload(&mut self) -> Result<()> {
        self.inner
            .reload_from_disk(&self.path)
            .map_err(|e| Error::from_reason(e.to_string()))
    }

    #[napi]
    pub fn list(
        &self,
        status: Option<String>,
        tag: Option<String>,
        due_before: Option<String>,
        limit: Option<u32>,
    ) -> Result<Vec<Value>> {
        let filter = LifeTaskFilter {
            status: parse_status(status)?,
            tag,
            due_before: parse_due_at(due_before)?,
            limit: limit.map(|n| n as usize),
            include_deleted: false,
        };
        self.inner
            .list(&filter)
            .iter()
            .map(|task| task_to_value(task))
            .collect()
    }

    #[napi]
    pub fn get(&self, task_id: String) -> Result<Option<Value>> {
        let id = parse_task_id(&task_id)?;
        Ok(self.inner.get(id).map(task_to_value).transpose()?)
    }

    #[napi]
    pub fn create(
        &mut self,
        title: String,
        description: Option<String>,
        tags: Option<Vec<String>>,
        status: Option<String>,
        due_at: Option<String>,
    ) -> Result<String> {
        let input = LifeTaskCreate {
            title,
            description,
            tags: tags.unwrap_or_default(),
            status: parse_status(status)?,
            due_at: parse_due_at(due_at)?,
        };
        let id = self
            .inner
            .create(input)
            .map_err(|e| Error::from_reason(e.to_string()))?;
        Ok(id.to_string())
    }

    #[napi]
    pub fn update(
        &mut self,
        task_id: String,
        title: Option<String>,
        description: Option<String>,
        tags: Option<Vec<String>>,
        due_at: Option<String>,
        clear_description: Option<bool>,
        clear_due_at: Option<bool>,
    ) -> Result<()> {
        let id = parse_task_id(&task_id)?;
        let description_update = if clear_description.unwrap_or(false) {
            Some(None)
        } else if description.is_some() {
            Some(
                description
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            )
        } else {
            None
        };
        let due_at_update = if clear_due_at.unwrap_or(false) {
            Some(None)
        } else if let Some(raw) = due_at {
            if raw.trim().is_empty() {
                Some(None)
            } else {
                Some(parse_due_at(Some(raw))?)
            }
        } else {
            None
        };
        self.inner
            .update(id, title, description_update, tags, due_at_update)
            .map_err(|e| Error::from_reason(e.to_string()))
    }

    #[napi]
    pub fn set_status(&mut self, task_id: String, status: String) -> Result<()> {
        let id = parse_task_id(&task_id)?;
        let status = parse_status(Some(status))?
            .ok_or_else(|| Error::from_reason("status is required".to_string()))?;
        self.inner
            .set_status(id, status)
            .map_err(|e| Error::from_reason(e.to_string()))
    }

    #[napi]
    pub fn add_note(&mut self, task_id: String, content: String) -> Result<String> {
        let id = parse_task_id(&task_id)?;
        let note_id = self
            .inner
            .add_note(id, content)
            .map_err(|e| Error::from_reason(e.to_string()))?;
        Ok(note_id.to_string())
    }

    #[napi]
    pub fn delete(&mut self, task_id: String) -> Result<()> {
        let id = parse_task_id(&task_id)?;
        self.inner
            .delete(id)
            .map_err(|e| Error::from_reason(e.to_string()))
    }
}

pub type TaskRegistry = TaskRegistryJs;
