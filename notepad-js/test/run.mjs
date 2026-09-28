#!/usr/bin/env node
/** Smoke tests + execute every examples/*.mjs. */

import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { Notepad, version } = require("..");

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");
const examplesDir = join(root, "examples");

function assert(cond, msg) {
  if (!cond) {
    console.error(`FAIL: ${msg}`);
    process.exit(1);
  }
}

function smokeTest() {
  assert(typeof version() === "string" && version().length > 0, "version()");

  const np = new Notepad();
  const id = np.store("hello");
  assert(typeof id === "string" && id.length > 0, "store returns id");

  const entry = np.get(id);
  assert(entry && entry.payload === "hello", "get round-trip");

  const tagged = np.storeTagged({ k: 1 }, ["t"], "lbl");
  assert(np.get(tagged)?.label === "lbl", "storeTagged");

  assert(np.render().includes("hello"), "render");
  assert(np.partition(10).length >= 1, "partition");
  assert(np.len() === 2, "len");
  assert(np.totalChars() > 0, "totalChars");
  console.log("smoke: ok");
}

function runExamples() {
  const scripts = readdirSync(examplesDir)
    .filter((f) => f.endsWith(".mjs"))
    .sort();
  assert(scripts.length > 0, "examples exist");

  for (const script of scripts) {
    const path = join(examplesDir, script);
    const result = spawnSync(process.execPath, [path], {
      cwd: root,
      encoding: "utf8",
      env: process.env,
    });
    if (result.status !== 0) {
      console.error(`example ${script} failed (exit ${result.status})`);
      console.error(result.stdout);
      console.error(result.stderr);
      process.exit(1);
    }
    console.log(`example ${script}: ok`);
  }
}

smokeTest();
runExamples();
console.log("all tests passed");
