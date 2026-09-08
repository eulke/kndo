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
    self as ev, DiagnosticLevel, EmbeddedRegion, EvidenceSink, EvidenceStream, EvidenceStreams,
    RegionMode, RootKind,
};
use kndo_contract::manifest::UnitKind;
use kndo_contract::plugin::{
    Activation, ActivationRule, Bearer, CycleTolerance, DeclaredSymbol, DependencyBuiltins,
    DependencyIdentity, DependencyScoping, DispatchRule, Effect, FileRole, Ladder, NamespaceSpan,
    Nesting, PluginSeverity, PluginSpec, PluginSpecParts, PluginTarget, PublishedSurface,
    RuleDescriptor, Rung, Step, Trigger, UnnamedUnit,
};
use kndo_contract::vocab::{Confidence, ProjectPath, Span};
use smol_str::SmolStr;

pub(crate) fn region_to_wire(region: &EmbeddedRegion) -> awire::EmbeddedRegion {
    awire::EmbeddedRegion {
        span: awire::Span {
            start: region.span.start,
            end: region.span.end,
        },
        language: region.language.to_string(),
        mode: match region.mode {
            RegionMode::Module => awire::RegionMode::Module,
            RegionMode::Script => awire::RegionMode::Script,
        },
    }
}

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
pub(crate) fn extension_spec(spec: awire::PluginSpec) -> PluginSpec {
    PluginSpecParts {
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
        dispatch: spec
            .dispatch
            .into_iter()
            .filter_map(dispatch_rule)
            .collect(),
        namespace_span: match spec.namespace_span {
            awire::NamespaceSpan::Unit => NamespaceSpan::Unit,
            awire::NamespaceSpan::Compilation => NamespaceSpan::Compilation,
        },
        unnamed_unit: match spec.unnamed_unit {
            awire::UnnamedUnit::Unbounded => UnnamedUnit::Unbounded,
            awire::UnnamedUnit::Namespace => UnnamedUnit::Namespace,
        },
        nesting: match spec.nesting {
            awire::Nesting::PerFile => Nesting::PerFile,
            awire::Nesting::Flat => Nesting::Flat,
            awire::Nesting::ByDirectory => Nesting::ByDirectory,
            awire::Nesting::ByPath(roots) => Nesting::ByPath {
                roots: roots.into_iter().map(SmolStr::new).collect(),
            },
            awire::Nesting::Mounted => Nesting::Mounted,
        },
        file_roles: spec
            .file_roles
            .into_iter()
            .map(|r| FileRole {
                glob: SmolStr::new(r.glob),
                kind: root_kind(r.kind),
                confidence: confidence(r.confidence),
            })
            .collect(),
        dependency_scoping: match spec.dependency_scoping {
            awire::DependencyScoping::Scoped => DependencyScoping::Scoped,
            awire::DependencyScoping::Unscoped => DependencyScoping::Unscoped,
        },
        dependency_identity: match spec.dependency_identity {
            awire::DependencyIdentity::Underivable => DependencyIdentity::Underivable,
            awire::DependencyIdentity::PackageName => DependencyIdentity::PackageName,
            awire::DependencyIdentity::ModulePath => DependencyIdentity::ModulePath,
            awire::DependencyIdentity::CrateRoot => DependencyIdentity::CrateRoot,
        },
        dependency_importers: spec
            .dependency_importers
            .into_iter()
            .map(SmolStr::new)
            .collect(),
        dependency_builtins: match spec.dependency_builtins {
            awire::DependencyBuiltins::None => DependencyBuiltins::None,
            awire::DependencyBuiltins::Named(names) => {
                DependencyBuiltins::Named(names.into_iter().map(SmolStr::new).collect())
            }
            awire::DependencyBuiltins::UndottedFirstSegment => {
                DependencyBuiltins::UndottedFirstSegment
            }
        },
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
        ignores: spec.ignores.into_iter().map(SmolStr::new).collect(),
        ecosystem: spec.ecosystem.map(SmolStr::new),
        hidden_opt_in: spec.hidden_opt_in.into_iter().map(SmolStr::new).collect(),
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
                        awire::ActivationRule::FileImports(s) => {
                            ActivationRule::FileImports(SmolStr::new(s))
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

pub(crate) fn reach_from_wire(reach: &awire::Reach) -> ev::Reach {
    match reach {
        awire::Reach::Owner => ev::Reach::Owner,
        awire::Reach::File => ev::Reach::File,
        awire::Reach::Namespace(up) => ev::Reach::Namespace { up: *up },
        awire::Reach::Unit(up) => ev::Reach::Unit { up: *up },
        awire::Reach::Directory(up) => ev::Reach::Directory { up: *up },
        awire::Reach::Heirs(and_namespace) => ev::Reach::Heirs {
            and_namespace: *and_namespace,
        },
        awire::Reach::Named(namespace) => ev::Reach::Named {
            namespace: namespace.iter().map(SmolStr::new).collect(),
        },
        awire::Reach::Inherited => ev::Reach::Inherited,
        awire::Reach::Exported => ev::Reach::Exported,
    }
}

/// A rule the guest declared. `None` where its trigger tree is malformed —
/// an owner index that is not a smaller node than the one naming it, or an
/// empty list — because a rule the host cannot read is a rule it must not
/// guess at.
fn dispatch_rule(rule: awire::DispatchRule) -> Option<DispatchRule> {
    Some(DispatchRule {
        when: rebuild_trigger(&rule.when, rule.when.len().checked_sub(1)?)?,
        then: match rule.then {
            awire::Effect::Root(kind) => Effect::Root(root_kind(kind)),
            awire::Effect::Exempt => Effect::Exempt,
            awire::Effect::Generated => Effect::Generated,
            awire::Effect::Witness => Effect::Witness,
        },
        confidence: confidence(rule.confidence),
    })
}

fn unit_kind(kind: awire::UnitKind) -> UnitKind {
    match kind {
        awire::UnitKind::Library => UnitKind::Library,
        awire::UnitKind::Executable => UnitKind::Executable,
        awire::UnitKind::Test => UnitKind::Test,
        awire::UnitKind::Bench => UnitKind::Bench,
        awire::UnitKind::Example => UnitKind::Example,
        awire::UnitKind::Tooling => UnitKind::Tooling,
    }
}

fn qualifier(q: awire::Qualifier) -> ev::Qualifier {
    match q {
        awire::Qualifier::Binding(local) => ev::Qualifier::Binding(SmolStr::new(local)),
        awire::Qualifier::Path(segments) => {
            ev::Qualifier::Path(segments.into_iter().map(SmolStr::new).collect())
        }
    }
}

fn type_ref(t: awire::TypeRef) -> ev::TypeRef {
    ev::TypeRef {
        name: SmolStr::new(t.name),
        via: t.via.map(qualifier),
    }
}

kndo_contract::variant_map! {
    fn relation_kind(awire::RelationKind => ev::RelationKind) {
        Extends, Conforms, Implements, Overrides,
    }
}

/// The trigger at `at`, with its owner chain rebuilt. An owner must sit
/// EARLIER in the list than the node naming it, which is what the SDK's
/// flattening guarantees and what makes the walk terminate.
fn rebuild_trigger(nodes: &[awire::TriggerNode], at: usize) -> Option<Trigger> {
    Some(match nodes.get(at)? {
        awire::TriggerNode::Marker(m) => Trigger::Marker {
            path: SmolStr::new(&m.path),
            arg: m.arg.as_deref().map(SmolStr::new),
            target: m.target.clone().map(symbol_kind_from_wire),
        },
        awire::TriggerNode::Name(n) => Trigger::Name {
            pattern: SmolStr::new(&n.pattern),
            kind: n.kind.clone().map(symbol_kind_from_wire),
            in_unit: n.in_unit.map(unit_kind),
        },
        awire::TriggerNode::Relation(r) => Trigger::Relation {
            kind: relation_kind(r.kind),
            to: SmolStr::new(&r.to),
        },
        awire::TriggerNode::MemberOf(m) => {
            let owner = ((m.owner as usize) < at).then_some(m.owner as usize)?;
            Trigger::MemberOf {
                owner: Box::new(rebuild_trigger(nodes, owner)?),
                name: SmolStr::new(&m.name),
            }
        }
        awire::TriggerNode::ExternalWitness(w) => Trigger::ExternalWitness {
            base: SmolStr::new(&w.base),
            members: w.members.iter().map(SmolStr::new).collect(),
        },
    })
}

pub(crate) fn project_root(root: awire::ProjectRoot) -> ProjectRoot {
    ProjectRoot {
        file: ProjectPath::new(root.file),
        kind: root_kind(root.kind),
        confidence: confidence(root.confidence),
    }
}

/// Everything a guest said one manifest states, replayed through the real
/// `ManifestSink` — the same validation a native adapter's writes go through,
/// so a guest cannot assemble evidence the engine would not accept.
pub(crate) fn manifest_evidence(
    read: awire::ManifestEvidence,
    out: &mut kndo_contract::manifest::ManifestSink,
) {
    use kndo_contract::manifest::{PathAlias, Publication, Unit, UnitDep, UnitRoot};
    for unit in read.units {
        out.unit(Unit {
            name: SmolStr::new(unit.name),
            kind: unit_kind(unit.kind),
            roots: unit
                .roots
                .into_iter()
                .map(|r| UnitRoot {
                    path: SmolStr::new(r.path),
                    recursive: r.recursive,
                })
                .collect(),
            excludes: unit.excludes.into_iter().map(SmolStr::new).collect(),
            entries: unit.entries.into_iter().map(ProjectPath::new).collect(),
            depends_on: unit
                .depends_on
                .into_iter()
                .map(|d| UnitDep {
                    unit: SmolStr::new(d.unit),
                    friend: d.friend,
                })
                .collect(),
            publication: match unit.publication {
                awire::Publication::Published => Publication::Published,
                awire::Publication::Unpublished => Publication::Unpublished,
                awire::Publication::Unstated => Publication::Unstated,
            },
            namespace_root: unit.namespace_root.map(SmolStr::new),
        });
    }
    for entry in read.packages {
        out.package(package_entry(entry));
    }
    for declaration in read.dependencies {
        out.dependency(kndo_contract::adapter::DependencyDeclaration {
            name: SmolStr::new(declaration.name),
            scope: declaration.scope.map(dependency_scope),
            version_req: declaration.version_req.map(version_req),
        });
    }
    for name in read.mentions {
        out.mention(name);
    }
    for alias in read.aliases {
        out.alias(PathAlias {
            prefix: SmolStr::new(alias.prefix),
            targets: alias.targets.into_iter().map(SmolStr::new).collect(),
        });
    }
    for root in read.roots {
        out.root(project_root(root));
    }
    for path in read.ignores {
        out.ignore(path);
    }
    for member in read.members {
        out.member(ProjectPath::new(member));
    }
    for d in read.diagnostics {
        let level = match d.level {
            awire::DiagnosticLevel::Info => DiagnosticLevel::Info,
            awire::DiagnosticLevel::Warn => DiagnosticLevel::Warn,
            awire::DiagnosticLevel::Error => DiagnosticLevel::Error,
        };
        out.diagnostic(level, d.message);
    }
}

fn dependency_scope(scope: awire::DependencyScope) -> kndo_contract::adapter::DependencyScope {
    use kndo_contract::adapter::DependencyScope as S;
    match scope {
        awire::DependencyScope::Prod => S::Prod,
        awire::DependencyScope::Dev => S::Dev,
        awire::DependencyScope::Build => S::Build,
        awire::DependencyScope::Optional => S::Optional,
        awire::DependencyScope::Peer => S::Peer,
        awire::DependencyScope::Transitive => S::Transitive,
    }
}

pub(crate) fn plugin_target(target: awire::PluginTarget) -> PluginTarget {
    match target {
        awire::PluginTarget::File(path) => PluginTarget::File(ProjectPath::new(path)),
        awire::PluginTarget::Symbol(s) => PluginTarget::Symbol {
            path: ProjectPath::new(s.path),
            name: SmolStr::new(s.name),
        },
    }
}

pub(crate) fn plugin_severity(s: awire::PluginSeverity) -> PluginSeverity {
    match s {
        awire::PluginSeverity::Error => PluginSeverity::Error,
        awire::PluginSeverity::Warning => PluginSeverity::Warning,
        awire::PluginSeverity::Info => PluginSeverity::Info,
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
        aliases: entry.aliases.into_iter().map(SmolStr::new).collect(),
    }
}

pub(crate) fn package_entry_to_wire(entry: &PackageEntry) -> awire::PackageEntry {
    awire::PackageEntry {
        name: entry.name.to_string(),
        entry: entry.entry.as_ref().map(|p| p.as_str().to_string()),
        dir: entry.dir.to_string(),
        aliases: entry.aliases.iter().map(SmolStr::to_string).collect(),
    }
}

fn version_req(req: awire::VersionReq) -> kndo_contract::manifest::VersionReq {
    kndo_contract::manifest::VersionReq {
        spelled: SmolStr::new(req.spelled),
        range: req.range.map(|(lo, hi)| (version(lo), version(hi))),
    }
}

fn version(v: awire::Version) -> kndo_contract::manifest::Version {
    kndo_contract::manifest::Version::new(v.major, v.minor, v.patch)
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

kndo_contract::variant_map! {
    fn ref_kind(awire::RefKind => ev::RefKind) {
        Call, Read, Write, Extend, Implement, TypeUse,
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
    sink.namespace(evidence.namespace.iter().map(SmolStr::new));
    sink.attachment(match evidence.attachment {
        awire::Attachment::Regular => ev::Attachment::Regular,
        awire::Attachment::TestOnly => ev::Attachment::TestOnly,
    });
    let ids: Vec<_> = evidence
        .declarations
        .iter()
        .map(|d| {
            sink.declaration(
                SmolStr::new(&d.name),
                symbol_kind_from_wire(d.kind.clone()),
                span(d.span),
                reach_from_wire(&d.reach),
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
                awire::ImportTarget::Pattern(s) => ev::ImportTarget::Pattern(SmolStr::new(s)),
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
                awire::ImportShape::Include => ev::ImportShape::Include,
                awire::ImportShape::Mount(m) => ev::ImportShape::Mount {
                    namespace: SmolStr::new(m.namespace),
                    reach: reach_from_wire(&m.reach),
                },
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
            Some(id) => sink.relation(*id, relation_kind(r.kind), type_ref(r.to), span(r.span)),
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
    for r in evidence.embedded {
        sink.region(
            span(r.span),
            SmolStr::new(r.language),
            match r.mode {
                awire::RegionMode::Module => RegionMode::Module,
                awire::RegionMode::Script => RegionMode::Script,
            },
        );
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
