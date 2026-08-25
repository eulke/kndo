//! The `LanguageAdapter` contract.
//!
//! An adapter is the only component that understands a language: it translates source files
//! into the neutral vocabulary. Adapters are pure with respect to the filesystem — all content
//! arrives via parameters (determinism, sandboxing, testing) — and own exactly
//! what the language specification defines; ecosystem knowledge belongs to plugins.

use smol_str::SmolStr;

use crate::vocab::{Confidence, DependencyScope, FileClass, RootKind, SymbolKind};

// ---------------------------------------------------------------- shared primitives
//
// `ProjectPath`/`Span`/`Diagnostic`/`DiagnosticLevel` live in `vocab.rs` now (they're not
// adapter-specific — query/engine/plugin carry them too); re-exported here so existing
// adapter code keeps compiling against `crate::adapter::{ProjectPath, Span, ...}` unchanged.
pub use crate::vocab::{Diagnostic, DiagnosticLevel, ProjectPath, Span};

/// A file's content as handed to an adapter by the core. Adapters never read the fs.
#[derive(Debug)]
pub struct SourceFile<'a> {
    pub path: &'a ProjectPath,
    pub content: &'a [u8],
}

// ---------------------------------------------------------------- descriptor & claiming

#[derive(Debug, Clone)]
pub struct AdapterDescriptor {
    /// e.g. "js-ts". Participates in cache keys. When adapters componentize these become
    /// component coordinates (`kndo:js-ts`, `github.com/owner/repo`) —
    /// not done eagerly because renaming ids churns cache keys.
    pub id: SmolStr,
    /// Bumping invalidates only this adapter's cached facts.
    pub facts_schema_version: u32,
    pub file_globs: Vec<SmolStr>,
    pub manifest_globs: Vec<SmolStr>,
    pub grammar_version: SmolStr,
    /// The ladder [`VisibilityLevel`] indexes into — index = level. Empty means
    /// the language has no visibility semantics (CSS, JSON): visibility analyses skip its
    /// files entirely, and its member declarations (it should have none) are treated as
    /// `Public` by the fallback's conservative default. Assembly carries the ladder onto the
    /// graph keyed by claim language, so analyses (pure graph functions) never touch adapters.
    pub visibility_ladder: Vec<VisibilityRung>,
    /// Cycle tolerance per graph level — same data-on-the-descriptor pattern as
    /// the ladder: assembly carries it onto the graph keyed by claim language, and `cyclic`
    /// maps `Hazard → warning`; `Idiomatic` and `Impossible` → the level emits nothing.
    pub cycle_policy: CyclePolicy,
    /// Whether this adapter's `resolve()` can ever produce an `ImportsDependency` edge for a
    /// manifest-declared dependency of this language. `true` for every language
    /// whose import specifier structurally identifies the declared package (npm's flat name,
    /// Go's module-path prefix, Cargo's crate name). `false` when the language's import
    /// namespace has no reliable mapping to its dependency-manifest coordinates without
    /// resolving the classpath (Java: `com.google.common.*` says nothing about groupId
    /// `com.google.guava` without asking Maven/Gradle to actually resolve it, which kndo — a
    /// static source analyzer — structurally never does). `false` makes `dependency_hygiene`
    /// skip this language's packages entirely (one diagnostic, not a false-positive flood of
    /// every declared dependency reading `unused`) — `version-skew` is unaffected, since it
    /// compares declared versions across manifests and needs no usage edge at all. Defaults to
    /// `true` in spirit (every adapter sets it explicitly; there is no `Default` impl here so a
    /// new adapter must make the call, not inherit a silent default).
    pub resolves_dependency_usage: bool,
    /// Dormant reservation: the machine-checkable activation predicates
    /// a *globally installed* adapter will be gated by when adapters componentize
    /// (same rules and semantics as `PluginDescriptor::activation`). Nothing evaluates
    /// this yet; compiled-in and project-local adapters are scoped by their file claims alone.
    /// Reserved before the 1.0 freeze so componentization is additive, not breaking.
    pub activation: Vec<crate::plugin::ActivationRule>,
    /// Dormant reservation: component dependencies by coordinate id,
    /// with co-install/co-activate semantics once componentization exists. Unread
    /// today, same reservation rationale as [`activation`](Self::activation).
    pub dependencies: Vec<SmolStr>,
    /// Directory names that mark files as test-role only when the directory is an
    /// *immediate child of the owning package's manifest directory* — conventions that are
    /// package-relative, not path-global (a build-tool convention like Cargo's `tests/` binds
    /// to the manifest beside it; a nested package whose sources happen to live under some
    /// ancestor's `tests/` tree is NOT test code). Names are adapter data; the matching runs
    /// in assembly — the only layer that knows which manifest owns which file. Files in the
    /// implicit no-manifest package match against the project root. Promotion only: a file
    /// already test-role by any other signal is never demoted. Contrast with
    /// the toolkit's `PathPatterns::test_dirs`, which matches the segment anywhere in the
    /// path — right for conventions like `__tests__/` that hold at any depth.
    pub package_test_dirs: Vec<SmolStr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileClaim {
    pub language: SmolStr,
    pub class: FileClass,
}

// ---------------------------------------------------------------- extracted facts

/// Ladder index on the adapter-declared visibility ladder: 0 = most private,
/// higher = wider. The adapter names the levels; the core only compares them.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct VisibilityLevel(pub u8);

/// What the core can *check* about a visibility level: the graph region a
/// symbol at that level is visible to. Ordered narrowest → widest (`File < Unit < Package <
/// Public`) — derive order is normative. Scopes nest: a `Unit` symbol is visible to its own
/// file too, a `Package` one to its own unit, and so on — containment checks treat them as
/// concentric, not disjoint.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub enum VisibilityScope {
    /// Same file only.
    File,
    /// Same `FileFacts::unit` key (Go package, Rust module) — contains `File`.
    Unit,
    /// Same `PackageId` (manifest ownership) — contains `Unit`.
    Package,
    /// Everywhere.
    Public,
}

