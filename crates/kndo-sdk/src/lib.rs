//! The guest half of the ABI — one world, one macro, the REAL trait. An external
//! author implements [`kndo_contract::plugin::Plugin`] — the same trait,
//! the same `EvidenceSink`, `ResolveContext`, `PluginSink` and content scope a
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
use kndo_contract::manifest::UnitKind;
use kndo_contract::plugin::{
    Activation, ActivationRule, Bearer, ContentAccess, CycleTolerance, DeclaredSymbol,
    DependencyBuiltins, DependencyIdentity, DependencyScoping, DispatchRule, Effect, GraphAccess,
    Nesting, Plugin, PluginSeverity, PluginSink, PluginSpec, PluginTarget, Rung, Step, Trigger,
    UnnamedUnit,
};
use kndo_contract::vocab::{Confidence, ProjectPath, Span};
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

#[doc(hidden)]
// The generated export lowers a record's fields to scalar arguments: the
// region an `extract` call carries makes its lowering wider than the lint
// allows, and the shape is the canonical ABI's, not ours.
#[allow(clippy::too_many_arguments)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "plugin",
        pub_export_macro: true,
    });
}

/// The vocabulary's generated types — one world, one Rust spelling of each record.
#[doc(hidden)]
pub use bindings::kndo::vocab::types as wire;

// ---------------------------------------------------------------- contract → wire

pub fn spec_to_wire(spec: &PluginSpec) -> wire::PluginSpec {
    wire::PluginSpec {
        coordinate: spec.coordinate().to_string(),
        version: spec.version(),
        suffixes: spec.suffixes().iter().map(|s| s.to_string()).collect(),
        ladder: spec.ladder().steps().iter().map(step_to_wire).collect(),
        import_cycles: match spec.import_cycles() {
            CycleTolerance::Tolerated => wire::CycleTolerance::Tolerated,
            CycleTolerance::Hazard => wire::CycleTolerance::Hazard,
        },
        dispatch: spec
            .dispatch_rules()
            .iter()
            .filter_map(dispatch_rule_to_wire)
            .collect(),
        claims: spec.claims().iter().map(|s| s.to_string()).collect(),
        // The declared set itself is the wire spelling — no second list to
        // forget when the contract grows a stream; a variant this SDK build
        // does not know yet degrades by omission, not by silent stripping.
        emits: spec.emits().iter().filter_map(stream_to_wire).collect(),
        manifests: spec.manifests().iter().map(|s| s.to_string()).collect(),
        launchers: spec.launchers().iter().map(|s| s.to_string()).collect(),
        ignores: spec.ignores().iter().map(|s| s.to_string()).collect(),
        // A variant this SDK build predates has no honest spelling, so it
        // degrades to the DEFAULT rather than to a neighbour: the same posture
        // `dispatch_rule_to_wire` takes, and the same one the whole contract
        // takes toward an undeclared capability.
        unnamed_unit: match spec.unnamed_unit() {
            UnnamedUnit::Namespace => wire::UnnamedUnit::Namespace,
            _ => wire::UnnamedUnit::Unbounded,
        },
        nesting: match spec.nesting() {
            Nesting::PerFile => wire::Nesting::PerFile,
            Nesting::ByUnit => wire::Nesting::ByUnit,
            Nesting::ByDirectory => wire::Nesting::ByDirectory,
            Nesting::ByPath { roots } => {
                wire::Nesting::ByPath(roots.iter().map(SmolStr::to_string).collect())
            }
            Nesting::Mounted => wire::Nesting::Mounted,
        },
        file_roles: spec
            .file_roles()
            .iter()
            .map(|r| wire::FileRole {
                glob: r.glob.to_string(),
                kind: root_kind_to_wire(r.kind),
                confidence: confidence_to_wire(r.confidence),
            })
            .collect(),
        dependency_scoping: match spec.dependency_scoping() {
            DependencyScoping::Scoped => wire::DependencyScoping::Scoped,
            DependencyScoping::Unscoped => wire::DependencyScoping::Unscoped,
        },
        dependency_identity: match spec.dependency_identity() {
            DependencyIdentity::Underivable => wire::DependencyIdentity::Underivable,
            DependencyIdentity::PackageName => wire::DependencyIdentity::PackageName,
            DependencyIdentity::ModulePath => wire::DependencyIdentity::ModulePath,
            DependencyIdentity::CrateRoot => wire::DependencyIdentity::CrateRoot,
        },
        dependency_importers: spec
            .dependency_importers()
            .iter()
            .map(SmolStr::to_string)
            .collect(),
        dependency_builtins: match spec.dependency_builtins() {
            DependencyBuiltins::None => wire::DependencyBuiltins::None,
            DependencyBuiltins::Named(names) => {
                wire::DependencyBuiltins::Named(names.iter().map(SmolStr::to_string).collect())
            }
            DependencyBuiltins::UndottedFirstSegment => {
                wire::DependencyBuiltins::UndottedFirstSegment
            }
        },
        ecosystem: spec.ecosystem().map(SmolStr::to_string),
        hidden_opt_in: spec
            .hidden_opt_in()
            .iter()
            .map(SmolStr::to_string)
            .collect(),
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

fn step_to_wire(step: &Step) -> wire::Step {
    wire::Step {
        rung: match step.rung {
            Rung::Owner => wire::Rung::Owner,
            Rung::File => wire::Rung::File,
            Rung::Namespace => wire::Rung::Namespace,
            Rung::Unit => wire::Rung::Unit,
            Rung::Exported => wire::Rung::Exported,
            // A rung this wire has no word for is the widest one — it can
            // never make a narrower step appear.
            _ => wire::Rung::Exported,
        },
        word: step.word.to_string(),
        bearer: match step.bearer {
            Bearer::Any => wire::Bearer::Any,
            Bearer::Free => wire::Bearer::Free,
            Bearer::Member => wire::Bearer::Member,
        },
    }
}

/// A rung this wire has no word for crosses as the widest one: a guest can
/// never accuse through a narrowness the host would have to guess at.
fn reach_to_wire(reach: &ev::Reach) -> wire::Reach {
    match reach {
        ev::Reach::Owner => wire::Reach::Owner,
        ev::Reach::File => wire::Reach::File,
        ev::Reach::Namespace { up } => wire::Reach::Namespace(*up),
        ev::Reach::Unit { up } => wire::Reach::Unit(*up),
        ev::Reach::Directory { up } => wire::Reach::Directory(*up),
        ev::Reach::Heirs { and_namespace } => wire::Reach::Heirs(*and_namespace),
        ev::Reach::Named { namespace } => {
            wire::Reach::Named(namespace.iter().map(|s| s.to_string()).collect())
        }
        ev::Reach::Inherited => wire::Reach::Inherited,
        _ => wire::Reach::Exported,
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
                    ActivationRule::FileImports(s) => {
                        wire::ActivationRule::FileImports(s.to_string())
                    }
                })
                .collect(),
        ),
    }
}

