//! Project Graph assembly — turns discovered files into the language-neutral
//! graph via registered adapters. This module is **language-blind** (the
//! ignorance rule): it references only the `LanguageAdapter` trait, never a concrete
//! language. Adapter *registration* happens at the binary level (`kndo-cli` composes core +
//! first-party adapters) — the core must never know which languages exist.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::adapter::{ProjectPath, Span, VisibilityLevel};
use crate::vocab::{
    Confidence, DependencyScope, Edge, FileClass, FileId, PackageId, SymbolId, SymbolKind,
};
use smol_str::SmolStr;

#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct FileNode {
    pub path: ProjectPath,
    pub content_hash: [u8; 32],
    /// `None` when no registered adapter claims this file — it still exists as a File node
    /// (e.g. a README, or a CSS file before a CSS adapter exists) so import edges *to* it
    /// still resolve, per the cross-language model.
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub language: Option<SmolStr>,
    pub class: Option<FileClass>,
    /// Every file belongs to exactly one Package (nearest-manifest-ancestor).
    /// `PackageId(0)` is always the implicit package (see [`ProjectGraph::packages`]) — never
    /// `None`, since ownership is total even when nothing real claims a file.
    pub package: PackageId,
    /// The file's `FileFacts::unit` key, persisted onto the graph: visibility-
    /// scope containment checks (`internal-only`'s tightest-sufficient computation, the
    /// member fallback's candidate scoping) need "same unit?" answerable from the graph
    /// alone, warm path included. `None` for file-scoped languages, exactly as in the facts.
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub unit: Option<SmolStr>,
    /// The unit containing this file's unit — the link that makes unit keys a TREE
    /// ([`crate::adapter::FileFacts::unit_parent`]). Carried onto the graph so
    /// `VisibilityScope::Module` containment stays a pure graph question.
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub unit_parent: Option<SmolStr>,
    /// Sub-file test regions ([`crate::adapter::FileFacts::test_spans`]), sorted by span.
    /// Role-sensitive consumers check containment via [`span_in_test_region`]: `crap` and
    /// health's symbol tallies skip contained symbols, `dependency_hygiene` treats a
    /// contained import site as test-role usage. Empty for languages whose test detection
    /// is per-file.
    pub test_spans: Vec<Span>,
    /// String-literal call sites ([`crate::adapter::FileFacts::string_call_args`]),
    /// canonically sorted. Persisted onto the graph so plugins query them through
    /// [`crate::plugin::GraphView::string_call_sites_in`] (natively) or `call-sites-in`
    /// (WASM) on warm paths too — no analysis consumes them directly; they are plugin fuel.
    pub string_call_sites: Vec<crate::adapter::StringCallArg>,
    /// Attribute string literals ([`crate::adapter::FileFacts::string_attr_args`]),
    /// canonically sorted. Same posture as [`Self::string_call_sites`] and here for the same
    /// reason: no analysis consumes them, and a plugin must be able to read them on a warm
    /// run too — through [`crate::plugin::GraphView::attr_strings_in`] (natively) or
    /// `attr-strings-in` (WASM).
    pub string_attr_args: Vec<crate::adapter::StringAttrArg>,
}

/// Whether `span` lies inside any of `regions` — inclusive containment on the `(line,
/// column)` order [`Span`] already carries. Regions are disjoint by construction
/// (extraction records outermost extents) and per-file counts are small; linear scan.
pub fn span_in_test_region(regions: &[Span], span: Span) -> bool {
    regions
        .iter()
        .any(|r| r.start <= span.start && span.end <= r.end)
}

