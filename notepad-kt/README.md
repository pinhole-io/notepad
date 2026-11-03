# notepad-kt

Kotlin and Android bindings for [notepad](https://github.com/pinhole-io/notepad).

The surface matches [notepad-py](../notepad-py) and [notepad-js](../notepad-js). The native library is a JNI `cdylib`.

This package is not on Maven Central. Build it from this repository. The parent crate needs a sibling [superglue](https://github.com/pinhole-io/superglue) checkout.

## Layout

| Path | Role |
| --- | --- |
| `native/` | Rust crate `notepad_kt`. It builds `libnotepad_kt.so`. |
| `notepad-android/` | Android library. Package `com.notepad.kt`. |

The AAR targets `arm64-v8a` and `x86_64`.

## Requirements

- A Rust toolchain
- The Android NDK
- [`cargo-ndk`](https://github.com/bbqsrc/cargo-ndk)

## Build

From `notepad-kt/notepad-android`:

```bash
export ANDROID_NDK_HOME=/path/to/ndk
./gradlew :notepad-android:buildRustJni
./gradlew :notepad-android:assembleDebug
```

Without an NDK, build a desktop library for host tests:

```bash
./gradlew :notepad-android:buildRustJniHost
```

The binding enables `superglue` and `synthesize`.

## Store and render

```kotlin
import com.notepad.kt.Notepad
import com.notepad.kt.version

println(version())

Notepad().use { np ->
    np.store("First finding from the tool loop.")
    println(np.sessionId)
    println(np.render())
}
```

Example tests: [examples/README.md](examples/README.md).

## License

The license is [MIT](../LICENSE).
