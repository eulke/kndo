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
    // { id: "js-ts", facts_schema_version: u32, file_globs, manifest_globs, grammar_version,
    //   visibility_ladder: Vec<VisibilityRung> }
    // visibility_ladder (RFC 0012 §6): what VisibilityLevel indexes into — each rung a
    // { scope: File|Unit|Package|Public, label } pair; the scope is what the core can check
    // (same file / same FileFacts::unit / same PackageId / anywhere — nested, narrowest to
    // widest), the label is the language's own word, used verbatim in remediation text.
    // Empty ladder = no visibility semantics (CSS, JSON): visibility analyses skip the
    // language. Conservative-mapping rule: a language level with no exact scope maps to the
    // nearest WIDER one (Java protected → Public) — over-approximating who may see a symbol
    // can only suppress a finding, never fabricate one. Assembly copies the ladder onto
    // ProjectGraph::visibility_ladders keyed by claim language; analyses are pure graph
    // functions and never touch adapters. Consumers: internal-only's tightest-sufficient
    // rung, private-type-leak's scope comparison, the member fallback's candidate scoping.
    // cycle_policy (RFC 0005 §8): { file_cycles, package_cycles }, each a CycleTolerance —
    // Hazard (cycles are ecosystem hazards → warning), Idiomatic (routine → info), or
    // Impossible (the compiler forbids them → the level is skipped outright). Same
    // data-on-the-descriptor pattern as the ladder, carried onto
    // ProjectGraph::cycle_policies; a mixed-language cycle takes the most severe tolerance
    // among its participants' languages.

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
    /// (workspace:*, path deps, alias paths) resolve as WorkspaceMember — the concrete
    /// sibling file plus the package name, from which assembly derives BOTH edge kinds
    /// (ImportsFile for reachability, ImportsDependency for the declaration contract —
    /// RFC 0011 §4 validates it both ways). ResolveCtx carries the workspace-member index
    /// (name → { dir, resolved entry }) the core builds from every named manifest's facts.
    /// When no concrete in-repo file matches (a source checkout whose published entries are
    /// build artifacts), the specifier falls through to the external ladder as a plain
    /// Dependency — the package is still consumed, and dropping to Unresolved would silently
    /// un-count a genuinely used dependency; only the file edge is unknowable.
    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx) -> Resolution;
    // Resolution = File(ProjectPath, Confidence) | Dependency(DependencyName, Confidence)
    //            | WorkspaceMember { name, target: ProjectPath, confidence }
    //            | Stdlib | Unresolved
}
```

```rust
pub struct FileFacts {
    pub declarations: Vec<Declaration>,     // { name, kind: SymbolKind, span, exported: bool,
                                             //   visibility, member_of: Option<Name>,
                                             //   signature_span: Option<Span> }
                                             // signature_span (RFC 0012 §5): the declaration's
                                             // *promise* — everything before the body block
                                             // (name, parameters, return/result types).
                                             // Callables only in v1; None on type/value
                                             // declarations (a struct's whole body is not a
                                             // signature — firing on it would falsely accuse
                                             // exported-struct/unexported-field shapes). Feeds
                                             // the `private-type-leak` analysis: a TypeUse
                                             // reference attributed to an exported callable
                                             // whose site lies inside this span, naming a
                                             // lower-visibility type, is a lying public API.
                                             // member_of (RFC 0012 §3): the owning type's
                                             // declared name when this is a member (a Go
                                             // method's receiver type, a class method's class);
                                             // name is then the BARE member name — ownership is
                                             // a structured fact, never string encoding. Members
                                             // resolve on their own track: an unqualified
                                             // reference never certain-resolves to a member; it
                                             // reaches members only through the duck-typed
                                             // fallback (assembly phase 3b — candidates are
                                             // same-language members whose declared visibility
                                             // scope contains the site, per the RFC 0012 §6
                                             // ladder; one candidate ⇒ Probable, several ⇒
                                             // Possible each, per RFC 0002 §5), and a
                                             // RawRoot targeting a member names it in qualified
                                             // `Owner.name` form. Display/selectors use the
                                             // qualified form; ids fold it in, so same-named
                                             // members of different owners stay distinct.
    pub references:   Vec<RawReference>,    // { name, scope-context, span, within, kind } —
                                             // resolved
                                             // core-side (graph::assemble) against same-file
                                             // declarations and this file's own import bindings
                                             // (below); no adapter resolution hook.
                                             // within (RFC 0012 §4): the declared symbol this
                                             // reference executes INSIDE — "the symbol whose
                                             // use triggers this code": callable bodies → the
                                             // callable (members in qualified Owner.name form);
                                             // load-time code → None; on-instantiation code →
                                             // the type. Assembly attributes the References
                                             // edge to that symbol, making transitive death
                                             // visible (a dead function's calls no longer keep
                                             // its callees alive). An unresolvable within falls
                                             // back to file attribution — the safe direction —
                                             // and None (adapters not emitting it) reproduces
                                             // prior behavior exactly.
                                             // kind (RFC 0012 §5): RefKind — what the use *is*
                                             // (Read | TypeUse | Extend | …), carried verbatim
                                             // onto the References edge. Untagged/default is
                                             // Read, byte-identical to pre-§5 behavior; tree-
                                             // sitter grammars make TypeUse nearly free (Go/TS:
                                             // `type_identifier` IS the type-position signal).
                                             // Reachability ignores kind entirely — every kind
                                             // keeps its target alive; only kind-selective
                                             // analyses (private-type-leak) filter by it.
                                             // scope_context (RFC 0012 §9): the qualifier text
                                             // of a qualified access (`json.Marshal` → { name:
                                             // "Marshal", scope_context: Some("json") }).
                                             // Assembly matches it against the file's imports —
                                             // explicit local_alias, or the resolved target's
                                             // unit_name — and resolves the name INSIDE that
                                             // target at Certain (hit or miss, a matched
                                             // qualifier settles resolution; the local tables
                                             // are never candidates). An unmatched qualifier is
                                             // a receiver expression: member access by
                                             // construction — skips free-name tables, goes
                                             // straight to the §3 member fallback.
    pub imports:      Vec<RawImport>,       // { specifier, kind: Relative|Package, span,
                                             //   side_effect_only, type_only, confidence,
                                             //   bindings: Vec<ImportBinding>, reexported,
                                             //   opaque_namespace_use,
                                             //   local_alias: Option<Name> }
                                             // local_alias (RFC 0012 §9): the EXPLICIT alias a
                                             // namespace import binds its target under (Go's
                                             // `import j "enc/json"` → Some("j")); None for
                                             // unaliased imports — assembly then derives the
                                             // qualifier from the resolved target's own
                                             // unit_name, fixing dir≠package specifiers
                                             // (gopkg.in/yaml.v3 binds as `yaml`). JS/TS
                                             // (name-binding imports) always None.
                                             // kind is syntactic shape only — Stdlib is a
                                             // resolve()-time fact, never claimed here.
                                             // ImportBinding { local, imported: Option<Name> } —
                                             // None imported = default import (binds to the
                                             // target's synthetic "default" export); lets a
                                             // same-name RawReference resolve to the *target
                                             // file's* symbol instead of (incorrectly) a
                                             // same-file one. Statically-tracked namespace
                                             // member accesses (`ns.foo`) become bindings with
                                             // a dotted local ("ns.foo") plus a same-named
                                             // RawReference. opaque_namespace_use: the imported
                                             // namespace is consumed in ways static tracking
                                             // can't follow (computed member `ns[key]`, or ns
                                             // escaping into a call/assignment) — assembly then
                                             // wildcards over the resolved target's symbols.
    pub roots:        Vec<RawRoot>,         // language-defined only (main, pub API…), target is
                                             // *within this file* — WholeFile | Declaration(name)
    pub functions:    Vec<FunctionMetrics>, // { symbol, cyclomatic: u32, loc, fingerprints }
                                             // (RFC 0005 §6): one entry per callable, over its
                                             // BODY. symbol uses the roots/within naming
                                             // convention (bare, or qualified Owner.name for
                                             // members); assembly resolves it to a SymbolId
                                             // onto ProjectGraph::function_metrics.
                                             // fingerprints = winnowing over the normalized
                                             // token stream (toolkit metrics module: IDs/
                                             // literals canonicalized, comments skipped), empty
                                             // under the 50-token granularity gate — cyclomatic
                                             // and loc always real (crap's inputs, M4).
    pub dynamics:     Vec<DynamicUse>,      // constructs forcing Wildcard edges (span + reason +
                                            // optional narrowed_to: a *project-relative* dir the
                                            // adapter already resolved — the core only prefix-
                                            // matches it, staying language-blind)
    pub suppressions: Vec<RawSuppression>,  // kndo:allow pragmas found in comments (§2.1)
    pub diagnostics:  Vec<Diagnostic>,
    pub unit:         Option<SmolStr>,      // reference-resolution scope beyond "this file" —
                                             // see below; `None` for file-scoped languages
    pub unit_name:    Option<SmolStr>,      // the name IMPORTERS bind this unit by (RFC 0012
                                             // §9): Go's `package` clause name. Distinct from
                                             // `unit` (the opaque grouping key, dir#package):
                                             // unit_name is the visible qualifier assembly
                                             // resolves unaliased qualified references
                                             // against. None where imports don't bind a
                                             // namespace by the target's declared name (JS).
    pub detected_origin: Option<FileOrigin>, // content-derived correction of the claim-time
                                             // origin axis (RFC 0012 §7): a generated banner
                                             // (`// Code generated … DO NOT EDIT.`,
                                             // `@generated`) is a fact about the CONTENT,
                                             // which claim (path-only, fast by design) can't
                                             // see. Assembly applies it to the FileNode
                                             // before role roots and every analysis, so all
                                             // Generated/Vendored exemptions see the
                                             // corrected value. Role is never content-
                                             // corrected. None = claim-time origin stands.
                                             // Toolkit `ContentMarkers` scans a bounded
                                             // first-N-lines window for comment-marker
                                             // languages; adapters with structured signals
                                             // (Java @Generated) set it from their own AST.
}
```

**`unit` (added M3, surfaced by the Go adapter):** an unqualified [`RawReference`] resolves
against, in order, import-bound names, this file's own declarations, and — new — every other
file sharing this file's non-`None` `unit` key. Exists because "a reference resolves within its
own file, or through an explicit import" is a JS/TS-ism, not universal: Go's visibility unit is
the *package* (its containing directory), and two files in one package calling each other's
unexported functions with no `import` statement at all is the ordinary case, not an edge case a
file-scoped model can shrug off. The adapter computes the key from information it already has
(for Go: the file's own directory — no new input); the core only groups declarations by it
(`graph::assemble` phase 3a) and adds it as a third, last-resort lookup in phase 3b's reference
resolution. `None` (every adapter before Go, and JS/TS today) reproduces prior behavior exactly —
this is additive, not a breaking change to the trait's existing implementors. Import-binding
resolution gets the same fallback for the same reason, one level removed: `Resolution::File`
names one concrete file, but for a package-scoped language a single import can still name a whole
directory of files (Go: `resolve()` picks one representative file in the target package so an
`ImportsFile` edge has somewhere to point; the specific symbol an import binding names may live in
any of that directory's *other* files) — so a binding lookup that misses in the target file's own
symbol table falls back to the target file's `unit` table before giving up, mirroring the
reference-resolution fallback exactly.

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
    pub resolved_entries:   Vec<(ProjectPath, Confidence)>, // import entry points, resolved, in
                                                       // precedence order (main > module > exports
                                                       // leaves) — the WorkspaceMember.entry source.
                                                       // NOT private-gated (a sibling importing a
                                                       // private member still resolves through its
                                                       // entry; privateness only gates roots)
    pub declares_surface:   bool,                     // `exports` map or equivalent present —
                                                       // the contract gate for `deep-import` (RFC 0011 §4).
                                                       // Assembly carries it (plus resolved_entries
                                                       // as FileIds) onto PackageNode
                                                       // { declares_surface, surface }, so the
                                                       // analysis is a pure graph function: an
                                                       // ImportsFile edge from another package
                                                       // landing off-surface while the gate holds
                                                       // is a deep import.
    pub script_invoked_names: Vec<SmolStr>,           // names invoked as the leading command of a
                                                       // build/tooling script (npm `scripts`,
                                                       // Cargo `[[bin]]`-adjacent xtasks, …) — a
                                                       // CLI-only tool dependency never produces an
                                                       // Imports edge, so this is dependency
                                                       // hygiene's (RFC 0005 §5) only usage evidence
                                                       // for it; cross-referenced by name against
                                                       // declared dependencies downstream, so an
                                                       // unrelated match is simply never looked up
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
- `ConfigOverrides.use_cache` (default `true`) is the `--no-cache` switch (RFC 0004 §4), gating
  two cache layers (`cache.rs`, ADR 0004): the facts layer (`.kndo/cache/facts/`) skips
  re-parsing any file whose content hash already has a current entry; the graph layer
  (`.kndo/cache/graphs/<key>.bin`, content-addressed like facts entries so full mode's working
  tree and diff modes' before/after tree states coexist instead of evicting each other) skips
  claim/extract/resolve/link *entirely* when a single digest — folding in the whole discovered
  file set, every registered adapter's id/version, and the core graph-schema version — matches a
  stored snapshot exactly. There is no partial reuse yet: a graph-snapshot miss falls through to
  full assembly (itself facts-cache-warm for whichever files didn't change). `RunResult
  .cache_enabled`/`cache_hits` are how a frontend learns whether a run was actually warm —
  `run.cache` in the JSON envelope is `"warm"` only when the cache was on *and* served at least
  one file or the whole graph; an enabled-but-empty cache (first run, or a change big enough that
  nothing hit) is honestly `"cold"`. Correctness never depends on this: `--no-cache` must produce
  byte-identical findings (verified on the fixture matrix and the 5k-file benchmark; not yet
  wired into a CI workflow — RFC 0004 §4). The findings snapshot, the warm-run *patch* algorithm
  (reusing part of a stale graph), and dirty-region incrementality (RFC 0004 §2, §4–6) aren't
  implemented yet — a changed file forces a full rebuild, not a targeted patch; measured
  sufficient for the M2 budget at benchmark scale (ROADMAP M2 close-out note), revisit if a
  larger real repo's rebuild cost grows past budget.

## 6. Stability tiers

| Surface | Tier |
|---------|------|
| Graph vocabulary (§1), `LanguageAdapter`, `Plugin` | **Contract** — semver'd from 1.0; WASM ABI versioned independently |
| `Engine` facade (§5) | **Contract** — semver'd from 1.0; the only surface frontends may touch |
| `Analysis`, cache layouts | Internal — may change any release (cache self-invalidates) |
| Output schema | Contract — see [output-schema.md](output-schema.md) |
