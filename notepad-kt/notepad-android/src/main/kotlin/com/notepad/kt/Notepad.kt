package com.notepad.kt

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive

private val json = Json {
    encodeDefaults = true
    ignoreUnknownKeys = true
}

/** notepad version string (Rust crate `CARGO_PKG_VERSION`). */
public fun version(): String = NotepadNativeJni.version()

public class NotepadException(message: String) : RuntimeException(message)

@Serializable
public data class Entry(
    public val id: String,
    public val label: String? = null,
    public val tags: List<String> = emptyList(),
    public val payload: JsonElement,
    public val metadata: JsonElement = JsonNull,
    @SerialName("createdAt")
    public val createdAt: String,
)

@Serializable
private data class StoreTaggedRequest(
    public val content: JsonElement,
    public val tags: List<String> = emptyList(),
    public val label: String? = null,
    public val metadata: Map<String, JsonElement> = emptyMap(),
)

@Serializable
public data class RenderOptions(
    public val format: String = "markdown",
    @SerialName("includeMetadata")
    public val includeMetadata: Boolean = true,
    @SerialName("maxChars")
    public val maxChars: Int? = null,
)

@Serializable
public data class SynthesizerConfig(
    @SerialName("apiKey")
    public val apiKey: String,
    public val model: String = "gpt-5.4-nano-2026-03-17-mini",
    @SerialName("baseUrl")
    public val baseUrl: String = "https://api.openai.com",
    @SerialName("systemPrompt")
    public val systemPrompt: String? = null,
    @SerialName("maxContextChars")
    public val maxContextChars: Int = 120_000,
    @SerialName("synthesisBudgetRatio")
    public val synthesisBudgetRatio: Float = 0.8f,
    @SerialName("extractionBudgetRatio")
    public val extractionBudgetRatio: Float = 0.6f,
    @SerialName("maxExtractionRounds")
    public val maxExtractionRounds: Int = 8,
)

@Serializable
public data class SynthesisOutcome(
    public val answer: String,
    public val passes: Int,
)

@Serializable
public data class OffloadConfig(
    @SerialName("autoOffload")
    public val autoOffload: Boolean = false,
    @SerialName("minChars")
    public val minChars: Int = 2048,
    @SerialName("alwaysOffloadTools")
    public val alwaysOffloadTools: List<String> = listOf("charlotte_site_crawl_chunks"),
    @SerialName("neverOffloadTools")
    public val neverOffloadTools: List<String> = emptyList(),
    @SerialName("previewChars")
    public val previewChars: Int = 200,
) {
    public fun toJson(): String =
        NotepadNativeJni.offloadConfigToJson(json.encodeToString(this))
}

/** Return true if [content] is a notepad offload stub JSON object. */
public fun isNotepadStub(content: String): Boolean = NotepadNativeJni.isNotepadStub(content)

/** Build a user message for synthesis over collected material. */
public fun synthesisUserPrompt(question: String, material: String): String =
    NotepadNativeJni.synthesisUserPrompt(question, material)

/** Session-scoped working memory for agent runs. */
public class Notepad @PublishedApi internal constructor(
    @PublishedApi internal val handle: Long,
) : AutoCloseable {
    public constructor() : this(NotepadNativeJni.notepadCreate())

    public val sessionId: String
        get() = NotepadNativeJni.notepadSessionId(handle)

    public val length: Int
        get() = NotepadNativeJni.notepadLen(handle)

    public val totalChars: Int
        get() = NotepadNativeJni.notepadTotalChars(handle)

    /** Opaque handle for superglue Client registration. */
    public val nativeHandle: Long
        get() = NotepadNativeJni.notepadNativeHandle(handle)

    public fun store(content: JsonElement): String =
        NotepadNativeJni.notepadStore(handle, json.encodeToString(content))

    public fun store(text: String): String =
        NotepadNativeJni.notepadStore(handle, json.encodeToString(JsonPrimitive(text)))

    public fun storeTagged(
        content: JsonElement,
        tags: List<String> = emptyList(),
        label: String? = null,
        metadata: Map<String, JsonElement> = emptyMap(),
    ): String {
        val req = StoreTaggedRequest(content, tags, label, metadata)
        return NotepadNativeJni.notepadStoreTagged(handle, json.encodeToString(req))
    }

    public fun get(entryId: String): Entry? {
        val raw = NotepadNativeJni.notepadGet(handle, entryId)
        return json.decodeFromString<Entry?>(raw)
    }

    public fun remove(entryId: String): Entry? {
        val raw = NotepadNativeJni.notepadRemove(handle, entryId)
        return json.decodeFromString<Entry?>(raw)
    }

    public fun entries(): List<Entry> {
        val raw = NotepadNativeJni.notepadEntries(handle)
        return json.decodeFromString(raw)
    }

    public fun render(options: RenderOptions = RenderOptions()): String =
        NotepadNativeJni.notepadRender(handle, json.encodeToString(options))

    public fun partition(maxChars: Int): List<List<String>> {
        val raw = NotepadNativeJni.notepadPartition(handle, maxChars)
        return json.decodeFromString(raw)
    }

    override fun close() {
        if (handle != 0L) {
            NotepadNativeJni.notepadDestroy(handle)
        }
    }
}

/** Multi-pass LLM synthesis over a filled [Notepad]. */
public class Synthesizer @PublishedApi internal constructor(
    @PublishedApi internal val handle: Long,
) : AutoCloseable {
    public constructor(notepad: Notepad, config: SynthesizerConfig) : this(
        NotepadNativeJni.synthesizerCreate(
            notepad.handle,
            json.encodeToString(config),
        ),
    )

    public fun synthesize(question: String): SynthesisOutcome {
        val raw = NotepadNativeJni.synthesizerSynthesize(handle, question)
        return json.decodeFromString(raw)
    }

    override fun close() {
        if (handle != 0L) {
            NotepadNativeJni.synthesizerDestroy(handle)
        }
    }
}
