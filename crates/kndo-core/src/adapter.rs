//! The `LanguageAdapter` contract (contracts/core-traits.md §2).
//!
//! An adapter is the only component that understands a language: it translates source files
//! into the neutral vocabulary. Adapters are pure with respect to the filesystem — all content
//! arrives via parameters (determinism, sandboxing, testing — RFC 0002 §6) — and own exactly
//! what the language specification defines; ecosystem knowledge belongs to plugins.

use smol_str::SmolStr;

use crate::vocab::{Confidence, DependencyScope, FileClass, RootKind, SymbolKind};

// ---------------------------------------------------------------- shared primitives

/// Project-relative path with `/` separators, the only path form that crosses the adapter
/// boundary (case handling and symlink resolution are the core's discovery concern).
#[derive(
    Debug,
    Clone,
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct ProjectPath(#[rkyv(with = crate::rkyv_support::SmolStrAsString)] pub SmolStr);

/// 1-indexed line/column span, `start` inclusive, `end` exclusive. Serializes as the
/// `[line, col]` pair shape the output schema uses (contracts/output-schema.md §2), not an
/// object — tuples serialize as JSON arrays by default.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Span {
    pub start: (u32, u32),
    pub end: (u32, u32),
}

/// A file's content as handed to an adapter by the core. Adapters never read the fs.
#[derive(Debug)]
pub struct SourceFile<'a> {
    pub path: &'a ProjectPath,
    pub content: &'a [u8],
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Warn,
    Info,
}

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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    /// The file this diagnostic is about, when there is one — `None` for project-level
    /// diagnostics (e.g. "cannot walk the project root"). A diagnostic merged from many
    /// files without this field would be unattributable; adapters emit diagnostics scoped
    /// to the file they're extracting, the core fills this in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<ProjectPath>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

// ---------------------------------------------------------------- descriptor & claiming

#[derive(Debug, Clone)]
pub struct AdapterDescriptor {
    /// e.g. "js-ts". Participates in cache keys. RFC 0016 §4 migrates these to component
    /// coordinates (`kndo:js-ts`, `github.com/owner/repo`) when adapters componentize —
    /// deferred to that RFC's phase 2 because renaming ids churns cache keys.
    pub id: SmolStr,
    /// Bumping invalidates only this adapter's cached facts (RFC 0004 §3).
    pub facts_schema_version: u32,
    pub file_globs: Vec<SmolStr>,
    pub manifest_globs: Vec<SmolStr>,
    pub grammar_version: SmolStr,
    /// The ladder [`VisibilityLevel`] indexes into (RFC 0012 §6) — index = level. Empty means
    /// the language has no visibility semantics (CSS, JSON): visibility analyses skip its
    /// files entirely, and its member declarations (it should have none) are treated as
    /// `Public` by the fallback's conservative default. Assembly carries the ladder onto the
    /// graph keyed by claim language, so analyses (pure graph functions) never touch adapters.
    pub visibility_ladder: Vec<VisibilityRung>,
    /// Cycle tolerance per graph level (RFC 0005 §8) — same data-on-the-descriptor pattern as
    /// the ladder: assembly carries it onto the graph keyed by claim language, and `cyclic`
    /// maps `Hazard → warning`, `Idiomatic → info`, `Impossible → skip the level`.
    pub cycle_policy: CyclePolicy,
    /// Whether this adapter's `resolve()` can ever produce an `ImportsDependency` edge for a
    /// manifest-declared dependency of this language (RFC 0005 §5). `true` for every language
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
    /// Dormant reservation (RFC 0016 §8 phase 0): the machine-checkable activation predicates
    /// a *globally installed* adapter will be gated by when adapters componentize (RFC 0016
    /// §4 — same rules and semantics as `PluginDescriptor::activation`). Nothing evaluates
    /// this yet; compiled-in and project-local adapters are scoped by their file claims alone.
    /// Reserved before the 1.0 freeze so componentization is additive, not breaking.
    pub activation: Vec<crate::plugin::ActivationRule>,
    /// Dormant reservation (RFC 0016 §8 phase 0): component dependencies by coordinate id,
    /// with RFC 0015 §3's co-install/co-activate semantics once RFC 0016 §4 lands. Unread
    /// today, same reservation rationale as [`activation`](Self::activation).
    pub dependencies: Vec<SmolStr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileClaim {
    pub language: SmolStr,
    pub class: FileClass,
}

// ---------------------------------------------------------------- extracted facts

/// Ladder index on the adapter-declared visibility ladder (RFC 0005 §7): 0 = most private,
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

/// What the core can *check* about a visibility level (RFC 0012 §6): the graph region a
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
    /// Same `PackageId` (RFC 0011 manifest ownership) — contains `Unit`.
    Package,
    /// Everywhere.
    Public,
}

