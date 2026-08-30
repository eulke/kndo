//! Host-side conversions between the wire and the contract — the SOURCE TEXT
//! exists once per conversion. The generator names separate Rust types per world
//! for the same WIT records, so the plugin-family conversions are macro bodies
//! instantiated for the plugin and ingester worlds; the adapter world's are plain
//! functions (it has one consumer). The load-bearing piece is `replay_evidence`: a
//! component's evidence is never trusted as a value, it is REPLAYED through a real
//! [`EvidenceSink`] under the spec's declared streams, so every clamp, drop and
//! pairing rule applies to a WASM adapter exactly as to a native one — and the
//! ids the wire spells as indices come back out sink-issued and unforgeable, or
//! not at all.

use crate::bindings::adapter::kndo::vocab::types as awire;
use kndo_contract::adapter::{PackageEntry, Resolution};
use kndo_contract::evidence::{
    self as ev, DiagnosticLevel, EvidenceSink, EvidenceStream, EvidenceStreams, FileEvidence,
    RootKind,
};
use kndo_contract::extension::{ExtensionSpec, ExtensionSpecParts};
use kndo_contract::vocab::{Confidence, ProjectPath, Span};
use smol_str::SmolStr;

// ------------------------------------------------------------ the adapter world

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

pub(crate) fn adapter_spec(spec: awire::AdapterSpec) -> ExtensionSpec {
    ExtensionSpecParts {
        coordinate: SmolStr::new(spec.id),
        version: spec.semantics_version,
        claims: spec.claims.into_iter().map(SmolStr::new).collect(),
        emits: EvidenceStreams::of(
            &spec
                .emits
                .into_iter()
                .map(|s| match s {
                    awire::EvidenceStream::Comments => EvidenceStream::Comments,
                    awire::EvidenceStream::Metrics => EvidenceStream::Metrics,
                })
                .collect::<Vec<_>>(),
        ),
        manifests: spec.manifests.into_iter().map(SmolStr::new).collect(),
        extensions: spec.extensions.into_iter().map(SmolStr::new).collect(),
        ..Default::default()
    }
    .into()
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

fn symbol_kind(kind: awire::SymbolKind) -> ev::SymbolKind {
    match kind {
        awire::SymbolKind::Function => ev::SymbolKind::Function,
        awire::SymbolKind::Method => ev::SymbolKind::Method,
        awire::SymbolKind::Type => ev::SymbolKind::Type,
        awire::SymbolKind::Constant => ev::SymbolKind::Constant,
        awire::SymbolKind::Variable => ev::SymbolKind::Variable,
        awire::SymbolKind::Module => ev::SymbolKind::Module,
        awire::SymbolKind::Other(name) => ev::SymbolKind::Other(SmolStr::new(name)),
    }
}

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

/// Wire evidence through a real sink. `declares` comes from the loaded spec — the
/// pairing rule's host end — and every index the wire carries is bounds-checked
/// into a sink-issued id or dropped with a diagnostic.
pub(crate) fn replay_evidence(
    evidence: awire::FileEvidence,
    file_len: u32,
    declares: EvidenceStreams,
) -> FileEvidence {
    let mut sink = EvidenceSink::new(file_len, declares);

    let ids: Vec<_> = evidence
        .declarations
        .iter()
        .map(|d| {
            sink.declaration(
                SmolStr::new(&d.name),
                symbol_kind(d.kind.clone()),
                span(d.span),
                match d.reach {
                    awire::Reach::Private => ev::Reach::Private,
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
    }

    for r in evidence.references {
        sink.reference(SmolStr::new(r.name), ref_kind(r.kind), span(r.span));
    }
    for i in evidence.imports {
        sink.import(
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
                awire::ImportShape::TypeOnly(b) => ev::ImportShape::TypeOnly(bindings(b)),
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
    sink.finish()
}

// ------------------------------------------------------- the plugin-family worlds

/// The plugin-vocabulary conversions, written once and instantiated per world
/// (the plugin and ingester generations each name their own Rust types for the
/// same WIT records).
macro_rules! plugin_family_conversions {
    ($wire:path) => {
        use kndo_contract::extension::{
            Activation, ActivationRule, ExtensionSpecParts, RuleDescriptor,
        };
        use kndo_contract::vocab::ProjectPath;
        use smol_str::SmolStr;
        use $wire as w;

        /// The conduct half of a loaded spec, as owned parts — the caller states
        /// what its world implies (`mutates_graph` for plugins, report paths for
        /// ingesters) before assembling.
        pub(crate) fn plugin_parts(spec: w::PluginSpec) -> ExtensionSpecParts {
            ExtensionSpecParts {
                coordinate: SmolStr::new(spec.coordinate),
                version: spec.version,
                conducts: true,
                activation: match spec.activation {
                    w::Activation::Always => Activation::Always,
                    w::Activation::AnyRule(rules) => Activation::AnyRule(
                        rules
                            .into_iter()
                            .map(|r| match r {
                                w::ActivationRule::FileExists(g) => {
                                    ActivationRule::FileExists(SmolStr::new(g))
                                }
                                w::ActivationRule::ManifestDependency(n) => {
                                    ActivationRule::ManifestDependency(SmolStr::new(n))
                                }
                            })
                            .collect(),
                    ),
                },
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
                ..Default::default()
            }
        }
    };
}

pub(crate) mod plugin_wire {
    plugin_family_conversions!(crate::bindings::plugin::kndo::vocab::types);
    use kndo_contract::evidence::RootKind;
    use kndo_contract::vocab::Confidence;
    use kndo_core::plugin::{PluginSeverity, PluginTarget};

    pub(crate) fn confidence(c: w::Confidence) -> Confidence {
        match c {
            w::Confidence::Possible => Confidence::Possible,
            w::Confidence::Probable => Confidence::Probable,
            w::Confidence::Certain => Confidence::Certain,
        }
    }

    pub(crate) fn root_kind(k: w::RootKind) -> RootKind {
        match k {
            w::RootKind::Production => RootKind::Production,
            w::RootKind::Test => RootKind::Test,
            w::RootKind::Tooling => RootKind::Tooling,
        }
    }

    pub(crate) fn plugin_target(target: w::PluginTarget) -> PluginTarget {
        match target {
            w::PluginTarget::File(path) => PluginTarget::File(ProjectPath::new(path)),
            w::PluginTarget::Symbol(s) => PluginTarget::Symbol {
                path: ProjectPath::new(s.path),
                name: SmolStr::new(s.name),
            },
        }
    }

    pub(crate) fn plugin_severity(s: w::PluginSeverity) -> PluginSeverity {
        match s {
            w::PluginSeverity::Error => PluginSeverity::Error,
            w::PluginSeverity::Warning => PluginSeverity::Warning,
            w::PluginSeverity::Info => PluginSeverity::Info,
        }
    }
}

pub(crate) mod ingester_wire {
    plugin_family_conversions!(crate::bindings::ingester::kndo::vocab::types);

    /// Wire coverage records, grouped by the report's own paths — ready for
    /// `kndo_coverage::assemble`.
    pub(crate) fn coverage_records(records: w::CoverageRecords) -> kndo_coverage::CoverageRecords {
        let mut out = kndo_coverage::CoverageRecords::default();
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
}
