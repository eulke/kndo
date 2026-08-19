//! Project Graph assembly (RFC 0001 §3–4) — turns discovered files into the language-neutral
//! graph via registered adapters. This module is **language-blind** (RFC 0001 §2, the
//! ignorance rule): it references only the `LanguageAdapter` trait, never a concrete
//! language. Adapter *registration* happens at the binary level (`kndo-cli` composes core +
//! first-party adapters) — the core must never know which languages exist.

use std::collections::{HashMap, HashSet};
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

#[derive(Debug, Clone)]
pub struct FileNode {
    pub path: ProjectPath,
    pub content_hash: [u8; 32],
    /// `None` when no registered adapter claims this file — it still exists as a File node
    /// (e.g. a README, or a CSS file before a CSS adapter exists) so import edges *to* it
    /// still resolve, per RFC 0002 §4's cross-language model.
    pub language: Option<SmolStr>,
    pub class: Option<FileClass>,
    /// Every file belongs to exactly one Package (RFC 0011 §3, nearest-manifest-ancestor).
    /// `PackageId(0)` is always the implicit package (see [`ProjectGraph::packages`]) — never
    /// `None`, since ownership is total even when nothing real claims a file.
    pub package: PackageId,
}

#[derive(Debug, Clone)]
pub struct SymbolNode {
    pub file: FileId,
    pub name: SmolStr,
    pub kind: SymbolKind,
    pub span: Span,
    pub exported: bool,
    pub visibility: VisibilityLevel,
}

/// A package consumed *as a dependency* — external (npm/crates.io/…) or an in-repo workspace
/// member imported by name (RFC 0011 §4: the workspace case carries the same
/// declaration-contract obligations, so it lives in the same node kind; its file-level
/// reachability is carried separately by the `ImportsFile` edge the same resolution emits).
#[derive(Debug, Clone)]
pub struct DependencyNode {
    pub name: SmolStr,
}

/// A workspace unit: one manifest + the file tree it governs (RFC 0011 §3). `PackageId(0)` is
/// always the implicit package with `manifest: None` — "a repo with no manifest at all is one
/// implicit Package" generalizes to "whatever no real manifest's subtree claims," so ownership
/// is total (every file has a package) even in a repo with zero manifests, or with manifests
/// that don't cover every directory.
#[derive(Debug, Clone)]
pub struct PackageNode {
    pub manifest: Option<ProjectPath>,
    pub name: Option<SmolStr>,
    /// Publish signal from the manifest — mirrors `ManifestFacts::private` (RFC 0011 §5).
    pub private: bool,
}

/// One manifest's declaration of an external dependency — the raw fact `undeclared` and
/// `version-skew` compare against, kept separate from [`DependencyNode`] because a declaration
/// can exist with zero importers (nothing wrong with that on its own — that's `unused`'s
/// concern) and a project can have many manifests declaring the same name differently (that's
/// `version-skew`'s).
#[derive(Debug, Clone)]
pub struct DeclaredDependency {
    pub package: PackageId,
    pub manifest: ProjectPath,
    pub name: SmolStr,
    pub version_req: SmolStr,
    pub scope: DependencyScope,
}

/// The assembled language-neutral graph (contracts §1). Read-only once built; incremental
/// patching lands with the cache (RFC 0004).
#[derive(Debug, Default)]
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
    file_index: HashMap<ProjectPath, FileId>,
}

impl ProjectGraph {
    pub fn file_id(&self, path: &ProjectPath) -> Option<FileId> {
        self.file_index.get(path).copied()
    }

    /// The declared package name for a `PackageId`, when the owning manifest declared one
    /// (`package.json` `name`, …) — `None` for the implicit package and for manifests that
    /// never named themselves.
    pub fn package_name(&self, package: PackageId) -> Option<&str> {
        self.packages[package.0 as usize].name.as_deref()
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
            script_invoked_dependencies: HashSet::new(),
            packages: vec![PackageNode {
                manifest: None,
                name: None,
                private: false,
            }],
            edges,
            file_index,
        }
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
}

