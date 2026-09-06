//! Host-side conversions between the wire and the contract — ONE world, so the
//! source text exists once, with one set of generated types, and the macro
//! instantiation the three-world generator forced is gone. The load-bearing
//! piece is `replay_evidence`: a component's evidence is never trusted as a
//! value, it is REPLAYED through a real [`EvidenceSink`] under the spec's
//! declared streams, so every clamp, drop and pairing rule applies to a WASM
//! extension exactly as to a native one — and the ids the wire spells as
//! indices come back out sink-issued and unforgeable, or not at all.

use crate::bindings::kndo::vocab::types as awire;
use kndo_contract::adapter::{PackageEntry, ProjectRoot, Resolution};
use kndo_contract::evidence::{
    self as ev, DiagnosticLevel, EvidenceSink, EvidenceStream, EvidenceStreams, RootKind,
};
use kndo_contract::extension::{
    Activation, ActivationRule, Bearer, ConductSeverity, ConductTarget, CycleTolerance,
    DeclaredSymbol, DispatchRule, Effect, ExtensionSpec, ExtensionSpecParts, Ladder,
    PublishedSurface, RuleDescriptor, Rung, Step, Trigger,
};
use kndo_contract::vocab::{Confidence, ProjectPath, Span};
use smol_str::SmolStr;

pub(crate) fn span(s: awire::Span) -> Span {
    // A hostile start > end normalizes to the empty span at the lower offset; the
    // sink's own clamp handles ends past the file.
    Span::new(s.start.min(s.end), s.end.max(s.start))
}

pub(crate) fn confidence(c: awire::Confidence) -> Confidence {
    match c {
        awire::Confidence::Possible => Confidence::Possible,
        awire::Confidence::Probable => Confidence::Probable,
        awire::Confidence::Certain => Confidence::Certain,
    }
}

pub(crate) fn root_kind(k: awire::RootKind) -> RootKind {
    match k {
        awire::RootKind::Production => RootKind::Production,
        awire::RootKind::Test => RootKind::Test,
        awire::RootKind::Tooling => RootKind::Tooling,
    }
}

/// The one spec record, assembled as owned parts. `conducts` comes from the
/// record itself: a hand-rolled guest is forced by the shape to state it.
pub(crate) fn extension_spec(spec: awire::ExtensionSpec) -> ExtensionSpec {
    ExtensionSpecParts {
        coordinate: SmolStr::new(spec.coordinate),
        version: spec.version,
        suffixes: spec.suffixes.into_iter().map(SmolStr::new).collect(),
        ladder: Ladder::new(spec.ladder.into_iter().map(step).collect()),
        published_surface: match spec.published_surface {
            awire::PublishedSurface::Exports => PublishedSurface::Exports,
            awire::PublishedSurface::Entries => PublishedSurface::Entries,
        },
        claims: spec.claims.into_iter().map(SmolStr::new).collect(),
        import_cycles: match spec.import_cycles {
            awire::CycleTolerance::Tolerated => CycleTolerance::Tolerated,
            awire::CycleTolerance::Hazard => CycleTolerance::Hazard,
        },
        dispatch: spec.dispatch.into_iter().map(dispatch_rule).collect(),
        namespace_span: Default::default(),
        // The wire world speaks no dependency vocabulary yet; absence
        // defaults to silence, like every other undeclared capability.
        dependency_scoping: Default::default(),
        dependency_identity: Default::default(),
        dependency_importers: Vec::new(),
        dependency_builtins: Default::default(),
        emits: EvidenceStreams::of(
            &spec
                .emits
                .into_iter()
                .map(|s| match s {
                    awire::EvidenceStream::Comments => EvidenceStream::Comments,
                    awire::EvidenceStream::Metrics => EvidenceStream::Metrics,
                    awire::EvidenceStream::Markers => EvidenceStream::Markers,
                    awire::EvidenceStream::Relations => EvidenceStream::Relations,
                    awire::EvidenceStream::Qualifiers => EvidenceStream::Qualifiers,
                })
                .collect::<Vec<_>>(),
        ),
        manifests: spec.manifests.into_iter().map(SmolStr::new).collect(),
        launchers: spec.launchers.into_iter().map(SmolStr::new).collect(),
        conducts: spec.conducts,
        activation: match spec.activation {
            awire::Activation::Always => Activation::Always,
            awire::Activation::AnyRule(rules) => Activation::AnyRule(
                rules
                    .into_iter()
                    .map(|r| match r {
                        awire::ActivationRule::FileExists(g) => {
                            ActivationRule::FileExists(SmolStr::new(g))
                        }
                        awire::ActivationRule::ManifestDependency(n) => {
                            ActivationRule::ManifestDependency(SmolStr::new(n))
                        }
                    })
                    .collect(),
            ),
        },
        mutates_graph: spec.mutates_graph,
        dependencies: spec.dependencies.into_iter().map(SmolStr::new).collect(),
        requested_file_access: spec
            .requested_file_access
            .into_iter()
            .map(SmolStr::new)
            .collect(),
        rules: spec
            .rules
            .into_iter()
            .map(|r| RuleDescriptor {
                name: SmolStr::new(r.name),
                description: SmolStr::new(r.description),
            })
            .collect(),
        reads_reports: spec.reads_reports.into_iter().map(SmolStr::new).collect(),
    }
    .into()
}

