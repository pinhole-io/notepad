#!/usr/bin/env node
/** Copy libnotepad_bridge next to native .node artifacts for $ORIGIN rpath. */

import { copyFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const bridge = join(root, "..", "target", "debug", "libnotepad_bridge.so");
const dest = join(root, "libnotepad_bridge.so");

if (!existsSync(bridge)) {
  console.error(`missing bridge library: ${bridge}`);
  process.exit(1);
}
copyFileSync(bridge, dest);
console.log(`copied ${bridge} -> ${dest}`);