/// One file's claim + extracted facts, plus which adapter produced them (by index into the
/// `adapters` slice passed to [`assemble`] — stable for the duration of one assembly call).
struct Claimed {
    claim: FileClaim,
    facts: crate::adapter::FileFacts,
    adapter_index: usize,
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

/// Discovers, claims, extracts, resolves, and links — the full RFC 0001 §4 pipeline up to
/// (not including) analyses. Diagnostics accumulate rather than abort: a graph that omits one
/// unreadable file's facts is far more useful than no graph at all (RFC 0001 §6). Always cold
/// (no facts cache consulted) — see [`assemble_with_cache`] for the warm path.
pub fn assemble(
    root: &Path,
    adapters: &[Box<dyn LanguageAdapter>],
) -> Result<(ProjectGraph, Vec<Diagnostic>), DiscoveryError> {
    assemble_with_cache(root, adapters, None)
}

/// Same pipeline as [`assemble`], additionally consulting/populating a facts cache (RFC 0004
/// §2–4, ADR 0004): a file whose content hash already has a cached-and-current entry skips
/// re-parsing entirely, which is the warm path's dominant win since parsing dominates cold-run
/// cost (spike 0001). `cache: None` is exactly [`assemble`]'s behavior — this must hold
/// byte-for-byte, since `--no-cache` ≡ cached results is an RFC 0004 §4 correctness gate.
pub fn assemble_with_cache(
    root: &Path,
    adapters: &[Box<dyn LanguageAdapter>],
    cache: Option<&crate::cache::FactsCache>,
) -> Result<(ProjectGraph, Vec<Diagnostic>), DiscoveryError> {
    let discovered = discovery::discover(root)?;
    let mut diagnostics = discovered.diagnostics;

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
        .map(|df| {
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
                return Ok(Some(Claimed {
                    claim,
                    facts,
                    adapter_index,
                }));
            }
            let abs = root.join(df.path.0.as_str());
            let content = std::fs::read(&abs).map_err(|e| Diagnostic {
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
            Ok(Some(Claimed {
                claim,
                facts,
                adapter_index,
            }))
        })
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
            let abs = root.join(df.path.0.as_str());
            let content = std::fs::read(&abs).map_err(|e| Diagnostic {
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

    // Phase 2 — assign FileId (already the discovery-sorted index) and build File nodes.
    let mut files = Vec::with_capacity(discovered.files.len());
    let mut file_index = HashMap::with_capacity(discovered.files.len());
    for (i, df) in discovered.files.iter().enumerate() {
        let file_id = FileId(i as u32);
        file_index.insert(df.path.clone(), file_id);
        let (language, class) = match &claimed_per_file[i] {
            Some(c) => (Some(c.claim.language.clone()), Some(c.claim.class)),
            None => (None, None),
        };
        files.push(FileNode {
            path: df.path.clone(),
            content_hash: df.content_hash,
            language,
            class,
            package: PackageId(0), // patched in phase 2a once ownership is computed
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
    }];
    let mut manifest_package: Vec<Option<PackageId>> = vec![None; manifests_per_file.len()];
    for (i, slot) in manifests_per_file.iter().enumerate() {
        if let Some((_, facts)) = slot {
            let package_id = PackageId(packages.len() as u32);
            packages.push(PackageNode {
                manifest: Some(files[i].path.clone()),
                name: facts.package_name.clone(),
                private: facts.private,
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
    let mut declared_dependency_names: HashSet<SmolStr> = HashSet::new();
    let mut declared_dependencies: Vec<DeclaredDependency> = Vec::new();
    let mut script_invoked_dependencies: HashSet<(PackageId, SmolStr)> = HashSet::new();
    // Every file a manifest names as a production root, at that root's own confidence — used
    // after phase 3a to promote the file's *exported* symbols to production roots too (RFC
    // 0011 §5: "Published/library: its public API is a production root — external consumers
    // exist by definition"). Keyed by file, keeping the strongest confidence when more than
    // one manifest field roots the same file (e.g. both `main` and an `exports` leaf).
    let mut library_root_files: HashMap<FileId, Confidence> = HashMap::new();
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
                    kind: EdgeKind::Root {
                        kind: root.kind,
                        target: NodeRef::File(target),
                    },
                    confidence: root.confidence,
                    source: provenance(),
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

    // Phase 2.6 — role-derived roots (RFC 0005 §2, literally): "Test roots — test
    // functions/files (language role detection…)"; "Tooling roots — build/config scripts
    // (webpack.config…)". The adapter's role classification *is* the seed for these two root
    // kinds — the runner/tool that consumes the file lives outside the graph, so the file's
    // existence under the convention is the whole evidence. `Probable`, not certain: a
    // convention names the file, nothing declares it (same reasoning as `exports`-map leaves).
    // Production roots stay manifest/API-driven (phase 2.5) — never role-derived.
    let mut role_root_files: HashMap<FileId, crate::vocab::RootKind> = HashMap::new();
    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        let kind = match claimed.claim.class.role {
            crate::vocab::FileRole::Test => crate::vocab::RootKind::Test,
            crate::vocab::FileRole::Tooling => crate::vocab::RootKind::Tooling,
            crate::vocab::FileRole::Production => continue,
        };
        let file_id = FileId(i as u32);
        edges.push(Edge {
            kind: EdgeKind::Root {
                kind,
                target: NodeRef::File(file_id),
            },
            confidence: Confidence::Probable,
            source: Provenance::Adapter(adapters[claimed.adapter_index].descriptor().id.clone()),
        });
        role_root_files.insert(file_id, kind);
    }

    // Phase 3a — symbols (Declares edges) and in-source roots, sequentially in FileId order.
    // Split from imports/references (phase 3b) because resolving a reference or an import
    // binding to *another* file's symbol needs that file's symbol table already built —
    // forward references (file 0 importing from file 5) are the common case, not an edge case,
    // so every file's declarations must exist before any file's imports are resolved.
    let mut symbols = Vec::new();
    let mut symbol_by_name_per_file: Vec<HashMap<SmolStr, SymbolId>> =
        vec![HashMap::new(); claimed_per_file.len()];
    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        let file_id = FileId(i as u32);
        let adapter = &adapters[claimed.adapter_index];
        let provenance = || Provenance::Adapter(adapter.descriptor().id.clone());

        for decl in &claimed.facts.declarations {
            let symbol_id = SymbolId(symbols.len() as u32);
            symbol_by_name_per_file[i].insert(decl.name.clone(), symbol_id);
            symbols.push(SymbolNode {
                file: file_id,
                name: decl.name.clone(),
                kind: decl.kind.clone(),
                span: decl.span,
                exported: decl.exported,
                visibility: decl.visibility,
            });
            edges.push(Edge {
                kind: EdgeKind::Declares {
                    file: file_id,
                    symbol: symbol_id,
                },
                confidence: Confidence::Certain,
                source: provenance(),
            });

            // Library-mode promotion (RFC 0011 §5): this file is a manifest-declared production
            // root and this symbol is exported from it, so it's part of the package's public
            // API — a production root in its own right, not just "alive because the file is."
            // Without this, every public export a root file doesn't also call internally reads
            // as dead code (confirmed against real npm packages during M1 conformance work —
            // e.g. a library's second named export, never self-invoked, otherwise false-
            // positives as `unused`).
            if decl.exported {
                if let Some(&confidence) = library_root_files.get(&file_id) {
                    edges.push(Edge {
                        kind: EdgeKind::Root {
                            kind: crate::vocab::RootKind::Production,
                            target: NodeRef::Symbol(symbol_id),
                        },
                        confidence,
                        source: provenance(),
                    });
                }
                // Same promotion for role-derived roots (phase 2.6): a config file's exports
                // ARE its interface to the tool that loads it (`export default {…}` in
                // webpack.config consumed by webpack), and a test file's exports may be
                // shared fixtures — the consumer is outside the graph either way, so the
                // export surface is the whole visible contract.
                if let Some(&kind) = role_root_files.get(&file_id) {
                    edges.push(Edge {
                        kind: EdgeKind::Root {
                            kind,
                            target: NodeRef::Symbol(symbol_id),
                        },
                        confidence: Confidence::Probable,
                        source: provenance(),
                    });
                }
            }
        }

        // In-source roots (RawRoot — e.g. a language-level `export =`/`pub` API marker), as
        // distinct from the manifest-declared roots phase 2.5 already linked: this targets
        // something *within* the file being extracted, never a different file.
        for root in &claimed.facts.roots {
            let target = match &root.target {
                RawRootTarget::WholeFile => Some(NodeRef::File(file_id)),
                // A root naming a declaration this extraction didn't actually produce is an
                // adapter contract violation — defensive skip, not a silent crash.
                RawRootTarget::Declaration(name) => symbol_by_name_per_file[i]
                    .get(name)
                    .map(|&s| NodeRef::Symbol(s)),
            };
            if let Some(target) = target {
                edges.push(Edge {
                    kind: EdgeKind::Root {
                        kind: root.kind,
                        target,
                    },
                    confidence: root.confidence,
                    source: provenance(),
                });
            }
        }
    }

    // Workspace-member index (RFC 0011 §4): every *named* manifest in the graph, keyed by
    // package name, with its directory and adapter-resolved primary entry — what lets a bare
    // specifier (`@org/ui`) resolve to the sibling's internal files instead of an external
    // dependency. Built from manifest facts, consumed by import resolution — strictly after
    // manifest extraction, so no circularity. Duplicate names keep the first in
    // file-discovery order (deterministic); a repo with two same-named manifests is broken
    // in ways no resolution order fixes.
    let mut workspace_member_index: HashMap<SmolStr, crate::adapter::WorkspaceMember> =
        HashMap::new();
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

    let ctx = ResolveCtx::new(&known_files)
        .with_declared_dependencies(&declared_dependency_names)
        .with_workspace_members(&workspace_member_index);

    // Phase 3a-bis — re-export aliasing (`export {a} from './b'`, `export type {a} from
    // './b'`): a barrel's re-exported bindings become resolvable as *its own* exports too, not
    // merely usable inside it (js-ts.md §5: "Barrel files… resolved through, transparently").
    // Must run for every file before phase 3b resolves any file's import bindings — the same
    // forward-reference reasoning as the 3a/3b split, one level deeper: a barrel can be
    // imported before or after this loop reaches the barrel's own re-export statement. Scoped
    // to one hop: a barrel re-exporting from another barrel only resolves correctly when
    // file-discovery order happens to process the deeper barrel first in this same pass — true
    // multi-hop chain resolution is a future increment, not attempted here.
    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        let file_id = FileId(i as u32);
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
            let target_path = match adapter.resolve(&spec, &ctx) {
                Resolution::File(path, _) => path,
                Resolution::WorkspaceMember { target, .. } => target,
                _ => continue,
            };
            let Some(&target) = file_index.get(&target_path) else {
                continue;
            };
            for binding in &imp.bindings {
                let exported_name = binding
                    .imported
                    .clone()
                    .unwrap_or_else(|| SmolStr::new("default"));
                let Some(&original_symbol) =
                    symbol_by_name_per_file[target.0 as usize].get(&exported_name)
                else {
                    continue;
                };
                symbol_by_name_per_file[i].insert(binding.local.clone(), original_symbol);
                // The barrel itself is a manifest-declared production root, so everything it
                // re-exports is part of the package's public API too (RFC 0011 §5) — same
                // promotion phase 3a already applies to the barrel's *own* declarations,
                // extended through one level of re-export indirection.
                if let Some(&confidence) = library_root_files.get(&file_id) {
                    edges.push(Edge {
                        kind: EdgeKind::Root {
                            kind: crate::vocab::RootKind::Production,
                            target: NodeRef::Symbol(original_symbol),
                        },
                        confidence,
                        source: Provenance::Adapter(adapter.descriptor().id.clone()),
                    });
                }
            }
        }
    }

    // Phase 3b — imports, import-bindings, references, and diagnostics. Every file's symbol
    // table is complete now (phase 3a), so cross-file lookups are safe regardless of
    // discovery order.
    let mut dependencies = Vec::new();
    let mut dep_index: HashMap<SmolStr, DependencyId> = HashMap::new();

    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        let file_id = FileId(i as u32);
        let adapter = &adapters[claimed.adapter_index];
        let provenance = || Provenance::Adapter(adapter.descriptor().id.clone());

        // Local name -> target symbol, from this file's import bindings — the fact that lets a
        // `RawReference` to an *imported* name resolve cross-file instead of only same-file.
        let mut bound_symbols: HashMap<SmolStr, SymbolId> = HashMap::new();

        for imp in &claimed.facts.imports {
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
            let (file_target, dep_target) = match adapter.resolve(&spec, &ctx) {
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
                    edges.push(Edge {
                        kind: EdgeKind::ImportsFile { from: file_id, to },
                        confidence,
                        source: provenance(),
                    });
                    for binding in &imp.bindings {
                        let exported_name = binding
                            .imported
                            .clone()
                            .unwrap_or_else(|| SmolStr::new("default"));
                        if let Some(&symbol_id) =
                            symbol_by_name_per_file[to.0 as usize].get(&exported_name)
                        {
                            bound_symbols.insert(binding.local.clone(), symbol_id);
                        }
                    }
                    // The namespace escaped static tracking (`ns[key]`, ns passed
                    // along) — every symbol in the target is plausibly used
                    // (RFC 0005 §1: "wildcard over that namespace's exports").
                    if imp.opaque_namespace_use {
                        edges.push(Edge {
                            kind: EdgeKind::Wildcard { from: to },
                            confidence: Confidence::Possible,
                            source: provenance(),
                        });
                    }
                }
            }
            if let Some((name, confidence)) = dep_target {
                let to = *dep_index.entry(name.clone()).or_insert_with(|| {
                    let id = DependencyId(dependencies.len() as u32);
                    dependencies.push(DependencyNode { name: name.clone() });
                    id
                });
                edges.push(Edge {
                    kind: EdgeKind::ImportsDependency { from: file_id, to },
                    confidence,
                    source: provenance(),
                });
            }
        }

        // File-granularity (`NodeRef::File`, not a specific symbol): extraction doesn't track
        // which enclosing declaration contains a reference, only which file — sufficient for
        // reachability (a reachable file referencing a symbol makes that symbol reachable
        // regardless of which of the file's functions did it) though not for finer-grained
        // "which caller" evidence later. Bound (imported) names resolve first, falling back to
        // same-file declarations — real JS/TS can't have both share a name at module scope, so
        // this ordering is never actually contested by valid code, just a defensive default.
        // Neither lookup models block/parameter shadowing: a same-named local could
        // (incorrectly, but safely — see module docs) resolve to an unrelated declaration.
        for reference in &claimed.facts.references {
            let target = bound_symbols
                .get(&reference.name)
                .or_else(|| symbol_by_name_per_file[i].get(&reference.name))
                .copied();
            if let Some(to) = target {
                edges.push(Edge {
                    kind: EdgeKind::References {
                        from: NodeRef::File(file_id),
                        to,
                        kind: crate::vocab::RefKind::Read,
                    },
                    confidence: Confidence::Certain,
                    source: provenance(),
                });
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
        for dynamic in &claimed.facts.dynamics {
            match dynamic.narrowed_to.as_deref().filter(|d| !d.is_empty()) {
                Some(dir) => {
                    for (j, file) in files.iter().enumerate() {
                        if j == i || !package_owns(dir, core_dirname(file.path.0.as_str())) {
                            continue;
                        }
                        let target = FileId(j as u32);
                        edges.push(Edge {
                            kind: EdgeKind::ImportsFile {
                                from: file_id,
                                to: target,
                            },
                            confidence: Confidence::Possible,
                            source: provenance(),
                        });
                        edges.push(Edge {
                            kind: EdgeKind::Wildcard { from: target },
                            confidence: Confidence::Possible,
                            source: provenance(),
                        });
                    }
                }
                // Empty-string narrowing would prefix-match the whole project — treat it as
                // the adapter meaning "no narrowing" rather than "everything".
                None => edges.push(Edge {
                    kind: EdgeKind::Wildcard { from: file_id },
                    confidence: Confidence::Possible,
                    source: provenance(),
                }),
            }
        }

        for d in &claimed.facts.diagnostics {
            diagnostics.push(Diagnostic {
                level: d.level,
                path: Some(files[i].path.clone()),
                message: d.message.clone(),
                span: d.span,
            });
        }
    }

    Ok((
        ProjectGraph {
            files,
            symbols,
            dependencies,
            declared_dependencies,
            script_invoked_dependencies,
            packages,
            edges,
            file_index,
        },
        diagnostics,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{
        AdapterDescriptor, Declaration, FileFacts, ImportBinding, ImportKind, ManifestDependency,
        ManifestFacts, ManifestRoot, RawImport, RawReference, RawRoot, RawRootTarget,
    };
    use crate::vocab::{DependencyScope, FileOrigin, FileRole, RootKind};
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
            let text = std::str::from_utf8(file.content).unwrap_or("");
            let mut facts = FileFacts::default();
            for line in text.lines() {
                if let Some(name) = line.strip_prefix("decl ") {
                    facts.declarations.push(Declaration {
                        name: SmolStr::new(name),
                        kind: SymbolKind::Function,
                        span: Span::default(),
                        exported: true,
                        visibility: VisibilityLevel(1),
                    });
                } else if let Some(name) = line.strip_prefix("private-decl ") {
                    facts.declarations.push(Declaration {
                        name: SmolStr::new(name),
                        kind: SymbolKind::Function,
                        span: Span::default(),
                        exported: false,
                        visibility: VisibilityLevel(0),
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
                    });
                } else if let Some(name) = line.strip_prefix("ref ") {
                    facts.references.push(RawReference {
                        name: SmolStr::new(name),
                        scope_context: None,
                        span: Span::default(),
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
                }
            }
            facts
        }

        fn extract_manifest(&self, file: &SourceFile<'_>, ctx: &ResolveCtx<'_>) -> ManifestFacts {
            // Content format for the mock manifest: one directive per line.
            //   dep <name>        -> a prod-scope declared dependency
            //   root <path>       -> a Production root targeting that known file, if it exists
            //   cli-invoke <name> -> a script-invoked dependency name
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
        let (graph, diags) = assemble(&dir, &mock_adapters()).unwrap();
        assert!(diags.is_empty());
        assert_eq!(graph.files.len(), 1);
        assert!(graph.files[0].language.is_none());
    }

    #[test]
    fn declarations_become_symbols_with_declares_edges() {
        let dir = project("decls", &[("a.mock", "decl foo\ndecl bar")]);
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
    fn relative_import_produces_imports_file_edge() {
        let dir = project(
            "imports-file",
            &[("a.mock", "import ./b.mock"), ("b.mock", "decl target")],
        );
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
        let b = graph.file_id(&ProjectPath(SmolStr::new("b.mock"))).unwrap();
        assert!(graph
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::ImportsFile { from: a, to: b }));
    }

    #[test]
    fn bare_import_produces_dependency_node_and_edge() {
        let dir = project("imports-dep", &[("a.mock", "import lodash")]);
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        assert_eq!(graph.dependencies.len(), 1);
    }

    #[test]
    fn unresolved_import_produces_no_edge_and_no_diagnostic() {
        let dir = project("unresolved", &[("a.mock", "import ./missing.mock")]);
        let (graph, diags) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        assert_eq!(graph.files.len(), 1);
        assert!(
            graph.files[0].language.is_none(),
            "spec: manifests are not claimed as source (docs/adapters/js-ts.md §1)"
        );
    }

    #[test]
    fn no_manifest_means_everyone_owns_the_implicit_package() {
        let dir = project("pkg-implicit", &[("a.mock", "decl f")]);
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        assert!(graph
            .edges
            .iter()
            .all(|e| !matches!(e.kind, EdgeKind::Root { .. })));
    }

    #[test]
    fn manifest_dependencies_reach_the_resolver_as_declared() {
        // With no manifest, `lodash` resolves undeclared (the mock demotes to `probable`).
        let dir = project("no-manifest-dep", &[("a.mock", "import lodash")]);
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let findings = crate::analysis::run_all(&graph);
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        let findings = crate::analysis::run_all(&graph);
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        let findings = crate::analysis::run_all(&graph);
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        let findings = crate::analysis::run_all(&graph);
        let flagged: Vec<Option<&str>> = findings
            .iter()
            .map(|f| f.location.symbol.as_deref())
            .collect();
        assert!(flagged.contains(&Some("dead")));
        assert!(!flagged.contains(&Some("util")));
    }

    #[test]
    fn cli_invoke_directive_reaches_script_invoked_dependencies() {
        let dir = project("cli-invoke", &[("manifest.json", "dep xo\ncli-invoke xo")]);
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        assert!(graph
            .script_invoked_dependencies
            .contains(&(PackageId(1), SmolStr::new("xo"))));
    }

    #[test]
    fn raw_root_whole_file_becomes_root_edge_to_the_file() {
        let dir = project("raw-root-file", &[("a.mock", "root-file")]);
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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

        let findings = crate::analysis::run_all(&graph);
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        let findings = crate::analysis::run_all(&graph);
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        let b = graph.file_id(&ProjectPath(SmolStr::new("b.mock"))).unwrap();
        let wildcard = graph
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Wildcard { from: b })
            .expect("wildcard from the opaquely-consumed target");
        assert_eq!(wildcard.confidence, Confidence::Possible);

        let findings = crate::analysis::run_all(&graph);
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        let findings = crate::analysis::run_all(&graph);
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
        assert!(graph
            .edges
            .iter()
            .any(|e| matches!(e.kind, EdgeKind::References { .. })));
    }

    #[test]
    fn same_file_reference_resolves_without_an_import() {
        let dir = project("ref-same-file", &[("a.mock", "decl helper\nref helper")]);
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (graph, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let (g1, _) = assemble(&dir, &mock_adapters()).unwrap();
        let (g2, _) = assemble(&dir, &mock_adapters()).unwrap();
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
        let cold = assemble(&dir, &mock_adapters()).unwrap().0;

        let cache_dir = std::env::temp_dir().join("kndo-graph-test-cache-equivalence-cache");
        let _ = fs::remove_dir_all(&cache_dir);
        let cache = crate::cache::FactsCache::open(&cache_dir);
        // First cached run populates every entry (all misses); second is fully warm.
        assemble_with_cache(&dir, &mock_adapters(), Some(&cache)).unwrap();
        let warm = assemble_with_cache(&dir, &mock_adapters(), Some(&cache))
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
        let cache = crate::cache::FactsCache::open(&cache_dir);

        assemble_with_cache(&dir, &mock_adapters(), Some(&cache)).unwrap();
        assert_eq!(cache.hits(), 0); // first run: every file is a miss, then gets stored

        assemble_with_cache(&dir, &mock_adapters(), Some(&cache)).unwrap();
        assert_eq!(cache.hits(), 2); // second run: both files served from disk, none re-parsed
    }
}
