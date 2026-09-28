use std::fmt;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Stable identifier for a session artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ArtifactId(pub Uuid);

impl ArtifactId {
    #[must_use]
    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Session-scoped markdown artifact (Claude-style deliverable).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Artifact {
    pub id: ArtifactId,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub description: String,
    pub version: u32,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl Artifact {
    pub(crate) fn new(title: String, content: String, description: String) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: ArtifactId::new_v4(),
            title,
            content,
            description,
            version: 1,
            created_at: now,
            updated_at: now,
        }
    }
}

/// One-line summary for inline artifact cards — strips headings, comments, and boilerplate.
#[must_use]
pub fn derive_artifact_description(content: &str, title: &str) -> String {
    let mut stripped = String::new();
    let mut rest = content;
    while let Some(start) = rest.find("<!--") {
        stripped.push_str(&rest[..start]);
        rest = rest[start + 4..]
            .split_once("-->")
            .map(|(_, after)| after)
            .unwrap_or("");
    }
    stripped.push_str(rest);
    let without_comments = stripped;

    let title_norm = title.trim().to_lowercase();
    let mut paragraph = String::new();
    for line in without_comments.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with("<!--") || line.contains("-->") {
            continue;
        }
        let plain = line
            .trim_start_matches("- ")
            .trim_start_matches("* ")
            .trim_start_matches("> ")
            .replace("**", "")
            .replace('*', "")
            .replace('`', "");
        if plain.is_empty() {
            continue;
        }
        if plain.to_lowercase() == title_norm {
            continue;
        }
        if paragraph.is_empty() {
            paragraph = plain;
        } else {
            paragraph.push(' ');
            paragraph.push_str(&plain);
        }
        if paragraph.chars().count() >= 160 {
            break;
        }
    }
    let flat: String = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 160 {
        flat
    } else {
        format!("{}…", flat.chars().take(158).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_skips_headings_and_html_comments() {
        let content = "# Untitled Template\n\n## Overview\n<!-- Add a brief summary here. -->\n\nA starter template for project writeups.";
        assert_eq!(
            derive_artifact_description(content, "Untitled Template"),
            "A starter template for project writeups."
        );
    }

    #[test]
    fn derive_uses_explicit_summary_section() {
        let content = "# Advanced Template\n\n## Executive Summary\n\nStructured template for complex deliverables with sections for goals and risks.";
        assert_eq!(
            derive_artifact_description(content, "Advanced Template"),
            "Structured template for complex deliverables with sections for goals and risks."
        );
    }
}