/// One rung of an adapter's visibility ladder: the **scope** is what the core
/// checks; the **label** is the language's own word for the level, used verbatim in
/// remediation text ("in the language's own terms, supplied by the adapter").
/// Two rungs may share a scope (Java `protected`/`public` both map to `Public` — the
/// conservative-mapping rule: a level with no exact scope maps to the nearest *wider* one,
/// which can only suppress an `internal-only` finding, never fabricate one).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct VisibilityRung {
    pub scope: VisibilityScope,
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub label: SmolStr,
    /// Whether a re-export chain can carry a declaration at this rung *outside its package* —
    /// the axis `scope` alone cannot express. Rust `pub` and a JS `export`
    /// are **relative**: as visible as the module path that re-exports them (`true`). Rust
    /// `pub(crate)`, Java package-private, Swift `internal`, and Go exports under an
    /// `internal/` path element are **capped**: no re-export makes them consumable from
    /// outside (`false`). The core's library-surface promotion and surface closure promote
    /// only transitive rungs; capped rungs keep full `unused`/`internal-only` precision.
    pub surface_transitive: bool,
}

/// How a language's ecosystem regards an import cycle at one graph level —
/// tolerance is a *language fact*, declared by the adapter as data, so `cyclic`'s severity
/// stays honest per ecosystem instead of one-size-fits-none.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub enum CycleTolerance {
    /// The ecosystem treats cycles as hazards (JS/TS file cycles — init-order bugs) →
    /// severity `warning`.
    Hazard,
    /// Idiomatic and routinely tolerated (Rust modules within a crate) — the level emits
    /// nothing: a cycle here is true information about legal structure, and information is
    /// never dressed up as a defect. Kept distinct from [`CycleTolerance::Impossible`]
    /// because it documents the language's stance (the cycle is real and legal, not an
    /// artifact), which descriptors and doctor still surface.
    Idiomatic,
    /// The compiler/toolchain forbids it outright (Go package imports) — a cycle at this
    /// level cannot exist in building code, so the analysis skips the level entirely rather
    /// than accusing what must be a resolution artifact.
    Impossible,
}

/// An adapter's cycle tolerance per graph level: file-import cycles, and
/// package/module-graph cycles where manifests define units.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct CyclePolicy {
    pub file_cycles: CycleTolerance,
    pub package_cycles: CycleTolerance,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Declaration {
    /// The declaration's own name — for a member, the *bare* member name (`Method`, never
    /// `"T.Method"`): ownership is a structured fact (`member_of`), not string encoding
    ///. Display joins them (`T.Method`); resolution reasons about them apart.
    pub name: SmolStr,
    pub kind: SymbolKind,
    pub span: Span,
    pub exported: bool,
    pub visibility: VisibilityLevel,
    /// The owning type's declared name, when this declaration is a member of one (a Go
    /// method's receiver type, a class method's class, an enum member's enum — each
    /// language defines its own owners). `None` for free-standing declarations. Members resolve differently
    /// from free names: an unqualified reference never `certain`-resolves to a member — it
    /// reaches members only through the duck-typed fallback (`graph::assemble` phase 3b),
    /// at `Probable`/`Possible`, per the ladder. A [`RawRoot`] targeting a member
    /// names it in qualified `Owner.name` form.
    pub member_of: Option<SmolStr>,
    /// The sub-span covering this declaration's *signature* — parameters and return/result
    /// types, everything before the body. Only the adapter knows where a body
    /// starts; the core must not. `Some` on callables; `None` where the concept doesn't apply
    /// (a type's leak surface is its member fields, which land with member extraction — v1 of
    /// `private-type-leak` is deliberately callables-only rather than falsely accusing
    /// exported-struct/unexported-field shapes).
    pub signature_span: Option<Span>,
    /// This member is invoked by the language's own machinery when its OWNER is used, never
    /// by name at the call site (the machinery-dispatch rule): an operator overload
    /// (`==` → `eq`), a formatting hook (`{}` → `fmt`), a destructor (scope end → `drop`), a
    /// loop protocol (`for` → `next`). WHICH traits/protocols qualify is the adapter's
    /// curated knowledge — the criterion is "the call site never writes the method's name",
    /// which is exactly why no reference edge can ever exist for these. Reachability derives
    /// an implicit owner → member edge at `Probable` (using the type IS plausibly using the
    /// hook — degrade toward silence); requires `member_of`, ignored without it. Name-called
    /// trait methods (`.clone()`, `.into()`) stay out — the duck fallback already reaches
    /// those. Default `false`.
    pub implicitly_invoked: bool,
    /// The declaration lives in a scope unit nested *inside* its file (an inline
    /// module, a local namespace) rather than at the file's top level. The ladder's
    /// file-scope rung means "visible to this file"; for a nested declaration the language's
    /// tightest level means "visible to the enclosing scope" — strictly narrower — so
    /// file-local usage evidence alone cannot justify recommending that rung
    /// (`internal-only` advances past it). Default `false`.
    pub nested_scope: bool,
    /// The declaration has no declarable visibility of its own — the recorded level is
    /// inherited from its container (an enum's variants in Rust; any member the language
    /// scopes strictly through its owner). Narrowing advice cannot apply to the member
    /// itself — it belongs to the container, whose own declaration is measured separately —
    /// so visibility analyses skip it. Default `false`.
    pub visibility_inherited: bool,
    /// Language-visible MARKERS attached to this declaration: annotation names in Java and
    /// Kotlin (`@Controller`, `@AfterEach`), attribute paths in Rust, attributes in Swift,
    /// decorators in JS/TS. Bare names, in source order, duplicates kept — the adapter
    /// reports what is written, and never interprets it.
    ///
    /// The core never interprets them either: it has no list of framework names and cannot
    /// acquire one without breaking the ignorance rule. What it does is MATCH them against a
    /// list the project supplies (`kndo.toml`'s `[[externally-invoked]]`), because the
    /// question these answer is one static analysis cannot decide on its own — "is this
    /// declaration invoked from outside the analyzed source?" A Spring `@Controller` is
    /// instantiated by classpath component scanning and called by a servlet dispatcher; a
    /// JUnit `@AfterEach` method is called by the test runner; a `@Advice.OnMethodEnter`
    /// body is inlined into instrumented bytecode; a Koin `@Scoped` annotation is read by an
    /// annotation processor in a different repository. Every one of them reads `unused` or
    /// `test-only` with perfect correctness from the graph alone, and every one is a false
    /// accusation (see `internal/detection-gaps.md`).
    ///
    /// Markers are FACTS, not verdicts: an adapter emits them for every declaration that
    /// carries them, whether or not any framework is involved, and whether or not the
    /// project configures anything. Empty for languages with no such syntax (Go).
    pub markers: Vec<SmolStr>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawReference {
    pub name: SmolStr,
    /// The qualifier text of a qualified access: `json.Marshal` is
    /// `{ name: "Marshal", scope_context: Some("json") }`. Assembly resolves the qualifier
    /// against the file's imports — the explicit [`RawImport::local_alias`], or (unaliased)
    /// the resolved target's [`FileFacts::unit_name`] — and, on a match, resolves `name`
    /// inside that target's declarations at `Certain`. A qualifier matching no import is a
    /// receiver expression (`t.helper()`): the name is a *member* access by construction, so
    /// it skips the free-name tables entirely and goes straight to the duck-typed member
    /// fallback. `None` = an unqualified name, resolved as before.
    pub scope_context: Option<SmolStr>,
    pub span: Span,
    /// The declared symbol this reference executes *inside*, under one
    /// language-blind rule: **`within` = the symbol whose use triggers this code.** Bodies of
    /// callables → that callable (a member body names it in qualified `Owner.name` form, same
    /// convention as member root targets); code that runs at module/file *load* (top-level
    /// statements, package-level initializers) → `None`; code that runs on a type's
    /// instantiation/first use (constructors, field initializers) → that type. Assembly
    /// resolves it against the file's own declarations and attributes the `References` edge to
    /// that symbol — so a dead function's calls don't keep its callees alive (transitive
    /// death is visible). **Any `within` that doesn't resolve falls back to file
    /// attribution — an over-approximation, the safe direction** — and `None` (every
    /// adapter that doesn't emit it) keeps plain file attribution.
    pub within: Option<SmolStr>,
    /// What kind of use this is — `TypeUse` for type-position references (the
    /// fact `private-type-leak` is built on), `Extend`/`Implement` for inheritance clauses,
    /// `Read` otherwise. Assembly passes it straight onto the `References` edge; adapters that
    /// don't differentiate emit `Read`, the undifferentiated default.
    pub kind: crate::vocab::RefKind,
}

/// Syntactic shape only — what the specifier text looks like, not what it resolves to.
/// Builtins (`node:fs`, bare `fs`) are a *resolution*-time fact (the resolver owns the
/// builtins list); extraction never claims `Stdlib`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ImportKind {
    Relative,
    Package,
}

