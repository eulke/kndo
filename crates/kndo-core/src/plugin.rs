//! The `Plugin` contract (contracts/core-traits.md §3, RFC 0003).
//!
//! Adapters describe what code *is*; plugins describe what an ecosystem *means* by it.
//! All hooks are optional; the same trait serves built-ins (statically linked) and external
//! WASM components (bridged via `kndo-plugin-api`, ADR 0003 — `kndo:plugin@0.1.0` for the four
//! graph-mutation hooks below, `kndo:adapter@0.1.0` for `LanguageAdapter`; see
//! docs/contracts/wasm-abi.md §5). `ingest_coverage`/`suppress` aren't bridged either way yet.
//! `GraphView` is read-only; mutation happens only through typed sinks the core validates and
//! attributes (`Provenance::Plugin`).
//!
//! **Targets are named, never addressed by internal id** (`ProjectPath` + an optional bare or
//! `Owner.name` symbol name) — the same contract shape `RawRoot`/`RawReference` already use for
//! adapters (contracts/core-traits.md §2). A plugin naming a target that doesn't resolve is a
//! silent no-op, exactly like an adapter's own miss — no new failure mode, and it keeps
//! `FileId`/`SymbolId` (internal, renumbered every run) off the `Plugin` trait's stable-from-1.0
//! surface entirely.

use rustc_hash::FxHashMap as HashMap;
use smol_str::SmolStr;

use crate::adapter::ProjectPath;
use crate::graph::{FileNode, SymbolNode};
use crate::vocab::{Confidence, FileClass, FileId, RefKind, RootKind};

#[derive(Debug, Clone)]
pub struct PluginDescriptor {
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
}

/// One machine-checkable activation predicate (RFC 0003 §4). Evaluated against the project
/// root before a globally installed plugin is even instantiated — cheap, filesystem-only checks,
/// no code execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationRule {
    /// At least one file under the project root matches this glob (e.g. `"next.config.*"`).
    FileExists(SmolStr),
    /// A root manifest (`package.json`, `Cargo.toml` today — RFC 0003 §4's stated v1 scope)
    /// declares a dependency with this name, in any dependency section.
    ManifestDependency(SmolStr),
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
    symbols_by_file: HashMap<FileId, Vec<u32>>,
}

impl<'a> GraphView<'a> {
    pub(crate) fn new(
        files: &'a [FileNode],
        symbols: &'a [SymbolNode],
        file_index: &'a HashMap<ProjectPath, FileId>,
    ) -> Self {
        let mut symbols_by_file: HashMap<FileId, Vec<u32>> = HashMap::default();
        for (i, s) in symbols.iter().enumerate() {
            symbols_by_file.entry(s.file).or_default().push(i as u32);
        }
        GraphView {
            files,
            symbols,
            file_index,
            symbols_by_file,
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
/// class, CSS class names used from HTML templates. Always a `References` edge — plugins can't
/// mint new edge kinds (RFC 0003 §2: "cannot define new node/edge kinds").
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

// ---------------------------------------------------------------- the trait

pub trait Plugin: Send + Sync {
    fn descriptor(&self) -> PluginDescriptor;

    /// Adjust a file's role/origin beyond language defaults (e.g. `*.stories.tsx` → tooling).
    /// Runs once per claimed file, right after RFC 0012 §7's content-derived origin correction
    /// and before role-derived roots (phase 2.6) — so a plugin's answer is what every downstream
    /// consumer (root promotion, `unused`/`test-only`'s per-file exemptions) sees.
    fn classify_file(&self, _path: &ProjectPath, _current: FileClass) -> Option<FileClass> {
        None
    }

    /// Framework entry points: routes, DI-registered beans, handlers…
    fn contribute_roots(&self, _graph: &GraphView<'_>, _out: &mut RootSink) {}

    /// Edges invisible to the language: DI wiring, template → class, route → handler…
    fn contribute_edges(&self, _graph: &GraphView<'_>, _out: &mut EdgeSink) {}

    /// Mark symbols externally consumed (FFI, serialization targets, public SDK surface).
    fn annotate_symbols(&self, _graph: &GraphView<'_>, _out: &mut AnnotationSink) {}

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
            id: SmolStr::new("coverage-lcov"),
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
        }
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

        let view = GraphView::new(&files, &symbols, &file_index);

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

        let view = GraphView::new(&files, &symbols, &file_index);
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

        let view = GraphView::new(&files, &symbols, &file_index);
        let paths: Vec<&str> = view.files().map(|f| f.path.0.as_str()).collect();
        assert_eq!(paths, vec!["a.mock", "b.mock"]);
    }
}
