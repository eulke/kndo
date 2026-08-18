# RFC 0001 — Core Architecture

**Status:** Draft · **Depends on:** — · **Depended on by:** all other RFCs

## 1. Overview

kondo is organized as a pipeline around one central data structure, the **Project Graph**:

```
┌────────────┐   ┌───────────────────┐   ┌───────────────┐   ┌────────────┐   ┌──────────┐
│  Discovery │ → │ Language Adapters │ → │ Project Graph │ → │  Analyses  │ → │ Reporting│
│ (walk fs,  │   │ (parse, extract   │   │ (files,       │   │ (reachab., │   │ (human,  │
│  git diff) │   │  facts per file)  │   │  symbols,     │   │  dup, CRAP,│   │  json,   │
│            │   │                   │   │  edges)       │   │  deps, ...)│   │  sarif)  │
└────────────┘   └───────────────────┘   └───────┬───────┘   └────────────┘   └──────────┘
                                                 │  ▲
                                            ┌────▼──┴────┐
                                            │   Cache    │  .kondo/ (content-addressed)
                                            └────────────┘
```

Everything left of the graph is **per-file and parallel**; everything right of the graph is
**whole-program**. The cache sits under the graph so that per-file work is skipped for unchanged
files and whole-program work is re-run only on the affected subgraph (RFC 0004).

## 2. Layering & the ignorance rule

```
kondo-cli          ── CLI, terminal UI, exit codes
kondo-core         ── graph model, analysis engine, cache, orchestration, plugin host
kondo-adapter-*    ── one crate per language (js, go, java, kotlin, swift, rust, json, css)
kondo-plugin-api   ── stable API surface for third-party plugins (WASM)
```

**The ignorance rule:** `kondo-core` must not contain the name of any language. It defines a
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
- `File` — a source file (path, content hash, language, flavor: production | test | generated | vendored)
- `Symbol` — a named declarable (function, method, type, class, const, css-rule…), owned by a File
- `Package` — a dependency declared in a manifest (name, version req, scope: prod | dev | build)
- `Manifest` — the declaring file of Packages (package.json, go.mod, Cargo.toml…)

**Edges**
- `File imports File` — module-level dependency (resolved by the adapter)
- `File imports Package` — external dependency usage
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

1. **Discovery** — enumerate candidate files (respecting `.gitignore` + kondo config), or take the
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
suite in CI from milestone M2 (ROADMAP).

## 6. Error philosophy

- A file that fails to parse degrades to an *opaque file node*: it keeps previous cached facts if
  any, else contributes no facts — and this is reported as a diagnostic, never a crash.
- Adapter/plugin panics are caught at the file boundary; one bad file cannot kill the run.
- kondo's own exit codes distinguish "findings" from "kondo failed" (RFC 0006 §5).

## 7. Alternatives considered

- **Reusing per-language tools and aggregating their output.** Rejected: N configs, N output
  formats, no shared graph → cannot answer cross-cutting questions (test-only reachability,
  blast-radius diffs), and cold starts of N processes blow the 500 ms budget.
- **Compiler-grade semantic analysis per language** (tsc API, gopls, javac…). Rejected for the
  core path: accuracy gains don't justify multi-second startup and per-language runtimes.
  Adapters may *optionally* shell out to native tooling in a future "deep mode" (post-1.0).
