package com.notepad.kt

import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json

private val json = Json { ignoreUnknownKeys = true }

/**
 * Register notepad scratchpad tools on a superglue client.
 *
 * @param toolsRegistryPtr opaque pointer from `SuperglueClient.toolsRegistryPtr()`
 */
public fun registerTools(toolsRegistryPtr: Long, notepad: Notepad) {
    NotepadNativeJni.bridgeRegisterTools(toolsRegistryPtr, notepad.nativeHandle)
}

/**
 * Register automatic tool-result offload hook on a superglue client.
 *
 * @param hooksRegistryPtr opaque pointer from `SuperglueClient.hooksRegistryPtr()`
 */
public fun registerOffloadHook(
    hooksRegistryPtr: Long,
    notepad: Notepad,
    config: OffloadConfig? = null,
) {
    NotepadNativeJni.bridgeRegisterOffloadHook(
        hooksRegistryPtr,
        notepad.nativeHandle,
        config?.toJson().orEmpty(),
    )
}

/** Stub large tool-role messages, storing payloads in notepad. */
public fun compactToolMessages(
    messagesJson: String,
    notepad: Notepad,
    config: OffloadConfig? = null,
): String =
    NotepadNativeJni.bridgeCompactToolMessages(
        messagesJson,
        notepad.nativeHandle,
        config?.toJson().orEmpty(),
    )

/** Stub large tool-role messages using kotlinx-serialization message lists. */
public fun <T> compactToolMessages(
    messages: List<T>,
    notepad: Notepad,
    config: OffloadConfig? = null,
    serializer: KSerializer<T>,
): List<T> {
    val messagesJson = json.encodeToString(ListSerializer(serializer), messages)
    val raw = compactToolMessages(messagesJson, notepad, config)
    return json.decodeFromString(ListSerializer(serializer), raw)
}
