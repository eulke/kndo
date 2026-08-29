# Architecture

Core system design: the project graph pipeline (RFC 0001), the language-adapter contract
(RFC 0002), and the uniform component model unifying adapters and plugins (RFC 0016). Each
section below was originally its own RFC document; they are merged here per the consolidation
recorded in `.wayfinder/tickets/33-consolidation-decision.md`.

## RFC 0001: Architecture

**Status:** Accepted · **Depends on:** — · **Depended on by:** all other RFCs

### 1. Overview

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

### 2. Layering & the ignorance rule

```
kndo-cli           ── one frontend: terminal UI, exit codes, human rendering (RFC 0009)
kndo               ── the DISTRIBUTION layer: the composed product (core + all first-party
                      adapters + built-in plugins), one `open()` for every frontend
kndo-core          ── the system: graph model, analysis engine, cache, orchestration, plugin host
kndo-adapter-*     ── one crate per language (js, go, java, kotlin, swift, rust, json, css, html)
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

### 3. The Project Graph

The graph is the language-neutral model of the project. Node and edge kinds (normative definition
in [contracts/core-traits.md](CONTRACTS.md)):

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

### 4. Execution model

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

### 5. Concurrency & performance budget

Target: warm incremental p95 **< 500 ms** on a 5k-file repo (pre-commit path).
Validated empirically by [spike 0001](PERFORMANCE-WORKSPACES-AND-RELEASE.md): measured warm composite
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
subsequent run warm. The 500 ms contract is for the *warm* path. The phase breakdown above is the
RFC 0004 §4-5 patch/dirty-region path, and it is built and landed: [RFC 0013](GRAPH-CACHE-AND-ANALYSES.md)
(`crates/kndo-core/src/graph/patch.rs`) makes "Graph patch + analyses" a real dirty-region
recomputation — changed nodes plus their forward/reverse closure — not a full recompute. A
benchmark regression gate exists (`cargo xtask bench --gate`, CONTRIBUTING "Benchmarks"),
comparing warm end-to-end wall time at 1k/5k/50k files against a recorded baseline; it is
deliberately *not* wired into CI — the baseline is machine-specific, so ephemeral runners of
varying hardware would fail it for reasons unrelated to any change — and instead runs locally,
by hand, before and after a change expected to cost time. The full parallelism model —
per-phase strategy,
determinism under any thread count, adaptive sequential fallback, and the CI performance gates —
is specified in [RFC 0008](PERFORMANCE-WORKSPACES-AND-RELEASE.md).

### 6. Error philosophy

- A file that fails to parse degrades to an *opaque file node*: it keeps previous cached facts if
  any, else contributes no facts — and this is reported as a diagnostic, never a crash.
- Adapter/plugin panics are caught at the file boundary; one bad file cannot kill the run.
- kndo's own exit codes distinguish "findings" from "kndo failed" (RFC 0006 §5).

### 7. Alternatives considered

- **Reusing per-language tools and aggregating their output.** Rejected: N configs, N output
  formats, no shared graph → cannot answer cross-cutting questions (test-only reachability,
  blast-radius diffs), and cold starts of N processes blow the 500 ms budget.
- **Compiler-grade semantic analysis per language** (tsc API, gopls, javac…). Rejected for the
  core path: accuracy gains don't justify multi-second startup and per-language runtimes.
  Adapters may *optionally* shell out to native tooling in a future "deep mode" (post-1.0).

## RFC 0002: Language adapters

**Status:** Accepted · **Depends on:** RFC 0001 · **Normative contract:** [contracts/core-traits.md](CONTRACTS.md)

### 1. Purpose

A **language adapter** is the only component that understands a language. It translates source
files into the core's language-neutral vocabulary. The core discovers adapters through a registry
and treats them uniformly; adding a language is adding one crate that implements one trait.

### 2. Responsibilities (exactly these, no more)

An adapter owns what the **language specification and its standard toolchain** define:

1. **Claiming files** — which extensions/filenames it handles (`.ts`, `go.mod`, `BUILD.gradle.kts`…),
   including classifying each file on two orthogonal axes — *role* (production / test / tooling:
   `_test.go`, `*.spec.ts`) and *origin* (authored / generated / vendored: headers like
   `// Code generated … DO NOT EDIT`).
