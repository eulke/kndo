//! The guest half of the ABI — one world, one macro, the REAL trait. An external
//! author implements [`kndo_contract::extension::Extension`] — the same trait,
//! the same `EvidenceSink`, `ResolveContext`, `ConductSink` and content scope a
//! built-in uses — and exports it with [`export_extension!`]. This crate rebuilds
//! the extraction context from the host's enumeration imports, hands conduct
//! hooks a graph and content view backed by the conduct imports, and converts
//! finished values to the wire once, here.
//!
//! The raw generated bindings are `#[doc(hidden)]`: the documented surface is
//! phase-correct by construction (an extraction hook is handed nothing that can
//! reach the graph), and a guest that digs into the hidden module anyway meets
//! the host's phase scoping — a named trap, not an answer.
//!
//! Compiles natively too (test suites can link the conversion helpers), but its
//! purpose is `wasm32-unknown-unknown` guests.

use kndo_contract::adapter::{PackageEntry, ProjectRoot, Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{
    self as ev, CoverageRecords, EvidenceSink, EvidenceStream, FileEvidence,
};
use kndo_contract::extension::{
    Activation, ActivationRule, ConductSeverity, ConductSink, ConductTarget, ContentAccess,
    DeclaredSymbol, Extension, ExtensionSpec, GraphAccess,
};
use kndo_contract::vocab::{Confidence, ProjectPath, Span};
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "extension",
        pub_export_macro: true,
    });
}

/// The vocabulary's generated types — one world, one Rust spelling of each record.
#[doc(hidden)]
pub use bindings::kndo::vocab::types as wire;

// ---------------------------------------------------------------- contract → wire

pub fn spec_to_wire(spec: &ExtensionSpec) -> wire::ExtensionSpec {
    wire::ExtensionSpec {
        coordinate: spec.coordinate().to_string(),
        version: spec.version(),
        suffixes: spec.suffixes().iter().map(|s| s.to_string()).collect(),
        narrowable_scopes: spec
            .narrowable_scopes()
            .iter()
            .map(|s| s.to_string())
            .collect(),
        claims: spec.claims().iter().map(|s| s.to_string()).collect(),
        // The declared set itself is the wire spelling — no second list to
        // forget when the contract grows a stream; a variant this SDK build
        // does not know yet degrades by omission, not by silent stripping.
        emits: spec.emits().iter().filter_map(stream_to_wire).collect(),
        manifests: spec.manifests().iter().map(|s| s.to_string()).collect(),
        conducts: spec.declares_conduct(),
        activation: activation_to_wire(spec.activation()),
        mutates_graph: spec.mutates_graph(),
        dependencies: spec.dependencies().iter().map(|s| s.to_string()).collect(),
        requested_file_access: spec
            .requested_file_access()
            .iter()
            .map(|s| s.to_string())
            .collect(),
        rules: spec
            .rules()
            .iter()
            .map(|r| wire::RuleDescriptor {
                name: r.name.to_string(),
                description: r.description.to_string(),
            })
            .collect(),
        reads_reports: spec.reads_reports().iter().map(|s| s.to_string()).collect(),
    }
}

fn activation_to_wire(activation: &Activation) -> wire::Activation {
    match activation {
        Activation::Always => wire::Activation::Always,
        Activation::AnyRule(rules) => wire::Activation::AnyRule(
            rules
                .iter()
                .map(|r| match r {
                    ActivationRule::FileExists(g) => {
                        wire::ActivationRule::FileExists(g.to_string())
                    }
                    ActivationRule::ManifestDependency(n) => {
                        wire::ActivationRule::ManifestDependency(n.to_string())
                    }
                })
                .collect(),
        ),
    }
}

fn stream_to_wire(stream: EvidenceStream) -> Option<wire::EvidenceStream> {
    match stream {
        EvidenceStream::Comments => Some(wire::EvidenceStream::Comments),
        EvidenceStream::Metrics => Some(wire::EvidenceStream::Metrics),
        // A stream this SDK build predates cannot cross this wire: omitted from
        // the declaration, so host-side pairing stays truthful (writes to it
        // would drop with a diagnostic rather than lie).
        _ => None,
    }
}

fn span_to_wire(span: Span) -> wire::Span {
    wire::Span {
        start: span.start,
        end: span.end,
    }
}