/// One rung of an adapter's visibility ladder (RFC 0012 §6): the **scope** is what the core
/// checks; the **label** is the language's own word for the level, used verbatim in
/// remediation text (RFC 0005 §7: "in the language's own terms, supplied by the adapter").
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
}

/// How a language's ecosystem regards an import cycle at one graph level (RFC 0005 §8) —
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
    /// Idiomatic and routinely tolerated (Rust modules within a crate) → severity `info`.
    Idiomatic,
    /// The compiler/toolchain forbids it outright (Go package imports) — a cycle at this
    /// level cannot exist in building code, so the analysis skips the level entirely rather
    /// than accusing what must be a resolution artifact.
    Impossible,
}

/// An adapter's cycle tolerance per graph level (RFC 0005 §8): file-import cycles, and
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
    /// (RFC 0012 §3). Display joins them (`T.Method`); resolution reasons about them apart.
    pub name: SmolStr,
    pub kind: SymbolKind,
    pub span: Span,
    pub exported: bool,
    pub visibility: VisibilityLevel,
    /// The owning type's declared name, when this declaration is a member of one (a Go
    /// method's receiver type, a class method's class, an enum member's enum — RFC 0012 §3's
    /// per-language table). `None` for free-standing declarations. Members resolve differently
    /// from free names: an unqualified reference never `certain`-resolves to a member — it
    /// reaches members only through the duck-typed fallback (`graph::assemble` phase 3b),
    /// at `Probable`/`Possible`, per RFC 0002 §5's ladder. A [`RawRoot`] targeting a member
    /// names it in qualified `Owner.name` form.
    pub member_of: Option<SmolStr>,
    /// The sub-span covering this declaration's *signature* — parameters and return/result
    /// types, everything before the body (RFC 0012 §5). Only the adapter knows where a body
    /// starts; the core must not. `Some` on callables; `None` where the concept doesn't apply
    /// (a type's leak surface is its member fields, which land with member extraction — v1 of
    /// `private-type-leak` is deliberately callables-only rather than falsely accusing
    /// exported-struct/unexported-field shapes).
    pub signature_span: Option<Span>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawReference {
    pub name: SmolStr,
    /// The qualifier text of a qualified access (RFC 0012 §9): `json.Marshal` is
    /// `{ name: "Marshal", scope_context: Some("json") }`. Assembly resolves the qualifier
    /// against the file's imports — the explicit [`RawImport::local_alias`], or (unaliased)
    /// the resolved target's [`FileFacts::unit_name`] — and, on a match, resolves `name`
    /// inside that target's declarations at `Certain`. A qualifier matching no import is a
    /// receiver expression (`t.helper()`): the name is a *member* access by construction, so
    /// it skips the free-name tables entirely and goes straight to the duck-typed member
    /// fallback (RFC 0012 §3). `None` = an unqualified name, resolved as before.
    pub scope_context: Option<SmolStr>,
    pub span: Span,
    /// The declared symbol this reference executes *inside* (RFC 0012 §4), under one
    /// language-blind rule: **`within` = the symbol whose use triggers this code.** Bodies of
    /// callables → that callable (a member body names it in qualified `Owner.name` form, same
    /// convention as member root targets); code that runs at module/file *load* (top-level
    /// statements, package-level initializers) → `None`; code that runs on a type's
    /// instantiation/first use (constructors, field initializers) → that type. Assembly
    /// resolves it against the file's own declarations and attributes the `References` edge to
    /// that symbol — so a dead function's calls no longer keep its callees alive (transitive
    /// death becomes visible). **Any `within` that doesn't resolve falls back to file
    /// attribution — today's over-approximation, the safe direction** — and `None` (every
    /// adapter that doesn't emit it) reproduces prior behavior exactly.
    pub within: Option<SmolStr>,
    /// What kind of use this is (RFC 0012 §5) — `TypeUse` for type-position references (the
    /// fact `private-type-leak` is built on), `Extend`/`Implement` for inheritance clauses,
    /// `Read` otherwise. Assembly passes it straight onto the `References` edge; adapters that
    /// don't differentiate emit `Read`, the pre-§5 behavior.
    pub kind: crate::vocab::RefKind,
}