2. **Parsing** — producing a syntax tree (tree-sitter grammar, ADR 0002) and surviving broken code.
3. **Extraction** — emitting `FileFacts`:
   - declared symbols (name, kind, span, visibility, exported?)
   - references (identifier uses with enough context for resolution)
   - imports (raw specifier + kind: relative, package, stdlib)
   - language-defined roots (`main` functions, `pub` API of a library crate, exported members of
     an npm package's `main`/`exports`, `@main`/top-level code in Swift…)
   - per-function cyclomatic complexity (for CRAP, RFC 0005 §10)
   - normalized token streams per function/block (for duplicate detection, RFC 0005 §6)
4. **Resolution** — mapping an import specifier or a reference to its target, given the graph
   assembly context (e.g. Node resolution algorithm incl. `tsconfig` paths; Go module paths; Java
   package/classpath conventions; Cargo module tree).
5. **Manifests** — parsing the language's dependency manifests (`package.json`, `go.mod`,
   `Cargo.toml`, Gradle version catalogs, `Package.swift`) into `ManifestDependency` nodes, and
   mapping import specifiers → package names (e.g. `lodash/fp` → `lodash`, `golang.org/x/net/html`
   → module).

**Explicitly out of adapter scope** (goes to plugins, RFC 0003): framework conventions (a React
component "used" via JSX by a router config; Spring beans; SwiftUI previews), test frameworks
beyond the standard library/dominant convention, coverage formats, org-specific entry points.

Rationale for the boundary: language specs are stable and versioned; ecosystems are fashion.
Keeping fashion out of adapters keeps them small, testable, and slow-changing.

### 3. Non-source languages (JSON, CSS, HTML)

The vocabulary must not assume "code". For data/style languages the mapping is:

- **JSON**: files claimed only when *referenced* semantics exist (e.g. imported by JS/TS, listed
  in a manifest). Symbols are not extracted; JSON participates as import *targets* so file-level
  `unused` findings cover config/data files. Well-known manifests (`package.json`, `tsconfig.json`) are
  claimed by the *owning* adapter instead.
- **CSS/SCSS/LESS**: symbols are selectors/mixins/variables; references are `@import`/`@use`,
  `composes`, and — via the cross-language edge mechanism (§4) — class-name usage from JS/TS/HTML.
  This enables "unused CSS rule" as a normal unused-symbol finding.
- **HTML**: no symbols, no visibility ladder, no metrics — a document declares nothing a caller
  can name, the same non-source posture as JSON. Unlike JSON, a document is never an import
  *target*: nothing imports a page, so every claimed `.html`/`.htm` file roots itself (a browser
  loads it, a server renders it, a bundler is handed it), and the scripts/stylesheets/assets it
  names via `<script src>`, `<link href>`, `<img src>`, `<source src>`, `<iframe src>` become
  reachable through that root. A tag scan, not a grammar-backed parse: HTML's error recovery
  means a "malformed" document is still one a browser renders, so the adapter reads attribute
  values directly and under-reports (skips) anything it cannot read plainly rather than guessing.

### 4. Cross-language edges

Real projects cross language boundaries (TS imports a CSS module; JS reads a JSON file; Kotlin and
Java in one Gradle module). Adapters never call each other. Instead, an adapter emits an import
with a raw specifier; the **core's resolution driver** asks *each* registered adapter's resolver
whether it can resolve that specifier to a file it claims. First unambiguous claim wins; ambiguity
demotes the edge to `probable`.

### 5. Confidence & dynamic constructs

Static analysis of dynamic features must degrade honestly, not guess:

- `certain` — spec-level static resolution (a Go import, a Rust `use`, a TS named import).
- `probable` — resolution relied on convention or a single plausible candidate (string literal in
  `require(x)` with a resolvable literal value; duck-typed method with one candidate).
- `possible` — dynamic construct detected but not resolvable (`import(variable)`, reflection,
  `eval`). The adapter emits a **wildcard edge** from the file to *unknown* instead of guessing
  a target.

This is per-edge strength; how many edges of differing strength combine into a node's overall
reachability color and confidence — including how wildcard edges fold in — is the tiered
algorithm in RFC 0005 §1. Findings inherit the *weakest* confidence on their evidence path and
report it (RFC 0006).

### 6. Adapter lifecycle & versioning

- Adapters implement the `LanguageAdapter` trait (normative in contracts/core-traits.md) and
  register capabilities: claimed globs, manifest patterns, grammar version, **facts schema version**.
- The facts schema version participates in the cache key (RFC 0004 §3): bumping it invalidates
  only that adapter's cached facts.
- First-party adapters are compiled into the binary; the same trait is bridged to WASM for
  third-party adapters (ADR 0003). An adapter must not perform I/O beyond the file content handed
  to it — all filesystem access goes through the core (determinism, sandboxability, testability).