fn step(s: awire::Step) -> Step {
    Step {
        rung: match s.rung {
            awire::Rung::Owner => Rung::Owner,
            awire::Rung::File => Rung::File,
            awire::Rung::Namespace => Rung::Namespace,
            awire::Rung::Unit => Rung::Unit,
            awire::Rung::Exported => Rung::Exported,
        },
        word: SmolStr::new(s.word),
        bearer: match s.bearer {
            awire::Bearer::Any => Bearer::Any,
            awire::Bearer::Free => Bearer::Free,
            awire::Bearer::Member => Bearer::Member,
        },
    }
}

/// The reach the host asks a guest's `seen-from` about. A rung the wire has
/// no word for crosses as the widest one: the guest then bounds nothing, and
/// the host judges the declaration as exported, keep-alive.
pub(crate) fn reach_to_wire(reach: &ev::Reach) -> awire::Reach {
    match reach {
        ev::Reach::Private => awire::Reach::Private,
        ev::Reach::Unit => awire::Reach::Unit,
        ev::Reach::Scoped { scope } => awire::Reach::Scoped(scope.to_string()),
        _ => awire::Reach::Exported,
    }
}

fn dispatch_rule(rule: awire::DispatchRule) -> DispatchRule {
    DispatchRule {
        when: match rule.when {
            awire::Trigger::Marker(m) => Trigger::Marker {
                path: SmolStr::new(m.path),
                arg: m.arg.map(SmolStr::new),
            },
        },
        then: match rule.then {
            awire::Effect::Root(kind) => Effect::Root(root_kind(kind)),
            awire::Effect::Exempt => Effect::Exempt,
        },
        confidence: confidence(rule.confidence),
    }
}

pub(crate) fn project_root(root: awire::ProjectRoot) -> ProjectRoot {
    ProjectRoot {
        file: ProjectPath::new(root.file),
        kind: root_kind(root.kind),
        confidence: confidence(root.confidence),
    }
}

pub(crate) fn conduct_target(target: awire::ConductTarget) -> ConductTarget {
    match target {
        awire::ConductTarget::File(path) => ConductTarget::File(ProjectPath::new(path)),
        awire::ConductTarget::Symbol(s) => ConductTarget::Symbol {
            path: ProjectPath::new(s.path),
            name: SmolStr::new(s.name),
        },
    }
}

pub(crate) fn conduct_severity(s: awire::ConductSeverity) -> ConductSeverity {
    match s {
        awire::ConductSeverity::Error => ConductSeverity::Error,
        awire::ConductSeverity::Warning => ConductSeverity::Warning,
        awire::ConductSeverity::Info => ConductSeverity::Info,
    }
}

/// Wire coverage records, grouped by the report's own paths — ready for
/// the engine's coverage assembly.
pub(crate) fn coverage_records(records: awire::CoverageRecords) -> ev::CoverageRecords {
    let mut out = ev::CoverageRecords::default();
    for line in records.lines {
        let file = out.files.entry(ProjectPath::new(line.path)).or_default();
        *file.lines.entry(line.line).or_insert(0) += line.hits;
    }
    for f in records.functions {
        out.files
            .entry(ProjectPath::new(f.path))
            .or_default()
            .functions
            .push((f.line, f.hits));
    }
    out
}

