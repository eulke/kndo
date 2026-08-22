//! The `Plugin` contract (contracts/core-traits.md §3, RFC 0003).
//!
//! Adapters describe what code *is*; plugins describe what an ecosystem *means* by it.
//! All hooks are optional; the same trait serves built-ins (statically linked) and external
//! WASM components (bridged via `kndo-plugin-api`, ADR 0003 — `kndo:plugin@0.1.0` for the four
//! graph-mutation hooks below, `kndo:adapter@0.1.0` for `LanguageAdapter`; see
//! docs/contracts/wasm-abi.md §5). `ingest_coverage`/`suppress` aren't bridged either way yet.
//! `GraphView` is read-only; mutation happens only through typed sinks the core validates and
//! attributes (`Provenance::Plugin`). `contribute_roots`/`contribute_edges`/`annotate_symbols`
//! additionally get [`ContentView`], RFC 0016 §5's host-mediated content channel for files
//! *outside* the language graph (configs, manifests, templates) — scoped to the descriptor's
//! own `requested_file_access` globs and budgeted; `classify_file` does not (it runs once per
//! file across every component, not once per component per round — see `ContentView`'s own
//! doc for why that hook is left out).
//!
//! **Targets are named, never addressed by internal id** (`ProjectPath` + an optional bare or
//! `Owner.name` symbol name) — the same contract shape `RawRoot`/`RawReference` already use for
//! adapters (contracts/core-traits.md §2). A plugin naming a target that doesn't resolve is a
//! silent no-op, exactly like an adapter's own miss — no new failure mode, and it keeps
//! `FileId`/`SymbolId` (internal, renumbered every run) off the `Plugin` trait's stable-from-1.0
//! surface entirely.

use std::cell::RefCell;

use rustc_hash::FxHashMap as HashMap;
use smol_str::SmolStr;

use crate::adapter::{Diagnostic, DiagnosticLevel, ProjectPath};
use crate::graph::{FileNode, SymbolNode};
use crate::vocab::{Confidence, FileClass, FileId, RefKind, RootKind};

#[derive(Debug, Clone)]
pub struct PluginDescriptor {
    /// The plugin's identity, which is also its provenance (RFC 0015 §2): built-ins use the
    /// reserved `kndo:` namespace (`kndo:coverage-lcov`, `kndo:nextjs`) that no external
    /// component may claim; external plugins use the source coordinate they can be fetched
    /// from (`github.com/<owner>/<repo>`), so identity is never a name lookup and two
    /// same-purpose plugins from different authors can never collide. A hand-dropped
    /// `.kndo/plugins/*.wasm` may carry a plain name, but a plain name can never be the
    /// target of a [`dependencies`](Self::dependencies) edge — depending on a plugin requires
    /// it to be addressable.
    pub id: SmolStr,
    pub version: SmolStr,
    /// Auto-detection predicates, in prose ("package.json depends on react") — shown by
    /// `kndo doctor`, never evaluated. [`activation`](Self::activation) is the machine-checkable
    /// counterpart these describe.
    pub detection: Vec<SmolStr>,
    /// Globs whose content the host will provide; no ambient fs/net (RFC 0003 §5).
    pub requested_file_access: Vec<SmolStr>,
    /// Structured, machine-evaluable version of [`detection`](Self::detection) — what actually
    /// decides whether a *globally* installed plugin (RFC 0003 §4) turns on for a given project.
    /// A project-local `.kndo/plugins/*.wasm` file is unconditional (its presence there already
    /// is the opt-in); this only gates the XDG-wide install path, and only when non-empty — an
    /// empty list means "no known structural signal," so a globally installed plugin with none
    /// never self-activates rather than guessing.
    pub activation: Vec<ActivationRule>,
    /// Plugins whose conventions are part of this plugin's own — the wrapper relationship
    /// (RFC 0015 §3). Each entry is an [`id`](Self::id)-style coordinate (`kndo:nextjs`,
    /// `github.com/owner/repo`). Exactly two coupled effects, and nothing else: installing this
    /// plugin installs its dependencies transitively (RFC 0015 §4), and this plugin being
    /// *active* activates every dependency that is present — computed as a fixpoint, so wrapper
    /// chains (`company-framework → nextjs → …`) compose to any depth from one manifest match.
    /// No version constraints and no ordering implications: plugins structurally cannot consume
    /// each other's contributions, so there is no inter-plugin ABI to be compatible about. A
    /// missing dependency is never a runtime error — `kndo doctor` names the missing coordinate.
    pub dependencies: Vec<SmolStr>,
}

/// Whether `id` claims the reserved built-in namespace (RFC 0015 §2) — external components
/// carrying such an id are rejected at load: the namespace is not claimable, which is what
/// makes `dependencies: ["kndo:nextjs"]` unambiguous from any source.
pub fn is_reserved_id(id: &str) -> bool {
    id.starts_with("kndo:")
}

/// One machine-checkable activation predicate (RFC 0003 §4). Evaluated against the project
/// root before a globally installed plugin is even instantiated — cheap, filesystem-only checks,
/// no code execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationRule {
    /// At least one file under the project root matches this glob (e.g. `"next.config.*"`).
    FileExists(SmolStr),
    /// Any `package.json`/`Cargo.toml` anywhere under the project root (not just the root's
    /// own — RFC 0003 §4) declares a dependency with this name, in any dependency section.
    ManifestDependency(SmolStr),
}

impl ActivationRule {
    /// Short, stable rendering for `kndo doctor` and any other "why did/didn't this activate"
    /// display surface — not a serialization format, just human-readable.
    pub fn describe(&self) -> String {
        match self {
            ActivationRule::FileExists(glob) => format!("file-exists: {glob}"),
            ActivationRule::ManifestDependency(name) => format!("manifest-dependency: {name}"),
        }
    }
}

// ---------------------------------------------------------------- read side: GraphView

/// Read-only view over the assembled-so-far graph, handed to `contribute_roots`/
/// `contribute_edges`/`annotate_symbols`. Borrows the core's own `files`/`symbols` vectors
/// directly (no copy) — by the phase these hooks run (`graph::assemble_from_source`, after
/// phase 3b's reference resolution, before the canonical sort), both are fully populated and
/// never mutated again for this run.
///
/// `symbols_in` is backed by a one-time `FileId -> Vec<symbol index>` index built when the view
/// is constructed (one linear pass over `symbols`, not a scan per call) — a plugin that walks
/// every file's symbols still costs `O(files + symbols)` total, not `O(files * symbols)`.
pub struct GraphView<'a> {
    files: &'a [FileNode],
    symbols: &'a [SymbolNode],
    file_index: &'a HashMap<ProjectPath, FileId>,
    packages: &'a [crate::graph::PackageNode],
    /// The adapter-derived edge list as of this round. Queries answer from
    /// `Provenance::Adapter` edges only (RFC 0017 §2's rule R1): a plugin never sees another
    /// plugin's contributions — without that, results would depend on registration order and
    /// determinism across compositions would silently break.
    edges: &'a [crate::vocab::Edge],
    symbols_by_file: HashMap<FileId, Vec<u32>>,
    /// Reverse/forward `ImportsFile` index, built lazily on the first edge query (RFC 0017
    /// §2's rule R2): a plugin that never asks pays nothing. `RefCell`, not a lock — a view
    /// lives inside one single-threaded plugin round, same pattern as `ContentView`'s budget.
    import_index: RefCell<Option<ImportIndex>>,
}