fn confidence_to_wire(c: Confidence) -> wire::Confidence {
    match c {
        Confidence::Possible => wire::Confidence::Possible,
        Confidence::Probable => wire::Confidence::Probable,
        Confidence::Certain => wire::Confidence::Certain,
    }
}

use wire::SymbolKind as WireSymbolKind;
kndo_contract::symbol_kind_conversions!(WireSymbolKind);

fn ref_kind_to_wire(kind: ev::RefKind) -> wire::RefKind {
    match kind {
        ev::RefKind::Call => wire::RefKind::Call,
        ev::RefKind::Read => wire::RefKind::Read,
        ev::RefKind::Write => wire::RefKind::Write,
        ev::RefKind::Extend => wire::RefKind::Extend,
        ev::RefKind::Implement => wire::RefKind::Implement,
        ev::RefKind::TypeUse => wire::RefKind::TypeUse,
        // An unknown kind counts as a use, never an accusation — Read is the
        // weakest keep-alive spelling the wire has.
        _ => wire::RefKind::Read,
    }
}

fn bindings_to_wire(bindings: &[ev::ImportBinding]) -> Vec<wire::ImportBinding> {
    bindings
        .iter()
        .map(|b| wire::ImportBinding {
            imported: b.imported.to_string(),
            local: b.local.to_string(),
        })
        .collect()
}

fn import_to_wire(import: &ev::Import) -> wire::Import {
    wire::Import {
        target: match &import.target {
            ev::ImportTarget::Relative(s) => wire::ImportTarget::Relative(s.to_string()),
            ev::ImportTarget::Package(s) => wire::ImportTarget::Package(s.to_string()),
            // An unknown target keeps its import alive unresolved; Package of the
            // empty string resolves nowhere and accuses nothing.
            _ => wire::ImportTarget::Package(String::new()),
        },
        shape: match &import.shape {
            ev::ImportShape::Bindings(b) => wire::ImportShape::Bindings(bindings_to_wire(b)),
            ev::ImportShape::Namespace { local } => wire::ImportShape::Namespace(local.to_string()),
            ev::ImportShape::SideEffect => wire::ImportShape::SideEffect,
            ev::ImportShape::Reexport(b) => wire::ImportShape::Reexport(bindings_to_wire(b)),
            ev::ImportShape::ReexportAll => wire::ImportShape::ReexportAll,
            ev::ImportShape::TypeOnly(b) => wire::ImportShape::TypeOnly(bindings_to_wire(b)),
            ev::ImportShape::Glob => wire::ImportShape::Glob,
            // An unknown shape keeps everything alive — SideEffect is that posture.
            _ => wire::ImportShape::SideEffect,
        },
        span: span_to_wire(import.span),
        confidence: confidence_to_wire(import.confidence),
    }
}

/// Finished evidence to the wire — what the extract shim sends back after the
/// author's real `EvidenceSink` pass.
pub fn evidence_to_wire(evidence: &FileEvidence) -> wire::FileEvidence {
    wire::FileEvidence {
        declarations: evidence
            .declarations
            .iter()
            .map(|d| wire::Declaration {
                name: d.name.to_string(),
                kind: symbol_kind_to_wire(&d.kind),
                span: span_to_wire(d.span),
                reach: match &d.reach {
                    ev::Reach::Private => wire::Reach::Private,
                    ev::Reach::Scoped { scope } => wire::Reach::Scoped(scope.to_string()),
                    ev::Reach::Exported => wire::Reach::Exported,
                },
                owner: d.owner.map(|id| id.index() as u32),
                exported_as: d.exported_as.as_ref().map(|s| s.to_string()),
            })
            .collect(),
        references: evidence
            .references
            .iter()
            .map(|r| wire::Reference {
                name: r.name.to_string(),
                kind: ref_kind_to_wire(r.kind),
                span: span_to_wire(r.span),
            })
            .collect(),
        imports: evidence.imports.iter().map(import_to_wire).collect(),
        roots: evidence
            .roots
            .iter()
            .map(|r| wire::Root {
                target: match &r.target {
                    ev::RootTarget::WholeFile => wire::RootTarget::WholeFile,
                    ev::RootTarget::Declaration(id) => {
                        wire::RootTarget::Declaration(id.index() as u32)
                    }
                    // An unknown target keeps the whole file alive.
                    _ => wire::RootTarget::WholeFile,
                },
                kind: root_kind_to_wire(r.kind),
                confidence: confidence_to_wire(r.confidence),
            })
            .collect(),
        comments: evidence
            .comments
            .iter()
            .map(|c| wire::CommentSpan {
                span: span_to_wire(c.span),
                text: span_to_wire(c.text),
            })
            .collect(),
        metrics: evidence
            .metrics
            .iter()
            .map(|(id, m)| wire::MetricEntry {
                declaration: id.index() as u32,
                metrics: wire::FunctionMetrics {
                    cyclomatic: m.cyclomatic,
                    loc: m.loc,
                    token_count: m.token_count,
                    fingerprints: m.fingerprints.clone(),
                },
            })
            .collect(),
        diagnostics: evidence
            .diagnostics
            .iter()
            .map(|d| wire::Diagnostic {
                level: match d.level {
                    ev::DiagnosticLevel::Info => wire::DiagnosticLevel::Info,
                    ev::DiagnosticLevel::Warn => wire::DiagnosticLevel::Warn,
                    ev::DiagnosticLevel::Error => wire::DiagnosticLevel::Error,
                },
                message: d.message.clone(),
                span: d.span.map(span_to_wire),
            })
            .collect(),
    }
}

