# Examples (Kotlin)

These tests match the Python examples in [`notepad-py/examples/`](../../notepad-py/examples/) and the JavaScript examples in [`notepad-js/examples/`](../../notepad-js/examples/).

## Runnable JUnit examples

Live synthesis needs `OPENAI_API_KEY`. `OPENAI_MODEL` is optional.

**Warning:** Keep the API key in the environment. Do not commit it.

```bash
export OPENAI_API_KEY=sk-...

# From the notepad repository root.
cargo build --release -p notepad_kt
mkdir -p notepad-kt/notepad-android/src/main/jniLibs/x86_64
cp target/release/libnotepad_kt.so notepad-kt/notepad-android/src/main/jniLibs/x86_64/

# From the sibling superglue repository root.
cargo build --release -p superglue_kt
cp target/release/libsuperglue_kt.so ../notepad/notepad-kt/notepad-android/src/main/jniLibs/x86_64/

# From the notepad repository root.
cd notepad-kt
gradle :notepad-android:testDebugUnitTest
```

| Example | Test class | Test method |
|---------|------------|-------------|
| In-memory notepad demo | `NotepadExamplesTest` | `example_notepad_demo` |
| LLM synthesis (live when key set) | `NotepadExamplesTest` | `example_synthesize_demo` |
| superglue tools + compaction | `SuperglueToolsExamplesTest` | `registerToolsNoThrow`, `compactToolMessages` |

Serialization parity (no native lib required for JSON shape):

```bash
gradle :notepad-android:testDebugUnitTest --tests 'com.notepad.kt.NotepadJsonTest'
```

Superglue integration tests require `libsuperglue_kt.so` alongside `libnotepad_kt.so` in `notepad-android/src/main/jniLibs/x86_64/`.