/// Forward and reverse file-import adjacency, `FileId`-indexed, targets sorted — built once
/// per view on first use.
struct ImportIndex {
    imports_of: HashMap<FileId, Vec<FileId>>,
    importers_of: HashMap<FileId, Vec<FileId>>,
}

/// One [`GraphView::packages`] entry — RFC 0011 §3's package topology, plugin-visible
/// (RFC 0017 §5.2). `root_dir` is the manifest's own directory (`""` for the implicit
/// package that owns whatever no manifest governs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageView<'a> {
    pub manifest: Option<&'a ProjectPath>,
    pub name: Option<&'a str>,
    pub root_dir: &'a str,
}

/// One [`GraphView::references_to`] result — a site referencing the queried symbol, named by
/// path/symbol like every other plugin-facing value (never internal ids).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefSiteView<'a> {
    pub from_path: &'a ProjectPath,
    /// `None` when the adapter only knew the referencing *file* (file-granularity site).
    pub from_symbol: Option<&'a str>,
    pub kind: RefKind,
    pub confidence: Confidence,
}

impl<'a> GraphView<'a> {
    pub(crate) fn new(
        files: &'a [FileNode],
        symbols: &'a [SymbolNode],
        file_index: &'a HashMap<ProjectPath, FileId>,
        packages: &'a [crate::graph::PackageNode],
        edges: &'a [crate::vocab::Edge],
    ) -> Self {
        let mut symbols_by_file: HashMap<FileId, Vec<u32>> = HashMap::default();
        for (i, s) in symbols.iter().enumerate() {
            symbols_by_file.entry(s.file).or_default().push(i as u32);
        }
        GraphView {
            files,
            symbols,
            file_index,
            packages,
            edges,
            symbols_by_file,
            import_index: RefCell::new(None),
        }
    }

    /// Every claimed and unclaimed file discovered this run, in `FileId` order (path-sorted —
    /// the same order every other deterministic pass over `files` uses).
    pub fn files(&self) -> impl Iterator<Item = &'a FileNode> + '_ {
        self.files.iter()
    }

    /// This file's declarations, in extraction order — empty for an unknown or unclaimed path.
    pub fn symbols_in(&self, path: &ProjectPath) -> impl Iterator<Item = &'a SymbolNode> + '_ {
        let indices = self
            .file_index
            .get(path)
            .and_then(|id| self.symbols_by_file.get(id))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        indices.iter().map(|&i| &self.symbols[i as usize])
    }

    /// Package topology (RFC 0011 §3, plugin-visible per RFC 0017 §5.2), in `PackageId`
    /// order: the implicit package first, then one entry per manifest.
    pub fn packages(&self) -> impl Iterator<Item = PackageView<'a>> + '_ {
        self.packages.iter().map(package_view)
    }

