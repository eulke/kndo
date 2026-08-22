//! Project Graph assembly (RFC 0001 §3–4) — turns discovered files into the language-neutral
//! graph via registered adapters. This module is **language-blind** (RFC 0001 §2, the
//! ignorance rule): it references only the `LanguageAdapter` trait, never a concrete
//! language. Adapter *registration* happens at the binary level (`kndo-cli` composes core +
//! first-party adapters) — the core must never know which languages exist.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::path::Path;

use rayon::prelude::*;

use crate::adapter::{
    Diagnostic, DiagnosticLevel, FileClaim, ImportSpec, LanguageAdapter, ManifestFacts,
    ProjectPath, RawRootTarget, Resolution, ResolveCtx, SourceFile, Span, VisibilityLevel,
};
use crate::discovery::{self, DiscoveryError};
use crate::vocab::{
    Confidence, DependencyId, DependencyScope, Edge, EdgeKind, FileClass, FileId, NodeRef,
    PackageId, Provenance, SymbolId, SymbolKind,
};
use smol_str::SmolStr;

#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct FileNode {
    pub path: ProjectPath,
    pub content_hash: [u8; 32],
    /// `None` when no registered adapter claims this file — it still exists as a File node
    /// (e.g. a README, or a CSS file before a CSS adapter exists) so import edges *to* it
    /// still resolve, per RFC 0002 §4's cross-language model.
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub language: Option<SmolStr>,
    pub class: Option<FileClass>,
    /// Every file belongs to exactly one Package (RFC 0011 §3, nearest-manifest-ancestor).
    /// `PackageId(0)` is always the implicit package (see [`ProjectGraph::packages`]) — never
    /// `None`, since ownership is total even when nothing real claims a file.
    pub package: PackageId,
    /// The file's `FileFacts::unit` key, persisted onto the graph (RFC 0012 §6): visibility-
    /// scope containment checks (`internal-only`'s tightest-sufficient computation, the
    /// member fallback's candidate scoping) need "same unit?" answerable from the graph
    /// alone, warm path included. `None` for file-scoped languages, exactly as in the facts.
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub unit: Option<SmolStr>,
    /// Sub-file test regions ([`crate::adapter::FileFacts::test_spans`]), sorted by span.
    /// Role-sensitive consumers check containment via [`span_in_test_region`]: `crap` and
    /// health's symbol tallies skip contained symbols, `dependency_hygiene` treats a
    /// contained import site as test-role usage. Empty for languages whose test detection
    /// is per-file.
    pub test_spans: Vec<Span>,
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
    /// Bare name — for members, ownership lives in `member_of`, never in the name string
    /// (RFC 0012 §3). Renderers and selectors use [`SymbolNode::qualified_name`].
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub name: SmolStr,
    pub kind: SymbolKind,
    pub span: Span,
    pub exported: bool,
    pub visibility: VisibilityLevel,
    /// Mirrors [`crate::adapter::Declaration::member_of`].
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub member_of: Option<SmolStr>,
    /// Mirrors [`crate::adapter::Declaration::signature_span`] (RFC 0012 §5).
    pub signature_span: Option<Span>,
}

impl SymbolNode {
    /// The display/selector form: `Owner.name` for members, the bare name otherwise. This is
    /// what finding messages, `location.symbol`, finding ids, and selector round-trips use —
    /// so ids stay distinct for same-named members of different owners, and stay *stable* for
    /// adapters that previously encoded the owner into the name itself.
    pub fn qualified_name(&self) -> String {
        match &self.member_of {
            Some(owner) => format!("{owner}.{}", self.name),
            None => self.name.to_string(),
        }
    }
}

/// One callable's computed shape (RFC 0005 §6): cyclomatic + LOC feed `crap` (M4), the
/// winnowing fingerprints feed structural `duplicate`. Keyed by `SymbolId` in
/// [`ProjectGraph::function_metrics`] — the adapter-side `FunctionMetrics::symbol` name is
/// resolved to the id at assembly and dropped.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct SymbolMetrics {
    pub cyclomatic: u32,
    pub loc: u32,
    /// Normalized-stream token count (RFC 0005 §11's duplication ratio basis).
    pub token_count: u32,
    pub fingerprints: Vec<u64>,
}

/// Per-file state the incremental patch (RFC 0013 §4) needs beyond the graph proper —
/// indexed by FileId, parallel to [`ProjectGraph::files`]. Grouped here rather than
/// scattered onto [`FileNode`]: these fields serve the patch layer, not graph consumers.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct FilePatchMeta {
    /// RFC 0013 §4's span-normalized surface signature; `None` for unclaimed files (nothing
    /// derived to guard).
    pub surface_sig: Option<[u8; 32]>,
    /// The file's declared unit name (Go `package` clause — RFC 0012 §9's qualifier default),
    /// persisted so the patch never re-fetches an unchanged target's facts.
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub unit_name: Option<SmolStr>,
    /// The re-export aliases phase 3a-bis resolved *into this file's own table* — the one
    /// resolution table not derivable from `symbols`. Order-independent state after RFC 0013
    /// §3b's fixpoint, hence safe to persist and reuse.
    pub reexport_aliases: Vec<AliasEntry>,
}

/// One resolved re-export alias: importing `name` from the owning file resolves to `symbol`.
#[derive(Debug, Clone, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AliasEntry {
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub name: SmolStr,
    pub symbol: SymbolId,
}

/// A package consumed *as a dependency* — external (npm/crates.io/…) or an in-repo workspace
/// member imported by name (RFC 0011 §4: the workspace case carries the same
/// declaration-contract obligations, so it lives in the same node kind; its file-level
/// reachability is carried separately by the `ImportsFile` edge the same resolution emits).
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct DependencyNode {
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub name: SmolStr,
}

/// A workspace unit: one manifest + the file tree it governs (RFC 0011 §3). `PackageId(0)` is
/// always the implicit package with `manifest: None` — "a repo with no manifest at all is one
/// implicit Package" generalizes to "whatever no real manifest's subtree claims," so ownership
/// is total (every file has a package) even in a repo with zero manifests, or with manifests
/// that don't cover every directory.
#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PackageNode {
    pub manifest: Option<ProjectPath>,
    #[rkyv(with = rkyv::with::Map<crate::rkyv_support::SmolStrAsString>)]
    pub name: Option<SmolStr>,
    /// Publish signal from the manifest — mirrors `ManifestFacts::private` (RFC 0011 §5).
    pub private: bool,
    /// Whether the manifest declares an explicit entry-point surface (an `exports` map or the
    /// language's equivalent) — mirrors `ManifestFacts::declares_surface`, and is the
    /// **contract gate** for `deep-import` (RFC 0011 §4): no declared surface = no declared
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
    /// (RFC 0013 §4: the patch rebuilds the workspace index from the snapshot; `surface`
    /// can't stand in — it drops out-of-tree entries and confidences).
    pub workspace_entry: Option<(ProjectPath, Confidence)>,
    /// Mirrors the claiming adapter's [`crate::adapter::AdapterDescriptor::resolves_dependency_usage`]
    /// (`true` for the implicit no-manifest package, which declares nothing). `dependency_hygiene`
    /// reads this per `DeclaredDependency::package` to decide whether "zero usage edges" means
    /// "genuinely unused" or "this language can't produce usage edges at all."
    pub resolves_dependency_usage: bool,
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
    #[rkyv(with = crate::rkyv_support::SmolStrAsString)]
    pub version_req: SmolStr,
    pub scope: DependencyScope,
}

/// [`ProjectGraph::from_snapshot_parts`]'s input, bundled into one struct purely to stay under
/// clippy's argument-count lint — every field here is one `ProjectGraph` field, verbatim.
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
    pub function_metrics: Vec<(SymbolId, SymbolMetrics)>,
    pub patch_meta: Vec<FilePatchMeta>,
}

/// The assembled language-neutral graph (contracts §1). Read-only once built; incremental
/// patching lands with the cache (RFC 0004).
#[derive(Debug, Default, PartialEq)]
pub struct ProjectGraph {
    pub files: Vec<FileNode>,
    pub symbols: Vec<SymbolNode>,
    pub dependencies: Vec<DependencyNode>,
    pub declared_dependencies: Vec<DeclaredDependency>,
    /// `(package, name)` pairs a manifest's `scripts` invoke as a leading command — dependency
    /// hygiene's (RFC 0005 §5) only usage evidence for CLI-only tools, which never produce an
    /// `ImportsDependency` edge (nothing `import`s a binary). Names here aren't necessarily
    /// declared dependencies — cross-referencing is `dependency_hygiene`'s job, not assembly's.
    pub script_invoked_dependencies: HashSet<(PackageId, SmolStr)>,
    pub packages: Vec<PackageNode>,
    pub edges: Vec<Edge>,
    /// `kndo:allow`/`kndo:allow-file` pragmas as extracted, one per file — binding (line
    /// adjacency to a declaration), validation and marking of matched findings is core logic
    /// downstream of assembly, not here (contracts §2.1). Persisted through the graph-snapshot
    /// cache like everything else in this struct, so a warm run never silently drops them.
    pub suppressions: Vec<(FileId, crate::adapter::RawSuppression)>,
    /// Each claimed language's visibility ladder (RFC 0012 §6), copied off the claiming
    /// adapter's descriptor at assembly time and keyed by the claim language `FileNode::
    /// language` stores — so analyses (pure graph functions, no adapter access) can turn a
    /// symbol's `VisibilityLevel` index into a checkable [`crate::adapter::VisibilityScope`]
    /// plus the language's own remediation label. Sorted by language for determinism.
    pub visibility_ladders: Vec<(SmolStr, Vec<crate::adapter::VisibilityRung>)>,
    /// Each claimed language's cycle tolerance (RFC 0005 §8), collected exactly like the
    /// ladders — adapter-declared data, carried here so `cyclic` stays a pure graph function.
    pub cycle_policies: Vec<(SmolStr, crate::adapter::CyclePolicy)>,
    /// Callable shapes (RFC 0005 §6), sparse — only symbols whose adapter emitted
    /// `FileFacts::functions` for them (callables), resolved to ids at assembly.
    pub function_metrics: Vec<(SymbolId, SymbolMetrics)>,
    /// RFC 0013 §4 — indexed by FileId, always `files.len()` entries; empty-defaulted for
    /// test-built graphs (the patch layer never runs there).
    pub patch_meta: Vec<FilePatchMeta>,
    /// Symbols a plugin's `annotate_symbols` marked externally consumed this run (RFC 0003 §2;
    /// consumed by `internal_only`/`private_type_leak` per RFC 0005 §7's documented exemption).
    /// Sorted, deduplicated. Always empty for a cache-hit or patched graph — plugins with
    /// graph-mutation hooks force a full rebuild every run (see `assemble_from_source`), so
    /// there is no cached-graph case where this could go stale.
    pub externally_consumed: Vec<SymbolId>,
    file_index: HashMap<ProjectPath, FileId>,
}

impl ProjectGraph {
    pub fn file_id(&self, path: &ProjectPath) -> Option<FileId> {
        self.file_index.get(path).copied()
    }

    /// Whether a plugin marked this symbol externally consumed (RFC 0003 §2 `annotate_symbols`,
    /// RFC 0005 §7's exemption). `externally_consumed` is sorted — a binary search, not a scan,
    /// since `internal_only`/`private_type_leak` call this once per symbol they'd otherwise
    /// flag.
    pub fn is_externally_consumed(&self, id: SymbolId) -> bool {
        self.externally_consumed.binary_search(&id).is_ok()
    }

    /// The visibility ladder for a claim language (RFC 0012 §6) — `None` when the language
    /// never declared one (unclaimed files, pre-ladder snapshots). An empty ladder is a
    /// deliberate declaration ("no visibility semantics") and returns `Some(&[])`.
    pub fn ladder_for(&self, language: &str) -> Option<&[crate::adapter::VisibilityRung]> {
        self.visibility_ladders
            .iter()
            .find(|(l, _)| l == language)
            .map(|(_, rungs)| rungs.as_slice())
    }

    /// The cycle policy for a claim language (RFC 0005 §8) — `None` for unclaimed files and
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