/// One local name this import introduces, and which export it names — the fact the core needs
/// to resolve a same-name-elsewhere `RawReference` to the *target file's* symbol rather than
/// (incorrectly) a same-file one. `imported: None` is a default import — binds to the target's
/// synthetic `"default"` export, the same name declarations.rs already uses for anonymous
/// default exports.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportBinding {
    pub local: SmolStr,
    pub imported: Option<SmolStr>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawImport {
    pub specifier: SmolStr,
    pub kind: ImportKind,
    pub span: Span,
    /// `import "./polyfill"` — counts as usage without binding names.
    pub side_effect_only: bool,
    /// `import type { T } from "..."` / type-only re-export — only the adapter can know
    /// this; it decides whether the resulting edge is a value reference or `TypeUse`.
    pub type_only: bool,
    pub confidence: Confidence,
    /// Empty for side-effect-only imports and namespace imports (`import * as ns`) —
    /// resolving `ns.foo` back to a specific export needs member-expression-aware reference
    /// resolution that isn't attempted; the import edge itself is unaffected, only the
    /// finer-grained "this binding referenced that export" fact is missed.
    pub bindings: Vec<ImportBinding>,
    /// `export { a } from "./b"` / `export * from "./b"` — a re-export, not a plain import:
    /// `bindings` become part of *this file's own* export surface too, so another file
    /// importing `a` from here should resolve straight through to `./b`'s original declaration
    /// ("Barrel files… resolved through, transparently"). `false` for an ordinary
    /// `import` statement, which only makes a name usable inside the importing file.
    pub reexported: bool,
    /// The imported namespace is consumed in ways static tracking can't follow — a computed
    /// member access (`ns[key]`) or the namespace value escaping into a call/assignment/return.
    /// Assembly then adds a `Wildcard` edge *from the resolved target file*, making every
    /// symbol in it plausibly used (`possible`) — the "wildcard over that
    /// namespace's exports" rule. Statically-tracked accesses (`ns.foo`) don't set this; they
    /// resolve precisely through `bindings` instead.
    pub opaque_namespace_use: bool,
    /// Swift module-import semantics: `import SomeKit` puts
    /// every top-level name of the imported MODULE (the whole unit, not one file) in bare
    /// scope — no per-name bindings exist in the syntax. Assembly then lets this file's
    /// bare-name resolution fall back to the resolved target's unit table at Certain (it
    /// is the language's own scoping rule, not a guess). `false` everywhere bindings or
    /// qualifiers carry the visibility (JS/Go/Java/Kotlin/Rust).
    #[serde(default)]
    pub module_names_visible: bool,
    /// The *explicit* local alias this import binds its target under: Go's
    /// `import j "encoding/json"` → `Some("j")`. `None` for unaliased imports — assembly then
    /// derives the qualifier from the resolved target's own [`FileFacts::unit_name`], which is
    /// correct by construction where a "last specifier segment" guess would go wrong
    /// (dir≠package mismatches like `gopkg.in/yaml.v3` →
    /// `yaml`). Languages whose imports bind names, not namespaces (JS/TS), leave it `None`.
    pub local_alias: Option<SmolStr>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RawRootTarget {
    WholeFile,
    /// By declared name within this file.
    Declaration(SmolStr),
}

/// Language-defined roots only (`main`, `pub` API…); ecosystem roots come from plugins.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawRoot {
    pub kind: RootKind,
    pub target: RawRootTarget,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FunctionMetrics {
    /// Declared name (symbol path within the file).
    pub symbol: SmolStr,
    /// The span of the [`Declaration`] these metrics describe — byte-for-byte the same
    /// `Declaration::span`, which is what makes it an exact identity. Metrics resolve to their
    /// symbol by span, never by name: a file may legitimately declare the same name twice
    /// (cfg-alternated `impl` blocks each declaring `Data.from_path`, platform-gated
    /// overloads), and a name lookup against the file's single-slot symbol table silently
    /// resolved BOTH entries to whichever declaration was inserted last — which then read as a
    /// structural clone of itself and double-counted its tokens into health's duplication
    /// ratio. There is deliberately no default: an adapter that emits metrics must say which
    /// declaration they belong to.
    pub span: Span,
    pub cyclomatic: u32,
    pub loc: u32,
    /// Normalized-stream token count — `health`'s duplication ratio
    /// basis ("duplicated tokens / total tokens").
    pub token_count: u32,
    /// Winnowing fingerprints over the normalized token stream.
    pub fingerprints: Vec<u64>,
}

/// A construct forcing a `Wildcard` edge (plausible-target-set expansion).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DynamicUse {
    pub span: Span,
    /// Human-readable reason ("non-literal import()", "eval").
    pub reason: SmolStr,
    /// Static narrowing when available (`./locales/${x}` → that directory), as a
    /// **project-relative directory path** — the adapter resolves its language's specifier
    /// semantics (what `./` is relative to) before handing this over; the core only
    /// prefix-matches it against discovered paths, staying language-blind. `None` (never
    /// `""`) when nothing narrows the scope — a bare `eval` — leaving the plausible target
    /// set at the default: the dynamic file's own symbols.
    pub narrowed_to: Option<SmolStr>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub enum SuppressionScope {
    Declaration,
    File,
}

/// A `kndo:allow` pragma as extracted; validation, binding, counting and staleness are core
/// logic, identical across languages (the no-flicker guarantee).
/// Carries its own rkyv derives (not just serde's) so it survives the graph-snapshot cache
/// unchanged — a graph-snapshot hit skips `FileFacts` extraction entirely, so without
/// this, suppressions would silently vanish on a warm run and violate the no-flicker guarantee.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct RawSuppression {
    pub span: Span,
    /// Verdict name — validated by the core against the registry.
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub category: SmolStr,
    /// Optional `:subject` facet (kebab-case).
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub subject: Option<SmolStr>,
    pub reason: Option<String>,
    pub scope: SuppressionScope,
}