#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct SymbolNode {
    pub file: FileId,
    /// Bare name — for members, ownership lives in `member_of`, never in the name string.
    /// Renderers and selectors use [`SymbolNode::qualified_name`].
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub name: SmolStr,
    pub kind: SymbolKind,
    pub span: Span,
    pub exported: bool,
    pub visibility: VisibilityLevel,
    /// Mirrors [`crate::adapter::Declaration::member_of`].
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub member_of: Option<SmolStr>,
    /// Mirrors [`crate::adapter::Declaration::signature_span`].
    pub signature_span: Option<Span>,
    /// Mirrors [`crate::adapter::Declaration::implicitly_invoked`] (the
    /// machinery-dispatch rule) — reachability derives the implicit owner → member edge.
    pub implicitly_invoked: bool,
    /// Mirrors [`crate::adapter::Declaration::nested_scope`] — `internal-only` never
    /// recommends the file-scope rung for a declaration nested inside its file.
    pub nested_scope: bool,
    /// Mirrors [`crate::adapter::Declaration::visibility_inherited`] — visibility analyses
    /// skip a symbol whose level belongs to its container.
    pub visibility_inherited: bool,
    /// The unit anchoring this declaration's [`crate::adapter::VisibilityScope::Module`]
    /// region, when it declares one — carried onto the graph so visibility analyses stay
    /// pure graph functions (`crate::adapter::Declaration::visible_in_unit`).
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub visible_in_unit: Option<SmolStr>,
    /// Mirrors [`crate::adapter::Declaration::implements`] — the trait/protocol whose
    /// implementation declares this member. Carried onto the graph (and through the snapshot)
    /// because the consumer is a PLUGIN, which runs after assembly: an ecosystem plugin
    /// matches its curated trait table against this instead of re-reading source the adapter
    /// already parsed. Facts here, interpretation at plugin/analysis time.
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub implements: Option<SmolStr>,
    /// Mirrors [`crate::adapter::Declaration::markers`] — the language-visible annotations,
    /// attributes or decorators written on this declaration, verbatim. Carried into the graph
    /// (and through the snapshot) because the consumer is CONFIG, which arrives after
    /// assembly: `kndo.toml`'s `[[externally-invoked]]` matches its own marker list against
    /// these to seed reachability roots. The graph itself stays configuration-independent —
    /// facts here, interpretation at analysis time.
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub markers: Vec<SmolStr>,
}

impl SymbolNode {
    /// The display/selector form: `Owner.name` for members, the bare name otherwise. This is
    /// what finding messages, `location.symbol`, finding ids, and selector round-trips use —
    /// so ids stay distinct for same-named members of different owners, and stay *stable*
    /// even for adapters that encode the owner into the name itself.
    pub fn qualified_name(&self) -> String {
        match &self.member_of {
            Some(owner) => format!("{owner}.{}", self.name),
            None => self.name.to_string(),
        }
    }
}

/// One callable **shape**: cyclomatic + LOC feed `crap`, the winnowing fingerprints feed
/// structural `duplicate`. Keyed by `SymbolId` in [`ProjectGraph::function_metrics`] —
/// assembly resolves the adapter-side `FunctionMetrics::span` to that id (an exact match
/// against the paired declaration's own span) and drops both it and the entry's name.
/// Resolution is deliberately NOT by name: a file may declare one name twice (cfg-alternated
/// impls, platform-gated overloads), and the per-file name tables are single-slot, so name
/// lookup collapsed both entries onto one symbol — which `duplicate` then reported as a clone
/// of itself.
///
/// **One symbol may own several shapes.** A declaration contributes its own shape plus one per
/// callable nested inside it, so `function_metrics` is a vec of pairs and not a map: nothing
/// may index it expecting a single entry per `SymbolId`. `shape_ordinal` tells them apart and
/// `shape_span` says where each one is.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct SymbolMetrics {
    /// This shape's own extent — equal to the symbol's span for the declaration's own shape,
    /// the nested callable's own span otherwise. Every location a consumer reports comes from
    /// here; `SymbolNode::span` cannot separate two closures in one function.
    pub shape_span: crate::vocab::Span,
    /// 0 for the declaration's own shape, 1..N for nested callables in pre-order — the stable
    /// half of a nested shape's finding id (a line number would churn every baseline above it).
    pub shape_ordinal: u16,
    pub cyclomatic: u32,
    pub loc: u32,
    /// Normalized-stream token count (the duplication ratio basis). A nested callable's tokens
    /// belong to ITS shape, not the enclosing one, which keeps a single `FN` placeholder in
    /// their place — so summing over shapes still counts every token exactly once.
    pub token_count: u32,
    pub fingerprints: Vec<u64>,
    /// This shape's body is a single value-construction expression — a struct/object literal
    /// or a constructor call — and nothing else.
    ///
    /// A FACT about the body's shape, never a verdict. What consumes it is `duplicate`, and
    /// the reason is that the fingerprint's own normalization inverts on such a body:
    /// identifiers and literals canonicalize away, and for a construction that is the entire
    /// authored content, leaving only the field list the TYPE dictates. Two constructions of
    /// one type therefore fingerprint alike by definition of the type, not by evidence of
    /// copying.
    pub body_is_construction: bool,
}

