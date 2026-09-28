package com.notepad.kt.examples

import com.notepad.kt.Notepad
import com.notepad.kt.RenderOptions
import com.notepad.kt.Synthesizer
import com.notepad.kt.SynthesizerConfig
import com.notepad.kt.version
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Runnable examples mirroring `notepad-py/examples` and `notepad-js/examples`.
 * Set `OPENAI_API_KEY` in the environment to run live synthesis sections.
 */
class NotepadExamplesTest {
    private val apiKey = System.getenv("OPENAI_API_KEY").orEmpty()
    private val model = System.getenv("OPENAI_MODEL") ?: "gpt-5.4-nano-2026-03-17-mini"

    @Test
    fun example_notepad_demo() {
        assertTrue(version().isNotEmpty())

        Notepad().use { np ->
            val id1 = np.store("First finding: Rust 1.88 is stable.")
            val id2 =
                np.storeTagged(
                    buildJsonObject {
                        put("url", "https://example.com")
                        put("title", "Example")
                    },
                    tags = listOf("web", "crawl"),
                    label = "docs page",
                )

            assertTrue(np.sessionId.isNotEmpty())
            assertEquals(2, np.length)
            assertTrue(np.totalChars > 0)

            val entry = np.get(id1)
            assertNotNull(entry)
            assertEquals(id1, entry!!.id)

            val digest = np.render(RenderOptions())
            assertTrue(digest.contains("First finding"))

            val batches = np.partition(500)
            assertTrue(batches.isNotEmpty())
            assertTrue(batches.flatten().contains(id2))
        }
    }

    @Test
    fun example_synthesize_demo() {
        Notepad().use { np ->
            np.store("Rust 1.88 stabilized edition 2024.")
            np.storeTagged(
                JsonPrimitive("Session scratchpads keep tool-loop context small."),
                tags = listOf("pattern"),
                label = "architecture",
            )

            if (apiKey.isEmpty()) {
                println("Set OPENAI_API_KEY to run synthesis live.")
                return
            }

            Synthesizer(
                np,
                SynthesizerConfig(apiKey = apiKey, model = model),
            ).use { synth ->
                val outcome = synth.synthesize("What did we collect? Answer in one sentence.")
                assertTrue(outcome.answer.isNotEmpty())
                assertTrue(outcome.passes >= 1)
            }
        }
    }
}
