#!/usr/bin/env node
// Regenerates the js-ts adapter's stdlib dataset in `kndo-stdlib v1` format (the shared
// mechanism every adapter uses — kndo-adapter-toolkit::stdlib). Authoritative source: Node's
// own `module.builtinModules`. Bare-name builtins are a frozen set by Node's prefix-only
// policy for new builtins; the `node:` prefix is handled structurally in the adapter.
//
// Usage: node scripts/gen-stdlib-js.mjs > crates/kndo-adapter-js/src/stdlib.txt

import { builtinModules } from "node:module";
import { execSync } from "node:child_process";

const version = execSync("node --version").toString().trim();
const names = builtinModules.filter((m) => !m.startsWith("node:")).sort();

console.log("# kndo-stdlib v1");
console.log("# language: js-ts");
console.log("# source: node -p require('module').builtinModules");
console.log(`# source-version: ${version}`);
console.log("# regenerate: node scripts/gen-stdlib-js.mjs > crates/kndo-adapter-js/src/stdlib.txt");
for (const name of names) console.log(name);