/// Syntactic shape only — what the specifier text looks like, not what it resolves to.
/// Builtins (`node:fs`, bare `fs`) are a *resolution*-time fact (the resolver owns the
/// builtins list, RFC 0002 §5); extraction never claims `Stdlib`.
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
    /// `import "./polyfill"` — counts as usage without binding names (RFC 0005 §5).
    pub side_effect_only: bool,
    /// `import type { T } from "..."` / type-only re-export — only the adapter can know
    /// this; it decides whether the resulting edge is a value reference or `TypeUse`
    /// (docs/adapters/js-ts.md §3).
    pub type_only: bool,
    pub confidence: Confidence,
    /// Empty for side-effect-only imports and (for now) namespace imports (`import * as ns`) —
    /// resolving `ns.foo` back to a specific export needs member-expression-aware reference
    /// resolution this slice doesn't attempt; the import edge itself is unaffected, only the
    /// finer-grained "this binding referenced that export" fact is missed.
    pub bindings: Vec<ImportBinding>,
    /// `export { a } from "./b"` / `export * from "./b"` — a re-export, not a plain import:
    /// `bindings` become part of *this file's own* export surface too, so another file
    /// importing `a` from here should resolve straight through to `./b`'s original declaration
    /// (js-ts.md §5: "Barrel files… resolved through, transparently"). `false` for an ordinary
    /// `import` statement, which only makes a name usable inside the importing file.
    pub reexported: bool,
    /// The imported namespace is consumed in ways static tracking can't follow — a computed
    /// member access (`ns[key]`) or the namespace value escaping into a call/assignment/return.
    /// Assembly then adds a `Wildcard` edge *from the resolved target file*, making every
    /// symbol in it plausibly used (`possible`) — the RFC 0005 §1 "wildcard over that
    /// namespace's exports" rule. Statically-tracked accesses (`ns.foo`) don't set this; they
    /// resolve precisely through `bindings` instead.
    pub opaque_namespace_use: bool,
    /// The *explicit* local alias this import binds its target under (RFC 0012 §9): Go's
    /// `import j "encoding/json"` → `Some("j")`. `None` for unaliased imports — assembly then
    /// derives the qualifier from the resolved target's own [`FileFacts::unit_name`], which is
    /// the correct-by-construction answer the old adapter-side "last specifier segment" guess
    /// approximated (and got wrong for dir≠package mismatches like `gopkg.in/yaml.v3` →
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
    pub cyclomatic: u32,
    pub loc: u32,
    /// Normalized-stream token count — `health`'s duplication ratio basis (RFC 0005 §11:
    /// "duplicated tokens / total tokens").
    pub token_count: u32,
    /// Winnowing fingerprints over the normalized token stream (RFC 0005 §6).
    pub fingerprints: Vec<u64>,
}

