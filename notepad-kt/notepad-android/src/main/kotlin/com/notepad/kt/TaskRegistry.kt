package com.notepad.kt

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

private val json = Json {
    encodeDefaults = true
    ignoreUnknownKeys = true
}

@Serializable
public data class LifeTaskNote(
    public val id: String,
    public val content: String,
    @SerialName("created_at")
    public val createdAt: String,
)

@Serializable
public data class LifeTask(
    public val id: String,
    public val title: String,
    public val description: String? = null,
    public val status: String,
    public val tags: List<String> = emptyList(),
    public val notes: List<LifeTaskNote> = emptyList(),
    @SerialName("created_at")
    public val createdAt: String,
    @SerialName("updated_at")
    public val updatedAt: String,
    @SerialName("completed_at")
    public val completedAt: String? = null,
    @SerialName("due_at")
    public val dueAt: String? = null,
    @SerialName("deleted_at")
    public val deletedAt: String? = null,
    public val revision: Long = 0,
)

@Serializable
private data class ListReq(
    public val status: String? = null,
    public val tag: String? = null,
    @SerialName("dueBefore")
    public val dueBefore: String? = null,
    public val limit: Int? = null,
)

@Serializable
private data class CreateReq(
    public val title: String,
    public val description: String? = null,
    public val tags: List<String> = emptyList(),
    public val status: String? = null,
    @SerialName("dueAt")
    public val dueAt: String? = null,
)

@Serializable
private data class CreateResp(
    @SerialName("taskId")
    public val taskId: String,
)

@Serializable
private data class UpdateReq(
    @SerialName("taskId")
    public val taskId: String,
    public val title: String? = null,
    public val description: String? = null,
    public val tags: List<String>? = null,
    @SerialName("clearDescription")
    public val clearDescription: Boolean = false,
    @SerialName("dueAt")
    public val dueAt: String? = null,
    @SerialName("clearDueAt")
    public val clearDueAt: Boolean = false,
)

@Serializable
private data class SetStatusReq(
    @SerialName("taskId")
    public val taskId: String,
    public val status: String,
)

@Serializable
private data class AddNoteReq(
    @SerialName("taskId")
    public val taskId: String,
    public val content: String,
)

@Serializable
private data class AddNoteResp(
    @SerialName("noteId")
    public val noteId: String,
)

/** Durable life-wide personal task store backed by a JSON registry file. */
public class TaskRegistry @PublishedApi internal constructor(
    @PublishedApi internal val handle: Long,
    public val path: String,
) : AutoCloseable {
    public constructor(path: String) : this(NotepadNativeJni.taskRegistryCreate(path), path)

    public fun save() {
        NotepadNativeJni.taskRegistrySave(handle)
    }

    public fun reload() {
        NotepadNativeJni.taskRegistryReload(handle)
    }

    public fun list(
        status: String? = null,
        tag: String? = null,
        dueBefore: String? = null,
        limit: Int? = null,
    ): List<LifeTask> {
        val raw = NotepadNativeJni.taskRegistryList(
            handle,
            json.encodeToString(ListReq(status, tag, dueBefore, limit)),
        )
        return json.decodeFromString(raw)
    }

    public fun get(taskId: String): LifeTask {
        val raw = NotepadNativeJni.taskRegistryGet(handle, taskId)
        return json.decodeFromString(raw)
    }

    public fun create(
        title: String,
        description: String? = null,
        tags: List<String> = emptyList(),
        status: String? = null,
        dueAt: String? = null,
    ): String {
        val raw = NotepadNativeJni.taskRegistryCreateTask(
            handle,
            json.encodeToString(CreateReq(title, description, tags, status, dueAt)),
        )
        return json.decodeFromString<CreateResp>(raw).taskId
    }

    public fun update(
        taskId: String,
        title: String? = null,
        description: String? = null,
        tags: List<String>? = null,
        dueAt: String? = null,
        clearDescription: Boolean = false,
        clearDueAt: Boolean = false,
    ) {
        NotepadNativeJni.taskRegistryUpdate(
            handle,
            json.encodeToString(
                UpdateReq(taskId, title, description, tags, clearDescription, dueAt, clearDueAt),
            ),
        )
    }

    public fun setStatus(taskId: String, status: String) {
        NotepadNativeJni.taskRegistrySetStatus(
            handle,
            json.encodeToString(SetStatusReq(taskId, status)),
        )
    }

    public fun addNote(taskId: String, content: String): String {
        val raw = NotepadNativeJni.taskRegistryAddNote(
            handle,
            json.encodeToString(AddNoteReq(taskId, content)),
        )
        return json.decodeFromString<AddNoteResp>(raw).noteId
    }

    public fun delete(taskId: String) {
        NotepadNativeJni.taskRegistryDelete(handle, taskId)
    }

    override fun close() {
        if (handle != 0L) {
            NotepadNativeJni.taskRegistryDestroy(handle)
        }
    }
}
