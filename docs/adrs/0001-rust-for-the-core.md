# ADR 0001 — Rust for the core

**Status:** Accepted · **Date:** 2026-08-18

## Context
kondo's defining constraint is a < 500 ms warm run (pre-commit path) over multi-thousand-file
repos, distributed as a tool users install once and trust everywhere (macOS/Linux/Windows, CI
containers, developer laptops).

## Decision
Implement kondo in Rust: single static binary, no runtime, rayon for data-parallel extraction,
first-class tree-sitter bindings, mature WASM hosting (wasmtime) for the plugin tier, memory
safety for a tool that parses untrusted input.

## Consequences
- Fast cold starts (~ms) — essential for pre-commit; trivial distribution (one file per platform,
  installable via cargo/brew/npm shim/curl).
- Adapter/plugin authors targeting the WASM tier can use other languages; only first-party
  (compiled-in) adapters must be Rust.
- Slower iteration than a scripting language — mitigated by settling contracts in docs first
  (this repo's process) and by the conformance-fixture harness catching regressions cheaply.

## Alternatives
Go (fine, but weaker parser/WASM ecosystem and no rayon-grade data parallelism story);
TypeScript/Node (startup + memory costs incompatible with the 500 ms contract, would also make
the analyzer share a runtime with the code it analyzes).