/// An adapter-reported problem, minus [`Diagnostic::path`] — an adapter is always reporting
/// about the one file or manifest it was just handed, so its own path is never adapter
/// information; every ingestion site (`graph::assemble`'s phase 1 file merge, the manifest
/// merge) fills it in from the file it was extracting, unconditionally. Adapters that used to
/// write `path: None` at every call site (verified: never anything else) now simply don't
/// have the field to set.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdapterDiagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    pub span: Option<Span>,
}

/// Everything an adapter owes the core for one file. Must be deterministic for identical
/// content (conformance harness). Round-trips through the facts cache
/// (`cache.rs`) — `Deserialize` exists for that alone, never for adapters to read.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FileFacts {
    pub declarations: Vec<Declaration>,
    pub references: Vec<RawReference>,
    pub imports: Vec<RawImport>,
    pub roots: Vec<RawRoot>,
    pub functions: Vec<FunctionMetrics>,
    pub dynamics: Vec<DynamicUse>,
    pub suppressions: Vec<RawSuppression>,
    pub diagnostics: Vec<AdapterDiagnostic>,
    /// Reference-resolution scope, when the language's isn't file-scoped (contract
    /// extension surfaced by the Go adapter): files sharing the same non-`None` key resolve
    /// each other's declarations for an unqualified [`RawReference`] with no import binding, in
    /// addition to their own. `None` (every adapter before Go) keeps today's exact behavior —
    /// same-file-only, unless an import binds the name. Exists because file-scoped resolution is
    /// a JS/TS-ism, not a universal: Go's visibility unit is the *package* (its containing
    /// directory) — two files in one package call each other's unexported functions with no
    /// `import` at all, the ordinary, common case, not an edge case a per-file model can treat as
    /// safely-wrong. The adapter computes the key (for Go: the file's directory, from its own
    /// path — no extra input needed); the core only groups by it, staying language-blind.
    pub unit: Option<SmolStr>,
    /// Content-derived correction of the claim-time origin axis: extraction may
    /// report what the *content* proves about origin — a `// Code generated … DO NOT EDIT.`
    /// banner, an `@generated` marker — which claim (path-only, by design fast and name-based)
    /// cannot see. Assembly applies the override when building the `FileNode`, before any
    /// role-derived roots, so every origin exemption (`unused`, `test-only`, `untested`,
    /// `internal-only`, `private-type-leak` all exempt `Generated`) sees the corrected value.
    /// Role stays claim-time — no use case justifies content-derived roles. `None` = the
    /// claim-time origin stands. Rides the facts cache like every other content-derived fact.
    pub detected_origin: Option<crate::vocab::FileOrigin>,
    /// The name *importers bind this unit by*: Go's `package` clause name,
    /// Rust's module name. Distinct from [`FileFacts::unit`] (the opaque resolution *key* —
    /// `dir#package`): `unit_name` is the visible qualifier. Assembly uses the resolved
    /// import target's `unit_name` to resolve unaliased qualified references
    /// (`RawReference::scope_context`) — the fact that makes `gopkg.in/yaml.v3` → `yaml`
    /// resolve correctly, which no single-file guess about the specifier's last segment can.
    /// `None` for languages whose imports don't bind a namespace by the target's declared
    /// name (JS/TS).
    pub unit_name: Option<SmolStr>,
    /// Sub-file **test regions** (contract extension surfaced by Rust — the first language
    /// whose tests live *inside* production files): span extents whose contents are
    /// test-role, overriding the file's claim-time role for anything span-contained. Role
    /// stays per-file on the claim axis (a path names one role); this is the extraction-side
    /// truth that a region of a production file is test infrastructure — Rust's
    /// `#[cfg(test)]` items and `#[test]`/`#[bench]` functions, attribute extents included.
    /// Consumers: `crap` and health's symbol tallies skip span-contained symbols (parity
    /// with test files, which they skip wholesale); `dependency_hygiene` treats an import
    /// whose site lies in a test region as a test-role usage (a `prod`-scoped dependency
    /// used only under `#[cfg(test)]` is a `test-only dependency`, exactly as if the imports
    /// lived in test files); assembly demotes a claimed-production file to test role when
    /// every module-linking import reaching it originates inside a test region
    /// (`#[cfg(test)] mod tests;` pointing at `src/tests.rs` — a whole-file test the
    /// path-based claim cannot see). Duplicate detection deliberately does NOT consult
    /// regions — it already fingerprints test files, so inline test clones stay findings.
    /// Regions never overlap by construction (extraction records the outermost extent).
    /// Empty for languages whose test detection is per-file (JS/TS, Go).
    pub test_spans: Vec<Span>,
    /// Call sites whose argument is a **string literal**: `(callee dotted
    /// path, the literal, span)` — `res.render("index")`, `app.get("/users", …)`,
    /// `flags.isEnabled("checkout-v2")`. A generic, ecosystem-blind fact: the adapter
    /// records "a call with a string-literal argument", never what any framework means by
    /// it — interpretation belongs to plugins, which read these through
    /// `GraphView::string_call_sites_in` (natively) or `call-sites-in` (WASM) instead of
    /// re-parsing claimed source through the content channel. Optional per adapter,
    /// default empty (same contract posture as [`Self::test_spans`]); JS/TS implements it
    /// first. Only the call's *first* string-literal argument is recorded — the
    /// convention-bearing position in every motivating pattern — and only for direct
    /// literals, never computed strings (determinism over coverage).
    pub string_call_args: Vec<StringCallArg>,
    /// Member type facts (the cross-file tier): what accessing a member of
    /// a type YIELDS, as declared in this file — a struct field's annotated type, a
    /// method's return type, an associated const's type. Pure resolution metadata (no
    /// symbol semantics, fields stay non-declarations in v1): assembly indexes these per
    /// file, and a dotted qualifier pointer (`scope_context = "Config.separator"`)
    /// resolves hop by hop — base name in the reference's scope, `yields` looked up in the
    /// OWNER's home file, the yielded type name resolved in that same home (annotations
    /// mean what they mean where they were written), and the final member in the yielded
    /// type's home. Every hop is a declared-annotation fact → Certain; any miss falls to
    /// the duck fallback. Default empty; Rust implements it first.
    pub member_types: Vec<RawMemberType>,
    /// CJS's "the local IS the module value" fact (`module.exports = res`):
    /// the named LOCAL declaration is what a consumer's whole-module/default binding
    /// receives. Assembly aliases `default` → that symbol in this file's own table
    /// (vacant-only, like re-export aliases), so `var res = require('./response')` credits
    /// a real cross-file reference to the local instead of silently finding no symbol —
    /// liveness never needs it, but `internal-only`'s "only used within its own file"
    /// does. No synthetic symbol: a phantom-`default` false
    /// positive must never exist.
    #[serde(default)]
    pub default_export_alias: Option<SmolStr>,
    /// Names of workspace executable targets this file invokes **as a subprocess** — the
    /// process boundary no import edge can cross (the invoked-program rule). Only
    /// declared, literal invocations the language itself vouches for (Rust:
    /// `env!("CARGO_BIN_EXE_<name>")`, Cargo's own documented handshake for exactly this);
    /// never inferred from arbitrary strings — determinism over coverage.
    /// Assembly resolves each name against the workspace's [`ManifestFacts::executables`]
    /// and emits an `InvokesFile` edge to the target's entry file; an unknown name emits
    /// nothing. Default empty; Rust implements it first.
    pub invoked_executables: Vec<SmolStr>,
}