    /// Rebuilds a full graph from its persisted parts (RFC 0004 §2's `graph.bin`, `cache.rs`) —
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
            function_metrics: parts.function_metrics,
            patch_meta: parts.patch_meta,
            // Never populated for a snapshot hit or a patch (see the field's own doc comment):
            // both paths are unreachable whenever a plugin with graph-mutation hooks is
            // registered, so there is no snapshot format for this to round-trip through yet.
            externally_consumed: Vec::new(),
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
                resolves_dependency_usage: true,
            }],
            edges,
            suppressions: Vec::new(),
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
                    },
                    crate::adapter::VisibilityRung {
                        scope: crate::adapter::VisibilityScope::Public,
                        label: SmolStr::new("exported"),
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
            function_metrics: Vec::new(),
            patch_meta: vec![FilePatchMeta::default(); files_len],
            externally_consumed: Vec::new(),
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

/// One file's claim + extracted facts, plus which adapter produced them (by index into the
/// `adapters` slice passed to [`assemble`] — stable for the duration of one assembly call).
struct Claimed {
    claim: FileClaim,
    facts: crate::adapter::FileFacts,
    adapter_index: usize,
    /// RFC 0013 §4's surface signature, computed once per (adapter, content) in phase 1's
    /// parallel pass — cached and fresh facts get it identically.
    surface_sig: [u8; 32],
}

/// Phase 1's per-file work — claim, fetch-or-extract facts (facts cache first), compute the
/// surface signature — shared verbatim by the parallel full build and the incremental
/// patch's re-extraction of changed files (RFC 0013 §5).
fn claim_and_extract(
    df: &discovery::DiscoveredFile,
    adapters: &[Box<dyn LanguageAdapter>],
    cache: Option<&crate::cache::ProjectCache>,
    discovered: &discovery::DiscoveredTree,
) -> Result<Option<Claimed>, Diagnostic> {
    let Some((adapter_index, claim)) = adapters
        .iter()
        .enumerate()
        .find_map(|(i, a)| a.claim(&df.path).map(|c| (i, c)))
    else {
        return Ok(None); // no adapter claims it — still a valid, factless File node
    };
    let descriptor = adapters[adapter_index].descriptor();
    if let Some(facts) = cache.and_then(|c| {
        c.get(
            descriptor.id.as_str(),
            descriptor.facts_schema_version,
            &df.content_hash,
        )
    }) {
        let surface_sig = surface_signature(
            descriptor.id.as_str(),
            descriptor.facts_schema_version,
            &claim,
            &facts,
        );
        return Ok(Some(Claimed {
            claim,
            facts,
            adapter_index,
            surface_sig,
        }));
    }
    let content = discovered.read(&df.path).map_err(|e| Diagnostic {
        level: DiagnosticLevel::Warn,
        path: Some(df.path.clone()),
        message: format!(
            "claimed by {} but unreadable at extraction time ({e})",
            claim.language
        ),
        span: None,
    })?;
    let source = SourceFile {
        path: &df.path,
        content: &content,
    };
    let facts = adapters[adapter_index].extract(&source);
    if let Some(c) = cache {
        c.put(
            descriptor.id.as_str(),
            descriptor.facts_schema_version,
            &df.content_hash,
            &facts,
        );
    }
    let surface_sig = surface_signature(
        descriptor.id.as_str(),
        descriptor.facts_schema_version,
        &claim,
        &facts,
    );
    Ok(Some(Claimed {
        claim,
        facts,
        adapter_index,
        surface_sig,
    }))
}

/// RFC 0013 §5 — the incremental patch. Applies when the previous snapshot exists, the file
/// **set** is unchanged, no changed file is a manifest, at most 30% of files changed, and
/// every changed claimed file's surface signature is unchanged — in which case the dirty set
/// is exactly the changed files (§2.1: no resolution input moved). Everything else returns
/// `None` and the caller full-rebuilds: one fallback, always correct. Every guard runs
/// BEFORE any mutation — a half-patched graph must be unrepresentable.
///
/// The correctness obligation (§6): the returned graph is byte-identical to what the full
/// rebuild of the same tree produces — enforced by the equivalence suite, made possible by
/// the canonical-order invariant (§3a) and by sharing the exact per-file machinery
/// ([`claim_and_extract`], [`emit_file_declarations`], [`resolve_file`]) with the full path.
fn try_patch(
    discovered: &discovery::DiscoveredTree,
    adapters: &[Box<dyn LanguageAdapter>],
    cache: &crate::cache::ProjectCache,
) -> Option<(ProjectGraph, Vec<Diagnostic>)> {
    let (mut graph, mut extraction_diagnostics) = cache.latest_graph()?;

    // ---- guards, in cheapest-first order ----
    if graph.files.len() != discovered.files.len() {
        return None;
    }
    if graph
        .files
        .iter()
        .zip(&discovered.files)
        .any(|(old, new)| old.path != new.path)
    {
        return None; // adds/removes/renames renumber FileIds — §7's honest fallback
    }
    let changed: Vec<usize> = graph
        .files
        .iter()
        .zip(&discovered.files)
        .enumerate()
        .filter(|(_, (old, new))| old.content_hash != new.content_hash)
        .map(|(i, _)| i)
        .collect();
    if changed.is_empty() {
        return None; // identical tree would have hit the snapshot key — defensive
    }
    if changed.len() * 20 > graph.files.len() {
        // Stricter than RFC 0004 §5's 30% ceiling, and measured rather than assumed: the
        // patch's fixed costs (snapshot load, table rebuild) beat the saved resolution once
        // more than ~5% of files changed — the E0b suite's 1k/100-file scenario regressed
        // +22% under a 30% threshold and recovers at 5%. RFC 0013 §5 records the number.
        return None;
    }
    for &c in &changed {
        if adapters
            .iter()
            .any(|a| a.claim_manifest(&discovered.files[c].path))
        {
            return None; // manifests feed global inputs (§2.1) — full rebuild
        }
    }

    // Re-extract every changed claimed file and check its surface signature. Still no
    // mutation: any failure here must leave nothing behind.
    struct ChangedFile {
        index: usize,
        claimed: Option<Claimed>,
    }
    let mut changed_files: Vec<ChangedFile> = Vec::with_capacity(changed.len());
    for &c in &changed {
        match claim_and_extract(&discovered.files[c], adapters, Some(cache), discovered) {
            Err(_) => return None, // unreadable at extraction — let the full path diagnose
            Ok(None) => {
                if graph.files[c].language.is_some() {
                    return None; // claim is path-based; a flip here means a stale snapshot
                }
                changed_files.push(ChangedFile {
                    index: c,
                    claimed: None,
                });
            }
            Ok(Some(claimed)) => {
                if graph.patch_meta[c].surface_sig != Some(claimed.surface_sig) {
                    return None; // §2.1: some resolution input moved — full rebuild
                }
                changed_files.push(ChangedFile {
                    index: c,
                    claimed: Some(claimed),
                });
            }
        }
    }

    // Symbol runs must be contiguous per file (the full build constructs them that way) and
    // each changed file's run must align 1:1 with its fresh declarations. The signature
    // already implies alignment — but SymbolIds are load-bearing, so verify, never trust.
    let mut symbol_range: Vec<(u32, u32)> = vec![(0, 0); graph.files.len()];
    {
        let mut last_file: Option<u32> = None;
        for (idx, sym) in graph.symbols.iter().enumerate() {
            let f = sym.file.0;
            match last_file {
                Some(prev) if f == prev => symbol_range[f as usize].1 = idx as u32 + 1,
                Some(prev) if f < prev => return None, // non-contiguous — stale/corrupt
                _ => {
                    if symbol_range[f as usize].1 != 0 {
                        return None; // a second run for the same file — non-contiguous
                    }
                    symbol_range[f as usize] = (idx as u32, idx as u32 + 1);
                }
            }
            last_file = Some(f);
        }
    }
    for cf in &changed_files {
        let Some(claimed) = &cf.claimed else { continue };
        let (start, end) = symbol_range[cf.index];
        let decls = &claimed.facts.declarations;
        if (end - start) as usize != decls.len() {
            return None;
        }
        for (d, decl) in decls.iter().enumerate() {
            let sym = &graph.symbols[start as usize + d];
            if sym.name != decl.name
                || sym.kind != decl.kind
                || sym.exported != decl.exported
                || sym.visibility != decl.visibility
                || sym.member_of != decl.member_of
            {
                return None;
            }
        }
    }

    // ---- every guard passed: mutation begins ----
    let changed_set: HashSet<u32> = changed.iter().map(|&c| c as u32).collect();
    let changed_paths: HashSet<ProjectPath> = changed
        .iter()
        .map(|&c| graph.files[c].path.clone())
        .collect();

    for cf in &changed_files {
        let c = cf.index;
        graph.files[c].content_hash = discovered.files[c].content_hash;
        if let Some(claimed) = &cf.claimed {
            let (start, _) = symbol_range[c];
            for (d, decl) in claimed.facts.declarations.iter().enumerate() {
                let sym = &mut graph.symbols[start as usize + d];
                sym.span = decl.span;
                sym.signature_span = decl.signature_span;
            }
            // Test-region extents move with every edit, same as symbol spans; the *gating*
            // of imports (the only cross-file consequence) is surface-signature-guarded, so
            // refreshing the extents here keeps crap/health/hygiene containment exact while
            // `FileNode::class` (phase 2.55's demotion included) stays valid untouched.
            let mut spans = claimed.facts.test_spans.clone();
            spans.sort_unstable();
            graph.files[c].test_spans = spans;
            graph.patch_meta[c].surface_sig = Some(claimed.surface_sig);
        }
    }

    // Remove everything the changed files own — exact, thanks to Edge.owner (§2).
    graph.edges.retain(|e| !changed_set.contains(&e.owner.0));
    let mut function_metrics = std::mem::take(&mut graph.function_metrics);
    function_metrics.retain(|(id, _)| !changed_set.contains(&graph.symbols[id.0 as usize].file.0));
    graph
        .suppressions
        .retain(|(f, _)| !changed_set.contains(&f.0));
    extraction_diagnostics.retain(|d| d.path.as_ref().is_none_or(|p| !changed_paths.contains(p)));

    // ---- rebuild the resolution environment from the snapshot (§4: everything derivable) ----
    let known_files: HashSet<ProjectPath> = graph.files.iter().map(|f| f.path.clone()).collect();
    let declared_dependency_names: HashSet<SmolStr> = graph
        .declared_dependencies
        .iter()
        .map(|d| d.name.clone())
        .collect();
    let mut workspace_member_index: HashMap<SmolStr, crate::adapter::WorkspaceMember> =
        HashMap::default();
    for pkg in &graph.packages {
        let (Some(manifest), Some(name)) = (&pkg.manifest, &pkg.name) else {
            continue;
        };
        workspace_member_index
            .entry(name.clone())
            .or_insert_with(|| crate::adapter::WorkspaceMember {
                dir: SmolStr::new(core_dirname(manifest.0.as_str())),
                entry: pkg.workspace_entry.clone(),
            });
    }
    // Unit reverse-index (Java, docs/adapters/java.md §3): an import specifier there IS a
    // unit value directly, so resolution needs unit → declaring files, not just the forward
    // per-file `unit` already carried on `FileNode`. The surface-signature guard already
    // ensures a changed file's `unit` never silently drifts under the patch (§2.1), so
    // reading `graph.files`' current state here stays byte-identical to a full rebuild.
    let mut unit_index: HashMap<SmolStr, Vec<ProjectPath>> = HashMap::default();
    for f in &graph.files {
        if let Some(u) = &f.unit {
            unit_index
                .entry(u.clone())
                .or_default()
                .push(f.path.clone());
        }
    }
    for files in unit_index.values_mut() {
        files.sort();
    }
    let ctx = ResolveCtx::new(&known_files)
        .with_declared_dependencies(&declared_dependency_names)
        .with_workspace_members(&workspace_member_index)
        .with_units(&unit_index);

    let files_len = graph.files.len();
    let mut symbol_by_name_per_file: Vec<HashMap<SmolStr, SymbolId>> =
        vec![HashMap::default(); files_len];
    let mut symbol_by_qualified_per_file: Vec<HashMap<String, SymbolId>> =
        vec![HashMap::default(); files_len];
    let mut symbol_by_name_per_unit: HashMap<SmolStr, HashMap<SmolStr, SymbolId>> =
        HashMap::default();
    let mut member_by_name: HashMap<SmolStr, Vec<SymbolId>> = HashMap::default();
    for (idx, sym) in graph.symbols.iter().enumerate() {
        let i = sym.file.0 as usize;
        let id = SymbolId(idx as u32);
        match &sym.member_of {
            None => {
                symbol_by_name_per_file[i].insert(sym.name.clone(), id);
                if let Some(unit) = &graph.files[i].unit {
                    symbol_by_name_per_unit
                        .entry(unit.clone())
                        .or_default()
                        .insert(sym.name.clone(), id);
                }
            }
            Some(owner) => {
                member_by_name.entry(sym.name.clone()).or_default().push(id);
                symbol_by_qualified_per_file[i].insert(format!("{owner}.{}", sym.name), id);
            }
        }
    }
    // Aliases go in after declarations, vacant-only — the same outcome pass A + the fixpoint
    // produce (declarations always precede aliases there too).
    for (i, meta) in graph.patch_meta.iter().enumerate() {
        for alias in &meta.reexport_aliases {
            symbol_by_name_per_file[i]
                .entry(alias.name.clone())
                .or_insert(alias.symbol);
        }
    }
    let file_unit: Vec<Option<SmolStr>> = graph.files.iter().map(|f| f.unit.clone()).collect();
    let unit_name_by_file: Vec<Option<SmolStr>> = graph
        .patch_meta
        .iter()
        .map(|m| m.unit_name.clone())
        .collect();
    let ladders: std::collections::BTreeMap<SmolStr, Vec<crate::adapter::VisibilityRung>> =
        graph.visibility_ladders.iter().cloned().collect();

    // Library roots for the changed files, from the KEPT (manifest-owned) edges — a changed
    // file's own in-source production roots were just removed and regenerate below.
    let mut library_root_files: HashMap<FileId, Confidence> = HashMap::default();
    for e in &graph.edges {
        if let EdgeKind::Root {
            kind: crate::vocab::RootKind::Production,
            target: NodeRef::File(f),
        } = e.kind
        {
            if changed_set.contains(&f.0) {
                let entry = library_root_files.entry(f).or_insert(e.confidence);
                *entry = (*entry).max(e.confidence);
            }
        }
    }
    let mut role_root_files: HashMap<FileId, crate::vocab::RootKind> = HashMap::default();
    for cf in &changed_files {
        if cf.claimed.is_some() {
            if let Some(class) = graph.files[cf.index].class {
                let kind = match class.role {
                    crate::vocab::FileRole::Test => Some(crate::vocab::RootKind::Test),
                    crate::vocab::FileRole::Tooling => Some(crate::vocab::RootKind::Tooling),
                    crate::vocab::FileRole::Production => None,
                };
                if let Some(kind) = kind {
                    role_root_files.insert(FileId(cf.index as u32), kind);
                }
            }
        }
    }

    // ---- regenerate the changed files' contributions, via the SAME machinery as the full
    // build (emit_file_declarations + resolve_file) ----
    let mut new_edges: Vec<Edge> = Vec::new();
    let mut new_metrics: Vec<(SymbolId, SymbolMetrics)> = Vec::new();
    let mut resolved_outputs: Vec<ResolvedFile> = Vec::new();
    {
        let tables = ResolveTables {
            files: &graph.files,
            file_index: &graph.file_index,
            symbols: &graph.symbols,
            symbol_by_name_per_file: &symbol_by_name_per_file,
            symbol_by_qualified_per_file: &symbol_by_qualified_per_file,
            symbol_by_name_per_unit: &symbol_by_name_per_unit,
            member_by_name: &member_by_name,
            file_unit: &file_unit,
            unit_name_by_file: &unit_name_by_file,
            ladders: &ladders,
            ctx: &ctx,
        };
        for cf in &changed_files {
            let Some(claimed) = &cf.claimed else { continue };
            let c = cf.index;
            let file_id = FileId(c as u32);
            let adapter = &adapters[claimed.adapter_index];
            let adapter_id = adapter.descriptor().id;

            // Phase 2.6's role-derived file root (owned by the file, hence removed above).
            if let Some(&kind) = role_root_files.get(&file_id) {
                new_edges.push(Edge {
                    kind: EdgeKind::Root {
                        kind,
                        target: NodeRef::File(file_id),
                    },
                    confidence: Confidence::Probable,
                    source: Provenance::Adapter(adapter_id.clone()),
                    span: None,
                    owner: file_id,
                });
            }
            // Phase 3a emissions, shared emitter.
            let em = emit_file_declarations(
                c,
                &claimed.facts,
                adapter_id.as_str(),
                symbol_range[c].0,
                &graph.symbols,
                &symbol_by_name_per_file[c],
                &symbol_by_qualified_per_file[c],
                &library_root_files,
                &role_root_files,
            );
            new_edges.extend(em.edges);
            new_metrics.extend(em.metrics);
            // Phase 3a-bis's promotions: the aliases themselves are unchanged under the guard
            // (persisted, order-independent state); only the edges — owned by this file and
            // removed above — regenerate, spans refreshed from the fresh imports.
            if let Some(&confidence) = library_root_files.get(&file_id) {
                for alias in &graph.patch_meta[c].reexport_aliases {
                    let span = claimed
                        .facts
                        .imports
                        .iter()
                        .filter(|imp| imp.reexported)
                        .find(|imp| imp.bindings.iter().any(|b| b.local == alias.name))
                        .map(|imp| imp.span);
                    new_edges.push(Edge {
                        kind: EdgeKind::Root {
                            kind: crate::vocab::RootKind::Production,
                            target: NodeRef::Symbol(alias.symbol),
                        },
                        confidence,
                        source: Provenance::Adapter(adapter_id.clone()),
                        span,
                        owner: file_id,
                    });
                }
            }
            // Phase 2.7's surface-expansion edges (owned by this file, removed above):
            // membership itself is stable under the guard — kept edges from other owners
            // still mark this file (and its targets stay members through edges THEY own) —
            // so only the edges this file emits regenerate, from its unchanged re-exports.
            if let Some(&confidence) = library_root_files.get(&file_id) {
                for imp in claimed
                    .facts
                    .imports
                    .iter()
                    .filter(|i| i.reexported && i.bindings.is_empty())
                {
                    let spec = ImportSpec {
                        specifier: imp.specifier.clone(),
                        from: graph.files[c].path.clone(),
                    };
                    let target = match adapter.resolve(&spec, &ctx) {
                        Resolution::File(path, _) => path,
                        Resolution::WorkspaceMember { target, .. } => target,
                        _ => continue,
                    };
                    let Some(&target) = graph.file_index.get(&target) else {
                        continue;
                    };
                    new_edges.push(Edge {
                        kind: EdgeKind::Root {
                            kind: crate::vocab::RootKind::Production,
                            target: NodeRef::File(target),
                        },
                        confidence,
                        span: Some(imp.span),
                        source: Provenance::Adapter(adapter_id.clone()),
                        owner: file_id,
                    });
                }
            }
            // Phase 3b, shared resolver.
            resolved_outputs.push(resolve_file(c, &claimed.facts, &**adapter, &tables));
        }
    }

    // ---- apply, then restore the canonical order (§3a) ----
    let mut dep_index: HashMap<SmolStr, DependencyId> = graph
        .dependencies
        .iter()
        .enumerate()
        .map(|(i, d)| (d.name.clone(), DependencyId(i as u32)))
        .collect();
    graph.edges.extend(new_edges);
    for out in resolved_outputs {
        graph.edges.extend(out.edges);
        for (name, confidence, span, from, source) in out.dep_imports {
            let to = *dep_index.entry(name.clone()).or_insert_with(|| {
                let id = DependencyId(graph.dependencies.len() as u32);
                graph
                    .dependencies
                    .push(DependencyNode { name: name.clone() });
                id
            });
            graph.edges.push(Edge {
                kind: EdgeKind::ImportsDependency { from, to },
                confidence,
                source,
                span: Some(span),
                owner: from,
            });
        }
        extraction_diagnostics.extend(out.diagnostics);
        graph.suppressions.extend(out.suppressions);
    }
    function_metrics.extend(new_metrics);
    function_metrics.sort_by_key(|(id, _)| *id);
    graph.function_metrics = function_metrics;
    graph.edges.sort_unstable();
    graph.suppressions.sort_by_key(|(f, _)| *f);
    extraction_diagnostics.sort_unstable();

    cache.count_graph_hit(); // the previous snapshot genuinely served this run
    Some((graph, extraction_diagnostics))
}

/// One file's phase-3b output, merged deterministically in FileId order.
struct ResolvedFile {
    edges: Vec<Edge>,
    /// `(name, confidence, span, from, provenance)` — becomes `ImportsDependency` in the
    /// merge once the name has a deterministic id.
    dep_imports: Vec<(
        SmolStr,
        Confidence,
        crate::adapter::Span,
        FileId,
        Provenance,
    )>,
    diagnostics: Vec<Diagnostic>,
    suppressions: Vec<(FileId, crate::adapter::RawSuppression)>,
}

// Whether a declaration in `decl_file` at `scope` is visible to a reference site in
// `site_file` (RFC 0012 §6). Scopes nest (File ⊂ Unit ⊂ Package ⊂ Public), so each arm
// accepts everything the narrower one would: a Unit-scoped Go method is visible to its own
// file whether or not the adapter set a unit key.
fn scope_contains_site(
    scope: crate::adapter::VisibilityScope,
    decl_file: usize,
    site_file: usize,
    file_unit: &[Option<SmolStr>],
    files: &[FileNode],
) -> bool {
    use crate::adapter::VisibilityScope::*;
    match scope {
        File => decl_file == site_file,
        Unit => {
            decl_file == site_file
                || matches!(
                    (&file_unit[decl_file], &file_unit[site_file]),
                    (Some(a), Some(b)) if a == b
                )
        }
        Package => files[decl_file].package == files[site_file].package,
        Public => true,
    }
}

/// Everything phase 3b's per-file resolution reads — immutable once the symbol tables are
/// built. A named struct (not captured locals) because the incremental patch (RFC 0013 §5)
/// builds the same tables from the snapshot and calls the same [`resolve_file`]: one
/// resolution semantics, two data sources, zero drift.
struct ResolveTables<'a> {
    files: &'a [FileNode],
    file_index: &'a HashMap<ProjectPath, FileId>,
    symbols: &'a [SymbolNode],
    symbol_by_name_per_file: &'a [HashMap<SmolStr, SymbolId>],
    symbol_by_qualified_per_file: &'a [HashMap<String, SymbolId>],
    symbol_by_name_per_unit: &'a HashMap<SmolStr, HashMap<SmolStr, SymbolId>>,
    member_by_name: &'a HashMap<SmolStr, Vec<SymbolId>>,
    file_unit: &'a [Option<SmolStr>],
    unit_name_by_file: &'a [Option<SmolStr>],
    ladders: &'a std::collections::BTreeMap<SmolStr, Vec<crate::adapter::VisibilityRung>>,
    ctx: &'a ResolveCtx<'a>,
}

