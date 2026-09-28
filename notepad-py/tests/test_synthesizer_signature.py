"""Synthesizer constructor defaults (no network)."""

from __future__ import annotations

import inspect

import notepad


def test_synthesizer_defaults() -> None:
    sig = inspect.signature(notepad.Synthesizer.__init__)
    assert sig.parameters["model"].default == "gpt-5.4-nano-2026-03-17-mini"
    assert sig.parameters["base_url"].default == "https://api.openai.com"
    assert sig.parameters["max_context_chars"].default == 120_000
    assert sig.parameters["synthesis_budget_ratio"].default == 0.8
    assert sig.parameters["extraction_budget_ratio"].default == 0.6
    assert sig.parameters["max_extraction_rounds"].default == 8
