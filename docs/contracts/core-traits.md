# Contract — Core Traits & Graph Vocabulary

**Status:** Accepted · Normative for RFC 0001/0002/0003. Code must match this document; changing
either requires updating both in the same PR. Sketches are simplified Rust (lifetimes, error
types and non-essential fields elided) — shape is normative, exact signatures may be refined
during M1 with a PR to this file.

## 1. Graph vocabulary

```rust
pub struct FileId(u32);      // interned; stable within a snapshot
pub struct SymbolId(u32);
pub struct DependencyId(u32); // an external dependency declared in a manifest
pub struct PackageId(u32);    // a workspace unit: one manifest + the files it governs (RFC 0011)

// A file's classification is two orthogonal axes, never one enum: a generated test file and a
// vendored production file are both expressible. `Role` values mirror `RootKind` on purpose.
pub enum FileRole   { Production, Test, Tooling }
pub enum FileOrigin { Authored, Generated, Vendored }
pub struct FileClass { pub role: FileRole, pub origin: FileOrigin }

pub enum SymbolKind {
    Function, Method, Class, Interface, Struct, Enum, EnumMember, TypeAlias,
    Const, Static, Variable, Field, Module, CssRule, CssVariable, Other(SmolStr),
}
// kebab-case names are `subject_kind` facet values (alongside file | directory | dependency |
// import | suppression) used in output and `category:subject` targeting (RFC 0005)

pub enum RootKind { Production, Test, Tooling }

pub enum DependencyScope { Prod, Dev, Build, Peer, Optional }
// analysis semantics per scope: RFC 0005 §5

pub enum RefKind { Call, Read, Write, Extend, Implement, Override, TypeUse }

pub enum Confidence { Certain, Probable, Possible }

pub enum EdgeKind {
    ImportsFile      { from: FileId, to: FileId },         // may cross Package boundaries (RFC 0011 §4)
    ImportsDependency{ from: FileId, to: DependencyId },
    References       { from: NodeRef, to: SymbolId, kind: RefKind },  // from: File | Symbol — see below
    Declares         { file: FileId, symbol: SymbolId },
    Root             { kind: RootKind, target: NodeRef },  // NodeRef = File | Symbol
    Wildcard         { from: FileId },                     // dynamic construct; resolved against a
                                                            // plausible target set, not a fixed target
}
// Every edge (Root and Wildcard included) carries its own Confidence and contributes to
// reachability only from that strength onward; how per-edge confidence combines into a node's
// (color, confidence) — including Wildcard's plausible-target-set expansion — is the tiered
// algorithm normatively defined in RFC 0005 §1, not left to each analysis to reinvent.
// `References.from` is `NodeRef` rather than always `SymbolId`: an adapter that hasn't tracked
// which declaration encloses a reference (only which file) emits `NodeRef::File` — sufficient
// for reachability (a reachable file referencing a symbol makes that symbol reachable
// regardless of which of the file's own functions did the referencing) though not for
// finer-grained "which caller" evidence. `NodeRef::Symbol` once an adapter tracks enclosing
// scope precisely enough to say more.
// Every File is owned by exactly one Package (nearest-manifest rule, RFC 0011 §3);
// Package depends-on Package edges are derived by the core, never emitted by adapters.
// RefKind matters to analyses: Implement/Override edges drive dispatch-aware member liveness
// (RFC 0005 §2); Extend/TypeUse distinguish type-level from value-level consumption.
// every edge: { kind: EdgeKind, confidence: Confidence, source: Provenance }
// Provenance = Adapter(AdapterId) | Plugin(PluginId)  — for attribution in output
```

## 2. `LanguageAdapter`

One implementation per language, registered at startup. Adapters are pure with respect to the
filesystem: all content arrives via parameters (determinism, sandboxing, testing — RFC 0002 §6).