/// One file's phase-3b contributions (imports, bindings, references, dynamics, diagnostics,
/// suppressions) — the parallel full build and the incremental patch both call this.
fn resolve_file(
    i: usize,
    facts: &crate::adapter::FileFacts,
    adapter: &dyn LanguageAdapter,
    t: &ResolveTables<'_>,
) -> ResolvedFile {
    let ResolveTables {
        files,
        file_index,
        symbols,
        symbol_by_name_per_file,
        symbol_by_qualified_per_file,
        symbol_by_name_per_unit,
        member_by_name,
        file_unit,
        unit_name_by_file,
        ladders,
        ctx,
    } = t;
    let file_id = FileId(i as u32);
    let provenance = || Provenance::Adapter(adapter.descriptor().id.clone());
    let mut out = ResolvedFile {
        edges: Vec::new(),
        dep_imports: Vec::new(),
        diagnostics: Vec::new(),
        suppressions: Vec::new(),
    };

    // Local name -> target symbol, from this file's import bindings — the fact that lets a
    // `RawReference` to an *imported* name resolve cross-file instead of only same-file.
    let mut bound_symbols: HashMap<SmolStr, SymbolId> = HashMap::default();
    // Qualifier -> resolved in-repo target file (RFC 0012 §9): the import's explicit
    // `local_alias`, or — unaliased — the *target's own* declared `unit_name`. This is
    // where the dir≠package problem dissolves: only assembly holds both sides, so the
    // qualifier for `gopkg.in/yaml.v3`-style imports comes from the target's `package`
    // clause, never from a guess about the specifier. First import wins on a duplicate
    // qualifier (Go rejects that program anyway — deterministic either way).
    let mut qualifier_targets: HashMap<SmolStr, FileId> = HashMap::default();

    for imp in &facts.imports {
        let spec = ImportSpec {
            specifier: imp.specifier.clone(),
            from: files[i].path.clone(),
        };
        // A workspace-member resolution is BOTH targets at once (RFC 0011 §4): the
        // concrete internal file (reachability is real, cross-package) and the named
        // dependency (the declaration contract is real too — undeclared siblings are
        // phantom internal dependencies, declared-but-unimported ones are unused).
        // Stdlib: not a graph node — there is nothing to point an edge at. Unresolved:
        // resolution is intentionally incomplete right now (self-reference imports,
        // exports maps — spec §3); turning it into a finding is the future `unresolved`
        // analysis's job, not assembly's (RFC 0005 §5).
        let (file_target, dep_target) = match adapter.resolve(&spec, ctx) {
            Resolution::File(path, confidence) => (Some((path, confidence)), None),
            Resolution::Dependency(name, confidence) => (None, Some((name, confidence))),
            Resolution::WorkspaceMember {
                name,
                target,
                confidence,
            } => (Some((target, confidence)), Some((name, confidence))),
            Resolution::Stdlib | Resolution::Unresolved => (None, None),
        };

        if let Some((path, confidence)) = file_target {
            // Resolvers only ever match against `ctx`'s known-files set, so this
            // must be Some — defensive skip, not a silent contract violation, if not.
            if let Some(&to) = file_index.get(&path) {
                out.edges.push(Edge {
                    owner: file_id,
                    kind: EdgeKind::ImportsFile { from: file_id, to },
                    confidence,
                    source: provenance(),
                    span: Some(imp.span),
                });
                for binding in &imp.bindings {
                    let exported_name = binding
                        .imported
                        .clone()
                        .unwrap_or_else(|| SmolStr::new("default"));
                    // Same-file first; then the target file's own unit (package-scoped
                    // languages, RFC 0002 §2 `FileFacts::unit`) — a Go import names a
                    // *package* (a directory of files), and `Resolution::File`'s target is
                    // necessarily just one representative file in it (contracts §2 has no
                    // multi-file resolution target), so the symbol a qualified access binds
                    // to may live in any of that directory's other files.
                    let symbol_id = symbol_by_name_per_file[to.0 as usize]
                        .get(&exported_name)
                        .or_else(|| {
                            file_unit[to.0 as usize].as_ref().and_then(|unit| {
                                symbol_by_name_per_unit
                                    .get(unit)
                                    .and_then(|t| t.get(&exported_name))
                            })
                        })
                        .copied();
                    if let Some(symbol_id) = symbol_id {
                        bound_symbols.insert(binding.local.clone(), symbol_id);
                    }
                }
                let qualifier = imp
                    .local_alias
                    .clone()
                    .or_else(|| unit_name_by_file[to.0 as usize].clone());
                if let Some(q) = qualifier {
                    qualifier_targets.entry(q).or_insert(to);
                }
                // The namespace escaped static tracking (`ns[key]`, ns passed
                // along) — every symbol in the target is plausibly used
                // (RFC 0005 §1: "wildcard over that namespace's exports").
                if imp.opaque_namespace_use {
                    out.edges.push(Edge {
                        owner: file_id,
                        kind: EdgeKind::Wildcard { from: to },
                        confidence: Confidence::Possible,
                        source: provenance(),
                        span: Some(imp.span),
                    });
                }
            }
        }
        if let Some((name, confidence)) = dep_target {
            out.dep_imports
                .push((name, confidence, imp.span, file_id, provenance()));
        }
    }

    // Edge attribution (RFC 0012 §4): a reference carrying `within` is attributed to the
    // enclosing symbol it executes inside — resolved against this file's own declarations
    // (bare names, then the qualified member table, same convention as member root
    // targets). **Any miss falls back to file attribution — today's over-approximation,
    // the safe direction** (regression-tested; this fallback is the design's load-bearing
    // safety property). With symbol attribution, a dead function's calls no longer keep
    // its callees alive: RFC 0005 §1's execution rule ("a symbol-attributed reference
    // fires only when its symbol is reached") plus its module-load rule make transitive
    // death visible. `within: None` — module-level code, and every adapter that doesn't
    // emit the field — keeps file attribution: load-time references fire when the file
    // loads, exactly as before.
    //
    // Resolution order for the *target*: bound (imported) names first, then same-file
    // declarations, then same-unit siblings (`FileFacts::unit` — Go's package-scoped
    // visibility, absent for file-scoped languages) — real JS/TS can't have both of the
    // first two share a name at module scope, so that ordering is never actually contested
    // by valid code, just a defensive default; the unit fallback is the one genuinely load-
    // bearing case (a sibling file in the same Go package, no import involved at all).
    // No lookup models block/parameter shadowing: a same-named local could (incorrectly,
    // but safely — see module docs) resolve to an unrelated declaration.
    for reference in &facts.references {
        let from = reference
            .within
            .as_ref()
            .and_then(|within| {
                symbol_by_name_per_file[i]
                    .get(within)
                    .or_else(|| symbol_by_qualified_per_file[i].get(within.as_str()))
            })
            .map(|&s| NodeRef::Symbol(s))
            .unwrap_or(NodeRef::File(file_id));

        // Qualified references (RFC 0012 §9): `q.name` where `q` matches an import
        // qualifier resolves `name` inside that target (its own declarations, then its
        // unit siblings — a Go import names a package, and the symbol may live in any of
        // the package's files) at Certain. Hit or miss, a matched qualifier *settles*
        // resolution — the name lives in that target or nowhere; this file's own tables
        // are never candidates. A qualifier matching no import is a receiver expression
        // (`t.helper()`): the name is a member access by construction, so it skips the
        // free-name tables and goes straight to the duck-typed member fallback below —
        // where before §9 a same-file free function sharing the member's name would have
        // (incorrectly, if safely) captured the reference.
        let mut is_receiver_access = false;
        if let Some(q) = &reference.scope_context {
            match qualifier_targets.get(q) {
                Some(&target_file) => {
                    let t = target_file.0 as usize;
                    let sym = symbol_by_name_per_file[t]
                        .get(&reference.name)
                        .or_else(|| {
                            file_unit[t].as_ref().and_then(|unit| {
                                symbol_by_name_per_unit
                                    .get(unit)
                                    .and_then(|tab| tab.get(&reference.name))
                            })
                        })
                        .copied();
                    if let Some(to) = sym {
                        out.edges.push(Edge {
                            owner: file_id,
                            kind: EdgeKind::References {
                                from,
                                to,
                                kind: reference.kind,
                            },
                            confidence: Confidence::Certain,
                            source: provenance(),
                            span: Some(reference.span),
                        });
                    }
                    continue;
                }
                None => is_receiver_access = true,
            }
        }

        let target = if is_receiver_access {
            None
        } else {
            bound_symbols
                .get(&reference.name)
                .or_else(|| symbol_by_name_per_file[i].get(&reference.name))
                .or_else(|| {
                    file_unit[i].as_ref().and_then(|unit| {
                        symbol_by_name_per_unit
                            .get(unit)
                            .and_then(|t| t.get(&reference.name))
                    })
                })
                .copied()
        };
        if let Some(to) = target {
            out.edges.push(Edge {
                owner: file_id,
                kind: EdgeKind::References {
                    from,
                    to,
                    kind: reference.kind,
                },
                confidence: Confidence::Certain,
                source: provenance(),
                span: Some(reference.span),
            });
            continue;
        }

        // Duck-typed member fallback (RFC 0012 §3, implementing RFC 0002 §5's ladder rule
        // "duck-typed method with one candidate → probable"): an unresolved name that
        // matches member declarations plausibly targets any of them — extraction has no
        // receiver types, so honesty lives in the confidence, not in a guess. The
        // plausible set is scoped by each candidate's own declared visibility (RFC 0012
        // §6): a member is a candidate iff its visibility scope *contains this reference
        // site* — an unexported Go method (scope Unit) only for sites in its own unit, a
        // public member (scope Public) project-wide. A rung the ladder doesn't cover
        // (index out of range, no ladder declared) counts as Public — the conservative
        // wider mapping: over-approximating who may see a member only adds keep-alive
        // edges. Cross-language candidates are excluded (a bare-name site never plausibly
        // calls another language's member — same reasoning as §5's ladder-index guard).
        // One candidate ⇒ Probable, several ⇒ Possible each — all get edges (conservative
        // keep-alive; dead-is-certain is untouched, since a member no call-site anywhere
        // matches still has zero edges).
        let candidates: Vec<SymbolId> = member_by_name
            .get(&reference.name)
            .map(|all| {
                all.iter()
                    .copied()
                    .filter(|&m| {
                        let sym = &symbols[m.0 as usize];
                        let j = sym.file.0 as usize;
                        if files[j].language != files[i].language {
                            return false;
                        }
                        let scope = files[j]
                            .language
                            .as_ref()
                            .and_then(|lang| ladders.get(lang))
                            .and_then(|ladder| ladder.get(sym.visibility.0 as usize))
                            .map(|rung| rung.scope)
                            .unwrap_or(crate::adapter::VisibilityScope::Public);
                        scope_contains_site(scope, j, i, file_unit, files)
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !candidates.is_empty() {
            let confidence = if candidates.len() == 1 {
                Confidence::Probable
            } else {
                Confidence::Possible
            };
            for to in candidates {
                out.edges.push(Edge {
                    owner: file_id,
                    kind: EdgeKind::References {
                        from, // same within-or-file attribution as the exact-match path
                        to,
                        kind: reference.kind,
                    },
                    confidence,
                    source: provenance(),
                    span: Some(reference.span),
                });
            }
        }
    }

    // Dynamic constructs → wildcard edges (RFC 0005 §1: "one mechanism, not two").
    // Un-narrowed (`eval`, `require(expr)` with no static prefix): a `Wildcard` edge from
    // this file — reachability expands it over the file's own symbols at `possible`.
    // Narrowed (`import(`./locales/${x}`)` → that directory): the plausible target set is
    // the directory's files instead, expressed with existing edge kinds — a `possible`
    // ImportsFile edge to every discovered file under the directory (unclaimed ones
    // included: a dynamically-loaded .json is a real target), plus a `Wildcard` edge
    // *from each target*, because a dynamically-imported module is consumed opaquely —
    // no binding names exist, so every symbol in it is plausibly used. Without that
    // second edge the target files would be alive but their exported symbols still
    // certain-dead: exactly the false positive the narrowing exists to prevent.
    for dynamic in &facts.dynamics {
        match dynamic.narrowed_to.as_deref().filter(|d| !d.is_empty()) {
            Some(dir) => {
                for (j, file) in files.iter().enumerate() {
                    if j == i || !package_owns(dir, core_dirname(file.path.0.as_str())) {
                        continue;
                    }
                    let target = FileId(j as u32);
                    out.edges.push(Edge {
                        owner: file_id,
                        kind: EdgeKind::ImportsFile {
                            from: file_id,
                            to: target,
                        },
                        confidence: Confidence::Possible,
                        source: provenance(),
                        span: Some(dynamic.span),
                    });
                    out.edges.push(Edge {
                        owner: file_id,
                        kind: EdgeKind::Wildcard { from: target },
                        confidence: Confidence::Possible,
                        source: provenance(),
                        span: Some(dynamic.span),
                    });
                }
            }
            // Empty-string narrowing would prefix-match the whole project — treat it as
            // the adapter meaning "no narrowing" rather than "everything".
            None => out.edges.push(Edge {
                owner: file_id,
                kind: EdgeKind::Wildcard { from: file_id },
                confidence: Confidence::Possible,
                source: provenance(),
                span: Some(dynamic.span),
            }),
        }
    }

    for d in &facts.diagnostics {
        out.diagnostics.push(Diagnostic {
            level: d.level,
            path: Some(files[i].path.clone()),
            message: d.message.clone(),
            span: d.span,
        });
    }

    for s in &facts.suppressions {
        out.suppressions.push((file_id, s.clone()));
    }

    out
}

/// One file's declaration-derived emissions — Declares edges, library/role export promotions
/// (RFC 0011 §5), in-source roots, and function metrics — given the file's facts and its
/// already-assigned contiguous symbol run starting at `first_symbol`. THE single emitter for
/// this logic: the full build's pass B and the incremental patch (RFC 0013 §5) both call it,
/// so the two paths cannot drift.
struct DeclarationEmissions {
    edges: Vec<Edge>,
    metrics: Vec<(SymbolId, SymbolMetrics)>,
}

#[allow(clippy::too_many_arguments)]
fn emit_file_declarations(
    i: usize,
    facts: &crate::adapter::FileFacts,
    adapter_id: &str,
    first_symbol: u32,
    symbols: &[SymbolNode],
    bare_table: &HashMap<SmolStr, SymbolId>,
    qualified_table: &HashMap<String, SymbolId>,
    library_root_files: &HashMap<FileId, Confidence>,
    role_root_files: &HashMap<FileId, crate::vocab::RootKind>,
) -> DeclarationEmissions {
    let file_id = FileId(i as u32);
    let provenance = || Provenance::Adapter(SmolStr::new(adapter_id));
    let mut edges = Vec::new();
    let mut metrics = Vec::new();

    for (d, decl) in facts.declarations.iter().enumerate() {
        let symbol_id = SymbolId(first_symbol + d as u32);
        edges.push(Edge {
            kind: EdgeKind::Declares {
                file: file_id,
                symbol: symbol_id,
            },
            confidence: Confidence::Certain,
            source: provenance(),
            span: Some(decl.span),
            owner: file_id,
        });

        // In-source Test roots, DERIVED (contracts §2): a declaration inside a test region
        // (`FileFacts::test_spans`) is test infrastructure — `#[test]` fns and everything in
        // a `#[cfg(test)]` module alike. The spans are the single producer-side declaration;
        // adapters never emit these roots themselves, so the two representations cannot
        // drift. Certain: the gate is declared in source, the runner is the consumer.
        if span_in_test_region(&facts.test_spans, decl.span) {
            edges.push(Edge {
                kind: EdgeKind::Root {
                    kind: crate::vocab::RootKind::Test,
                    target: NodeRef::Symbol(symbol_id),
                },
                confidence: Confidence::Certain,
                source: provenance(),
                span: Some(decl.span),
                owner: file_id,
            });
        }

        // Library-mode promotion (RFC 0011 §5): this file is a manifest-declared production
        // root and this symbol is exported from it, so it's part of the package's public
        // API — a production root in its own right, not just "alive because the file is."
        if decl.exported {
            if let Some(&confidence) = library_root_files.get(&file_id) {
                edges.push(Edge {
                    kind: EdgeKind::Root {
                        kind: crate::vocab::RootKind::Production,
                        target: NodeRef::Symbol(symbol_id),
                    },
                    confidence,
                    source: provenance(),
                    span: Some(decl.span),
                    owner: file_id,
                });
            }
            // Same promotion for role-derived roots: a config file's exports ARE its
            // interface to the tool that loads it, and a test file's exports may be shared
            // fixtures — the consumer is outside the graph either way.
            if let Some(&kind) = role_root_files.get(&file_id) {
                edges.push(Edge {
                    kind: EdgeKind::Root {
                        kind,
                        target: NodeRef::Symbol(symbol_id),
                    },
                    confidence: Confidence::Probable,
                    source: provenance(),
                    span: Some(decl.span),
                    owner: file_id,
                });
            }
        }
    }

    // Callable shapes (RFC 0005 §6): adapter names resolve exactly like root targets — bare
    // table first, then the qualified member table; a no-match is dropped silently.
    for fm in &facts.functions {
        let resolved = bare_table
            .get(fm.symbol.as_str())
            .or_else(|| qualified_table.get(fm.symbol.as_str()));
        if let Some(&symbol_id) = resolved {
            metrics.push((
                symbol_id,
                SymbolMetrics {
                    cyclomatic: fm.cyclomatic,
                    loc: fm.loc,
                    token_count: fm.token_count,
                    fingerprints: fm.fingerprints.clone(),
                },
            ));
        }
    }

    // In-source roots (RawRoot), as distinct from manifest-declared ones: they target
    // something *within* the file being extracted, never a different file.
    for root in &facts.roots {
        let target = match &root.target {
            RawRootTarget::WholeFile => Some(NodeRef::File(file_id)),
            RawRootTarget::Declaration(name) => bare_table
                .get(name)
                .or_else(|| qualified_table.get(name.as_str()))
                .map(|&s| NodeRef::Symbol(s)),
        };
        if let Some(target) = target {
            let span = match target {
                NodeRef::Symbol(s) => Some(symbols[s.0 as usize].span),
                NodeRef::File(_) => None,
            };
            edges.push(Edge {
                kind: EdgeKind::Root {
                    kind: root.kind,
                    target,
                },
                confidence: root.confidence,
                source: provenance(),
                span,
                owner: file_id,
            });
        }
    }

    DeclarationEmissions { edges, metrics }
}

/// The span-normalized surface signature (RFC 0013 §4): everything about a file that OTHER
/// files' resolution — or this file's own derived-id stability — can depend on, hashed;
/// bodies, spans, references, metrics, and suppressions excluded, so they move freely under
/// the patch guard. Field-for-field per the RFC's table; serde+bincode gives an unambiguous
/// byte layout without hand-rolled framing.
fn surface_signature(
    adapter_id: &str,
    facts_schema_version: u32,
    claim: &FileClaim,
    facts: &crate::adapter::FileFacts,
) -> [u8; 32] {
    /// One import's surface tuple: specifier, kind, side_effect_only, type_only,
    /// confidence, bindings (local, imported), reexported, opaque_namespace_use, local_alias,
    /// in-test-region (derived from `FileFacts::test_spans` containment — see the View site).
    type ImportView<'a> = (
        &'a str,
        &'a crate::adapter::ImportKind,
        bool,
        bool,
        Confidence,
        Vec<(&'a str, Option<&'a str>)>,
        bool,
        bool,
        Option<&'a str>,
        bool,
    );

    #[derive(serde::Serialize)]
    struct View<'a> {
        adapter_id: &'a str,
        facts_schema_version: u32,
        language: &'a str,
        class: crate::vocab::FileClass,
        detected_origin: Option<crate::vocab::FileOrigin>,
        unit: Option<&'a str>,
        unit_name: Option<&'a str>,
        declarations: Vec<(
            &'a str,
            &'a crate::vocab::SymbolKind,
            bool,
            crate::adapter::VisibilityLevel,
            Option<&'a str>,
        )>,
        imports: Vec<ImportView<'a>>,
        roots: Vec<(
            crate::vocab::RootKind,
            &'a crate::adapter::RawRootTarget,
            Confidence,
        )>,
        dynamics: Vec<(&'a str, Option<&'a str>)>,
    }
    let view = View {
        adapter_id,
        facts_schema_version,
        language: claim.language.as_str(),
        class: claim.class,
        detected_origin: facts.detected_origin,
        unit: facts.unit.as_deref(),
        unit_name: facts.unit_name.as_deref(),
        declarations: facts
            .declarations
            .iter()
            .map(|d| {
                (
                    d.name.as_str(),
                    &d.kind,
                    d.exported,
                    d.visibility,
                    d.member_of.as_deref(),
                )
            })
            .collect(),
        imports: facts
            .imports
            .iter()
            .map(|i| {
                (
                    i.specifier.as_str(),
                    &i.kind,
                    i.side_effect_only,
                    i.type_only,
                    i.confidence,
                    i.bindings
                        .iter()
                        .map(|b| (b.local.as_str(), b.imported.as_deref()))
                        .collect(),
                    i.reexported,
                    i.opaque_namespace_use,
                    i.local_alias.as_deref(),
                    // Span-derived but span-*stable*: pure reformatting preserves whether an
                    // import sits inside a test region; a move across the boundary changes
                    // resolution-relevant behavior (phase 2.55's demotion, hygiene's site
                    // role) and must decline the patch.
                    span_in_test_region(&facts.test_spans, i.span),
                )
            })
            .collect(),
        roots: facts
            .roots
            .iter()
            .map(|r| (r.kind, &r.target, r.confidence))
            .collect(),
        dynamics: facts
            .dynamics
            .iter()
            .map(|d| (d.reason.as_str(), d.narrowed_to.as_deref()))
            .collect(),
    };
    let bytes = bincode::serialize(&view).unwrap_or_default();
    *blake3::hash(&bytes).as_bytes()
}

/// Directory part of a project-relative path (`""` for root-level files). A private duplicate
/// of `kndo-adapter-toolkit::paths::dirname` — trivial string logic, but the core cannot depend
/// on an adapter-side crate (the ignorance rule runs both directions: adapters depend on the
/// core, never the reverse). `pub(crate)`: sibling modules (analyses doing their own directory
/// reasoning, e.g. `unused`'s rollup) reuse it rather than re-deriving the same logic.
pub(crate) fn core_dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Does `ancestor_dir` govern (contain, at any depth, or equal) `dir`? The empty (project-root)
/// dir governs everything. Doubles as both RFC 0011 §3's nearest-manifest-ancestor test (this
/// module's own use) and a generic "is A an ancestor-or-self of B" check other analyses reuse
/// (e.g. `unused`'s directory rollup, deciding whether a narrower rollup is already covered by
/// a wider one).
pub(crate) fn package_owns(manifest_dir: &str, file_dir: &str) -> bool {
    manifest_dir.is_empty()
        || file_dir == manifest_dir
        || file_dir
            .strip_prefix(manifest_dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Bumped whenever the *persisted* shape of a graph snapshot changes in a way that isn't
/// already covered by an adapter's own `facts_schema_version` — e.g. a new node/edge kind, or
/// an assembly-algorithm change that could produce a different graph from the same facts. Feeds
/// [`compute_graph_key`] (RFC 0004 §3's "core graph-schema version"); a bump here invalidates
/// every project's cached `graph.bin` on the next run, same as any other key-input change.
pub const GRAPH_SCHEMA_VERSION: u32 = 14; // 14: in-source Test roots derived from test_spans containment (adapters no longer emit them); 13: FileNode.test_spans + phase 2.55 test-gated module demotion (sub-file test regions); 12: library-surface fixpoint (phase 2.7 — same inputs now assemble surface Root edges, prior snapshots are semantically stale); 11: PackageNode.workspace_entry (RFC 0013 §4); 10: patch layer (RFC 0013 §4 — Edge.owner, FilePatchMeta, extraction-only stored diagnostics); 9: SymbolMetrics.token_count (RFC 0005 §11); 8: function_metrics (RFC 0005 §6); 7: cycle policies (§8); 6: PackageNode surface (RFC 0011 §4); 5: ladders + FileNode.unit (RFC 0012 §6); 4: RefKind + signature_span (§5); 3: within (§4)

/// The graph snapshot's cache key (RFC 0004 §3, `cache.rs`'s `graph.bin`): a single digest
/// folding in the *whole* discovered file set (every path + content hash — this already
/// subsumes "manifest hashes," since a manifest is just one more discovered file) plus each
/// registered adapter's id and facts-schema version plus [`GRAPH_SCHEMA_VERSION`] itself. Two
/// key inputs RFC 0004 §3 also names — a kndo config hash and the active plugin set — don't
/// exist as subsystems yet, so they're honestly absent rather than faked.
///
/// Every variable-length field (paths, adapter ids) is length-prefixed before its bytes so the
/// scheme is unambiguous by construction, not merely collision-resistant by luck of the input
/// distribution — two different file sets can never fold to the same byte stream before
/// hashing.
pub(crate) fn compute_graph_key(
    discovered_files: &[discovery::DiscoveredFile],
    adapters: &[Box<dyn LanguageAdapter>],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&GRAPH_SCHEMA_VERSION.to_le_bytes());

    // `discovered_files` is already sorted by path (discovery.rs's own determinism invariant),
    // so this fold is stable across runs regardless of filesystem walk order.
    for f in discovered_files {
        let path_bytes = f.path.0.as_bytes();
        hasher.update(&(path_bytes.len() as u32).to_le_bytes());
        hasher.update(path_bytes);
        hasher.update(&f.content_hash);
    }

    let mut adapter_versions: Vec<(String, u32)> = adapters
        .iter()
        .map(|a| {
            let d = a.descriptor();
            (d.id.to_string(), d.facts_schema_version)
        })
        .collect();
    adapter_versions.sort();
    for (id, version) in &adapter_versions {
        hasher.update(&(id.len() as u32).to_le_bytes());
        hasher.update(id.as_bytes());
        hasher.update(&version.to_le_bytes());
    }

    *hasher.finalize().as_bytes()
}

/// Discovers, claims, extracts, resolves, and links — the full RFC 0001 §4 pipeline up to
/// (not including) analyses. Diagnostics accumulate rather than abort: a graph that omits one
/// unreadable file's facts is far more useful than no graph at all (RFC 0001 §6). Always cold
/// (no facts cache consulted) — see [`assemble_with_cache`] for the warm path.
pub fn assemble(
    root: &Path,
    adapters: &[Box<dyn LanguageAdapter>],
    plugins: &[Box<dyn crate::plugin::Plugin>],
) -> Result<(ProjectGraph, Vec<Diagnostic>), DiscoveryError> {
    assemble_with_cache(root, adapters, plugins, None)
}

/// Same pipeline as [`assemble`], additionally consulting/populating a facts cache (RFC 0004
/// §2–4, ADR 0004): a file whose content hash already has a cached-and-current entry skips
/// re-parsing entirely, which is the warm path's dominant win since parsing dominates cold-run
/// cost (spike 0001). `cache: None` is exactly [`assemble`]'s behavior — this must hold
/// byte-for-byte, since `--no-cache` ≡ cached results is an RFC 0004 §4 correctness gate.
pub fn assemble_with_cache(
    root: &Path,
    adapters: &[Box<dyn LanguageAdapter>],
    plugins: &[Box<dyn crate::plugin::Plugin>],
    cache: Option<&crate::cache::ProjectCache>,
) -> Result<(ProjectGraph, Vec<Diagnostic>), DiscoveryError> {
    let assembled = assemble_from_source(
        &discovery::TreeSource::Directory(root),
        adapters,
        plugins,
        cache,
    )?;
    // This convenience entry point persists inline — only the engine's own path defers the
    // write to a background thread (it owns a place to join it; callers here don't).
    if let Some(writer) = &assembled.pending_snapshot {
        writer.write(&assembled.graph, &assembled.extraction_diagnostics);
    }
    let mut diagnostics = assembled.discovery_diagnostics;
    diagnostics.extend(assembled.extraction_diagnostics);
    Ok((assembled.graph, diagnostics))
}

/// Resolves a plugin-named [`crate::plugin::PluginTarget`] against the same bare/qualified
/// symbol tables phase 3b's own reference resolution reads (`graph.rs` §4 of the plugin-wiring
/// investigation) — the exact two-step fallback [`emit_file_declarations`]'s `bare_table`/
/// `qualified_table` already use for `RawRoot`. A path or name that doesn't resolve returns
/// `None`; callers drop it silently, the same miss behavior an adapter's own `RawRoot`/
/// `RawReference` already has.
fn resolve_plugin_target(
    target: &crate::plugin::PluginTarget,
    file_index: &HashMap<ProjectPath, FileId>,
    symbol_by_name_per_file: &[HashMap<SmolStr, SymbolId>],
    symbol_by_qualified_per_file: &[HashMap<String, SymbolId>],
) -> Option<NodeRef> {
    let file_id = *file_index.get(&target.path)?;
    match &target.symbol {
        None => Some(NodeRef::File(file_id)),
        Some(name) => {
            let idx = file_id.0 as usize;
            symbol_by_name_per_file
                .get(idx)
                .and_then(|t| t.get(name))
                .or_else(|| {
                    symbol_by_qualified_per_file
                        .get(idx)
                        .and_then(|t| t.get(name.as_str()))
                })
                .copied()
                .map(NodeRef::Symbol)
        }
    }
}

/// [`assemble_with_cache`] over any [`discovery::TreeSource`] — a directory, or a git tree-ish
/// read in memory (diff modes, RFC 0004 §6). Everything past discovery is source-blind:
/// identical content produces identical facts, hashes, ids, and findings whether the bytes came
/// from disk or the object database.
pub fn assemble_from_source(
    source: &discovery::TreeSource<'_>,
    adapters: &[Box<dyn LanguageAdapter>],
    plugins: &[Box<dyn crate::plugin::Plugin>],
    cache: Option<&crate::cache::ProjectCache>,
) -> Result<AssembledGraph, DiscoveryError> {
    // Only graph-mutating plugins (`Plugin::mutates_graph`) participate in assembly at all —
    // both in the hook call sites below AND in the cache/patch bypass decision. Filtering here,
    // at the single entry point, is what makes the declaration self-enforcing: a plugin
    // claiming `false` never has its hooks called, so it can't be the reason a cached graph
    // is stale. Deterministic call order (RFC 0003 §5) — interim rule pending a real
    // ordering-constraints field on `PluginDescriptor` (docs/rfcs/0003-plugin-system.md §5):
    // sort by id once, reused by every hook site below instead of re-sorting per phase.
    let mut sorted_plugins: Vec<&Box<dyn crate::plugin::Plugin>> =
        plugins.iter().filter(|p| p.mutates_graph()).collect();
    sorted_plugins.sort_by(|a, b| a.descriptor().id.cmp(&b.descriptor().id));
    let sorted_plugins = sorted_plugins.as_slice();

    let mut phase_start = std::time::Instant::now();
    let mut timings: Vec<(&'static str, u64)> = Vec::new();
    let mut tick = |label: &'static str, start: &mut std::time::Instant| {
        timings.push((label, start.elapsed().as_micros() as u64));
        *start = std::time::Instant::now();
    };
    let known_blob_hashes = cache.map(|c| c.load_blob_hashes()).unwrap_or_default();
    let stat_index = cache.and_then(|c| c.load_stat_index());
    tick("sidecar-load", &mut phase_start);
    let mut discovered =
        discovery::discover_source(source, &known_blob_hashes, stat_index.as_ref())?;
    // RFC 0013 §3c: discovery diagnostics are always the fresh walk's — they never enter the
    // snapshot, whose stored diagnostics are extraction + manifest only (the producers warm
    // paths skip). One composition rule for hit, patch, and full alike.
    let mut discovery_diagnostics = std::mem::take(&mut discovered.diagnostics);
    discovery_diagnostics.sort_unstable();
    if let Some(cache) = cache {
        // Persist fresh (git blob → blake3) pairs immediately — the graph-snapshot hit below
        // returns early, and the sidecar must grow even on runs that never reach extraction.
        cache.save_blob_hashes(&discovered.new_blob_hashes);
        // Same for the stat sidecar (RFC 0004 §4 step 2), rewritten wholesale (the current
        // file set IS the index) — but only when something actually changed: on a no-op run
        // every entry matched, and rewriting a 50k-entry sidecar costs more than the stat
        // fast path saves.
        let index_current = stat_index
            .as_ref()
            .is_some_and(|i| i.is_current_for(&discovered.stat_entries));
        if !index_current {
            cache.save_stat_index(&discovered.stat_entries, discovered.stat_written_at_ns);
        }
    }
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // The graph-snapshot fast path (RFC 0004 §2, §4 step 1): if every input the key folds in —
    // the whole discovered file set, each registered adapter's identity/version, and the graph
    // schema itself — matches the last snapshot exactly, skip claim/extract/resolve/link
    // entirely and hand back the persisted graph. Any mismatch (a single changed byte anywhere
    // is enough) is a plain miss; there's no partial reuse yet, only all-or-nothing.
    let graph_key = compute_graph_key(&discovered.files, adapters);
    tick("discovery", &mut phase_start);
    // Graph-mutating plugins bypass BOTH the snapshot-hit and incremental-patch fast paths
    // (RFC 0003 §5: plugin identity should participate in the cache key so upgrading a plugin
    // invalidates exactly what it influenced — not yet implemented, docs/rfcs/0003-plugin-
    // system.md §5). Neither `cache.get_graph` nor `try_patch` re-invokes `contribute_roots`/
    // `contribute_edges`/`annotate_symbols`, so serving either would silently reuse a graph a
    // currently-registered plugin never touched. The condition is `sorted_plugins` (already
    // filtered to `mutates_graph()` plugins), NOT the raw registry: this line originally
    // checked `plugins.is_empty()`, which — with `LcovPlugin` unconditionally registered by
    // `default_plugins()` — was never true in the shipped product, silently disabling the
    // snapshot cache and the incremental patch on every real run. A coverage-only plugin must
    // not cost the fast paths anything.
    if let Some(cache) = cache.filter(|_| sorted_plugins.is_empty()) {
        if let Some((graph, graph_diagnostics)) = cache.get_graph(&graph_key) {
            tick("snapshot-load", &mut phase_start);
            return Ok(AssembledGraph {
                graph,
                discovery_diagnostics,
                extraction_diagnostics: graph_diagnostics,
                pending_snapshot: None,
                timings,
            });
        }
        tick("snapshot-probe", &mut phase_start);
        // RFC 0013 §5: on a key miss, try the incremental patch off the previous snapshot —
        // any guard failure falls through to the full rebuild below, the one fallback.
        if let Some((graph, extraction_diagnostics)) = try_patch(&discovered, adapters, cache) {
            tick("patch", &mut phase_start);
            let pending_snapshot = cache.graph_writer(graph_key);
            return Ok(AssembledGraph {
                graph,
                discovery_diagnostics,
                extraction_diagnostics,
                pending_snapshot,
                timings,
            });
        }
        tick("patch-probe", &mut phase_start);
    }

    let known_files: HashSet<ProjectPath> =
        discovered.files.iter().map(|f| f.path.clone()).collect();

    // Phase 1 — claim + extract, in parallel. rayon's collect preserves input order (the
    // path-sorted order discovery already established), so the FileId assignment in phase 2
    // stays deterministic regardless of which file's extraction happens to finish first
    // (RFC 0008 §4: parallel compute, deterministic reduce). A facts-cache hit/miss changes
    // only *how* `facts` is obtained, never the order or shape of this collection — cached and
    // freshly-extracted facts are indistinguishable to every phase downstream.
    let outcomes: Vec<Result<Option<Claimed>, Diagnostic>> = discovered
        .files
        .par_iter()
        .map(|df| claim_and_extract(df, adapters, cache, &discovered))
        .collect();

    let mut claimed_per_file: Vec<Option<Claimed>> = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        match outcome {
            Ok(c) => claimed_per_file.push(c),
            Err(d) => {
                diagnostics.push(d);
                claimed_per_file.push(None);
            }
        }
    }

    // Phase 1b — claim + extract manifests, in parallel, mirroring phase 1. A manifest never
    // gets a `FileClaim`/language of its own (docs/adapters/js-ts.md §1: "manifests are not
    // claimed") — it contributes `ManifestFacts` through this wholly separate path. Entry-point
    // resolution only needs the known-files index, not declared dependencies (RFC 0011 §5
    // roots are always filesystem-relative, never a bare-package lookup), so a plain `ctx`
    // suffices here — the dependency-augmented one phase 3 needs is built after this collects.
    let manifest_ctx = ResolveCtx::new(&known_files);
    let manifest_outcomes: Vec<Result<Option<(usize, ManifestFacts)>, Diagnostic>> = discovered
        .files
        .par_iter()
        .map(|df| {
            let Some(adapter_index) = adapters.iter().position(|a| a.claim_manifest(&df.path))
            else {
                return Ok(None);
            };
            let content = discovered.read(&df.path).map_err(|e| Diagnostic {
                level: DiagnosticLevel::Warn,
                path: Some(df.path.clone()),
                message: format!("manifest unreadable at extraction time ({e})"),
                span: None,
            })?;
            let source = SourceFile {
                path: &df.path,
                content: &content,
            };
            let facts = adapters[adapter_index].extract_manifest(&source, &manifest_ctx);
            Ok(Some((adapter_index, facts)))
        })
        .collect();

    let mut manifests_per_file: Vec<Option<(usize, ManifestFacts)>> =
        Vec::with_capacity(manifest_outcomes.len());
    for outcome in manifest_outcomes {
        match outcome {
            Ok(m) => manifests_per_file.push(m),
            Err(d) => {
                diagnostics.push(d);
                manifests_per_file.push(None);
            }
        }
    }

    tick("extract", &mut phase_start);

    // Phase 2 — assign FileId (already the discovery-sorted index) and build File nodes.
    let mut files = Vec::with_capacity(discovered.files.len());
    let mut file_index =
        HashMap::with_capacity_and_hasher(discovered.files.len(), Default::default());
    for (i, df) in discovered.files.iter().enumerate() {
        let file_id = FileId(i as u32);
        file_index.insert(df.path.clone(), file_id);
        let (language, class, unit) = match &claimed_per_file[i] {
            Some(c) => {
                // Content-derived origin override (RFC 0012 §7): extraction saw the bytes,
                // claim only saw the path — the content wins on the origin axis. Applied here,
                // before role-derived roots (phase 2.6) and every analysis, so all origin
                // exemptions see the corrected value. Role is never content-corrected.
                let mut class = c.claim.class;
                if let Some(origin) = c.facts.detected_origin {
                    class.origin = origin;
                }
                // Plugin classify_file (RFC 0003 §2): ecosystem convention beats the language
                // default (`*.stories.tsx` -> tooling). Sorted-by-id plugin order (interim
                // determinism rule — RFC 0003 §5's full topological-ordering-constraints field
                // doesn't exist yet, docs/rfcs/0003-plugin-system.md); each plugin sees the
                // prior one's answer, so a later plugin can refine an earlier one's override.
                for plugin in sorted_plugins {
                    if let Some(overridden) = plugin.classify_file(&df.path, class) {
                        class = overridden;
                    }
                }
                (
                    Some(c.claim.language.clone()),
                    Some(class),
                    c.facts.unit.clone(),
                )
            }
            None => (None, None, None),
        };
        let test_spans = match &claimed_per_file[i] {
            Some(c) => {
                let mut spans = c.facts.test_spans.clone();
                spans.sort_unstable(); // canonical order invariant (RFC 0013 §3)
                spans
            }
            None => Vec::new(),
        };
        files.push(FileNode {
            path: df.path.clone(),
            content_hash: df.content_hash,
            language,
            class,
            package: PackageId(0), // patched in phase 2a once ownership is computed
            unit,
            test_spans,
        });
    }

    // Phase 2a — packages and ownership (RFC 0011 §3): one implicit `Package` covering
    // whatever no real manifest's subtree claims (index 0 — "a repo with no manifest at all
    // is one implicit Package" generalizes to "the part of any repo no manifest governs"),
    // plus one `Package` per manifest found. Ownership is nearest-manifest-ancestor, resolved
    // by trying manifest directories deepest-first so a nested manifest shadows its parent.
    let mut packages = vec![PackageNode {
        manifest: None,
        name: None,
        private: false,
        declares_surface: false,
        surface: Vec::new(),
        workspace_entry: None,
        resolves_dependency_usage: true,
    }];
    let mut manifest_package: Vec<Option<PackageId>> = vec![None; manifests_per_file.len()];
    for (i, slot) in manifests_per_file.iter().enumerate() {
        if let Some((adapter_index, facts)) = slot {
            let package_id = PackageId(packages.len() as u32);
            // The declared surface as FileIds (RFC 0011 §4): entries naming files outside the
            // discovered tree (published build artifacts in a source checkout) drop out here —
            // an absent surface file can never be imported in-repo, so nothing is lost.
            let surface = facts
                .resolved_entries
                .iter()
                .filter_map(|(path, _)| file_index.get(path).copied())
                .collect();
            packages.push(PackageNode {
                manifest: Some(files[i].path.clone()),
                name: facts.package_name.clone(),
                private: facts.private,
                declares_surface: facts.declares_surface,
                surface,
                workspace_entry: facts.resolved_entries.first().cloned(),
                resolves_dependency_usage: adapters[*adapter_index]
                    .descriptor()
                    .resolves_dependency_usage,
            });
            manifest_package[i] = Some(package_id);
        }
    }
    let mut manifest_dirs: Vec<(String, PackageId)> = manifest_package
        .iter()
        .enumerate()
        .filter_map(|(i, pkg)| {
            pkg.map(|id| (core_dirname(files[i].path.0.as_str()).to_string(), id))
        })
        .collect();
    manifest_dirs.sort_by_key(|(dir, _)| std::cmp::Reverse(dir.len()));
    for file in files.iter_mut() {
        let file_dir = core_dirname(file.path.0.as_str());
        file.package = manifest_dirs
            .iter()
            .find(|(manifest_dir, _)| package_owns(manifest_dir, file_dir))
            .map(|&(_, id)| id)
            .unwrap_or(PackageId(0));
    }

    // Phase 2.5 — manifest roots and declared dependencies, sequentially in file-discovery
    // order for determinism (same reasoning as phase 3 below). Declared dependencies feed the
    // stdlib-shadowing precedence rule (RFC 0002 §6) that phase 3's resolver calls already
    // implement but, until now, were never handed anything to check against.
    let mut edges = Vec::new();
    let mut declared_dependency_names: HashSet<SmolStr> = HashSet::default();
    let mut declared_dependencies: Vec<DeclaredDependency> = Vec::new();
    let mut script_invoked_dependencies: HashSet<(PackageId, SmolStr)> = HashSet::default();
    // Every file a manifest names as a production root, at that root's own confidence — used
    // after phase 3a to promote the file's *exported* symbols to production roots too (RFC
    // 0011 §5: "Published/library: its public API is a production root — external consumers
    // exist by definition"). Keyed by file, keeping the strongest confidence when more than
    // one manifest field roots the same file (e.g. both `main` and an `exports` leaf).
    let mut library_root_files: HashMap<FileId, Confidence> = HashMap::default();
    for (i, slot) in manifests_per_file.iter().enumerate() {
        let Some((adapter_index, facts)) = slot else {
            continue;
        };
        let provenance = || Provenance::Adapter(adapters[*adapter_index].descriptor().id.clone());
        // Set alongside this manifest's own PackageNode a few lines above — always Some here.
        let package = manifest_package[i].unwrap_or(PackageId(0));
        for dep in &facts.dependencies {
            declared_dependency_names.insert(dep.name.clone());
            declared_dependencies.push(DeclaredDependency {
                package,
                manifest: files[i].path.clone(),
                name: dep.name.clone(),
                version_req: dep.version_req.clone(),
                scope: dep.scope,
            });
        }
        for name in &facts.script_invoked_names {
            script_invoked_dependencies.insert((package, name.clone()));
        }
        for root in &facts.roots {
            // Resolved against `manifest_ctx`'s known-files set, so this must be Some —
            // defensive skip, not a silent contract violation, if not (same stance as the
            // `Resolution::File` lookup in phase 3).
            if let Some(&target) = file_index.get(&root.target) {
                edges.push(Edge {
                    owner: FileId(i as u32),
                    kind: EdgeKind::Root {
                        kind: root.kind,
                        target: NodeRef::File(target),
                    },
                    confidence: root.confidence,
                    source: provenance(),
                    span: None, // manifest-declared root: a marker, no extraction-time span
                });
                if root.kind == crate::vocab::RootKind::Production {
                    let entry = library_root_files.entry(target).or_insert(root.confidence);
                    *entry = (*entry).max(root.confidence);
                }
            }
        }
        for d in &facts.diagnostics {
            diagnostics.push(Diagnostic {
                level: d.level,
                path: Some(files[i].path.clone()),
                message: d.message.clone(),
                span: d.span,
            });
        }
    }

    // Phase 2.55 — test-gated module demotion (the whole-file case of
    // `FileFacts::test_spans`): `#[cfg(test)] mod tests;` puts an entire *file* behind a
    // test gate, which the path-based claim cannot see. A claimed-production file becomes
    // test-role when at least one module-linking import (side-effect import binding a module
    // name — Rust's `mod foo;` / `#[path]`) reaches it from inside a test region and NO
    // module link reaches it from production code. Runs before phase 2.6 so the demoted file
    // gets its Test root and every role consumer downstream sees the corrected value. Patch
    // parity: the per-import "test-gated" bit is part of the surface signature, so any change
    // to the gating declines the patch and the preserved `FileNode::class` stays truthful.
    // Gated on any-test-spans-present: corpora without sub-file tests skip the pass entirely.
    if claimed_per_file
        .iter()
        .flatten()
        .any(|c| !c.facts.test_spans.is_empty())
    {
        let ctx = ResolveCtx::new(&known_files);
        // Per target: (reached from a test region, reached from production).
        let mut links: HashMap<FileId, (bool, bool)> = HashMap::default();
        for (i, slot) in claimed_per_file.iter().enumerate() {
            let Some(c) = slot else { continue };
            for imp in c
                .facts
                .imports
                .iter()
                .filter(|imp| imp.side_effect_only && imp.local_alias.is_some())
            {
                let spec = crate::adapter::ImportSpec {
                    specifier: imp.specifier.clone(),
                    from: files[i].path.clone(),
                };
                let Resolution::File(path, _) = adapters[c.adapter_index].resolve(&spec, &ctx)
                else {
                    continue;
                };
                let Some(&target) = file_index.get(&path) else {
                    continue;
                };
                let entry = links.entry(target).or_insert((false, false));
                if span_in_test_region(&c.facts.test_spans, imp.span) {
                    entry.0 = true;
                } else {
                    entry.1 = true;
                }
            }
        }
        for (target, (from_test, from_production)) in links {
            if from_test && !from_production {
                if let Some(class) = &mut files[target.0 as usize].class {
                    if class.role == crate::vocab::FileRole::Production {
                        class.role = crate::vocab::FileRole::Test;
                    }
                }
            }
        }
    }

    // Phase 2.6 — role-derived roots (RFC 0005 §2, literally): "Test roots — test
    // functions/files (language role detection…)"; "Tooling roots — build/config scripts
    // (webpack.config…)". The adapter's role classification *is* the seed for these two root
    // kinds — the runner/tool that consumes the file lives outside the graph, so the file's
    // existence under the convention is the whole evidence. `Probable`, not certain: a
    // convention names the file, nothing declares it (same reasoning as `exports`-map leaves).
    // Production roots stay manifest/API-driven (phase 2.5) — never role-derived. Reads the
    // *node's* class, not the raw claim — phase 2.55's demotion and RFC 0012 §7's origin
    // override are already applied there.
    let mut role_root_files: HashMap<FileId, crate::vocab::RootKind> = HashMap::default();
    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        let Some(class) = files[i].class else {
            continue;
        };
        let kind = match class.role {
            crate::vocab::FileRole::Test => crate::vocab::RootKind::Test,
            crate::vocab::FileRole::Tooling => crate::vocab::RootKind::Tooling,
            crate::vocab::FileRole::Production => continue,
        };
        let file_id = FileId(i as u32);
        edges.push(Edge {
            owner: file_id,
            kind: EdgeKind::Root {
                kind,
                target: NodeRef::File(file_id),
            },
            confidence: Confidence::Probable,
            source: Provenance::Adapter(adapters[claimed.adapter_index].descriptor().id.clone()),
            span: None, // role-derived root: the convention names the file, nothing spans it
        });
        role_root_files.insert(file_id, kind);
    }

    // Each claimed language's visibility ladder (RFC 0012 §6), off its claiming adapter's
    // descriptor — keyed by claim language (what `FileNode::language` stores), BTreeMap for
    // deterministic order. Only languages with at least one claimed file appear: an unused
    // adapter's ladder is dead data. Built before phase 3 because the member fallback (3b)
    // scopes its candidates by ladder rung.
    let mut ladders: std::collections::BTreeMap<SmolStr, Vec<crate::adapter::VisibilityRung>> =
        std::collections::BTreeMap::new();
    let mut cycle_policies: std::collections::BTreeMap<SmolStr, crate::adapter::CyclePolicy> =
        std::collections::BTreeMap::new();
    for slot in claimed_per_file.iter().flatten() {
        let descriptor = adapters[slot.adapter_index].descriptor();
        ladders
            .entry(slot.claim.language.clone())
            .or_insert(descriptor.visibility_ladder);
        cycle_policies
            .entry(slot.claim.language.clone())
            .or_insert(descriptor.cycle_policy);
    }

    // Phase 3a — symbols (Declares edges) and in-source roots, sequentially in FileId order.
    // Split from imports/references (phase 3b) because resolving a reference or an import
    // binding to *another* file's symbol needs that file's symbol table already built —
    // forward references (file 0 importing from file 5) are the common case, not an edge case,
    // so every file's declarations must exist before any file's imports are resolved.
    let mut symbols = Vec::new();
    let mut function_metrics: Vec<(SymbolId, SymbolMetrics)> = Vec::new();
    let mut symbol_by_name_per_file: Vec<HashMap<SmolStr, SymbolId>> =
        vec![HashMap::default(); claimed_per_file.len()];
    // Package-scoped (not file-scoped) resolution, for languages where it's the ordinary case
    // rather than an edge case (Go's directory-is-the-package visibility unit — see
    // `FileFacts::unit`'s doc). `None` for every file whose adapter doesn't set `unit` (JS/TS
    // today), so this is purely additive: those files never populate or consult these two maps.
    let mut file_unit: Vec<Option<SmolStr>> = vec![None; claimed_per_file.len()];
    let mut patch_meta: Vec<FilePatchMeta> = vec![FilePatchMeta::default(); claimed_per_file.len()];
    let mut symbol_by_name_per_unit: HashMap<SmolStr, HashMap<SmolStr, SymbolId>> =
        HashMap::default();
    // Member declarations (`member_of: Some(..)`, RFC 0012 §3) resolve on a separate track:
    // an unqualified reference must never `certain`-resolve to a member (bare member names
    // collide across owners by construction — `T.get` and `U.get` are both just `get`), so
    // members stay OUT of the exact-name tables above and live here, name → every same-named
    // member project-wide; phase 3b's duck-typed fallback narrows the set per site by each
    // candidate's declared visibility scope (RFC 0012 §6 — replacing the interim same-file/
    // same-unit tiers). Qualified lookup (for `RawRoot` targets naming `Owner.name`) gets its
    // own exact table.
    let mut member_by_name: HashMap<SmolStr, Vec<SymbolId>> = HashMap::default();
    let mut symbol_by_qualified_per_file: Vec<HashMap<String, SymbolId>> =
        vec![HashMap::default(); claimed_per_file.len()];
    // Workspace-member index (RFC 0011 §4): every *named* manifest in the graph, keyed by
    // package name, with its directory and adapter-resolved primary entry — what lets a bare
    // specifier (`@org/ui`) resolve to the sibling's internal files instead of an external
    // dependency. Built from manifest facts, consumed by import resolution — strictly after
    // manifest extraction, so no circularity. Duplicate names keep the first in
    // file-discovery order (deterministic); a repo with two same-named manifests is broken
    // in ways no resolution order fixes.
    let mut workspace_member_index: HashMap<SmolStr, crate::adapter::WorkspaceMember> =
        HashMap::default();
    for (i, slot) in manifests_per_file.iter().enumerate() {
        let Some((_, facts)) = slot else { continue };
        let Some(name) = &facts.package_name else {
            continue;
        };
        workspace_member_index
            .entry(name.clone())
            .or_insert_with(|| crate::adapter::WorkspaceMember {
                dir: SmolStr::new(core_dirname(files[i].path.0.as_str())),
                entry: facts.resolved_entries.first().cloned(),
            });
    }

    // Unit reverse-index (Java, docs/adapters/java.md §3) — see try_patch's identical
    // construction for why this mirrors the patch path byte-for-byte.
    let mut unit_index: HashMap<SmolStr, Vec<ProjectPath>> = HashMap::default();
    for f in &files {
        if let Some(u) = &f.unit {
            unit_index
                .entry(u.clone())
                .or_default()
                .push(f.path.clone());
        }
    }
    for fs in unit_index.values_mut() {
        fs.sort();
    }
    let ctx = ResolveCtx::new(&known_files)
        .with_declared_dependencies(&declared_dependency_names)
        .with_workspace_members(&workspace_member_index)
        .with_units(&unit_index);

    // Phase 2.7 — library-surface expansion (completing RFC 0011 §5's library mode): a
    // package-surface file's *whole-surface* re-exports — `pub mod x;` in Rust, `export *
    // from './x'` in a published JS package: `reexported` with no named bindings — extend the
    // surface into the target file, transitively to a fixpoint. Each expansion emits a
    // production Root edge for the target file, owned by the re-exporting file: reachability
    // consumes it directly, pass B's export promotion picks the target up from
    // `library_root_files` exactly like a manifest-named root, and the incremental patch
    // (RFC 0013) re-derives membership from the kept edges. Without this, any library whose
    // API lives behind a public module tree — every real Rust crate — reads as dead.
    {
        let mut work: Vec<FileId> = {
            let mut v: Vec<FileId> = library_root_files.keys().copied().collect();
            v.sort();
            v
        };
        while let Some(f) = work.pop() {
            let Some(claimed) = &claimed_per_file[f.0 as usize] else {
                continue;
            };
            let confidence = library_root_files[&f];
            let adapter = &adapters[claimed.adapter_index];
            for imp in claimed
                .facts
                .imports
                .iter()
                .filter(|i| i.reexported && i.bindings.is_empty())
            {
                let spec = ImportSpec {
                    specifier: imp.specifier.clone(),
                    from: files[f.0 as usize].path.clone(),
                };
                let target_path = match adapter.resolve(&spec, &ctx) {
                    Resolution::File(path, _) => path,
                    Resolution::WorkspaceMember { target, .. } => target,
                    _ => continue,
                };
                let Some(&target) = file_index.get(&target_path) else {
                    continue;
                };
                edges.push(Edge {
                    kind: EdgeKind::Root {
                        kind: crate::vocab::RootKind::Production,
                        target: NodeRef::File(target),
                    },
                    confidence,
                    source: Provenance::Adapter(adapter.descriptor().id.clone()),
                    span: Some(imp.span),
                    owner: f,
                });
                use std::collections::hash_map::Entry;
                match library_root_files.entry(target) {
                    Entry::Vacant(slot) => {
                        slot.insert(confidence);
                        work.push(target);
                    }
                    Entry::Occupied(mut slot) => {
                        let merged = (*slot.get()).max(confidence);
                        slot.insert(merged);
                    }
                }
            }
        }
    }

    // Pass A — tables + symbol nodes, sequentially in FileId order (SymbolId assignment is
    // order itself). Emissions (Declares edges, promotions, in-source roots, metrics) moved
    // to pass B below so the *same* emitter serves the full build and the incremental patch
    // (RFC 0013 §5 — one source of truth, no drift between paths).
    let mut symbol_range_per_file: Vec<(u32, u32)> = vec![(0, 0); claimed_per_file.len()];
    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        file_unit[i] = claimed.facts.unit.clone();
        let start = symbols.len() as u32;

        for decl in &claimed.facts.declarations {
            let symbol_id = SymbolId(symbols.len() as u32);
            match &decl.member_of {
                None => {
                    symbol_by_name_per_file[i].insert(decl.name.clone(), symbol_id);
                    if let Some(unit) = &claimed.facts.unit {
                        symbol_by_name_per_unit
                            .entry(unit.clone())
                            .or_default()
                            .insert(decl.name.clone(), symbol_id);
                    }
                }
                Some(owner) => {
                    member_by_name
                        .entry(decl.name.clone())
                        .or_default()
                        .push(symbol_id);
                    symbol_by_qualified_per_file[i]
                        .insert(format!("{owner}.{}", decl.name), symbol_id);
                }
            }
            symbols.push(SymbolNode {
                file: FileId(i as u32),
                name: decl.name.clone(),
                kind: decl.kind.clone(),
                span: decl.span,
                exported: decl.exported,
                visibility: decl.visibility,
                member_of: decl.member_of.clone(),
                signature_span: decl.signature_span,
            });
        }
        symbol_range_per_file[i] = (start, symbols.len() as u32);
    }

    // Pass B — per-file declaration emissions, via the shared emitter (also the patch's).
    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        let out = emit_file_declarations(
            i,
            &claimed.facts,
            adapters[claimed.adapter_index].descriptor().id.as_str(),
            symbol_range_per_file[i].0,
            &symbols,
            &symbol_by_name_per_file[i],
            &symbol_by_qualified_per_file[i],
            &library_root_files,
            &role_root_files,
        );
        edges.extend(out.edges);
        function_metrics.extend(out.metrics);
    }

    // Phase 3a-bis — re-export aliasing (`export {a} from './b'`, `export type {a} from
    // './b'`): a barrel's re-exported bindings become resolvable as *its own* exports too, not
    // merely usable inside it (js-ts.md §5: "Barrel files… resolved through, transparently").
    // Must run for every file before phase 3b resolves any file's import bindings — the same
    // forward-reference reasoning as the 3a/3b split, one level deeper.
    //
    // Resolved to a **fixpoint** (RFC 0013 §3b): rounds over every unresolved re-export
    // binding in (file, import, binding) order until a round makes no progress. The result is
    // the least fixpoint — order-independent, so barrels chaining through other barrels
    // resolve regardless of discovery order (the multi-hop increment js-ts.md promised), and
    // a re-export cycle simply never resolves (no progress ⇒ termination). Collision rule,
    // deliberate: a name already present in a file's table — its own declaration, or an
    // earlier-in-order alias — wins over a later alias (`or_insert` semantics; the previous
    // single-pass code let a re-export stomp a same-named own declaration, which was an
    // artifact, not a design).
    struct PendingReexport {
        source_file: usize,
        target: FileId,
        exported_name: SmolStr,
        local: SmolStr,
        span: crate::adapter::Span,
        adapter_index: usize,
    }
    let mut pending: Vec<PendingReexport> = Vec::new();
    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        let adapter = &adapters[claimed.adapter_index];
        for imp in claimed.facts.imports.iter().filter(|imp| imp.reexported) {
            let spec = ImportSpec {
                specifier: imp.specifier.clone(),
                from: files[i].path.clone(),
            };
            // A re-export resolves through to a file target whether the specifier was
            // relative (`./b`) or a workspace-member name (`@org/ui`) — the aliasing works
            // off the concrete target file either way. (The member's ImportsDependency side
            // is phase 3b's job when it re-resolves this same import.)
            let target = match adapter.resolve(&spec, &ctx) {
                Resolution::File(path, _) => path,
                Resolution::WorkspaceMember { target, .. } => target,
                _ => continue,
            };
            let Some(&target) = file_index.get(&target) else {
                continue;
            };
            for binding in &imp.bindings {
                pending.push(PendingReexport {
                    source_file: i,
                    target,
                    exported_name: binding
                        .imported
                        .clone()
                        .unwrap_or_else(|| SmolStr::new("default")),
                    local: binding.local.clone(),
                    span: imp.span,
                    adapter_index: claimed.adapter_index,
                });
            }
        }
    }
    let mut resolved_reexport: Vec<bool> = vec![false; pending.len()];
    loop {
        let mut progress = false;
        for (b, reexport) in pending.iter().enumerate() {
            if resolved_reexport[b] {
                continue;
            }
            let Some(&original_symbol) =
                symbol_by_name_per_file[reexport.target.0 as usize].get(&reexport.exported_name)
            else {
                continue; // maybe next round, once the target's own aliases resolve
            };
            resolved_reexport[b] = true;
            progress = true;
            let i = reexport.source_file;
            if let std::collections::hash_map::Entry::Vacant(slot) =
                symbol_by_name_per_file[i].entry(reexport.local.clone())
            {
                slot.insert(original_symbol);
                patch_meta[i].reexport_aliases.push(AliasEntry {
                    name: reexport.local.clone(),
                    symbol: original_symbol,
                });
                // The barrel itself is a manifest-declared production root, so everything it
                // re-exports is part of the package's public API too (RFC 0011 §5) — same
                // promotion phase 3a already applies to the barrel's *own* declarations,
                // extended through re-export indirection.
                if let Some(&confidence) = library_root_files.get(&FileId(i as u32)) {
                    edges.push(Edge {
                        owner: FileId(reexport.source_file as u32),
                        kind: EdgeKind::Root {
                            kind: crate::vocab::RootKind::Production,
                            target: NodeRef::Symbol(original_symbol),
                        },
                        confidence,
                        source: Provenance::Adapter(
                            adapters[reexport.adapter_index].descriptor().id.clone(),
                        ),
                        span: Some(reexport.span),
                    });
                }
            }
        }
        if !progress {
            break;
        }
    }

    // Every file's declared unit name (RFC 0012 §9 qualifier defaults) as a plain slice —
    // resolve_file consumes this instead of reaching into other files' facts, which is what
    // lets the incremental patch feed it from the snapshot (RFC 0013 §4).
    let unit_name_by_file: Vec<Option<SmolStr>> = claimed_per_file
        .iter()
        .map(|s| s.as_ref().and_then(|c| c.facts.unit_name.clone()))
        .collect();

    // Phase 3b — imports, import-bindings, references, and diagnostics. Every file's symbol
    // table is complete now (phase 3a), so cross-file lookups are safe regardless of
    // discovery order — which is also what makes this phase embarrassingly parallel
    // (RFC 0008 §2: "per-import resolution ... resolved concurrently"): every table it reads
    // is immutable by now, and each file's contributions collect into a private
    // `ResolvedFile` merged below in FileId order (§4: parallel compute, deterministic
    // reduce). `DependencyId` assignment stays in the sequential merge — ids are
    // first-appearance-in-file-order, exactly as the sequential loop assigned them.
    let mut dependencies = Vec::new();
    let mut dep_index: HashMap<SmolStr, DependencyId> = HashMap::default();
    let mut suppressions: Vec<(FileId, crate::adapter::RawSuppression)> = Vec::new();

    let tables = ResolveTables {
        files: &files,
        file_index: &file_index,
        symbols: &symbols,
        symbol_by_name_per_file: &symbol_by_name_per_file,
        symbol_by_qualified_per_file: &symbol_by_qualified_per_file,
        symbol_by_name_per_unit: &symbol_by_name_per_unit,
        member_by_name: &member_by_name,
        file_unit: &file_unit,
        unit_name_by_file: &unit_name_by_file,
        ladders: &ladders,
        ctx: &ctx,
    };
    let resolved_files: Vec<Option<ResolvedFile>> = claimed_per_file
        .par_iter()
        .enumerate()
        .map(|(i, slot)| {
            let claimed = slot.as_ref()?;
            Some(resolve_file(
                i,
                &claimed.facts,
                &*adapters[claimed.adapter_index],
                &tables,
            ))
        })
        .collect();

    for resolved in resolved_files.into_iter().flatten() {
        edges.extend(resolved.edges);
        for (name, confidence, span, from, source) in resolved.dep_imports {
            let to = *dep_index.entry(name.clone()).or_insert_with(|| {
                let id = DependencyId(dependencies.len() as u32);
                dependencies.push(DependencyNode { name: name.clone() });
                id
            });
            edges.push(Edge {
                owner: from,
                kind: EdgeKind::ImportsDependency { from, to },
                confidence,
                source,
                span: Some(span),
            });
        }
        diagnostics.extend(resolved.diagnostics);
        suppressions.extend(resolved.suppressions);
    }

    // Plugin graph-mutation hooks (RFC 0003 §2): contribute_roots/contribute_edges/
    // annotate_symbols run once, right here — Pass A's symbol tables and every file's
    // references (phase 3b, just merged above) are both stable, and nothing downstream (the
    // canonical sort, `ProjectGraph` construction) has run yet, so a plugin's target-by-name
    // lookups see the real, final graph and its contributions fold into the one sort below
    // rather than needing a second pass. Skipped whole when no plugin is registered — the
    // `GraphView` index build is one more O(symbols) pass, negligible next to Pass A's own, but
    // there's no reason to pay it for the common zero-plugin-with-graph-hooks case.
    let mut externally_consumed: Vec<SymbolId> = Vec::new();
    if !sorted_plugins.is_empty() {
        let view = crate::plugin::GraphView::new(&files, &symbols, &file_index);
        for plugin in sorted_plugins {
            let provenance = crate::vocab::Provenance::Plugin(plugin.descriptor().id);

            let mut root_sink = crate::plugin::RootSink::default();
            plugin.contribute_roots(&view, &mut root_sink);
            for root in root_sink.items {
                let Some(target) = resolve_plugin_target(
                    &root.target,
                    &file_index,
                    &symbol_by_name_per_file,
                    &symbol_by_qualified_per_file,
                ) else {
                    continue;
                };
                let owner = match target {
                    NodeRef::File(f) => f,
                    NodeRef::Symbol(s) => symbols[s.0 as usize].file,
                };
                edges.push(Edge {
                    kind: EdgeKind::Root {
                        kind: root.kind,
                        target,
                    },
                    confidence: root.confidence,
                    source: provenance.clone(),
                    span: None,
                    owner,
                });
            }

            let mut edge_sink = crate::plugin::EdgeSink::default();
            plugin.contribute_edges(&view, &mut edge_sink);
            for contributed in edge_sink.items {
                let Some(from) = resolve_plugin_target(
                    &contributed.from,
                    &file_index,
                    &symbol_by_name_per_file,
                    &symbol_by_qualified_per_file,
                ) else {
                    continue;
                };
                // References always targets a symbol — a `to` naming a whole file (no `symbol`
                // set) isn't a resolvable reference target and is dropped, same as a miss.
                let Some(NodeRef::Symbol(to)) = resolve_plugin_target(
                    &contributed.to,
                    &file_index,
                    &symbol_by_name_per_file,
                    &symbol_by_qualified_per_file,
                ) else {
                    continue;
                };
                let owner = match from {
                    NodeRef::File(f) => f,
                    NodeRef::Symbol(s) => symbols[s.0 as usize].file,
                };
                edges.push(Edge {
                    kind: EdgeKind::References {
                        from,
                        to,
                        kind: contributed.kind,
                    },
                    confidence: contributed.confidence,
                    source: provenance.clone(),
                    span: None,
                    owner,
                });
            }

            let mut annotation_sink = crate::plugin::AnnotationSink::default();
            plugin.annotate_symbols(&view, &mut annotation_sink);
            for target in annotation_sink.externally_consumed {
                if let Some(NodeRef::Symbol(id)) = resolve_plugin_target(
                    &target,
                    &file_index,
                    &symbol_by_name_per_file,
                    &symbol_by_qualified_per_file,
                ) {
                    externally_consumed.push(id);
                }
            }
        }
        externally_consumed.sort_unstable();
        externally_consumed.dedup();
    }

    // Canonical order (RFC 0013 §3a): edge and diagnostic order is *data*, not construction
    // history. Two semantically identical graphs must be identical vectors — the property the
    // patched ≡ full-rebuild gate compares, and the property that keeps tie-breaks (e.g.
    // cyclic's strongest-edge-per-pair evidence pick) independent of which assembly path or
    // parallel schedule produced the graph. One total comparator, derived field order.
    edges.sort_unstable();
    diagnostics.sort_unstable();
    // Same canonical-order rule for the remaining order-bearing vectors (RFC 0013 §3a):
    // stable sorts, so same-key entries keep facts order — identical on both build paths.
    function_metrics.sort_by_key(|(id, _)| *id);
    suppressions.sort_by_key(|(f, _)| *f);

    // RFC 0013 §4: per-file patch metadata — surface signatures and unit names from phase
    // 1's facts; the re-export aliases were recorded by the 3a-bis fixpoint above.
    for (i, slot) in claimed_per_file.iter().enumerate() {
        if let Some(claimed) = slot {
            patch_meta[i].surface_sig = Some(claimed.surface_sig);
            patch_meta[i].unit_name = claimed.facts.unit_name.clone();
        }
    }

    let graph = ProjectGraph {
        files,
        symbols,
        dependencies,
        declared_dependencies,
        script_invoked_dependencies,
        packages,
        edges,
        suppressions,
        visibility_ladders: ladders.into_iter().collect(),
        cycle_policies: cycle_policies.into_iter().collect(),
        function_metrics,
        patch_meta,
        externally_consumed,
        file_index,
    };
    // The snapshot is NOT written here (RFC 0008 §2: cache persist happens off the critical
    // path) — the freshly assembled graph hands back the key, and the engine defers the
    // serialize + write to a background thread that overlaps with analysis and rendering.
    // Skipped entirely when graph-mutating plugins are registered (see the read-side comment
    // above, including why the condition is the filtered set and not the raw registry): writing
    // a plugin-influenced graph under a key that carries no plugin identity would let a *later*,
    // plugin-less run read it back and silently inherit contributions no plugin made for it.
    let pending_snapshot = cache
        .filter(|_| sorted_plugins.is_empty())
        .and_then(|c| c.graph_writer(graph_key));
    tick("resolve+link", &mut phase_start);
    Ok(AssembledGraph {
        graph,
        discovery_diagnostics,
        extraction_diagnostics: diagnostics,
        pending_snapshot,
        timings,
    })
}