/// A rule to the wire. `None` where a coordinate of its trigger is one this
/// SDK build cannot spell: a narrowing that cannot cross would cross as
/// ABSENT, which widens the rule to every compilation instead of none, so the
/// rule is dropped whole rather than sent wider than its author wrote it.
fn dispatch_rule_to_wire(rule: &DispatchRule) -> Option<wire::DispatchRule> {
    let mut when = Vec::new();
    flatten_trigger(&rule.when, &mut when)?;
    Some(wire::DispatchRule {
        when,
        then: match rule.then {
            Effect::Root(kind) => wire::Effect::Root(root_kind_to_wire(kind)),
            Effect::Exempt => wire::Effect::Exempt,
            Effect::Generated => wire::Effect::Generated,
            Effect::Witness => wire::Effect::Witness,
        },
        confidence: confidence_to_wire(rule.confidence),
    })
}

/// Extends, and any link this SDK build predates: the weaker promise
/// crosses, which keeps a witness alive without claiming an interface the
/// guest never named.
fn qualifier_to_wire(q: &kndo_contract::evidence::Qualifier) -> wire::Qualifier {
    use kndo_contract::evidence::Qualifier;
    match q {
        Qualifier::Binding(local) => wire::Qualifier::Binding(local.to_string()),
        Qualifier::Path(segments) => {
            wire::Qualifier::Path(segments.iter().map(|s| s.to_string()).collect())
        }
    }
}

