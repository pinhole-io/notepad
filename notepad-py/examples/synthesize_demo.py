#!/usr/bin/env python3
"""LLM synthesis demo — runs live only when OPENAI_API_KEY is set."""

from __future__ import annotations

import os

import notepad


def main() -> None:
    np = notepad.Notepad()
    np.store("Rust 1.88 stabilized edition 2024.")
    np.store_tagged(
        "Session scratchpads keep tool-loop context small.",
        tags=["pattern"],
        label="architecture",
    )

    api_key = os.environ.get("OPENAI_API_KEY", "")
    if not api_key:
        print("Set OPENAI_API_KEY to run synthesis live; notepad demo data stored OK.")
        return

    model = os.environ.get("OPENAI_MODEL", "gpt-5.4-nano-2026-03-17-mini")
    synth = notepad.Synthesizer(np, api_key=api_key, model=model)
    outcome = synth.synthesize("What did we collect? Answer in one sentence.")
    print(f"passes={outcome.passes}")
    print(outcome.answer)


if __name__ == "__main__":
    main()