/// [`assemble_from_source`]'s result: the graph, assembly-time diagnostics (exactly what a
/// snapshot stores and a warm hit replays), and — on a snapshot miss with a writable cache —
/// the deferred writer the engine schedules off the critical path.
pub struct AssembledGraph {
    pub graph: ProjectGraph,
    /// Always the fresh walk's (RFC 0013 §3c) — never stored, never replayed.
    pub discovery_diagnostics: Vec<Diagnostic>,
    /// Extraction + manifest diagnostics — what the snapshot stores and warm paths replay
    /// (their producers were skipped). Canonically sorted, like the edges.
    pub extraction_diagnostics: Vec<Diagnostic>,
    pub pending_snapshot: Option<crate::cache::GraphSnapshotWriter>,
    /// Assembly sub-phase wall times `(phase, µs)` — merged into `RunResult::timings` so
    /// `--verbose` shows where assembly goes (discovery+hash, snapshot load, extract incl.
    /// facts-cache fetches, resolve+link). Empty on the snapshot fast path except its two
    /// entries.
    pub timings: Vec<(&'static str, u64)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{
        AdapterDescriptor, Declaration, FileFacts, ImportBinding, ImportKind, ManifestDependency,
        ManifestFacts, ManifestRoot, RawImport, RawReference, RawRoot, RawRootTarget,
    };
    use crate::vocab::{DependencyScope, FileOrigin, FileRole, RefKind, RootKind};
    use std::fs;