fn root_kind_to_wire(kind: ev::RootKind) -> wire::RootKind {
    match kind {
        ev::RootKind::Production => wire::RootKind::Production,
        ev::RootKind::Test => wire::RootKind::Test,
        ev::RootKind::Tooling => wire::RootKind::Tooling,
    }
}

pub fn resolution_to_wire(resolution: Resolution) -> wire::Resolution {
    match resolution {
        Resolution::File(p) => wire::Resolution::File(p.as_str().to_string()),
        Resolution::Files(ps) => {
            wire::Resolution::Files(ps.iter().map(|p| p.as_str().to_string()).collect())
        }
        _ => wire::Resolution::Unresolved,
    }
}

pub fn project_root_to_wire(root: &ProjectRoot) -> wire::ProjectRoot {
    wire::ProjectRoot {
        file: root.file.as_str().to_string(),
        kind: root_kind_to_wire(root.kind),
        confidence: confidence_to_wire(root.confidence),
    }
}

pub fn package_entry_to_wire(entry: &PackageEntry) -> wire::PackageEntry {
    wire::PackageEntry {
        name: entry.name.to_string(),
        entry: entry.entry.as_ref().map(|p| p.as_str().to_string()),
        dir: entry.dir.to_string(),
    }
}

fn package_entry_from_wire(entry: wire::PackageEntry) -> PackageEntry {
    PackageEntry {
        name: SmolStr::new(entry.name),
        entry: entry.entry.map(ProjectPath::new),
        dir: SmolStr::new(entry.dir),
    }
}

fn conduct_target_to_wire(target: &ConductTarget) -> wire::ConductTarget {
    match target {
        ConductTarget::File(p) => wire::ConductTarget::File(p.as_str().to_string()),
        ConductTarget::Symbol { path, name } => wire::ConductTarget::Symbol(wire::SymbolRef {
            path: path.as_str().to_string(),
            name: name.to_string(),
        }),
    }
}

fn severity_to_wire(severity: ConductSeverity) -> wire::ConductSeverity {
    match severity {
        ConductSeverity::Error => wire::ConductSeverity::Error,
        ConductSeverity::Warning => wire::ConductSeverity::Warning,
        ConductSeverity::Info => wire::ConductSeverity::Info,
    }
}

pub fn records_to_wire(records: &CoverageRecords) -> wire::CoverageRecords {
    let mut lines = Vec::new();
    let mut functions = Vec::new();
    for (path, rec) in &records.files {
        for (line, hits) in &rec.lines {
            lines.push(wire::CoverageRecord {
                path: path.as_str().to_string(),
                line: *line,
                hits: *hits,
            });
        }
        for (line, hits) in &rec.functions {
            functions.push(wire::CoverageRecord {
                path: path.as_str().to_string(),
                line: *line,
                hits: *hits,
            });
        }
    }
    wire::CoverageRecords { lines, functions }
}

// ------------------------------------------------- the guest-side resolve context