- **Version-dependent language data is generated data, never hand-maintained code — through the
  shared mechanism, not per-adapter improvisation.** Facts that track a language/runtime release
  cadence — stdlib/builtin module lists (Node builtins, Go packages, Java modules), reserved
  words, version-gated syntax tables — ship as **`kndo-stdlib v1`** data files (toolkit `stdlib`
  module: one format, one loader, one validation, provenance headers surfaced by `kndo doctor`),
  embedded at build time and produced by **one generic generator** — `cargo xtask gen-stdlib
  <language>` — where each language is a *table entry* (source command, version command,
  exclusion prefixes), never a per-language script. The generator validates its output with the
  same loader that consumes it at build time, and queries the authoritative source
  (`module.builtinModules`, `go list std`, `java --list-modules`). A new runtime version means
  regenerating a file; a new language means one table row — never new tooling, never editing
  adapter code. The bare-specifier **precedence is
  written once in the toolkit**, never re-derived per language: (1) structural stdlib signal
  (Node's `node:` prefix — unambiguous by the language's own construction, version-proof) >
  (2) manifest-declared dependency (declared intent beats shipped data — the userland `punycode`
  package is real) > (3) the stdlib list > (4) external dependency. An adapter supplies only
  what it alone knows: the structural check and the subpath→package mapping. Never query the
  *ambient* installed runtime at analysis time: that would make findings depend on the machine,
  violating determinism (RFC 0008 §4) — generators run at kndo development time, not at the
  user's analysis time.

### 7. Per-language notes (initial scope)

| Language | Resolution highlights | Roots (language-defined) | Test-role detection |
|----------|----------------------|--------------------------|----------------------|
| JS/TS | Node ESM+CJS, `tsconfig` paths/baseUrl, package.json `exports`; JSX/TSX | package entry points (`main`, `exports`, `bin`), scripts referenced files | `*.test.*`, `*.spec.*`, `__tests__/` |
| Go | Go modules, internal/ visibility | `main.main`, exported identifiers of library modules, `init` | `_test.go` |
| Java | package + source roots (Maven/Gradle layout) | `public static void main`, public API of published modules | `src/test/` |
| Kotlin | as Java + top-level functions, multiplatform source sets (post-1.0) | `main`, public API | `src/test/`, `commonTest` |
| Swift | SPM targets, module imports | `@main`, top-level code in `main.swift`, public API of library targets | `Tests/` targets |
| Rust | module tree from crate roots, `use`/paths, features (coarse: any-feature = live) | `main`, `lib.rs` `pub` API, `#[no_mangle]`/`export` | `#[cfg(test)]`, `tests/` |
| JSON | n/a (target-only) | n/a | n/a |
| CSS | `@import`/`@use` graph, CSS Modules | none (reachability comes from consumers) | n/a |
| HTML | none — a tag scan (`<script src>`, `<link href>`, `<img src>`, `<source src>`, `<iframe src>`), not module resolution | the whole file (a document is an entry point, not a module) | n/a |

Each adapter gets its own detailed spec section under `internal/ADAPTERS.md` before its
implementation milestone (ROADMAP), including the tricky cases above — HTML's included, since
the section covering it now exists alongside the other eight.

### 8. Testing contract

Every adapter ships **conformance fixtures**: a miniature project + the expected `FileFacts` and
expected findings, executed by a shared test harness in the core. This doubles as the compliance
suite third-party adapters run against.

## RFC 0016: Uniform component model

**Status:** Accepted (design), phased (§8) · **Depends on:** RFC 0002 (adapter contract),
RFC 0003 (plugin system), RFC 0004 (cache), RFC 0015 (identity & installation), ADR 0003
(linking strategy), ADR 0006 (single binary, zero-config) · **Ships:** post-1.0, except the
freeze reservations in §8 phase 0, which must land in M6

### 1. The question this RFC answers

With RFC 0015 fully landed, the extension story has two visible seams:

1. **Capability gap.** The first real convention plugins (`kndo:nextjs`, `kndo:express`)
   documented concrete detections as out of scope *solely because plugins cannot read file
   content*: express's `package.json` `main`/`scripts` parsing (its spec names host-mediated
   content access as "the right long-term fix"), nextjs's `pageExtensions`, route-string →
   page edges, `res.render` → template edges. The capability exists in the descriptor
   (`requested_file_access`, RFC 0003 §2) but is plumbed for `ingest_coverage` (native and,
   via the `coverage-ingester` world, WASM) only.
2. **Asymmetry gap.** Plugins now have coordinate identity, activation rules, dependencies, a
   global tier, and an installer. `LanguageAdapter`s have none of those — a stated gap in RFC
   0003 §4. An external adapter works only hand-dropped per-project; it cannot be installed,
   depended on, or gated.

