use std::fmt;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

const PLAN_NAMESPACE: Uuid = Uuid::from_u128(0x8d4c2f4a_5b68_4c63_9f72_6c3e1f4a2b90);

mod rfc3339_or_legacy {
    use serde::{Deserialize, Deserializer, Serializer};
    use time::OffsetDateTime;

    pub fn serialize<S>(value: &OffsetDateTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let text = value
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&text)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<OffsetDateTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        deserialize_value(value)
    }

    fn deserialize_value<E>(value: serde_json::Value) -> Result<OffsetDateTime, E>
    where
        E: serde::de::Error,
    {
        if let Some(text) = value.as_str() {
            return OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
                .map_err(|error| {
                    serde::de::Error::custom(format!("invalid RFC3339 time: {error}"))
                });
        }
        serde_json::from_value(value)
            .map_err(|error| serde::de::Error::custom(format!("invalid legacy time: {error}")))
    }

    pub mod option {
        use super::{OffsetDateTime, deserialize_value};
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S>(
            value: &Option<OffsetDateTime>,
            serializer: S,
        ) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match value {
                Some(value) => serializer.serialize_some(
                    &value
                        .format(&time::format_description::well_known::Rfc3339)
                        .map_err(serde::ser::Error::custom)?,
                ),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<OffsetDateTime>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let value = Option::<serde_json::Value>::deserialize(deserializer)?;
            value.map(deserialize_value).transpose()
        }
    }
}

/// Stable identifier for a persisted plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlanId(pub Uuid);

impl PlanId {
    #[must_use]
    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for PlanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Stable identifier for a plan task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub Uuid);

impl TaskId {
    #[must_use]
    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Stable identifier for a plan note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NoteId(pub Uuid);

impl NoteId {
    #[must_use]
    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Lifecycle status of a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    #[default]
    Draft,
    Active,
    Completed,
    Paused,
    Abandoned,
}

impl PlanStatus {
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Active => "ACTIVE",
            Self::Completed => "DONE",
            Self::Paused => "PAUSED",
            Self::Abandoned => "ABANDONED",
        }
    }
}

/// Status of an individual plan task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Skipped,
}

/// A file or area referenced by the plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyFile {
    pub path: String,
    pub description: String,
}

/// Concrete implementation anchor for a plan task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskAnchor {
    pub path: String,
    pub symbol_or_lines: String,
    pub change: String,
}

/// One executable step in a plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanTask {
    pub id: TaskId,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<TaskAnchor>,
    pub status: TaskStatus,
    pub order: usize,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rfc3339_or_legacy::option"
    )]
    pub completed_at: Option<OffsetDateTime>,
}

/// A note attached to a plan or a specific task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanNote {
    pub id: NoteId,
    pub content: String,
    #[serde(with = "rfc3339_or_legacy")]
    pub created_at: OffsetDateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
}

/// A persisted implementation plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    pub id: PlanId,
    /// Stable short identifier shown to agents and clients.
    #[serde(default)]
    pub public_id: String,
    pub title: String,
    pub overview: String,
    pub status: PlanStatus,
    #[serde(default)]
    pub key_files: Vec<KeyFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<PlanTask>,
    #[serde(default)]
    pub verification: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub published_output: String,
    #[serde(default)]
    pub notes: Vec<PlanNote>,
    #[serde(with = "rfc3339_or_legacy")]
    pub created_at: OffsetDateTime,
    #[serde(with = "rfc3339_or_legacy")]
    pub updated_at: OffsetDateTime,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rfc3339_or_legacy::option"
    )]
    pub completed_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
}

/// Task input for plan commit (no ids or timestamps).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanDraftTask {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<TaskAnchor>,
}

/// Task input for an in-place plan body update.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanTaskUpdate {
    /// Existing task to keep. Omit to add a new task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<TaskAnchor>,
    /// When set, replaces the task status. Otherwise the prior status stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
}

/// Partial body rewrite for an existing plan. Omitted fields stay unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlanBodyUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_files: Option<Vec<KeyFile>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<Vec<PlanTaskUpdate>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_output: Option<String>,
}

