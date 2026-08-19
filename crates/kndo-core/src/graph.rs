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
/// core, never the reverse).
fn core_dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Does `manifest_dir` govern `file_dir` (RFC 0011 §3, nearest-manifest-ancestor)? The empty
/// (project-root) manifest dir governs everything — callers only rely on that once every more
/// specific candidate has already been tried (ownership resolution sorts deepest-first).
fn package_owns(manifest_dir: &str, file_dir: &str) -> bool {
    manifest_dir.is_empty()
        || file_dir == manifest_dir
        || file_dir
            .strip_prefix(manifest_dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Discovers, claims, extracts, resolves, and links — the full RFC 0001 §4 pipeline up to
/// (not including) analyses. Diagnostics accumulate rather than abort: a graph that omits one
/// unreadable file's facts is far more useful than no graph at all (RFC 0001 §6).
pub fn assemble(
    root: &Path,
    adapters: &[Box<dyn LanguageAdapter>],
) -> Result<(ProjectGraph, Vec<Diagnostic>), DiscoveryError> {
    let discovered = discovery::discover(root)?;
    let mut diagnostics = discovered.diagnostics;

    let known_files: HashSet<ProjectPath> =
        discovered.files.iter().map(|f| f.path.clone()).collect();

    // Phase 1 — claim + extract, in parallel. rayon's collect preserves input order (the
    // path-sorted order discovery already established), so the FileId assignment in phase 2
    // stays deterministic regardless of which file's extraction happens to finish first
    // (RFC 0008 §4: parallel compute, deterministic reduce).
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

    // Phase 3b — imports, import-bindings, references, and diagnostics. Every file's symbol
    // table is complete now (phase 3a), so cross-file lookups are safe regardless of
    // discovery order.
    let ctx = ResolveCtx::new(&known_files).with_declared_dependencies(&declared_dependency_names);
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
            match adapter.resolve(&spec, &ctx) {
                Resolution::File(path, confidence) => {
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
                    }
                }
                Resolution::Dependency(name, confidence) => {
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
                // Stdlib: not a graph node — there is nothing to point an edge at.
                // Unresolved: resolution is intentionally incomplete right now (self-
                // reference imports, exports maps, workspace packages — spec §3); turning
                // this into a finding is the future `unresolved` analysis's job, not
                // assembly's (RFC 0005 §5).
                Resolution::Stdlib | Resolution::Unresolved => {}
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
            path.0.ends_with(".mock").then(|| FileClaim {
                language: SmolStr::new("mock"),
                class: FileClass {
                    role: FileRole::Production,
                    origin: FileOrigin::Authored,
                },
            })
        }

        fn claim_manifest(&self, path: &ProjectPath) -> bool {
            path.0.rsplit('/').next() == Some("manifest.json")
        }

        fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
            // Content format for the mock: one directive per line.
            //   decl <name>                 -> a Function declaration
            //   import <specifier> [binding[,binding...]]
            //       binding := name          -> ImportBinding { local: name, imported: Some(name) }
            //                | local=imported -> ImportBinding { local, imported: Some(imported) }
            //                | local=          -> ImportBinding { local, imported: None } (default)
            //   ref <name>                  -> a RawReference to that name
            //   root-file                   -> a Production root targeting this whole file
            //   root-decl <name>            -> a Production root targeting the named declaration
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
                } else if let Some(rest) = line.strip_prefix("import ") {
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
                }
            }
            facts
        }

        fn extract_manifest(&self, file: &SourceFile<'_>, ctx: &ResolveCtx<'_>) -> ManifestFacts {
            // Content format for the mock manifest: one directive per line.
            //   dep <name>  -> a prod-scope declared dependency
            //   root <path> -> a Production root targeting that known file, if it exists
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
}
