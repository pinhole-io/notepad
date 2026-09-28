"""notepad — Python bindings for session working memory.

Quick-start
-----------

    import notepad

    np = notepad.Notepad()
    entry_id = np.store("First finding from the tool loop.")
    entry_id = np.store_tagged(
        {"url": "https://example.com", "title": "Example"},
        tags=["web"],
        label="docs page",
    )

    print(np.session_id)
    print(np.render())
    print(np.partition(500))

    # LLM synthesis (requires OPENAI_API_KEY or pass api_key=)
    synth = notepad.Synthesizer(np, api_key="sk-...")
    outcome = synth.synthesize("Summarize everything collected.")
    print(outcome.answer, outcome.passes)

    # superglue integration (requires superglue package)
    import superglue
    client = superglue.Client(api_key="sk-...")
    notepad.register_tools(client, np)
    notepad.register_offload_hook(client, np, config=notepad.OffloadConfig(auto_offload=True))
"""

from __future__ import annotations

import ctypes
import sys
from pathlib import Path
from typing import TYPE_CHECKING, Any


def _preload_bridge() -> None:
    """Load libnotepad_bridge.so once so handle tables are shared across extensions."""
    if getattr(_preload_bridge, "_done", False):
        return
    here = Path(__file__).resolve().parent
    candidates = [
        here / "libnotepad_bridge.so",
        here.parent / "target" / "debug" / "libnotepad_bridge.so",
        here.parent.parent / "target" / "debug" / "libnotepad_bridge.so",
        here.parent.parent.parent / "superglue" / "target" / "debug" / "libnotepad_bridge.so",
    ]
    for path in candidates:
        if path.is_file():
            ctypes.CDLL(str(path), mode=ctypes.RTLD_GLOBAL)
            _preload_bridge._done = True  # type: ignore[attr-defined]
            return


_preload_bridge()

from notepad._notepad import TaskRegistry as _RustTaskRegistry
from notepad._notepad import Notepad as _RustNotepad
from notepad._notepad import OffloadConfig as _RustOffloadConfig
from notepad._notepad import SynthesisOutcome
from notepad._notepad import Synthesizer as _RustSynthesizer
from notepad._notepad import bridge_compact_tool_messages as _bridge_compact_tool_messages
from notepad._notepad import bridge_register_offload_hook as _bridge_register_offload_hook
from notepad._notepad import bridge_register_tools as _bridge_register_tools
from notepad._notepad import is_notepad_stub as _is_notepad_stub
from notepad._notepad import synthesis_user_prompt as _synthesis_user_prompt
from notepad._notepad import version

if TYPE_CHECKING:
    import superglue

__version__ = version()


class OffloadConfig:
    """When to replace tool results with notepad stubs (proto-backed)."""

    def __init__(
        self,
        *,
        auto_offload: bool = True,
        min_chars: int = 2048,
        always_offload_tools: list[str] | None = None,
        never_offload_tools: list[str] | None = None,
        preview_chars: int = 200,
    ) -> None:
        self._rust = _RustOffloadConfig(
            auto_offload,
            min_chars,
            always_offload_tools,
            never_offload_tools,
            preview_chars,
        )

    @property
    def auto_offload(self) -> bool:
        return self._rust.auto_offload

    @property
    def min_chars(self) -> int:
        return self._rust.min_chars

    @property
    def always_offload_tools(self) -> list[str]:
        return self._rust.always_offload_tools

    @property
    def never_offload_tools(self) -> list[str]:
        return self._rust.never_offload_tools

    @property
    def preview_chars(self) -> int:
        return self._rust.preview_chars

    def to_dict(self) -> dict[str, Any]:
        return self._rust.to_dict()

    def __repr__(self) -> str:
        return repr(self._rust)


class Notepad:
    """Session-scoped working memory for agent runs."""

    def __init__(self) -> None:
        self._rust = _RustNotepad()

    @property
    def session_id(self) -> str:
        return self._rust.session_id

    def native_handle(self) -> int:
        """Opaque handle for superglue Client registration."""
        return self._rust.native_handle()

    def store(self, content: str | dict) -> str:
        """Store text or a JSON-serializable dict; returns entry id."""
        return self._rust.store(content)

    def store_tagged(
        self,
        content: str | dict,
        *,
        tags: list[str] | None = None,
        label: str | None = None,
        metadata: dict | None = None,
    ) -> str:
        return self._rust.store_tagged(content, tags, label, metadata)

    def get(self, entry_id: str) -> dict | None:
        return self._rust.get(entry_id)

    def remove(self, entry_id: str) -> dict | None:
        return self._rust.remove(entry_id)

    def entries(self) -> list[dict]:
        return list(self._rust.entries())

    def __len__(self) -> int:
        return len(self._rust)

    def is_empty(self) -> bool:
        return self._rust.is_empty()

    def total_chars(self) -> int:
        return self._rust.total_chars()

    def render(
        self,
        *,
        format: str = "markdown",
        include_metadata: bool = True,
        max_chars: int | None = None,
    ) -> str:
        return self._rust.render(format, include_metadata, max_chars)

    def partition(self, max_chars: int) -> list[list[str]]:
        return self._rust.partition(max_chars)

    def __repr__(self) -> str:
        return repr(self._rust)