impl PlanBodyUpdate {
    /// True when every field is omitted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.overview.is_none()
            && self.key_files.is_none()
            && self.context.is_none()
            && self.tasks.is_none()
            && self.verification.is_none()
            && self.published_output.is_none()
    }
}

/// Structured plan payload supplied by the agent via `plan_commit`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanDraft {
    pub title: String,
    pub overview: String,
    #[serde(default)]
    pub key_files: Vec<KeyFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<PlanDraftTask>,
    #[serde(default)]
    pub verification: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub published_output: String,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Task progress summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlanProgress {
    pub completed: usize,
    pub total: usize,
    pub in_progress: usize,
}

impl Plan {
    #[must_use]
    pub fn progress(&self) -> PlanProgress {
        let mut completed = 0usize;
        let mut in_progress = 0usize;
        for task in &self.tasks {
            match task.status {
                TaskStatus::Completed | TaskStatus::Skipped => completed += 1,
                TaskStatus::InProgress => in_progress += 1,
                TaskStatus::Pending => {}
            }
        }
        PlanProgress {
            completed,
            total: self.tasks.len(),
            in_progress,
        }
    }

    #[must_use]
    pub fn active_task(&self) -> Option<&PlanTask> {
        self.tasks
            .iter()
            .find(|t| t.status == TaskStatus::InProgress)
            .or_else(|| self.tasks.iter().find(|t| t.status == TaskStatus::Pending))
    }
}

