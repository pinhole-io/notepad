use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::entry::{Entry, EntryId, StoredPayload};
use crate::error::NotepadError;
use crate::render::{RenderOpts, render_entries, render_entry};

/// Session-scoped working memory: store arbitrary payloads, render for synthesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notepad {
    session_id: Uuid,
    entries: Vec<Entry>,
    content_hashes: HashMap<u64, EntryId>,
}

impl Default for Notepad {
    fn default() -> Self {
        Self::new()
    }
}

impl Notepad {
    #[must_use]
    pub fn new() -> Self {
        Self {
            session_id: Uuid::new_v4(),
            entries: Vec::new(),
            content_hashes: HashMap::new(),
        }
    }

    #[must_use]
    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    pub fn store(&mut self, content: impl Into<StoredPayload>) -> EntryId {
        self.store_tagged(&[], None, content, HashMap::new())
    }

    pub fn store_tagged(
        &mut self,
        tags: &[&str],
        label: Option<&str>,
        content: impl Into<StoredPayload>,
        metadata: HashMap<String, Value>,
    ) -> EntryId {
        let payload = content.into();
        let hash = payload_hash(&payload);
        if let Some(existing) = self.content_hashes.get(&hash) {
            return *existing;
        }
        let entry = Entry::new(
            label.map(String::from),
            tags.iter().map(|t| (*t).to_string()).collect(),
            payload,
            metadata,
        );
        let id = entry.id;
        self.content_hashes.insert(hash, id);
        self.entries.push(entry);
        id
    }

    #[must_use]
    pub fn get(&self, id: EntryId) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn remove(&mut self, id: EntryId) -> Option<Entry> {
        let pos = self.entries.iter().position(|e| e.id == id)?;
        Some(self.entries.remove(pos))
    }

    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn total_chars(&self) -> usize {
        self.entries.iter().map(Entry::char_len).sum()
    }

    /// Render a compact, body-free index for injecting into an active model turn.
    ///
    /// Entries are shown newest first and capped so the index remains cheap to
    /// carry across long tool loops. Callers can use the displayed id with
    /// `notepad`'s `read` action when they need the full entry body.
    #[must_use]
    pub fn render_index(&self, max_entries: usize) -> String {
        if max_entries == 0 || self.entries.is_empty() {
            return String::new();
        }

        let total = self.entries.len();
        let start = total.saturating_sub(max_entries);
        let mut lines = vec![
            "[Notepad index — tool_result ids hold offloaded bodies (notepad action=read); search ids are one-line summaries]"
                .to_string(),
        ];
        lines.extend(self.entries[start..].iter().rev().map(|entry| {
            let label = compact_line(entry.label.as_deref().unwrap_or("-"), 80);
            let tags = if entry.tags.is_empty() {
                "-".to_string()
            } else {
                compact_line(&entry.tags.join(","), 80)
            };
            let kind = if entry.tags.iter().any(|tag| tag == "tool_result") {
                "body"
            } else if entry.tags.iter().any(|tag| tag == "search") {
                "search"
            } else {
                "note"
            };
            format!(
                "{} | {label} | {tags} | {kind} | {}ch",
                entry.id,
                entry.char_len()
            )
        }));
        if start > 0 {
            lines.push(format!(
                "(+{start} more — use notepad action=list to inspect the index)"
            ));
        }
        lines.join("\n")
    }

    /// Store a compact one-line record of a search/navigation result.
    pub fn record_search(&mut self, tool_name: &str, arguments: &str, outcome: &str) -> EntryId {
        let line = format!(
            "tool={}; args={}; outcome={}",
            compact_line(tool_name, 48),
            compact_line(arguments, 128),
            compact_line(outcome, 160),
        );
        self.store_tagged(
            &["search"],
            Some(tool_name),
            StoredPayload::Text(line),
            HashMap::new(),
        )
    }

    #[must_use]
    pub fn render(&self, opts: &RenderOpts) -> String {
        render_entries(&self.entries, opts)
    }