/// A construct forcing a `Wildcard` edge (RFC 0005 §1 plausible-target-set expansion).
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
    /// set at the RFC's default: the dynamic file's own symbols.
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
/// logic, identical across languages (contracts §2.1 — the no-flicker guarantee lives there).
/// Carries its own rkyv derives (not just serde's) so it survives the graph-snapshot cache
/// (ADR 0004) unchanged — a graph-snapshot hit skips `FileFacts` extraction entirely, so without
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

/// Everything an adapter owes the core for one file. Must be deterministic for identical
/// content (conformance harness, RFC 0002 §8). Round-trips through the facts cache
/// (`cache.rs`, ADR 0004) — `Deserialize` exists for that alone, never for adapters to read.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FileFacts {
    pub declarations: Vec<Declaration>,
    pub references: Vec<RawReference>,
    pub imports: Vec<RawImport>,
    pub roots: Vec<RawRoot>,
    pub functions: Vec<FunctionMetrics>,
    pub dynamics: Vec<DynamicUse>,
    pub suppressions: Vec<RawSuppression>,
    pub diagnostics: Vec<Diagnostic>,
    /// Reference-resolution scope, when the language's isn't file-scoped (RFC 0002 §2, contract
    /// extension surfaced by the Go adapter, M3): files sharing the same non-`None` key resolve
    /// each other's declarations for an unqualified [`RawReference`] with no import binding, in
    /// addition to their own. `None` (every adapter before Go) keeps today's exact behavior —
    /// same-file-only, unless an import binds the name. Exists because file-scoped resolution is
    /// a JS/TS-ism, not a universal: Go's visibility unit is the *package* (its containing
    /// directory) — two files in one package call each other's unexported functions with no
    /// `import` at all, the ordinary, common case, not an edge case a per-file model can treat as
    /// safely-wrong. The adapter computes the key (for Go: the file's directory, from its own
    /// path — no extra input needed); the core only groups by it, staying language-blind.
    pub unit: Option<SmolStr>,
    /// Content-derived correction of the claim-time origin axis (RFC 0012 §7): extraction may
    /// report what the *content* proves about origin — a `// Code generated … DO NOT EDIT.`
    /// banner, an `@generated` marker — which claim (path-only, by design fast and name-based)
    /// cannot see. Assembly applies the override when building the `FileNode`, before any
    /// role-derived roots, so every origin exemption (`unused`, `test-only`, `untested`,
    /// `internal-only`, `private-type-leak` all exempt `Generated`) sees the corrected value.
    /// Role stays claim-time — no use case justifies content-derived roles. `None` = the
    /// claim-time origin stands. Rides the facts cache like every other content-derived fact.
    pub detected_origin: Option<crate::vocab::FileOrigin>,
    /// The name *importers bind this unit by* (RFC 0012 §9): Go's `package` clause name,
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
}

// ---------------------------------------------------------------- manifests (RFC 0011)

