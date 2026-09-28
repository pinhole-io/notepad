#!/usr/bin/env node
/**
 * Integration tests for notepad + superglue tool registration.
 * Requires superglue-js native module built (see scripts/ensure-superglue.mjs).
 */

import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  Notepad,
  isNotepadStub,
  synthesisUserPrompt,
  offloadConfigFromObject,
  offloadConfigToObject,
  registerTools,
  registerOffloadHook,
  compactToolMessages,
} from "../facade.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

function assert(cond, msg) {
  if (!cond) {
    console.error(`FAIL: ${msg}`);
    process.exit(1);
  }
}

// --- helpers (mirrors notepad-py test_superglue_integration.py) ---

function testIsNotepadStub() {
  assert(
    isNotepadStub('{"notepad_ref":"550e8400-e29b-41d4-a716-446655440000"}'),
    "stub detection positive",
  );
  assert(!isNotepadStub('{"url":"https://example.com"}'), "stub detection negative");
  console.log("testIsNotepadStub: ok");
}

function testSynthesisUserPrompt() {
  const prompt = synthesisUserPrompt("What?", "facts here");
  assert(prompt.includes("facts here"), "material in prompt");
  assert(prompt.includes("What?"), "question in prompt");
  console.log("testSynthesisUserPrompt: ok");
}

function testOffloadConfigProtoRoundTrip() {
  const cfg = offloadConfigFromObject({ autoOffload: true, minChars: 512 });
  const obj = offloadConfigToObject(cfg);
  assert(obj.autoOffload === true, "autoOffload");
  assert(obj.minChars === 512, "minChars");
  console.log("testOffloadConfigProtoRoundTrip: ok");
}

async function testRegisterToolsNoThrow() {
  const sgPath = join(__dirname, "../../../superglue/superglue-js");
  let Client;
  try {
    ({ Client } = require(sgPath));
  } catch (e) {
    console.error(
      "superglue-js not available — run: cd ../../../superglue/superglue-js && npm run build:native:debug",
    );
    throw e;
  }
  const client = new Client("sk-test", "gpt-5.4-nano-2026-03-17-mini");
  const np = new Notepad();
  await registerTools(client, np);
  await registerOffloadHook(client, np);
  console.log("testRegisterToolsNoThrow: ok");
}

function testCompactToolMessages() {
  const np = new Notepad();
  const messages = [{ role: "tool", name: "t", content: "z".repeat(3000) }];
  const config = { autoOffload: true, minChars: 100, previewChars: 40 };
  const out = compactToolMessages(messages, np, config);
  assert(out.length === 1, "one message");
  assert(isNotepadStub(out[0].content), "content stubbed");
  console.log("testCompactToolMessages: ok");
}

testIsNotepadStub();
testSynthesisUserPrompt();
testOffloadConfigProtoRoundTrip();
await testRegisterToolsNoThrow();
testCompactToolMessages();
console.log("superglue_integration: all passed");