```rust
pub trait LanguageAdapter: Send + Sync {
    fn descriptor(&self) -> AdapterDescriptor;
    // { id: "js-ts", facts_schema_version: u32, file_globs, manifest_globs, grammar_version }

    /// Claim & classify a path (fast; name-based, content peeking only when unavoidable).
    fn claim(&self, path: &ProjectPath) -> Option<FileClaim>;   // { language, class: FileClass }

    /// Is this path one of this adapter's manifest files? Manifests are claimed separately from
    /// source — they never get a `FileClaim`/language of their own (docs/adapters/js-ts.md §1).
    fn claim_manifest(&self, path: &ProjectPath) -> bool;

    /// Parse one file and extract every language-defined fact. Must not fail on broken code:
    /// return partial facts + diagnostics.
    fn extract(&self, file: &SourceFile) -> FileFacts;

    /// Parse a manifest into declared dependencies, package identity/topology (RFC 0011 §3),
    /// and roots. `ctx` lets the adapter resolve entry-point specifiers (main/module/exports/
    /// bin) against the known-files index itself — a manifest root always names a *different*
    /// file than the one being extracted, so (unlike `FileFacts::roots`) it must already be a
    /// concrete `ProjectPath` by the time the core sees it; the core has no language-specific
    /// resolution rules to guess one with.
    fn extract_manifest(&self, file: &SourceFile, ctx: &ResolveCtx) -> ManifestFacts;

    /// Resolve an import specifier to a concrete target, given an index of claimable paths.
    /// Called by the core's resolution driver — including for specifiers emitted by *other*
    /// adapters (cross-language edges, RFC 0002 §4). Internal-package specifiers
    /// (workspace:*, path deps, alias paths) resolve to File targets in the sibling package.
    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx) -> Resolution;
    // Resolution = File(ProjectPath, Confidence) | Dependency(DependencyName, Confidence)
    //            | Stdlib | Unresolved
}
```

```rust
pub struct FileFacts {
    pub declarations: Vec<Declaration>,     // { name, kind: SymbolKind, span, exported: bool, visibility }
    pub references:   Vec<RawReference>,    // { name, scope-context, span } — resolved core-side
                                             // (graph::assemble) against same-file declarations
                                             // and this file's own import bindings (below); no
                                             // adapter hook, no enclosing-scope tracking assumed
    pub imports:      Vec<RawImport>,       // { specifier, kind: Relative|Package, span,
                                             //   side_effect_only, type_only, confidence,
                                             //   bindings: Vec<ImportBinding> }
                                             // kind is syntactic shape only — Stdlib is a
                                             // resolve()-time fact, never claimed here.
                                             // ImportBinding { local, imported: Option<Name> } —
                                             // None imported = default import (binds to the
                                             // target's synthetic "default" export); lets a
                                             // same-name RawReference resolve to the *target
                                             // file's* symbol instead of (incorrectly) a
                                             // same-file one. Empty for side-effect-only and
                                             // namespace (`import * as ns`) imports — the latter
                                             // deferred, member-expression-aware resolution.
    pub roots:        Vec<RawRoot>,         // language-defined only (main, pub API…), target is
                                             // *within this file* — WholeFile | Declaration(name)
    pub functions:    Vec<FunctionMetrics>, // { symbol, cyclomatic: u32, loc, token_fingerprints }
    pub dynamics:     Vec<DynamicUse>,      // constructs forcing Wildcard edges (span + reason)
    pub suppressions: Vec<RawSuppression>,  // kndo:allow pragmas found in comments (§2.1)
    pub diagnostics:  Vec<Diagnostic>,
}
```

