#!/usr/bin/env node
/** Verify superglue-js native binding is built before integration tests. */

import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const sgDir = join(root, "..", "..", "superglue", "superglue-js");
const nodeFiles = [
  join(sgDir, "superglue-js.linux-x64-gnu.node"),
  join(sgDir, "superglue-js.linux-arm64-gnu.node"),
  join(sgDir, "superglue-js.darwin-x64.node"),
  join(sgDir, "superglue-js.darwin-arm64.node"),
];

if (!nodeFiles.some((f) => existsSync(f))) {
  console.error(
    "superglue-js native module not found.\n" +
      `Build it: cd ${sgDir} && npm install && npm run build:native:debug`,
  );
  process.exit(1);
}
