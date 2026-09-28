//! Multi-pass synthesis over notepad contents when the digest exceeds context budget.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use superglue::chat::{ChatOptions, complete_with_tools};
use superglue::guardrails::GuardrailRegistry;
use superglue::hooks::HookRegistry;
use superglue::http::HttpClient;
use superglue::openai::ChatMessage;
use superglue::tools::ToolRegistry;

use crate::error::SynthesisError;
use crate::notepad::Notepad;
use crate::render::{RenderFormat, RenderOpts};

/// Tunables for the synthesis pipeline.
#[derive(Debug, Clone)]
pub struct SynthesisConfig {
    /// Total context budget (chars) assumed available to the model.
    pub max_context_chars: usize,
    /// Fraction of `max_context_chars` used for single-pass digest + question.
    pub synthesis_budget_ratio: f32,
    /// Fraction of `max_context_chars` used per extraction batch.
    pub extraction_budget_ratio: f32,
    /// Maximum recursive merge rounds when merged extractions still exceed budget.
    pub max_extraction_rounds: u32,
}

impl Default for SynthesisConfig {
    fn default() -> Self {
        Self {
            max_context_chars: 120_000,
            synthesis_budget_ratio: 0.8,
            extraction_budget_ratio: 0.6,
            max_extraction_rounds: 8,
        }
    }
}

impl SynthesisConfig {
    #[must_use]
    pub fn synthesis_budget(&self) -> usize {
        ((self.max_context_chars as f32) * self.synthesis_budget_ratio) as usize
    }

    #[must_use]
    pub fn extraction_budget(&self) -> usize {
        ((self.max_context_chars as f32) * self.extraction_budget_ratio) as usize
    }
}

/// Outcome of a synthesis run.
#[derive(Debug, Clone)]
pub struct SynthesisOutcome {
    pub answer: String,
    /// `1` = single-pass; `>1` = multi-pass extraction + merge.
    pub passes: u32,
}

/// LLM-backed synthesis from a filled [`Notepad`].
pub struct NotepadSynthesizer<'a> {
    notepad: &'a Notepad,
    http: &'a HttpClient,
    chat_opts: ChatOptions,
    config: SynthesisConfig,
}

impl<'a> NotepadSynthesizer<'a> {
    pub fn new(notepad: &'a Notepad, http: &'a HttpClient, chat_opts: ChatOptions) -> Self {
        Self {
            notepad,
            http,
            chat_opts,
            config: SynthesisConfig::default(),
        }
    }

    #[must_use]
    pub fn with_config(mut self, config: SynthesisConfig) -> Self {
        self.config = config;
        self
    }

    pub async fn synthesize(&self, question: &str) -> Result<SynthesisOutcome, SynthesisError> {
        let digest_opts = RenderOpts {
            format: RenderFormat::Markdown,
            include_metadata: true,
            max_chars: None,
        };
        let digest = self.notepad.render(&digest_opts);
        let synthesis_budget = self.config.synthesis_budget();

        let question_overhead = question.chars().count().saturating_add(512);
        if digest.chars().count().saturating_add(question_overhead) <= synthesis_budget {
            let answer = self
                .single_pass_answer(&digest, question, SINGLE_PASS_SYSTEM)
                .await?;
            return Ok(SynthesisOutcome { answer, passes: 1 });
        }

        let extraction_budget = self.config.extraction_budget();
        let batches = self.notepad.partition(extraction_budget);
        let mut all_facts: Vec<ExtractedFact> = Vec::new();

        for batch in batches {
            let batch_render = {
                let entries: Vec<_> = batch
                    .iter()
                    .filter_map(|id| self.notepad.get(*id))
                    .cloned()
                    .collect();
                crate::render::render_entries(&entries, &digest_opts)
            };
            let extracted = self.extract_facts(&batch_render).await?;
            all_facts.extend(extracted);
        }

        let mut merged = dedupe_facts(all_facts);
        let mut rounds = 0u32;
        let final_overhead = question
            .chars()
            .count()
            .saturating_add(FINAL_ANSWER_PROMPT_OVERHEAD);

        while merged_json_len(&merged).saturating_add(final_overhead) > synthesis_budget {
            rounds += 1;
            if rounds > self.config.max_extraction_rounds {
                return Err(SynthesisError::MaxRoundsExceeded(
                    self.config.max_extraction_rounds,
                ));
            }
            merged = self.merge_facts_recursive(merged).await?;
        }

        let facts_json = serde_json::to_string_pretty(&merged).unwrap_or_else(|_| "[]".to_string());
        let answer = self
            .single_pass_answer(&facts_json, question, FINAL_ANSWER_SYSTEM)
            .await?;

        Ok(SynthesisOutcome {
            answer,
            passes: 2 + rounds,
        })
    }

    async fn single_pass_answer(
        &self,
        material: &str,
        question: &str,
        system: &str,
    ) -> Result<String, SynthesisError> {
        let user = format!(
            "## Collected material\n\n{material}\n\n## Question\n\n{question}\n\n\
             Answer using only the collected material. Cite entry ids or source_ref where relevant."
        );
        self.llm_text(system, &user).await
    }