```rust
pub struct ManifestFacts {
    pub package_name:       Option<SmolStr>,
    pub private:            bool,                    // publish signal: true → app mode (RFC 0011 §5)
    pub workspace_members:  Vec<SmolStr>,             // workspace globs (RFC 0011 §3)
    pub dependencies:       Vec<ManifestDependency>,  // { name, version_req, scope: DependencyScope }
    pub entry_points:       Vec<SmolStr>,             // main/module/exports/bin/types, raw and
                                                       // unresolved — future self-import resolution
                                                       // input; NOT the root-worthiness signal
    pub roots:              Vec<ManifestRoot>,        // { kind: RootKind, target: ProjectPath,
                                                       //   confidence } — already resolved by the
                                                       // adapter (a manifest root always names a
                                                       // *different* file, unlike FileFacts::roots)
    pub declares_surface:   bool,                     // `exports` map or equivalent present —
                                                       // the contract gate for `deep-import` (RFC 0011 §4)
    pub diagnostics:        Vec<Diagnostic>,
}
```

Root defaults (RFC 0011 §5, library mode lands fully at M3 — this is the default rule, no
per-package config override yet): `bin` targets are production roots unconditionally; `main`/
`module`/`exports` targets are production roots only when `private` is false — an unpublished
app's exports are not roots on their own, something must actually import them. `types`/
`typings` are never roots — a `.d.ts` target carries no runtime edge.

### 2.1 Suppression extraction

Comment syntax is language-defined, so **adapters extract suppression pragmas**; the core only
validates and binds them (RFC 0005 §12):

```rust
pub struct RawSuppression {
    pub span: Span,                    // the pragma comment itself
    pub category: SmolStr,             // verdict name — validated by the core against the registry
    pub subject: Option<SmolStr>,      // optional :subject facet (kebab-case)
    pub reason: Option<String>,        // free text after the directive
    pub scope: SuppressionScope,       // Declaration | File
}
pub enum SuppressionScope { Declaration, File }
```

- Grammar inside any comment style of the language:
  `kndo:allow <category>[:<subject>] [reason…]` (scope `Declaration`) and
  `kndo:allow-file <category>[:<subject>] [reason…]` (scope `File`).
- Binding (core-side): a `Declaration` pragma attaches to the declaration it precedes or shares
  a line with, covering that symbol *and everything it declares* (a class-level allow covers its
  members). `File` pragmas cover the whole file.
- **Evaluation order (no-flicker guarantee):** analyses run as if no pragmas existed and compute
  the full finding set; suppression then *marks* matched findings (hidden from report and
  `--fail-on`, still counted) — it never deletes them. A pragma is `stale` only when it binds to
  nothing, names an unknown category, or matches nothing in that **pre-suppression** set. Thus
  "actively suppressing" and "stale" are mutually exclusive by construction: deleting a stale
  pragma cannot resurrect a finding (it was stale precisely because the finding no longer
  exists), and deleting an active one correctly un-hides its finding.
- `stale` findings are not inline-suppressible (`kndo:allow stale` is rejected as unknown-target
  meta-suppression); acknowledge them via baseline or config if needed.
- Adapters do **not** interpret pragmas — extraction only. Validation, binding, counting, and
  staleness are core logic, identical across languages.

Compliance: every adapter must pass the shared conformance harness with its fixture corpus
(RFC 0002 §8). `FileFacts` must be deterministic for identical content.

## 3. `Plugin`

All hooks optional; a plugin implements what it needs (RFC 0003 §2). Same trait for built-ins
(statically linked) and external WASM components (bridged via `kndo-plugin-api`, ADR 0003).

```rust
pub trait Plugin: Send + Sync {
    fn descriptor(&self) -> PluginDescriptor;
    // { id, version, ordering constraints, detection: Vec<DetectRule>, requested_file_access: Vec<Glob> }

    fn classify_file(&self, path: &ProjectPath, current: FileClass) -> Option<FileClass> { None }
    fn contribute_roots(&self, graph: &GraphView, out: &mut RootSink) {}
    fn contribute_edges(&self, graph: &GraphView, out: &mut EdgeSink) {}
    fn annotate_symbols(&self, graph: &GraphView, out: &mut AnnotationSink) {}
    fn ingest_coverage(&self, request: &CoverageRequest) -> Option<CoverageData> { None }
    fn suppress(&self, finding: &Finding) -> Option<SuppressReason> { None }
}
```