/// The project as the host enumerated it, fetched once per instance and held for
/// the program's life (a component instance IS one program run). The context built
/// over it is the contract's own [`ResolveContext`] — innermost-package matching
/// and every other rule has exactly one owner, shared with native extensions.
struct ProjectSnapshot {
    known: BTreeSet<ProjectPath>,
    packages: BTreeMap<SmolStr, PackageEntry>,
}

fn project_snapshot() -> &'static ProjectSnapshot {
    static SNAPSHOT: OnceLock<ProjectSnapshot> = OnceLock::new();
    SNAPSHOT.get_or_init(|| ProjectSnapshot {
        known: bindings::known_files()
            .into_iter()
            .map(ProjectPath::new)
            .collect(),
        packages: bindings::package_entries()
            .into_iter()
            .map(package_entry_from_wire)
            .map(|p| (p.name.clone(), p))
            .collect(),
    })
}

/// EXTRACTION-PHASE CODE MUST NOT CALL THIS: the file listing is project data,
/// gated off during `extract` (evidence is cached by file content alone, so an
/// extraction that read the file SET would go stale invisibly) — the host traps
/// the call as a phase violation. Use it from `resolve`, `roots`, `packages`
/// and `sees`, where the project enumerations are the contract.
/// The real `ResolveContext`, rebuilt from the host's enumerations.
pub fn resolve_context() -> ResolveContext<'static> {
    let snap = project_snapshot();
    ResolveContext::with_packages(&snap.known, &snap.packages)
}

// --------------------------------------------------- the guest-side conduct views

/// The assembled graph over the conduct imports — the same [`GraphAccess`] shape
/// a native extension's hooks receive. Paths arrive at hook entry; the
/// declarations, the larger list, only when a hook first asks.
struct WireGraph {
    paths: Vec<ProjectPath>,
    declarations: OnceLock<Vec<WireDeclared>>,
}

struct WireDeclared {
    path: ProjectPath,
    name: SmolStr,
    kind: ev::SymbolKind,
    owner: Option<SmolStr>,
}

impl WireGraph {
    fn fetch() -> Self {
        WireGraph {
            paths: bindings::graph_paths()
                .into_iter()
                .map(ProjectPath::new)
                .collect(),
            declarations: OnceLock::new(),
        }
    }
}

impl GraphAccess for WireGraph {
    fn paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_> {
        Box::new(self.paths.iter())
    }

    fn contains(&self, path: &ProjectPath) -> bool {
        self.paths.binary_search(path).is_ok()
    }

    fn declarations(&self) -> Box<dyn Iterator<Item = DeclaredSymbol<'_>> + '_> {
        let declared = self.declarations.get_or_init(|| {
            bindings::graph_declarations()
                .into_iter()
                .map(|d| WireDeclared {
                    path: ProjectPath::new(d.path),
                    name: SmolStr::new(d.name),
                    kind: symbol_kind_from_wire(d.kind),
                    owner: d.owner.map(SmolStr::new),
                })
                .collect()
        });
        Box::new(declared.iter().map(|d| DeclaredSymbol {
            path: &d.path,
            name: d.name.as_str(),
            kind: &d.kind,
            owner: d.owner.as_deref(),
        }))
    }
}

/// Scoped content over the conduct imports. Fetched whole at hook entry: the
/// host prefetched this exact set through its own `ContentView` (budget charged
/// by declaration), so this copy is bounded by the content budget.
struct WireContent {
    contents: BTreeMap<ProjectPath, Vec<u8>>,
}

impl WireContent {
    fn fetch() -> Self {
        let mut contents = BTreeMap::new();
        for path in bindings::readable_paths() {
            if let Some(bytes) = bindings::read_file(&path) {
                contents.insert(ProjectPath::new(path), bytes);
            }
        }
        WireContent { contents }
    }
}

impl ContentAccess for WireContent {
    fn readable_paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_> {
        Box::new(self.contents.keys())
    }

    fn read(&self, path: &ProjectPath) -> Option<&[u8]> {
        self.contents.get(path).map(|b| b.as_slice())
    }
}

// ------------------------------------------------------------- the one export

/// Implements the generated `Guest` trait for any real [`Extension`]. Used
/// through [`export_extension!`]; public so the macro's expansion can name it.
pub struct ExportedExtension<E>(core::marker::PhantomData<E>);

impl<E: Extension + Default> bindings::Guest for ExportedExtension<E> {
    fn spec() -> wire::ExtensionSpec {
        spec_to_wire(E::default().spec())
    }

