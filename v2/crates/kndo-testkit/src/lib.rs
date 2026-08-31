//! Test machinery: the mock language (`.kmock`) every engine gate runs against, and a
//! temp-project builder. Contract-only by design — the same crate a third-party
//! adapter author can use, with no path to the engine's internals.
//!
//! The kmock DSL, one construct per line:
//!
//! ```text
//! fn name              private function declaration
//! pub fn name          exported function declaration
//! call name            a Call reference to `name`
//! import ./x           side-effect import of x.kmock in the same directory
//! import ./x { a, b }  binding import
//! root name            production root anchored on the declaration `name`
//! root-file            whole-file production root
//! # text               a comment (the Comments stream, declared)
//! ```

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{
    CoverageRecords, DeclarationId, DiagnosticLevel, EvidenceSink, EvidenceStream, EvidenceStreams,
    ImportBinding, ImportShape, ImportTarget, Reach, RefKind, RootKind, RootTarget, SymbolKind,
};
use kndo_contract::extension::{ConductSink, ContentAccess, Extension, ExtensionSpec, GraphAccess};
use kndo_contract::vocab::{Confidence, ProjectPath, Span};
use std::collections::BTreeMap;
use std::path::Path;

type ConductHook = dyn Fn(&dyn GraphAccess, &dyn ContentAccess, &mut ConductSink) + Send + Sync;
type IngestHook = dyn Fn(&str, &[u8]) -> Option<CoverageRecords> + Send + Sync;

/// The one mock for every cluster. [`MockExtension::new`] speaks the kmock
/// language (extraction + resolution); [`MockExtension::scripted`] carries any
/// spec and runs the closures a test hangs on its conduct and ingestion hooks —
/// including behavior a correct extension never has, because drops and refusals
/// are exactly what containment tests script.
pub struct MockExtension {
    spec: ExtensionSpec,
    speaks_kmock: bool,
    on_contribute: Option<Box<ConductHook>>,
    on_report: Option<Box<ConductHook>>,
    on_ingest: Option<Box<IngestHook>>,
}

/// The kmock-speaking mock under its historical name.
pub type MockAdapter = MockExtension;

impl MockExtension {
    pub fn new() -> Self {
        MockExtension {
            spec: ExtensionSpec::builder("kmock", 1)
                .suffixes(&["kmock"])
                .emits(EvidenceStreams::of(&[EvidenceStream::Comments]))
                .build(),
            speaks_kmock: true,
            on_contribute: None,
            on_report: None,
            on_ingest: None,
        }
    }

    /// A conduct/ingestion mock: no language, the given spec, and whatever the
    /// closures script.
    pub fn scripted(spec: ExtensionSpec) -> Self {
        MockExtension {
            spec,
            speaks_kmock: false,
            on_contribute: None,
            on_report: None,
            on_ingest: None,
        }
    }

    pub fn on_contribute(
        mut self,
        f: impl Fn(&dyn GraphAccess, &dyn ContentAccess, &mut ConductSink) + Send + Sync + 'static,
    ) -> Self {
        self.on_contribute = Some(Box::new(f));
        self
    }

    pub fn on_report(
        mut self,
        f: impl Fn(&dyn GraphAccess, &dyn ContentAccess, &mut ConductSink) + Send + Sync + 'static,
    ) -> Self {
        self.on_report = Some(Box::new(f));
        self
    }

    pub fn on_ingest(
        mut self,
        f: impl Fn(&str, &[u8]) -> Option<CoverageRecords> + Send + Sync + 'static,
    ) -> Self {
        self.on_ingest = Some(Box::new(f));
        self
    }
}

impl Default for MockExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl Extension for MockExtension {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn contribute_roots(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        if let Some(f) = &self.on_contribute {
            f(graph, content, out);
        }
    }

    fn report_findings(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        if let Some(f) = &self.on_report {
            f(graph, content, out);
        }
    }