    /// A minimal in-memory adapter for graph tests. kndo-core must never depend on a real
    /// language adapter (that would invert the ignorance rule, RFC 0001 §2) — even in tests.
    struct MockAdapter;

    impl LanguageAdapter for MockAdapter {
        fn descriptor(&self) -> AdapterDescriptor {
            AdapterDescriptor {
                id: SmolStr::new("mock"),
                facts_schema_version: 1,
                file_globs: vec![SmolStr::new("**/*.mock")],
                manifest_globs: vec![],
                grammar_version: SmolStr::new("n/a"),
                visibility_ladder: vec![
                    crate::adapter::VisibilityRung {
                        scope: crate::adapter::VisibilityScope::Unit,
                        label: SmolStr::new("private"),
                    },
                    crate::adapter::VisibilityRung {
                        scope: crate::adapter::VisibilityScope::Public,
                        label: SmolStr::new("exported"),
                    },
                ],
                cycle_policy: crate::adapter::CyclePolicy {
                    file_cycles: crate::adapter::CycleTolerance::Hazard,
                    package_cycles: crate::adapter::CycleTolerance::Hazard,
                },
                resolves_dependency_usage: true,
            }
        }

        fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
            path.0.ends_with(".mock").then(|| {
                // Role by filename convention, mirroring real adapters' classification:
                // "*.test.*" → Test, "*.config.*" → Tooling, everything else Production.
                let role = if path.0.contains(".test.") {
                    FileRole::Test
                } else if path.0.contains(".config.") {
                    FileRole::Tooling
                } else {
                    FileRole::Production
                };
                FileClaim {
                    language: SmolStr::new("mock"),
                    class: FileClass {
                        role,
                        origin: FileOrigin::Authored,
                    },
                }
            })
        }

        fn claim_manifest(&self, path: &ProjectPath) -> bool {
            path.0.rsplit('/').next() == Some("manifest.json")
        }

        fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
            // Content format for the mock: one directive per line.
            //   decl <name>                 -> an exported Function declaration
            //   private-decl <name>         -> an unexported Function declaration
            //   import <specifier> [binding[,binding...]]     -> RawImport { reexported: false }
            //   reexport <specifier> [binding[,binding...]]   -> RawImport { reexported: true }
            //   import-opaque <specifier>                     -> RawImport { opaque_namespace_use: true }
            //       binding := name          -> ImportBinding { local: name, imported: Some(name) }
            //                | local=imported -> ImportBinding { local, imported: Some(imported) }
            //                | local=          -> ImportBinding { local, imported: None } (default)
            //   ref <name>                  -> a RawReference to that name
            //   root-file                   -> a Production root targeting this whole file
            //   root-decl <name>            -> a Production root targeting the named declaration
            //   dynamic                     -> an un-narrowed DynamicUse (eval-style)
            //   dynamic-narrowed <dir>      -> a DynamicUse narrowed to that project dir
            //   suppress <category>         -> a Declaration-scope RawSuppression
            //   unit <key>                  -> FileFacts::unit (package-scoped resolution)
            //   detected-generated          -> FileFacts::detected_origin = Generated (§7)
            let text = std::str::from_utf8(file.content).unwrap_or("");
            let mut facts = FileFacts::default();
            for line in text.lines() {
                if let Some(key) = line.strip_prefix("unit ") {
                    facts.unit = Some(SmolStr::new(key));
                } else if line == "detected-generated" {
                    // Content-derived origin override (RFC 0012 §7).
                    facts.detected_origin = Some(FileOrigin::Generated);
                } else if let Some(name) = line.strip_prefix("decl ") {
                    facts.declarations.push(Declaration {
                        name: SmolStr::new(name),
                        kind: SymbolKind::Function,
                        span: Span::default(),
                        exported: true,
                        visibility: VisibilityLevel(1),
                        member_of: None,
                        signature_span: None,
                    });
                } else if let Some(name) = line.strip_prefix("private-decl ") {
                    facts.declarations.push(Declaration {
                        name: SmolStr::new(name),
                        kind: SymbolKind::Function,
                        span: Span::default(),
                        exported: false,
                        visibility: VisibilityLevel(0),
                        member_of: None,
                        signature_span: None,
                    });
                } else if let Some(rest) = line
                    .strip_prefix("member-decl ")
                    .or_else(|| line.strip_prefix("member-decl-exported "))
                {
                    // `member-decl <owner> <name>` — an unexported member declaration
                    // (RFC 0012 §3): bare name, structured owner. The `-exported` variant
                    // declares at ladder level 1 (`Public` on the mock ladder) for the
                    // fallback's visibility-scoped candidacy (RFC 0012 §6).
                    let exported = line.starts_with("member-decl-exported ");
                    let mut parts = rest.splitn(2, ' ');
                    let owner = parts.next().unwrap_or("");
                    let name = parts.next().unwrap_or("");
                    facts.declarations.push(Declaration {
                        name: SmolStr::new(name),
                        kind: SymbolKind::Method,
                        span: Span::default(),
                        exported,
                        visibility: VisibilityLevel(exported as u8),
                        member_of: Some(SmolStr::new(owner)),
                        signature_span: None,
                    });
                } else if let Some(rest) = line
                    .strip_prefix("import ")
                    .or_else(|| line.strip_prefix("reexport "))
                    .or_else(|| line.strip_prefix("import-opaque "))
                {
                    let reexported = line.starts_with("reexport ");
                    let opaque_namespace_use = line.starts_with("import-opaque ");
                    let mut parts = rest.splitn(2, ' ');
                    let spec = parts.next().unwrap_or("");
                    let bindings = parts
                        .next()
                        .map(|tokens| {
                            tokens
                                .split(',')
                                .map(|tok| match tok.split_once('=') {
                                    Some((local, imported)) => ImportBinding {
                                        local: SmolStr::new(local),
                                        imported: (!imported.is_empty())
                                            .then(|| SmolStr::new(imported)),
                                    },
                                    None => ImportBinding {
                                        local: SmolStr::new(tok),
                                        imported: Some(SmolStr::new(tok)),
                                    },
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    facts.imports.push(RawImport {
                        specifier: SmolStr::new(spec),
                        kind: ImportKind::Relative,
                        span: Span::default(),
                        side_effect_only: false,
                        type_only: false,
                        confidence: Confidence::Certain,
                        bindings,
                        reexported,
                        opaque_namespace_use,
                        local_alias: None,
                    });
                } else if let Some(rest) = line.strip_prefix("import-as ") {
                    // `import-as <alias> <specifier>` — an explicitly-aliased namespace
                    // import (RFC 0012 §9's `local_alias`).
                    let mut parts = rest.splitn(2, ' ');
                    let alias = parts.next().unwrap_or("");
                    let spec = parts.next().unwrap_or("");
                    facts.imports.push(RawImport {
                        specifier: SmolStr::new(spec),
                        kind: ImportKind::Relative,
                        span: Span::default(),
                        side_effect_only: false,
                        type_only: false,
                        confidence: Confidence::Certain,
                        bindings: Vec::new(),
                        reexported: false,
                        opaque_namespace_use: false,
                        local_alias: Some(SmolStr::new(alias)),
                    });
                } else if let Some(name) = line.strip_prefix("unit-name ") {
                    // The name importers bind this unit by (RFC 0012 §9).
                    facts.unit_name = Some(SmolStr::new(name));
                } else if let Some(rest) = line.strip_prefix("qref ") {
                    // `qref <qualifier> <name>` — a qualified reference
                    // (RFC 0012 §9's `scope_context`).
                    let mut parts = rest.splitn(2, ' ');
                    let qualifier = parts.next().unwrap_or("");
                    let name = parts.next().unwrap_or("");
                    facts.references.push(RawReference {
                        name: SmolStr::new(name),
                        scope_context: Some(SmolStr::new(qualifier)),
                        span: Span::default(),
                        within: None,
                        kind: RefKind::Read,
                    });
                } else if let Some(rest) = line.strip_prefix("ref-in ") {
                    // `ref-in <within> <name>` — a reference executing inside the named
                    // declaration (RFC 0012 §4 attribution).
                    let mut parts = rest.splitn(2, ' ');
                    let within = parts.next().unwrap_or("");
                    let name = parts.next().unwrap_or("");
                    facts.references.push(RawReference {
                        name: SmolStr::new(name),
                        scope_context: None,
                        span: Span::default(),
                        within: Some(SmolStr::new(within)),
                        kind: RefKind::Read,
                    });
                } else if let Some(name) = line.strip_prefix("ref ") {
                    facts.references.push(RawReference {
                        name: SmolStr::new(name),
                        scope_context: None,
                        span: Span::default(),
                        within: None,
                        kind: RefKind::Read,
                    });
                } else if line == "root-file" {
                    facts.roots.push(RawRoot {
                        kind: RootKind::Production,
                        target: RawRootTarget::WholeFile,
                        confidence: Confidence::Certain,
                    });
                } else if let Some(name) = line.strip_prefix("root-decl ") {
                    facts.roots.push(RawRoot {
                        kind: RootKind::Production,
                        target: RawRootTarget::Declaration(SmolStr::new(name)),
                        confidence: Confidence::Certain,
                    });
                } else if let Some(dir) = line.strip_prefix("dynamic-narrowed ") {
                    facts.dynamics.push(crate::adapter::DynamicUse {
                        span: Span::default(),
                        reason: SmolStr::new("mock dynamic"),
                        narrowed_to: Some(SmolStr::new(dir)),
                    });
                } else if line == "dynamic" {
                    facts.dynamics.push(crate::adapter::DynamicUse {
                        span: Span::default(),
                        reason: SmolStr::new("mock dynamic"),
                        narrowed_to: None,
                    });
                } else if let Some(category) = line.strip_prefix("suppress ") {
                    facts.suppressions.push(crate::adapter::RawSuppression {
                        span: Span::default(),
                        category: SmolStr::new(category),
                        subject: None,
                        reason: None,
                        scope: crate::adapter::SuppressionScope::Declaration,
                    });
                } else if let Some(rest) = line.strip_prefix("test-region ") {
                    // `test-region <start-line> <end-line>` — a sub-file test region
                    // (`FileFacts::test_spans`).
                    let mut parts = rest.splitn(2, ' ');
                    let a: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let b: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(a);
                    facts.test_spans.push(Span {
                        start: (a, 1),
                        end: (b, 999),
                    });
                } else if let Some(rest) = line.strip_prefix("mod-link ") {
                    // `mod-link <specifier> <line>` — a module-linking side-effect import
                    // (Rust's `mod x;` shape: side_effect_only + local_alias), sited at the
                    // given line so test-region containment is exercisable.
                    let mut parts = rest.splitn(2, ' ');
                    let spec = parts.next().unwrap_or("");
                    let line_no: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    facts.imports.push(RawImport {
                        specifier: SmolStr::new(spec),
                        kind: ImportKind::Relative,
                        span: Span {
                            start: (line_no, 1),
                            end: (line_no, 10),
                        },
                        side_effect_only: true,
                        type_only: false,
                        confidence: Confidence::Certain,
                        bindings: Vec::new(),
                        reexported: false,
                        opaque_namespace_use: false,
                        local_alias: Some(SmolStr::new(spec.rsplit('/').next().unwrap_or(spec))),
                    });
                } else if let Some(rest) = line.strip_prefix("decl-at ") {
                    // `decl-at <line> <name>` — an exported declaration spanning that line
                    // (for test-region containment: derived Test roots, crap/health skips).
                    let mut parts = rest.splitn(2, ' ');
                    let line_no: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let name = parts.next().unwrap_or("");
                    facts.declarations.push(Declaration {
                        name: SmolStr::new(name),
                        kind: SymbolKind::Function,
                        span: Span {
                            start: (line_no, 1),
                            end: (line_no, 50),
                        },
                        exported: true,
                        visibility: VisibilityLevel(1),
                        member_of: None,
                        signature_span: None,
                    });
                } else if let Some(rest) = line.strip_prefix("import-at ") {
                    // `import-at <line> <specifier>` — a plain import sited at a line (for
                    // dependency-hygiene's test-region site role).
                    let mut parts = rest.splitn(2, ' ');
                    let line_no: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let spec = parts.next().unwrap_or("");
                    facts.imports.push(RawImport {
                        specifier: SmolStr::new(spec),
                        kind: ImportKind::Relative,
                        span: Span {
                            start: (line_no, 1),
                            end: (line_no, 10),
                        },
                        side_effect_only: false,
                        type_only: false,
                        confidence: Confidence::Certain,
                        bindings: Vec::new(),
                        reexported: false,
                        opaque_namespace_use: false,
                        local_alias: None,
                    });
                }
            }
            facts
        }

        fn extract_manifest(&self, file: &SourceFile<'_>, ctx: &ResolveCtx<'_>) -> ManifestFacts {
            // Content format for the mock manifest: one directive per line.
            //   dep <name>        -> a prod-scope declared dependency
            //   root <path>       -> a Production root targeting that known file, if it exists
            //   cli-invoke <name> -> a script-invoked dependency name
            //   declares-surface  -> ManifestFacts::declares_surface = true (RFC 0011 §4)
            //   private           -> ManifestFacts::private = true (app mode)
            //   name <pkg>        -> the package's declared name
            //   entry <path>      -> a resolved entry (what a sibling's bare-name import lands on)
            let text = std::str::from_utf8(file.content).unwrap_or("");
            let mut facts = ManifestFacts::default();
            for line in text.lines() {
                if let Some(name) = line.strip_prefix("dep ") {
                    facts.dependencies.push(ManifestDependency {
                        name: SmolStr::new(name),
                        version_req: SmolStr::new("*"),
                        scope: DependencyScope::Prod,
                    });
                } else if let Some(p) = line.strip_prefix("root ") {
                    let target = ProjectPath(SmolStr::new(p));
                    if ctx.contains(&target) {
                        facts.roots.push(ManifestRoot {
                            kind: RootKind::Production,
                            target,
                            confidence: Confidence::Certain,
                        });
                    }
                } else if let Some(name) = line.strip_prefix("cli-invoke ") {
                    facts.script_invoked_names.push(SmolStr::new(name));
                } else if let Some(name) = line.strip_prefix("name ") {
                    facts.package_name = Some(SmolStr::new(name));
                } else if let Some(p) = line.strip_prefix("entry ") {
                    let target = ProjectPath(SmolStr::new(p));
                    if ctx.contains(&target) {
                        facts.resolved_entries.push((target, Confidence::Certain));
                    }
                } else if line == "declares-surface" {
                    // Explicit entry-point surface (`exports` map equivalent) — the
                    // deep-import contract gate (RFC 0011 §4).
                    facts.declares_surface = true;
                } else if line == "private" {
                    facts.private = true;
                }
            }
            facts
        }

        fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
            // Trivial resolver: relative specifiers are exact sibling filenames; anything
            // else is a bare dependency name.
            if let Some(rel) = spec.specifier.strip_prefix("./") {
                let dir = spec.from.0.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                let candidate = if dir.is_empty() {
                    rel.to_string()
                } else {
                    format!("{dir}/{rel}")
                };
                let path = ProjectPath(SmolStr::new(candidate));
                return if ctx.contains(&path) {
                    Resolution::File(path, Confidence::Certain)
                } else {
                    Resolution::Unresolved
                };
            }
            // Workspace member by name — mirrors the real adapters' precedence (an in-repo
            // name match outranks the external-dependency fallback below).
            if let Some(member) = ctx.workspace_member(&spec.specifier) {
                return match &member.entry {
                    Some((target, confidence)) => Resolution::WorkspaceMember {
                        name: spec.specifier.clone(),
                        target: target.clone(),
                        confidence: *confidence,
                    },
                    None => Resolution::Unresolved,
                };
            }
            // Confidence is deliberately observable here so tests can tell whether the
            // manifest's declared dependencies actually reached this resolver call —
            // otherwise wiring `declared_dependencies` through the pipeline is untestable
            // from graph.rs (the real shadowing rule itself is already covered where it's
            // implemented, kndo-adapter-toolkit's stdlib module).
            let confidence = if ctx.is_declared_dependency(&spec.specifier) {
                Confidence::Certain
            } else {
                Confidence::Probable
            };
            Resolution::Dependency(spec.specifier.clone(), confidence)
        }
    }

    fn project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kndo-graph-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (path, content) in files {
            let full = dir.join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(full, content).unwrap();
        }
        dir
    }

    fn mock_adapters() -> Vec<Box<dyn LanguageAdapter>> {
        vec![Box::new(MockAdapter)]
    }

    #[test]
    fn unclaimed_files_still_become_file_nodes() {
        let dir = project("unclaimed", &[("README.md", "hello")]);
        let (graph, diags) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(diags.is_empty());
        assert_eq!(graph.files.len(), 1);
        assert!(graph.files[0].language.is_none());
    }

    #[test]
    fn declarations_become_symbols_with_declares_edges() {
        let dir = project("decls", &[("a.mock", "decl foo\ndecl bar")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(graph.symbols.len(), 2);
        assert_eq!(graph.symbols[0].name.as_str(), "foo");
        let file_id = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        let declares: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Declares { file, .. } if file == file_id))
            .collect();
        assert_eq!(declares.len(), 2);
    }