fn type_ref_to_wire(t: &kndo_contract::evidence::TypeRef) -> wire::TypeRef {
    wire::TypeRef {
        name: t.name.to_string(),
        via: t.via.as_ref().map(qualifier_to_wire),
    }
}

kndo_contract::variant_map! {
    /// A kind this SDK build cannot spell degrades toward the widest true
    /// sentence: the type promises the other's surface.
    fn relation_kind_to_wire(ev::RelationKind => wire::RelationKind) {
        Extends, Conforms, Implements, Overrides,
    } else wire::RelationKind::Implements
}

/// A trigger tree as the wire carries it: a flat list whose ROOT is the last
/// node, every owner already pushed before the node naming it.
fn flatten_trigger(trigger: &Trigger, out: &mut Vec<wire::TriggerNode>) -> Option<u32> {
    let node = match trigger {
        Trigger::Marker { path, arg, target } => wire::TriggerNode::Marker(wire::MarkerTrigger {
            path: path.to_string(),
            arg: arg.as_ref().map(|a| a.to_string()),
            target: target.as_ref().map(symbol_kind_to_wire),
        }),
        Trigger::Name {
            pattern,
            kind,
            in_unit,
        } => wire::TriggerNode::Name(wire::NameTrigger {
            pattern: pattern.to_string(),
            kind: kind.as_ref().map(symbol_kind_to_wire),
            in_unit: match in_unit {
                Some(kind) => Some(unit_kind_to_wire(*kind)?),
                None => None,
            },
        }),
        Trigger::Relation { kind, to } => wire::TriggerNode::Relation(wire::RelationTrigger {
            kind: relation_kind_to_wire(*kind),
            to: to.to_string(),
        }),
        Trigger::MemberOf { owner, name } => {
            let owner = flatten_trigger(owner, out)?;
            wire::TriggerNode::MemberOf(wire::MemberOfNode {
                owner,
                name: name.to_string(),
            })
        }
        Trigger::ExternalWitness { base, members } => {
            wire::TriggerNode::ExternalWitness(wire::ExternalWitnessTrigger {
                base: base.to_string(),
                members: members.iter().map(|m| m.to_string()).collect(),
            })
        }
    };
    out.push(node);
    Some((out.len() - 1) as u32)
}

