#!/usr/bin/env node
// Regenerates crates/kndo-adapter-js/src/node_builtins.txt from the authoritative source:
// Node's own `module.builtinModules`. Run against the OLDEST Node version kndo claims to
// understand, then union manually if a newer version ever ADDS a bare name (it should not:
// since ~v18 Node's policy is that new builtins are `node:`-prefix-only, which kndo handles
// structurally — this list is the frozen legacy set of bare-importable names).
//
// Usage: node scripts/gen-node-builtins.mjs > crates/kndo-adapter-js/src/node_builtins.txt

import { builtinModules } from "node:module";
import { execSync } from "node:child_process";

const version = execSync("node --version").toString().trim();
const names = builtinModules
  .filter((m) => !m.startsWith("node:")) // prefix-only builtins are handled structurally
  .sort();

console.log(`# Node bare-importable builtin modules — GENERATED, do not edit by hand.`);
console.log(`# Source: require('module').builtinModules, ${version}.`);
console.log(`# Regenerate: node scripts/gen-node-builtins.mjs > crates/kndo-adapter-js/src/node_builtins.txt`);
for (const name of names) console.log(name);