/// One [`FileFacts::member_types`] entry: accessing `owner.member` yields a value of the
/// (base) type named `yields`. Carries rkyv derives because the incremental patch persists
/// these per file (`FilePatchMeta`) — resolution of changed files needs unchanged files'
/// member types.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct RawMemberType {
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub owner: SmolStr,
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub member: SmolStr,
    /// The BASE type name the access evaluates to, dispatch-reduced by the adapter
    /// (references stripped, auto-deref wrappers unwrapped, `Self` already resolved).
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub yields: SmolStr,
    /// The base names of EVERY type parameter, in declaration order, when `yields` is
    /// parameterized (`Result<Config, Error>` → `["Config", "Error"]`,
    /// `Map<Key, Value>` → `["Key", "Value"]`). A pointer segment marked `?N` projects
    /// parameter N instead of `yields` itself (`?` alone is `?0`); WHICH parameter an
    /// operation extracts is the adapter's knowledge (Rust's try operator → 0; a
    /// map-index fact would be 1) — the core's selection is purely structural.
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub yields_params: Vec<SmolStr>,
}

/// One [`FileFacts::string_call_args`] entry. Carries rkyv derives because assembly persists
/// these onto [`crate::graph::FileNode`] (the graph snapshot round-trips them), same pattern
/// as [`RawSuppression`].
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct StringCallArg {
    /// The callee as written, dotted path form: `res.render`, `app.get`, `require`. No
    /// resolution — this is the *syntactic* callee, which is what convention matching wants.
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub callee: SmolStr,
    /// The first string-literal argument's value, unescaped.
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub literal: SmolStr,
    /// The whole call expression's extent.
    pub span: Span,
}

// ---------------------------------------------------------------- manifests

#[derive(Debug, Clone)]
pub struct ManifestDependency {
    pub name: SmolStr,
    pub version_req: SmolStr,
    pub scope: DependencyScope,
    /// True when `version_req` is a placeholder — the real requirement lives in a
    /// project-wide shared version pool instead of this manifest (Cargo's
    /// `{ workspace = true }` today; the mechanism generalizes to any adapter with an
    /// equivalent concept — pnpm/Gradle version catalogs, etc.). Resolved against
    /// [`ManifestFacts::workspace_dependencies`] during graph assembly, before any analysis
    /// ever sees the value — `version_req` here is not meaningful to compare directly.
    pub inherited: bool,
}

/// A manifest-declared root, already resolved to a concrete file (unlike [`RawRoot`], which
/// targets something *within* the file being extracted — a manifest root always names a
/// *different* file, so the adapter resolves it itself against [`ResolveCtx`] rather than
/// handing the core an unresolved specifier to guess at).
#[derive(Debug, Clone)]
pub struct ManifestRoot {
    pub kind: RootKind,
    pub target: ProjectPath,
    pub confidence: Confidence,
}