    /// Greedy partition of entry ids into batches whose rendered size fits `max_chars`.
    #[must_use]
    pub fn partition(&self, max_chars: usize) -> Vec<Vec<EntryId>> {
        if max_chars == 0 {
            return self.entries.iter().map(|e| vec![e.id]).collect();
        }

        let opts = RenderOpts {
            include_metadata: true,
            ..Default::default()
        };

        let mut batches: Vec<Vec<EntryId>> = Vec::new();
        let mut current: Vec<EntryId> = Vec::new();
        let mut current_len = 0usize;

        for entry in &self.entries {
            let rendered = render_entry(entry, &opts);
            let entry_len = rendered.chars().count();

            if !current.is_empty()
                && current_len.saturating_add(entry_len).saturating_add(2) > max_chars
            {
                batches.push(current);
                current = Vec::new();
                current_len = 0;
            }

            if entry_len > max_chars && current.is_empty() {
                batches.push(vec![entry.id]);
                continue;
            }

            current_len = current_len.saturating_add(entry_len).saturating_add(2);
            current.push(entry.id);
        }

        if !current.is_empty() {
            batches.push(current);
        }

        batches
    }

    pub fn require(&self, id: EntryId) -> Result<&Entry, NotepadError> {
        self.get(id)
            .ok_or_else(|| NotepadError::EntryNotFound(id.to_string()))
    }
}

fn payload_hash(payload: &StoredPayload) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let body = match payload {
        StoredPayload::Text(s) => s.as_str(),
        StoredPayload::Json(v) => {
            return {
                let serialized = serde_json::to_string(v).unwrap_or_default();
                let mut hasher = DefaultHasher::new();
                serialized.hash(&mut hasher);
                hasher.finish()
            };
        }
    };
    let mut hasher = DefaultHasher::new();
    body.hash(&mut hasher);
    hasher.finish()
}

fn compact_line(value: &str, max_chars: usize) -> String {
    let flat = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        flat
    } else {
        format!(
            "{}…",
            flat.chars()
                .take(max_chars.saturating_sub(1))
                .collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_retrieve() {
        let mut np = Notepad::new();
        let id = np.store("hello");
        assert_eq!(np.len(), 1);
        assert_eq!(np.get(id).unwrap().payload.as_render_body(), "hello");
    }

    #[test]
    fn partition_respects_budget() {
        let mut np = Notepad::new();
        np.store("a".repeat(100));
        np.store("b".repeat(100));
        let batches = np.partition(120);
        assert!(batches.len() >= 2);
    }

    #[test]
    fn duplicate_payload_reuses_entry_id() {
        let mut np = Notepad::new();
        let id1 = np.store("same content");
        let id2 = np.store("same content");
        assert_eq!(id1, id2);
        assert_eq!(np.len(), 1);
    }

    #[test]
    fn render_index_is_newest_first_and_capped() {
        let mut np = Notepad::new();
        np.store_tagged(&["search"], Some("old"), "old body", HashMap::new());
        let newest = np.store_tagged(&["search"], Some("new"), "new body", HashMap::new());

        let index = np.render_index(1);

        assert!(index.contains(&newest.to_string()));
        assert!(index.contains("new | search | search | 8ch"));
        assert!(index.contains("tool_result ids hold offloaded bodies"));
        assert!(!index.contains("old body"));
        assert!(index.contains("(+1 more"));
    }

    #[test]
    fn record_search_stores_one_line_search_entry() {
        let mut np = Notepad::new();
        let id = np.record_search("grep", r#"{"pattern":"foo"}"#, r#"{"count":2}"#);
        let entry = np.get(id).unwrap();

        assert_eq!(entry.label.as_deref(), Some("grep"));
        assert_eq!(entry.tags, vec!["search"]);
        assert_eq!(
            entry.payload.as_render_body(),
            r#"tool=grep; args={"pattern":"foo"}; outcome={"count":2}"#
        );
    }
}
