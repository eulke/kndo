# Contract — Core Traits & Graph Vocabulary

**Status:** Draft · Normative for RFC 0001/0002/0003. Code must match this document; changing
either requires updating both in the same PR. Sketches are simplified Rust (lifetimes, error
types and non-essential fields elided) — shape is normative, exact signatures may be refined
during M1 with a PR to this file.

## 1. Graph vocabulary

```rust
pub struct FileId(u32);      // interned; stable within a snapshot
pub struct SymbolId(u32);
pub struct PackageId(u32);

pub enum FileFlavor { Production, Test, Tooling, Generated, Vendored }

pub enum SymbolKind {
    Function, Method, Class, Interface, Struct, Enum, TypeAlias,
    Const, Static, Field, Module, CssRule, CssVariable, Other(SmolStr),
}

pub enum RootKind { Production, Test, Tooling }

pub enum Confidence { Certain, Probable, Possible }

pub enum EdgeKind {
    ImportsFile   { from: FileId, to: FileId },
    ImportsPackage{ from: FileId, to: PackageId },
    References    { from: SymbolId, to: SymbolId },
    Declares      { file: FileId, symbol: SymbolId },
    Root          { kind: RootKind, target: NodeRef },     // NodeRef = File | Symbol
    Wildcard      { from: FileId },                        // dynamic construct: may reach anything visible
}
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
    fn claim(&self, path: &ProjectPath) -> Option<FileClaim>;   // { language, flavor: FileFlavor }

    /// Parse one file and extract every language-defined fact. Must not fail on broken code:
    /// return partial facts + diagnostics.
    fn extract(&self, file: &SourceFile) -> FileFacts;

    /// Parse a manifest into declared dependencies.
    fn extract_manifest(&self, file: &SourceFile) -> ManifestFacts;

    /// Resolve an import specifier to a concrete target, given an index of claimable paths.
    /// Called by the core's resolution driver — including for specifiers emitted by *other*
    /// adapters (cross-language edges, RFC 0002 §4).
    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx) -> Resolution;
    // Resolution = File(ProjectPath, Confidence) | Package(PackageName, Confidence)
    //            | Stdlib | Unresolved
}
```

```rust
pub struct FileFacts {
    pub declarations: Vec<Declaration>,     // { name, kind: SymbolKind, span, exported: bool, visibility }
    pub references:   Vec<RawReference>,    // { name, scope-context, span }  → resolved by driver
    pub imports:      Vec<RawImport>,       // { specifier, kind, span, side_effect_only: bool }
    pub roots:        Vec<RawRoot>,         // language-defined only (main, pub API…)
    pub functions:    Vec<FunctionMetrics>, // { symbol, cyclomatic: u32, loc, token_fingerprints }
    pub dynamics:     Vec<DynamicUse>,      // constructs forcing Wildcard edges (span + reason)
    pub diagnostics:  Vec<Diagnostic>,
}
```

Compliance: every adapter must pass the shared conformance harness with its fixture corpus
(RFC 0002 §8). `FileFacts` must be deterministic for identical content.

## 3. `Plugin`

All hooks optional; a plugin implements what it needs (RFC 0003 §2). Same trait for built-ins
(statically linked) and external WASM components (bridged via `kondo-plugin-api`, ADR 0003).

```rust
pub trait Plugin: Send + Sync {
    fn descriptor(&self) -> PluginDescriptor;
    // { id, version, ordering constraints, detection: Vec<DetectRule>, requested_file_access: Vec<Glob> }

    fn classify_file(&self, path: &ProjectPath, current: FileFlavor) -> Option<FileFlavor> { None }
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
    fn id(&self) -> AnalysisId;                       // "unused-code", "crap", …
    fn run_full(&self, graph: &GraphView, enrich: &Enrichments) -> Vec<Finding>;
    /// Incremental entry point; default = run_full (correct, slower). Implementations override
    /// with dirty-region logic (RFC 0004 §5). CI enforces full ≡ incremental on fixtures.
    fn run_incremental(&self, graph: &GraphView, dirty: &DirtyRegion, prev: &FindingsView,
                       enrich: &Enrichments) -> Vec<Finding> { ... }
}
```

## 5. Stability tiers

| Surface | Tier |
|---------|------|
| Graph vocabulary (§1), `LanguageAdapter`, `Plugin` | **Contract** — semver'd from 1.0; WASM ABI versioned independently |
| `Analysis`, cache layouts | Internal — may change any release (cache self-invalidates) |
| Output schema | Contract — see [output-schema.md](output-schema.md) |
