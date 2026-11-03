# notepad-py

Python bindings for the [notepad](https://github.com/pinhole-io/notepad) Rust core.

The import name is `notepad`. The package name is `notepad-py`.

This package is not on PyPI. Build it from this repository. The parent crate needs a sibling [superglue](https://github.com/pinhole-io/superglue) checkout.

## Requirements

- Python 3.12 or newer
- A Rust toolchain
- The notepad repository and SuperGlue as siblings

## Build

From `notepad-py/`:

```bash
uvx maturin develop --release
```

The binding enables `superglue` and `synthesize`.

## Store and render

```python
import notepad

np = notepad.Notepad()
np.store("First finding from the tool loop.")
np.store_tagged(
    {"url": "https://example.com", "title": "Example"},
    tags=["web"],
    label="docs page",
)
print(np.session_id)
print(np.render())
print(np.partition(500))
```

## Synthesis

**Warning:** Keep the API key in the environment. Do not commit it.

```python
import os
import notepad

np = notepad.Notepad()
synth = notepad.Synthesizer(np, api_key=os.environ["OPENAI_API_KEY"])
outcome = synth.synthesize("Summarize everything collected.")
print(outcome.answer, outcome.passes)
```

## SuperGlue tools

```python
import os
import notepad
import superglue

np = notepad.Notepad()
client = superglue.Client(api_key=os.environ["OPENAI_API_KEY"])
notepad.register_tools(client, np)
notepad.register_offload_hook(
    client,
    np,
    config=notepad.OffloadConfig(auto_offload=True),
)
```

Examples live in `examples/`.

## License

The license is [MIT](../LICENSE).
