#!/usr/bin/env python3
"""Pure in-memory notepad demo (no LLM)."""

from __future__ import annotations

import json

import notepad


def main() -> None:
    np = notepad.Notepad()

    id1 = np.store("First finding: Rust 1.88 is stable.")
    id2 = np.store_tagged(
        {"url": "https://example.com", "title": "Example"},
        tags=["web", "crawl"],
        label="docs page",
    )

    print(f"session: {np.session_id}")
    print(f"entries: {len(np)} ({np.total_chars()} chars total)")
    print(f"ids: {id1}, {id2}")

    digest = np.render()
    print(f"\n--- digest ---\n{digest}")

    batches = np.partition(500)
    print(f"\npartition (500 char budget): {len(batches)} batches")
    print(json.dumps(batches, indent=2))


if __name__ == "__main__":
    main()
