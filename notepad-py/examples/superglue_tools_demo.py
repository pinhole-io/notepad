#!/usr/bin/env python3
"""Register notepad tools on a superglue Client (offline-safe)."""

from __future__ import annotations

import notepad
import superglue


def main() -> None:
    client = superglue.Client(api_key="sk-offline-demo", model="gpt-5.4-nano-2026-03-17-mini")
    np = notepad.Notepad()
    np.store("demo note for tool registration")

    notepad.register_tools(client, np)
    notepad.register_offload_hook(
        client,
        np,
        config=notepad.OffloadConfig(auto_offload=True, min_chars=10),
    )

    assert notepad.is_notepad_stub('{"notepad_ref":"x","stored_chars":1}')
    assert "Question" in notepad.synthesis_user_prompt("Q?", "material")

    big = "x" * 5000
    messages = [
        {"role": "tool", "name": "demo_tool", "content": big},
    ]
    config = {"autoOffload": True, "minChars": 100, "previewChars": 50}
    compacted = notepad.compact_tool_messages(messages, np, config=config)
    assert len(compacted) == 1
    assert notepad.is_notepad_stub(compacted[0]["content"])

    print("superglue_tools_demo ok")


if __name__ == "__main__":
    main()