/// Declared dependencies AND package identity/topology.
#[derive(Debug, Default)]
pub struct ManifestFacts {
    pub package_name: Option<SmolStr>,
    /// Publish signal: `private: true` and friends → app mode; absent → library mode.
    pub private: bool,
    /// Workspace membership declarations (globs).
    pub workspace_members: Vec<SmolStr>,
    /// The shared version pool that other manifests' `inherited` dependencies resolve
    /// against (Cargo: `[workspace.dependencies]`). Only populated by an adapter that has
    /// the concept — empty otherwise, and assembly never assumes exactly one manifest
    /// declares it.
    pub workspace_dependencies: Vec<ManifestDependency>,
    pub dependencies: Vec<ManifestDependency>,
    /// Entry-point specifiers (main/module/exports/bin/types), raw and unresolved —
    /// resolution input for self-referencing imports (a package importing its own name),
    /// not yet consumed.
    /// Root-worthiness is a separate, already-resolved fact: see `roots`.
    pub entry_points: Vec<SmolStr>,
    /// Roots this manifest declares, already resolved to concrete files (the
    /// library-mode rule): `bin` targets unconditionally, plus `main`/`module`/`exports`
    /// targets when the package isn't `private` (an unpublished app's exports are not roots —
    /// something must actually import them). `types`/`typings` never contribute: `.d.ts` is
    /// declarations only, no runtime edge.
    pub roots: Vec<ManifestRoot>,
    /// The package's import entry points, resolved to concrete files in precedence order
    /// (main > module > exports leaves) — what a sibling's bare-name import of this package
    /// lands on (the core's `WorkspaceMember.entry` source). Unlike `roots`,
    /// NOT gated on `private`: a private package has no self-standing roots, but a sibling
    /// importing it by name still resolves through its entry. `bin` is excluded — an
    /// executable is invoked, never imported through.
    pub resolved_entries: Vec<(ProjectPath, Confidence)>,
    /// Whether an explicit surface is declared (`exports` map or equivalent) — the
    /// contract gate for `deep-import`.
    pub declares_surface: bool,
    /// Names invoked as the leading command of a `scripts` clause (`"test": "xo && ava"` →
    /// `["xo", "ava"]`) — a CLI-only tool never gets an `ImportsDependency` edge (nothing
    /// `import`s a binary), so without this signal a real, actively-invoked devDependency
    /// reads as `unused` by dependency hygiene despite genuinely being used,
    /// just not through source code. Distinct from `roots`: a command name doesn't resolve to
    /// a file, so it can never itself be a root — this only ever feeds dependency-usage
    /// classification, cross-referenced against declared dependency names downstream (a name
    /// that happens to match nothing declared is simply never looked up).
    pub script_invoked_names: Vec<SmolStr>,
    /// The manifest's **named executable targets** (the invoked-program rule): the
    /// name a build tool exposes the binary under, resolved to its entry file by the
    /// adapter (same posture as [`Self::roots`] — a manifest always names a *different*
    /// file). Cargo: `src/main.rs` under the package name, `src/bin/foo.rs` under `foo`,
    /// `[[bin]] name`/`path` as declared; npm's `bin` map is the same shape. This is what a
    /// source file's [`FileFacts::invoked_executables`] resolves against — the name is the
    /// process-boundary identity, which no import specifier carries.
    pub executables: Vec<ExecutableTarget>,
    /// Per-target unit assignment the path CONVENTION can't derive: SwiftPM's
    /// `.target(name: "SomeKit", path: "Source")` puts a whole module outside
    /// `Sources/<name>/`. Each entry is `(path prefix, unit name)`; assembly assigns the
    /// unit to files under the prefix whose extraction left `unit` unset — the convention,
    /// where it fired, already told the truth. Longest prefix wins.
    pub unit_overrides: Vec<(ProjectPath, SmolStr)>,
    pub diagnostics: Vec<AdapterDiagnostic>,
}

/// One named executable target: invoking `name` as a subprocess executes `entry`. Carries
/// rkyv derives because the graph snapshot persists these per package (`PackageNode`) — the
/// incremental patch rebuilds the name index without re-extracting unchanged manifests.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ExecutableTarget {
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub name: SmolStr,
    pub entry: ProjectPath,
}

// ---------------------------------------------------------------- resolution

#[derive(Debug, Clone)]
pub struct ImportSpec {
    pub specifier: SmolStr,
    pub from: ProjectPath,
}

/// One workspace member as resolvers see it: a named in-repo package a bare
/// specifier can resolve *into*. Built by the core from every named manifest's facts after
/// manifest extraction — "name matches against sibling manifests" needs no
/// workspace-glob gating: an in-repo manifest whose `name` matches the specifier is the
/// resolution regardless of how the workspace topology declares it (a name match to a
/// non-member would be a defect in the repo itself, not a resolution ambiguity).
#[derive(Debug, Clone)]
pub struct WorkspaceMember {
    /// The manifest's directory, project-relative (`""` for a root manifest) — the base
    /// subpath specifiers (`@org/ui/button`) resolve against.
    pub dir: SmolStr,
    /// The member's primary entry, resolved by its own adapter at manifest-extraction time
    /// (first of main/module/exports that named a real file) — what the bare name resolves
    /// to. Deliberately NOT gated on `private` like root-worthiness is: a private sibling is
    /// still imported *through its entry* by other members; privateness only says its exports
    /// aren't roots on their own.
    pub entry: Option<(ProjectPath, Confidence)>,
    /// Every resolved target file the member's manifest declares (`PackageNode::targets`):
    /// the module-tree anchors a resolver can anchor intra-package paths on when the
    /// language's directory convention doesn't hold (a Rust `[[bin]] path` placing a module
    /// tree outside `src/`).
    pub targets: Vec<ProjectPath>,
}

/// Index of claimable paths and manifest facts the core exposes to resolvers — populated from
/// discovery and manifest extraction. Adapters only ever *query* it; they never touch the
/// filesystem themselves (the purity rule). Read-only by construction.
pub struct ResolveCtx<'a> {
    known_files: &'a rustc_hash::FxHashSet<ProjectPath>,
    /// Dependency names declared in the importing file's package manifest. Feeds the
    /// declared-beats-stdlib-list shadowing rule; empty until the engine wires
    /// manifest facts through.
    declared_dependencies: Option<&'a rustc_hash::FxHashSet<SmolStr>>,
    /// Named in-repo packages, keyed by declared package name. Empty during
    /// manifest extraction itself (the map is *built from* manifest facts — no circularity),
    /// populated for import resolution.
    workspace_members: Option<&'a rustc_hash::FxHashMap<SmolStr, WorkspaceMember>>,
    /// Every claimed file's `unit`, reverse-indexed: unit key → every file that
    /// declares it, sorted. Exists for Java: an
    /// import specifier there IS a `unit` value (the declared package name) directly — unlike
    /// Go, which turns a specifier into a directory and uses [`Self::files_in_dir`], Java has
    /// no reliable specifier→directory mapping (the source root isn't visible to a bare
    /// dotted package name), so resolution needs the reverse lookup this index provides.
    units: Option<&'a rustc_hash::FxHashMap<SmolStr, Vec<ProjectPath>>>,
    /// The same reverse index, partitioned by the owning package (nearest-manifest-ancestor).
    /// A unit key is only unique *within* a package: Java and Kotlin key units on the declared
    /// package name (RFC 0012 §8 — never directory-derived, deliberately, since a source root
    /// is a build-tool convention a bare dotted name can't reveal), so two Gradle modules that
    /// both declare `package retrofit2;` share one unit key across the whole repo. Swift keys
    /// on the target name, and two packages may each declare a target `Core`. Left unset by
    /// callers that have no ownership map; [`Self::unit_files_from`] then behaves exactly like
    /// [`Self::unit_files`].
    units_by_package:
        Option<&'a rustc_hash::FxHashMap<u32, rustc_hash::FxHashMap<SmolStr, Vec<ProjectPath>>>>,
    /// Which package owns each claimed file — the lookup that makes `units_by_package` usable
    /// from a resolver, which knows only the importing file's path.
    file_package: Option<&'a rustc_hash::FxHashMap<ProjectPath, u32>>,
}