#[derive(Debug, Clone)]
pub struct ManifestDependency {
    pub name: SmolStr,
    pub version_req: SmolStr,
    pub scope: DependencyScope,
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

/// Declared dependencies AND package identity/topology (RFC 0011 §3).
#[derive(Debug, Default)]
pub struct ManifestFacts {
    pub package_name: Option<SmolStr>,
    /// Publish signal: `private: true` and friends → app mode; absent → library mode.
    pub private: bool,
    /// Workspace membership declarations (globs).
    pub workspace_members: Vec<SmolStr>,
    pub dependencies: Vec<ManifestDependency>,
    /// Entry-point specifiers (main/module/exports/bin/types), raw and unresolved — future
    /// resolution input for self-referencing imports (a package importing its own name).
    /// Root-worthiness is a separate, already-resolved fact: see `roots`.
    pub entry_points: Vec<SmolStr>,
    /// Roots this manifest declares, already resolved to concrete files (RFC 0011 §5, RFC 0005
    /// §1's library-mode rule): `bin` targets unconditionally, plus `main`/`module`/`exports`
    /// targets when the package isn't `private` (an unpublished app's exports are not roots —
    /// something must actually import them). `types`/`typings` never contribute: `.d.ts` is
    /// declarations only, no runtime edge (docs/adapters/js-ts.md §1).
    pub roots: Vec<ManifestRoot>,
    /// The package's import entry points, resolved to concrete files in precedence order
    /// (main > module > exports leaves) — what a sibling's bare-name import of this package
    /// lands on (RFC 0011 §4, the core's `WorkspaceMember.entry` source). Unlike `roots`,
    /// NOT gated on `private`: a private package has no self-standing roots, but a sibling
    /// importing it by name still resolves through its entry. `bin` is excluded — an
    /// executable is invoked, never imported through.
    pub resolved_entries: Vec<(ProjectPath, Confidence)>,
    /// Whether an explicit surface is declared (`exports` map or equivalent) — the
    /// contract gate for `deep-import` (RFC 0011 §4).
    pub declares_surface: bool,
    /// Names invoked as the leading command of a `scripts` clause (`"test": "xo && ava"` →
    /// `["xo", "ava"]`) — a CLI-only tool never gets an `ImportsDependency` edge (nothing
    /// `import`s a binary), so without this signal a real, actively-invoked devDependency
    /// reads as `unused` by dependency hygiene (RFC 0005 §5) despite genuinely being used,
    /// just not through source code. Distinct from `roots`: a command name doesn't resolve to
    /// a file, so it can never itself be a root — this only ever feeds dependency-usage
    /// classification, cross-referenced against declared dependency names downstream (a name
    /// that happens to match nothing declared is simply never looked up).
    pub script_invoked_names: Vec<SmolStr>,
    pub diagnostics: Vec<Diagnostic>,
}

// ---------------------------------------------------------------- resolution

#[derive(Debug, Clone)]
pub struct ImportSpec {
    pub specifier: SmolStr,
    pub from: ProjectPath,
}

/// One workspace member as resolvers see it (RFC 0011 §4): a named in-repo package a bare
/// specifier can resolve *into*. Built by the core from every named manifest's facts after
/// manifest extraction — "name matches against sibling manifests" (js-ts.md §3) needs no
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
}

/// Index of claimable paths and manifest facts the core exposes to resolvers — populated from
/// discovery and manifest extraction. Adapters only ever *query* it; they never touch the
/// filesystem themselves (the purity rule, RFC 0002 §6). Read-only by construction.
pub struct ResolveCtx<'a> {
    known_files: &'a rustc_hash::FxHashSet<ProjectPath>,
    /// Dependency names declared in the importing file's package manifest. Feeds the
    /// declared-beats-stdlib-list shadowing rule (RFC 0002 §6); empty until the engine wires
    /// manifest facts through.
    declared_dependencies: Option<&'a rustc_hash::FxHashSet<SmolStr>>,
    /// Named in-repo packages, keyed by declared package name (RFC 0011 §4). Empty during
    /// manifest extraction itself (the map is *built from* manifest facts — no circularity),
    /// populated for import resolution.
    workspace_members: Option<&'a rustc_hash::FxHashMap<SmolStr, WorkspaceMember>>,
    /// Every claimed file's `unit` (RFC 0002 §2), reverse-indexed: unit key → every file that
    /// declares it, sorted (RFC 0008 §4). Added for Java (docs/adapters/java.md §3): an
    /// import specifier there IS a `unit` value (the declared package name) directly — unlike
    /// Go, which turns a specifier into a directory and uses [`Self::files_in_dir`], Java has
    /// no reliable specifier→directory mapping (the source root isn't visible to a bare
    /// dotted package name), so resolution needs the reverse lookup this index provides.
    units: Option<&'a rustc_hash::FxHashMap<SmolStr, Vec<ProjectPath>>>,
}