/// Per-file state the incremental patch needs beyond the graph proper —
/// indexed by FileId, parallel to [`ProjectGraph::files`]. Grouped here rather than
/// scattered onto [`FileNode`]: these fields serve the patch layer, not graph consumers.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct FilePatchMeta {
    /// The span-normalized surface signature; `None` for unclaimed files (nothing
    /// derived to guard).
    pub surface_sig: Option<[u8; 32]>,
    /// The file's declared unit name (Go `package` clause — the qualifier default),
    /// persisted so the patch never re-fetches an unchanged target's facts.
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub unit_name: Option<SmolStr>,
    /// The re-export aliases phase 3a-bis resolved *into this file's own table* — the one
    /// resolution table not derivable from `symbols`. Order-independent state after the
    /// re-export fixpoint, hence safe to persist and reuse.
    pub reexport_aliases: Vec<AliasEntry>,
    /// The file's declared member-type facts (`FileFacts::member_types`),
    /// persisted verbatim: the patch resolves CHANGED files' chained qualifiers against
    /// UNCHANGED files' member types without re-fetching their facts.
    pub member_types: Vec<crate::adapter::RawMemberType>,
    /// Every name this file's imports bind to another FILE, as resolved
    /// (`ImportResolution::module_bindings`). Persisted for exactly the reason `member_types`
    /// is: the qualifier hop (`graph::assemble::link_module_bindings`) lets a CHANGED file's
    /// qualifier follow one binding through an UNCHANGED file's table, and re-resolving every
    /// unchanged file's imports to learn it would defeat the patch.
    pub module_bindings: Vec<ModuleBinding>,
}

/// One resolved import binding: the name `name` in the owning file's scope binds to `target`.
/// Named rather than a tuple for the same reason [`AliasEntry`] is — rkyv's `with` attribute
/// applies to fields, not to tuple elements.
#[derive(Debug, Clone, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ModuleBinding {
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub name: SmolStr,
    pub target: FileId,
}

/// One resolved re-export alias: importing `name` from the owning file resolves to `symbol`.
#[derive(Debug, Clone, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AliasEntry {
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub name: SmolStr,
    pub symbol: SymbolId,
}

/// A package consumed *as a dependency* — external (npm/crates.io/…) or an in-repo workspace
/// member imported by name (the workspace case carries the same
/// declaration-contract obligations, so it lives in the same node kind; its file-level
/// reachability is carried separately by the `ImportsFile` edge the same resolution emits).
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct DependencyNode {
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub name: SmolStr,
}

