use std::fmt;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Stable identifier for a life-wide personal task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LifeTaskId(pub Uuid);

impl LifeTaskId {
    #[must_use]
    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for LifeTaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Stable identifier for a note attached to a life task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LifeTaskNoteId(pub Uuid);

impl LifeTaskNoteId {
    #[must_use]
    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for LifeTaskNoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Lifecycle status of a life-wide personal task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LifeTaskStatus {
    #[default]
    Open,
    InProgress,
    Done,
    Cancelled,
}

impl LifeTaskStatus {
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::InProgress => "IN_PROGRESS",
            Self::Done => "DONE",
            Self::Cancelled => "CANCELLED",
        }
    }
}

/// A note attached to a life task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LifeTaskNote {
    pub id: LifeTaskNoteId,
    pub content: String,
    pub created_at: OffsetDateTime,
}

/// A durable life-wide personal task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LifeTask {
    pub id: LifeTaskId,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: LifeTaskStatus,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Vec<LifeTaskNote>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub revision: u64,
}

/// Input for creating a new life task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LifeTaskCreate {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<LifeTaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<OffsetDateTime>,
}

impl LifeTask {
    #[must_use]
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    pub(crate) fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
        self.updated_at = OffsetDateTime::now_utc();
    }

    #[must_use]
    pub fn from_create(input: LifeTaskCreate) -> Self {
        let now = OffsetDateTime::now_utc();
        let status = input.status.unwrap_or_default();
        let completed_at = if status == LifeTaskStatus::Done {
            Some(now)
        } else {
            None
        };
        Self {
            id: LifeTaskId::new_v4(),
            title: input.title,
            description: input.description,
            status,
            tags: input.tags,
            notes: Vec::new(),
            created_at: now,
            updated_at: now,
            completed_at,
            due_at: input.due_at,
            deleted_at: None,
            revision: 1,
        }
    }
}