impl PlanDraft {
    /// Serialize the identity-bearing draft fields in a stable field order.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        #[derive(Serialize)]
        struct CanonicalPlanDraft<'a> {
            title: &'a str,
            overview: &'a str,
            key_files: &'a [KeyFile],
            context: &'a [String],
            tasks: &'a [PlanDraftTask],
            verification: &'a [String],
            published_output: &'a str,
        }

        serde_json::to_vec(&CanonicalPlanDraft {
            title: &self.title,
            overview: &self.overview,
            key_files: &self.key_files,
            context: &self.context,
            tasks: &self.tasks,
            verification: &self.verification,
            published_output: &self.published_output,
        })
        .expect("plan draft canonical serialization cannot fail")
    }

    /// Content-addressed plan id for this draft.
    #[must_use]
    pub fn content_id(&self) -> PlanId {
        PlanId(Uuid::new_v5(&PLAN_NAMESPACE, &self.canonical_bytes()))
    }

    /// Build a new [`Plan`] with content-addressed ids and default statuses.
    pub fn into_plan(self, session_id: Option<Uuid>) -> Plan {
        let id = self.content_id();
        self.into_plan_with_id(session_id, id)
    }

    /// Build a new [`Plan`] with an explicit id (used when content id collides).
    pub fn into_plan_with_id(self, session_id: Option<Uuid>, id: PlanId) -> Plan {
        let now = OffsetDateTime::now_utc();
        let tasks = self
            .tasks
            .into_iter()
            .enumerate()
            .map(|(order, t)| PlanTask {
                id: TaskId(Uuid::new_v5(
                    &PLAN_NAMESPACE,
                    format!("{id}:task:{order}").as_bytes(),
                )),
                title: t.title,
                description: t.description,
                files: t.files,
                status: TaskStatus::Pending,
                order,
                completed_at: None,
            })
            .collect();
        let notes = self
            .notes
            .into_iter()
            .enumerate()
            .map(|(index, content)| PlanNote {
                id: NoteId(Uuid::new_v5(
                    &PLAN_NAMESPACE,
                    format!("{id}:note:{index}:{content}").as_bytes(),
                )),
                content,
                created_at: now,
                task_id: None,
            })
            .collect();
        Plan {
            id,
            public_id: super::plan_ref::public_id_for_plan_id(id),
            title: self.title,
            overview: self.overview,
            status: PlanStatus::Draft,
            key_files: self.key_files,
            context: self.context,
            tasks,
            verification: self.verification,
            published_output: self.published_output,
            notes,
            created_at: now,
            updated_at: now,
            completed_at: None,
            session_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_into_plan_assigns_ids() {
        let draft = PlanDraft {
            title: "Test plan".into(),
            overview: "Do the thing".into(),
            key_files: vec![KeyFile {
                path: "src/main.rs".into(),
                description: "entry point".into(),
            }],
            context: vec![],
            tasks: vec![
                PlanDraftTask {
                    title: "Step one".into(),
                    description: None,
                    files: vec![],
                },
                PlanDraftTask {
                    title: "Step two".into(),
                    description: Some("detail".into()),
                    files: vec![],
                },
            ],
            verification: vec!["cargo test".into()],
            published_output: String::new(),
            notes: vec!["Watch for edge cases".into()],
        };
        let plan = draft.into_plan(None);
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].status, TaskStatus::Pending);
        assert_eq!(plan.notes.len(), 1);
        assert_eq!(plan.status, PlanStatus::Draft);
    }

    #[test]
    fn progress_counts_statuses() {
        let mut plan = PlanDraft {
            title: "P".into(),
            overview: "O".into(),
            key_files: vec![],
            context: vec![],
            tasks: vec![
                PlanDraftTask {
                    title: "a".into(),
                    description: None,
                    files: vec![],
                },
                PlanDraftTask {
                    title: "b".into(),
                    description: None,
                    files: vec![],
                },
                PlanDraftTask {
                    title: "c".into(),
                    description: None,
                    files: vec![],
                },
            ],
            verification: vec![],
            published_output: String::new(),
            notes: vec![],
        }
        .into_plan(None);
        plan.tasks[0].status = TaskStatus::Completed;
        plan.tasks[1].status = TaskStatus::InProgress;
        let p = plan.progress();
        assert_eq!(p.completed, 1);
        assert_eq!(p.in_progress, 1);
        assert_eq!(p.total, 3);
    }

    #[test]
    fn draft_task_without_files_defaults_empty() {
        let task: PlanDraftTask = serde_json::from_str(r#"{"title":"Step"}"#).unwrap();
        assert!(task.files.is_empty());
    }

    #[test]
    fn draft_ids_are_deterministic() {
        let draft = PlanDraft {
            title: "Stable".into(),
            overview: "Keep ids stable".into(),
            key_files: vec![],
            context: vec![],
            tasks: vec![PlanDraftTask {
                title: "One".into(),
                description: None,
                files: vec![],
            }],
            verification: vec![],
            published_output: String::new(),
            notes: vec!["note".into()],
        };
        let first = draft.clone().into_plan(None);
        let second = draft.into_plan(None);

        assert_eq!(first.id, second.id);
        assert_eq!(first.public_id, second.public_id);
        assert_eq!(first.tasks[0].id, second.tasks[0].id);
        assert_eq!(first.notes[0].id, second.notes[0].id);
    }

    #[test]
    fn plan_identity_changes_when_content_changes() {
        let mut draft = PlanDraft {
            title: "Stable".into(),
            overview: "Overview".into(),
            key_files: vec![],
            context: vec![],
            tasks: vec![PlanDraftTask {
                title: "One".into(),
                description: None,
                files: vec![],
            }],
            verification: vec![],
            published_output: String::new(),
            notes: vec![],
        };
        let first_id = draft.clone().into_plan(None).id;
        draft.title = "Changed".into();

        assert_ne!(first_id, draft.into_plan(None).id);
    }

    #[test]
    fn plan_round_trips_anchors_and_context() {
        let draft = PlanDraft {
            title: "Anchored".into(),
            overview: "Do it".into(),
            key_files: vec![],
            context: vec!["Use existing error types".into()],
            tasks: vec![PlanDraftTask {
                title: "Wire handler".into(),
                description: None,
                files: vec![TaskAnchor {
                    path: "src/plans.rs".into(),
                    symbol_or_lines: "plan_execute_message".into(),
                    change: "add trust clause".into(),
                }],
            }],
            verification: vec!["cargo test".into()],
            published_output: String::new(),
            notes: vec![],
        };
        let plan = draft.into_plan(None);
        assert_eq!(plan.context, vec!["Use existing error types"]);
        assert_eq!(plan.tasks[0].files.len(), 1);
        assert_eq!(plan.tasks[0].files[0].path, "src/plans.rs");
    }
}