/// A workspace unit: one manifest + the file tree it governs. `PackageId(0)` is
/// always the implicit package with `manifest: None` — "a repo with no manifest at all is one
/// implicit Package" generalizes to "whatever no real manifest's subtree claims," so ownership
/// is total (every file has a package) even in a repo with zero manifests, or with manifests
/// that don't cover every directory.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PackageNode {
    pub manifest: Option<ProjectPath>,
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub name: Option<SmolStr>,
    /// Publish signal from the manifest — mirrors `ManifestFacts::private`.
    pub private: bool,
    /// Whether the manifest declares an explicit entry-point surface (an `exports` map or the
    /// language's equivalent) — mirrors `ManifestFacts::declares_surface`, and is the
    /// **contract gate** for `deep-import`: no declared surface = no declared
    /// boundary = never a finding, so monorepos where sibling deep imports are accepted
    /// practice see zero noise.
    pub declares_surface: bool,
    /// The declared surface as concrete files: `ManifestFacts::resolved_entries` mapped to
    /// `FileId`s (entries naming files outside the discovered tree — build artifacts — simply
    /// don't appear). An `ImportsFile` edge from another package landing on a file *not* in
    /// this set, while `declares_surface` holds, is a deep import.
    pub surface: Vec<FileId>,
    /// The member's primary entry as the manifest's adapter resolved it — the
    /// `WorkspaceMember::entry` input for bare-specifier resolution, persisted verbatim
    /// (the patch rebuilds the workspace index from the snapshot; `surface`
    /// can't stand in — it drops out-of-tree entries and confidences).
    pub workspace_entry: Option<(ProjectPath, Confidence)>,
    /// Every resolved target file this manifest declares — the union of
    /// `ManifestFacts::roots` and `ManifestFacts::resolved_entries`, deduped in declaration
    /// order. These are the package's module-tree anchors: what a resolver anchors
    /// intra-package paths on when the language's directory convention doesn't hold (a Rust
    /// `[[bin]] path = "crates/core/main.rs"` places a whole module tree outside `src/`).
    /// Persisted for the same reason as `workspace_entry`: the patch rebuilds
    /// the workspace index from the snapshot.
    pub targets: Vec<ProjectPath>,
    /// The manifest's named executable targets, verbatim (`ManifestFacts::executables`) —
    /// what a file's `invoked_executables` names resolve against (the
    /// invoked-program rule). Persisted for the same reason as `targets`: the
    /// patch rebuilds the name index from the snapshot.
    pub executables: Vec<crate::adapter::ExecutableTarget>,
    /// Mirrors the claiming adapter's [`crate::adapter::AdapterDescriptor::resolves_dependency_usage`]
    /// (`true` for the implicit no-manifest package, which declares nothing). `dependency_hygiene`
    /// reads this per `DeclaredDependency::package` to decide whether "zero usage edges" means
    /// "genuinely unused" or "this language can't produce usage edges at all."
    pub resolves_dependency_usage: bool,
    /// Every registered adapter's language whose `claim_manifest` accepts this manifest — not
    /// just the one that won the claim. Empty for the implicit no-manifest package.
    ///
    /// File→package ownership is nearest-ancestor by DIRECTORY, which is right for everything
    /// it feeds except one question: whose dependency declarations does this file answer to?
    /// A Jazzy-generated `.js` under `docs/` in a Swift repo, or a `web/app.js` beside a
    /// `go.mod`, owes nothing to `Package.swift` or `go.mod` — and charged its bare imports
    /// against them anyway, which is where every Swift repo's phantom `jquery` and hugo's
    /// whole `undeclared` column came from. The test is not "same language": Java and Kotlin
    /// both claim `pom.xml`, so a `.kt` file's Maven declarations must keep counting. It is
    /// "could this file's own adapter have claimed this manifest?", asked of the adapters
    /// themselves — the core never names a language to answer it.
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub manifest_claim_languages: Vec<SmolStr>,
}

impl PackageNode {
    /// Whether a file written in `language` answers to this package's dependency declarations
    /// — see [`PackageNode::manifest_claim_languages`]. The implicit no-manifest package
    /// declares nothing, so nothing can contradict it and every file "answers" to it.
    pub fn governs_dependencies_of(&self, language: Option<&str>) -> bool {
        if self.manifest.is_none() {
            return true;
        }
        language.is_some_and(|lang| self.manifest_claim_languages.iter().any(|l| l == lang))
    }
}

