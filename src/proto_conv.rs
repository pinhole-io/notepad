//! Conversions between domain types and generated proto messages.

use std::collections::HashMap;

use serde_json::Value;

use crate::entry::{Entry, EntryId, StoredPayload};
use crate::proto;
use crate::render::{RenderFormat, RenderOpts};

#[cfg(feature = "superglue")]
use crate::superglue::OffloadConfig;

#[cfg(feature = "synthesize")]
use crate::synthesize::{SynthesisConfig, SynthesisOutcome};

#[cfg(feature = "superglue")]
impl From<&OffloadConfig> for proto::OffloadConfig {
    fn from(c: &OffloadConfig) -> Self {
        Self {
            auto_offload: c.auto_offload,
            min_chars: c.min_chars as u32,
            always_offload_tools: c.always_offload_tools.clone(),
            never_offload_tools: c.never_offload_tools.clone(),
            preview_chars: c.preview_chars as u32,
        }
    }
}

#[cfg(feature = "superglue")]
impl From<proto::OffloadConfig> for OffloadConfig {
    fn from(c: proto::OffloadConfig) -> Self {
        Self {
            auto_offload: c.auto_offload,
            min_chars: c.min_chars as usize,
            min_chars_by_tool: HashMap::new(),
            always_offload_tools: c.always_offload_tools,
            never_offload_tools: c.never_offload_tools,
            preview_chars: c.preview_chars as usize,
        }
    }
}

#[cfg(feature = "synthesize")]
impl From<&SynthesisConfig> for proto::SynthesisConfig {
    fn from(c: &SynthesisConfig) -> Self {
        Self {
            max_context_chars: c.max_context_chars as u32,
            synthesis_budget_ratio: c.synthesis_budget_ratio,
            extraction_budget_ratio: c.extraction_budget_ratio,
            max_extraction_rounds: c.max_extraction_rounds,
        }
    }
}

#[cfg(feature = "synthesize")]
impl From<proto::SynthesisConfig> for SynthesisConfig {
    fn from(c: proto::SynthesisConfig) -> Self {
        Self {
            max_context_chars: c.max_context_chars as usize,
            synthesis_budget_ratio: c.synthesis_budget_ratio,
            extraction_budget_ratio: c.extraction_budget_ratio,
            max_extraction_rounds: c.max_extraction_rounds,
        }
    }
}

#[cfg(feature = "synthesize")]
impl From<&SynthesisOutcome> for proto::SynthesisOutcome {
    fn from(o: &SynthesisOutcome) -> Self {
        Self {
            answer: o.answer.clone(),
            passes: o.passes,
        }
    }
}

#[cfg(feature = "synthesize")]
impl From<proto::SynthesisOutcome> for SynthesisOutcome {
    fn from(o: proto::SynthesisOutcome) -> Self {
        Self {
            answer: o.answer,
            passes: o.passes,
        }
    }
}

impl From<&StoredPayload> for proto::StoredPayload {
    fn from(p: &StoredPayload) -> Self {
        match p {
            StoredPayload::Text(s) => proto::StoredPayload {
                value: Some(proto::stored_payload::Value::Text(s.clone())),
            },
            StoredPayload::Json(v) => proto::StoredPayload {
                value: Some(proto::stored_payload::Value::Json(v.to_string())),
            },
        }
    }
}

impl TryFrom<proto::StoredPayload> for StoredPayload {
    type Error = String;

    fn try_from(p: proto::StoredPayload) -> Result<Self, Self::Error> {
        match p.value {
            Some(proto::stored_payload::Value::Text(s)) => Ok(StoredPayload::Text(s)),
            Some(proto::stored_payload::Value::Json(s)) => {
                let v: Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
                Ok(StoredPayload::Json(v))
            }
            None => Err("StoredPayload.value is missing".into()),
        }
    }
}

impl From<&Entry> for proto::Entry {
    fn from(e: &Entry) -> Self {
        Self {
            id: e.id.to_string(),
            label: e.label.clone(),
            tags: e.tags.clone(),
            payload: Some((&e.payload).into()),
            metadata_json: serde_json::to_string(&e.metadata).unwrap_or_else(|_| "{}".into()),
            created_at: e.created_at.to_string(),
        }
    }
}

impl TryFrom<proto::Entry> for Entry {
    type Error = String;

    fn try_from(e: proto::Entry) -> Result<Self, Self::Error> {
        let uuid: uuid::Uuid =
            e.id.parse()
                .map_err(|_| format!("invalid entry id: {}", e.id))?;
        let payload = e
            .payload
            .ok_or_else(|| "Entry.payload is missing".to_string())?
            .try_into()?;
        let metadata: HashMap<String, Value> =
            serde_json::from_str(&e.metadata_json).unwrap_or_default();
        let created_at = time::OffsetDateTime::parse(
            &e.created_at,
            &time::format_description::well_known::Rfc3339,
        )
        .or_else(|_| {
            time::OffsetDateTime::parse(
                &e.created_at,
                &time::format_description::well_known::Iso8601::DEFAULT,
            )
        })
        .map_err(|e| e.to_string())?;
        Ok(Entry {
            id: EntryId(uuid),
            label: e.label,
            tags: e.tags,
            payload,
            metadata,
            created_at,
        })
    }
}

