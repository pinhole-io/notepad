//! Wiremock tests for single-pass and multi-pass synthesis.

use notepad::{Notepad, NotepadSynthesizer, StoredPayload, SynthesisConfig};
use serde_json::json;
use superglue::chat::ChatOptions;
use superglue::http::{ClientConfig, HttpClient};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn chat_opts(base_url: &str) -> ChatOptions {
    ChatOptions {
        base_url: base_url.to_string(),
        api_key: secrecy::Secret::new("sk-test".into()),
        model: "mock".into(),
        max_tool_rounds: 1,
        temperature: Some(0.0),
        ..Default::default()
    }
}

#[tokio::test]
async fn single_pass_synthesis_small_notepad() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-1",
            "object": "chat.completion",
            "model": "mock",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Rust 1.88 is the stable version."
                },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        })))
        .mount(&mock)
        .await;

    let http = HttpClient::new(ClientConfig::default()).expect("http");
    let mut np = Notepad::new();
    np.store("Rust stable version is 1.88 per rust-lang.org.");

    let synthesizer = NotepadSynthesizer::new(&np, &http, chat_opts(&mock.uri()));
    let outcome = synthesizer
        .synthesize("What is the stable Rust version?")
        .await
        .expect("synthesis");

    assert_eq!(outcome.passes, 1);
    assert!(outcome.answer.contains("1.88"));
}

#[tokio::test]
async fn multi_pass_synthesis_large_notepad() {
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-multi",
            "object": "chat.completion",
            "model": "mock",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "[{\"source_ref\":\"entry-a\",\"claim\":\"Fact A from source\"},{\"source_ref\":\"entry-b\",\"claim\":\"Fact B from source\"}]"
                },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}
        })))
        .mount(&mock)
        .await;

    let http = HttpClient::new(ClientConfig::default()).expect("http");
    let mut np = Notepad::new();
    np.store(StoredPayload::Text("A".repeat(800)));
    np.store(StoredPayload::Text("B".repeat(800)));

    let config = SynthesisConfig {
        max_context_chars: 4_000,
        synthesis_budget_ratio: 0.5,
        extraction_budget_ratio: 0.35,
        max_extraction_rounds: 4,
    };

    let synthesizer =
        NotepadSynthesizer::new(&np, &http, chat_opts(&mock.uri())).with_config(config);
    let outcome = synthesizer
        .synthesize("Summarize the findings.")
        .await
        .expect("multi-pass synthesis");

    assert!(outcome.passes >= 2);
    assert!(outcome.answer.contains("Fact A"));
}
