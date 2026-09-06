//! The query contract: the graph sold back to agents at minimal token cost.
//! One request shape, one response envelope (`kndo-query/1`), consumed
//! identically by the CLI verbs and the serve tools — and answered from the
//! SAME `navigate` index the `unused` judgment ran on, so `used-by` lists
//! exactly the evidence the judge counted.
//!
//! Envelope laws, inherited from the report's: pure (no wall-clock, no cache
//! state), deterministic (sorted, capped listings with EXPLICIT elision — a
//! model must never guess whether it saw everything), results aligned 1:1 with
//! the request's inputs, and one bad input never fails its siblings — it
//! becomes its own `not-found`/`error` outcome inline.
//!
//! Selectors are the [`Subject`] vocabulary — `path`, `path#name`,
//! `path#Owner.member` — so the finding address space and the query address
//! space are one space. A bare member name resolves when unique; ambiguity is
//! an error listing the qualified selectors to retry with.

use crate::analysis::Reachability;
use crate::graph::Graph;
pub use crate::navigate::ReachColor;
use crate::navigate::{self, Index, Keeper};
use crate::session::Snapshot;
use kndo_contract::evidence::DeclarationId;
use kndo_contract::finding::LineSpan;
use kndo_contract::vocab::{ProjectPath, Span};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::BTreeMap;

pub const QUERY_SCHEMA: &str = "kndo-query/1";

