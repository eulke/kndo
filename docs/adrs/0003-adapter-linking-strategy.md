# ADR 0003 — First-party adapters compiled in; third-party extensions via WASM

**Status:** Proposed · **Date:** 2026-08-18

## Context
Languages and plugins must be pluggable (RFC 0002/0003), but Rust has no stable native ABI, and
`dlopen` plugins would break the single-static-binary distribution story, complicate cross-platform
support, and execute untrusted code unsandboxed.

## Decision
Two tiers behind the same traits:
1. **Compiled-in** — the eight launch adapters and first-party plugins are crates statically
   linked into the `kndo` binary, selected/activated at runtime.
2. **WASM components** — third-party adapters and plugins target `kndo-plugin-api`, a versioned
   WIT/component-model ABI hosted by wasmtime, sandboxed (no ambient fs/net; host-mediated file
   access; fuel + time budgets).

The native traits are the source of truth; the WASM ABI is a generated bridge over them, so an
extension can be developed natively and shipped as WASM without rewrites.

## Consequences
- Hot path stays native and monomorphized — WASM cost is only paid by repos that add external
  extensions, and the sandbox protects the 500 ms budget (over-budget extension ⇒ disabled + diagnostic).
- The ABI must be versioned and conservative from day one; it ships in M5 (ROADMAP), after the
  contracts have survived several first-party adapters.
- "Pluggable" never means "recompile kndo": external languages are possible without a core PR —
  but first-party quality bar stays higher (conformance fixtures required for both tiers).

## Alternatives
`dlopen`/cdylib (ABI fragility, no sandbox); subprocess extensions with an IPC protocol (clean
isolation but per-file IPC overhead threatens the budget; kept as a fallback idea if WASM proves
limiting); everything-WASM including first-party (pointless overhead on the default path).