class Synthesizer:
    """Multi-pass LLM synthesis over a filled :class:`Notepad`."""

    def __init__(
        self,
        notepad: Notepad,
        *,
        api_key: str,
        model: str = "gpt-5.4-nano-2026-03-17-mini",
        base_url: str = "https://api.openai.com",
        system_prompt: str | None = None,
        max_context_chars: int = 120_000,
        synthesis_budget_ratio: float = 0.8,
        extraction_budget_ratio: float = 0.6,
        max_extraction_rounds: int = 8,
    ) -> None:
        self._rust = _RustSynthesizer(
            notepad._rust,
            api_key,
            model,
            base_url,
            system_prompt,
            max_context_chars,
            synthesis_budget_ratio,
            extraction_budget_ratio,
            max_extraction_rounds,
        )

    def synthesize(self, question: str) -> SynthesisOutcome:
        return self._rust.synthesize(question)

    def __repr__(self) -> str:
        return repr(self._rust)


class TaskRegistry:
    """Durable life-wide personal task store backed by a JSON registry file."""

    def __init__(self, path: str) -> None:
        self._rust = _RustTaskRegistry(path)

    @property
    def path(self) -> str:
        return self._rust.path

    def save(self) -> None:
        self._rust.save()

    def reload(self) -> None:
        self._rust.reload()

    def list(
        self,
        *,
        status: str | None = None,
        tag: str | None = None,
        due_before: str | None = None,
        limit: int | None = None,
    ) -> list[dict[str, Any]]:
        return self._rust.list(status=status, tag=tag, due_before=due_before, limit=limit)

    def get(self, task_id: str) -> dict[str, Any] | None:
        return self._rust.get(task_id)

    def create(
        self,
        title: str,
        *,
        description: str | None = None,
        tags: list[str] | None = None,
        status: str | None = None,
        due_at: str | None = None,
    ) -> str:
        return self._rust.create(title, description, tags, status, due_at)

    def update(
        self,
        task_id: str,
        *,
        title: str | None = None,
        description: str | None = None,
        tags: list[str] | None = None,
        due_at: str | None = None,
        clear_description: bool = False,
        clear_due_at: bool = False,
    ) -> None:
        self._rust.update(
            task_id,
            title,
            description,
            tags,
            due_at,
            clear_description,
            clear_due_at,
        )

    def set_status(self, task_id: str, status: str) -> None:
        self._rust.set_status(task_id, status)

    def add_note(self, task_id: str, content: str) -> str:
        return self._rust.add_note(task_id, content)

    def delete(self, task_id: str) -> None:
        self._rust.delete(task_id)

    def __repr__(self) -> str:
        return repr(self._rust)


def is_notepad_stub(content: str) -> bool:
    """Return True if *content* is a notepad offload stub JSON object."""
    return _is_notepad_stub(content)


def synthesis_user_prompt(question: str, material: str) -> str:
    """Build a user message for synthesis over collected material."""
    return _synthesis_user_prompt(question, material)


def register_tools(client: "superglue.Client", notepad: Notepad) -> None:
    """Register notepad scratchpad tools on a superglue Client."""
    _preload_bridge()
    _bridge_register_tools(client.tools_registry_ptr(), notepad.native_handle())


def register_offload_hook(
    client: "superglue.Client",
    notepad: Notepad,
    *,
    config: OffloadConfig | None = None,
) -> None:
    """Register automatic tool-result offload hook on a superglue Client."""
    _preload_bridge()
    _bridge_register_offload_hook(
        client.hooks_registry_ptr(),
        notepad.native_handle(),
        config.to_dict() if config is not None else None,
    )


def compact_tool_messages(
    messages: list[dict[str, Any]],
    notepad: Notepad,
    *,
    config: OffloadConfig | dict[str, Any] | None = None,
) -> list[dict[str, Any]]:
    """Stub large tool-role messages, storing payloads in notepad."""
    _preload_bridge()
    cfg = config.to_dict() if isinstance(config, OffloadConfig) else config
    return _bridge_compact_tool_messages(messages, notepad.native_handle(), cfg)
