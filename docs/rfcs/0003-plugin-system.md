# RFC 0003 — Plugin System

**Status:** Accepted · **Depends on:** RFC 0001, 0002 · **Normative contracts:** [contracts/core-traits.md](../contracts/core-traits.md), [contracts/wasm-abi.md](../contracts/wasm-abi.md) (the external-tier ABI's concrete v1 shape, adapters only — see that document's own scope notes)

## 1. Why plugins exist

The adapter boundary (RFC 0002) deliberately excludes everything a language *ecosystem* adds on
top of the language: frameworks, test runners, coverage formats, org conventions. Those things
change fast, are opinionated, and are optional — exactly what should be **pluggable and
composable** rather than baked into adapters. Plugins are how kndo learns them.

Guiding rule: **adapters describe what code *is*; plugins describe what an ecosystem *means* by it.**

## 2. Extension points

A plugin implements one or more of these hooks (trait `Plugin`, normative in contracts):

| Hook | Runs | Typical use |
|------|------|-------------|
| `classify_file` | discovery | adjust a file's role/origin beyond language defaults (e.g. `*.stories.tsx` → tooling) |
| `contribute_roots` | graph assembly | framework entry points: Next.js `pages/**`, Spring `@Component`, AWS Lambda handlers, `#[test]`-like macros of alt test frameworks |
| `contribute_edges` | graph assembly | edges invisible to the language: DI wiring, route-string → handler, Angular template → class, CSS class names used from HTML templates |
| `annotate_symbols` | graph assembly | mark symbols "externally consumed" (public SDK surface, FFI, serialization targets like `@JsonProperty`/serde fields) |
| `ingest_coverage` | pre-analysis | parse a coverage format (lcov, cobertura, JaCoCo, llvm-cov) into per-function coverage (RFC 0005 §10) |
| `suppress` | reporting | domain-specific suppression (e.g. migration files are exempt from dead-code) |

Plugins **cannot**: define new node/edge kinds, mutate other plugins' output, read arbitrary
files (they request file access through the host, which enforces scope), or veto core analyses.
This keeps the graph semantics owned by the core and results reproducible.

## 3. Packaging & distribution

Two tiers (decision in ADR 0003):

1. **Built-in plugins** — first-party, compiled into the binary, enabled by auto-detection or
   config. The initial set targets the dominant ecosystems of the supported languages
   (examples, each its own doc before implementation): `react`, `nextjs`, `jest/vitest`,
   `spring`, `junit`, `gradle-conventions`, `swiftui`, `coverage-lcov`, `coverage-jacoco`.
2. **External plugins** — WASM components implementing the same hooks over a versioned ABI
   (`kndo-plugin-api`), loaded from `.kndo/plugins/` or a configured path. Sandboxed (no fs/net;
   host-mediated file access), with per-file fuel/time limits so a plugin cannot break the 500 ms
   budget — a plugin that exceeds its budget is disabled for the run and reported as a diagnostic.
   **Shipped for `LanguageAdapter` (M5, v1 — [contracts/wasm-abi.md](../contracts/wasm-abi.md)):**
   `.kndo/plugins/*.wasm` adapters auto-discover through `kndo::open` and are indistinguishable
   from a compiled-in adapter to the `Engine`. The `Plugin` hooks below are not over WASM yet —
   their sink-based shape is a different, larger ABI surface than an adapter's three flat
   functions, and nothing has demanded it be built ahead of real external-plugin usage.

Both tiers use the same trait; built-ins are simply statically linked. Third parties can therefore
prototype a plugin natively and ship it as WASM unchanged.

## 4. Activation & configuration

- **Auto-detection**: a plugin declares detection predicates (e.g. "package.json depends on
  `react`", "a `build.gradle` exists"). Detected plugins activate silently; `kndo doctor` (RFC
  0006) shows what activated and why.
- **Explicit config** (`kndo.toml`) can force-enable/disable and pass plugin-scoped options:

```toml
[plugins.nextjs]
enabled = true            # override auto-detection
app-dir = "src/app"

[plugins.coverage-lcov]
report = "coverage/lcov.info"
max-age = "7d"            # stale reports are ignored (with a diagnostic), not trusted
```

## 5. Determinism & trust

- Plugin execution order is deterministic (topological by declared ordering constraints, then
  name). Same inputs ⇒ same graph ⇒ same findings.
- Plugin identity (name + version + content hash for WASM) participates in the cache key
  (RFC 0004 §3), so enabling/upgrading a plugin invalidates exactly what it influenced.
- External plugins are untrusted code: sandbox as above, and findings they influenced are
  attributed (`"sources": ["plugin:nextjs"]` in JSON output) for auditability.

## 6. What is *not* a plugin

- Languages (adapters have a richer contract: parsing, resolution, manifests).
- Output formats — reporting stays in the core for schema stability; new formats are core PRs.
- Custom *analyses* over the graph: deliberately post-1.0. First we stabilize the graph schema,
  then we can expose a query/analysis API safely. Tracked as an open question in the ROADMAP.
