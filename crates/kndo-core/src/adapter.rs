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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct ProjectPath(pub SmolStr);

/// 1-indexed line/column span, `start` inclusive, `end` exclusive. Serializes as the
/// `[line, col]` pair shape the output schema uses (contracts/output-schema.md §2), not an
/// object — tuples serialize as JSON arrays by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Warn,
    Info,
}

#[derive(Debug, Clone, serde::Serialize)]
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
    /// e.g. "js-ts". Participates in cache keys.
    pub id: SmolStr,
    /// Bumping invalidates only this adapter's cached facts (RFC 0004 §3).
    pub facts_schema_version: u32,
    pub file_globs: Vec<SmolStr>,
    pub manifest_globs: Vec<SmolStr>,
    pub grammar_version: SmolStr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileClaim {
    pub language: SmolStr,
    pub class: FileClass,
}

// ---------------------------------------------------------------- extracted facts

/// Ladder index on the adapter-declared visibility ladder (RFC 0005 §7): 0 = most private,
/// higher = wider. The adapter names the levels; the core only compares them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VisibilityLevel(pub u8);

#[derive(Debug, Clone)]
pub struct Declaration {
    pub name: SmolStr,
    pub kind: SymbolKind,
    pub span: Span,
    pub exported: bool,
    pub visibility: VisibilityLevel,
}

#[derive(Debug, Clone)]
pub struct RawReference {
    pub name: SmolStr,
    /// Enough context for the resolution driver (enclosing scope path, receiver hints…).
    pub scope_context: Option<SmolStr>,
    pub span: Span,
}

/// Syntactic shape only — what the specifier text looks like, not what it resolves to.
/// Builtins (`node:fs`, bare `fs`) are a *resolution*-time fact (the resolver owns the
/// builtins list, RFC 0002 §5); extraction never claims `Stdlib`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Relative,
    Package,
}

/// One local name this import introduces, and which export it names — the fact the core needs
/// to resolve a same-name-elsewhere `RawReference` to the *target file's* symbol rather than
/// (incorrectly) a same-file one. `imported: None` is a default import — binds to the target's
/// synthetic `"default"` export, the same name declarations.rs already uses for anonymous
/// default exports.
#[derive(Debug, Clone)]
pub struct ImportBinding {
    pub local: SmolStr,
    pub imported: Option<SmolStr>,
}

#[derive(Debug, Clone)]
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
}

#[derive(Debug, Clone)]
pub enum RawRootTarget {
    WholeFile,
    /// By declared name within this file.
    Declaration(SmolStr),
}

/// Language-defined roots only (`main`, `pub` API…); ecosystem roots come from plugins.
#[derive(Debug, Clone)]
pub struct RawRoot {
    pub kind: RootKind,
    pub target: RawRootTarget,
    pub confidence: Confidence,
}

#[derive(Debug, Clone)]
pub struct FunctionMetrics {
    /// Declared name (symbol path within the file).
    pub symbol: SmolStr,
    pub cyclomatic: u32,
    pub loc: u32,
    /// Winnowing fingerprints over the normalized token stream (RFC 0005 §6).
    pub fingerprints: Vec<u64>,
}

/// A construct forcing a `Wildcard` edge (RFC 0005 §1 plausible-target-set expansion).
#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressionScope {
    Declaration,
    File,
}

/// A `kndo:allow` pragma as extracted; validation, binding, counting and staleness are core
/// logic, identical across languages (contracts §2.1 — the no-flicker guarantee lives there).
#[derive(Debug, Clone)]
pub struct RawSuppression {
    pub span: Span,
    /// Verdict name — validated by the core against the registry.
    pub category: SmolStr,
    /// Optional `:subject` facet (kebab-case).
    pub subject: Option<SmolStr>,
    pub reason: Option<String>,
    pub scope: SuppressionScope,
}

/// Everything an adapter owes the core for one file. Must be deterministic for identical
/// content (conformance harness, RFC 0002 §8).
#[derive(Debug, Default)]
pub struct FileFacts {
    pub declarations: Vec<Declaration>,
    pub references: Vec<RawReference>,
    pub imports: Vec<RawImport>,
    pub roots: Vec<RawRoot>,
    pub functions: Vec<FunctionMetrics>,
    pub dynamics: Vec<DynamicUse>,
    pub suppressions: Vec<RawSuppression>,
    pub diagnostics: Vec<Diagnostic>,
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
    /// Whether an explicit surface is declared (`exports` map or equivalent) — the
    /// contract gate for `deep-import` (RFC 0011 §4).
    pub declares_surface: bool,
    pub diagnostics: Vec<Diagnostic>,
}

// ---------------------------------------------------------------- resolution

#[derive(Debug, Clone)]
pub struct ImportSpec {
    pub specifier: SmolStr,
    pub from: ProjectPath,
}

/// Index of claimable paths and manifest facts the core exposes to resolvers — populated from
/// discovery and manifest extraction. Adapters only ever *query* it; they never touch the
/// filesystem themselves (the purity rule, RFC 0002 §6). Read-only by construction.
pub struct ResolveCtx<'a> {
    known_files: &'a std::collections::HashSet<ProjectPath>,
    /// Dependency names declared in the importing file's package manifest. Feeds the
    /// declared-beats-stdlib-list shadowing rule (RFC 0002 §6); empty until the engine wires
    /// manifest facts through.
    declared_dependencies: Option<&'a std::collections::HashSet<SmolStr>>,
}

impl<'a> ResolveCtx<'a> {
    pub fn new(known_files: &'a std::collections::HashSet<ProjectPath>) -> Self {
        ResolveCtx {
            known_files,
            declared_dependencies: None,
        }
    }

    pub fn with_declared_dependencies(
        mut self,
        deps: &'a std::collections::HashSet<SmolStr>,
    ) -> Self {
        self.declared_dependencies = Some(deps);
        self
    }

    pub fn contains(&self, path: &ProjectPath) -> bool {
        self.known_files.contains(path)
    }

    pub fn is_declared_dependency(&self, name: &SmolStr) -> bool {
        self.declared_dependencies.is_some_and(|d| d.contains(name))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    File(ProjectPath, Confidence),
    Dependency(SmolStr, Confidence),
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