pub(crate) fn package_entry(entry: awire::PackageEntry) -> PackageEntry {
    PackageEntry {
        name: SmolStr::new(entry.name),
        entry: entry.entry.map(ProjectPath::new),
        dir: SmolStr::new(entry.dir),
    }
}

pub(crate) fn package_entry_to_wire(entry: &PackageEntry) -> awire::PackageEntry {
    awire::PackageEntry {
        name: entry.name.to_string(),
        entry: entry.entry.as_ref().map(|p| p.as_str().to_string()),
        dir: entry.dir.to_string(),
    }
}

pub(crate) fn resolution(r: awire::Resolution) -> Resolution {
    match r {
        awire::Resolution::File(p) => Resolution::File(ProjectPath::new(p)),
        awire::Resolution::Files(ps) => {
            Resolution::Files(ps.into_iter().map(ProjectPath::new).collect())
        }
        awire::Resolution::Unresolved => Resolution::Unresolved,
    }
}

pub(crate) fn declared_symbol_to_wire(d: DeclaredSymbol<'_>) -> awire::DeclaredSymbol {
    awire::DeclaredSymbol {
        path: d.path.as_str().to_string(),
        name: d.name.to_string(),
        kind: symbol_kind_to_wire(d.kind),
        owner: d.owner.map(str::to_string),
    }
}

use awire::SymbolKind as WireSymbolKind;
kndo_contract::symbol_kind_conversions!(WireSymbolKind);

fn ref_kind(kind: awire::RefKind) -> ev::RefKind {
    match kind {
        awire::RefKind::Call => ev::RefKind::Call,
        awire::RefKind::Read => ev::RefKind::Read,
        awire::RefKind::Write => ev::RefKind::Write,
        awire::RefKind::Extend => ev::RefKind::Extend,
        awire::RefKind::Implement => ev::RefKind::Implement,
        awire::RefKind::TypeUse => ev::RefKind::TypeUse,
    }
}

fn bindings(b: Vec<awire::ImportBinding>) -> Vec<ev::ImportBinding> {
    b.into_iter()
        .map(|b| ev::ImportBinding {
            imported: SmolStr::new(b.imported),
            local: SmolStr::new(b.local),
        })
        .collect()
}

