"""Notepad binding round-trip tests."""

from __future__ import annotations

import notepad


def test_store_and_get_text() -> None:
    np = notepad.Notepad()
    entry_id = np.store("hello world")
    assert len(np) == 1
    entry = np.get(entry_id)
    assert entry is not None
    assert entry["payload"] == "hello world"
    assert entry["id"] == entry_id


def test_store_tagged_json() -> None:
    np = notepad.Notepad()
    entry_id = np.store_tagged(
        {"url": "https://example.com"},
        tags=["web"],
        label="page",
        metadata={"source": "test"},
    )
    entry = np.get(entry_id)
    assert entry is not None
    assert entry["label"] == "page"
    assert entry["tags"] == ["web"]
    assert entry["payload"]["url"] == "https://example.com"


def test_remove_and_entries() -> None:
    np = notepad.Notepad()
    id1 = np.store("a")
    id2 = np.store("b")
    removed = np.remove(id1)
    assert removed is not None
    assert np.get(id1) is None
    assert len(np) == 1
    assert len(np.entries()) == 1
    assert np.entries()[0]["id"] == id2


def test_render_and_partition() -> None:
    np = notepad.Notepad()
    np.store("x" * 100)
    np.store("y" * 100)
    digest = np.render(format="markdown", include_metadata=True)
    assert "Entry `" in digest
    batches = np.partition(120)
    assert len(batches) >= 2


def test_session_id_and_total_chars() -> None:
    np = notepad.Notepad()
    assert np.session_id
    assert np.is_empty()
    np.store("abc")
    assert not np.is_empty()
    assert np.total_chars() >= 3


def test_version() -> None:
    assert notepad.__version__