impl<'a> ResolveCtx<'a> {
    pub fn new(known_files: &'a rustc_hash::FxHashSet<ProjectPath>) -> Self {
        ResolveCtx {
            known_files,
            declared_dependencies: None,
            workspace_members: None,
            units: None,
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

    pub fn contains(&self, path: &ProjectPath) -> bool {
        self.known_files.contains(path)
    }

    pub fn is_declared_dependency(&self, name: &SmolStr) -> bool {
        self.declared_dependencies.is_some_and(|d| d.contains(name))
    }

    pub fn workspace_member(&self, name: &str) -> Option<&'a WorkspaceMember> {
        self.workspace_members.and_then(|m| m.get(name))
    }

    /// Every known file declaring `unit` (empty when none do, or `with_units` was never
    /// called). Sorted by path — deterministic which entry a caller picking `.first()` gets.
    pub fn unit_files(&self, unit: &str) -> &'a [ProjectPath] {
        self.units
            .and_then(|u| u.get(unit))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Every known file whose *immediate* directory equals `dir` (`""` = project root) — one
    /// more path segment, not a recursive subtree. Added for languages whose import unit is a
    /// directory rather than a single file (Go's package — RFC 0002 §2, docs/adapters/go.md
    /// §3): resolving `foo.com/bar/sub` needs *some* concrete file in `sub/` to anchor an
    /// `ImportsFile` edge on, and there's no naming convention (unlike JS's `index.*`) to guess
    /// one from. Read-only, like every other `ResolveCtx` query; callers needing a deterministic
    /// pick (RFC 0008 §4) sort the result themselves — iteration order here follows the
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
    /// recursive counterpart to [`Self::files_in_dir`]. Added for Java (docs/adapters/java.md
    /// §4): a publishable module's root promotion needs every `.java` file under its source
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
    /// A bare specifier that resolved *into* a named in-repo package (RFC 0011 §4): `target`
    /// is the concrete internal file (the member's entry, or a subpath into it — deep
    /// imports included, their edges are recorded from M1 even though the `deep-import`
    /// verdict lands M3), and `name` is the member's package name. Distinct from plain
    /// `File` because assembly derives BOTH edge kinds from it: `ImportsFile` (reachability
    /// is real, cross-package) and `ImportsDependency` (the declaration contract is real
    /// too — RFC 0011 §4's table validates it both ways: declared-but-unimported workspace
    /// deps are `unused`, imported-but-undeclared siblings are phantom `undeclared`).
    WorkspaceMember {
        name: SmolStr,
        target: ProjectPath,
        confidence: Confidence,
    },
    Stdlib,
    Unresolved,
}

// ---------------------------------------------------------------- the trait

/// One implementation per language, registered at startup. See RFC 0002 for the boundary:
/// adapters own what the language *spec* defines — nothing ecosystem-shaped.
pub trait LanguageAdapter: Send + Sync {
    fn descriptor(&self) -> AdapterDescriptor;

    /// Claim & classify a path (fast; name-based, content peeking only when unavoidable).
    fn claim(&self, path: &ProjectPath) -> Option<FileClaim>;

    /// Is this path one of this adapter's manifest files (`package.json`, …)? Manifests are
    /// claimed separately from source (`claim`) — they never get a [`FileClaim`]/language of
    /// their own (docs/adapters/js-ts.md §1: "manifests are not claimed"), only manifest facts.
    fn claim_manifest(&self, path: &ProjectPath) -> bool;

    /// Parse one file and extract every language-defined fact. Must not fail on broken code:
    /// return partial facts + diagnostics.
    fn extract(&self, file: &SourceFile<'_>) -> FileFacts;

    /// Parse a manifest into declared dependencies, package identity/topology, and roots.
    /// `ctx` lets the adapter resolve entry-point specifiers (main/module/exports/bin) against
    /// the known-files index itself — the same Node-resolution knowledge `resolve()` already
    /// owns, not something the core can generically guess at.
    fn extract_manifest(&self, file: &SourceFile<'_>, ctx: &ResolveCtx<'_>) -> ManifestFacts;

    /// Resolve an import specifier to a concrete target. Called by the core's resolution
    /// driver — including for specifiers emitted by *other* adapters (RFC 0002 §4).
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
