"""Integration tests for notepad + superglue tool registration."""

from __future__ import annotations

import notepad
import pytest


def test_is_notepad_stub() -> None:
    assert notepad.is_notepad_stub('{"notepad_ref":"550e8400-e29b-41d4-a716-446655440000"}')
    assert not notepad.is_notepad_stub('{"url":"https://example.com"}')


def test_synthesis_user_prompt() -> None:
    prompt = notepad.synthesis_user_prompt("What?", "facts here")
    assert "facts here" in prompt
    assert "What?" in prompt


def test_offload_config_proto_round_trip() -> None:
    cfg = notepad.OffloadConfig(auto_offload=True, min_chars=512)
    d = cfg.to_dict()
    assert d["autoOffload"] is True
    assert d["minChars"] == 512


@pytest.fixture(scope="module")
def superglue_client():
    pytest.importorskip("superglue")
    import superglue as sg

    return sg.Client(api_key="sk-test", model="gpt-5.4-nano-2026-03-17-mini")


def test_register_tools_no_throw(superglue_client) -> None:
    import notepad as np_mod

    np = notepad.Notepad()
    np_mod.register_tools(superglue_client, np)
    np_mod.register_offload_hook(superglue_client, np)


def test_compact_tool_messages(superglue_client) -> None:
    np = notepad.Notepad()
    messages = [{"role": "tool", "name": "t", "content": "z" * 3000}]
    config = {"autoOffload": True, "minChars": 100, "previewChars": 40}
    out = notepad.compact_tool_messages(messages, np, config=config)
    assert notepad.is_notepad_stub(out[0]["content"])
