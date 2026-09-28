#!/usr/bin/env node
/** LLM synthesis demo — runs live only when OPENAI_API_KEY is set. */

import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

const require = createRequire(import.meta.url);
const { Notepad, Synthesizer } = require("..");

export async function main() {
  const np = new Notepad();
  np.store("Rust 1.88 stabilized edition 2024.");
  np.storeTagged(
    "Session scratchpads keep tool-loop context small.",
    ["pattern"],
    "architecture",
  );

  const apiKey = process.env.OPENAI_API_KEY ?? "";
  if (!apiKey) {
    console.log("Set OPENAI_API_KEY to run synthesis live; notepad demo data stored OK.");
    return;
  }

  const model = process.env.OPENAI_MODEL ?? "gpt-5.4-nano-2026-03-17-mini";
  const synth = new Synthesizer(np, apiKey, model);
  const outcome = await synth.synthesize("What did we collect? Answer in one sentence.");
  console.log(`passes=${outcome.passes}`);
  console.log(outcome.answer);
}

if (import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch((err) => {
    console.error(err);
    process.exit(1);
  });
}
