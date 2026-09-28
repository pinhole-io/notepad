use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

/// Stable identifier for a stored notepad entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryId(pub Uuid);

impl EntryId {
    #[must_use]
    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Arbitrary content stored in the notepad.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StoredPayload {
    Text(String),
    Json(Value),
}

impl StoredPayload {
    #[must_use]
    pub fn char_len(&self) -> usize {
        match self {
            Self::Text(s) => s.chars().count(),
            Self::Json(v) => serde_json::to_string(v).map_or(0, |s| s.chars().count()),
        }
    }

    #[must_use]
    pub fn as_render_body(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Json(v) => serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()),
        }
    }
}

impl From<String> for StoredPayload {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for StoredPayload {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

impl From<Value> for StoredPayload {
    fn from(v: Value) -> Self {
        Self::Json(v)
    }
}

/// One item in session working memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: EntryId,
    pub label: Option<String>,
    pub tags: Vec<String>,
    pub payload: StoredPayload,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,
    pub created_at: OffsetDateTime,
}

impl Entry {
    pub(crate) fn new(
        label: Option<String>,
        tags: Vec<String>,
        payload: StoredPayload,
        metadata: HashMap<String, Value>,
    ) -> Self {
        Self {
            id: EntryId::new_v4(),
            label,
            tags,
            payload,
            metadata,
            created_at: OffsetDateTime::now_utc(),
        }
    }

    #[must_use]
    pub fn char_len(&self) -> usize {
        self.payload.char_len()
    }
}