    fn ingest(&self, report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        self.on_ingest.as_ref()?(report_path, content)
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        if !self.speaks_kmock {
            return;
        }
        let text = String::from_utf8_lossy(file.content);

        // Pass 1: declarations, so roots can anchor by id regardless of line order.
        let mut decls: BTreeMap<&str, DeclarationId> = BTreeMap::new();
        for (line, span) in lines_with_spans(&text) {
            let (reach, rest) = match line.strip_prefix("pub fn ") {
                Some(rest) => (Reach::Exported, rest),
                None => match line.strip_prefix("fn ") {
                    Some(rest) => (Reach::Private, rest),
                    None => continue,
                },
            };
            let name = rest.trim();
            let id = out.declaration(name, SymbolKind::Function, span, reach);
            decls.insert(name, id);
        }

        // Pass 2: everything that may point at a declaration.
        for (line, span) in lines_with_spans(&text) {
            if let Some(name) = line.strip_prefix("call ") {
                out.reference(name.trim(), RefKind::Call, span);
            } else if line == "root-file" {
                out.root(
                    RootTarget::WholeFile,
                    RootKind::Production,
                    Confidence::Certain,
                );
            } else if let Some(name) = line.strip_prefix("root ") {
                match decls.get(name.trim()) {
                    Some(id) => out.root(
                        RootTarget::Declaration(*id),
                        RootKind::Production,
                        Confidence::Certain,
                    ),
                    None => out.diagnostic(
                        DiagnosticLevel::Warn,
                        format!("root names undeclared `{}`", name.trim()),
                        Some(span),
                    ),
                }
            } else if let Some(rest) = line.strip_prefix("import ") {
                let (specifier, shape) = match rest.split_once('{') {
                    Some((spec, names)) => {
                        let bindings = names
                            .trim_end_matches('}')
                            .split(',')
                            .map(str::trim)
                            .filter(|n| !n.is_empty())
                            .map(|n| ImportBinding {
                                imported: n.into(),
                                local: n.into(),
                            })
                            .collect();
                        (spec.trim(), ImportShape::Bindings(bindings))
                    }
                    None => (rest.trim(), ImportShape::SideEffect),
                };
                out.import(
                    ImportTarget::Relative(specifier.into()),
                    shape,
                    span,
                    Confidence::Certain,
                );
            } else if let Some(t) = line.strip_prefix("# ") {
                let text_start = span.start + (line.len() - t.len()) as u32;
                out.comment(span, Span::new(text_start, span.end));
            }
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        if !self.speaks_kmock {
            return Resolution::Unresolved;
        }
        let Some(name) = specifier.strip_prefix("./") else {
            return Resolution::Unresolved;
        };
        let dir = match from.as_str().rsplit_once('/') {
            Some((dir, _)) => format!("{dir}/"),
            None => String::new(),
        };
        let ext = &self.spec.suffixes()[0];
        let candidate = ProjectPath::new(format!("{dir}{name}.{ext}"));
        if cx.contains(&candidate) {
            Resolution::File(candidate)
        } else {
            Resolution::Unresolved
        }
    }
}

fn lines_with_spans(text: &str) -> Vec<(&str, Span)> {
    let mut out = Vec::new();
    let mut offset = 0u32;
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        if !line.is_empty() {
            out.push((line, Span::new(offset, offset + line.len() as u32)));
        }
        offset += raw.len() as u32;
    }
    out
}

/// A throwaway project on disk (tempfile-backed — never a hand-rolled temp path).
pub struct TempProject {
    dir: tempfile::TempDir,
}

impl TempProject {
    pub fn new() -> Self {
        TempProject {
            dir: tempfile::tempdir().expect("create temp project"),
        }
    }

    pub fn file(&self, rel: &str, content: &str) -> &Self {
        let path = self.dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(path, content).expect("write project file");
        self
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }
}

impl Default for TempProject {
    fn default() -> Self {
        Self::new()
    }
}

/// One extraction, harness-style: feed `source` to `adapter` as `path` through a
/// fresh sink and return the finished evidence. The shared front half of every
/// adapter's extraction tests.
pub fn extract_evidence(
    adapter: &dyn Extension,
    path: &str,
    source: &str,
) -> kndo_contract::evidence::FileEvidence {
    let path = ProjectPath::new(path);
    let mut sink = EvidenceSink::new(source.len() as u32, adapter.spec().emits().clone());
    adapter.extract(
        &SourceFile {
            path: &path,
            content: source.as_bytes(),
        },
        &mut sink,
    );
    sink.finish()
}

/// The declaration named `name`, or a panic that prints every declaration — the
/// assertion failure an extraction test wants to read.
pub fn declaration_named<'e>(
    ev: &'e kndo_contract::evidence::FileEvidence,
    name: &str,
) -> &'e kndo_contract::evidence::Declaration {
    ev.declarations
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("declaration {name} missing: {:#?}", ev.declarations))
}

/// One resolution against a synthetic file set — the shared front half of every
/// adapter's resolution tests.
pub fn resolve_in(
    adapter: &dyn Extension,
    files: &[&str],
    from: &str,
    specifier: &str,
) -> Resolution {
    let known: std::collections::BTreeSet<ProjectPath> =
        files.iter().map(|p| ProjectPath::new(*p)).collect();
    let cx = ResolveContext::new(&known);
    adapter.resolve(&ProjectPath::new(from), specifier, &cx)
}

/// The first import whose specifier matches, or a panic that prints every import —
/// the assertion failure an extraction test wants to read.
pub fn import_named<'e>(
    ev: &'e kndo_contract::evidence::FileEvidence,
    specifier: &str,
) -> &'e kndo_contract::evidence::Import {
    ev.imports
        .iter()
        .find(|i| match &i.target {
            ImportTarget::Relative(s) | ImportTarget::Package(s) => s == specifier,
            _ => false,
        })
        .unwrap_or_else(|| panic!("import {specifier} missing: {:#?}", ev.imports))
}