- `GraphView` is **read-only**; mutation happens only through the typed sinks, which the core
  validates (no dangling ids, no new kinds) and attributes (`Provenance::Plugin`).
- Host-mediated file access: content for `requested_file_access` globs is provided by the core;
  no ambient fs/net (enforced natively by convention, in WASM by the sandbox).
- Budget: per-hook fuel/time limit; an over-budget plugin is disabled for the run + diagnostic
  (RFC 0003 §3).

## 4. `Analysis`

Internal trait (not pluggable in 1.0 — RFC 0003 §6), listed here because its shape constrains
the graph API:

```rust
pub trait Analysis: Send + Sync {
    fn id(&self) -> AnalysisId;                       // "unused", "crap", …
    fn run_full(&self, graph: &GraphView, enrich: &Enrichments) -> Vec<Finding>;
    /// Incremental entry point; default = run_full (correct, slower). Implementations override
    /// with dirty-region logic (RFC 0004 §5). CI enforces full ≡ incremental on fixtures.
    fn run_incremental(&self, graph: &GraphView, dirty: &DirtyRegion, prev: &FindingsView,
                       enrich: &Enrichments) -> Vec<Finding> { ... }
}
```

## 5. `Engine` — the frontend boundary

`kndo-core` is a **library**; every interface to it — today's CLI, tomorrow's `kndo serve`/MCP,
an LSP, a GUI, a CI action — is a *frontend* consuming one facade. Nothing else is exported.

```rust
pub struct Engine { /* opaque: graph, cache, adapters, plugins */ }

impl Engine {
    /// `adapters` is composed by the DISTRIBUTION layer (the `kndo` crate, RFC 0001 §2) —
    /// frontends call `kndo::open(root, overrides)` and never touch this parameter; only
    /// embedders and tests pass a custom set.
    pub fn open(root: &Path, overrides: ConfigOverrides,
                adapters: Vec<Box<dyn LanguageAdapter>>) -> Result<Engine, EngineError>;
    pub fn check(&mut self, req: CheckRequest) -> RunResult;    // full | staged | diff
    pub fn query(&mut self, req: QueryRequest) -> QueryResult;  // RFC 0007 verbs, incl. batches
    pub fn explain(&self, id: FindingId) -> Option<Explanation>;
    pub fn baseline(&mut self, op: BaselineOp) -> BaselineResult;
    pub fn doctor(&self) -> DoctorReport;
}

// Distribution layer (crate `kndo`) — what frontends actually call:
// pub fn kndo::open(root: &Path, overrides: ConfigOverrides) -> Result<Engine, EngineError>
// pub fn kndo::default_adapters() -> Vec<Box<dyn LanguageAdapter>>
```

- `RunResult`/`QueryResult` are the **typed forms of the output schema**
  ([output-schema.md](output-schema.md)); the JSON and SARIF serializers live core-side so every
  frontend emits byte-identical machine output. *Human* rendering lives frontend-side (RFC 0009).
- **Separation rules, enforced by dependency direction:** the core contains no terminal concerns
  (no ANSI, no TTY detection, no exit codes, no stdout) — it returns data and never prints;
  frontends contain no analysis concerns — they cannot reach the graph, cache, or adapters
  except through `Engine`. A frontend that needs a new fact is a core PR adding it to
  `RunResult`, never a core import.
- `Engine` is synchronous and single-instance-per-project (the cache lock, RFC 0004 §7); a
  serving frontend wraps it in its own concurrency model.

## 6. Stability tiers

| Surface | Tier |
|---------|------|
| Graph vocabulary (§1), `LanguageAdapter`, `Plugin` | **Contract** — semver'd from 1.0; WASM ABI versioned independently |
| `Engine` facade (§5) | **Contract** — semver'd from 1.0; the only surface frontends may touch |
| `Analysis`, cache layouts | Internal — may change any release (cache self-invalidates) |
| Output schema | Contract — see [output-schema.md](output-schema.md) |
