use crate::entry::Entry;

/// Output format for [`Notepad::render`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderFormat {
    #[default]
    Markdown,
    Json,
}

/// Controls how entries are formatted for synthesis or inspection.
#[derive(Debug, Clone)]
pub struct RenderOpts {
    pub format: RenderFormat,
    pub include_metadata: bool,
    /// When set, truncate the output and append a marker.
    pub max_chars: Option<usize>,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            format: RenderFormat::Markdown,
            include_metadata: true,
            max_chars: None,
        }
    }
}

pub(crate) fn render_entry(entry: &Entry, opts: &RenderOpts) -> String {
    match opts.format {
        RenderFormat::Markdown => render_entry_markdown(entry, opts.include_metadata),
        RenderFormat::Json => render_entry_json(entry),
    }
}

fn render_entry_markdown(entry: &Entry, include_metadata: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!("### Entry `{}`\n\n", entry.id));
    if include_metadata {
        if let Some(label) = &entry.label {
            out.push_str(&format!("- **label:** {label}\n"));
        }
        if !entry.tags.is_empty() {
            out.push_str(&format!("- **tags:** {}\n", entry.tags.join(", ")));
        }
        if !entry.metadata.is_empty()
            && let Ok(meta) = serde_json::to_string(&entry.metadata)
        {
            out.push_str(&format!("- **metadata:** {meta}\n"));
        }
        out.push('\n');
    }
    out.push_str(&entry.payload.as_render_body());
    out
}

fn render_entry_json(entry: &Entry) -> String {
    serde_json::to_string_pretty(entry).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) fn render_entries(entries: &[Entry], opts: &RenderOpts) -> String {
    let body: String = entries
        .iter()
        .map(|e| render_entry(e, opts))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    apply_max_chars(body, opts.max_chars)
}

fn apply_max_chars(mut body: String, max_chars: Option<usize>) -> String {
    let Some(max) = max_chars else {
        return body;
    };
    if body.chars().count() <= max {
        return body;
    }
    body = body.chars().take(max.saturating_sub(40)).collect();
    body.push_str("\n\n… [truncated by RenderOpts.max_chars]");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::StoredPayload;

    #[test]
    fn markdown_includes_id_and_body() {
        let entry = Entry::new(
            Some("page".into()),
            vec!["web".into()],
            StoredPayload::Text("hello".into()),
            Default::default(),
        );
        let rendered = render_entry_markdown(&entry, true);
        assert!(rendered.contains(&entry.id.to_string()));
        assert!(rendered.contains("hello"));
        assert!(rendered.contains("page"));
    }
}
