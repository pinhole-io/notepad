package com.notepad.kt

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive

class NotepadJsonTest {
    private val json = Json { encodeDefaults = true }

    @Test
    fun `synthesizer config serializes to keys expected by native`() {
        val cfg =
            SynthesizerConfig(
                apiKey = "k",
                model = "m",
            )
        val s = json.encodeToString(cfg)
        assertTrue(s.contains("apiKey"))
        assertTrue(s.contains("maxContextChars"))
        assertTrue(s.contains("synthesisBudgetRatio"))
        val obj = Json.parseToJsonElement(s) as JsonObject
        assertEquals("k", obj["apiKey"]!!.jsonPrimitive.content)
    }

    @Test
    fun `render options serializes to keys expected by native`() {
        val opts = RenderOptions(format = "json", includeMetadata = false, maxChars = 100)
        val s = json.encodeToString(opts)
        assertTrue(s.contains("includeMetadata"))
        assertTrue(s.contains("maxChars"))
        val obj = Json.parseToJsonElement(s) as JsonObject
        assertEquals("json", obj["format"]!!.jsonPrimitive.content)
    }
}
