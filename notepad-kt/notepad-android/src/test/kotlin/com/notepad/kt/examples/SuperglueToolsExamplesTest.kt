package com.notepad.kt.examples

import com.notepad.kt.OffloadConfig
import com.notepad.kt.Notepad
import com.notepad.kt.compactToolMessages
import com.notepad.kt.isNotepadStub
import com.notepad.kt.registerOffloadHook
import com.notepad.kt.registerTools
import com.notepad.kt.synthesisUserPrompt
import com.superglue.kt.ClientConfig
import com.superglue.kt.SuperglueClient
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertTrue
import org.junit.Test

class SuperglueToolsExamplesTest {
    @Test
    fun stubDetection() {
        assertTrue(isNotepadStub("""{"notepad_ref":"550e8400-e29b-41d4-a716-446655440000"}"""))
        assertTrue(!isNotepadStub("""{"url":"https://example.com"}"""))
    }

    @Test
    fun synthesisPrompt() {
        val prompt = synthesisUserPrompt("What?", "facts")
        assertTrue(prompt.contains("What?"))
        assertTrue(prompt.contains("facts"))
    }

    @Test
    fun nativeHandleAndOffloadConfig() {
        Notepad().use { np ->
            assertTrue(np.nativeHandle > 0L)
            val cfg = OffloadConfig(autoOffload = true, minChars = 512)
            val json = cfg.toJson()
            assertTrue(json.contains("autoOffload"))
        }
    }

    @Test
    fun registerToolsNoThrow() {
        SuperglueClient.open(
            ClientConfig(apiKey = "sk-test", model = "gpt-5.4-nano-2026-03-17-mini"),
        ).use { client ->
            Notepad().use { np ->
                registerTools(client.toolsRegistryPtr(), np)
                registerOffloadHook(client.hooksRegistryPtr(), np)
            }
        }
    }

    @Test
    fun compactToolMessages() {
        Notepad().use { np ->
            val messagesJson =
                """[{"role":"tool","name":"t","content":"${"z".repeat(3000)}"}]"""
            val config = OffloadConfig(autoOffload = true, minChars = 100, previewChars = 40)
            val out = compactToolMessages(messagesJson, np, config)
            val content =
                Json.parseToJsonElement(out).jsonArray[0].jsonObject["content"]!!.jsonPrimitive.content
            assertTrue(isNotepadStub(content))
        }
    }
}