    #[test]
    fn suppressions_are_collected_per_file_during_assembly() {
        let dir = project(
            "suppressions",
            &[
                ("a.mock", "decl foo\nsuppress unused"),
                ("b.mock", "decl bar"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        assert_eq!(graph.suppressions.len(), 1);
        assert_eq!(graph.suppressions[0].0, a);
        assert_eq!(graph.suppressions[0].1.category.as_str(), "unused");
    }

    #[test]
    fn relative_import_produces_imports_file_edge() {
        let dir = project(
            "imports-file",
            &[("a.mock", "import ./b.mock"), ("b.mock", "decl target")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        let b = graph.file_id(&ProjectPath(SmolStr::new("b.mock"))).unwrap();
        assert!(graph
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::ImportsFile { from: a, to: b }));
    }

    #[test]
    fn same_unit_files_resolve_each_other_s_symbols_without_any_import() {
        // Go's ordinary case (RFC 0002 §2, FileFacts::unit): two files sharing a package
        // directory call each other's declarations with no import statement at all.
        let dir = project(
            "same-unit",
            &[
                ("pkg/a.mock", "unit pkg\nref target"),
                ("pkg/b.mock", "unit pkg\nprivate-decl target"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a = graph
            .file_id(&ProjectPath(SmolStr::new("pkg/a.mock")))
            .unwrap();
        let target = graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == "target")
            .map(|i| crate::vocab::SymbolId(i as u32))
            .unwrap();
        assert!(graph.edges.iter().any(|e| matches!(
            e.kind,
            EdgeKind::References { from, to, .. } if from == NodeRef::File(a) && to == target
        )));
    }

    #[test]
    fn different_unit_files_do_not_resolve_each_other_s_symbols() {
        let dir = project(
            "different-unit",
            &[
                ("pkg1/a.mock", "unit pkg1\nref target"),
                ("pkg2/b.mock", "unit pkg2\nprivate-decl target"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a = graph
            .file_id(&ProjectPath(SmolStr::new("pkg1/a.mock")))
            .unwrap();
        assert!(!graph.edges.iter().any(
            |e| matches!(e.kind, EdgeKind::References { from, .. } if from == NodeRef::File(a))
        ));
    }

    // -------------------------------------------------- member-call fallback (RFC 0012 §3)

    fn reference_edges_to<'g>(graph: &'g ProjectGraph, name: &str) -> Vec<&'g Edge> {
        let target = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == name)
                .unwrap() as u32,
        );
        graph
            .edges
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::References { to, .. } if to == target))
            .collect()
    }

    #[test]
    fn member_call_resolves_via_fallback_at_probable_with_one_candidate() {
        // The Go bug this exists for: `t.helper()` is a bare `helper` reference; the
        // declaration is a member of T. Exact resolution must miss (members never enter the
        // bare-name table), the duck-typed fallback must hit at Probable (RFC 0002 §5).
        let dir = project(
            "member-fallback-one",
            &[(
                "a.mock",
                "member-decl T helper\ndecl caller\nref helper\nroot-decl caller",
            )],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let edges = reference_edges_to(&graph, "helper");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].confidence, Confidence::Probable);
    }

    #[test]
    fn member_call_with_several_candidates_keeps_all_alive_at_possible() {
        let dir = project(
            "member-fallback-many",
            &[(
                "a.mock",
                "member-decl T get\nmember-decl U get\nref get\nroot-file",
            )],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let t_get = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.member_of.as_deref() == Some("T"))
                .unwrap() as u32,
        );
        let u_get = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.member_of.as_deref() == Some("U"))
                .unwrap() as u32,
        );
        for target in [t_get, u_get] {
            let edge = graph
                .edges
                .iter()
                .find(|e| matches!(e.kind, EdgeKind::References { to, .. } if to == target))
                .expect("every same-named member candidate gets a keep-alive edge");
            assert_eq!(edge.confidence, Confidence::Possible);
        }
    }

    #[test]
    fn member_fallback_reaches_same_unit_siblings() {
        // The cross-file half of the Go bug: the method lives in a sibling file of the same
        // package; the caller has no import and no same-file candidate.
        let dir = project(
            "member-fallback-unit",
            &[
                (
                    "pkg/a.mock",
                    "unit pkg\ndecl caller\nref helper\nroot-decl caller",
                ),
                ("pkg/b.mock", "unit pkg\nmember-decl T helper"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let edges = reference_edges_to(&graph, "helper");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].confidence, Confidence::Probable);
    }

    #[test]
    fn member_never_certain_resolves_and_exact_names_still_win() {
        // A free declaration with the same name as a member: the exact (Certain) resolution
        // wins and the fallback never fires — members must not pollute exact-name lookup.
        let dir = project(
            "member-vs-free",
            &[(
                "a.mock",
                "decl helper\nmember-decl T helper\nref helper\nroot-file",
            )],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let free = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == "helper" && s.member_of.is_none())
                .unwrap() as u32,
        );
        let member = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == "helper" && s.member_of.is_some())
                .unwrap() as u32,
        );
        assert!(graph.edges.iter().any(|e| matches!(
            e.kind, EdgeKind::References { to, .. } if to == free
        ) && e.confidence == Confidence::Certain));
        assert!(!graph
            .edges
            .iter()
            .any(|e| matches!(e.kind, EdgeKind::References { to, .. } if to == member)));
    }

    #[test]
    fn unexported_member_is_not_a_candidate_outside_its_unit() {
        // RFC 0012 §6's visibility-scoped candidacy: a Unit-scoped member (mock ladder level
        // 0) in another unit can't plausibly be the callee — Go's own rule (an unexported
        // method is only legally callable in-package).
        let dir = project(
            "member-scope-unit",
            &[
                (
                    "pkg1/a.mock",
                    "unit pkg1\ndecl caller\nref helper\nroot-decl caller",
                ),
                ("pkg2/b.mock", "unit pkg2\nmember-decl T helper"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(reference_edges_to(&graph, "helper").is_empty());
    }

    #[test]
    fn exported_member_is_a_candidate_project_wide() {
        // The other half: a Public-scoped member (mock ladder level 1) is a candidate for
        // any same-language site, unit boundaries notwithstanding.
        let dir = project(
            "member-scope-public",
            &[
                (
                    "pkg1/a.mock",
                    "unit pkg1\ndecl caller\nref helper\nroot-decl caller",
                ),
                ("pkg2/b.mock", "unit pkg2\nmember-decl-exported T helper"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let edges = reference_edges_to(&graph, "helper");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].confidence, Confidence::Probable);
    }

    // -------------------------------------------------- qualified references (RFC 0012 §9)

    #[test]
    fn aliased_import_qualifier_resolves_inside_the_target() {
        let dir = project(
            "qref-aliased",
            &[
                ("a.mock", "import-as j ./b.mock\nqref j Marshal\nroot-file"),
                ("b.mock", "decl Marshal"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let edges = reference_edges_to(&graph, "Marshal");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].confidence, Confidence::Certain);
    }

    #[test]
    fn unaliased_import_qualifier_comes_from_the_targets_unit_name() {
        // The dir≠package fix (RFC 0012 §9): the import specifier's last segment is
        // "b.mock", but the target declares itself `yaml` — the qualifier the importer
        // actually writes. Resolution must use the target's declared name, not a specifier
        // guess.
        let dir = project(
            "qref-unit-name",
            &[
                ("a.mock", "import ./b.mock\nqref yaml Parse\nroot-file"),
                ("b.mock", "unit-name yaml\ndecl Parse"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let edges = reference_edges_to(&graph, "Parse");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].confidence, Confidence::Certain);
    }

    #[test]
    fn qualified_resolution_reaches_the_targets_unit_siblings() {
        // A Go import names a *package*; the resolved target is one representative file, but
        // the accessed symbol may live in any same-unit sibling.
        let dir = project(
            "qref-unit-sibling",
            &[
                ("app/a.mock", "import-as p ./b.mock\nqref p X\nroot-file"),
                ("app/b.mock", "unit app#p"),
                ("app/c.mock", "unit app#p\ndecl X"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let edges = reference_edges_to(&graph, "X");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].confidence, Confidence::Certain);
    }

    #[test]
    fn a_matched_qualifier_settles_resolution_even_on_a_miss() {
        // `j.Marshal` where the target has no `Marshal`: the name lives in that target or
        // nowhere — a same-file free `Marshal` must NOT capture the qualified reference.
        let dir = project(
            "qref-miss",
            &[
                (
                    "a.mock",
                    "import-as j ./b.mock\ndecl Marshal\nqref j Marshal\nroot-file",
                ),
                ("b.mock", "decl Other"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(
            reference_edges_to(&graph, "Marshal").is_empty(),
            "the local free decl must not capture a qualified reference"
        );
    }

    #[test]
    fn receiver_qualifier_skips_free_names_and_duck_types_to_members() {
        // `t.helper()`: `t` matches no import, so the name is a member access by
        // construction — the same-file free `helper` is not a candidate; the member is,
        // via the §3 fallback.
        let dir = project(
            "qref-receiver",
            &[(
                "a.mock",
                "decl helper\nmember-decl T helper\nqref t helper\nroot-file",
            )],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let free = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == "helper" && s.member_of.is_none())
                .unwrap() as u32,
        );
        let member = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == "helper" && s.member_of.is_some())
                .unwrap() as u32,
        );
        assert!(!graph
            .edges
            .iter()
            .any(|e| matches!(e.kind, EdgeKind::References { to, .. } if to == free)));
        let member_edge = graph
            .edges
            .iter()
            .find(|e| matches!(e.kind, EdgeKind::References { to, .. } if to == member))
            .expect("member fallback edge");
        assert_eq!(member_edge.confidence, Confidence::Probable);
    }

    // -------------------------------------------------- detected_origin (RFC 0012 §7)

    #[test]
    fn detected_origin_overrides_the_claim_time_origin_on_the_file_node() {
        // Claim classifies by path (Authored here); extraction saw a generated banner — the
        // FileNode must carry the corrected origin so every analysis exemption sees it.
        let dir = project(
            "detected-origin",
            &[("a.mock", "detected-generated\ndecl dead")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let class = graph.files[0].class.expect("claimed");
        assert_eq!(class.origin, FileOrigin::Generated);
        assert_eq!(class.role, FileRole::Production, "role stays claim-time");
    }

    #[test]
    fn member_with_no_matching_call_anywhere_stays_certain_dead() {
        // Dead-is-certain survives the fallback: zero same-named call sites ⇒ zero edges.
        let dir = project(
            "member-still-dead",
            &[("a.mock", "member-decl T orphan\ndecl live\nroot-decl live")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(reference_edges_to(&graph, "orphan").is_empty());
    }

    // -------------------------------------------------- test regions (FileFacts::test_spans)

    #[test]
    fn test_gated_module_link_demotes_the_target_file_to_test_role() {
        // `#[cfg(test)] mod tests;` → the child file is a whole-file test the path claim
        // cannot see: phase 2.55 demotes it, and phase 2.6 then gives it the Test root.
        let dir = project(
            "test-gated-demotion",
            &[
                (
                    "a.mock",
                    "decl keep\nroot-decl keep\ntest-region 5 9\nmod-link ./child.mock 6",
                ),
                ("child.mock", "decl helper"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let child = graph
            .files
            .iter()
            .position(|f| f.path.0 == "child.mock")
            .unwrap();
        assert_eq!(
            graph.files[child].class.unwrap().role,
            FileRole::Test,
            "a file linked only from inside a test region is test infrastructure"
        );
        assert!(
            graph.edges.iter().any(|e| matches!(
                e.kind,
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(f),
                } if f.0 as usize == child
            )),
            "the demoted file gets its role-derived Test root"
        );
    }

    #[test]
    fn a_production_module_link_vetoes_the_demotion() {
        // The same child linked from a second file's production code stays production: any
        // ungated module link means the file is compiled outside test builds.
        let dir = project(
            "test-gated-veto",
            &[
                ("a.mock", "test-region 5 9\nmod-link ./child.mock 6"),
                ("b.mock", "mod-link ./child.mock 2"),
                ("child.mock", "decl helper"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let child = graph
            .files
            .iter()
            .position(|f| f.path.0 == "child.mock")
            .unwrap();
        assert_eq!(graph.files[child].class.unwrap().role, FileRole::Production);
    }

    #[test]
    fn declarations_inside_test_regions_get_derived_test_roots() {
        // The single-producer contract (contracts §2): adapters declare only the spans;
        // assembly derives the in-source Test roots by containment. A declaration outside
        // every region gets none.
        let dir = project(
            "derived-test-roots",
            &[(
                "a.mock",
                "decl-at 2 prod_fn\ntest-region 5 9\ndecl-at 6 test_helper",
            )],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let sym = |name: &str| {
            SymbolId(
                graph
                    .symbols
                    .iter()
                    .position(|s| s.name.as_str() == name)
                    .unwrap() as u32,
            )
        };
        let test_rooted = |s: SymbolId| {
            graph.edges.iter().any(|e| {
                matches!(
                    e.kind,
                    EdgeKind::Root {
                        kind: RootKind::Test,
                        target: NodeRef::Symbol(t),
                    } if t == s
                )
            })
        };
        assert!(
            test_rooted(sym("test_helper")),
            "span-contained declaration derives a Certain Test root"
        );
        assert!(!test_rooted(sym("prod_fn")));
    }

    #[test]
    fn file_node_carries_sorted_test_spans() {
        let dir = project(
            "test-span-store",
            &[("a.mock", "test-region 20 30\ntest-region 5 9\ndecl x")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(
            graph.files[0].test_spans,
            vec![
                Span {
                    start: (5, 1),
                    end: (9, 999)
                },
                Span {
                    start: (20, 1),
                    end: (30, 999)
                },
            ],
            "canonical order: sorted regardless of emission order"
        );
    }

    #[test]
    fn import_gating_participates_in_the_surface_signature() {
        // Moving an import across a test-region boundary changes phase 2.55's inputs and
        // hygiene's site role — the patch must decline, so the signature must move.
        let claim = MockAdapter
            .claim(&ProjectPath(SmolStr::new("a.mock")))
            .unwrap();
        let sig_of = |content: &str| {
            let path = ProjectPath(SmolStr::new("a.mock"));
            let facts = MockAdapter.extract(&SourceFile {
                path: &path,
                content: content.as_bytes(),
            });
            surface_signature("mock", 1, &claim, &facts)
        };
        let gated = sig_of("test-region 5 9\nmod-link ./child.mock 6");
        let ungated = sig_of("test-region 5 9\nmod-link ./child.mock 2");
        assert_ne!(gated, ungated, "gating flip must move the signature");
        // …but a pure region move that keeps the import on the same side does not.
        let same_side = sig_of("test-region 4 9\nmod-link ./child.mock 6");
        assert_eq!(
            gated, same_side,
            "reformat-shaped shifts keep the signature"
        );
    }

    // -------------------------------------------------- within attribution (RFC 0012 §4)

    #[test]
    fn within_attributes_the_reference_edge_to_the_enclosing_symbol() {
        let dir = project(
            "within-attribution",
            &[("a.mock", "decl caller\ndecl callee\nref-in caller callee")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let caller = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == "caller")
                .unwrap() as u32,
        );
        let callee = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == "callee")
                .unwrap() as u32,
        );
        assert!(graph.edges.iter().any(|e| matches!(
            e.kind,
            EdgeKind::References { from, to, .. }
                if from == NodeRef::Symbol(caller) && to == callee
        )));
    }

    #[test]
    fn unresolvable_within_falls_back_to_file_attribution() {
        // The design's load-bearing safety property (RFC 0012 §4): a `within` naming nothing
        // this file declares degrades to today's file attribution — keep-alive, never a new
        // way to lose an edge.
        let dir = project(
            "within-fallback",
            &[("a.mock", "decl callee\nref-in ghost callee\nroot-file")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        let callee = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == "callee")
                .unwrap() as u32,
        );
        assert!(graph.edges.iter().any(|e| matches!(
            e.kind,
            EdgeKind::References { from, to, .. }
                if from == NodeRef::File(a) && to == callee
        )));
    }

    #[test]
    fn transitively_dead_code_is_now_visible() {
        // The precision RFC 0012 §4 exists for: `main → a` (both alive); dead `z → b` — b must
        // die with z instead of surviving through the live file's blanket attribution.
        let dir = project(
            "transitive-dead",
            &[(
                "a.mock",
                "decl main\ndecl a\ndecl z\ndecl b\nref-in main a\nref-in z b\nroot-decl main",
            )],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let findings =
            crate::analysis::run_all(&graph, &crate::coverage::CoverageMap::default()).findings;
        let unused: Vec<&str> = findings
            .iter()
            .filter(|f| f.category == "unused")
            .filter_map(|f| f.location.symbol.as_deref())
            .collect();
        assert!(unused.contains(&"z"), "{unused:?}");
        assert!(
            unused.contains(&"b"),
            "b is only called by dead z and must die with it: {unused:?}"
        );
        assert!(!unused.contains(&"a"), "{unused:?}");
        assert!(!unused.contains(&"main"), "{unused:?}");
    }

    #[test]
    fn module_level_references_still_fire_when_the_file_loads() {
        // `within: None` = load-time code: importing the file keeps its module-level
        // references alive exactly as before (RFC 0005 §1's module-load rule).
        let dir = project(
            "module-level-refs",
            &[
                ("entry.mock", "import ./lib.mock\nroot-file"),
                ("lib.mock", "decl used\nref used"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let findings =
            crate::analysis::run_all(&graph, &crate::coverage::CoverageMap::default()).findings;
        // Alive is the claim — an `internal-only` info finding (exported, used same-file
        // only) is separate, correct, and out of scope here.
        assert!(!findings
            .iter()
            .any(|f| f.category == "unused" && f.location.symbol.as_deref() == Some("used")));
    }

    #[test]
    fn files_with_no_unit_are_unaffected_same_name_in_another_unit_does_not_leak_in() {
        // A file that never sets `unit` (every adapter before Go) must behave exactly as before
        // — no accidental cross-file resolution just because some *other*, unrelated file
        // happens to declare a `unit`.
        let dir = project(
            "no-unit-unaffected",
            &[
                ("a.mock", "ref target"),
                ("pkg/b.mock", "unit pkg\nprivate-decl target"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        assert!(!graph.edges.iter().any(
            |e| matches!(e.kind, EdgeKind::References { from, .. } if from == NodeRef::File(a))
        ));
    }

    #[test]
    fn bare_import_produces_dependency_node_and_edge() {
        let dir = project("imports-dep", &[("a.mock", "import lodash")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(graph.dependencies.len(), 1);
        assert_eq!(graph.dependencies[0].name.as_str(), "lodash");
        let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        assert!(graph.edges.iter().any(|e| e.kind
            == EdgeKind::ImportsDependency {
                from: a,
                to: DependencyId(0)
            }));
    }

    #[test]
    fn same_dependency_imported_twice_shares_one_node() {
        let dir = project(
            "dep-dedup",
            &[("a.mock", "import lodash"), ("b.mock", "import lodash")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(graph.dependencies.len(), 1);
    }

    #[test]
    fn unresolved_import_produces_no_edge_and_no_diagnostic() {
        let dir = project("unresolved", &[("a.mock", "import ./missing.mock")]);
        let (graph, diags) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(graph
            .edges
            .iter()
            .all(|e| !matches!(e.kind, EdgeKind::ImportsFile { .. })));
        assert!(
            diags.is_empty(),
            "unresolved is not yet a diagnostic (future `unresolved` analysis)"
        );
    }

    #[test]
    fn manifest_is_not_itself_claimed_as_source() {
        let dir = project("manifest-unclaimed", &[("manifest.json", "dep lodash")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(graph.files.len(), 1);
        assert!(
            graph.files[0].language.is_none(),
            "spec: manifests are not claimed as source (docs/adapters/js-ts.md §1)"
        );
    }

    #[test]
    fn no_manifest_means_everyone_owns_the_implicit_package() {
        let dir = project("pkg-implicit", &[("a.mock", "decl f")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(graph.packages.len(), 1);
        assert!(graph.packages[0].manifest.is_none());
        assert_eq!(graph.files[0].package, PackageId(0));
    }

    #[test]
    fn root_manifest_owns_every_file_under_it() {
        let dir = project(
            "pkg-root",
            &[("manifest.json", "dep lodash"), ("src/a.mock", "decl f")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(graph.packages.len(), 2);
        let manifest_id = graph
            .file_id(&ProjectPath(SmolStr::new("manifest.json")))
            .unwrap();
        let src_id = graph
            .file_id(&ProjectPath(SmolStr::new("src/a.mock")))
            .unwrap();
        assert_eq!(graph.files[manifest_id.0 as usize].package, PackageId(1));
        assert_eq!(graph.files[src_id.0 as usize].package, PackageId(1));
    }

    #[test]
    fn nested_manifest_shadows_the_root_package_for_its_own_subtree() {
        let dir = project(
            "pkg-nested",
            &[
                ("manifest.json", "dep lodash"),
                ("root.mock", "decl f"),
                ("packages/ui/manifest.json", "dep react"),
                ("packages/ui/button.mock", "decl g"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        // implicit(0) is never used (a root manifest exists); root manifest is 1, nested is 2 —
        // discovery order is alphabetical, so `manifest.json` (root) claims package 1 before
        // `packages/ui/manifest.json` claims package 2.
        assert_eq!(graph.packages.len(), 3);

        let root_file = graph
            .file_id(&ProjectPath(SmolStr::new("root.mock")))
            .unwrap();
        let ui_manifest = graph
            .file_id(&ProjectPath(SmolStr::new("packages/ui/manifest.json")))
            .unwrap();
        let ui_file = graph
            .file_id(&ProjectPath(SmolStr::new("packages/ui/button.mock")))
            .unwrap();

        let root_package = graph.files[root_file.0 as usize].package;
        let ui_package = graph.files[ui_manifest.0 as usize].package;
        assert_ne!(
            root_package, ui_package,
            "the nested manifest must shadow the root one for its own subtree"
        );
        assert_eq!(graph.files[ui_file.0 as usize].package, ui_package);
    }

    #[test]
    fn manifest_root_becomes_root_edge_to_target_file() {
        let dir = project(
            "manifest-root",
            &[
                ("manifest.json", "root entry.mock"),
                ("entry.mock", "decl f"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let entry = graph
            .file_id(&ProjectPath(SmolStr::new("entry.mock")))
            .unwrap();
        assert!(graph.edges.iter().any(|e| e.kind
            == EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::File(entry),
            }));
    }

    #[test]
    fn library_root_files_promote_their_exported_symbols_to_production_roots() {
        // RFC 0011 §5: "Published/library: its public API is a production root — external
        // consumers exist by definition." Discovered via M1 conformance-testing against real
        // npm packages (sindresorhus/p-limit): a library's second named export, never called
        // by the package's own code, was false-positive `unused` before this fix.
        let dir = project(
            "library-root-promotion",
            &[
                ("manifest.json", "root entry.mock"),
                ("entry.mock", "decl publicApi\nprivate-decl helper"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let symbol_id = |name: &str| {
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == name)
                .map(|i| SymbolId(i as u32))
                .unwrap()
        };
        assert!(graph.edges.iter().any(|e| e.kind
            == EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::Symbol(symbol_id("publicApi")),
            }));
        assert!(!graph.edges.iter().any(|e| matches!(e.kind,
            EdgeKind::Root { target: NodeRef::Symbol(s), .. } if s == symbol_id("helper"))));
    }

    #[test]
    fn barrel_reexport_resolves_transparently_to_the_original_symbol() {
        // js-ts.md §5: "Barrel files… resolved through, transparently." consumer.mock imports
        // `a` from barrel.mock, which never declares `a` itself — only re-exports it from
        // source.mock. Discovered dogfooding kndo against real npm packages (sindresorhus/
        // type-fest): a pure barrel entry point re-exporting hundreds of individual types is a
        // very common real-world shape.
        let dir = project(
            "barrel-reexport",
            &[
                ("source.mock", "decl a"),
                ("barrel.mock", "reexport ./source.mock a"),
                ("consumer.mock", "import ./barrel.mock a\nref a"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a_symbol = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == "a")
                .unwrap() as u32,
        );
        // Exactly one `a` symbol exists — the barrel didn't fabricate a second declaration.
        assert_eq!(
            graph
                .symbols
                .iter()
                .filter(|s| s.name.as_str() == "a")
                .count(),
            1
        );
        let consumer = graph
            .file_id(&ProjectPath(SmolStr::new("consumer.mock")))
            .unwrap();
        assert!(graph.edges.iter().any(|e| e.kind
            == EdgeKind::References {
                from: NodeRef::File(consumer),
                to: a_symbol,
                kind: crate::vocab::RefKind::Read,
            }));
    }

    /// RFC 0013 §6's equivalence obligation, at the unit level: assemble cold with a cache,
    /// mutate, assemble again (the patch path), and compare against a scratch full rebuild
    /// of the same tree — the graphs must be EQUAL, not merely finding-equivalent. The
    /// project needs ≥ 4 files so one changed file stays under the 30% dirty threshold.
    fn patch_equivalence_case(
        name: &str,
        files: &[(&str, &str)],
        mutate: (&str, &str),
        expect_patch: bool,
    ) {
        // Pad with filler files so one changed file sits under the measured 5% work
        // threshold (RFC 0013 §5) — the scenarios stay about the guard logic, not the
        // threshold arithmetic.
        let filler: Vec<(String, String)> = (0..20)
            .map(|i| (format!("filler{i}.mock"), format!("decl filler{i}")))
            .collect();
        let mut all: Vec<(&str, &str)> = files.to_vec();
        all.extend(filler.iter().map(|(n, c)| (n.as_str(), c.as_str())));
        let dir = project(name, &all);
        let cache_dir = std::env::temp_dir().join(format!("kndo-graph-test-{name}-cache"));
        let _ = fs::remove_dir_all(&cache_dir);
        let cache = crate::cache::ProjectCache::open(&cache_dir);
        assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();

        fs::write(dir.join(mutate.0), mutate.1).unwrap();
        let (patched, patched_diags) =
            assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();
        assert_eq!(
            cache.graph_hits() > 0,
            expect_patch,
            "patch application expectation for {name}"
        );

        let (scratch, scratch_diags) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(
            patched, scratch,
            "patched graph must be identical to the full rebuild ({name})"
        );
        assert_eq!(patched_diags, scratch_diags, "diagnostics too ({name})");
    }

    #[test]
    fn patch_applies_on_a_body_only_edit_and_is_byte_identical() {
        patch_equivalence_case(
            "patch-body-edit",
            &[
                ("a.mock", "decl x\nref helper"),
                ("b.mock", "decl helper\nimport ./a.mock x\nref x"),
                ("c.mock", "decl c1"),
                ("d.mock", "decl d1\nimport ./c.mock c1\nref c1"),
            ],
            // Same declarations, different references and spans — the body-only case.
            ("a.mock", "\n\ndecl x\nref c1\nref helper"),
            true,
        );
    }

    #[test]
    fn patch_falls_back_on_a_surface_change_and_stays_identical() {
        patch_equivalence_case(
            "patch-surface-change",
            &[
                ("a.mock", "decl x"),
                ("b.mock", "decl b1\nimport ./a.mock x\nref x"),
                ("c.mock", "decl c1"),
                ("d.mock", "decl d1"),
            ],
            // A new exported declaration — other files' resolution could change.
            ("a.mock", "decl x\ndecl brand_new"),
            false,
        );
    }

    #[test]
    fn patch_falls_back_when_imports_change() {
        patch_equivalence_case(
            "patch-import-change",
            &[
                ("a.mock", "decl a1\nimport ./c.mock c1"),
                ("b.mock", "decl b1"),
                ("c.mock", "decl c1"),
                ("d.mock", "decl d1"),
            ],
            // The import list is surface (dependency identity + workspace resolution).
            ("a.mock", "decl a1\nimport ./d.mock d1"),
            false,
        );
    }

    #[test]
    fn patch_handles_reference_retargeting_within_the_body() {
        // The regenerated references must resolve against the *other* files' unchanged
        // tables — a.mock stops referencing helper and starts referencing other.
        let name = "patch-ref-retarget";
        let filler: Vec<(String, String)> = (0..20)
            .map(|i| (format!("filler{i}.mock"), format!("decl filler{i}")))
            .collect();
        let mut all: Vec<(&str, &str)> = vec![
            ("a.mock", "decl a1\nimport ./b.mock helper\nref helper"),
            ("b.mock", "decl helper\ndecl other"),
            ("c.mock", "decl c1"),
            ("d.mock", "decl d1"),
        ];
        all.extend(filler.iter().map(|(n, c)| (n.as_str(), c.as_str())));
        let dir = project(name, &all);
        let cache_dir = std::env::temp_dir().join(format!("kndo-graph-test-{name}-cache"));
        let _ = fs::remove_dir_all(&cache_dir);
        let cache = crate::cache::ProjectCache::open(&cache_dir);
        assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();
        fs::write(
            dir.join("a.mock"),
            "decl a1\nimport ./b.mock other\nref other",
        )
        .unwrap();
        // Rebinding an import binding is an import change → surface change → full rebuild.
        let (patched, _) = assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();
        assert_eq!(cache.graph_hits(), 0, "import change must fall back");
        let (scratch, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(patched, scratch);
    }

    #[test]
    fn patched_snapshot_serves_the_next_run_verbatim() {
        // The patched graph is persisted under the new key; a third run with no further
        // changes must hit that snapshot and reproduce the patched graph exactly.
        let name = "patch-then-hit";
        let filler: Vec<(String, String)> = (0..20)
            .map(|i| (format!("filler{i}.mock"), format!("decl filler{i}")))
            .collect();
        let mut all: Vec<(&str, &str)> = vec![
            ("a.mock", "decl x\nref y"),
            ("b.mock", "decl y"),
            ("c.mock", "decl c1"),
            ("d.mock", "decl d1"),
        ];
        all.extend(filler.iter().map(|(n, c)| (n.as_str(), c.as_str())));
        let dir = project(name, &all);
        let cache_dir = std::env::temp_dir().join(format!("kndo-graph-test-{name}-cache"));
        let _ = fs::remove_dir_all(&cache_dir);
        let cache = crate::cache::ProjectCache::open(&cache_dir);
        assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();
        fs::write(dir.join("a.mock"), "\ndecl x\nref y").unwrap();
        let (patched, _) = assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();
        assert!(cache.graph_hits() > 0, "the edit should patch");
        let hits_after_patch = cache.graph_hits();
        let (warm, _) = assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();
        assert!(
            cache.graph_hits() > hits_after_patch,
            "third run hits the key"
        );
        assert_eq!(patched, warm);
    }

    #[test]
    fn a_coverage_only_plugin_keeps_the_snapshot_fast_path() {
        // Regression guard for a real shipped bug: the cache/patch bypass was keyed on
        // `plugins.is_empty()`, and `LcovPlugin` is registered unconditionally by
        // `default_plugins()` — so the graph-snapshot cache and the incremental patch were
        // silently dead on every real `kndo` run from the day the graph hooks were wired.
        // A plugin with `mutates_graph() == false` must be invisible to both fast paths.
        let name = "coverage-plugin-keeps-cache";
        let dir = project(name, &[("a.mock", "decl x\nref y"), ("b.mock", "decl y")]);
        let cache_dir = std::env::temp_dir().join(format!("kndo-graph-test-{name}-cache"));
        let _ = fs::remove_dir_all(&cache_dir);
        let cache = crate::cache::ProjectCache::open(&cache_dir);
        let plugins: Vec<Box<dyn crate::plugin::Plugin>> =
            vec![Box::new(crate::plugin::LcovPlugin)];
        assemble_with_cache(&dir, &mock_adapters(), &plugins, Some(&cache)).unwrap();
        let (warm, _) =
            assemble_with_cache(&dir, &mock_adapters(), &plugins, Some(&cache)).unwrap();
        assert!(
            cache.graph_hits() > 0,
            "a coverage-only plugin must not bypass the snapshot cache"
        );
        let (cold, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(
            warm, cold,
            "the served snapshot must equal a plugin-less cold build"
        );
    }

    #[test]
    fn a_graph_mutating_plugin_still_bypasses_the_snapshot() {
        // The other direction of the same rule: a plugin that participates in graph assembly
        // (the trait's `mutates_graph` default — hooks all defaulted is enough, the *claim* is
        // what gates) must keep forcing full rebuilds, since neither the snapshot hit nor the
        // patch re-invokes its hooks.
        struct HookedPlugin;
        impl crate::plugin::Plugin for HookedPlugin {
            fn descriptor(&self) -> crate::plugin::PluginDescriptor {
                crate::plugin::PluginDescriptor {
                    id: SmolStr::new("hooked"),
                    version: SmolStr::new("1"),
                    detection: vec![],
                    requested_file_access: vec![],
                    activation: vec![],
                    dependencies: vec![],
                }
            }
        }
        let name = "mutating-plugin-bypasses-cache";
        let dir = project(name, &[("a.mock", "decl x\nref y"), ("b.mock", "decl y")]);
        let cache_dir = std::env::temp_dir().join(format!("kndo-graph-test-{name}-cache"));
        let _ = fs::remove_dir_all(&cache_dir);
        let cache = crate::cache::ProjectCache::open(&cache_dir);
        let plugins: Vec<Box<dyn crate::plugin::Plugin>> = vec![Box::new(HookedPlugin)];
        assemble_with_cache(&dir, &mock_adapters(), &plugins, Some(&cache)).unwrap();
        assemble_with_cache(&dir, &mock_adapters(), &plugins, Some(&cache)).unwrap();
        assert_eq!(
            cache.graph_hits(),
            0,
            "a graph-mutating plugin must bypass the snapshot cache on every run"
        );
    }

    #[test]
    fn surface_signature_ignores_spans_but_sees_surface_changes() {
        // RFC 0013 §4: bodies and positions move freely under the patch guard; any change to
        // what other files can resolve against must move the signature.
        let base = project("sig-base", &[("a.mock", "decl x\nref y")]);
        let moved = project("sig-moved", &[("a.mock", "\n\ndecl x\nref y")]);
        let grown = project("sig-grown", &[("a.mock", "decl x\ndecl z\nref y")]);
        let sig = |dir: &std::path::Path| {
            let (g, _) = assemble(dir, &mock_adapters(), &[]).unwrap();
            g.patch_meta[g.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap().0 as usize]
                .surface_sig
                .expect("claimed files carry a signature")
        };
        assert_eq!(
            sig(&base),
            sig(&moved),
            "span-only movement must not move the signature"
        );
        assert_ne!(
            sig(&base),
            sig(&grown),
            "a new declaration must move the signature"
        );
    }

    #[test]
    fn patch_meta_records_unit_names_and_reexport_aliases() {
        let dir = project(
            "patch-meta",
            &[
                ("source.mock", "decl a"),
                ("barrel.mock", "reexport ./source.mock a"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let barrel = graph
            .file_id(&ProjectPath(SmolStr::new("barrel.mock")))
            .unwrap();
        let aliases = &graph.patch_meta[barrel.0 as usize].reexport_aliases;
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].name.as_str(), "a");
        assert_eq!(
            graph.symbols[aliases[0].symbol.0 as usize].name.as_str(),
            "a"
        );
    }

    #[test]
    fn barrel_chains_resolve_regardless_of_discovery_order() {
        // RFC 0013 §3b's fixpoint: `outer` re-exports from `zeta`, which re-exports from the
        // real source — and `outer.mock` sorts BEFORE `zeta.mock`, exactly the discovery
        // order the old single-pass one-hop resolution could not handle (outer's lookup ran
        // before zeta's alias existed). The consumer must still reach the one real symbol.
        let dir = project(
            "barrel-chain-order",
            &[
                ("aaa_source.mock", "decl deep"),
                ("outer.mock", "reexport ./zeta.mock deep"),
                ("zeta.mock", "reexport ./aaa_source.mock deep"),
                ("consumer.mock", "import ./outer.mock deep\nref deep"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(
            graph
                .symbols
                .iter()
                .filter(|s| s.name.as_str() == "deep")
                .count(),
            1
        );
        let deep = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == "deep")
                .unwrap() as u32,
        );
        let consumer = graph
            .file_id(&ProjectPath(SmolStr::new("consumer.mock")))
            .unwrap();
        assert!(
            graph.edges.iter().any(|e| e.kind
                == EdgeKind::References {
                    from: NodeRef::File(consumer),
                    to: deep,
                    kind: crate::vocab::RefKind::Read,
                }),
            "the consumer's reference must resolve through the two-hop chain"
        );
    }

    #[test]
    fn reexport_cycles_terminate_and_resolve_to_nothing() {
        // RFC 0013 §3b: a cycle of re-exports makes no progress and must simply terminate —
        // no alias ever materializes, nothing hangs, nothing panics.
        let dir = project(
            "barrel-cycle",
            &[
                ("ping.mock", "reexport ./pong.mock ghost"),
                ("pong.mock", "reexport ./ping.mock ghost"),
                ("consumer.mock", "import ./ping.mock ghost\nref ghost"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(graph.symbols.iter().all(|s| s.name.as_str() != "ghost"));
    }

    #[test]
    fn assembled_edge_and_diagnostic_order_is_canonical() {
        // RFC 0013 §3a: order is data. Assembling the same tree twice — or any two
        // construction paths over identical inputs — must yield identical vectors, which is
        // what the sort guarantees; spot-check that the vector is actually sorted.
        let dir = project(
            "canonical-order",
            &[
                ("a.mock", "decl x\nref y"),
                ("b.mock", "decl y\nimport ./a.mock x\nref x"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(graph.edges.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn barrel_reexport_from_a_library_root_promotes_the_original_symbol_to_a_production_root() {
        let dir = project(
            "barrel-root-reexport",
            &[
                ("manifest.json", "root barrel.mock"),
                ("source.mock", "decl a"),
                ("barrel.mock", "reexport ./source.mock a"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a_symbol = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == "a")
                .unwrap() as u32,
        );
        assert!(graph.edges.iter().any(|e| e.kind
            == EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::Symbol(a_symbol),
            }));
    }

    #[test]
    fn manifest_root_naming_an_unknown_file_is_dropped_not_fabricated() {
        let dir = project(
            "manifest-root-missing",
            &[("manifest.json", "root nope.mock")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(graph
            .edges
            .iter()
            .all(|e| !matches!(e.kind, EdgeKind::Root { .. })));
    }

    #[test]
    fn manifest_dependencies_reach_the_resolver_as_declared() {
        // With no manifest, `lodash` resolves undeclared (the mock demotes to `probable`).
        let dir = project("no-manifest-dep", &[("a.mock", "import lodash")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let edge = graph
            .edges
            .iter()
            .find(|e| matches!(e.kind, EdgeKind::ImportsDependency { .. }))
            .unwrap();
        assert_eq!(edge.confidence, Confidence::Probable);

        // Declared in a manifest, the same import resolves `certain` — proof
        // `declared_dependencies` actually threads from manifest facts into the resolver ctx.
        let dir = project(
            "manifest-dep",
            &[("manifest.json", "dep lodash"), ("a.mock", "import lodash")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let edge = graph
            .edges
            .iter()
            .find(|e| matches!(e.kind, EdgeKind::ImportsDependency { .. }))
            .unwrap();
        assert_eq!(edge.confidence, Confidence::Certain);
    }

    // ---------------------------------------------------------------- workspace members

    #[test]
    fn workspace_name_import_produces_both_file_and_dependency_edges() {
        // RFC 0011 §4: resolution yields the concrete internal file (real reachability) AND
        // the declaration contract stays checkable (an ImportsDependency edge by name).
        let dir = project(
            "ws-both-edges",
            &[
                ("packages/a/manifest.json", "name pkg-a\ndep pkg-b"),
                ("packages/a/src.mock", "import pkg-b\nroot-file"),
                (
                    "packages/b/manifest.json",
                    "name pkg-b\nentry packages/b/lib.mock",
                ),
                ("packages/b/lib.mock", "decl util"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a_src = graph
            .file_id(&ProjectPath(SmolStr::new("packages/a/src.mock")))
            .unwrap();
        let b_lib = graph
            .file_id(&ProjectPath(SmolStr::new("packages/b/lib.mock")))
            .unwrap();
        assert!(graph.edges.iter().any(|e| e.kind
            == EdgeKind::ImportsFile {
                from: a_src,
                to: b_lib,
            }));
        assert!(graph
            .dependencies
            .iter()
            .any(|d| d.name.as_str() == "pkg-b"));
        assert!(graph
            .edges
            .iter()
            .any(|e| matches!(e.kind, EdgeKind::ImportsDependency { from, .. } if from == a_src)));
        // And the reachability consequence: b's FILE is alive through the cross-package
        // import even though b is not a root of anything. (Its `util` symbol is still
        // correctly flagged — this fixture's import carries no bindings, nothing references
        // the symbol by name; symbol-level discrimination survives the file being alive.)
        let findings =
            crate::analysis::run_all(&graph, &crate::coverage::CoverageMap::default()).findings;
        assert!(!findings.iter().any(|f| f.subject_kind == "file"
            && f.location.path.as_ref().map(|p| p.0.as_str()) == Some("packages/b/lib.mock")));
    }

    #[test]
    fn phantom_internal_dependency_is_undeclared() {
        // packages/a imports pkg-b by name WITHOUT declaring it — RFC 0011 §4's table:
        // "import resolves into a sibling package not declared in the importer's manifest →
        // undeclared (phantom internal dependency)".
        let dir = project(
            "ws-phantom",
            &[
                ("packages/a/manifest.json", "name pkg-a"),
                ("packages/a/src.mock", "import pkg-b\nroot-file"),
                (
                    "packages/b/manifest.json",
                    "name pkg-b\nentry packages/b/lib.mock",
                ),
                ("packages/b/lib.mock", "decl util"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let findings =
            crate::analysis::run_all(&graph, &crate::coverage::CoverageMap::default()).findings;
        assert!(findings
            .iter()
            .any(|f| f.category == "undeclared" && f.location.symbol.as_deref() == Some("pkg-b")));
    }

    #[test]
    fn declared_but_unimported_workspace_dep_is_unused() {
        // The other direction of RFC 0011 §4's table: "internal dep declared, no import
        // resolves into that package → unused (subject dependency)".
        let dir = project(
            "ws-unused-dep",
            &[
                ("packages/a/manifest.json", "name pkg-a\ndep pkg-b"),
                ("packages/a/src.mock", "root-file"),
                (
                    "packages/b/manifest.json",
                    "name pkg-b\nentry packages/b/lib.mock",
                ),
                ("packages/b/lib.mock", "root-file"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let findings =
            crate::analysis::run_all(&graph, &crate::coverage::CoverageMap::default()).findings;
        assert!(findings.iter().any(|f| f.category == "unused"
            && f.subject_kind == "dependency"
            && f.location.symbol.as_deref() == Some("pkg-b")));
    }

    #[test]
    fn workspace_bindings_resolve_to_the_siblings_symbols() {
        // `import { util } from 'pkg-b'` — the binding resolves through b's entry file's
        // symbol table, so `util` is kept alive by a's reference while b's other export
        // is still caught.
        let dir = project(
            "ws-bindings",
            &[
                ("packages/a/manifest.json", "name pkg-a\ndep pkg-b"),
                (
                    "packages/a/src.mock",
                    "import pkg-b util\nref util\nroot-file",
                ),
                (
                    "packages/b/manifest.json",
                    "name pkg-b\nentry packages/b/lib.mock",
                ),
                ("packages/b/lib.mock", "decl util\ndecl dead"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let findings =
            crate::analysis::run_all(&graph, &crate::coverage::CoverageMap::default()).findings;
        let flagged: Vec<Option<&str>> = findings
            .iter()
            .map(|f| f.location.symbol.as_deref())
            .collect();
        assert!(flagged.contains(&Some("dead")));
        assert!(!flagged.contains(&Some("util")));
    }

    #[test]
    fn import_binding_resolves_through_the_target_s_unit_not_just_its_own_file() {
        // Go's shape: `import "pkg"` resolves to *one* representative file in the target
        // directory (contracts §2 has no multi-file resolution target), but the actually-used
        // symbol may be declared in a *different* file that merely shares the same package
        // (`unit`) — e.g. `resolve()` picks `pkg/x.mock` as the nominal target, but `target` is
        // declared in its sibling `pkg/y.mock`.
        let dir = project(
            "import-binding-unit-fallback",
            &[
                (
                    "a.mock",
                    "import ./pkg/x.mock target\nref target\nroot-file",
                ),
                ("pkg/x.mock", "unit pkg"),
                ("pkg/y.mock", "unit pkg\nprivate-decl target"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let findings =
            crate::analysis::run_all(&graph, &crate::coverage::CoverageMap::default()).findings;
        let flagged: Vec<Option<&str>> = findings
            .iter()
            .map(|f| f.location.symbol.as_deref())
            .collect();
        assert!(
            !flagged.contains(&Some("target")),
            "the binding should have resolved through pkg's unit table: {findings:?}"
        );
    }

    #[test]
    fn cli_invoke_directive_reaches_script_invoked_dependencies() {
        let dir = project("cli-invoke", &[("manifest.json", "dep xo\ncli-invoke xo")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(graph
            .script_invoked_dependencies
            .contains(&(PackageId(1), SmolStr::new("xo"))));
    }

    #[test]
    fn raw_root_whole_file_becomes_root_edge_to_the_file() {
        let dir = project("raw-root-file", &[("a.mock", "root-file")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        assert!(graph.edges.iter().any(|e| e.kind
            == EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::File(a),
            }));
    }

    #[test]
    fn raw_root_declaration_becomes_root_edge_to_the_symbol() {
        let dir = project("raw-root-decl", &[("a.mock", "decl f\nroot-decl f")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let f = graph.symbols.iter().position(|s| s.name == "f").unwrap() as u32;
        assert!(graph.edges.iter().any(|e| e.kind
            == EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::Symbol(SymbolId(f)),
            }));
    }

    #[test]
    fn raw_root_naming_an_unknown_declaration_is_dropped_not_fabricated() {
        let dir = project("raw-root-decl-missing", &[("a.mock", "root-decl ghost")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(graph
            .edges
            .iter()
            .all(|e| !matches!(e.kind, EdgeKind::Root { .. })));
    }

    #[test]
    fn test_role_files_are_test_roots_not_unused() {
        // RFC 0005 §2: "Test roots — test functions/files (language role detection…)". A test
        // file nothing imports is TestOnly, not Unreachable — while a production orphan next
        // to it is still caught.
        let dir = project(
            "role-roots-test",
            &[("a.test.mock", "decl helper"), ("orphan.mock", "decl gone")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let test_file = graph
            .file_id(&ProjectPath(SmolStr::new("a.test.mock")))
            .unwrap();
        let root = graph
            .edges
            .iter()
            .find(|e| {
                e.kind
                    == EdgeKind::Root {
                        kind: RootKind::Test,
                        target: NodeRef::File(test_file),
                    }
            })
            .expect("role-derived test root");
        assert_eq!(root.confidence, Confidence::Probable);

        let findings =
            crate::analysis::run_all(&graph, &crate::coverage::CoverageMap::default()).findings;
        let flagged_paths: Vec<&str> = findings
            .iter()
            .filter_map(|f| f.location.path.as_ref().map(|p| p.0.as_str()))
            .collect();
        assert!(!flagged_paths.contains(&"a.test.mock"));
        assert!(flagged_paths.contains(&"orphan.mock"));
    }

    #[test]
    fn tooling_role_files_root_their_exported_symbols_too() {
        // A config file's exports ARE its interface to the tool that loads it — neither the
        // file nor its exported symbol may be flagged; an unexported dead helper inside the
        // same config still is (symbol-level precision survives the promotion).
        let dir = project(
            "role-roots-tooling",
            &[(
                "build.config.mock",
                "decl configObject\nprivate-decl helper",
            )],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let findings =
            crate::analysis::run_all(&graph, &crate::coverage::CoverageMap::default()).findings;
        assert!(!findings
            .iter()
            .any(|f| f.location.symbol.as_deref() == Some("configObject")));
        assert!(findings
            .iter()
            .any(|f| f.location.symbol.as_deref() == Some("helper")));
    }

    #[test]
    fn opaque_namespace_import_wildcards_over_the_target() {
        // `import * as ns; f(ns)` / `ns[key]` — the namespace escaped static tracking, so
        // every symbol in the target is plausibly used (RFC 0005 §1). End-to-end: the
        // target's never-referenced-by-name symbol must stay out of `unused`.
        let dir = project(
            "opaque-namespace",
            &[
                ("a.mock", "root-file\nimport-opaque ./b.mock"),
                ("b.mock", "decl viaKey"),
                ("dead.mock", "decl gone"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let b = graph.file_id(&ProjectPath(SmolStr::new("b.mock"))).unwrap();
        let wildcard = graph
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Wildcard { from: b })
            .expect("wildcard from the opaquely-consumed target");
        assert_eq!(wildcard.confidence, Confidence::Possible);

        let findings =
            crate::analysis::run_all(&graph, &crate::coverage::CoverageMap::default()).findings;
        assert!(!findings
            .iter()
            .any(|f| f.location.symbol.as_deref() == Some("viaKey")));
        assert!(findings
            .iter()
            .any(|f| f.location.path.as_ref().map(|p| p.0.as_str()) == Some("dead.mock")));
    }

    #[test]
    fn unnarrowed_dynamic_becomes_a_wildcard_edge_from_the_file() {
        let dir = project("dynamic-plain", &[("a.mock", "dynamic")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        let wildcard = graph
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Wildcard { from: a })
            .expect("wildcard edge");
        assert_eq!(wildcard.confidence, Confidence::Possible);
    }

    #[test]
    fn narrowed_dynamic_imports_the_directorys_files_at_possible() {
        let dir = project(
            "dynamic-narrowed",
            &[
                ("a.mock", "dynamic-narrowed handlers"),
                ("handlers/one.mock", "decl run"),
                ("handlers/sub/two.mock", ""),
                ("handlers/data.json", "{}"), // unclaimed — still a plausible target
                ("elsewhere/other.mock", ""),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        let id = |p: &str| graph.file_id(&ProjectPath(SmolStr::new(p))).unwrap();
        let imports_from_a: Vec<FileId> = graph
            .edges
            .iter()
            .filter_map(|e| match e.kind {
                EdgeKind::ImportsFile { from, to } if from == a => {
                    assert_eq!(e.confidence, Confidence::Possible);
                    Some(to)
                }
                _ => None,
            })
            .collect();
        assert!(imports_from_a.contains(&id("handlers/one.mock")));
        assert!(imports_from_a.contains(&id("handlers/sub/two.mock")));
        assert!(imports_from_a.contains(&id("handlers/data.json")));
        assert!(!imports_from_a.contains(&id("elsewhere/other.mock")));
        assert!(!imports_from_a.contains(&a));
        // Each narrowed target also wildcards over its own symbols: a dynamically-imported
        // module is consumed opaquely, so its exports must not stay certain-dead.
        assert!(graph.edges.iter().any(|e| e.kind
            == EdgeKind::Wildcard {
                from: id("handlers/one.mock")
            }));
        assert!(!graph
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::Wildcard { from: a }));
    }

    #[test]
    fn narrowed_dynamic_keeps_target_symbols_possible_alive_end_to_end() {
        // The §6 corpus promise ("wildcard narrows, nothing false-positive"), at the graph +
        // analysis level: a root file dynamically loading `handlers/` keeps the handler's
        // exported symbol out of `unused`, while a file outside the narrowed scope is still
        // caught.
        let dir = project(
            "dynamic-liveness",
            &[
                ("a.mock", "root-file\ndynamic-narrowed handlers"),
                ("handlers/one.mock", "decl run"),
                ("dead.mock", "decl gone"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let findings =
            crate::analysis::run_all(&graph, &crate::coverage::CoverageMap::default()).findings;
        let subjects: Vec<(&str, Option<&str>)> = findings
            .iter()
            .map(|f| {
                (
                    f.subject_kind.as_str(),
                    f.location.path.as_ref().map(|p| p.0.as_str()),
                )
            })
            .collect();
        assert!(subjects.contains(&("file", Some("dead.mock"))));
        assert!(!subjects
            .iter()
            .any(|(_, p)| *p == Some("handlers/one.mock")));
    }

    #[test]
    fn cross_file_reference_resolves_via_import_binding() {
        // a.mock (index 0) references `used`, imported (bound) from b.mock (index 1) — a
        // forward reference in file-discovery order, the case phase 3a/3b split exists for.
        let dir = project(
            "ref-cross-file",
            &[
                ("a.mock", "import ./b.mock used\nref used"),
                ("b.mock", "decl used"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        let used = SymbolId(graph.symbols.iter().position(|s| s.name == "used").unwrap() as u32);
        assert!(graph.edges.iter().any(|e| e.kind
            == EdgeKind::References {
                from: NodeRef::File(a),
                to: used,
                kind: crate::vocab::RefKind::Read,
            }));
    }

    #[test]
    fn renamed_binding_resolves_to_the_original_exported_name() {
        // `import { used as alias }` — alias.local != alias.imported.
        let dir = project(
            "ref-renamed-binding",
            &[
                ("a.mock", "import ./b.mock alias=used\nref alias"),
                ("b.mock", "decl used"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let used_symbol = graph.symbols.iter().find(|s| s.name == "used").unwrap();
        assert!(graph
            .edges
            .iter()
            .any(|e| matches!(e.kind, EdgeKind::References { to, .. } if graph.symbols[to.0 as usize].name == used_symbol.name)));
    }

    #[test]
    fn default_binding_resolves_to_the_synthetic_default_export() {
        let dir = project(
            "ref-default-binding",
            &[
                ("a.mock", "import ./b.mock main=\nref main"),
                ("b.mock", "decl default"),
            ],
        );
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(graph
            .edges
            .iter()
            .any(|e| matches!(e.kind, EdgeKind::References { .. })));
    }

    #[test]
    fn same_file_reference_resolves_without_an_import() {
        let dir = project("ref-same-file", &[("a.mock", "decl helper\nref helper")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        let helper = SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name == "helper")
                .unwrap() as u32,
        );
        assert!(graph.edges.iter().any(|e| e.kind
            == EdgeKind::References {
                from: NodeRef::File(a),
                to: helper,
                kind: crate::vocab::RefKind::Read,
            }));
    }

    #[test]
    fn reference_to_an_unresolvable_name_produces_no_edge() {
        let dir = project("ref-unresolved", &[("a.mock", "ref ghost")]);
        let (graph, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert!(graph
            .edges
            .iter()
            .all(|e| !matches!(e.kind, EdgeKind::References { .. })));
    }

    #[test]
    fn assembly_is_deterministic_across_runs() {
        let dir = project(
            "determinism",
            &[
                ("a.mock", "decl x\nimport ./b.mock\nimport lodash"),
                ("b.mock", "decl y"),
                ("c.mock", "decl z\nimport ./a.mock"),
            ],
        );
        let (g1, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        let (g2, _) = assemble(&dir, &mock_adapters(), &[]).unwrap();
        assert_eq!(g1.edges, g2.edges);
        assert_eq!(
            g1.symbols
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>(),
            g2.symbols
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>()
        );
    }

    // RFC 0004 §4's correctness gate: `--no-cache` must produce byte-identical findings to a
    // cached run. This slice only caches `FileFacts` (no graph/findings snapshot yet), so the
    // gate is checked at the graph level — a warm assemble must yield the exact same edges and
    // symbols as a cold one on identical input.
    #[test]
    fn warm_assemble_matches_a_cold_assemble_byte_for_byte() {
        let dir = project(
            "cache-equivalence",
            &[
                ("a.mock", "decl x\nimport ./b.mock\nimport lodash"),
                ("b.mock", "decl y"),
                ("c.mock", "decl z\nimport ./a.mock"),
            ],
        );
        let cold = assemble(&dir, &mock_adapters(), &[]).unwrap().0;

        let cache_dir = std::env::temp_dir().join("kndo-graph-test-cache-equivalence-cache");
        let _ = fs::remove_dir_all(&cache_dir);
        let cache = crate::cache::ProjectCache::open(&cache_dir);
        // First cached run populates every entry (all misses); second is fully warm.
        assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();
        let warm = assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache))
            .unwrap()
            .0;

        assert_eq!(cold.edges, warm.edges);
        assert_eq!(
            cold.symbols
                .iter()
                .map(|s| (s.name.clone(), s.kind.clone(), s.file))
                .collect::<Vec<_>>(),
            warm.symbols
                .iter()
                .map(|s| (s.name.clone(), s.kind.clone(), s.file))
                .collect::<Vec<_>>()
        );
        assert_eq!(cold.files.len(), warm.files.len());
        assert_eq!(cold.dependencies.len(), warm.dependencies.len());
    }

    #[test]
    fn unchanged_files_are_served_from_the_facts_cache_on_the_second_assemble() {
        let dir = project(
            "cache-hits",
            &[("a.mock", "decl x"), ("b.mock", "decl y\nimport ./a.mock")],
        );
        let cache_dir = std::env::temp_dir().join("kndo-graph-test-cache-hits-cache");
        let _ = fs::remove_dir_all(&cache_dir);
        let cache = crate::cache::ProjectCache::open(&cache_dir);

        assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();
        assert_eq!(cache.hits(), 0); // first run: every file is a miss, then gets stored
        assert_eq!(cache.graph_hits(), 0);

        // Second run: nothing changed, so the *graph* snapshot itself hits (the stronger,
        // whole-assembly skip) before per-file facts are ever consulted — the facts layer
        // stays exactly where the first run left it.
        assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.graph_hits(), 1);
    }

    #[test]
    fn a_changed_file_misses_the_graph_snapshot_but_still_warms_its_sibling_from_facts() {
        let dir = project(
            "graph-key-sensitivity",
            &[("a.mock", "decl x"), ("b.mock", "decl y\nimport ./a.mock")],
        );
        let cache_dir = std::env::temp_dir().join("kndo-graph-test-graph-key-sensitivity-cache");
        let _ = fs::remove_dir_all(&cache_dir);
        let cache = crate::cache::ProjectCache::open(&cache_dir);

        assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();

        // Edit one file — the graph key changes (it folds in the whole file set), so the
        // snapshot must miss; but the *other*, untouched file's facts entry is still valid.
        fs::write(dir.join("a.mock"), "decl x2").unwrap();
        assemble_with_cache(&dir, &mock_adapters(), &[], Some(&cache)).unwrap();
        assert_eq!(cache.graph_hits(), 0); // never hit — the key never matched after the edit
        assert_eq!(cache.hits(), 1); // b.mock's facts, unchanged, still served from disk
    }
}