Behind both sits a strategic question worth answering explicitly rather than by drift:
**should languages themselves become plugins, with kndo reduced to a shell?**

This RFC's answer: **converge the *contracts*, not the *linkage*.** Every extension —
language or ecosystem — becomes a *component*: one identity scheme, one activation model, one
dependency mechanism, one installer, one doctor surface. Whether a given component is
statically linked or loaded as WASM stays a distribution detail, invisible at every one of
those surfaces. The "kndo as a shell" build becomes a supported, CI-proven *configuration* —
not the shipped default.

### 2. Why the shell must not be the default

Each argument is an existing, load-bearing decision; this section only connects them:

- **No third linkage exists.** Rust has no stable native ABI (ADR 0003): "native, dynamically
  loaded" plugins are not on the table. The real choice per component is statically linked
  (full speed, monomorphized, in the default binary) or WASM (sandboxed, marshalled,
  fuel-metered). Adapters are the hot path — tree-sitter over every file, `resolve` per import
  — and the M4.5 performance budget was won with native adapters under rayon. Moving
  first-party languages to WASM would tax every user to benefit none.
- **Zero-config is the product** (ADR 0006). A shell that fetches languages on first run puts
  network inside pre-commit/CI/air-gapped runs — the exact download-on-demand model ADR 0006
  already rejected. A shell that pre-bundles everything is today's binary renamed.
- **Determinism is simplest when the binary is the hash.** "Same inputs ⇒ same findings" with
  a dynamically composed set is contingent on what's installed; `plugins.lock` + doctor keep
  it auditable, but the default path shouldn't need the audit.