impl From<&RenderOpts> for proto::RenderOptions {
    fn from(o: &RenderOpts) -> Self {
        Self {
            format: match o.format {
                RenderFormat::Markdown => proto::RenderFormat::Markdown as i32,
                RenderFormat::Json => proto::RenderFormat::Json as i32,
            },
            include_metadata: o.include_metadata,
            max_chars: o.max_chars.map(|n| n as u32),
        }
    }
}

impl TryFrom<proto::RenderOptions> for RenderOpts {
    type Error = String;

    fn try_from(o: proto::RenderOptions) -> Result<Self, Self::Error> {
        let format = match proto::RenderFormat::try_from(o.format) {
            Ok(proto::RenderFormat::Json) => RenderFormat::Json,
            Ok(proto::RenderFormat::Markdown) | Ok(proto::RenderFormat::Unspecified) => {
                RenderFormat::Markdown
            }
            Err(e) => return Err(e.to_string()),
        };
        Ok(RenderOpts {
            format,
            include_metadata: o.include_metadata,
            max_chars: o.max_chars.map(|n| n as usize),
        })
    }
}

#[cfg(feature = "superglue")]
/// Parse `OffloadConfig` from a JSON object (proto JSON field names).
pub fn offload_config_from_json(value: &Value) -> Result<OffloadConfig, String> {
    if let Ok(proto) = serde_json::from_value::<proto::OffloadConfig>(value.clone()) {
        return Ok(proto.into());
    }
    let obj = value
        .as_object()
        .ok_or_else(|| "OffloadConfig: expected object".to_string())?;
    let get_bool = |snake: &str, camel: &str| -> Option<bool> {
        obj.get(snake)
            .or_else(|| obj.get(camel))
            .and_then(|v| v.as_bool())
    };
    let get_usize = |snake: &str, camel: &str| -> Option<usize> {
        obj.get(snake)
            .or_else(|| obj.get(camel))
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
    };
    let get_strings = |snake: &str, camel: &str| -> Option<Vec<String>> {
        obj.get(snake).or_else(|| obj.get(camel)).and_then(|v| {
            v.as_array().map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
        })
    };
    Ok(OffloadConfig {
        auto_offload: get_bool("auto_offload", "autoOffload").unwrap_or(false),
        min_chars: get_usize("min_chars", "minChars").unwrap_or(2048),
        min_chars_by_tool: get_min_chars_by_tool(obj),
        always_offload_tools: get_strings("always_offload_tools", "alwaysOffloadTools")
            .unwrap_or_else(|| vec!["charlotte_site_crawl_chunks".into()]),
        never_offload_tools: get_strings("never_offload_tools", "neverOffloadTools")
            .unwrap_or_default(),
        preview_chars: get_usize("preview_chars", "previewChars").unwrap_or(200),
    })
}

fn get_min_chars_by_tool(obj: &serde_json::Map<String, Value>) -> HashMap<String, usize> {
    let Some(raw) = obj
        .get("min_chars_by_tool")
        .or_else(|| obj.get("minCharsByTool"))
        .and_then(Value::as_object)
    else {
        return HashMap::from([("read".into(), 16_000)]);
    };
    raw.iter()
        .filter_map(|(tool, value)| {
            value
                .as_u64()
                .map(|n| (tool.clone(), n as usize))
                .or_else(|| {
                    value
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .map(|n| (tool.clone(), n))
                })
        })
        .collect()
}

#[cfg(feature = "superglue")]
/// Serialize `OffloadConfig` to a JSON object (proto JSON field names, camelCase).
pub fn offload_config_to_json(config: &OffloadConfig) -> Value {
    serde_json::json!({
        "autoOffload": config.auto_offload,
        "minChars": config.min_chars,
        "minCharsByTool": config.min_chars_by_tool,
        "alwaysOffloadTools": config.always_offload_tools,
        "neverOffloadTools": config.never_offload_tools,
        "previewChars": config.preview_chars,
    })
}

#[cfg(all(test, feature = "superglue"))]
mod tests {
    use super::*;

    #[test]
    fn offload_config_round_trip() {
        let cfg = OffloadConfig {
            auto_offload: true,
            min_chars: 1024,
            min_chars_by_tool: HashMap::from([("read".into(), 16_000)]),
            always_offload_tools: vec!["big_tool".into()],
            never_offload_tools: vec!["small_tool".into()],
            preview_chars: 100,
        };
        let json = offload_config_to_json(&cfg);
        let back = offload_config_from_json(&json).unwrap();
        assert_eq!(back.auto_offload, cfg.auto_offload);
        assert_eq!(back.min_chars, cfg.min_chars);
        assert_eq!(back.always_offload_tools, cfg.always_offload_tools);
    }
}