/// One manifest's declaration of an external dependency — the raw fact `undeclared` and
/// `version-skew` compare against, kept separate from [`DependencyNode`] because a declaration
/// can exist with zero importers (nothing wrong with that on its own — that's `unused`'s
/// concern) and a project can have many manifests declaring the same name differently (that's
/// `version-skew`'s).
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct DeclaredDependency {
    pub package: PackageId,
    pub manifest: ProjectPath,
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub name: SmolStr,
    /// Mirrors [`crate::adapter::ManifestDependency::version_req`], with `inherited`
    /// already resolved against the shared pool: `None` means this manifest states no
    /// comparable requirement, never "any version".
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub version_req: Option<SmolStr>,
    pub scope: DependencyScope,
}

/// [`ProjectGraph::from_snapshot_parts`]'s input, bundled into one struct purely to stay under
/// clippy's argument-count lint — every field here is one `ProjectGraph` field, verbatim.
/// An import whose specifier the claiming adapter understood as a path and could not resolve to
/// any file — `Resolution::Unresolved` after every adapter declined. Recorded by assembly so
/// the `unresolved` analysis can report it; assembly itself never turns facts into findings.
///
/// Only *relative* imports land here. A `Package` specifier that resolves to nothing is the
/// declaration contract's business (`undeclared`), never reported twice.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct UnresolvedImport {
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub specifier: SmolStr,
    pub span: crate::vocab::Span,
    /// The adapter's own confidence in the import. A dynamic specifier the adapter could only
    /// partly read arrives below `Certain` and the finding inherits that — an
    /// `import(templated)` is not evidence of a broken path.
    pub confidence: crate::vocab::Confidence,
}

pub(crate) struct GraphSnapshotParts {
    pub files: Vec<FileNode>,
    pub symbols: Vec<SymbolNode>,
    pub dependencies: Vec<DependencyNode>,
    pub declared_dependencies: Vec<DeclaredDependency>,
    pub script_invoked_dependencies: HashSet<(PackageId, SmolStr)>,
    pub packages: Vec<PackageNode>,
    pub edges: Vec<Edge>,
    pub suppressions: Vec<(FileId, crate::adapter::RawSuppression)>,
    pub visibility_ladders: Vec<(SmolStr, Vec<crate::adapter::VisibilityRung>)>,
    pub cycle_policies: Vec<(SmolStr, crate::adapter::CyclePolicy)>,
    pub testable_languages: Vec<(SmolStr, bool)>,
    pub function_metrics: Vec<(SymbolId, SymbolMetrics)>,
    pub patch_meta: Vec<FilePatchMeta>,
    pub externally_consumed: Vec<SymbolId>,
    pub plugin_implicitly_invoked: Vec<SymbolId>,
    pub unresolved_imports: Vec<(FileId, UnresolvedImport)>,
}