impl<'a> ResolveCtx<'a> {
    pub fn new(known_files: &'a rustc_hash::FxHashSet<ProjectPath>) -> Self {
        ResolveCtx {
            known_files,
            declared_dependencies: None,
            workspace_members: None,
            units: None,
            units_by_package: None,
            file_package: None,
        }
    }

    pub fn with_declared_dependencies(mut self, deps: &'a rustc_hash::FxHashSet<SmolStr>) -> Self {
        self.declared_dependencies = Some(deps);
        self
    }

    pub fn with_workspace_members(
        mut self,
        members: &'a rustc_hash::FxHashMap<SmolStr, WorkspaceMember>,
    ) -> Self {
        self.workspace_members = Some(members);
        self
    }

    pub fn with_units(
        mut self,
        units: &'a rustc_hash::FxHashMap<SmolStr, Vec<ProjectPath>>,
    ) -> Self {
        self.units = Some(units);
        self
    }

    /// Package-partitioned units plus the file→package lookup they're keyed by. Optional: a
    /// context without them resolves units repo-globally, exactly as before.
    pub fn with_package_units(
        mut self,
        units_by_package: &'a rustc_hash::FxHashMap<
            u32,
            rustc_hash::FxHashMap<SmolStr, Vec<ProjectPath>>,
        >,
        file_package: &'a rustc_hash::FxHashMap<ProjectPath, u32>,
    ) -> Self {
        self.units_by_package = Some(units_by_package);
        self.file_package = Some(file_package);
        self
    }

    pub fn contains(&self, path: &ProjectPath) -> bool {
        self.known_files.contains(path)
    }

    pub fn is_declared_dependency(&self, name: &SmolStr) -> bool {
        self.declared_dependencies.is_some_and(|d| d.contains(name))
    }

    pub fn workspace_member(&self, name: &str) -> Option<&'a WorkspaceMember> {
        self.workspace_members.and_then(|m| m.get(name))
    }

    /// Every named workspace member, unordered — for resolvers that need to locate the
    /// member owning a *directory* (e.g. anchoring intra-package module paths on its
    /// declared targets) rather than look one up by name.
    pub fn workspace_members_iter(&self) -> impl Iterator<Item = &'a WorkspaceMember> {
        self.workspace_members.into_iter().flat_map(|m| m.values())
    }

    /// Every known file declaring `unit` (empty when none do, or `with_units` was never
    /// called). Sorted by path — deterministic which entry a caller picking `.first()` gets.
    ///
    /// Repo-global, so it cannot tell two same-named units in different packages apart. Prefer
    /// [`Self::unit_files_from`], which can.
    pub fn unit_files(&self, unit: &str) -> &'a [ProjectPath] {
        self.units
            .and_then(|u| u.get(unit))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// [`Self::unit_files`] resolved from the perspective of the importing file: candidates in
    /// the importer's OWN package win, and only when it has none does the repo-global set
    /// answer.
    ///
    /// Both halves are load-bearing. Preferring the importer's package is what stops a module
    /// from binding an import to a same-named package in an unrelated sibling module — the
    /// resolver picks `.first()` by path order, so `retrofit/` importing `retrofit2.X` could
    /// land in `android-test/`, inventing a cross-module edge that `cyclic` then reports as a
    /// package cycle neither module's source supports. Falling back is what keeps *genuine*
    /// cross-module imports working: a package that genuinely lives only in a sibling module
    /// (guava's modules really do import each other) has no local candidate, and the global
    /// set is the right answer there. Reachability was always insulated from the choice —
    /// same-unit fallback makes every file in a unit reachable whichever one an edge lands on
    /// — but the literal edge is evidence, and cycle detection reads it as such.
    pub fn unit_files_from(&self, unit: &str, from: &ProjectPath) -> &'a [ProjectPath] {
        let local = self
            .file_package
            .and_then(|fp| fp.get(from))
            .zip(self.units_by_package)
            .and_then(|(pkg, by_pkg)| by_pkg.get(pkg))
            .and_then(|units| units.get(unit))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if local.is_empty() {
            return self.unit_files(unit);
        }
        local
    }

    /// Every known file whose *immediate* directory equals `dir` (`""` = project root) — one
    /// more path segment, not a recursive subtree. Exists for languages whose import unit is a
    /// directory rather than a single file (Go's
    /// package): resolving `foo.com/bar/sub` needs *some* concrete file in `sub/` to anchor an
    /// `ImportsFile` edge on, and there's no naming convention (unlike JS's `index.*`) to guess
    /// one from. Read-only, like every other `ResolveCtx` query; callers needing a deterministic
    /// pick sort the result themselves — iteration order here follows the
    /// underlying set, not insertion or path order.
    pub fn files_in_dir(&self, dir: &str) -> impl Iterator<Item = &'a ProjectPath> {
        let dir = dir.to_string();
        self.known_files
            .iter()
            .filter(move |p| match p.0.rfind('/') {
                Some(i) => p.0[..i] == dir,
                None => dir.is_empty(),
            })
    }

    /// Every known file under `dir` at any depth (`""` = every file in the project) — the
    /// recursive counterpart to [`Self::files_in_dir`]. Exists for
    /// Java: a publishable module's root promotion needs every `.java` file under its source
    /// root (`src/main/java/**`, arbitrary package nesting), not one representative file the
    /// way Go's directory-is-the-package model needs. Read-only; no ordering guarantee, same
    /// as `files_in_dir`.
    pub fn files_under(&self, dir: &str) -> impl Iterator<Item = &'a ProjectPath> {
        let is_root = dir.is_empty();
        let prefix = if is_root {
            String::new()
        } else {
            format!("{dir}/")
        };
        self.known_files
            .iter()
            .filter(move |p| is_root || p.0.starts_with(&prefix))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    File(ProjectPath, Confidence),
    Dependency(SmolStr, Confidence),
    /// A bare specifier that resolved *into* a named in-repo package: `target`
    /// is the concrete internal file (the member's entry, or a subpath into it — deep
    /// imports included; their edges are always recorded, the `deep-import`
    /// verdict is a separate concern), and `name` is the member's package name. Distinct from plain
    /// `File` because assembly derives BOTH edge kinds from it: `ImportsFile` (reachability
    /// is real, cross-package) and `ImportsDependency` (the declaration contract is real
    /// too — the table validates it both ways: declared-but-unimported workspace
    /// deps are `unused`, imported-but-undeclared siblings are phantom `undeclared`).
    WorkspaceMember {
        name: SmolStr,
        target: ProjectPath,
        confidence: Confidence,
        /// The import resolves *within the importing file's own package* (a package's tests
        /// or binaries naming its library by package name). The file edge and symbol
        /// bindings are as real as any sibling import — but no declaration contract exists
        /// for a package depending on itself (no ecosystem lets a manifest require its own
        /// package), so assembly derives `ImportsFile` and skips `ImportsDependency`:
        /// neither an `undeclared` accusation ("phantom dependency on itself") nor a
        /// dependency-usage credit can be right. `false` for genuine siblings, where the
        /// declaration contract holds both ways.
        same_package: bool,
    },
    Stdlib,
    Unresolved,
}

