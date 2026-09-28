use notepad::{Notepad, RenderFormat, RenderOpts, StoredPayload};
use serde_json::json;

#[test]
fn store_remove_render() {
    let mut np = Notepad::new();
    let id = np.store("alpha");
    assert_eq!(np.len(), 1);
    assert!(np.render(&RenderOpts::default()).contains("alpha"));

    let removed = np.remove(id).expect("removed");
    assert_eq!(removed.payload.as_render_body(), "alpha");
    assert!(np.is_empty());
}

#[test]
fn json_payload_rendered() {
    let mut np = Notepad::new();
    np.store(StoredPayload::Json(json!({"k": "v"})));
    let md = np.render(&RenderOpts::default());
    assert!(md.contains("\"k\""));

    let json_out = np.render(&RenderOpts {
        format: RenderFormat::Json,
        ..Default::default()
    });
    assert!(json_out.contains("\"payload\""));
}

#[test]
fn partition_single_oversized_entry() {
    let mut np = Notepad::new();
    np.store("x".repeat(500));
    let batches = np.partition(100);
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 1);
}

#[test]
fn render_truncates_with_marker() {
    let mut np = Notepad::new();
    np.store("word ".repeat(200));
    let out = np.render(&RenderOpts {
        max_chars: Some(50),
        ..Default::default()
    });
    assert!(out.contains("truncated"));
}