/// The assembled language-neutral graph. Read-only once built; the incremental
/// patch constructs a fresh graph off the cached snapshot rather than mutating in place.
#[derive(Debug, Default, PartialEq)]
pub struct ProjectGraph {
    pub files: Vec<FileNode>,
    pub symbols: Vec<SymbolNode>,
    pub dependencies: Vec<DependencyNode>,
    pub declared_dependencies: Vec<DeclaredDependency>,
    /// `(package, name)` pairs a manifest's `scripts` invoke as a leading command — dependency
    /// hygiene's only usage evidence for CLI-only tools, which never produce an
    /// `ImportsDependency` edge (nothing `import`s a binary). Names here aren't necessarily
    /// declared dependencies — cross-referencing is `dependency_hygiene`'s job, not assembly's.
    pub script_invoked_dependencies: HashSet<(PackageId, SmolStr)>,
    pub packages: Vec<PackageNode>,
    pub edges: Vec<Edge>,
    /// `kndo:allow`/`kndo:allow-file` pragmas as extracted, one per file — binding (line
    /// adjacency to a declaration), validation and marking of matched findings is core logic
    /// downstream of assembly, not here. Persisted through the graph-snapshot
    /// cache like everything else in this struct, so a warm run never silently drops them.
    pub suppressions: Vec<(FileId, crate::adapter::RawSuppression)>,
    /// Each claimed language's visibility ladder, copied off the claiming
    /// adapter's descriptor at assembly time and keyed by the claim language `FileNode::
    /// language` stores — so analyses (pure graph functions, no adapter access) can turn a
    /// symbol's `VisibilityLevel` index into a checkable [`crate::adapter::VisibilityScope`]
    /// plus the language's own remediation label. Sorted by language for determinism.
    pub visibility_ladders: Vec<(SmolStr, Vec<crate::adapter::VisibilityRung>)>,
    /// Each claimed language's answer to "can a file of mine hold a unit of testing", collected
    /// exactly like the ladders — adapter-declared data, carried here so `untested` stays a pure
    /// graph function. Absent language ⇒ `true`: a language nothing recorded is not one to
    /// silence. Sorted by language for determinism.
    pub testable_languages: Vec<(SmolStr, bool)>,
    /// Each claimed language's cycle tolerance, collected exactly like the
    /// ladders — adapter-declared data, carried here so `cyclic` stays a pure graph function.
    pub cycle_policies: Vec<(SmolStr, crate::adapter::CyclePolicy)>,
    /// Callable shapes, sparse — only symbols whose adapter emitted
    /// `FileFacts::functions` for them (callables), resolved to ids at assembly.
    pub function_metrics: Vec<(SymbolId, SymbolMetrics)>,
    /// Per-file patch metadata — indexed by FileId, always `files.len()` entries;
    /// empty-defaulted for test-built graphs (the patch layer never runs there).
    pub patch_meta: Vec<FilePatchMeta>,
    /// Symbols a plugin's `annotate_symbols` marked externally consumed this run
    /// (consumed by `internal_only`/`private_type_leak` as their documented exemption).
    /// Sorted, deduplicated. Always empty for a cache-hit or patched graph — plugins with
    /// graph-mutation hooks force a full rebuild every run (see `assemble_from_source`), so
    /// there is no cached-graph case where this could go stale.
    pub externally_consumed: Vec<SymbolId>,
    /// Members a plugin's `annotate_symbols` marked machinery-invoked this run (the
    /// framework counterpart of `SymbolNode::implicitly_invoked` — the
    /// machinery-dispatch rule reads both). Sorted, deduplicated; same snapshot round-trip
    /// rationale as `externally_consumed`.
    pub plugin_implicitly_invoked: Vec<SymbolId>,
    /// Relative imports that resolved to no file — see [`UnresolvedImport`]. Persisted through
    /// the snapshot like every other fact here, so a warm run reports them too.
    pub unresolved_imports: Vec<(FileId, UnresolvedImport)>,
    file_index: HashMap<ProjectPath, FileId>,
}

impl ProjectGraph {
    /// Whether a file of `language` can contain a unit of testing at all — the adapter's own
    /// answer (see `AdapterDescriptor::declares_units_of_testing`).
    ///
    /// A language this graph never recorded answers `true`. That direction is deliberate: an
    /// absent entry means "nothing was declared", and treating silence as an exemption is how a
    /// whole language's blind spots would disappear from the report without anyone choosing it.
    pub fn language_declares_units_of_testing(&self, language: Option<&SmolStr>) -> bool {
        let Some(language) = language else {
            return true;
        };
        self.testable_languages
            .iter()
            .find(|(l, _)| l == language)
            .map(|(_, testable)| *testable)
            .unwrap_or(true)
    }

    pub fn file_id(&self, path: &ProjectPath) -> Option<FileId> {
        self.file_index.get(path).copied()
    }

    /// Whether a plugin marked this symbol externally consumed (via `annotate_symbols`).
    /// `externally_consumed` is sorted — a binary search, not a scan,
    /// since `internal_only`/`private_type_leak` call this once per symbol they'd otherwise
    /// flag.
    pub fn is_externally_consumed(&self, id: SymbolId) -> bool {
        self.externally_consumed.binary_search(&id).is_ok()
    }

