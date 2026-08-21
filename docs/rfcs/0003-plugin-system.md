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
| `classify_file` | graph assembly, phase 2 | adjust a file's role/origin beyond language defaults (e.g. `*.stories.tsx` → tooling) |
| `contribute_roots` | graph assembly, after phase 3b | framework entry points: Next.js `pages/**`, Spring `@Component`, AWS Lambda handlers, `#[test]`-like macros of alt test frameworks |
| `contribute_edges` | graph assembly, after phase 3b | edges invisible to the language: DI wiring, route-string → handler, Angular template → class, CSS class names used from HTML templates |
| `annotate_symbols` | graph assembly, after phase 3b | mark symbols "externally consumed" (public SDK surface, FFI, serialization targets like `@JsonProperty`/serde fields) |
| `ingest_coverage` | pre-analysis | parse a coverage format (lcov, cobertura, JaCoCo, llvm-cov) into per-function coverage (RFC 0005 §10) |
| `suppress` | reporting | domain-specific suppression (e.g. migration files are exempt from dead-code) |

Plugins **cannot**: define new node/edge kinds, mutate other plugins' output, read arbitrary
files (they request file access through the host, which enforces scope), or veto core analyses.
This keeps the graph semantics owned by the core and results reproducible.

**Landed (M5).** The first four hooks are wired into `graph::assemble_from_source`, not just
declared on the trait: `classify_file` runs inline in phase 2's file-node build, right after RFC
0012 §7's content-derived origin correction and before role-derived roots — a plugin's answer is
what every downstream role/origin exemption sees. The other three run once, together, right
after phase 3b's reference-resolution merge (symbol tables and every adapter-emitted reference
are both stable by then) and before the canonical edge sort, through a read-only `GraphView`
(borrows the graph's own `files`/`symbols` vectors, no copy) and three typed sinks
(`RootSink`/`EdgeSink`/`AnnotationSink`). A plugin never names a target by internal id — every
sink call takes a `PluginTarget` (a `ProjectPath` plus an optional bare-or-`Owner.name` symbol
name), resolved core-side against the same bare/qualified lookup tables `RawRoot`/`RawReference`
already resolve against; an unresolvable target is dropped silently, the same miss behavior an
adapter's own facts already have. `contribute_edges` only ever produces a `References` edge
(plugins can't mint new edge kinds, per this section's own rule above); `annotate_symbols`'
marks land in a new `ProjectGraph::externally_consumed: Vec<SymbolId>` field, consumed as the
exemption RFC 0005 §7 already documented for `internal-only`/`private-type-leak` before there
was anything to populate it. Every contributed edge/root/annotation is attributed
`Provenance::Plugin(id)`.

`suppress` remains undeclared/unwired — no analysis calls it yet; tracked as an open item, not
folded into "landed" above.

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
   **Shipped for both `LanguageAdapter` and `Plugin` (M5, v1 —
   [contracts/wasm-abi.md](../contracts/wasm-abi.md)):** `.kndo/plugins/*.wasm` adapters and
   plugins auto-discover through `kndo::open` from the *same* directory — each `.wasm` file is
   tried against both loaders, and wasmtime's own component type-checking rejects whichever
   doesn't match, so nothing needs a naming convention to say which ABI a file targets. Both are
   indistinguishable from a compiled-in adapter/plugin to the `Engine`. `Plugin`'s bridge is
   bidirectional (the guest calls back into two host-provided graph queries,
   `wasm-abi.md` §5.1) — a materially different shape from the adapter ABI's one-directional
   three flat functions, built once real internal demand existed for it (this session), not
   speculatively ahead of it.

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
  name). Same inputs ⇒ same graph ⇒ same findings. **Landed interim rule (M5):** `PluginDescriptor`
  has no ordering-constraints field yet, so `assemble_from_source` sorts registered plugins by
  `id` once and reuses that order for every hook — real, but not yet the full topological rule
  this section names; a plugin depending on another's contribution being visible through
  `GraphView` needs the ordering-constraints field before that's expressible. Open item, not
  silently assumed solved.
- Plugin identity (name + version + content hash for WASM) participates in the cache key
  (RFC 0004 §3), so enabling/upgrading a plugin invalidates exactly what it influenced. **Not
  implemented yet** — landed instead (M5), and strictly sufficient for correctness today: any
  registered plugin (`classify_file`/`contribute_roots`/`contribute_edges`/`annotate_symbols`,
  none of which `LcovPlugin` — the only shipped plugin — implements) makes
  `assemble_from_source` skip *both* the graph-snapshot cache and the incremental patch
  entirely, full-rebuilding every run instead. Neither reuse path re-invokes a plugin's hooks, so
  serving either would silently miss whatever a currently-registered plugin contributes; bypass
  is the correct fallback until cache-key folding lands, and costs nothing today since it never
  triggers for the default product. Revisit once a real plugin with these hooks ships and warm
  performance matters for it.
- External plugins are untrusted code: sandbox as above, and findings they influenced are
  attributed (`"sources": ["plugin:nextjs"]` in JSON output) for auditability. The
  `Provenance::Plugin(id)` attribution itself is landed (every edge/root/annotation a plugin
  contributes carries it); JSON output surfacing it as `sources` is a separate, not-yet-done
  rendering step.

## 6. What is *not* a plugin

- Languages (adapters have a richer contract: parsing, resolution, manifests).
- Output formats — reporting stays in the core for schema stability; new formats are core PRs.
- Custom *analyses* over the graph: deliberately post-1.0. First we stabilize the graph schema,
  then we can expose a query/analysis API safely. Tracked as an open question in the ROADMAP.