What the vision *actually requires* is already true at the layering level: `kndo-core` knows
no language (RFC 0001's ignorance rule), and the `kndo` crate is pure composition over feature
gates. The gap is not architecture — it is that the two extension kinds have unequal contracts
and that the shell configuration is possible but unproven. §§3–7 close exactly that.

### 3. The component contract

A **component** is: a descriptor + one of the two capability traits.

```text
ComponentDescriptor (conceptual — realized as PluginDescriptor and AdapterDescriptor):
    id:                     coordinate identity (RFC 0015 §2: kndo:* reserved, or source coordinate)
    version:                semver string
    activation:             Vec<ActivationRule>   — gates the global tier (RFC 0003 §4)
    dependencies:           Vec<coordinate>       — co-install + co-activate fixpoint (RFC 0015 §3)
    requested_file_access:  Vec<glob>             — the §5 content channel's scope, and part of §6's cache key
```

`PluginDescriptor` already has all five fields. `AdapterDescriptor` already had `id`; it gains
`activation` and `dependencies` (§4 — landed; `version` was never added, §4's own note on why).
Its existing extension claims remain its file-claiming mechanism, untouched. The traits stay two: parsing/resolution/manifests belong to `LanguageAdapter`,
graph hooks to `Plugin` — merging them would either weaken the adapter contract into
uselessness or widen the plugin contract until it *is* the adapter contract renamed. "Adapters
describe what code is; plugins describe what an ecosystem means by it" survives this RFC
intact; what dissolves is every *operational* difference between the kinds.

Uniformity across linkage is the invariant to protect: a statically linked component and an
installed WASM component must be indistinguishable at the descriptor, activation, doctor,
dependency, and (where applicable) installer surfaces. First-party components are simply
components whose distribution happens to be "compiled into the default binary."

### 4. Adapters become installable components — Landed

The concrete closure of RFC 0003 §4's stated gap.

1. **Identity, landed narrower than first drafted.** External adapters use source coordinates
   (`github.com/<owner>/<repo>`), with §2's identity binding enforced at install exactly as for
   plugins (`kndo::plugin_install::wasm_probe` tries the plugin loader, then the adapter loader,
   and identity binding runs on whichever accepts the bytes). The loader's `kndo:` rejection
   extends to the adapter ABI (`WasmAdapter::load` now checks `is_reserved_id`, mirroring
   `WasmPlugin::load`). **First-party adapter ids stay as they are** (`"js-ts"`, `"go"`, …) — not
   renamed into the `kndo:` namespace. Nothing requires the rename for the protection to work:
   the reservation is "no *external* component may claim an id starting with `kndo:`,"
   independent of what compiled-in ids actually are, and compiled-in adapters never pass through
   the loader that rejection lives in. Renaming would only churn the graph cache key
   (`compute_graph_key` hashes adapter ids) for zero behavioral gain — deferred indefinitely, not
   just to this phase.
2. **Activation.** `AdapterDescriptor.activation`, wired end to end: a new `activation-rule`
   variant plus `activation`/`dependencies` fields on `adapter.wit`'s `adapter-descriptor`
   (duplicated from `plugin.wit`'s own type, same cross-package-independence reasoning that
   module already documents), read by the host bridge (`host.rs`'s `native_descriptor`).
   `dependencies` rode the wire unevaluated at this phase's landing — no adapter had needed
   cross-adapter implication yet, and wiring a fixpoint nothing exercised would have been
   exactly the speculative machinery this project's standing rules reject. (RFC 0017 §6 later
   evaluated it, under the platform criterion, with the plugin tier's own fixpoint made
   kind-neutral.) Semantics mirror plugins exactly: compiled-in and project-local
   adapters are unconditional regardless of `activation` (their file-extension claims already
   scope the cost — an adapter claiming `.go` files is dormant in a Go-less repo); a
   **globally installed** adapter requires a matching rule to join composition, empty rules
   never self-activate globally (`kndo::compose_adapters`, reusing the plugin tier's
   `activation::activates`/`global_plugin_dir` machinery in `crates/kndo/src/lib.rs`).
3. **Installation.** `kndo plugin install` accepts adapter components with no structural
   change to `kndo::plugin_install` — fetch, checksum, identity binding, and the lockfile were
   already kind-agnostic (`ProbedDescriptor` never carried anything plugin-specific); only the
   probe closure grew a fallback try. The command name stays `kndo plugin`.
   `AdapterDescriptor` gains no `version` field — the release *tag* already is the version
   (`plugins.lock` records it directly; `ProbedDescriptor.version` turns out to be unused by
   the install pipeline for either kind, discovered auditing this, not by design) and nothing
   inside the descriptor needs to restate it.
4. **Claim conflicts get one deterministic rule — and the *actual* prior behavior was the
   opposite of this RFC's first draft.** Composition order for file claims is now project-local
   externals > globally installed externals > compiled-in, ties within a tier broken by id
   (`kndo::compose_adapters`). Auditing the code before writing this down found the pre-existing
   order was **compiled-in first, externals appended after** — meaning a project-local adapter
   could never win a contested extension against a built-in one, silently. That's corrected
   here, not merely documented: presence in `.kndo/plugins/` is deliberate, strong opt-in signal
   (RFC 0003 §3's own framing), and a user who drops a custom adapter there almost certainly
   means to override, not to be silently shadowed. `kndo doctor`/`kndo::adapter_resolution`
   show the composed order so which adapter would win a contested extension is inspectable, not
   just implied by list position.

### 5. The content channel: `requested_file_access` for graph hooks — Landed

The highest-value extension, and the one both shipped plugin specs already pointed at.
Implemented in full (`kndo_core::plugin::ContentView`, `crates/kndo-plugin-api`'s `read-file`
host import — wasm-abi.md §5.1/§5.3/§8); two amendments from this section's original design,
made at implementation time and recorded here rather than left as silent drift:

- **Three hooks, not four.** `contribute_roots`/`contribute_edges`/`annotate_symbols` gain
  `content: &ContentView<'_>`; `classify_file` does not. It runs once per *file* across every
  registered component (`graph.rs`'s phase-2 loop, `O(files × components)`), where the other
  three run once per *component* per round — giving it the same channel would mean
  instantiating a WASM guest's content snapshot on that hot path for a use case nothing has
  asked for. Revisit if a real `classify_file` consumer needs it.
- **The boundary is narrower than "outside the graph" alone suggests.** The channel is for
  files the language graph doesn't itself claim and parse — configs, manifests, templates.
  What it does *not* cover, even though nothing stops a component from trying: reading a
  claimed source file to extract a fact the adapter itself owns. Concretely, this ruled out
  two items this section originally listed as unlocked — route-string → page edges from
  `<Link href="...">` and `res.render("name")` → template edges both require parsing `.tsx`/
  `.jsx` *source*, which the JS/TS adapter already claims; second-guessing it through the
  content-channel side door is exactly RFC 0002's boundary this RFC promised not to erode
  (docs/src/plugins/nextjs.md §5 records the final call). What *did* land as real consumers:
  `kndo:express` reading `package.json` `main`/`scripts` instead of guessing entry files by
  name (docs/src/plugins/express.md §3/§4), and `kndo:nextjs` statically reading a literal
  `pageExtensions: [...]` array out of `next.config.*` to narrow which extensions count as
  routed (docs/src/plugins/nextjs.md §5) — both bounded, both degrade to the pre-channel behavior
  on anything they can't statically read.
- **Shape, as built.** Glob matching runs in memory against paths this run already discovered
  (no second disk walk; source-blind — identical behavior for a directory or an in-memory git
  tree, RFC 0004 §6) rather than against the real filesystem the way `ActivationRule::
  FileExists` does, so a recursive glob like `**/package.json` never touches `node_modules`
  regardless of gitignore state. The WASM bridge prefetches every glob-matched path into an
  owned snapshot before instantiating each round's guest (`HostViewData`, mirroring how it
  already snapshots `list-files`/`symbols-in`); `read-file` on the guest side is a lookup into
  that snapshot, never a live call.
- **Budgets.** Per-component caps on distinct paths read and total bytes (`CONTENT_MAX_FILES`/
  `CONTENT_MAX_BYTES` in `kndo_core::plugin`) — conservative constants for the channel's
  stated scope (configs/manifests/templates, never source), not derived from a benchmark
  sweep; exceeding them cuts the component off from further reads for the rest of the run,
  with one diagnostic recording why — the same posture as the WASM fuel budget. Accounting is
  keyed by *path*, not by call: a component's read scope shouldn't depend on how many hooks
  look at the same file. (When this landed, the keying also compensated the WASM bridge's
  then-current instance-per-hook triple-fetch; RFC 0017 §4's one-instance-per-round lifecycle
  removed that motivation, and the keying stays on its own merits.)
- **Determinism note:** content-derived contributions are already correct under the
  `mutates_graph` bypass (RFC 0003 §5) — every run re-reads. §6 is what makes them *fast*.

### 6. Cache-key folding — the performance gate for a component-heavy world — Landed

Before this landed, any graph-mutating component forfeited both the snapshot cache and the
incremental patch (RFC 0003 §5, wasm-abi §5.4) — correct, and acceptable while such components
were rare. In the world §§4–5 create, matching projects would full-rebuild every run. Landed:

1. **Component identity folds into the graph cache key** (RFC 0004 §3's original design, now
   in `compute_graph_key`): every registered *graph-mutating* plugin contributes its id and
   declared version; WASM plugins additionally contribute the component's own content hash
   (`blake3` over the `.wasm` bytes, computed once at load and exposed via a new
   `Plugin::content_hash()` default-`None` method — compiled-in plugins need no content term
   because the binary's own version already subsumes their code). Enable/upgrade/remove of a
   plugin now invalidates exactly the snapshots it could have touched, sorted by id so
   composition order never perturbs the key.
2. **No separate content-channel read-set tracking was needed** — the RFC's original draft (2)
   assumed one would be, but implementation-time analysis found the existing key already closes
   the gap: `compute_graph_key` folds in `discovered_files`, the full unfiltered discovery
   output, and `ContentView::read` (plugin.rs) can only ever return a path already inside that
   same discovered set — it never reads outside the project tree it was handed. So every file a
   content-channel read could observe already has its content hash in the key via the discovery
   term; a change to that file was already a cache miss before this phase, with zero added
   machinery. Building dedicated per-component read-set bookkeeping would have duplicated
   information the key already carries. Glob *result* changes (a new file matching a declared
   glob) are likewise already caught, for the same reason.
3. **The blanket `mutates_graph` bypass narrows, but only on the snapshot path.** A
   graph-mutating plugin's presence no longer forces `graph_key`'s cache lookup/write to be
   skipped — (1)+(2) make the key itself sufficient to detect any input change, so
   snapshot reuse (`cache.get_graph`/`graph_writer`) is now unconditional. At this phase's
   landing, the incremental *patch* path (`try_patch`) stayed bypassed whenever any
   graph-mutating plugin was registered: a patch mutates an existing graph in place from a
   source-file diff alone, and proving a plugin's hook output composes correctly with a
   partial re-derivation was a materially harder claim than "the whole snapshot is either
   valid or it's rebuilt" — not attempted here. *(Subsequently closed: RFC 0017 §3 removed
   that bypass structurally — the patch strips provenance-tagged plugin contributions and
   re-runs the round, guarded by a snapshot-stored plugin-set digest — so the composition
   proof this paragraph declined to attempt was never needed.)* The posture is the same as
   phase 0–2: earn scope incrementally, keep the bypass as the correctness backstop wherever
   the narrower claim isn't proven, never remove it wholesale.

Performance consequence: a plugin-bearing project's *first* run after a plugin changes still
full-rebuilds (no different from before), but every unchanged repeat run now takes the snapshot
path instead of forced-bypass — the same cost as a plugin-free project's warm run. The existing
50k-fixture baseline (`xtask/perf-baseline.json`) already measures that path: `50k/cold-full` is
7333.1ms, `50k/warm-noop` is 600.8ms. Those numbers weren't re-measured with a plugin attached
because they don't need to be — the warm-run code path a plugin-bearing project now takes on a
no-op re-run is the identical `cache.get_graph` hit already covered by `50k/warm-noop`, not a
new one; the mechanism, not the fixture composition, is what determines the cost.

### 7. Smaller alignments — decided

- **`suppress` is cut, not wired.** Declared since RFC 0003 §2, never called. This phase's
  review found no shipped component — `kndo:nextjs`, `kndo:express`, or the reference examples —
  needs domain-specific suppression: every exemption those two plugins' own detections require
  is already reachable through `classify_file`/`contribute_roots` narrowing what gets analyzed
  in the first place, not through suppressing a finding after the fact. Building a reporting-side
  hook against a use case that doesn't exist yet is exactly the speculative surface this RFC's
  own freeze discipline (§8 phase 0) argues against adding. The decision is closed, not merely
  deferred: `suppress` is not part of either WIT package (wasm-abi §0/§5.2 already said so) and
  stays undeclared on the native `Plugin` trait too. A real use case reopens this — nothing about
  the freeze forecloses adding it later as a new, additive hook — but none exists today.
- **`GraphView` does not widen.** Both candidates named in the original draft were evaluated
  against an actual consumer, not a hypothetical one, and neither clears the bar:
  - *Package/unit topology* — motivated by `kndo:nextjs`'s `conventions::app_roots`, which scans
    every discovered path for a `package.json`/`next.config.*` anchor and takes its directory.
    `FileNode` already carries `package: PackageId` and `unit: Option<SmolStr>` (RFC 0011 §3,
    RFC 0012 §6), so exposing the package table through `GraphView` would let a plugin ask "which
    package owns this file" — but only the `package.json` half of `app_roots`' two anchors maps
    onto core's manifest-derived package boundary; `next.config.*` is a Next.js-specific
    convention core has no reason to treat as a package boundary. Widening `GraphView` here would
    add ABI surface without letting `app_roots` actually delete its own scan — a partial,
    speculative win, not an earned one.
  - *Import-edge queries* ("which files reference X") — motivated by route-string → handler and
    template → class edges. §5's own landed scope explicitly keeps both out of the content
    channel's contract (a graph-claimed source file read through the content side door, not a
    file outside the graph) — the motivating consumer was scoped out in phase 1, before this
    phase started. There is no current caller left to widen `GraphView` for.
  Both stay open for a future RFC with a real, landed consumer on the table — not ruled out,
  just not built speculatively now.
- **The shell build is CI-proven.** `.github/workflows/ci.yml` gained a standalone `shell-build`
  job: `cargo build -p kndo-cli --no-default-features --features external-adapters,plugin-install`
  (plus its own `cargo clippy -D warnings` pass), then a smoke check — the reference external
  adapter (`examples/kndo-plugin-demo`, M5's own exit-bar fixture) built to `wasm32-unknown-unknown`
  and componentized via a new `cargo xtask componentize` step (the same
  `wit_component::ComponentEncoder` call `crates/kndo/tests/external_adapter.rs` already makes
  in-process, exposed as its own dev-time command so CI needs no extra tool install), dropped
  into a fixture project's `.kndo/plugins/` with zero first-party adapters compiled in, and
  `kndo doctor`/`kndo check` asserted to auto-discover and run it. This surfaced one real,
  previously-latent bug: `kndo::default_adapters`'s `let mut adapters` was unconditionally
  mutable across every feature combination, which is only true when at least one language
  feature is on — with all eight off (the shell configuration exactly), the `mut` is unused and
  `rustc` warns. Fixed with an `#[allow(unused_mut)]` alongside the existing
  `#[allow(clippy::vec_init_then_push)]`, not by adding a language back. `crates/kndo-cli`
  gained its own `[features]` table (`default-features = false` on its `kndo` dependency, one
  pass-through feature per `kndo` feature) — without it, `--no-default-features` on `kndo-cli`
  had nothing of its own to disable and `kndo`'s defaults would activate regardless of what
  `kndo-cli`'s own flags said. That artifact *is* the "kndo as cascarón" configuration — real,
  tested on every push, and one flag away for embedders — while the default binary keeps ADR
  0006's promise unchanged.

### 8. Phases

0. **Freeze reservations (M6, before ABI/schema freeze):** none of §§4–6 needs to ship at
   1.0, but the freeze must not wall it off. Concretely: wasm-abi §8's versioning note gains
   the planned additive evolutions (adapter-world descriptor export; plugin-world `read-file`
   host import) so they're declared forward-compatible extensions, not breaking changes;
   `AdapterDescriptor` gains the dormant `id`/`activation`/`dependencies` fields native-side
   (cheap, invisible to behavior) so first-party adapters can populate them without a contract
   break later. **Landed**: wasm-abi §8 carries the two declared extensions;
   `AdapterDescriptor.activation`/`.dependencies` exist (empty everywhere, unread) — `id`
   needed no new field, only the §4 migration note on the existing one, deferred to phase 2
   because renaming ids churns cache keys.
1. **Content channel (§5) — Landed.** Highest value per unit of new surface; upgrades
   `kndo:express` and `kndo:nextjs` from their documented approximations, which also made it
   the phase with built-in dogfood — both plugins' own baseline-then-plugin fixture suites
   (`crates/kndo/tests/builtin_plugin_proofs.rs`) grew a scenario apiece proving the
   content-derived rescue actually fires, plus native (`kndo-core`) and WASM
   (`kndo-plugin-api`'s compliance suite, `examples/kndo-plugin-hooks-demo`) round-trip tests
   for the channel mechanism itself.
2. **Adapter componentization (§4) — Landed.** Identity (loader rejection of the reserved
   namespace), activation (wired end to end including the WIT wire format), installer
   acceptance, the corrected claim-order contract, and doctor parity
   (`kndo::adapter_resolution`/`global_adapter_candidates`, `kndo doctor`'s new sections).
   Proven at every layer: `kndo-core` compiles under both the full and `--no-default-features
   --features js` builds unchanged; `crates/kndo/tests/global_adapter_activation.rs` proves the
   global-tier gate and the claim-priority ordering against a real WASM component; `crates/
   kndo/tests/plugin_install_probe.rs` proves the installer's dual probe against a real adapter
   component. Closing this phase also surfaced and fixed a real, if narrow, pre-existing
   concurrency bug in `kndo::plugin_install::wasm_probe`'s temp-file naming (PID-only, so two
   concurrent calls in one process could race on the same path) — found because this phase's
   own test suite was the first caller to exercise `wasm_probe` from two `#[test]`s in the same
   binary.
3. **Cache-key folding (§6) — Landed.** `compute_graph_key` folds in every graph-mutating
   plugin's id, declared version, and (WASM only) component content hash, sorted by id;
   snapshot reuse (`cache.get_graph`/`graph_writer`) is unconditional now — a narrower,
   honestly-scoped claim, not the full read-set-tracking design originally drafted in §6(2),
   which implementation-time analysis showed was already subsumed by the existing
   `discovered_files` term. The half this phase left bypassed — the incremental patch — was
   subsequently closed by RFC 0017 §3 (strip & re-run, with the plugin-set digest guard);
   `crates/kndo-core/src/graph.rs`'s
   `the_patch_re_derives_plugin_contributions_instead_of_bypassing` proves the combined
   result, and `compute_graph_key_distinguishes_wasm_plugin_content_from_its_own_id_and_version`
   the key term.
4. **`suppress` decision + GraphView additions + shell CI job (§7) — Landed.** `suppress`
   decided cut (no shipped consumer); both `GraphView`-widening candidates evaluated and left
   unbuilt (neither had a real, current, landed consumer — see §7 for each); the shell build is
   now a standalone, CI-proven `shell-build` job (`.github/workflows/ci.yml`) with its own
   smoke check, plus the `crates/kndo-cli` feature-passthrough and `cargo xtask componentize`
   plumbing that job needed to exist at all.

Order matters: 1 before 2 because installable external adapters are more attractive once the
plugin side demonstrates the full component surface; 3 before the shell is advertised because
a shell that full-rebuilds every run would demo badly and deserve it.

### 9. Explicitly out of scope

- **Analyses as components.** The zero-false-positive bar is enforceable because the core owns
  analysis semantics end to end (RFC 0003 §6's deliberate post-1.0 deferral of custom
  analyses; unchanged here). "Shell" never means the analyses move out.
- **Output formats as components** — unchanged from RFC 0003 §6, same schema-stability reason.
- **`dlopen`/cdylib native loading** — re-rejected for the reasons in ADR 0003; nothing in
  this RFC creates new pressure for it.
- **Making WASM the default for first-party anything** — §2 is the argument; revisit only if
  the WASM tier's measured overhead becomes negligible on the 50k fixture.
- **Splitting first-party components out of the workspace/repo.** Separate distribution ≠
  separate development: conformance fixtures, cross-adapter tests, and atomic contract changes
  (the standing rule: contract and doc change in the same PR) all depend on co-location.