fn stream_to_wire(stream: EvidenceStream) -> Option<wire::EvidenceStream> {
    match stream {
        EvidenceStream::Comments => Some(wire::EvidenceStream::Comments),
        EvidenceStream::Metrics => Some(wire::EvidenceStream::Metrics),
        EvidenceStream::Markers => Some(wire::EvidenceStream::Markers),
        EvidenceStream::Relations => Some(wire::EvidenceStream::Relations),
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

fn region_mode_to_wire(mode: ev::RegionMode) -> wire::RegionMode {
    match mode {
        ev::RegionMode::Module => wire::RegionMode::Module,
        ev::RegionMode::Script => wire::RegionMode::Script,
    }
}

fn region_to_wire(region: &ev::EmbeddedRegion) -> wire::EmbeddedRegion {
    wire::EmbeddedRegion {
        span: span_to_wire(region.span),
        language: region.language.to_string(),
        mode: region_mode_to_wire(region.mode),
    }
}

fn region_from_wire(region: wire::EmbeddedRegion) -> ev::EmbeddedRegion {
    ev::EmbeddedRegion {
        span: Span::new(region.span.start, region.span.end),
        language: SmolStr::new(region.language),
        mode: match region.mode {
            wire::RegionMode::Module => ev::RegionMode::Module,
            wire::RegionMode::Script => ev::RegionMode::Script,
        },
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

kndo_contract::variant_map! {
    /// An unknown kind counts as a use, never an accusation — `Read` is the
    /// weakest keep-alive spelling the wire has.
    fn ref_kind_to_wire(ev::RefKind => wire::RefKind) {
        Call, Read, Write, Extend, Implement, TypeUse,
    } else wire::RefKind::Read
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
            ev::ImportTarget::Pattern(s) => wire::ImportTarget::Pattern(s.to_string()),
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
            ev::ImportShape::Glob => wire::ImportShape::Glob,
            ev::ImportShape::Include => wire::ImportShape::Include,
            ev::ImportShape::Mount { namespace, reach } => {
                wire::ImportShape::Mount(wire::MountPoint {
                    namespace: namespace.to_string(),
                    reach: reach_to_wire(reach),
                })
            }
            // An unknown shape keeps everything alive — SideEffect is that posture.
            _ => wire::ImportShape::SideEffect,
        },
        span: span_to_wire(import.span),
        confidence: confidence_to_wire(import.confidence),
        timing: match import.timing {
            ev::Timing::Load => wire::Timing::Load,
            ev::Timing::Erased => wire::Timing::Erased,
            // Lazy, and any moment this SDK build predates: reached, never a
            // hazard — the contract's own reading of an unknown timing.
            _ => wire::Timing::Lazy,
        },
    }
}

/// Finished evidence to the wire — what the extract shim sends back after the
/// author's real `EvidenceSink` pass.
pub fn evidence_to_wire(evidence: &FileEvidence) -> wire::FileEvidence {
    wire::FileEvidence {
        namespace: evidence.namespace.iter().map(|s| s.to_string()).collect(),
        attachment: match evidence.attachment {
            ev::Attachment::Regular => wire::Attachment::Regular,
            ev::Attachment::TestOnly => wire::Attachment::TestOnly,
        },
        declarations: evidence
            .declarations
            .iter()
            .map(|d| wire::Declaration {
                name: d.name.to_string(),
                kind: symbol_kind_to_wire(&d.kind),
                span: span_to_wire(d.span),
                reach: reach_to_wire(&d.reach),
                owner: d.owner.map(|id| id.index() as u32),
                exported_as: d.exported_as.as_ref().map(|s| s.to_string()),
                signature: d.signature.as_ref().map(|s| s.to_string()),
            })
            .collect(),
        references: evidence
            .references
            .iter()
            .map(|r| wire::Reference {
                name: r.name.to_string(),
                kind: ref_kind_to_wire(r.kind),
                on: r.on.as_ref().map(|o| o.to_string()),
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
        markers: evidence
            .markers
            .iter()
            .map(|m| wire::Marker {
                on: match &m.on {
                    ev::MarkerTarget::File => wire::MarkerTarget::File,
                    ev::MarkerTarget::Declaration(id) => {
                        wire::MarkerTarget::Declaration(id.index() as u32)
                    }
                    // A target this SDK build predates marks the file: the
                    // rules see it, and a file marker can only add roots or a
                    // reported blanket — never an accusation.
                    _ => wire::MarkerTarget::File,
                },
                path: m.path.to_string(),
                args: m.args.iter().map(|a| a.to_string()).collect(),
                span: span_to_wire(m.span),
            })
            .collect(),
        relations: evidence
            .relations
            .iter()
            .map(|r| wire::Relation {
                from: r.from.index() as u32,
                kind: relation_kind_to_wire(r.kind),
                to: type_ref_to_wire(&r.to),
                span: span_to_wire(r.span),
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
        embedded: evidence.embedded.iter().map(region_to_wire).collect(),
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

/// A unit kind to the wire, or `None` for one this SDK build predates — see
/// [`dispatch_rule_to_wire`] for why the rule then goes nowhere at all.
fn unit_kind_to_wire(kind: UnitKind) -> Option<wire::UnitKind> {
    Some(match kind {
        UnitKind::Library => wire::UnitKind::Library,
        UnitKind::Executable => wire::UnitKind::Executable,
        UnitKind::Test => wire::UnitKind::Test,
        UnitKind::Bench => wire::UnitKind::Bench,
        UnitKind::Example => wire::UnitKind::Example,
        UnitKind::Tooling => wire::UnitKind::Tooling,
        _ => return None,
    })
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
        aliases: entry.aliases.iter().map(SmolStr::to_string).collect(),
        subpaths: entry.subpaths.iter().map(path_alias_to_wire).collect(),
    }
}

/// A path alias off the wire, with its conditions.
fn path_alias(a: wire::PathAlias) -> kndo_contract::manifest::PathAlias {
    kndo_contract::manifest::PathAlias {
        pattern: SmolStr::new(a.pattern),
        targets: a
            .targets
            .into_iter()
            .map(|t| kndo_contract::manifest::AliasTarget {
                template: SmolStr::new(t.template),
                conditions: t.conditions.into_iter().map(SmolStr::new).collect(),
            })
            .collect(),
    }
}

/// A path alias to the wire, with its conditions.
fn path_alias_to_wire(a: &kndo_contract::manifest::PathAlias) -> wire::PathAlias {
    wire::PathAlias {
        pattern: a.pattern.to_string(),
        targets: a
            .targets
            .iter()
            .map(|t| wire::AliasTarget {
                template: t.template.to_string(),
                conditions: t.conditions.iter().map(SmolStr::to_string).collect(),
            })
            .collect(),
    }
}

/// Everything one manifest stated, to the wire. A unit kind this SDK build
/// predates has no honest spelling, so the unit carrying it is dropped rather
/// than recoloured — the same posture `dispatch_rule_to_wire` takes.
pub fn manifest_evidence_to_wire(
    read: &kndo_contract::manifest::ManifestEvidence,
) -> wire::ManifestEvidence {
    wire::ManifestEvidence {
        units: read
            .units
            .iter()
            .filter_map(|u| {
                Some(wire::Unit {
                    name: u.name.to_string(),
                    kind: unit_kind_to_wire(u.kind)?,
                    roots: u
                        .roots
                        .iter()
                        .map(|r| wire::UnitRoot {
                            path: r.path.to_string(),
                            recursive: r.recursive,
                        })
                        .collect(),
                    excludes: u.excludes.iter().map(|e| e.to_string()).collect(),
                    entries: u.entries.iter().map(|e| e.as_str().to_string()).collect(),
                    depends_on: u
                        .depends_on
                        .iter()
                        .map(|d| wire::UnitDep {
                            unit: d.unit.to_string(),
                            grants: match d.grants {
                                kndo_contract::manifest::Grant::Exports => wire::Grant::Exports,
                                kndo_contract::manifest::Grant::Namespace => wire::Grant::Namespace,
                                kndo_contract::manifest::Grant::Unit => wire::Grant::Unit,
                            },
                        })
                        .collect(),
                    publication: match u.publication {
                        kndo_contract::manifest::Publication::ByName => wire::Publication::ByName,
                        kndo_contract::manifest::Publication::ByEntry => wire::Publication::ByEntry,
                        kndo_contract::manifest::Publication::Unpublished => {
                            wire::Publication::Unpublished
                        }
                        kndo_contract::manifest::Publication::Unstated => {
                            wire::Publication::Unstated
                        }
                    },
                    namespace_root: u.namespace_root.as_ref().map(SmolStr::to_string),
                })
            })
            .collect(),
        packages: read.packages.iter().map(package_entry_to_wire).collect(),
        dependencies: read
            .dependencies
            .iter()
            .map(|d| wire::DependencyDeclaration {
                name: d.name.to_string(),
                scope: d.scope.map(dependency_scope_to_wire),
                version_req: d.version_req.as_ref().map(version_req_to_wire),
            })
            .collect(),
        mentions: read.mentions.iter().map(|m| m.to_string()).collect(),
        aliases: read.aliases.iter().map(path_alias_to_wire).collect(),
        roots: read.roots.iter().map(project_root_to_wire).collect(),
        ignores: read.ignores.iter().map(SmolStr::to_string).collect(),
        members: read
            .members
            .iter()
            .map(|m| m.as_str().to_string())
            .collect(),
        diagnostics: read
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

fn version_req_to_wire(req: &kndo_contract::manifest::VersionReq) -> wire::VersionReq {
    let version = |v: &kndo_contract::manifest::Version| wire::Version {
        major: v.major,
        minor: v.minor,
        patch: v.patch,
    };
    wire::VersionReq {
        spelled: req.spelled.to_string(),
        range: req
            .range
            .as_ref()
            .map(|(lo, hi)| (version(lo), version(hi))),
    }
}

fn dependency_scope_to_wire(
    scope: kndo_contract::adapter::DependencyScope,
) -> wire::DependencyScope {
    use kndo_contract::adapter::DependencyScope as S;
    match scope {
        S::Prod => wire::DependencyScope::Prod,
        S::Dev => wire::DependencyScope::Dev,
        S::Build => wire::DependencyScope::Build,
        S::Optional => wire::DependencyScope::Optional,
        S::Peer => wire::DependencyScope::Peer,
        S::Transitive => wire::DependencyScope::Transitive,
    }
}

fn package_entry_from_wire(entry: wire::PackageEntry) -> PackageEntry {
    PackageEntry {
        name: SmolStr::new(entry.name),
        entry: entry.entry.map(ProjectPath::new),
        dir: SmolStr::new(entry.dir),
        aliases: entry.aliases.into_iter().map(SmolStr::new).collect(),
        subpaths: entry.subpaths.into_iter().map(path_alias).collect(),
    }
}

fn plugin_target_to_wire(target: &PluginTarget) -> wire::PluginTarget {
    match target {
        PluginTarget::File(p) => wire::PluginTarget::File(p.as_str().to_string()),
        PluginTarget::Symbol { path, name } => wire::PluginTarget::Symbol(wire::SymbolRef {
            path: path.as_str().to_string(),
            name: name.to_string(),
        }),
    }
}

fn severity_to_wire(severity: PluginSeverity) -> wire::PluginSeverity {
    match severity {
        PluginSeverity::Error => wire::PluginSeverity::Error,
        PluginSeverity::Warning => wire::PluginSeverity::Warning,
        PluginSeverity::Info => wire::PluginSeverity::Info,
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
/// the call as a phase violation. Use it from `resolve` and `extract_manifest`,
/// where the project enumerations are the contract.
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

/// Implements the generated `Guest` trait for any real [`Plugin`]. Used
/// through [`export_extension!`]; public so the macro's expansion can name it.
pub struct ExportedExtension<E>(core::marker::PhantomData<E>);

impl<E: Plugin + Default> bindings::Guest for ExportedExtension<E> {
    fn spec() -> wire::PluginSpec {
        spec_to_wire(E::default().spec())
    }

    fn extract(
        path: String,
        content: Vec<u8>,
        region: Option<wire::EmbeddedRegion>,
    ) -> wire::FileEvidence {
        let extension = E::default();
        let path = ProjectPath::new(path);
        let region = region.map(region_from_wire);
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
                region: region.as_ref(),
            },
            &mut sink,
        );
        evidence_to_wire(&sink.finish())
    }

    fn resolve(from: String, specifier: String) -> wire::Resolution {
        let from = ProjectPath::new(from);
        resolution_to_wire(E::default().resolve(&from, &specifier, &resolve_context()))
    }

    fn extract_manifest(manifest_path: String, content: Vec<u8>) -> wire::ManifestEvidence {
        let path = ProjectPath::new(manifest_path);
        let manifest = SourceFile {
            path: &path,
            content: &content,
            region: None,
        };
        let mut sink = kndo_contract::manifest::ManifestSink::new();
        E::default().extract_manifest(&manifest, &resolve_context(), &mut sink);
        manifest_evidence_to_wire(&sink.finish())
    }

    fn contribute_roots() -> Vec<wire::ContributedRoot> {
        let extension = E::default();
        let graph = WireGraph::fetch();
        let content = WireContent::fetch();
        let mut sink = PluginSink::default();
        extension.contribute_roots(&graph, &content, &mut sink);
        let (roots, _, _) = sink.into_parts();
        roots
            .into_iter()
            .map(|r| wire::ContributedRoot {
                target: plugin_target_to_wire(&r.target),
                kind: root_kind_to_wire(r.kind),
                confidence: confidence_to_wire(r.confidence),
            })
            .collect()
    }

    fn report_findings() -> Vec<wire::ContributedFinding> {
        let extension = E::default();
        let graph = WireGraph::fetch();
        let content = WireContent::fetch();
        let mut sink = PluginSink::default();
        extension.report_findings(&graph, &content, &mut sink);
        let (_, findings, _) = sink.into_parts();
        findings
            .into_iter()
            .map(|f| wire::ContributedFinding {
                rule: f.rule.to_string(),
                severity: severity_to_wire(f.severity),
                target: plugin_target_to_wire(&f.target),
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

/// Export a real [`Plugin`] as this component's `kndo:vocab/extension` world.
/// The author's type needs `Default`; everything else is the same trait a
/// built-in implements — whichever clusters its spec declares.
#[macro_export]
macro_rules! export_extension {
    ($extension:ty) => {
        type __KndoExportedExtension = $crate::ExportedExtension<$extension>;
        $crate::bindings::export!(__KndoExportedExtension with_types_in $crate::bindings);
    };
}
