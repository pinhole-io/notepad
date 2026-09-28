#!/usr/bin/env node
/** Pure in-memory notepad demo (no LLM). */

import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

const require = createRequire(import.meta.url);
const { Notepad } = require("..");

export function main() {
  const np = new Notepad();

  const id1 = np.store("First finding: Rust 1.88 is stable.");
  const id2 = np.storeTagged(
    { url: "https://example.com", title: "Example" },
    ["web", "crawl"],
    "docs page",
  );

  console.log(`session: ${np.sessionId}`);
  console.log(`entries: ${np.len()} (${np.totalChars()} chars total)`);
  console.log(`ids: ${id1}, ${id2}`);

  const digest = np.render();
  console.log(`\n--- digest ---\n${digest}`);

  const batches = np.partition(500);
  console.log(`\npartition (500 char budget): ${batches.length} batches`);
  console.log(JSON.stringify(batches, null, 2));
}

if (import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main();
}
