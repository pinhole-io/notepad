//! Pure in-memory notepad demo (no LLM).

use notepad::{Notepad, RenderOpts, StoredPayload};
use serde_json::json;

fn main() {
    let mut np = Notepad::new();

    let id1 = np.store("First finding: Rust 1.88 is stable.");
    let id2 = np.store_tagged(
        &["web", "crawl"],
        Some("docs page"),
        StoredPayload::Json(json!({"url": "https://example.com", "title": "Example"})),
        Default::default(),
    );

    println!("session: {}", np.session_id());
    println!("entries: {} ({} chars total)", np.len(), np.total_chars());
    println!("ids: {id1}, {id2}");

    let digest = np.render(&RenderOpts::default());
    println!("\n--- digest ---\n{digest}");

    let batches = np.partition(500);
    println!("\npartition (500 char budget): {} batches", batches.len());
}