    /// The package owning `path` (ownership is total — RFC 0011 §3's
    /// nearest-manifest-ancestor rule); `None` only for a path not in this run's file set.
    pub fn package_of(&self, path: &ProjectPath) -> Option<PackageView<'a>> {
        let file = *self.file_index.get(path)?;
        self.packages
            .get(self.files[file.0 as usize].package.0 as usize)
            .map(package_view)
    }

    /// Files `path` imports (`ImportsFile` edges, adapter-derived only — rule R1), sorted by
    /// path. Empty for an unknown path or a file with no imports.
    pub fn imports_of(&self, path: &ProjectPath) -> Vec<&'a ProjectPath> {
        let Some(&file) = self.file_index.get(path) else {
            return Vec::new();
        };
        self.with_import_index(|idx| {
            idx.imports_of
                .get(&file)
                .map(|targets| {
                    targets
                        .iter()
                        .map(|t| &self.files[t.0 as usize].path)
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// Files importing `path` — [`Self::imports_of`]'s reverse, same rules.
    pub fn importers_of(&self, path: &ProjectPath) -> Vec<&'a ProjectPath> {
        let Some(&file) = self.file_index.get(path) else {
            return Vec::new();
        };
        self.with_import_index(|idx| {
            idx.importers_of
                .get(&file)
                .map(|sources| {
                    sources
                        .iter()
                        .map(|s| &self.files[s.0 as usize].path)
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// Every adapter-derived reference site targeting `path`'s symbol named `symbol` (bare
    /// name), sorted by referencing path then symbol. Linear over the edge list — reference
    /// queries are rare per round and a full reverse index over References edges would cost
    /// every round what only some plugins use; revisit with a real hot consumer.
    pub fn references_to(&self, path: &ProjectPath, symbol: &str) -> Vec<RefSiteView<'a>> {
        let Some(&file) = self.file_index.get(path) else {
            return Vec::new();
        };
        let Some(target) = self
            .symbols_by_file
            .get(&file)
            .and_then(|indices| {
                indices
                    .iter()
                    .find(|&&i| self.symbols[i as usize].name.as_str() == symbol)
            })
            .copied()
        else {
            return Vec::new();
        };
        let mut sites: Vec<RefSiteView<'a>> = self
            .edges
            .iter()
            .filter(|e| matches!(e.source, crate::vocab::Provenance::Adapter(_)))
            .filter_map(|e| match e.kind {
                crate::vocab::EdgeKind::References { from, to, kind } if to.0 == target => {
                    let (from_path, from_symbol) = match from {
                        crate::vocab::NodeRef::File(f) => (&self.files[f.0 as usize].path, None),
                        crate::vocab::NodeRef::Symbol(s) => {
                            let sym = &self.symbols[s.0 as usize];
                            (
                                &self.files[sym.file.0 as usize].path,
                                Some(sym.name.as_str()),
                            )
                        }
                    };
                    Some(RefSiteView {
                        from_path,
                        from_symbol,
                        kind,
                        confidence: e.confidence,
                    })
                }
                _ => None,
            })
            .collect();
        sites.sort_by(|a, b| (a.from_path, a.from_symbol).cmp(&(b.from_path, b.from_symbol)));
        sites
    }

    /// `path`'s string-literal call sites ([`crate::adapter::FileFacts::string_call_args`],
    /// RFC 0017 §5.4), canonically sorted; empty for an unknown path or an adapter that
    /// doesn't extract them.
    pub fn string_call_sites_in(&self, path: &ProjectPath) -> &'a [crate::adapter::StringCallArg] {
        self.file_index
            .get(path)
            .map(|f| self.files[f.0 as usize].string_call_sites.as_slice())
            .unwrap_or(&[])
    }

    /// Every adapter-derived `ImportsFile` edge, fully named, in edge-list order — the bulk
    /// form host bridges use to snapshot the graph before instantiating a guest (wasm-abi
    /// §5.3's borrow constraint); per-path queries stay on [`Self::imports_of`]. Rule R1
    /// applies here too.
    pub fn all_import_edges(
        &self,
    ) -> impl Iterator<Item = (&'a ProjectPath, &'a ProjectPath)> + '_ {
        self.edges
            .iter()
            .filter(|e| matches!(e.source, crate::vocab::Provenance::Adapter(_)))
            .filter_map(|e| match e.kind {
                crate::vocab::EdgeKind::ImportsFile { from, to } => Some((
                    &self.files[from.0 as usize].path,
                    &self.files[to.0 as usize].path,
                )),
                _ => None,
            })
    }

    /// Every adapter-derived reference edge as `(target path, target bare name, site)` — the
    /// bulk counterpart of [`Self::references_to`], same bridge rationale as
    /// [`Self::all_import_edges`].
    pub fn all_reference_sites(
        &self,
    ) -> impl Iterator<Item = (&'a ProjectPath, &'a str, RefSiteView<'a>)> + '_ {
        self.edges
            .iter()
            .filter(|e| matches!(e.source, crate::vocab::Provenance::Adapter(_)))
            .filter_map(|e| match e.kind {
                crate::vocab::EdgeKind::References { from, to, kind } => {
                    let target = &self.symbols[to.0 as usize];
                    let (from_path, from_symbol) = match from {
                        crate::vocab::NodeRef::File(f) => (&self.files[f.0 as usize].path, None),
                        crate::vocab::NodeRef::Symbol(s) => {
                            let sym = &self.symbols[s.0 as usize];
                            (
                                &self.files[sym.file.0 as usize].path,
                                Some(sym.name.as_str()),
                            )
                        }
                    };
                    Some((
                        &self.files[target.file.0 as usize].path,
                        target.name.as_str(),
                        RefSiteView {
                            from_path,
                            from_symbol,
                            kind,
                            confidence: e.confidence,
                        },
                    ))
                }
                _ => None,
            })
    }

    fn with_import_index<R>(&self, f: impl FnOnce(&ImportIndex) -> R) -> R {
        let mut slot = self.import_index.borrow_mut();
        let idx = slot.get_or_insert_with(|| build_import_index(self.files, self.edges));
        f(idx)
    }
}

/// See [`GraphView::with_import_index`]: forward + reverse `ImportsFile` adjacency from the
/// adapter-derived edges (rule R1), targets path-sorted and deduplicated so results are
/// deterministic regardless of edge-list order (the full build hands the view a
/// pre-canonical-sort list; the patch path a sorted one).
fn build_import_index(files: &[FileNode], edges: &[crate::vocab::Edge]) -> ImportIndex {
    let mut imports_of: HashMap<FileId, Vec<FileId>> = HashMap::default();
    let mut importers_of: HashMap<FileId, Vec<FileId>> = HashMap::default();
    for (from, to) in adapter_import_pairs(edges) {
        imports_of.entry(from).or_default().push(to);
        importers_of.entry(to).or_default().push(from);
    }
    for ids in imports_of.values_mut() {
        sort_ids_by_path(files, ids);
    }
    for ids in importers_of.values_mut() {
        sort_ids_by_path(files, ids);
    }
    ImportIndex {
        imports_of,
        importers_of,
    }
}

/// The adapter-derived (rule R1) `ImportsFile` pairs in `edges`, edge-list order.
fn adapter_import_pairs(
    edges: &[crate::vocab::Edge],
) -> impl Iterator<Item = (FileId, FileId)> + '_ {
    edges
        .iter()
        .filter(|e| matches!(e.source, crate::vocab::Provenance::Adapter(_)))
        .filter_map(|e| match e.kind {
            crate::vocab::EdgeKind::ImportsFile { from, to } => Some((from, to)),
            _ => None,
        })
}

fn sort_ids_by_path(files: &[FileNode], ids: &mut Vec<FileId>) {
    ids.sort_by(|a, b| files[a.0 as usize].path.cmp(&files[b.0 as usize].path));
    ids.dedup();
}

fn package_view(p: &crate::graph::PackageNode) -> PackageView<'_> {
    PackageView {
        manifest: p.manifest.as_ref(),
        name: p.name.as_deref(),
        root_dir: p
            .manifest
            .as_ref()
            .map(|m| {
                let s = m.0.as_str();
                s.rfind('/').map(|i| &s[..i]).unwrap_or("")
            })
            .unwrap_or(""),
    }
}

// ---------------------------------------------------------------- read side: ContentView

/// Per-run cap on distinct paths a single component may read through the content channel
/// (RFC 0016 §5) — generous for the channel's stated scope (configs, manifests, templates,
/// never source: reading language-graph files through this side door is a review-level
/// boundary the channel doesn't mechanically enforce, but no shipped consumer does it), tight
/// enough that a component can't use it to walk the whole project file by file.
const CONTENT_MAX_FILES: usize = 200;
/// Per-run cap on total bytes read through the content channel by a single component —
/// generous for text configs/manifests/templates, small next to the 500 ms analysis budget's
/// own I/O.
const CONTENT_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
struct ContentBudget {
    // Keyed by path, not a call counter: a component's read scope shouldn't depend on how
    // many hooks look at the same file — charging by first-seen path makes the budget mean
    // what its doc comment says, distinct paths. (Historically this also compensated the
    // WASM bridge's instance-per-hook triple-fetch; RFC 0017 §4's one-instance-per-round
    // lifecycle removed that motivation, and the keying stays on its own merits.)
    seen: rustc_hash::FxHashSet<ProjectPath>,
    bytes_read: usize,
    cut_off: bool,
}

/// Host-mediated, read-only access to file content *outside* the language graph — configs,
/// manifests, templates (RFC 0016 §5's content channel). Scoped to the calling component's own
/// `PluginDescriptor::requested_file_access` globs: a path outside the declared set is treated
/// exactly like one that doesn't exist, the same "declare then get" contract the WASM fuel
/// budget and `ingest_coverage`'s well-known-path scoping already use elsewhere in this trait.
/// Metered by [`CONTENT_MAX_FILES`]/[`CONTENT_MAX_BYTES`]: once exceeded, every further read
/// this run returns `None` and one diagnostic records why — "degrade to silence, never crash
/// the run," the same posture the WASM per-call fuel budget already established (RFC 0003 §3).
///
/// Source-blind by construction (borrows [`crate::discovery::DiscoveredTree`], the same reader
/// extraction itself uses): identical behavior whether this run's source is a real directory or
/// an in-memory git tree (RFC 0004 §6's diff modes), with no second disk walk of its own — glob
/// matching runs in memory against paths this run already discovered.
pub struct ContentView<'a> {
    tree: &'a crate::discovery::DiscoveredTree,
    globs: Vec<glob::Pattern>,
    component_id: SmolStr,
    budget: RefCell<ContentBudget>,
    diagnostic: RefCell<Option<Diagnostic>>,
}

impl<'a> ContentView<'a> {
    pub(crate) fn new(
        tree: &'a crate::discovery::DiscoveredTree,
        component_id: SmolStr,
        requested_file_access: &[SmolStr],
    ) -> Self {
        // An unparsable glob is dropped silently rather than erroring the whole component —
        // same "malformed input degrades, never aborts" posture as everything else a component
        // declares (an activation rule that fails to compile is likewise just never satisfied).
        let globs = requested_file_access
            .iter()
            .filter_map(|g| glob::Pattern::new(g.as_str()).ok())
            .collect();
        ContentView {
            tree,
            globs,
            component_id,
            budget: RefCell::new(ContentBudget::default()),
            diagnostic: RefCell::new(None),
        }
    }

    /// Read one file's bytes, if it matches this component's declared globs and the run's
    /// content budget isn't exhausted. Every miss — no glob match, an unreadable/missing path,
    /// or a budget cutoff — is indistinguishable to the caller as `None`, matching every other
    /// host-mediated lookup's miss behavior in this trait (an adapter's own claim miss, a
    /// `PluginTarget` that doesn't resolve).
    pub fn read(&self, path: &ProjectPath) -> Option<Vec<u8>> {
        if !self.globs.iter().any(|g| g.matches(path.0.as_str())) {
            return None;
        }
        if self.already_seen(path) {
            // A previously charged path is served again for free (no rebudgeting on repeat
            // access) even after a cutoff — the cutoff is about *new* reads, not about
            // punishing a caller for asking twice.
            return self.tree.read(path).ok();
        }
        if self.budget.borrow().cut_off {
            return None;
        }
        let bytes = self.tree.read(path).ok()?;
        self.charge(path, bytes.len());
        Some(bytes)
    }

    fn already_seen(&self, path: &ProjectPath) -> bool {
        self.budget.borrow().seen.contains(path)
    }

    /// Records a fresh path against the budget and flips `cut_off` (plus the one diagnostic)
    /// the moment either cap is crossed. Called only for a path not yet in `seen`, with the
    /// byte count of a read that already happened — never re-reads to find out.
    fn charge(&self, path: &ProjectPath, len: usize) {
        let mut budget = self.budget.borrow_mut();
        budget.seen.insert(path.clone());
        budget.bytes_read += len;
        let over_budget =
            budget.seen.len() > CONTENT_MAX_FILES || budget.bytes_read > CONTENT_MAX_BYTES;
        budget.cut_off |= over_budget;
        drop(budget);
        if over_budget {
            self.record_cutoff();
        }
    }

    /// Every discovered path matching this component's declared globs, in path order. Native
    /// components don't need this (they already know the one path they want — `package.json`
    /// relative to a known app root — and call [`read`](Self::read) directly); it exists for
    /// the WASM tier's bridge, which must prefetch every matching path into an owned snapshot
    /// before instantiating a guest that can't make host round-trips of its own choosing
    /// mid-call (`kndo-plugin-api`'s `HostViewData`, mirroring how it already snapshots
    /// `list-files`/`symbols-in`).
    pub fn matching_paths(&self) -> impl Iterator<Item = &ProjectPath> + '_ {
        self.tree
            .files
            .iter()
            .map(|f| &f.path)
            .filter(move |p| self.globs.iter().any(|g| g.matches(p.0.as_str())))
    }

    fn record_cutoff(&self) {
        let mut diagnostic = self.diagnostic.borrow_mut();
        if diagnostic.is_none() {
            *diagnostic = Some(Diagnostic {
                level: DiagnosticLevel::Warn,
                path: None,
                message: format!(
                    "plugin {} exceeded its content-channel budget ({CONTENT_MAX_FILES} files \
                     / {CONTENT_MAX_BYTES} bytes this run) — further content reads return \
                     nothing for the rest of this run",
                    self.component_id
                ),
                span: None,
            });
        }
    }

    /// Drains the one budget-cutoff diagnostic, if this run tripped it — called once per
    /// component per assembly, after its hooks have all run.
    pub(crate) fn take_diagnostic(&self) -> Option<Diagnostic> {
        self.diagnostic.borrow_mut().take()
    }
}

// ---------------------------------------------------------------- write side: sinks

/// A named target a plugin contributes to: a file, or (with `symbol`) one of its declarations —
/// bare name, or `Owner.name` for a member, mirroring [`crate::adapter::Declaration::name`] +
/// `member_of`'s display convention. Resolved core-side against the same bare/qualified tables
/// `RawRoot`/`RawReference` resolve against; an unresolvable target is dropped silently.
#[derive(Debug, Clone)]
pub struct PluginTarget {
    pub path: ProjectPath,
    pub symbol: Option<SmolStr>,
}

impl PluginTarget {
    pub fn file(path: ProjectPath) -> Self {
        PluginTarget { path, symbol: None }
    }

    pub fn symbol(path: ProjectPath, name: impl Into<SmolStr>) -> Self {
        PluginTarget {
            path,
            symbol: Some(name.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ContributedRoot {
    pub target: PluginTarget,
    pub kind: RootKind,
    pub confidence: Confidence,
}

/// Framework entry points (RFC 0003 §2): routes, DI-registered beans, handlers — anything a
/// plugin knows is invoked by the ecosystem even though nothing in-repo calls it. Same
/// `EdgeKind::Root` mechanism adapter-emitted `RawRoot`s use, attributed `Provenance::Plugin`.
#[derive(Debug, Default)]
pub struct RootSink {
    pub(crate) items: Vec<ContributedRoot>,
}

impl RootSink {
    pub fn add(&mut self, target: PluginTarget, kind: RootKind, confidence: Confidence) {
        self.items.push(ContributedRoot {
            target,
            kind,
            confidence,
        });
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ContributedEdge {
    pub from: PluginTarget,
    pub to: PluginTarget,
    pub kind: RefKind,
    pub confidence: Confidence,
}

/// Edges invisible to the language (RFC 0003 §2): DI wiring, route-string → handler, template →
/// class, CSS class names used from HTML templates. A symbol-target `to` becomes a
/// `References` edge; a file-target `to` (no `symbol` set) becomes RFC 0017 §5.4's
/// file-liveness edge (`EdgeKind::ReferencesFile` — "if `from` is alive, that file is in
/// use", the template/asset shape; the `kind` argument doesn't apply there and is ignored).
/// Either way plugins can't mint new edge kinds (RFC 0003 §2: "cannot define new node/edge
/// kinds") — the two mappings above are the whole vocabulary, and plugin edges are liveness
/// evidence only: `cyclic` and every analysis that would *create* a finding from an edge's
/// existence ignore them (RFC 0005).
#[derive(Debug, Default)]
pub struct EdgeSink {
    pub(crate) items: Vec<ContributedEdge>,
}

impl EdgeSink {
    pub fn add(
        &mut self,
        from: PluginTarget,
        to: PluginTarget,
        kind: RefKind,
        confidence: Confidence,
    ) {
        self.items.push(ContributedEdge {
            from,
            to,
            kind,
            confidence,
        });
    }
}

/// Marks a symbol externally consumed (RFC 0003 §2: public SDK surface, FFI, serialization
/// targets) — the exemption `internal-only`/`private-type-leak` already document as available
/// "via `annotate_symbols`" (RFC 0005 §7) but had nothing to read until this landed.
#[derive(Debug, Default)]
pub struct AnnotationSink {
    pub(crate) externally_consumed: Vec<PluginTarget>,
}

impl AnnotationSink {
    pub fn mark_externally_consumed(&mut self, path: ProjectPath, symbol: impl Into<SmolStr>) {
        self.externally_consumed
            .push(PluginTarget::symbol(path, symbol));
    }
}

/// What one plugin actually landed in the graph during a round (RFC 0017 §7's auditability
/// requirement): counts of resolved contributions — sink items whose targets missed resolution
/// are not counted, because they changed nothing. Recorded per round by
/// `graph::run_plugin_round`, persisted as the cache's last-run record, rendered by
/// `kndo doctor` — the observable half of the threat model (a component can only lie about
/// graph facts, and here is exactly what facts it asserted).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PluginContribution {
    pub id: String,
    pub roots: u32,
    pub edges: u32,
    pub annotations: u32,
    /// Sink items whose targets did NOT resolve, described one per line — the miss itself
    /// stays a silent no-op in the graph (the [`PluginTarget`] contract), but the *author*
    /// debugging "contributed 0 roots" needs the why, and `kndo plugin verify` prints these.
    /// Capped per round (`graph::DROPPED_CAP`) so a pathological plugin can't bloat the
    /// record.
    #[serde(default)]
    pub dropped: Vec<String>,
}

// ---------------------------------------------------------------- findings (RFC 0018)

/// A plugin rule's declared severity (RFC 0018 §2.3) — deliberately its own enum, not
/// `engine::Severity`: the engine maps it, applying the advisory-channel rules (§2.2), and
/// keeping the vocabularies separate means a plugin can never *construct* a gate-eligible
/// severity directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSeverity {
    Error,
    Warning,
    Info,
}

/// One rule a plugin may emit findings under, declared up front (RFC 0018 §4) so `kndo
/// doctor`/`kndo plugin verify` can show what a component *may* assert before it runs, and so
/// the gate config can be validated against real names. A finding emitted under an undeclared
/// rule is dropped with a diagnostic — declaration is the contract, not decoration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDescriptor {
    /// Lower-kebab (`[a-z0-9-]+`); becomes the suffix of the namespaced category
    /// `plugin:<coordinate>/<rule>`.
    pub name: SmolStr,
    pub description: SmolStr,
    /// The severity every finding under this rule carries — one severity per rule, declared
    /// once, never chosen per finding (RFC 0018 §2.3). Config can cap it lower, never raise.
    pub severity: PluginSeverity,
}

/// Sink for [`Plugin::contribute_findings`] (RFC 0018 §4): third-party verdicts, targeted at
/// graph nodes. Severity is NOT a parameter — it comes from the rule's declaration.
#[derive(Debug, Default)]
pub struct FindingSink {
    pub(crate) items: Vec<ContributedFindingItem>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContributedFindingItem {
    pub rule: SmolStr,
    pub target: PluginTarget,
    pub confidence: Confidence,
    pub message: String,
}

impl FindingSink {
    pub fn add(
        &mut self,
        rule: impl Into<SmolStr>,
        target: PluginTarget,
        confidence: Confidence,
        message: impl Into<String>,
    ) {
        self.items.push(ContributedFindingItem {
            rule: rule.into(),
            target,
            confidence,
            message: message.into(),
        });
    }
}

/// One resolved, namespaced plugin finding as the finding round (graph.rs) hands it to the
/// engine — everything needed to build an `engine::Finding` except the advisory/gate mapping,
/// which is config-dependent and therefore the engine's job (RFC 0018 §2.2).
#[derive(Debug, Clone)]
pub struct ProtoFinding {
    /// The full namespaced category: `plugin:<coordinate>/<rule>` — assembled host-side from
    /// the plugin's registered id, never guest-supplied (RFC 0018 §2.1).
    pub category: String,
    pub plugin_id: SmolStr,
    pub rule: SmolStr,
    pub severity: PluginSeverity,
    pub confidence: Confidence,
    pub message: String,
    pub path: ProjectPath,
    pub symbol: Option<SmolStr>,
    pub span: Option<crate::adapter::Span>,
    pub subject_kind: String,
    pub package: Option<String>,
}

/// A valid rule name: lower-kebab, non-empty (RFC 0018 §2.1's charset).
pub fn is_valid_rule_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// ---------------------------------------------------------------- the trait

pub trait Plugin: Send + Sync {
    fn descriptor(&self) -> PluginDescriptor;

    /// Whether this plugin participates in graph assembly at all — i.e. implements any of the
    /// four graph-mutation hooks (`classify_file`/`contribute_roots`/`contribute_edges`/
    /// `annotate_symbols`). Load-bearing for performance, not a hint: any registered
    /// graph-mutating plugin forces `graph::assemble_from_source` to bypass the incremental
    /// patch (RFC 0013's patch never re-invokes plugin hooks, so it can't safely reuse a graph
    /// one influenced — RFC 0016 §6 landed the snapshot half of this differently: plugin
    /// identity now folds into the graph cache key, §6's own reasoning, so the snapshot fast
    /// path stays available — any input a plugin's hooks could react to, including everything
    /// its content channel might read, was already part of the key before this trait method
    /// existed). A plugin that only implements `ingest_coverage`/`suppress` (like
    /// [`LcovPlugin`]) must still return `false` here, or its mere registration keeps the
    /// incremental patch off for the whole product even though it never touches the graph.
    /// The declaration is self-enforcing rather than trusted: assembly only *calls* the four
    /// hooks on plugins that return `true`, so returning `false` while implementing a hook
    /// means the hook never runs (identically on cold and cached runs) — never that a cached
    /// graph silently misses its contributions. Default `true`: the conservative direction for
    /// the common case of a plugin that exists precisely to contribute graph facts.
    fn mutates_graph(&self) -> bool {
        true
    }

    /// Content identity for the graph cache key (RFC 0016 §6). `None` for compiled-in
    /// plugins — `PluginDescriptor.version` is already the trust boundary there, the same
    /// discipline `AdapterDescriptor.facts_schema_version` established: the author bumps it
    /// when behavior changes, and a new kndo binary release is what ships that change anyway.
    /// A WASM plugin overrides this to the content hash of its own component bytes, because a
    /// `.wasm` file can be swapped in `.kndo/plugins/` (or the global directory) with no
    /// version bump at all — id+version alone would silently miss exactly the case this exists
    /// to catch.
    fn content_hash(&self) -> Option<[u8; 32]> {
        None
    }

    /// Adjust a file's role/origin beyond language defaults (e.g. `*.stories.tsx` → tooling).
    /// Runs once per claimed file, right after RFC 0012 §7's content-derived origin correction
    /// and before role-derived roots (phase 2.6) — so a plugin's answer is what every downstream
    /// consumer (root promotion, `unused`/`test-only`'s per-file exemptions) sees.
    fn classify_file(&self, _path: &ProjectPath, _current: FileClass) -> Option<FileClass> {
        None
    }

    /// Framework entry points: routes, DI-registered beans, handlers… `content` is the RFC
    /// 0016 §5 host-mediated channel, scoped to this plugin's own declared
    /// `requested_file_access` globs — configs, manifests, templates the language graph itself
    /// never sees (`package.json` scripts, `next.config.*`, `views/**`); reading source files
    /// the graph already covers through this side door is out of contract even though nothing
    /// here stops it mechanically (spec'd per plugin, e.g. docs/plugins/*.md).
    fn contribute_roots(
        &self,
        _graph: &GraphView<'_>,
        _content: &ContentView<'_>,
        _out: &mut RootSink,
    ) {
    }

    /// Edges invisible to the language: DI wiring, template → class, route → handler… See
    /// [`contribute_roots`](Self::contribute_roots) for `content`'s scope.
    fn contribute_edges(
        &self,
        _graph: &GraphView<'_>,
        _content: &ContentView<'_>,
        _out: &mut EdgeSink,
    ) {
    }

    /// Mark symbols externally consumed (FFI, serialization targets, public SDK surface). See
    /// [`contribute_roots`](Self::contribute_roots) for `content`'s scope.
    fn annotate_symbols(
        &self,
        _graph: &GraphView<'_>,
        _content: &ContentView<'_>,
        _out: &mut AnnotationSink,
    ) {
    }

    /// The rules this plugin may emit findings under (RFC 0018 §4) — declared up front, once,
    /// so tooling can show them before any hook runs and so an emitted rule name can be
    /// validated. Default empty: a plugin with no rules never has `contribute_findings`
    /// called, and costs the finding round nothing.
    fn rules(&self) -> Vec<RuleDescriptor> {
        Vec::new()
    }

    /// Emit third-party findings (RFC 0018) — verdicts, not graph facts. NOT a graph-mutation
    /// hook: it runs *after* assembly on every path (cold, patch, warm snapshot hit), reads
    /// the same R1-scoped `GraphView`/`ContentView` the mutation hooks see, and its output
    /// lands in the run's findings under the namespaced category
    /// `plugin:<coordinate>/<rule>` on the advisory severity channel (§2.2) — it can never
    /// touch the graph, core findings, or (without explicit user opt-in) the exit code.
    fn contribute_findings(
        &self,
        _graph: &GraphView<'_>,
        _content: &ContentView<'_>,
        _out: &mut FindingSink,
    ) {
    }

    /// Parse one coverage report into per-file line coverage (ADR 0005, RFC 0003 §2's
    /// `ingest_coverage` hook). Content arrives via the host — the report was matched by this
    /// plugin's `descriptor().requested_file_access` globs and freshness-checked before this
    /// is called; no ambient fs. Paths inside the report must be normalized to
    /// project-relative form by the plugin (it alone knows the format's path conventions).
    fn ingest_coverage(
        &self,
        _path: &ProjectPath,
        _content: &[u8],
        _out: &mut crate::coverage::CoverageSink,
    ) {
    }
}

/// The built-in lcov ingester (ADR 0005's first launch format — the lingua franca:
/// jest/vitest/nyc, llvm-cov, gcov, Go via converters). Statically linked, same trait external
/// WASM plugins implement (RFC 0003: "the same trait serves built-ins").
pub struct LcovPlugin;

impl Plugin for LcovPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            // RFC 0015 §2: built-ins live in the reserved namespace (migrated from the plain
            // "coverage-lcov" this plugin carried pre-RFC-0015).
            id: SmolStr::new("kndo:coverage-lcov"),
            version: SmolStr::new("1"),
            detection: vec![SmolStr::new("an lcov.info file at a well-known path")],
            // Well-known locations (ADR 0005: "located by config or well-known paths");
            // config-based locations land with the config parser.
            requested_file_access: vec![
                SmolStr::new("coverage/lcov.info"),
                SmolStr::new("lcov.info"),
            ],
            activation: vec![
                ActivationRule::FileExists(SmolStr::new("coverage/lcov.info")),
                ActivationRule::FileExists(SmolStr::new("lcov.info")),
            ],
            dependencies: vec![],
        }
    }

    /// Coverage ingestion only — no graph-mutation hooks. Without this override, this plugin's
    /// unconditional registration in `default_plugins()` would force every real `kndo` run to
    /// bypass the graph-snapshot cache and the incremental patch (see the trait method's doc) —
    /// which is exactly the bug this override fixed: both fast paths were silently dead in the
    /// shipped product from the day the graph hooks were wired until this landed.
    fn mutates_graph(&self) -> bool {
        false
    }

    /// The lcov subset that matters: `SF:<path>` opens a file section, `DA:<line>,<hits>`
    /// records one instrumented line, `end_of_record` closes it — everything else (function/
    /// branch records, checksums) is ignored, since kndo maps lines to functions itself via
    /// symbol spans. Absolute `SF:` paths are relativized when they contain the project's
    /// layout; ones that can't be are kept verbatim and simply match nothing — degrade to
    /// silence, never to a wrong file.
    fn ingest_coverage(
        &self,
        _path: &ProjectPath,
        content: &[u8],
        out: &mut crate::coverage::CoverageSink,
    ) {
        let Ok(text) = std::str::from_utf8(content) else {
            return;
        };
        let mut current: Option<ProjectPath> = None;
        for line in text.lines() {
            let line = line.trim_end();
            if let Some(sf) = line.strip_prefix("SF:") {
                let normalized = sf.trim().trim_start_matches("./").replace('\\', "/");
                current = Some(ProjectPath(SmolStr::new(normalized)));
            } else if let Some(da) = line.strip_prefix("DA:") {
                if let Some(file) = &current {
                    let mut parts = da.splitn(3, ',');
                    let line_no = parts.next().and_then(|s| s.trim().parse::<u32>().ok());
                    let hits = parts.next().and_then(|s| s.trim().parse::<u64>().ok());
                    if let (Some(line_no), Some(hits)) = (line_no, hits) {
                        out.add_line(file.clone(), line_no, hits);
                    }
                }
            } else if line == "end_of_record" {
                current = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::VisibilityLevel;
    use crate::vocab::{FileOrigin, FileRole, PackageId, SymbolKind};

    fn file(path: &str) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass {
                role: FileRole::Production,
                origin: FileOrigin::Authored,
            }),
            package: PackageId(0),
            unit: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
        }
    }

    fn symbol(file: FileId, name: &str) -> SymbolNode {
        SymbolNode {
            file,
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: crate::adapter::Span::default(),
            exported: true,
            visibility: VisibilityLevel(0),
            member_of: None,
            signature_span: None,
        }
    }

    #[test]
    fn symbols_in_returns_only_that_files_declarations_in_extraction_order() {
        let files = [file("a.mock"), file("b.mock")];
        let symbols = [
            symbol(FileId(0), "aFirst"),
            symbol(FileId(1), "bOnly"),
            symbol(FileId(0), "aSecond"),
        ];
        let mut file_index = HashMap::default();
        for (i, f) in files.iter().enumerate() {
            file_index.insert(f.path.clone(), FileId(i as u32));
        }

        let view = GraphView::new(&files, &symbols, &file_index, &[], &[]);

        let a_names: Vec<&str> = view
            .symbols_in(&ProjectPath(SmolStr::new("a.mock")))
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(a_names, vec!["aFirst", "aSecond"]);

        let b_names: Vec<&str> = view
            .symbols_in(&ProjectPath(SmolStr::new("b.mock")))
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(b_names, vec!["bOnly"]);
    }

    #[test]
    fn symbols_in_is_empty_for_an_unknown_path() {
        let files = [file("a.mock")];
        let symbols = [symbol(FileId(0), "aFirst")];
        let mut file_index = HashMap::default();
        file_index.insert(files[0].path.clone(), FileId(0));

        let view = GraphView::new(&files, &symbols, &file_index, &[], &[]);
        assert_eq!(
            view.symbols_in(&ProjectPath(SmolStr::new("nowhere.mock")))
                .count(),
            0
        );
    }

    #[test]
    fn files_iterates_every_discovered_file() {
        let files = [file("a.mock"), file("b.mock")];
        let symbols: [SymbolNode; 0] = [];
        let file_index = HashMap::default();

        let view = GraphView::new(&files, &symbols, &file_index, &[], &[]);
        let paths: Vec<&str> = view.files().map(|f| f.path.0.as_str()).collect();
        assert_eq!(paths, vec!["a.mock", "b.mock"]);
    }

    // -------------------------------------------------- GraphView v2 (RFC 0017 §5)

    fn edge(kind: crate::vocab::EdgeKind, source: crate::vocab::Provenance) -> crate::vocab::Edge {
        crate::vocab::Edge {
            kind,
            confidence: Confidence::Certain,
            source,
            span: None,
            owner: FileId(0),
        }
    }

    fn adapter_import(from: u32, to: u32) -> crate::vocab::Edge {
        edge(
            crate::vocab::EdgeKind::ImportsFile {
                from: FileId(from),
                to: FileId(to),
            },
            crate::vocab::Provenance::Adapter(SmolStr::new("mock")),
        )
    }

    #[test]
    fn package_topology_is_queryable_and_ownership_is_total() {
        let mut files = [file("a.mock"), file("pkg/b.mock")];
        files[1].package = crate::vocab::PackageId(1);
        let implicit = crate::graph::PackageNode {
            manifest: None,
            name: None,
            private: false,
            declares_surface: false,
            surface: Vec::new(),
            workspace_entry: None,
            resolves_dependency_usage: true,
        };
        let mut real = implicit.clone();
        real.manifest = Some(ProjectPath(SmolStr::new("pkg/package.json")));
        real.name = Some(SmolStr::new("pkg"));
        let packages = [implicit, real];
        let mut file_index = HashMap::default();
        for (i, f) in files.iter().enumerate() {
            file_index.insert(f.path.clone(), FileId(i as u32));
        }
        let view = GraphView::new(&files, &[], &file_index, &packages, &[]);

        let pkgs: Vec<_> = view.packages().collect();
        assert_eq!(pkgs.len(), 2);
        assert_eq!(
            pkgs[0].root_dir, "",
            "implicit package roots at the project root"
        );
        assert_eq!(pkgs[1].root_dir, "pkg");
        assert_eq!(pkgs[1].name, Some("pkg"));

        let owner = view
            .package_of(&ProjectPath(SmolStr::new("pkg/b.mock")))
            .expect("ownership is total");
        assert_eq!(owner.root_dir, "pkg");
        let implicit = view
            .package_of(&ProjectPath(SmolStr::new("a.mock")))
            .expect("ownership is total");
        assert_eq!(implicit.manifest, None);
        assert!(view
            .package_of(&ProjectPath(SmolStr::new("nowhere.mock")))
            .is_none());
    }

    #[test]
    fn import_queries_answer_both_directions_sorted_and_adapter_only() {
        let files = [file("a.mock"), file("b.mock"), file("c.mock")];
        let mut file_index = HashMap::default();
        for (i, f) in files.iter().enumerate() {
            file_index.insert(f.path.clone(), FileId(i as u32));
        }
        let edges = [
            adapter_import(2, 0), // c -> a
            adapter_import(1, 0), // b -> a
            // Rule R1: another plugin's contributed file edge must be invisible.
            edge(
                crate::vocab::EdgeKind::ReferencesFile {
                    from: crate::vocab::NodeRef::File(FileId(0)),
                    to: FileId(1),
                },
                crate::vocab::Provenance::Plugin(SmolStr::new("other-plugin")),
            ),
        ];
        let view = GraphView::new(&files, &[], &file_index, &[], &edges);

        let importers: Vec<&str> = view
            .importers_of(&ProjectPath(SmolStr::new("a.mock")))
            .into_iter()
            .map(|p| p.0.as_str())
            .collect();
        assert_eq!(importers, vec!["b.mock", "c.mock"], "sorted by path");

        let imports: Vec<&str> = view
            .imports_of(&ProjectPath(SmolStr::new("b.mock")))
            .into_iter()
            .map(|p| p.0.as_str())
            .collect();
        assert_eq!(imports, vec!["a.mock"]);

        assert!(
            view.imports_of(&ProjectPath(SmolStr::new("a.mock")))
                .is_empty(),
            "the plugin-contributed edge must not answer (rule R1)"
        );
    }

    #[test]
    fn references_to_names_the_sites_and_ignores_plugin_edges() {
        let files = [file("a.mock"), file("b.mock")];
        let symbols = [symbol(FileId(0), "target"), symbol(FileId(1), "caller")];
        let mut file_index = HashMap::default();
        for (i, f) in files.iter().enumerate() {
            file_index.insert(f.path.clone(), FileId(i as u32));
        }
        let edges = [
            edge(
                crate::vocab::EdgeKind::References {
                    from: crate::vocab::NodeRef::Symbol(crate::vocab::SymbolId(1)),
                    to: crate::vocab::SymbolId(0),
                    kind: RefKind::Call,
                },
                crate::vocab::Provenance::Adapter(SmolStr::new("mock")),
            ),
            edge(
                crate::vocab::EdgeKind::References {
                    from: crate::vocab::NodeRef::Symbol(crate::vocab::SymbolId(1)),
                    to: crate::vocab::SymbolId(0),
                    kind: RefKind::Read,
                },
                crate::vocab::Provenance::Plugin(SmolStr::new("other-plugin")),
            ),
        ];
        let view = GraphView::new(&files, &symbols, &file_index, &[], &edges);

        let sites = view.references_to(&ProjectPath(SmolStr::new("a.mock")), "target");
        assert_eq!(sites.len(), 1, "the plugin edge is invisible (rule R1)");
        assert_eq!(sites[0].from_path.0.as_str(), "b.mock");
        assert_eq!(sites[0].from_symbol, Some("caller"));
        assert_eq!(sites[0].kind, RefKind::Call);
        assert!(view
            .references_to(&ProjectPath(SmolStr::new("a.mock")), "missing")
            .is_empty());
    }

    #[test]
    fn string_call_sites_come_off_the_file_node() {
        let mut files = [file("a.mock")];
        files[0].string_call_sites = vec![crate::adapter::StringCallArg {
            callee: SmolStr::new("res.render"),
            literal: SmolStr::new("index"),
            span: crate::adapter::Span::default(),
        }];
        let mut file_index = HashMap::default();
        file_index.insert(files[0].path.clone(), FileId(0));
        let view = GraphView::new(&files, &[], &file_index, &[], &[]);

        let sites = view.string_call_sites_in(&ProjectPath(SmolStr::new("a.mock")));
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].callee.as_str(), "res.render");
        assert_eq!(sites[0].literal.as_str(), "index");
        assert!(view
            .string_call_sites_in(&ProjectPath(SmolStr::new("nowhere.mock")))
            .is_empty());
    }

    // ------------------------------------------------------------ ContentView (RFC 0016 §5)

    fn content_tree_fixture(
        name: &str,
        files: &[(&str, &str)],
    ) -> crate::discovery::DiscoveredTree {
        let dir = std::env::temp_dir().join(format!("kndo-plugin-content-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (path, content) in files {
            let full = dir.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, content).unwrap();
        }
        crate::discovery::discover_source(
            &crate::discovery::TreeSource::Directory(&dir),
            &HashMap::default(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn content_view_reads_a_path_matching_its_declared_glob() {
        let tree = content_tree_fixture(
            "reads-declared",
            &[("package.json", "{\"main\":\"index.js\"}")],
        );
        let view = ContentView::new(
            &tree,
            SmolStr::new("test-plugin"),
            &[SmolStr::new("package.json")],
        );
        let bytes = view.read(&ProjectPath(SmolStr::new("package.json")));
        assert_eq!(bytes.as_deref(), Some(&b"{\"main\":\"index.js\"}"[..]));
    }

    #[test]
    fn content_view_refuses_a_path_outside_its_declared_globs() {
        let tree = content_tree_fixture(
            "refuses-undeclared",
            &[("package.json", "{}"), ("src/index.js", "code")],
        );
        // Declares only package.json — src/index.js is source the language graph already
        // covers, and the point of scoping is that a plugin can't read it through this door
        // even though the file genuinely exists and is genuinely readable.
        let view = ContentView::new(
            &tree,
            SmolStr::new("test-plugin"),
            &[SmolStr::new("package.json")],
        );
        assert_eq!(view.read(&ProjectPath(SmolStr::new("src/index.js"))), None);
    }

    #[test]
    fn content_view_matching_paths_reflects_the_glob_not_the_whole_tree() {
        let tree = content_tree_fixture(
            "matching-paths",
            &[
                ("views/index.ejs", "a"),
                ("views/about.ejs", "b"),
                ("package.json", "{}"),
            ],
        );
        let view = ContentView::new(
            &tree,
            SmolStr::new("test-plugin"),
            &[SmolStr::new("views/*.ejs")],
        );
        let mut matched: Vec<&str> = view.matching_paths().map(|p| p.0.as_str()).collect();
        matched.sort_unstable();
        assert_eq!(matched, vec!["views/about.ejs", "views/index.ejs"]);
    }

    #[test]
    fn content_view_cuts_off_after_its_file_budget_and_emits_one_diagnostic() {
        let files: Vec<(String, String)> = (0..CONTENT_MAX_FILES + 5)
            .map(|i| (format!("f{i}.marker"), "x".to_string()))
            .collect();
        let file_refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_str()))
            .collect();
        let tree = content_tree_fixture("budget-cutoff", &file_refs);
        let view = ContentView::new(
            &tree,
            SmolStr::new("greedy-plugin"),
            &[SmolStr::new("*.marker")],
        );
        let mut successes = 0;
        for (path, _) in &files {
            if view
                .read(&ProjectPath(SmolStr::new(path.as_str())))
                .is_some()
            {
                successes += 1;
            }
        }
        // Every read up to and including the one that trips the cutoff still succeeds (it had
        // already happened); everything after returns None.
        assert_eq!(successes, CONTENT_MAX_FILES + 1);
        let diagnostic = view
            .take_diagnostic()
            .expect("exceeding the file budget must record one diagnostic");
        assert!(diagnostic.message.contains("greedy-plugin"));
        // A second drain finds nothing left — the diagnostic is emitted exactly once per view.
        assert!(view.take_diagnostic().is_none());
    }

    #[test]
    fn content_view_an_unparsable_glob_is_dropped_not_fatal() {
        let tree = content_tree_fixture("bad-glob", &[("package.json", "{}")]);
        // "[" is an unterminated character class — invalid glob syntax.
        let view = ContentView::new(
            &tree,
            SmolStr::new("test-plugin"),
            &[SmolStr::new("["), SmolStr::new("package.json")],
        );
        assert_eq!(
            view.read(&ProjectPath(SmolStr::new("package.json")))
                .as_deref(),
            Some(&b"{}"[..])
        );
    }
}
