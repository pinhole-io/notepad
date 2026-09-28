#!/usr/bin/env node
/** Register notepad tools on a superglue Client (offline-safe). */

import { createRequire } from "node:module";
import { registerOffloadHook, registerTools, isNotepadStub, synthesisUserPrompt, compactToolMessages } from "../facade.mjs";

const require = createRequire(import.meta.url);
const { Notepad } = require("..");
const { Client } = require("../../../superglue/superglue-js");

const client = new Client("sk-offline-demo", "gpt-5.4-nano-2026-03-17-mini");
const np = new Notepad();
np.store("demo note");

await registerTools(client, np);
await registerOffloadHook(client, np, {
  autoOffload: true,
  minChars: 10,
  previewChars: 50,
});

if (!isNotepadStub('{"notepad_ref":"x","storedChars":1}')) {
  throw new Error("stub detection failed");
}
if (!synthesisUserPrompt("Q?", "material").includes("Question")) {
  throw new Error("synthesis prompt failed");
}

const big = "x".repeat(5000);
const messages = [{ role: "tool", name: "demo_tool", content: big }];
const compacted = compactToolMessages(messages, np, {
  autoOffload: true,
  minChars: 100,
  previewChars: 50,
});
if (!isNotepadStub(compacted[0].content)) {
  throw new Error("compaction failed");
}

console.log("superglue_tools_demo ok");
