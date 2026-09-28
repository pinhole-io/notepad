# notepad-kt (Kotlin / Android)

JNI bindings to the in-process [notepad](../) Rust crate, mirroring [notepad-js](../notepad-js) and [notepad-py](../notepad-py).

## Layout

- `native/` – Rust cdylib `libnotepad_kt.so` (`notepad_kt` crate) using the `jni` crate
- `notepad-android/` – Android library (Kotlin + Java `NotepadNativeJni`)

## Build native (Android)

Prerequisites: Rust, Android NDK, [`cargo-ndk`](https://github.com/bbqsrc/cargo-ndk).

From `notepad-kt/notepad-android`:

```bash
export ANDROID_NDK_HOME=/path/to/ndk
./gradlew :notepad-android:buildRustJni
./gradlew :notepad-android:assembleDebug
```

Without NDK, host tests can build a desktop `.so` via `./gradlew :notepad-android:buildRustJniHost`.

## Usage

```kotlin
import com.notepad.kt.Notepad
import com.notepad.kt.version

println(version())

Notepad().use { np ->
    val id = np.store("First finding from the tool loop.")
    println(np.sessionId)
    println(np.render())
}
```
