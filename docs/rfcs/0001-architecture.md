# RFC 0001 — Core Architecture

**Status:** Accepted · **Depends on:** — · **Depended on by:** all other RFCs

## 1. Overview

kndo is organized as a pipeline around one central data structure, the **Project Graph**:

```
┌────────────┐   ┌───────────────────┐   ┌───────────────┐   ┌────────────┐   ┌──────────┐
│  Discovery │ → │ Language Adapters │ → │ Project Graph │ → │  Analyses  │ → │ Reporting│
│ (walk fs,  │   │ (parse, extract   │   │ (files,       │   │ (reachab., │   │ (human,  │
│  git diff) │   │  facts per file)  │   │  symbols,     │   │  dup, CRAP,│   │  json,   │
│            │   │                   │   │  edges)       │   │  deps, ...)│   │  sarif)  │
└────────────┘   └───────────────────┘   └───────┬───────┘   └────────────┘   └──────────┘
                                                 │  ▲
                                            ┌────▼──┴────┐
                                            │   Cache    │  .kndo/ (content-addressed)
                                            └────────────┘
```

Everything left of the graph is **per-file and parallel**; everything right of the graph is
**whole-program**. The cache sits under the graph so that per-file work is skipped for unchanged
files and whole-program work is re-run only on the affected subgraph (RFC 0004).

## 2. Layering & the ignorance rule

```
kndo-cli           ── one frontend: terminal UI, exit codes, human rendering (RFC 0009)
kndo               ── the DISTRIBUTION layer: the composed product (core + all first-party
                      adapters + built-in plugins), one `open()` for every frontend
kndo-core          ── the system: graph model, analysis engine, cache, orchestration, plugin host
kndo-adapter-*     ── one crate per language (js, go, java, kotlin, swift, rust, json, css)
kndo-plugin-api    ── stable API surface for third-party plugins (WASM)
```

`kndo-core` is a library; the CLI is one frontend among future ones (`kndo serve`/MCP, LSP,
GUI, CI actions) and holds **zero** analysis logic. All frontends consume the same `Engine`
facade (contracts §5): the core never prints, frontends never compute. Machine output (JSON,
SARIF) is serialized core-side so every frontend emits identical data; only *human* rendering
is frontend-owned.

**Which languages the product ships is distribution knowledge, not frontend knowledge.** The
`kndo` crate owns the composition — it depends downward on the core *and* on every first-party
adapter (feature-gated for slim embedder builds, ADR 0006) and hands frontends a single
`kndo::open()`. Frontends therefore never name a language, can never ship a kndo missing one,
and adding a language touches exactly one crate. The ignorance rule is preserved: the core
still depends on no adapter — composition happens *above* both.

**The ignorance rule:** `kndo-core` must not contain the name of any language. It defines a
language-neutral vocabulary — `SourceFile`, `Symbol`, `Reference`, `Root`, `ManifestDependency` —
and adapters translate language reality into that vocabulary. If implementing a feature requires
`if language == X` in the core, the vocabulary is missing a concept and must be extended instead.

Conversely, adapters own only what the **language specification** defines: syntax, module
resolution rules, visibility/export semantics, canonical entry points, the language's manifest
format(s). Anything conventional or ecosystem-specific — framework magic (Spring DI, React
components referenced by JSX, dependency-injection containers), test-framework detection beyond
the standard library, coverage formats — belongs to **plugins** (RFC 0003), which enrich adapter
output rather than fork it.

## 3. The Project Graph

The graph is the language-neutral model of the project. Node and edge kinds (normative definition
in [contracts/core-traits.md](../contracts/core-traits.md)):

**Nodes**
- `File` — a source file (path, content hash, language, role: production | test | tooling,
  origin: authored | generated | vendored — two orthogonal axes)
- `Symbol` — a named declarable (function, method, type, class, const, css-rule…), owned by a File
- `Dependency` — an external dependency declared in a manifest (name, version req, scope: prod | dev | build | peer | optional)
- `Package` — a workspace unit: one manifest + the file tree it governs; every File is owned by exactly one Package (RFC 0011)
- `Manifest` — the declaring file of Dependencies and Package identity (package.json, go.mod, Cargo.toml…)

