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
}

impl Verb {
    pub fn as_str(self) -> &'static str {
        match self {
            Verb::Find => "find",
            Verb::Describe => "describe",
            Verb::Uses => "uses",
            Verb::UsedBy => "used-by",
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
    /// The reach as the contract spells it: `private`, `scoped:<token>`, or
    /// `exported`.
    pub reach: String,
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

// ------------------------------------------------------------------ selectors

enum Selector {
    File(usize),
    Symbol { file: usize, decl: usize },
}

enum Resolve {
    Hit(Selector),
    Miss,
    Ambiguous(Vec<String>),
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
    let decls = &graph.files[file].evidence.declarations;
    let (owner, name) = match symbol.split_once('.') {
        Some((o, n)) => (Some(o), n),
        None => (None, symbol),
    };
    let mut hits: Vec<usize> = Vec::new();
    for (ix, d) in decls.iter().enumerate() {
        if d.name != name {
            continue;
        }
        match (owner, d.owner) {
            (Some(o), Some(od)) if decls[od.index()].name == o => hits.push(ix),
            (Some(_), _) => {}
            // A bare name matches free declarations first; members join only
            // when no free declaration carries the name.
            (None, None) => hits.push(ix),
            (None, Some(_)) => {}
        }
    }
    if owner.is_none() && hits.is_empty() {
        for (ix, d) in decls.iter().enumerate() {
            if d.name == name && d.owner.is_some() {
                hits.push(ix);
            }
        }
    }
    match hits.len() {
        0 => Resolve::Miss,
        1 => Resolve::Hit(Selector::Symbol {
            file,
            decl: hits[0],
        }),
        _ => Resolve::Ambiguous(
            hits.iter()
                .map(|&ix| selector_of(graph, file, Some(ix)))
                .collect(),
        ),
    }
}

/// The canonical spelling — what every NodeRef carries and every verb accepts.
fn selector_of(graph: &Graph, file: usize, decl: Option<usize>) -> String {
    let f = &graph.files[file];
    match decl {
        None => f.path.as_str().to_string(),
        Some(ix) => {
            let d = &f.evidence.declarations[ix];
            match d.owner {
                Some(o) => format!(
                    "{}#{}.{}",
                    f.path.as_str(),
                    f.evidence.declarations[o.index()].name,
                    d.name
                ),
                None => format!("{}#{}", f.path.as_str(), d.name),
            }
        }
    }
}

// ------------------------------------------------------------------ the door

/// Everything one query call needs, built once per call: the same index the
/// analyses ran on, rebuilt from the snapshot's graph (a pure function of it).
struct QueryContext<'a> {
    graph: &'a Graph,
    reach: Reachability,
    index: Index,
    lines: &'a BTreeMap<ProjectPath, Vec<u32>>,
    findings: &'a [kndo_contract::finding::Finding],
}

impl Snapshot {
    /// The one door: CLI verbs and serve tools alike build a [`Request`] and
    /// read a [`Response`].
    pub fn query(&self, request: &Request) -> Response {
        let reach = Reachability::compute(&self.graph);
        let index = Index::build(&self.graph, &reach);
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
        color: ReachColor::of(&cx.reach, file),
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
        Keeper::EntrySurface => ("entry-surface", None),
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
            let preview = navigate::keepers(cx.graph, &cx.index, file, decl, 4);
            let more = preview.len() > 3;
            Answer::Describe(Box::new(DescribeAnswer {
                node: node_ref(cx, file, Some(decl)),
                declaration: Some(DeclarationFacts {
                    reach: match &d.reach {
                        kndo_contract::evidence::Reach::Private => "private".to_string(),
                        kndo_contract::evidence::Reach::Scoped { scope } => {
                            format!("scoped:{scope}")
                        }
                        kndo_contract::evidence::Reach::Exported => "exported".to_string(),
                    },
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
                ImportShape::Bindings(bs)
                | ImportShape::Reexport(bs)
                | ImportShape::TypeOnly(bs) => bs.iter().map(|b| b.imported.clone()).collect(),
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
            ImportShape::Bindings(bs) | ImportShape::Reexport(bs) | ImportShape::TypeOnly(bs) => {
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
            let keepers = navigate::keepers(cx.graph, &cx.index, file, decl, limit + 1);
            let elided = keepers.len().saturating_sub(limit) as u32;
            let mut by_color: BTreeMap<&'static str, u32> = BTreeMap::new();
            for k in keepers.iter().take(limit) {
                if let Some(site) = keeper_site(k) {
                    *by_color
                        .entry(ReachColor::of(&cx.reach, site.file as usize).as_str())
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
                                ImportShape::Bindings(_)
                                    | ImportShape::Reexport(_)
                                    | ImportShape::TypeOnly(_)
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
            for r in cx.graph.files[file]
                .evidence
                .roots
                .iter()
                .chain(&cx.graph.files[file].anchored)
            {
                if matches!(r.target, kndo_contract::evidence::RootTarget::WholeFile) {
                    kept_by.push(edge_ref(cx, &Keeper::Root { kind: r.kind }));
                }
            }
            let mut by_color: BTreeMap<&'static str, u32> = BTreeMap::new();
            for e in &kept_by {
                if let Some(site) = &e.site
                    && let Some(ix) = cx.graph.files.iter().position(|f| f.path == site.path)
                {
                    *by_color
                        .entry(ReachColor::of(&cx.reach, ix).as_str())
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
        Keeper::Root { .. } | Keeper::EntrySurface => None,
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