    fn extract(path: String, content: Vec<u8>) -> wire::FileEvidence {
        let extension = E::default();
        let path = ProjectPath::new(path);
        let mut sink = EvidenceSink::new(
            content.len() as u32,
            // The same pairing rule as the engine's own claim wiring: the sink is
            // constructed from the spec's declared streams.
            extension.spec().emits().clone(),
        );
        extension.extract(
            &SourceFile {
                path: &path,
                content: &content,
            },
            &mut sink,
        );
        evidence_to_wire(&sink.finish())
    }

    fn resolve(from: String, specifier: String) -> wire::Resolution {
        let from = ProjectPath::new(from);
        resolution_to_wire(E::default().resolve(&from, &specifier, &resolve_context()))
    }

    fn roots(manifest_path: String, content: Vec<u8>) -> Vec<wire::ProjectRoot> {
        let path = ProjectPath::new(manifest_path);
        let manifest = SourceFile {
            path: &path,
            content: &content,
        };
        E::default()
            .roots(&manifest, &resolve_context())
            .iter()
            .map(project_root_to_wire)
            .collect()
    }

    fn packages(manifest_path: String, content: Vec<u8>) -> Vec<wire::PackageEntry> {
        let path = ProjectPath::new(manifest_path);
        let manifest = SourceFile {
            path: &path,
            content: &content,
        };
        E::default()
            .packages(&manifest, &resolve_context())
            .iter()
            .map(package_entry_to_wire)
            .collect()
    }

    fn manifest_dependencies(manifest_path: String, content: Vec<u8>) -> Vec<String> {
        let path = ProjectPath::new(manifest_path);
        let manifest = SourceFile {
            path: &path,
            content: &content,
        };
        // The ABI speaks names only; a guest's scope/requirement stay guest-side
        // until a versioned world carries them.
        E::default()
            .manifest_dependencies(&manifest)
            .iter()
            .map(|d| d.name.to_string())
            .collect()
    }

    fn sees(path: String) -> Vec<String> {
        let path = ProjectPath::new(path);
        E::default()
            .sees(&path, &resolve_context())
            .iter()
            .map(|p| p.as_str().to_string())
            .collect()
    }

    fn seen_from(path: String, scope: String) -> Option<Vec<String>> {
        let path = ProjectPath::new(path);
        E::default()
            .seen_from(&path, &scope, &resolve_context())
            .map(|files| files.iter().map(|p| p.as_str().to_string()).collect())
    }

    fn contribute_roots() -> Vec<wire::ContributedRoot> {
        let extension = E::default();
        let graph = WireGraph::fetch();
        let content = WireContent::fetch();
        let mut sink = ConductSink::default();
        extension.contribute_roots(&graph, &content, &mut sink);
        let (roots, _, _) = sink.into_parts();
        roots
            .into_iter()
            .map(|r| wire::ContributedRoot {
                target: conduct_target_to_wire(&r.target),
                kind: root_kind_to_wire(r.kind),
                confidence: confidence_to_wire(r.confidence),
            })
            .collect()
    }

    fn report_findings() -> Vec<wire::ContributedFinding> {
        let extension = E::default();
        let graph = WireGraph::fetch();
        let content = WireContent::fetch();
        let mut sink = ConductSink::default();
        extension.report_findings(&graph, &content, &mut sink);
        let (_, findings, _) = sink.into_parts();
        findings
            .into_iter()
            .map(|f| wire::ContributedFinding {
                rule: f.rule.to_string(),
                severity: severity_to_wire(f.severity),
                target: conduct_target_to_wire(&f.target),
                confidence: confidence_to_wire(f.confidence),
                message: f.message,
            })
            .collect()
    }

    fn ingest(path: String, content: Vec<u8>) -> Option<wire::CoverageRecords> {
        E::default()
            .ingest(&path, &content)
            .as_ref()
            .map(records_to_wire)
    }
}

/// Export a real [`Extension`] as this component's `kndo:vocab/extension` world.
/// The author's type needs `Default`; everything else is the same trait a
/// built-in implements — whichever clusters its spec declares.
#[macro_export]
macro_rules! export_extension {
    ($extension:ty) => {
        type __KndoExportedExtension = $crate::ExportedExtension<$extension>;
        $crate::bindings::export!(__KndoExportedExtension with_types_in $crate::bindings);
    };
}
