# Notepad

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust edition 2024](https://img.shields.io/badge/rust-edition%202024-orange.svg)](Cargo.toml)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](Cargo.toml)
[![Python 3.12+](https://img.shields.io/badge/python-3.12%2B-blue.svg)](notepad-py)
[![Node.js 18+](https://img.shields.io/badge/node.js-18%2B-green.svg)](notepad-js)

**Session working memory for agent runs.** Store payloads during a tool loop, keep the active model context small, then synthesize an answer from what you collected.

Notepad is one Rust core. Python, JavaScript, and Kotlin call the same store. Agent tools and synthesis use [SuperGlue](https://github.com/pinhole-io/superglue).

[Quick start](#quick-start) · [Install](#install) · [Features](#cargo-features) · [Bindings](#language-bindings)

Release `1.0.0`. Every package uses `[workspace.package].version` in [Cargo.toml](Cargo.toml).

## Contents

- [Requirements](#requirements)
- [Install](#install)
- [Quick start](#quick-start)
- [Features](#features)
- [Cargo features](#cargo-features)
- [How it fits together](#how-it-fits-together)
- [Language bindings](#language-bindings)
- [Collect and synthesize](#collect-and-synthesize)
- [Troubleshooting](#troubleshooting)
- [Development](#development)
- [License](#license)

## Requirements

- Rust 1.88 or newer. `rust-version` in `Cargo.toml` sets this floor.
- A sibling checkout of [superglue](https://github.com/pinhole-io/superglue). Cargo resolves `path = "../superglue"` even when the `superglue` feature is off.
- `OPENAI_API_KEY` when you call `NotepadSynthesizer`.

## Install

There is no crates.io package. Clone the two repositories as siblings.

```bash
mkdir -p ~/src && cd ~/src
git clone https://github.com/pinhole-io/notepad.git
git clone https://github.com/pinhole-io/superglue.git
```

You need this layout:

```text
~/src/
  notepad/
  superglue/
```

| Surface | Where it runs | How to build |
| --- | --- | --- |
| Rust library | This repository | `cargo test` |
| Python | CPython 3.12 or newer | `notepad-py/` |
| JavaScript | Node.js 18 or newer | `notepad-js/` |
| Kotlin | Android JNI (`arm64-v8a`, `x86_64`) | `notepad-kt/` |

The binding crates enable `superglue` and `synthesize` by default. They need the sibling SuperGlue checkout.

## Quick start

`notepad_demo` does not call a model. The sibling checkout is still required.

```bash
cd notepad
cargo test
cargo run --example notepad_demo
```

## Features

| Feature | What you get |
| --- | --- |
| Store | `Notepad` keeps session entries, tags, and payloads |
| Render | `render` builds a digest for the active context |
| Partition | `partition` splits a long digest into a character budget |
| Agent tools | `register_notepad_tools` lets the model store, list, read, and remove notes |
| Offload | `register_offload_hook` can spill large tool JSON into the notepad |
| Synthesis | `NotepadSynthesizer` writes a final answer from the collected notes |

## Cargo features

| Feature | Default | Purpose |
| --- | --- | --- |
| *(none)* | yes | `Notepad` store, render, and partition |
| `superglue` | no | Agent tools, `OffloadHook`, and `compact_tool_messages` |
| `synthesize` | no | `NotepadSynthesizer`. Also enables `superglue`. |

```bash
cargo test
cargo test --features synthesize
```

## How it fits together

```text
Python / JavaScript / Kotlin / Rust
            │
            ▼
         Notepad
            │
            └── SuperGlue    tools, offload, and synthesis
```

- The Rust core owns the session store.
- A binding exposes `Notepad`, `render`, `partition`, and `Synthesizer`.
- SuperGlue runs the model loop. Notepad does not call a provider by itself until you enable `synthesize`.

## Language bindings

| Package | Path | Stack | Guide |
| --- | --- | --- | --- |
| Python | `notepad-py/` | PyO3 / maturin | [notepad-py/README.md](notepad-py/README.md) |
| JavaScript | `notepad-js/` | napi-rs | [notepad-js/README.md](notepad-js/README.md) |
| Kotlin | `notepad-kt/` | JNI | [notepad-kt/README.md](notepad-kt/README.md) |

Build each package from its directory.

## Collect and synthesize

1. Enable the `superglue` feature.
2. Register notepad tools with `register_notepad_tools`.
3. Run the agent tool loop. Let the model call `notepad_store` for distilled notes.
4. Enable `synthesize`. Call `NotepadSynthesizer::synthesize(question)` for the final answer.

Set `OffloadConfig::auto_offload` and call `register_offload_hook` when large tool JSON should move into the notepad.

Keep the API key in the environment. Do not commit it.

## Troubleshooting

| What you see | What to do |
| --- | --- |
| `can't find crate superglue` | Clone SuperGlue next to this repository. See [Install](#install). |
| `notepad_demo` still fails to build | The example does not call a model. The path dependency is still required. |
| Synthesis cannot find a key | Export `OPENAI_API_KEY`, or pass the key to the synthesizer. |
| Python cannot import `notepad` | Run `uvx maturin develop --release` in `notepad-py/`. |
| Node cannot load `notepad-js` | Run `npm run build` in `notepad-js/`. |
| The `synthesize` API is missing | Rebuild with `--features synthesize`. The default library build omits it. |

## Development

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Bump `[workspace.package].version` in `Cargo.toml` only. Set the same version in `notepad-py/pyproject.toml` and `notepad-js/package.json`.

## License

The license is [MIT](LICENSE).

Keep the copyright notice and the permission notice in every copy.