/// Listings are capped here when the request does not say; elision is explicit
/// either way.
pub const DEFAULT_LIMIT: u32 = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Request {
    pub verb: Verb,
    /// One input per answer: selectors for the node verbs, search patterns for
    /// `find`. Results align 1:1, in this order.
    pub inputs: Vec<String>,
    #[serde(default)]
    pub options: Options,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Verb {
    Find,
    Describe,
    Uses,
    UsedBy,
    Trace,
    Impact,
    Explain,
}

impl Verb {
    /// The one text spelling — indexed by discriminant, tied to serde's
    /// kebab-case output by a test so a reorder cannot drift silently.
    pub fn as_str(self) -> &'static str {
        [
            "find", "describe", "uses", "used-by", "trace", "impact", "explain",
        ][self as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::Verb;

    #[test]
    fn verb_spelling_is_the_serde_spelling() {
        for verb in [
            Verb::Find,
            Verb::Describe,
            Verb::Uses,
            Verb::UsedBy,
            Verb::Trace,
            Verb::Impact,
            Verb::Explain,
        ] {
            let json = serde_json::to_string(&verb).unwrap();
            assert_eq!(json, format!("\"{}\"", verb.as_str()));
        }
        for set in [
            super::RootSet::Production,
            super::RootSet::Test,
            super::RootSet::Tooling,
        ] {
            let json = serde_json::to_string(&set).unwrap();
            assert_eq!(json, format!("\"{}\"", set.as_str()));
        }
    }
}

/// One superset of options; a verb reads what concerns it and ignores the rest,
/// so one request shape works everywhere — but an unknown KEY is still a typo
/// and still refuses.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Options {
    /// Listing cap (default 50); every listing reports what it elided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// `find`: keep only this kind (a symbol kind, or `file`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// `find`: keep only nodes of this reachability color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ReachColor>,
    /// `trace`: which root set to trace from. Default: production, falling
    /// back to test, then tooling — the first set that reaches the node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootSet>,
    /// `impact`: additionally simulate the deletion and report the typed
    /// reachability flips — never fabricated findings.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub if_deleted: bool,
    /// `trace`: the directed form — the shortest path from each input TO this
    /// node, instead of from the root sets. One target for the whole request,
    /// so results stay 1:1 with inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum RootSet {
    Production,
    Test,
    Tooling,
}

impl RootSet {
    /// The one text spelling — indexed by discriminant, tied to serde's
    /// kebab-case output by a test so a reorder cannot drift silently.
    pub fn as_str(self) -> &'static str {
        ["production", "test", "tooling"][self as usize]
    }
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Response {
    /// Stamped from [`QUERY_SCHEMA`]; a consumer of any other envelope version
    /// fails loudly instead of drifting.
    pub schema: &'static str,
    pub verb: Verb,
    /// 1:1 with the request's `inputs`, in order.
    pub results: Vec<Outcome>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Outcome {
    Ok {
        #[serde(flatten)]
        answer: Answer,
    },
    NotFound {
        input: String,
    },
    Error {
        input: String,
        message: String,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Answer {
    Find(FindAnswer),
    Describe(Box<DescribeAnswer>),
    Uses(UsesAnswer),
    UsedBy(UsedByAnswer),
    Trace(TraceAnswer),
    Impact(Box<ImpactAnswer>),
    Explain(Box<ExplainAnswer>),
}

/// A node, addressable: feed `selector` straight back into any verb.
#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NodeRef {
    pub selector: String,
    /// `file`, or the declaration's symbol kind.
    pub kind: SmolStr,
    pub color: ReachColor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<LineSpan>,
}

/// One piece of evidence keeping a node alive — [`navigate::Keeper`], serialized.
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EdgeRef {
    pub kind: &'static str,
    /// The concrete source site, when the keeper has one (roots and entry
    /// surface are facts, not sites).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<SiteRef>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SiteRef {
    pub path: ProjectPath,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<LineSpan>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FindAnswer {
    pub matches: Vec<NodeRef>,
    pub elided: u32,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DescribeAnswer {
    pub node: NodeRef,
    /// Symbol subjects only: the declaration's own facts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declaration: Option<DeclarationFacts>,
    /// File subjects only: the file's own facts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<FileFacts>,
    /// A preview of what keeps it alive (symbols only) — `used-by` lists them
    /// all; `more: true` says the preview is not the whole story.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept_by: Option<KeptPreview>,
    /// Ids of current findings on this exact subject.
    pub findings: Vec<String>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DeclarationFacts {
    /// The reach as declared: `owner`, `file`, `namespace` (`+N` for an
    /// ancestor), `directory+N`, `unit`, `group`, `named:<a.b>`,
    /// `inherited`, `scoped:<token>`, or `exported`.
    pub reach: String,
    /// The reach after every owner above caps it — what the engine pools and
    /// judges by; equal to `reach` for a declaration nobody owns.
    pub effective_reach: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exported_as: Option<SmolStr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileFacts {
    /// The extension that claimed the file.
    pub extension: SmolStr,
    pub declarations: u32,
    pub imports: u32,
    /// Files importing this one (bindings and whole-surface alike).
    pub importers: u32,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct KeptPreview {
    pub entries: Vec<EdgeRef>,
    pub more: bool,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UsesAnswer {
    pub node: NodeRef,
    /// Resolved imports out of the node's file (whole file) or none for a
    /// symbol whose file needs no imports for it.
    pub imports: Vec<ImportUse>,
    /// Names the node references, each with its use count and — when exactly
    /// one declaration in the legal pool carries the name — its resolution.
    pub references: Vec<ReferenceUse>,
    pub elided: u32,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImportUse {
    pub target: ProjectPath,
    /// Bound names, empty for whole-surface shapes.
    pub names: Vec<SmolStr>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReferenceUse {
    pub name: SmolStr,
    pub count: u32,
    /// Where the name resolves, when the pool holds exactly one declaration
    /// with it — ambiguity stays honest by absence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<NodeRef>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UsedByAnswer {
    pub node: NodeRef,
    /// Everything keeping it, most direct first — the exact evidence the
    /// `unused` judgment counted.
    pub kept_by: Vec<EdgeRef>,
    /// The deletion question at a glance: keeper sites per color.
    pub by_color: BTreeMap<&'static str, u32>,
    pub elided: u32,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TraceAnswer {
    pub node: NodeRef,
    /// The shortest root-to-node path — `null` means NOT reachable from the
    /// requested root set, which is itself the answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<TracePath>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TracePath {
    /// Which root set anchors this path — absent on the directed form, whose
    /// origin is the input itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootSet>,
    /// The rooted file the path starts from.
    pub root: NodeRef,
    /// File hops, root-side first; each names the edge that led into it.
    pub hops: Vec<TraceHop>,
    /// For symbol targets: the in-file evidence that finally keeps it — the
    /// same keeper vocabulary `used-by` speaks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keeper: Option<EdgeRef>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TraceHop {
    pub node: NodeRef,
    /// `import` (with its recorded confidence) or `sees` (structural).
    pub via: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<kndo_contract::vocab::Confidence>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImpactAnswer {
    pub node: NodeRef,
    /// The reverse closure: everything that transitively depends on the
    /// node's file, nearest first.
    pub affected: Vec<Affected>,
    pub by_color: BTreeMap<&'static str, u32>,
    pub elided: u32,
    /// Root kinds whose reach passes through the affected set.
    pub affected_roots: Vec<RootSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_deleted: Option<IfDeleted>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Affected {
    pub node: NodeRef,
    pub depth: u32,
}

/// Typed reachability flips from simulating the removal — graph facts, never
/// fabricated finding objects: those findings don't exist until the edit does.
#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IfDeleted {
    /// Files no color reaches once the node is gone (file subjects).
    pub newly_unreachable: Vec<NodeRef>,
    pub newly_unreachable_elided: u32,
    /// Files production no longer reaches but tests still do.
    pub newly_test_only: Vec<NodeRef>,
    pub newly_test_only_elided: u32,
    /// Symbol subjects: declarations whose EVERY reference lives inside the
    /// deleted declaration's span — they lose their last reference with it.
    /// A precise subset, not a re-judgment.
    pub orphans: Vec<NodeRef>,
    pub orphans_elided: u32,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExplainAnswer {
    pub finding: FindingBrief,
    /// The subject, described in full — for `unused` the empty keeper preview
    /// IS the why; for `internal-only` the scoped reach and in-file keepers
    /// are; category-specific evidence deepens per analysis over time.
    pub subject: DescribeAnswer,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FindingBrief {
    pub id: String,
    pub category: String,
    pub severity: kndo_contract::finding::Severity,
    pub confidence: kndo_contract::vocab::Confidence,
    pub message: String,
    pub location: String,
}

// ------------------------------------------------------------------ selectors

#[derive(Clone, Copy)]
enum Selector {
    File(usize),
    Symbol { file: usize, decl: usize },
}

enum Resolve {
    Hit(Selector),
    Miss,
    Ambiguous(Vec<String>),
}

/// Whether `raw` — a path, or `path#` followed by a selector's exact render —
/// names one thing the graph holds. What a fixture's expectations use to
/// refuse a subject nothing declares: a pin is exact, so the leniency the
/// verbs extend to a bare `Owner.name` does not apply here — an expectation
/// spelled `Widget.size` when the tree declares `Widget.size(int)` names
/// nothing, and says so, instead of quietly matching whichever came first.
pub fn selector_exists(graph: &Graph, raw: &str) -> bool {
    let (path, symbol) = match raw.split_once('#') {
        None => (raw, None),
        Some((p, s)) => (p, Some(s)),
    };
    let Some(file) = graph.files.iter().position(|f| f.path.as_str() == path) else {
        return false;
    };
    let Some(symbol) = symbol else {
        return true;
    };
    let evidence = &graph.files[file].evidence;
    evidence
        .declarations_with_ids()
        .any(|(id, _)| evidence.selector_of(id).render() == symbol)
}

fn resolve(graph: &Graph, raw: &str) -> Resolve {
    let (path, symbol) = match raw.split_once('#') {
        None => (raw, None),
        Some((p, s)) => (p, Some(s)),
    };
    let Some(file) = graph.files.iter().position(|f| f.path.as_str() == path) else {
        return Resolve::Miss;
    };
    let Some(symbol) = symbol else {
        return Resolve::Hit(Selector::File(file));
    };
    match declaration_named(graph, file, symbol) {
        Ok(Some(id)) => Resolve::Hit(Selector::Symbol {
            file,
            decl: id.index(),
        }),
        Ok(None) => Resolve::Miss,
        Err(candidates) => Resolve::Ambiguous(
            candidates
                .into_iter()
                .map(|id| selector_of(graph, file, Some(id.index())))
                .collect(),
        ),
    }
}

/// The declaration a symbol spelling names in one file — the ONE resolver the
/// query verbs, a fixture's expectations and a plugin's targets share, so all
/// three accept the same spellings. A selector's exact render
/// (`Owner.name(int)`, `name#2`) hits directly; a bare `name` or `Owner.name`
/// is accepted when it names one thing, free declarations first. `Err` lists
/// the ids of everything it could mean, each with a distinct render to retry
/// with — never two identical suggestions.
pub(crate) fn declaration_named(
    graph: &Graph,
    file: usize,
    symbol: &str,
) -> Result<Option<DeclarationId>, Vec<DeclarationId>> {
    let evidence = &graph.files[file].evidence;
    if let Some((id, _)) = evidence
        .declarations_with_ids()
        .find(|(id, _)| evidence.selector_of(*id).render() == symbol)
    {
        return Ok(Some(id));
    }
    let (owner, name) = match symbol.split_once('.') {
        Some((o, n)) => (Some(o), n),
        None => (None, symbol),
    };
    let decls = &evidence.declarations;
    let mut hits: Vec<DeclarationId> = Vec::new();
    for (id, d) in evidence.declarations_with_ids() {
        if d.name != name {
            continue;
        }
        match (owner, d.owner) {
            (Some(o), Some(od)) if decls[od.index()].name == o => hits.push(id),
            (Some(_), _) => {}
            // A bare name matches free declarations first; members join only
            // when no free declaration carries the name.
            (None, None) => hits.push(id),
            (None, Some(_)) => {}
        }
    }
    if owner.is_none() && hits.is_empty() {
        hits.extend(
            evidence
                .declarations_with_ids()
                .filter(|(_, d)| d.name == name && d.owner.is_some())
                .map(|(id, _)| id),
        );
    }
    match hits.len() {
        0 => Ok(None),
        1 => Ok(Some(hits[0])),
        _ => Err(hits),
    }
}

/// The canonical spelling — what every NodeRef carries and every verb accepts:
/// the path, and for a declaration its selector's one render after `#`.
fn selector_of(graph: &Graph, file: usize, decl: Option<usize>) -> String {
    let f = &graph.files[file];
    match decl {
        None => f.path.as_str().to_string(),
        Some(ix) => {
            let (id, _) = f
                .evidence
                .declarations_with_ids()
                .nth(ix)
                .expect("a declaration index the caller took from this file");
            format!(
                "{}#{}",
                f.path.as_str(),
                f.evidence.selector_of(id).render()
            )
        }
    }
}

// ------------------------------------------------------------------ the door

/// Everything one query call needs: the same index the analyses ran on,
/// rebuilt from the snapshot's graph (a pure function of it) on the first
/// query and held for the snapshot's lifetime.
struct QueryContext<'a> {
    graph: &'a Graph,
    reach: &'a Reachability,
    index: &'a Index,
    lines: &'a BTreeMap<ProjectPath, Vec<u32>>,
    findings: &'a [kndo_contract::finding::Finding],
}

impl Snapshot {
    /// The one door: CLI verbs and serve tools alike build a [`Request`] and
    /// read a [`Response`].
    pub fn query(&self, request: &Request) -> Response {
        let (reach, index) = self.navigation.get_or_init(|| {
            let reach = Reachability::compute(&self.graph);
            let index = Index::build(&self.graph, &reach, &self.capabilities);
            (reach, index)
        });
        let cx = QueryContext {
            graph: &self.graph,
            reach,
            index,
            lines: self.line_index(),
            findings: &self.findings,
        };
        let limit = request.options.limit.unwrap_or(DEFAULT_LIMIT).max(1) as usize;
        let results = request
            .inputs
            .iter()
            .map(|input| match request.verb {
                Verb::Find => find(&cx, input, &request.options, limit),
                Verb::Describe => node_verb(&cx, input, describe),
                Verb::Uses => node_verb(&cx, input, |cx, sel| uses(cx, sel, limit)),
                Verb::UsedBy => node_verb(&cx, input, |cx, sel| used_by(cx, sel, limit)),
                Verb::Trace => match request.options.to.as_deref() {
                    None => node_verb(&cx, input, |cx, sel| trace(cx, sel, request.options.roots)),
                    Some(raw) => match resolve(cx.graph, raw) {
                        Resolve::Hit(target) => {
                            node_verb(&cx, input, |cx, sel| trace_to(cx, sel, target))
                        }
                        Resolve::Miss => Outcome::NotFound {
                            input: raw.to_string(),
                        },
                        Resolve::Ambiguous(candidates) => Outcome::Error {
                            input: raw.to_string(),
                            message: format!(
                                "ambiguous — retry with one of: {}",
                                candidates.join(", ")
                            ),
                        },
                    },
                },
                Verb::Impact => node_verb(&cx, input, |cx, sel| {
                    impact(cx, sel, request.options.if_deleted, limit)
                }),
                Verb::Explain => explain(&cx, input),
            })
            .collect();
        Response {
            schema: QUERY_SCHEMA,
            verb: request.verb,
            results,
        }
    }
}

fn node_verb(
    cx: &QueryContext<'_>,
    input: &str,
    answer: impl Fn(&QueryContext<'_>, Selector) -> Answer,
) -> Outcome {
    match resolve(cx.graph, input) {
        Resolve::Hit(selector) => Outcome::Ok {
            answer: answer(cx, selector),
        },
        Resolve::Miss => Outcome::NotFound {
            input: input.to_string(),
        },
        Resolve::Ambiguous(candidates) => Outcome::Error {
            input: input.to_string(),
            message: format!("ambiguous — retry with one of: {}", candidates.join(", ")),
        },
    }
}

fn line_span(
    lines: &BTreeMap<ProjectPath, Vec<u32>>,
    path: &ProjectPath,
    span: Span,
) -> Option<LineSpan> {
    let starts = lines.get(path)?;
    let line_of = |offset: u32| starts.partition_point(|s| *s <= offset) as u32;
    Some(LineSpan {
        start: line_of(span.start),
        end: line_of(span.end.saturating_sub(1).max(span.start)),
    })
}

fn node_ref(cx: &QueryContext<'_>, file: usize, decl: Option<usize>) -> NodeRef {
    let f = &cx.graph.files[file];
    let (kind, lines) = match decl {
        None => (SmolStr::new_static("file"), None),
        Some(ix) => {
            let d = &f.evidence.declarations[ix];
            (
                SmolStr::new(d.kind.as_str()),
                line_span(cx.lines, &f.path, d.span),
            )
        }
    };
    NodeRef {
        selector: selector_of(cx.graph, file, decl),
        kind,
        color: ReachColor::of(cx.reach, file),
        lines,
    }
}

fn edge_ref(cx: &QueryContext<'_>, keeper: &Keeper) -> EdgeRef {
    let (kind, site) = match keeper {
        Keeper::Reference { site } => ("reference", Some(*site)),
        Keeper::Binding { site } => ("binding", Some(*site)),
        Keeper::Root { kind } => (
            match kind {
                kndo_contract::evidence::RootKind::Production => "root:production",
                kndo_contract::evidence::RootKind::Test => "root:test",
                kndo_contract::evidence::RootKind::Tooling => "root:tooling",
            },
            None,
        ),
        Keeper::Dispatch { kind } => (
            match kind {
                kndo_contract::evidence::RootKind::Production => "dispatch:production",
                kndo_contract::evidence::RootKind::Test => "dispatch:test",
                kndo_contract::evidence::RootKind::Tooling => "dispatch:tooling",
            },
            None,
        ),
        Keeper::Exempt => ("exempt", None),
        Keeper::Witness { .. } => ("witness", None),
        Keeper::EntrySurface => ("entry-surface", None),
        Keeper::Published { .. } => ("published", None),
        Keeper::SurfaceImport { site } => ("surface-import", Some(*site)),
        Keeper::OwnerBinding { site } => ("owner-binding", Some(*site)),
    };
    EdgeRef {
        kind,
        site: site.map(|s| {
            let path = &cx.graph.files[s.file as usize].path;
            SiteRef {
                path: path.clone(),
                lines: line_span(cx.lines, path, s.span),
            }
        }),
    }
}

// ------------------------------------------------------------------ verbs

fn find(cx: &QueryContext<'_>, pattern: &str, options: &Options, limit: usize) -> Outcome {
    // Rank: exact > prefix > substring, then path and name — deterministic and
    // copy-paste friendly (agents quote identifiers verbatim, so matching is
    // case-sensitive).
    let mut ranked: Vec<(u8, String, NodeRef)> = Vec::new();
    let rank_of = |name: &str| -> Option<u8> {
        if name == pattern {
            Some(0)
        } else if name.starts_with(pattern) {
            Some(1)
        } else if name.contains(pattern) {
            Some(2)
        } else {
            None
        }
    };
    for (i, f) in cx.graph.files.iter().enumerate() {
        let basename = f.path.as_str().rsplit('/').next().unwrap_or("");
        if let Some(rank) = rank_of(basename)
            && kind_passes(options, "file")
        {
            let node = node_ref(cx, i, None);
            if color_passes(options, node.color) {
                ranked.push((rank, node.selector.clone(), node));
            }
        }
        for (ix, d) in f.evidence.declarations.iter().enumerate() {
            let qualified = match d.owner {
                Some(o) => format!("{}.{}", f.evidence.declarations[o.index()].name, d.name),
                None => d.name.to_string(),
            };
            if let Some(rank) = rank_of(&qualified).or_else(|| rank_of(d.name.as_str()))
                && kind_passes(options, d.kind.as_str())
            {
                let node = node_ref(cx, i, Some(ix));
                if color_passes(options, node.color) {
                    ranked.push((rank, node.selector.clone(), node));
                }
            }
        }
    }
    ranked.sort_by(|a, b| (a.0, a.1.as_str()).cmp(&(b.0, b.1.as_str())));
    if ranked.is_empty() {
        return Outcome::NotFound {
            input: pattern.to_string(),
        };
    }
    let elided = ranked.len().saturating_sub(limit) as u32;
    Outcome::Ok {
        answer: Answer::Find(FindAnswer {
            matches: ranked
                .into_iter()
                .take(limit)
                .map(|(_, _, node)| node)
                .collect(),
            elided,
        }),
    }
}

fn kind_passes(options: &Options, kind: &str) -> bool {
    options.kind.as_deref().is_none_or(|want| want == kind)
}

fn color_passes(options: &Options, color: ReachColor) -> bool {
    options.color.is_none_or(|want| want == color)
}

fn describe(cx: &QueryContext<'_>, selector: Selector) -> Answer {
    match selector {
        Selector::File(file) => {
            let f = &cx.graph.files[file];
            let importers = cx.index.surface_importers(file).len() as u32
                + cx.graph
                    .files
                    .iter()
                    .filter(|other| other.imports.binary_search(&(file as u32)).is_ok())
                    .count() as u32;
            Answer::Describe(Box::new(DescribeAnswer {
                node: node_ref(cx, file, None),
                declaration: None,
                file: Some(FileFacts {
                    extension: f.adapter.clone(),
                    declarations: f.evidence.declarations.len() as u32,
                    imports: f.evidence.imports.len() as u32,
                    importers,
                }),
                kept_by: None,
                findings: findings_on(cx, file, None),
            }))
        }
        Selector::Symbol { file, decl } => {
            let d = &cx.graph.files[file].evidence.declarations[decl];
            let preview = navigate::keepers(cx.graph, cx.index, file, decl, 4);
            let more = preview.len() > 3;
            Answer::Describe(Box::new(DescribeAnswer {
                node: node_ref(cx, file, Some(decl)),
                declaration: Some(DeclarationFacts {
                    reach: render_reach(&d.reach),
                    effective_reach: render_reach(&cx.index.effective(cx.graph, file, decl)),
                    exported_as: d.exported_as.clone(),
                    owner: d.owner.map(|o| {
                        cx.graph.files[file].evidence.declarations[o.index()]
                            .name
                            .to_string()
                    }),
                }),
                file: None,
                kept_by: Some(KeptPreview {
                    entries: preview.iter().take(3).map(|k| edge_ref(cx, k)).collect(),
                    more,
                }),
                findings: findings_on(cx, file, Some(decl)),
            }))
        }
    }
}

/// Ids of current findings on this exact subject — the snapshot's
/// post-suppression truth (the baseline hides nothing here), matched by the
/// subject's own address.
fn findings_on(cx: &QueryContext<'_>, file: usize, decl: Option<usize>) -> Vec<String> {
    use kndo_contract::subject::Subject;
    let f = &cx.graph.files[file];
    let wanted = decl.map(|ix| {
        let d = &f.evidence.declarations[ix];
        match d.owner {
            Some(o) => format!("{}.{}", f.evidence.declarations[o.index()].name, d.name),
            None => d.name.to_string(),
        }
    });
    cx.findings
        .iter()
        .filter(|finding| match (&finding.subject, &wanted) {
            (Subject::File { path }, None) => path == &f.path,
            (Subject::Symbol { path, selector, .. }, Some(name)) => {
                path == &f.path && &selector.render() == name
            }
            _ => false,
        })
        .map(|finding| finding.id.as_str().to_string())
        .collect()
}

fn uses(cx: &QueryContext<'_>, selector: Selector, limit: usize) -> Answer {
    let (file, span): (usize, Option<Span>) = match selector {
        Selector::File(f) => (f, None),
        Selector::Symbol { file, decl } => (
            file,
            Some(cx.graph.files[file].evidence.declarations[decl].span),
        ),
    };
    let f = &cx.graph.files[file];
    let node = match selector {
        Selector::File(_) => node_ref(cx, file, None),
        Selector::Symbol { decl, .. } => node_ref(cx, file, Some(decl)),
    };
    // Imports: the whole file's, resolved — for a symbol they are context (its
    // names may bind through them).
    let mut imports: Vec<ImportUse> = Vec::new();
    for (import, targets) in f.evidence.imports.iter().zip(&f.import_targets) {
        for &t in targets {
            use kndo_contract::evidence::ImportShape;
            let names = match &import.shape {
                ImportShape::Bindings(bs) | ImportShape::Reexport(bs) => {
                    bs.iter().map(|b| b.imported.clone()).collect()
                }
                _ => Vec::new(),
            };
            imports.push(ImportUse {
                target: cx.graph.files[t as usize].path.clone(),
                names,
            });
        }
    }
    imports.sort_by(|a, b| a.target.as_str().cmp(b.target.as_str()));
    imports.dedup_by(|a, b| a.target == b.target && a.names == b.names);

    // References: within the symbol's span, or the whole file's — counted per
    // name, resolved when the legal pool holds exactly one declaration.
    let mut counts: BTreeMap<&SmolStr, u32> = BTreeMap::new();
    for r in &f.evidence.references {
        let inside = span.is_none_or(|s| s.contains(&r.span));
        if inside {
            *counts.entry(&r.name).or_insert(0) += 1;
        }
    }
    let total = counts.len();
    let references: Vec<ReferenceUse> = counts
        .into_iter()
        .take(limit)
        .map(|(name, count)| ReferenceUse {
            resolved: resolve_name(cx, file, name),
            name: name.clone(),
            count,
        })
        .collect();
    Answer::Uses(UsesAnswer {
        node,
        imports,
        references,
        elided: total.saturating_sub(limit) as u32,
    })
}

/// The one declaration a name can mean from `file`'s point of view — its own
/// file, the files it sees, and its bound import targets; `None` when zero or
/// several candidates hold the name (ambiguity stays honest by absence).
fn resolve_name(cx: &QueryContext<'_>, file: usize, name: &SmolStr) -> Option<NodeRef> {
    let mut pool: Vec<u32> = vec![file as u32];
    pool.extend_from_slice(&cx.graph.files[file].sees);
    for (import, targets) in cx.graph.files[file]
        .evidence
        .imports
        .iter()
        .zip(&cx.graph.files[file].import_targets)
    {
        use kndo_contract::evidence::ImportShape;
        let binds = match &import.shape {
            ImportShape::Bindings(bs) | ImportShape::Reexport(bs) => {
                bs.iter().any(|b| &b.imported == name)
            }
            _ => true,
        };
        if binds {
            pool.extend_from_slice(targets);
        }
    }
    pool.sort_unstable();
    pool.dedup();
    let mut hit: Option<(usize, usize)> = None;
    for &candidate in &pool {
        for (ix, d) in cx.graph.files[candidate as usize]
            .evidence
            .declarations
            .iter()
            .enumerate()
        {
            if &d.name == name || d.exported_as.as_ref() == Some(name) {
                if hit.is_some() {
                    return None;
                }
                hit = Some((candidate as usize, ix));
            }
        }
    }
    hit.map(|(f, ix)| node_ref(cx, f, Some(ix)))
}

fn used_by(cx: &QueryContext<'_>, selector: Selector, limit: usize) -> Answer {
    match selector {
        Selector::Symbol { file, decl } => {
            let keepers = navigate::keepers(cx.graph, cx.index, file, decl, limit + 1);
            let elided = keepers.len().saturating_sub(limit) as u32;
            let mut by_color: BTreeMap<&'static str, u32> = BTreeMap::new();
            for k in keepers.iter().take(limit) {
                if let Some(site) = keeper_site(k) {
                    *by_color
                        .entry(ReachColor::of(cx.reach, site.file as usize).as_str())
                        .or_insert(0) += 1;
                }
            }
            Answer::UsedBy(UsedByAnswer {
                node: node_ref(cx, file, Some(decl)),
                kept_by: keepers
                    .iter()
                    .take(limit)
                    .map(|k| edge_ref(cx, k))
                    .collect(),
                by_color,
                elided,
            })
        }
        Selector::File(file) => {
            // A file is used by its importers — bindings and whole-surface
            // alike — and by roots anchoring it.
            let mut kept_by: Vec<EdgeRef> = Vec::new();
            for &site in cx.index.surface_importers(file) {
                kept_by.push(edge_ref(cx, &Keeper::SurfaceImport { site }));
            }
            for (i, other) in cx.graph.files.iter().enumerate() {
                if other.imports.binary_search(&(file as u32)).is_ok() {
                    for (import, targets) in
                        other.evidence.imports.iter().zip(&other.import_targets)
                    {
                        use kndo_contract::evidence::ImportShape;
                        if targets.contains(&(file as u32))
                            && matches!(
                                import.shape,
                                ImportShape::Bindings(_) | ImportShape::Reexport(_)
                            )
                        {
                            kept_by.push(edge_ref(
                                cx,
                                &Keeper::Binding {
                                    site: navigate::Site {
                                        file: i as u32,
                                        span: import.span,
                                    },
                                },
                            ));
                        }
                    }
                }
            }
            let whole_file = |r: &kndo_contract::evidence::Root| {
                matches!(r.target, kndo_contract::evidence::RootTarget::WholeFile)
            };
            let f = &cx.graph.files[file];
            for r in f.evidence.roots.iter().chain(&f.anchored) {
                if whole_file(r) {
                    kept_by.push(edge_ref(cx, &Keeper::Root { kind: r.kind }));
                }
            }
            for r in &f.dispatched {
                if whole_file(r) {
                    kept_by.push(edge_ref(cx, &Keeper::Dispatch { kind: r.kind }));
                }
            }
            let mut by_color: BTreeMap<&'static str, u32> = BTreeMap::new();
            for e in &kept_by {
                if let Some(site) = &e.site
                    && let Some(ix) = cx.graph.files.iter().position(|f| f.path == site.path)
                {
                    *by_color
                        .entry(ReachColor::of(cx.reach, ix).as_str())
                        .or_insert(0) += 1;
                }
            }
            let elided = kept_by.len().saturating_sub(limit) as u32;
            kept_by.truncate(limit);
            Answer::UsedBy(UsedByAnswer {
                node: node_ref(cx, file, None),
                kept_by,
                by_color,
                elided,
            })
        }
    }
}

fn keeper_site(keeper: &Keeper) -> Option<navigate::Site> {
    match keeper {
        Keeper::Reference { site }
        | Keeper::Binding { site }
        | Keeper::SurfaceImport { site }
        | Keeper::OwnerBinding { site } => Some(*site),
        Keeper::Root { .. }
        | Keeper::Dispatch { .. }
        | Keeper::Exempt
        | Keeper::Witness { .. }
        | Keeper::EntrySurface
        | Keeper::Published { .. } => None,
    }
}

// ------------------------------------------------------------------ Q3 verbs

/// Reverse file adjacency (imports + sees), sorted — built per call, pure.
fn reverse_edges(graph: &Graph) -> Vec<Vec<u32>> {
    let mut reverse: Vec<Vec<u32>> = vec![Vec::new(); graph.files.len()];
    for (i, f) in graph.files.iter().enumerate() {
        for &t in &f.imports {
            reverse[t as usize].push(i as u32);
        }
        for &t in &f.sees {
            reverse[t as usize].push(i as u32);
        }
    }
    for edges in &mut reverse {
        edges.sort_unstable();
        edges.dedup();
    }
    reverse
}

/// BFS shortest path over forward edges (imports + sees) from any file in
/// `from` to `to`; deterministic because adjacency is sorted and the frontier
/// is scanned in insertion order.
fn shortest_path(graph: &Graph, from: &[u32], to: u32) -> Option<Vec<u32>> {
    let n = graph.files.len();
    let mut prev: Vec<Option<u32>> = vec![None; n];
    let mut seen = vec![false; n];
    if from.contains(&to) {
        return Some(vec![to]);
    }
    let mut frontier: Vec<u32> = from.to_vec();
    for &f in from {
        seen[f as usize] = true;
    }
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for &at in &frontier {
            let f = &graph.files[at as usize];
            for &t in f.imports.iter().chain(&f.sees) {
                if !seen[t as usize] {
                    seen[t as usize] = true;
                    prev[t as usize] = Some(at);
                    if t == to {
                        let mut path = vec![to, at];
                        let mut cursor = at;
                        while let Some(p) = prev[cursor as usize] {
                            path.push(p);
                            cursor = p;
                        }
                        path.reverse();
                        return Some(path);
                    }
                    next.push(t);
                }
            }
        }
        frontier = next;
    }
    None
}

/// The edge kind and confidence between two adjacent files on a path: an
/// import (with its recorded confidence) wins over `sees` when both exist.
fn edge_between(
    graph: &Graph,
    from: u32,
    to: u32,
) -> (&'static str, Option<kndo_contract::vocab::Confidence>) {
    let f = &graph.files[from as usize];
    for (import, targets) in f.evidence.imports.iter().zip(&f.import_targets) {
        if targets.contains(&to) {
            return ("import", Some(import.confidence));
        }
    }
    ("sees", None)
}

fn rooted_files(graph: &Graph, kind: kndo_contract::evidence::RootKind) -> Vec<u32> {
    graph
        .files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.roots().any(|r| r.kind == kind))
        .map(|(i, _)| i as u32)
        .collect()
}

fn trace(cx: &QueryContext<'_>, selector: Selector, roots: Option<RootSet>) -> Answer {
    use kndo_contract::evidence::RootKind;
    let (file, decl) = match selector {
        Selector::File(f) => (f, None),
        Selector::Symbol { file, decl } => (file, Some(decl)),
    };
    let node = node_ref(cx, file, decl);
    let sets: Vec<RootSet> = match roots {
        Some(set) => vec![set],
        None => vec![RootSet::Production, RootSet::Test, RootSet::Tooling],
    };
    for set in sets {
        let kind = match set {
            RootSet::Production => RootKind::Production,
            RootSet::Test => RootKind::Test,
            RootSet::Tooling => RootKind::Tooling,
        };
        let from = rooted_files(cx.graph, kind);
        if from.is_empty() {
            continue;
        }
        if let Some(path) = shortest_path(cx.graph, &from, file as u32) {
            let root = node_ref(cx, path[0] as usize, None);
            let hops: Vec<TraceHop> = path
                .windows(2)
                .map(|pair| {
                    let (via, confidence) = edge_between(cx.graph, pair[0], pair[1]);
                    TraceHop {
                        node: node_ref(cx, pair[1] as usize, None),
                        via,
                        confidence,
                    }
                })
                .collect();
            let keeper = decl.and_then(|d| {
                navigate::keepers(cx.graph, cx.index, file, d, 1)
                    .first()
                    .map(|k| edge_ref(cx, k))
            });
            return Answer::Trace(TraceAnswer {
                node,
                path: Some(TracePath {
                    roots: Some(set),
                    root,
                    hops,
                    keeper,
                }),
            });
        }
    }
    Answer::Trace(TraceAnswer { node, path: None })
}

/// The directed form: the shortest path from the input's file to the target's
/// file over the same forward edges, no root set involved — the origin is the
/// input itself. For a symbol target the final hop is its in-file keeper, in
/// the same vocabulary the liveness form and `used-by` speak.
fn trace_to(cx: &QueryContext<'_>, from: Selector, target: Selector) -> Answer {
    let (from_file, from_decl) = match from {
        Selector::File(f) => (f, None),
        Selector::Symbol { file, decl } => (file, Some(decl)),
    };
    let (to_file, to_decl) = match target {
        Selector::File(f) => (f, None),
        Selector::Symbol { file, decl } => (file, Some(decl)),
    };
    let node = node_ref(cx, from_file, from_decl);
    match shortest_path(cx.graph, &[from_file as u32], to_file as u32) {
        Some(path) => {
            let root = node_ref(cx, path[0] as usize, None);
            let hops: Vec<TraceHop> = path
                .windows(2)
                .map(|pair| {
                    let (via, confidence) = edge_between(cx.graph, pair[0], pair[1]);
                    TraceHop {
                        node: node_ref(cx, pair[1] as usize, None),
                        via,
                        confidence,
                    }
                })
                .collect();
            let keeper = to_decl.and_then(|d| {
                navigate::keepers(cx.graph, cx.index, to_file, d, 1)
                    .first()
                    .map(|k| edge_ref(cx, k))
            });
            Answer::Trace(TraceAnswer {
                node,
                path: Some(TracePath {
                    roots: None,
                    root,
                    hops,
                    keeper,
                }),
            })
        }
        None => Answer::Trace(TraceAnswer { node, path: None }),
    }
}

fn impact(cx: &QueryContext<'_>, selector: Selector, if_deleted: bool, limit: usize) -> Answer {
    use kndo_contract::evidence::RootKind;
    let (file, decl) = match selector {
        Selector::File(f) => (f, None),
        Selector::Symbol { file, decl } => (file, Some(decl)),
    };
    let node = node_ref(cx, file, decl);
    let reverse = reverse_edges(cx.graph);
    let mut depth: Vec<Option<u32>> = vec![None; cx.graph.files.len()];
    let mut frontier = vec![file as u32];
    depth[file] = Some(0);
    let mut level = 0u32;
    let mut affected: Vec<Affected> = Vec::new();
    while !frontier.is_empty() {
        level += 1;
        let mut next = Vec::new();
        for &at in &frontier {
            for &dependent in &reverse[at as usize] {
                if depth[dependent as usize].is_none() {
                    depth[dependent as usize] = Some(level);
                    affected.push(Affected {
                        node: node_ref(cx, dependent as usize, None),
                        depth: level,
                    });
                    next.push(dependent);
                }
            }
        }
        frontier = next;
    }
    let mut by_color: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut affected_roots: Vec<RootSet> = Vec::new();
    for (i, d) in depth.iter().enumerate() {
        if d.is_none() || i == file {
            continue;
        }
        *by_color
            .entry(ReachColor::of(cx.reach, i).as_str())
            .or_insert(0) += 1;
        for r in cx.graph.files[i].roots() {
            let set = match r.kind {
                RootKind::Production => RootSet::Production,
                RootKind::Test => RootSet::Test,
                RootKind::Tooling => RootSet::Tooling,
            };
            if !affected_roots.contains(&set) {
                affected_roots.push(set);
            }
        }
    }
    let elided = affected.len().saturating_sub(limit) as u32;
    affected.truncate(limit);

    let if_deleted = if_deleted.then(|| simulate_deletion(cx, file, decl, limit));
    Answer::Impact(Box::new(ImpactAnswer {
        node,
        affected,
        by_color,
        elided,
        affected_roots,
        if_deleted,
    }))
}

/// File subjects: re-flood the graph with the file (its edges and roots)
/// masked, and report which files flip. Symbol subjects: declarations whose
/// EVERY reference site lives inside the deleted declaration's span.
fn simulate_deletion(
    cx: &QueryContext<'_>,
    file: usize,
    decl: Option<usize>,
    limit: usize,
) -> IfDeleted {
    use kndo_contract::evidence::RootKind;
    let mut newly_unreachable = Vec::new();
    let mut newly_test_only = Vec::new();
    let mut orphans = Vec::new();
    match decl {
        None => {
            let flood = |kind: RootKind| -> Vec<bool> {
                let n = cx.graph.files.len();
                let mut alive = vec![false; n];
                let mut frontier: Vec<u32> = Vec::new();
                for (i, f) in cx.graph.files.iter().enumerate() {
                    if i != file && f.roots().any(|r| r.kind == kind) {
                        alive[i] = true;
                        frontier.push(i as u32);
                    }
                }
                while let Some(at) = frontier.pop() {
                    if at as usize == file {
                        continue;
                    }
                    let f = &cx.graph.files[at as usize];
                    for &t in f.imports.iter().chain(&f.sees) {
                        if t as usize != file && !alive[t as usize] {
                            alive[t as usize] = true;
                            frontier.push(t);
                        }
                    }
                }
                alive
            };
            let production = flood(RootKind::Production);
            let test = flood(RootKind::Test);
            let tooling = flood(RootKind::Tooling);
            for i in 0..cx.graph.files.len() {
                if i == file || !cx.index.reachable(i as u32) {
                    continue;
                }
                let now = (production[i], test[i], tooling[i]);
                let before = ReachColor::of(cx.reach, i);
                match (before, now) {
                    (_, (false, false, false)) => newly_unreachable.push(node_ref(cx, i, None)),
                    (ReachColor::Production, (false, true, _)) => {
                        newly_test_only.push(node_ref(cx, i, None));
                    }
                    _ => {}
                }
            }
        }
        Some(d_ix) => {
            let span = cx.graph.files[file].evidence.declarations[d_ix].span;
            // Names whose EVERY reachable site sits inside the deleted span
            // lose their last reference; their unambiguous resolutions are
            // the orphans.
            let mut inside: Vec<&SmolStr> = Vec::new();
            for r in &cx.graph.files[file].evidence.references {
                if span.contains(&r.span) {
                    inside.push(&r.name);
                }
            }
            inside.sort();
            inside.dedup();
            for name in inside {
                let all = cx.index.reference_sites(name.as_str());
                let in_span_here =
                    |s: &navigate::Site| s.file as usize == file && span.contains(&s.span);
                if !all.is_empty()
                    && all.iter().all(in_span_here)
                    && let Some(target) = resolve_name(cx, file, name)
                {
                    orphans.push(target);
                }
            }
            orphans.sort_by(|a, b| a.selector.cmp(&b.selector));
            orphans.dedup_by(|a, b| a.selector == b.selector);
        }
    }
    let cap = |list: &mut Vec<NodeRef>| -> u32 {
        let elided = list.len().saturating_sub(limit) as u32;
        list.truncate(limit);
        elided
    };
    let newly_unreachable_elided = cap(&mut newly_unreachable);
    let newly_test_only_elided = cap(&mut newly_test_only);
    let orphans_elided = cap(&mut orphans);
    IfDeleted {
        newly_unreachable,
        newly_unreachable_elided,
        newly_test_only,
        newly_test_only_elided,
        orphans,
        orphans_elided,
    }
}

fn explain(cx: &QueryContext<'_>, input: &str) -> Outcome {
    use kndo_contract::subject::Subject;
    let Some(finding) = cx.findings.iter().find(|f| f.id.as_str() == input) else {
        return Outcome::NotFound {
            input: input.to_string(),
        };
    };
    let subject = match &finding.subject {
        Subject::File { path } => match cx.graph.files.iter().position(|f| &f.path == path) {
            Some(file) => describe(cx, Selector::File(file)),
            None => {
                return Outcome::Error {
                    input: input.to_string(),
                    message: "the finding's file is not in the current graph".to_string(),
                };
            }
        },
        Subject::Symbol { path, selector, .. } => {
            let raw = format!("{}#{}", path.as_str(), selector.render());
            match resolve(cx.graph, &raw) {
                Resolve::Hit(sel) => describe(cx, sel),
                _ => {
                    return Outcome::Error {
                        input: input.to_string(),
                        message: format!(
                            "the finding's subject `{raw}` is not in the current graph"
                        ),
                    };
                }
            }
        }
        _ => {
            return Outcome::Error {
                input: input.to_string(),
                message: "explain covers file and symbol subjects today".to_string(),
            };
        }
    };
    let Answer::Describe(subject) = subject else {
        unreachable!("describe answers describe");
    };
    Outcome::Ok {
        answer: Answer::Explain(Box::new(ExplainAnswer {
            finding: FindingBrief {
                id: finding.id.as_str().to_string(),
                category: finding.category.as_str().to_string(),
                severity: finding.severity,
                confidence: finding.confidence,
                message: finding.message.clone(),
                location: finding.location(),
            },
            subject: *subject,
        })),
    }
}

/// The generated schema for the query contract — both sides, one file each,
/// committed and gate-checked like the report's.
#[cfg(feature = "schema")]
pub fn request_schema() -> String {
    let schema = schemars::schema_for!(Request);
    let mut json = serde_json::to_string_pretty(&schema).expect("schema serializes");
    json.push('\n');
    json
}

#[cfg(feature = "schema")]
pub fn response_schema() -> String {
    let schema = schemars::schema_for!(Response);
    let mut json = serde_json::to_string_pretty(&schema).expect("schema serializes");
    json.push('\n');
    json
}

/// [`Options`] alone, as a JSON value — for a caller (serve's tool listing)
/// that names the verb out of band and embeds the options shape verbatim, so
/// its advertised schema is derived from the same type the door parses.
#[cfg(feature = "schema")]
pub fn options_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Options)).expect("schema serializes")
}

/// A reach as `describe` spells it. A reach this build does not know reads as
/// the widest one: `describe` never claims a narrowness it cannot name.
fn render_reach(reach: &kndo_contract::evidence::Reach) -> String {
    use kndo_contract::evidence::Reach;
    match reach {
        Reach::Owner => "owner".to_string(),
        Reach::File => "file".to_string(),
        Reach::Namespace { up: 0 } => "namespace".to_string(),
        Reach::Namespace { up } => format!("namespace+{up}"),
        Reach::Unit { up: 0 } => "unit".to_string(),
        Reach::Unit { up: 1 } => "group".to_string(),
        Reach::Unit { up } => format!("unit+{up}"),
        Reach::Directory { up } => format!("directory+{up}"),
        Reach::Heirs {
            and_namespace: false,
        } => "subtypes".to_string(),
        Reach::Heirs {
            and_namespace: true,
        } => "subtypes+namespace".to_string(),
        Reach::Named { namespace } => format!("named:{}", namespace.join(".")),
        Reach::Inherited => "inherited".to_string(),
        Reach::Exported => "exported".to_string(),
        _ => "exported".to_string(),
    }
}
