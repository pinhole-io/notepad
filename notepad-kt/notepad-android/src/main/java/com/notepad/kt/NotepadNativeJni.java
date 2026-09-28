package com.notepad.kt;

/** Low-level JNI surface for {@code libnotepad_kt.so}. Prefer {@link Notepad} / {@link Synthesizer}. */
public final class NotepadNativeJni {
    static {
        System.loadLibrary("notepad_kt");
    }

    private NotepadNativeJni() {}

    public static native String version();

    public static native long notepadCreate();

    public static native void notepadDestroy(long handle);

    public static native String notepadSessionId(long handle);

    public static native String notepadStore(long handle, String contentJson);

    public static native String notepadStoreTagged(long handle, String reqJson);

    public static native String notepadGet(long handle, String entryId);

    public static native String notepadRemove(long handle, String entryId);

    public static native String notepadEntries(long handle);

    public static native int notepadLen(long handle);

    public static native int notepadTotalChars(long handle);

    public static native String notepadRender(long handle, String optsJson);

    public static native String notepadPartition(long handle, int maxChars);

    public static native long synthesizerCreate(long notepadHandle, String configJson);

    public static native void synthesizerDestroy(long handle);

    public static native String synthesizerSynthesize(long handle, String question);

    /** Bridge handle for superglue Client registration. */
    public static native long notepadNativeHandle(long handle);

    public static native boolean isNotepadStub(String content);

    public static native String synthesisUserPrompt(String question, String material);

    /** Normalize OffloadConfig JSON via proto schema. */
    public static native String offloadConfigToJson(String configJson);

    public static native void bridgeRegisterTools(long registryPtr, long notepadHandle);

    public static native void bridgeRegisterOffloadHook(
        long hooksPtr,
        long notepadHandle,
        String configJson
    );

    public static native String bridgeCompactToolMessages(
        String messagesJson,
        long notepadHandle,
        String configJson
    );

    public static native long taskRegistryCreate(String path);

    public static native void taskRegistryDestroy(long handle);

    public static native void taskRegistrySave(long handle);

    public static native void taskRegistryReload(long handle);

    public static native String taskRegistryList(long handle, String reqJson);

    public static native String taskRegistryGet(long handle, String taskId);

    public static native String taskRegistryCreateTask(long handle, String reqJson);

    public static native void taskRegistryUpdate(long handle, String reqJson);

    public static native void taskRegistrySetStatus(long handle, String reqJson);

    public static native String taskRegistryAddNote(long handle, String reqJson);

    public static native void taskRegistryDelete(long handle, String taskId);
}