**Edges**
- `File imports File` — module-level dependency (resolved by the adapter; may cross Packages)
- `File imports Dependency` — external dependency usage
- `Package depends-on Package` — derived by the core from cross-package edges and manifests
- `Symbol references Symbol` — call/use/extend/implement/type-reference
- `File declares Symbol`
- `Root → Symbol | File` — entry-point marking (bin main, exported public API, framework handler,
  test root), with a `RootKind ∈ {production, test, tooling}`
- Every edge carries a `Confidence ∈ {certain, probable, possible}` (dynamic constructs demote
  confidence, RFC 0002 §5).

Analyses (RFC 0005) are pure functions over this graph plus optional enrichments (coverage data,
plugin-provided roots). They never read source text — if an analysis needs a fact, the fact
becomes part of the extraction contract.

## 4. Execution model

1. **Discovery** — enumerate candidate files (respecting `.gitignore` + kndo config), or take the
   changed set from `--staged` / `--diff <ref>`. Output: file list + content hashes (blake3).
2. **Extraction** (parallel, rayon) — for each file whose hash is not in cache: adapter parses
   (tree-sitter, ADR 0002) and emits `FileFacts` (declarations, references, imports, roots,
   complexity per function, duplication fingerprints). Cached facts are loaded for unchanged files.
3. **Graph assembly** — resolve imports/references into edges (adapters provide resolvers; the
   core provides the resolution driver). Plugins may add/annotate nodes and edges here.
4. **Analysis** — run enabled analyses over the graph. In incremental mode, only the *dirty
   region* is recomputed: changed nodes plus their forward/reverse closure (RFC 0004 §5).
5. **Reporting** — findings are diffed against the previous snapshot and the baseline; output is
   rendered for the selected audience (RFC 0006).

## 5. Concurrency & performance budget

Target: warm incremental p95 **< 500 ms** on a 5k-file repo (pre-commit path).
Validated empirically by [spike 0001](../spikes/0001-performance.md): measured warm composite
~75 ms on 4 cores — 6.8× headroom.

| Phase | Budget (warm, small diff) | Notes |
|-------|--------------------------|-------|
| Process start + config | 20 ms | single static binary, no runtime deps |
| Discovery + hashing | 80 ms | hash only stat-changed files; git index for `--staged` |
| Extraction | 100 ms | only changed files re-parsed; tree-sitter is incremental-friendly |
| Cache load (graph) | 100 ms | memory-mappable snapshot, ADR 0004 |
| Graph patch + analyses | 150 ms | dirty-region recomputation only |
| Reporting | 50 ms | |

Cold full runs are allowed seconds (parallel across cores) — they build the cache that makes every
subsequent run warm. The 500 ms contract is for the *warm* path and is enforced by a benchmark
suite in CI from milestone M2 (ROADMAP). The full parallelism model — per-phase strategy,
determinism under any thread count, adaptive sequential fallback, and the CI performance gates —
is specified in [RFC 0008](0008-performance-and-parallelism.md).

## 6. Error philosophy

- A file that fails to parse degrades to an *opaque file node*: it keeps previous cached facts if
  any, else contributes no facts — and this is reported as a diagnostic, never a crash.
- Adapter/plugin panics are caught at the file boundary; one bad file cannot kill the run.
- kndo's own exit codes distinguish "findings" from "kndo failed" (RFC 0006 §5).

## 7. Alternatives considered

- **Reusing per-language tools and aggregating their output.** Rejected: N configs, N output
  formats, no shared graph → cannot answer cross-cutting questions (test-only reachability,
  blast-radius diffs), and cold starts of N processes blow the 500 ms budget.
- **Compiler-grade semantic analysis per language** (tsc API, gopls, javac…). Rejected for the
  core path: accuracy gains don't justify multi-second startup and per-language runtimes.
  Adapters may *optionally* shell out to native tooling in a future "deep mode" (post-1.0).
