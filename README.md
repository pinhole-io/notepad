# notepad

Session-scoped working memory for agent runs: store payloads during a tool loop, keep the active LLM context small, then synthesize an answer from what you collected.

## Install

Install a Rust toolchain that meets `rust-version` in `Cargo.toml` (1.88 or newer).

## Hard dependency

[Superglue](https://github.com/Bioto/superglue) is a required path dependency. Clone it next to this repo before `cargo build` or `cargo test`. Cargo resolves that path even when the `superglue` feature is off.

The `superglue` feature enables agent tools and offload hooks. The `synthesize` feature enables `NotepadSynthesizer` and turns the `superglue` feature on.

## Quick start

```bash
# parent/
git clone https://github.com/Bioto/notepad.git
git clone https://github.com/Bioto/superglue.git
cd notepad
cargo test
cargo run --example notepad_demo
```

`notepad_demo` does not call an LLM. The sibling checkout is still required.

## Optional features

| Feature | Purpose |
|---------|---------|
| *(default)* | Core `Notepad` store, render, and partition |
| `superglue` | Agent tools, `OffloadHook`, `compact_tool_messages`, stub helpers |
| `synthesize` | `NotepadSynthesizer` multi-pass LLM synthesis (implies `superglue`) |

```bash
cargo test --features synthesize
```

Layout:

```text
parent/
  notepad/
  superglue/
```

## Language bindings

| Package | Path | Stack |
|---------|------|-------|
| Python | `notepad-py/` | PyO3 / maturin |
| JavaScript | `notepad-js/` | napi-rs |
| Kotlin / Android | `notepad-kt/` | JNI |

Build from each package directory. See the README in that directory.

Bindings expose `Notepad`, `render`, `partition`, and `Synthesizer` (LLM synthesis). Binding crates enable `superglue` and `synthesize` by default, so they also need the sibling `superglue` checkout.

## Collect and synthesize

1. Enable the `superglue` feature.
2. Register notepad tools with `register_notepad_tools`.
3. Run your agent tool loop. Let the model call `notepad_store` for distilled notes.
4. With the `synthesize` feature, call `NotepadSynthesizer::synthesize(question)` for the final answer.

Optional: set `OffloadConfig::auto_offload` and call `register_offload_hook` to spill large tool JSON into the notepad.

## License

This project uses the BSD-4-Clause license. See `LICENSE`.

Public materials that mention this software must display the acknowledgement in the LICENSE file.

This license is GPL-incompatible.
