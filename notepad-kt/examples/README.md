# Examples (Kotlin)

Mirror [`../notepad-py/examples/`](../notepad-py/examples/) and [`../notepad-js/examples/`](../notepad-js/examples/).

## Runnable JUnit examples

Live synthesis requires `OPENAI_API_KEY` and optional `OPENAI_MODEL`:

```bash
export OPENAI_API_KEY=sk-...
cd notepad-kt
# Build host .so files first (when NDK unavailable):
cd native && cargo build --release
cp ../../target/release/libnotepad_kt.so ../notepad-android/src/main/jniLibs/x86_64/
cd ../../superglue/superglue-kt/native && cargo build --release
cp ../../../superglue/target/release/libsuperglue_kt.so ../../notepad/notepad-kt/notepad-android/src/main/jniLibs/x86_64/
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
