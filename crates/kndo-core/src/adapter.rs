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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectPath(pub SmolStr);

/// 1-indexed line/column span, `start` inclusive, `end` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Warn,
    Info,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Relative,
    Package,
    Stdlib,
}

#[derive(Debug, Clone)]
pub struct RawImport {
    pub specifier: SmolStr,
    pub kind: ImportKind,
    pub span: Span,
    /// `import "./polyfill"` — counts as usage without binding names (RFC 0005 §5).
    pub side_effect_only: bool,
    pub confidence: Confidence,
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
    /// Static narrowing when available (`./locales/${x}` → that directory).
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

/// Declared dependencies AND package identity/topology (RFC 0011 §3).
#[derive(Debug, Default)]
pub struct ManifestFacts {
    pub package_name: Option<SmolStr>,
    /// Publish signal: `private: true` and friends → app mode; absent → library mode.
    pub private: bool,
    /// Workspace membership declarations (globs).
    pub workspace_members: Vec<SmolStr>,
    pub dependencies: Vec<ManifestDependency>,
    /// Entry-point specifiers (bin/main/exports targets) — resolution inputs and roots.
    pub entry_points: Vec<SmolStr>,
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

/// Index of claimable paths and manifests the driver exposes to resolvers.
/// (Grows with the resolution driver in M1; opaque to adapters by design.)
#[derive(Debug, Default)]
pub struct ResolveCtx {}

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

    /// Parse one file and extract every language-defined fact. Must not fail on broken code:
    /// return partial facts + diagnostics.
    fn extract(&self, file: &SourceFile<'_>) -> FileFacts;

    /// Parse a manifest into declared dependencies and package identity/topology.
    fn extract_manifest(&self, file: &SourceFile<'_>) -> ManifestFacts;

    /// Resolve an import specifier to a concrete target. Called by the core's resolution
    /// driver — including for specifiers emitted by *other* adapters (RFC 0002 §4).
    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx) -> Resolution;
}