// ---------------------------------------------------------------- the trait

/// One implementation per language, registered at startup. The boundary:
/// adapters own what the language *spec* defines — nothing ecosystem-shaped.
pub trait LanguageAdapter: Send + Sync {
    fn descriptor(&self) -> AdapterDescriptor;

    /// Claim & classify a path (fast; name-based, content peeking only when unavoidable).
    fn claim(&self, path: &ProjectPath) -> Option<FileClaim>;

    /// Is this path one of this adapter's manifest files (`package.json`, …)? Manifests are
    /// claimed separately from source (`claim`) — they never get a [`FileClaim`]/language of
    /// their own ("manifests are not claimed"), only manifest facts. Defaults to `false` —
    /// mirrors the WASM v1 ABI's own scope cut (no manifest extraction over that boundary
    /// yet): a language with no manifest concept of its own (CSS, JSON) needs no override at
    /// all, rather than a stub that only exists to satisfy the trait.
    fn claim_manifest(&self, _path: &ProjectPath) -> bool {
        false
    }

    /// Parse one file and extract every language-defined fact. Must not fail on broken code:
    /// return partial facts + diagnostics.
    fn extract(&self, file: &SourceFile<'_>) -> FileFacts;

    /// Parse a manifest into declared dependencies, package identity/topology, and roots.
    /// `ctx` lets the adapter resolve entry-point specifiers (main/module/exports/bin) against
    /// the known-files index itself — the same Node-resolution knowledge `resolve()` already
    /// owns, not something the core can generically guess at. Defaults to
    /// `ManifestFacts::default()` (empty) — unreachable in practice whenever
    /// [`claim_manifest`](Self::claim_manifest) keeps its own `false` default, kept only for
    /// trait completeness (same posture the WASM bridge documents at its own call site).
    fn extract_manifest(&self, _file: &SourceFile<'_>, _ctx: &ResolveCtx<'_>) -> ManifestFacts {
        ManifestFacts::default()
    }

    /// Resolve an import specifier to a concrete target. Called by the core's resolution
    /// driver — including for specifiers emitted by *other* adapters.
    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet as HashSet;

    fn files(paths: &[&str]) -> HashSet<ProjectPath> {
        paths
            .iter()
            .map(|p| ProjectPath(SmolStr::new(*p)))
            .collect()
    }

    #[test]
    fn files_in_dir_matches_immediate_children_only() {
        let known = files(&["pkg/a.go", "pkg/b.go", "pkg/sub/deeper.go", "root.go"]);
        let ctx = ResolveCtx::new(&known);
        let mut got: Vec<&str> = ctx.files_in_dir("pkg").map(|p| p.0.as_str()).collect();
        got.sort();
        assert_eq!(got, vec!["pkg/a.go", "pkg/b.go"]);
    }

    #[test]
    fn files_in_dir_empty_string_means_project_root() {
        let known = files(&["root.go", "pkg/a.go"]);
        let ctx = ResolveCtx::new(&known);
        let got: Vec<&str> = ctx.files_in_dir("").map(|p| p.0.as_str()).collect();
        assert_eq!(got, vec!["root.go"]);
    }

    #[test]
    fn files_in_dir_with_no_matches_is_empty() {
        let known = files(&["pkg/a.go"]);
        let ctx = ResolveCtx::new(&known);
        assert_eq!(ctx.files_in_dir("nowhere").count(), 0);
    }

    #[test]
    fn files_under_matches_at_any_depth() {
        let known = files(&[
            "src/main/java/com/foo/A.java",
            "src/main/java/com/foo/bar/B.java",
            "src/test/java/com/foo/ATest.java",
            "pom.xml",
        ]);
        let ctx = ResolveCtx::new(&known);
        let mut got: Vec<&str> = ctx
            .files_under("src/main/java")
            .map(|p| p.0.as_str())
            .collect();
        got.sort();
        assert_eq!(
            got,
            vec![
                "src/main/java/com/foo/A.java",
                "src/main/java/com/foo/bar/B.java",
            ]
        );
    }

    #[test]
    fn files_under_empty_string_means_every_file() {
        let known = files(&["a.go", "pkg/b.go"]);
        let ctx = ResolveCtx::new(&known);
        assert_eq!(ctx.files_under("").count(), 2);
    }

    #[test]
    fn files_under_does_not_match_a_sibling_with_a_shared_prefix() {
        // "pkg-extra/file.go" must NOT match "pkg" — the trailing "/" join prevents a
        // partial-segment collision.
        let known = files(&["pkg/a.go", "pkg-extra/file.go"]);
        let ctx = ResolveCtx::new(&known);
        let got: Vec<&str> = ctx.files_under("pkg").map(|p| p.0.as_str()).collect();
        assert_eq!(got, vec!["pkg/a.go"]);
    }
}