/// Wire evidence replayed into the ENGINE'S sink — the one the engine primed
/// with the spec's declared streams, so the pairing rule and every clamp apply
/// to the wire exactly as to native writes, and every index the wire carries
/// is bounds-checked into a sink-issued id or dropped with a diagnostic. One
/// sink, not a validating copy then a field-by-field transfer: a transfer
/// enumerates fields by hand, and the first field it forgot (an import's
/// timing) was dropped in silence.
pub(crate) fn replay_evidence(evidence: awire::FileEvidence, sink: &mut EvidenceSink) {
    let ids: Vec<_> = evidence
        .declarations
        .iter()
        .map(|d| {
            sink.declaration(
                SmolStr::new(&d.name),
                symbol_kind_from_wire(d.kind.clone()),
                span(d.span),
                match &d.reach {
                    awire::Reach::Private => ev::Reach::Private,
                    awire::Reach::Unit => ev::Reach::Unit,
                    awire::Reach::Scoped(scope) => ev::Reach::Scoped {
                        scope: SmolStr::new(scope),
                    },
                    awire::Reach::Exported => ev::Reach::Exported,
                },
            )
        })
        .collect();
    for (ix, d) in evidence.declarations.iter().enumerate() {
        if let Some(owner) = d.owner {
            let owner = owner as usize;
            if owner < ids.len() && owner != ix {
                sink.member_of(ids[ix], ids[owner]);
            } else {
                sink.diagnostic(
                    DiagnosticLevel::Warn,
                    format!("owner index {owner} out of range (component defect)"),
                    None,
                );
            }
        }
        if let Some(alias) = &d.exported_as {
            sink.exported_as(ids[ix], SmolStr::new(alias));
        }
        if let Some(signature) = &d.signature {
            sink.signature(ids[ix], SmolStr::new(signature));
        }
    }

    for r in evidence.references {
        sink.reference_on(
            SmolStr::new(r.name),
            ref_kind(r.kind),
            r.on.map(SmolStr::new),
            span(r.span),
        );
    }
    for i in evidence.imports {
        sink.import_at(
            match i.timing {
                awire::Timing::Load => ev::Timing::Load,
                awire::Timing::Lazy => ev::Timing::Lazy,
                awire::Timing::Erased => ev::Timing::Erased,
            },
            match i.target {
                awire::ImportTarget::Relative(s) => ev::ImportTarget::Relative(SmolStr::new(s)),
                awire::ImportTarget::Package(s) => ev::ImportTarget::Package(SmolStr::new(s)),
            },
            match i.shape {
                awire::ImportShape::Bindings(b) => ev::ImportShape::Bindings(bindings(b)),
                awire::ImportShape::Namespace(local) => ev::ImportShape::Namespace {
                    local: SmolStr::new(local),
                },
                awire::ImportShape::SideEffect => ev::ImportShape::SideEffect,
                awire::ImportShape::Reexport(b) => ev::ImportShape::Reexport(bindings(b)),
                awire::ImportShape::ReexportAll => ev::ImportShape::ReexportAll,
                awire::ImportShape::Glob => ev::ImportShape::Glob,
            },
            span(i.span),
            confidence(i.confidence),
        );
    }
    for r in evidence.roots {
        let target = match r.target {
            awire::RootTarget::WholeFile => ev::RootTarget::WholeFile,
            awire::RootTarget::Declaration(ix) => match ids.get(ix as usize) {
                Some(id) => ev::RootTarget::Declaration(*id),
                None => {
                    sink.diagnostic(
                        DiagnosticLevel::Warn,
                        format!("root declaration index {ix} out of range (component defect)"),
                        None,
                    );
                    continue;
                }
            },
        };
        sink.root(target, root_kind(r.kind), confidence(r.confidence));
    }
    for m in evidence.markers {
        let on = match m.on {
            awire::MarkerTarget::File => ev::MarkerTarget::File,
            awire::MarkerTarget::Declaration(ix) => match ids.get(ix as usize) {
                Some(id) => ev::MarkerTarget::Declaration(*id),
                None => {
                    sink.diagnostic(
                        DiagnosticLevel::Warn,
                        format!("marker declaration index {ix} out of range (component defect)"),
                        None,
                    );
                    continue;
                }
            },
        };
        sink.marker(
            on,
            SmolStr::new(m.path),
            m.args.into_iter().map(SmolStr::new).collect(),
            span(m.span),
        );
    }
    for r in evidence.relations {
        match ids.get(r.from as usize) {
            Some(id) => sink.relation(
                *id,
                match r.kind {
                    awire::RelationKind::Extends => ev::RelationKind::Extends,
                    awire::RelationKind::Implements => ev::RelationKind::Implements,
                },
                SmolStr::new(r.to),
                span(r.span),
            ),
            None => sink.diagnostic(
                DiagnosticLevel::Warn,
                format!(
                    "relation declaration index {} out of range (component defect)",
                    r.from
                ),
                None,
            ),
        }
    }
    for c in evidence.comments {
        sink.comment(span(c.span), span(c.text));
    }
    for m in evidence.metrics {
        match ids.get(m.declaration as usize) {
            Some(id) => sink.metrics(
                *id,
                ev::FunctionMetrics {
                    cyclomatic: m.metrics.cyclomatic,
                    loc: m.metrics.loc,
                    token_count: m.metrics.token_count,
                    fingerprints: m.metrics.fingerprints,
                },
            ),
            None => sink.diagnostic(
                DiagnosticLevel::Warn,
                format!(
                    "metrics declaration index {} out of range (component defect)",
                    m.declaration
                ),
                None,
            ),
        }
    }
    for d in evidence.diagnostics {
        sink.diagnostic(
            match d.level {
                awire::DiagnosticLevel::Info => DiagnosticLevel::Info,
                awire::DiagnosticLevel::Warn => DiagnosticLevel::Warn,
                awire::DiagnosticLevel::Error => DiagnosticLevel::Error,
            },
            d.message,
            d.span.map(span),
        );
    }
}