    /// Whether a plugin marked this member machinery-invoked (`mark_implicitly_invoked` —
    /// the machinery-dispatch rule). Sorted input, binary search.
    pub fn is_plugin_implicitly_invoked(&self, id: SymbolId) -> bool {
        self.plugin_implicitly_invoked.binary_search(&id).is_ok()
    }

    /// The visibility ladder for a claim language — `None` when the language
    /// never declared one (unclaimed files, pre-ladder snapshots). An empty ladder is a
    /// deliberate declaration ("no visibility semantics") and returns `Some(&[])`.
    pub fn ladder_for(&self, language: &str) -> Option<&[crate::adapter::VisibilityRung]> {
        self.visibility_ladders
            .iter()
            .find(|(l, _)| l == language)
            .map(|(_, rungs)| rungs.as_slice())
    }

    /// The cycle policy for a claim language — `None` for unclaimed files and
    /// pre-policy snapshots (the analysis then stays silent for that participant).
    pub fn cycle_policy_for(&self, language: &str) -> Option<crate::adapter::CyclePolicy> {
        self.cycle_policies
            .iter()
            .find(|(l, _)| l == language)
            .map(|(_, p)| *p)
    }

    /// The declared package name for a `PackageId`, when the owning manifest declared one
    /// (`package.json` `name`, …) — `None` for the implicit package and for manifests that
    /// never named themselves.
    pub fn package_name(&self, package: PackageId) -> Option<&str> {
        self.packages[package.0 as usize].name.as_deref()
    }