    async fn extract_facts(&self, batch_text: &str) -> Result<Vec<ExtractedFact>, SynthesisError> {
        let user = format!(
            "Extract every factual claim from the material below. \
             Return a JSON array only (no markdown fences). Each item must have:\n\
             - source_ref (entry id from the material)\n\
             - claim (complete factual statement)\n\
             - optional quote, url, confidence\n\n\
             Do not omit numbers, URLs, names, or dates.\n\n\
             ## Material\n\n{batch_text}"
        );
        let raw = self.llm_text(EXTRACTION_SYSTEM, &user).await?;
        parse_facts_json(&raw)
    }

    async fn merge_facts_recursive(
        &self,
        facts: Vec<ExtractedFact>,
    ) -> Result<Vec<ExtractedFact>, SynthesisError> {
        let budget = self.config.extraction_budget();
        let chunks = partition_facts_by_json_size(&facts, budget);
        if chunks.len() <= 1 {
            return Ok(facts);
        }

        let mut merged = Vec::new();
        for chunk in chunks {
            let chunk_json =
                serde_json::to_string_pretty(&chunk).unwrap_or_else(|_| "[]".to_string());
            let user = format!(
                "Merge and deduplicate these extracted facts. \
                 Return a JSON array with the same schema. \
                 Preserve all distinct claims; do not drop details.\n\n{chunk_json}"
            );
            let raw = self.llm_text(MERGE_SYSTEM, &user).await?;
            merged.extend(parse_facts_json(&raw)?);
        }
        Ok(dedupe_facts(merged))
    }

    async fn llm_text(&self, system: &str, user: &str) -> Result<String, SynthesisError> {
        let mut opts = self.chat_opts.clone();
        opts.system_prompt = Some(system.to_string());
        opts.max_tool_rounds = opts.max_tool_rounds.max(1);

        let messages = vec![ChatMessage::text("user", user)];
        let hooks = HookRegistry::new();
        let guardrails = GuardrailRegistry::new();
        let tools = ToolRegistry::new();

        let outcome =
            complete_with_tools(self.http, &tools, &hooks, &guardrails, messages, &opts).await?;

        outcome
            .content
            .filter(|s| !s.trim().is_empty())
            .ok_or(SynthesisError::EmptyResponse)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ExtractedFact {
    pub source_ref: String,
    pub claim: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
}

const SINGLE_PASS_SYSTEM: &str = "You are a research synthesizer. Answer the user's question \
    using only the collected material. Do not invent facts. Cite entry ids where helpful.";

const EXTRACTION_SYSTEM: &str = "You are a precise fact extractor. Output valid JSON array only.";

const MERGE_SYSTEM: &str =
    "You merge extracted research facts without losing detail. Output valid JSON array only.";

const FINAL_ANSWER_SYSTEM: &str = "You answer from extracted facts only. \
    Cite source_ref where possible. Do not invent information.";

const FINAL_ANSWER_PROMPT_OVERHEAD: usize = 512;

fn parse_facts_json(raw: &str) -> Result<Vec<ExtractedFact>, SynthesisError> {
    let trimmed = raw.trim();
    let json_str = if trimmed.starts_with('[') {
        trimmed.to_string()
    } else if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            trimmed[start..=end].to_string()
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    };

    serde_json::from_str(&json_str).map_err(|e| SynthesisError::ParseExtraction(e.to_string()))
}

fn dedupe_facts(facts: Vec<ExtractedFact>) -> Vec<ExtractedFact> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for f in facts {
        let key = (f.source_ref.clone(), f.claim.trim().to_lowercase());
        if seen.insert(key) {
            out.push(f);
        }
    }
    out
}

fn merged_json_len(facts: &[ExtractedFact]) -> usize {
    serde_json::to_string(facts)
        .map(|s| s.chars().count())
        .unwrap_or(0)
}

fn partition_facts_by_json_size(
    facts: &[ExtractedFact],
    max_chars: usize,
) -> Vec<Vec<ExtractedFact>> {
    if facts.is_empty() {
        return vec![];
    }
    if max_chars == 0 {
        return facts.iter().cloned().map(|f| vec![f]).collect();
    }

    let mut batches: Vec<Vec<ExtractedFact>> = Vec::new();
    let mut current: Vec<ExtractedFact> = Vec::new();

    for fact in facts {
        let mut trial = current.clone();
        trial.push(fact.clone());
        let trial_len = merged_json_len(&trial);

        if !current.is_empty() && trial_len > max_chars {
            batches.push(current);
            current = vec![fact.clone()];
        } else {
            current = trial;
        }
    }

    if !current.is_empty() {
        batches.push(current);
    }

    batches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_removes_duplicate_claims() {
        let facts = vec![
            ExtractedFact {
                source_ref: "a".into(),
                claim: "Rust 1.88".into(),
                quote: None,
                url: None,
                confidence: None,
            },
            ExtractedFact {
                source_ref: "a".into(),
                claim: "rust 1.88".into(),
                quote: None,
                url: None,
                confidence: None,
            },
        ];
        assert_eq!(dedupe_facts(facts).len(), 1);
    }

    #[test]
    fn parse_facts_from_fenced_response() {
        let raw = r#"Here are facts:
[{"source_ref":"id1","claim":"test"}]"#;
        let facts = parse_facts_json(raw).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].claim, "test");
    }
}
