//! Project Graph assembly (RFC 0001 §3–4) — turns discovered files into the language-neutral
//! graph via registered adapters. This module is **language-blind** (RFC 0001 §2, the
//! ignorance rule): it references only the `LanguageAdapter` trait, never a concrete
//! language. Adapter *registration* happens at the binary level (`kndo-cli` composes core +
//! first-party adapters) — the core must never know which languages exist.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rayon::prelude::*;

use crate::adapter::{
    Diagnostic, DiagnosticLevel, FileClaim, ImportSpec, LanguageAdapter, ProjectPath, Resolution,
    ResolveCtx, SourceFile, Span, VisibilityLevel,
};
use crate::discovery::{self, DiscoveryError};
use crate::vocab::{
    Confidence, DependencyId, Edge, EdgeKind, FileClass, FileId, Provenance, SymbolId, SymbolKind,
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

/// The assembled language-neutral graph (contracts §1). Read-only once built; incremental
/// patching lands with the cache (RFC 0004).
#[derive(Debug, Default)]
pub struct ProjectGraph {
    pub files: Vec<FileNode>,
    pub symbols: Vec<SymbolNode>,
    pub dependencies: Vec<DependencyNode>,
    pub edges: Vec<Edge>,
    file_index: HashMap<ProjectPath, FileId>,
}

impl ProjectGraph {
    pub fn file_id(&self, path: &ProjectPath) -> Option<FileId> {
        self.file_index.get(path).copied()
    }
}

/// One file's claim + extracted facts, plus which adapter produced them (by index into the
/// `adapters` slice passed to [`assemble`] — stable for the duration of one assembly call).
struct Claimed {
    claim: FileClaim,
    facts: crate::adapter::FileFacts,
    adapter_index: usize,
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
        });
    }

    // Phase 3 — symbols, dependencies, and edges, sequentially in FileId order (the loop
    // order below), which is what makes the whole assembly deterministic without an explicit
    // post-hoc sort of symbols/edges.
    let ctx = ResolveCtx::new(&known_files); // declared_dependencies: wired once manifests land
    let mut symbols = Vec::new();
    let mut edges = Vec::new();
    let mut dependencies = Vec::new();
    let mut dep_index: HashMap<SmolStr, DependencyId> = HashMap::new();

    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        let file_id = FileId(i as u32);
        let adapter = &adapters[claimed.adapter_index];
        let provenance = || Provenance::Adapter(adapter.descriptor().id.clone());

        for decl in &claimed.facts.declarations {
            let symbol_id = SymbolId(symbols.len() as u32);
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
        AdapterDescriptor, Declaration, FileFacts, ImportKind, ManifestFacts, RawImport,
    };
    use crate::vocab::{FileOrigin, FileRole};
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

        fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
            // Content format for the mock: one directive per line.
            //   decl <name>       -> a Function declaration
            //   import <specifier> -> a certain, non-side-effect import
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
                } else if let Some(spec) = line.strip_prefix("import ") {
                    facts.imports.push(RawImport {
                        specifier: SmolStr::new(spec),
                        kind: ImportKind::Relative,
                        span: Span::default(),
                        side_effect_only: false,
                        type_only: false,
                        confidence: Confidence::Certain,
                    });
                }
            }
            facts
        }

        fn extract_manifest(&self, _file: &SourceFile<'_>) -> ManifestFacts {
            ManifestFacts::default()
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
            Resolution::Dependency(spec.specifier.clone(), Confidence::Certain)
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