    /// Rebuilds a full graph from its persisted parts (`cache.rs`'s `graph.bin`) —
    /// the warm-path counterpart to [`assemble`]: same shape, but skipping claim/extract/
    /// resolve/link entirely when nothing changed. `file_index` isn't itself persisted (cheap
    /// to rebuild, and doing so means the cache format never has to carry a second, derived
    /// copy of `files` in lockstep). Bundled into [`GraphSnapshotParts`] rather than taken as
    /// separate arguments — one more than clippy's default argument-count lint allows.
    pub(crate) fn from_snapshot_parts(parts: GraphSnapshotParts) -> Self {
        let file_index = parts
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.path.clone(), FileId(i as u32)))
            .collect();
        ProjectGraph {
            files: parts.files,
            symbols: parts.symbols,
            dependencies: parts.dependencies,
            declared_dependencies: parts.declared_dependencies,
            script_invoked_dependencies: parts.script_invoked_dependencies,
            packages: parts.packages,
            edges: parts.edges,
            suppressions: parts.suppressions,
            visibility_ladders: parts.visibility_ladders,
            cycle_policies: parts.cycle_policies,
            testable_languages: parts.testable_languages,
            function_metrics: parts.function_metrics,
            patch_meta: parts.patch_meta,
            // Snapshots are written even with graph-mutating plugins registered, so this
            // must round-trip through the snapshot — dropping it here would silently lose
            // `annotate_symbols` exemptions on every warm hit.
            externally_consumed: parts.externally_consumed,
            plugin_implicitly_invoked: parts.plugin_implicitly_invoked,
            unresolved_imports: parts.unresolved_imports,
            file_index,
        }
    }

    /// Crate-internal only: lets sibling modules (analyses) build exact graphs in tests —
    /// including edges (Root, References, Wildcard) real extraction doesn't produce yet.
    /// External code can never fabricate a graph; only [`assemble`] does, for real.
    #[cfg(test)]
    pub(crate) fn for_test(
        files: Vec<FileNode>,
        symbols: Vec<SymbolNode>,
        dependencies: Vec<DependencyNode>,
        edges: Vec<Edge>,
    ) -> Self {
        let files_len = files.len();
        let file_index = files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.path.clone(), FileId(i as u32)))
            .collect();
        ProjectGraph {
            files,
            symbols,
            dependencies,
            declared_dependencies: Vec::new(),
            script_invoked_dependencies: HashSet::default(),
            packages: vec![PackageNode {
                manifest: None,
                name: None,
                private: false,
                declares_surface: false,
                surface: Vec::new(),
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                resolves_dependency_usage: true,
                manifest_claim_languages: Vec::new(),
            }],
            edges,
            suppressions: Vec::new(),
            unresolved_imports: Vec::new(),
            // The "mock" test language's ladder, mirroring Go's shape (the language whose
            // rules the member-fallback and internal-only tests exercise): 0 = unit-private,
            // 1 = public. Tests needing a different shape override via
            // `with_visibility_ladders`.
            visibility_ladders: vec![(
                SmolStr::new("mock"),
                vec![
                    crate::adapter::VisibilityRung {
                        scope: crate::adapter::VisibilityScope::Unit,
                        label: SmolStr::new("private"),
                        surface_transitive: false,
                    },
                    crate::adapter::VisibilityRung {
                        scope: crate::adapter::VisibilityScope::Public,
                        label: SmolStr::new("exported"),
                        surface_transitive: true,
                    },
                ],
            )],
            // Hazard at both levels: the shape most core tests want to exercise; override
            // with `with_cycle_policies` where a different tolerance is the point.
            cycle_policies: vec![(
                SmolStr::new("mock"),
                crate::adapter::CyclePolicy {
                    file_cycles: crate::adapter::CycleTolerance::Hazard,
                    package_cycles: crate::adapter::CycleTolerance::Hazard,
                },
            )],
            // The mock language is code: `untested` judges its files like any other's, which is
            // what every core test that builds a graph here expects.
            testable_languages: vec![(SmolStr::new("mock"), true)],
            function_metrics: Vec::new(),
            patch_meta: vec![FilePatchMeta::default(); files_len],
            externally_consumed: Vec::new(),
            plugin_implicitly_invoked: Vec::new(),
            file_index,
        }
    }

    /// Test-only: set which symbols a plugin would have marked externally consumed, without
    /// going through a real `Plugin`/assembly round trip.
    #[cfg(test)]
    pub(crate) fn with_externally_consumed(mut self, mut ids: Vec<SymbolId>) -> Self {
        ids.sort_unstable();
        ids.dedup();
        self.externally_consumed = ids;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_function_metrics(mut self, metrics: Vec<(SymbolId, SymbolMetrics)>) -> Self {
        self.function_metrics = metrics;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_cycle_policies(
        mut self,
        policies: Vec<(SmolStr, crate::adapter::CyclePolicy)>,
    ) -> Self {
        self.cycle_policies = policies;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_visibility_ladders(
        mut self,
        ladders: Vec<(SmolStr, Vec<crate::adapter::VisibilityRung>)>,
    ) -> Self {
        self.visibility_ladders = ladders;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_declared_dependencies(mut self, deps: Vec<DeclaredDependency>) -> Self {
        self.declared_dependencies = deps;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_script_invoked_dependencies(
        mut self,
        deps: Vec<(PackageId, SmolStr)>,
    ) -> Self {
        self.script_invoked_dependencies = deps.into_iter().collect();
        self
    }

    #[cfg(test)]
    pub(crate) fn with_packages(mut self, packages: Vec<PackageNode>) -> Self {
        self.packages = packages;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_suppressions(
        mut self,
        suppressions: Vec<(FileId, crate::adapter::RawSuppression)>,
    ) -> Self {
        self.suppressions = suppressions;
        self
    }
}

mod assemble;
mod patch;
mod plugin_round;
pub(crate) mod provenance;
mod surface;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub(crate) use assemble::*;
pub use assemble::{
    assemble, assemble_from_source, assemble_with_cache, AssembledGraph, GRAPH_SCHEMA_VERSION,
};
#[allow(unused_imports)]
pub(crate) use patch::*;
#[allow(unused_imports)]
pub(crate) use plugin_round::*;
#[allow(unused_imports)]
pub(crate) use surface::*;
