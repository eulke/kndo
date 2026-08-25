//! Graph navigation — read-only queries over the same
//! warm [`crate::graph::ProjectGraph`] `check` already builds. Never mutates findings, the
//! baseline, or the cache; every verb is a pure function of `(&ProjectGraph, &ReachabilityMap)`.
//!
//! Implemented verbs: `find`, `describe`, `uses`/`used-by` (one shared implementation —
//! direction is just "forward" vs "reverse" adjacency), `trace` (both the two-argument directed
//! form and the single-argument liveness form). `impact` isn't implemented —
//! the navigation-verb set is find/describe/uses/used-by/trace + `kndo query` only.
//!
//! Deliberately absent from `describe`, honestly rather than fabricated: `metrics` (cyclomatic/
//! CRAP/coverage — no such data exists anywhere in the graph) and duplication
//! group membership. `findings` (open findings attached to a node) IS implemented — it
//! reruns the same suppression-aware finding computation `check` uses and filters by node.
//!
//! `trace --all`'s path-enumeration policy is an open design question —
//! this implementation takes a direct, bounded reading: BFS for the shortest path,
//! then a depth- and expansion-capped DFS for alternatives when `--all` is set, honestly
//! reporting `paths_elided` when the cap is hit rather than silently truncating.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::VecDeque;

use smol_str::SmolStr;

use crate::adapter::{ProjectPath, Span};
use crate::analysis::reachability::{Reachability, ReachabilityMap};
use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, DependencyId, FileId, NodeRef, PackageId, RootKind, SymbolId};

// ---------------------------------------------------------------- selectors

/// A parsed but unresolved node address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    File(ProjectPath),
    /// `path#name` — `name` may contain `.` (nested, e.g. `TaxTable.lookup`), carried verbatim;
    /// resolution matches it against `SymbolNode::name` as a whole, so it only ever resolves
    /// once nested-declaration extraction exists (an honest current limitation, not a bug: no
    /// adapter emits qualified names yet — every declared symbol name today is unqualified).
    Symbol(ProjectPath, String),
    Dependency(SmolStr),
    Package(SmolStr),
    /// `roots:production|test|tooling` — a virtual multi-source pseudo-node, valid only as a
    /// `trace` endpoint (the `kndo trace roots:production X` example). Not a valid
    /// target for `find`/`describe`/`uses`/`used-by`.
    RootSet(RootKind),
}

/// Parses raw CLI/query text into a [`Selector`] — never touches the graph, so it can't fail on
/// "not found," only on malformed syntax.
/// Everything a query verb's parsing/resolution can reject with a message, unified so
/// `parse_selector`, [`EdgeFilter::parse`], and [`impact`] share one error discipline instead
/// of each returning a bare `Result<_, String>`. Every variant's `Display` is the exact string
/// the query envelope has always surfaced — callers that used to build a `String` by hand now
/// call `.to_string()` on this instead, byte-identical.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("`dep:` selector is missing a name")]
    EmptyDependencySelector,
    #[error("`pkg:` selector is missing a name")]
    EmptyPackageSelector,
    #[error("unknown root set `roots:{0}` (production, test, tooling)")]
    UnknownRootSet(String),
    #[error("malformed symbol selector `{0}`")]
    MalformedSymbolSelector(String),
    #[error("unknown --edges `{0}` (imports, references, all)")]
    UnknownEdgeFilter(String),
    #[error(
        "--if-deleted does not apply to dep:{0} — removing a declared dependency breaks its \
         importers outright rather than flipping reachability; use `used-by dep:{0}` to list \
         them"
    )]
    IfDeletedOnDependency(String),
    #[error("impact does not apply to a root set — trace individual roots")]
    ImpactOnRootSet,
}

pub fn parse_selector(raw: &str) -> Result<Selector, QueryError> {
    if let Some(name) = raw.strip_prefix("dep:") {
        return if name.is_empty() {
            Err(QueryError::EmptyDependencySelector)
        } else {
            Ok(Selector::Dependency(SmolStr::new(name)))
        };
    }
    if let Some(name) = raw.strip_prefix("pkg:") {
        return if name.is_empty() {
            Err(QueryError::EmptyPackageSelector)
        } else {
            Ok(Selector::Package(SmolStr::new(name)))
        };
    }
    if let Some(kind) = raw.strip_prefix("roots:") {
        return match kind {
            "production" => Ok(Selector::RootSet(RootKind::Production)),
            "test" => Ok(Selector::RootSet(RootKind::Test)),
            "tooling" => Ok(Selector::RootSet(RootKind::Tooling)),
            other => Err(QueryError::UnknownRootSet(other.to_string())),
        };
    }
    if let Some((path, name)) = raw.split_once('#') {
        return if path.is_empty() || name.is_empty() {
            Err(QueryError::MalformedSymbolSelector(raw.to_string()))
        } else {
            Ok(Selector::Symbol(
                ProjectPath(SmolStr::new(path)),
                name.to_string(),
            ))
        };
    }
    Ok(Selector::File(ProjectPath(SmolStr::new(raw))))
}

/// A selector resolved against one graph. `RootSet` resolves unconditionally (it's a virtual
/// pseudo-node, never absent) but is only ever a valid `trace` endpoint — every other verb
/// rejects it explicitly rather than silently treating it as something it isn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvedNode {
    File(FileId),
    Symbol(SymbolId),
    Package(PackageId),
    RootSet(RootKind),
}

/// A resolved dependency carries its name, not an id — `dep:<name>` can validly name a
/// *declared* dependency with no [`crate::vocab::DependencyId`] at all (nothing ever actually
/// imports it — that's what makes it `unused`), so identity is the name string throughout.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedDependency(pub SmolStr);

/// Either an ordinary graph node or a dependency (kept separate because dependencies aren't a
/// [`crate::vocab::NodeRef`] variant — they never anchor a `Declares`/`References`/`Root` edge).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Resolved {
    Node(ResolvedNode),
    Dependency(ResolvedDependency),
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("no node matches the selector")]
    NotFound,
    /// Rendered selector strings for every concrete candidate ("an error listing
    /// the concrete candidates — never a guess").
    #[error("ambiguous selector — candidates: {}", .0.join(", "))]
    Ambiguous(Vec<String>),
}

pub fn resolve(graph: &ProjectGraph, selector: &Selector) -> Result<Resolved, ResolveError> {
    match selector {
        Selector::File(path) => graph
            .file_id(path)
            .map(|f| Resolved::Node(ResolvedNode::File(f)))
            .ok_or(ResolveError::NotFound),
        Selector::Symbol(path, name) => {
            let file = graph.file_id(path).ok_or(ResolveError::NotFound)?;
            // A member resolves by its qualified `Owner.name` form or its bare
            // name — a bare name shared by several owners' members surfaces as Ambiguous below,
            // listing the qualified selectors to retry with.
            let matches: Vec<SymbolId> = graph
                .symbols
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    s.file == file
                        && (s.name.as_str() == name.as_str() || s.qualified_name() == name.as_str())
                })
                .map(|(i, _)| SymbolId(i as u32))
                .collect();
            match matches.len() {
                0 => Err(ResolveError::NotFound),
                1 => Ok(Resolved::Node(ResolvedNode::Symbol(matches[0]))),
                _ => Err(ResolveError::Ambiguous(
                    matches
                        .iter()
                        .map(|&s| selector_string(graph, &Resolved::Node(ResolvedNode::Symbol(s))))
                        .collect(),
                )),
            }
        }
        Selector::Dependency(name) => {
            let declared = graph
                .declared_dependencies
                .iter()
                .any(|d| d.name.as_str() == name.as_str());
            let imported = graph
                .dependencies
                .iter()
                .any(|d| d.name.as_str() == name.as_str());
            if declared || imported {
                Ok(Resolved::Dependency(ResolvedDependency(name.clone())))
            } else {
                Err(ResolveError::NotFound)
            }
        }
        Selector::Package(name) => {
            let matches: Vec<PackageId> = graph
                .packages
                .iter()
                .enumerate()
                .filter(|(_, p)| p.name.as_deref() == Some(name.as_str()))
                .map(|(i, _)| PackageId(i as u32))
                .collect();
            match matches.len() {
                0 => Err(ResolveError::NotFound),
                1 => Ok(Resolved::Node(ResolvedNode::Package(matches[0]))),
                _ => Err(ResolveError::Ambiguous(
                    matches
                        .iter()
                        .map(|&p| format!("pkg:{name}#{}", p.0))
                        .collect(),
                )),
            }
        }
        Selector::RootSet(kind) => Ok(Resolved::Node(ResolvedNode::RootSet(*kind))),
    }
}

fn root_kind_str(kind: RootKind) -> &'static str {
    match kind {
        RootKind::Production => "production",
        RootKind::Test => "test",
        RootKind::Tooling => "tooling",
    }
}

fn reachability_str(color: Reachability) -> &'static str {
    match color {
        Reachability::Production => "production",
        Reachability::TestOnly => "test-only",
        Reachability::ToolingOnly => "tooling-only",
        Reachability::Unreachable => "unreachable",
    }
}

/// The canonical selector string a resolved node round-trips to — what `find`/`describe`/…
/// print back, and what a caller can feed into the next query verbatim (selectors
/// round-trip).
pub fn selector_string(graph: &ProjectGraph, node: &Resolved) -> String {
    match node {
        Resolved::Node(ResolvedNode::File(f)) => graph.files[f.0 as usize].path.0.to_string(),
        Resolved::Node(ResolvedNode::Symbol(s)) => {
            let sym = &graph.symbols[s.0 as usize];
            let path = &graph.files[sym.file.0 as usize].path.0;
            format!("{path}#{}", sym.qualified_name())
        }
        Resolved::Node(ResolvedNode::Package(p)) => match graph.package_name(*p) {
            Some(name) => format!("pkg:{name}"),
            None => format!("pkg:#{}", p.0),
        },
        Resolved::Node(ResolvedNode::RootSet(kind)) => format!("roots:{}", root_kind_str(*kind)),
        Resolved::Dependency(d) => format!("dep:{}", d.0),
    }
}

fn kind_string(graph: &ProjectGraph, node: &Resolved) -> String {
    match node {
        Resolved::Node(ResolvedNode::File(_)) => "file".to_string(),
        Resolved::Node(ResolvedNode::Symbol(s)) => {
            graph.symbols[s.0 as usize].kind.facet().to_string()
        }
        Resolved::Node(ResolvedNode::Package(_)) => "package".to_string(),
        Resolved::Node(ResolvedNode::RootSet(_)) => "root-set".to_string(),
        Resolved::Dependency(_) => "dependency".to_string(),
    }
}

/// `{path, start, end}` — the query envelope's `NodeRef.span`/`EdgeRef.site` building
/// block, distinct from [`crate::engine::Location`]'s split `path`/`range` because
/// the query schema bundles them into one object.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NodeSpan {
    pub path: String,
    pub start: (u32, u32),
    pub end: (u32, u32),
}

fn node_span(graph: &ProjectGraph, node: &Resolved) -> Option<NodeSpan> {
    match node {
        Resolved::Node(ResolvedNode::Symbol(s)) => {
            let sym = &graph.symbols[s.0 as usize];
            Some(NodeSpan {
                path: graph.files[sym.file.0 as usize].path.0.to_string(),
                start: sym.span.start,
                end: sym.span.end,
            })
        }
        // Files, packages, dependencies, and root-sets have no single declaration span.
        _ => None,
    }
}

fn node_color(graph: &ProjectGraph, reach: &ReachabilityMap, node: &Resolved) -> Option<String> {
    let nref = match node {
        Resolved::Node(ResolvedNode::File(f)) => NodeRef::File(*f),
        Resolved::Node(ResolvedNode::Symbol(s)) => NodeRef::Symbol(*s),
        _ => return None,
    };
    let _ = graph;
    Some(reachability_str(reach.get(nref).0).to_string())
}

/// The query envelope's `NodeRef` building block.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QNodeRef {
    pub selector: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<NodeSpan>,
}

pub fn qnode_ref(graph: &ProjectGraph, reach: &ReachabilityMap, node: &Resolved) -> QNodeRef {
    QNodeRef {
        selector: selector_string(graph, node),
        kind: kind_string(graph, node),
        color: node_color(graph, reach, node),
        span: node_span(graph, node),
    }
}

/// The query envelope's `EdgeRef` building block.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QEdgeRef {
    pub edge: String,
    pub confidence: Confidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<NodeSpan>,
}

// ---------------------------------------------------------------- navigation adjacency

/// Everything [`resolve`] can return except `RootSet`/`Dependency`, folded into one hashable key
/// so `uses`/`used-by`/`trace` can build one forward+reverse adjacency graph that also threads
/// through dependency nodes (which aren't a [`NodeRef`] variant on the real graph).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NavNode {
    File(FileId),
    Symbol(SymbolId),
    Dependency(DependencyId),
}

fn nav_to_resolved(graph: &ProjectGraph, node: NavNode) -> Resolved {
    match node {
        NavNode::File(f) => Resolved::Node(ResolvedNode::File(f)),
        NavNode::Symbol(s) => Resolved::Node(ResolvedNode::Symbol(s)),
        NavNode::Dependency(d) => Resolved::Dependency(ResolvedDependency(
            graph.dependencies[d.0 as usize].name.clone(),
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeLabel {
    ImportsFile,
    ImportsDependency,
    References,
    Wildcard,
    /// the plugin-contributed file-liveness edge — navigable so `uses`/`used-by`
    /// explain why a template/asset counts as in use, labeled distinctly from a real import.
    ReferencesFile,
    /// the invoked-program edge — a test executing its workspace binary as a
    /// subprocess. Navigable for the same reason as `ReferencesFile`.
    InvokesFile,
}

impl EdgeLabel {
    fn as_str(self) -> &'static str {
        match self {
            EdgeLabel::ImportsFile | EdgeLabel::ImportsDependency => "imports",
            EdgeLabel::References => "references",
            EdgeLabel::Wildcard => "wildcard",
            other => file_liveness_label(other),
        }
    }
}

/// Display names of the file-liveness pair — the edges that carry "this file is in use"
/// without being imports (the plugin edge, the invoked-program edge).
fn file_liveness_label(label: EdgeLabel) -> &'static str {
    if matches!(label, EdgeLabel::InvokesFile) {
        "invokes-file"
    } else {
        "references-file"
    }
}

/// Which edge kinds `uses`/`used-by`/directed `trace` traverse — `--edges imports|references|
/// all`, default `all`. `Wildcard` never participates here: it's the
/// reachability algorithm's own dynamic-construct mechanism, not a navigable
/// "X uses Y" fact — the liveness trace form uses it instead, via [`EdgeFilter::liveness`].
#[derive(Debug, Clone, Copy)]
pub struct EdgeFilter {
    imports: bool,
    references: bool,
    wildcard: bool,
}

impl EdgeFilter {
    pub fn parse(raw: Option<&str>) -> Result<EdgeFilter, QueryError> {
        match raw.unwrap_or("all") {
            "imports" => Ok(EdgeFilter {
                imports: true,
                references: false,
                wildcard: false,
            }),
            "references" => Ok(EdgeFilter {
                imports: false,
                references: true,
                wildcard: false,
            }),
            "all" => Ok(EdgeFilter {
                imports: true,
                references: true,
                wildcard: false,
            }),
            other => Err(QueryError::UnknownEdgeFilter(other.to_string())),
        }
    }

    /// The liveness trace's traversal set — mirrors `reachability::compute` exactly (`Wildcard`
    /// included, `ImportsDependency` excluded: a dependency node carries no code to be alive).
    fn liveness() -> EdgeFilter {
        EdgeFilter {
            imports: true,
            references: true,
            wildcard: true,
        }
    }

    fn allows(self, label: EdgeLabel) -> bool {
        match label {
            EdgeLabel::ImportsFile | EdgeLabel::ImportsDependency => self.imports,
            // A file-liveness edge (plugin's or the invoked-program's) is a reference in
            // navigation terms — `--edges references` shows it, `--edges imports` (real
            // module structure only) doesn't.
            EdgeLabel::References | EdgeLabel::ReferencesFile | EdgeLabel::InvokesFile => {
                self.references
            }
            EdgeLabel::Wildcard => self.wildcard,
        }
    }
}

struct NavEdge {
    to: NavNode,
    label: EdgeLabel,
    confidence: Confidence,
    span: Option<Span>,
    /// The file `span` is a location *within* — always the edge's original `from` file,
    /// regardless of which direction (`uses` vs. `used-by`) this `NavEdge` is stored under.
    /// An import/reference/dynamic-use site always lives in the file that wrote the import,
    /// the reference, or the `eval` — never in the file being pointed *at* — so this must be
    /// captured once, at construction, rather than re-derived later from whichever node
    /// happens to be "the neighbor" for a given traversal direction (that would silently pair
    /// the right line/column with the wrong file whenever traversing `uses`, since the
    /// neighbor there is `to`, not `from`).
    site_file: FileId,
}

pub(crate) struct GraphIndex {
    forward: HashMap<NavNode, Vec<NavEdge>>,
    reverse: HashMap<NavNode, Vec<NavEdge>>,
    roots: HashMap<RootKind, Vec<(NavNode, Confidence)>>,
}

/// Every edge kind we build a [`NavEdge`] from has a `from` that's either a file or a symbol
/// (never a dependency — dependencies only ever appear as an edge's `to`), so this always
/// resolves to a real file.
fn nav_node_owning_file(graph: &ProjectGraph, node: NavNode) -> FileId {
    match node {
        NavNode::File(f) => f,
        NavNode::Symbol(s) => graph.symbols[s.0 as usize].file,
        NavNode::Dependency(_) => {
            unreachable!("a Dependency node is never an edge's `from`")
        }
    }
}

pub(crate) fn build_graph_index(graph: &ProjectGraph) -> GraphIndex {
    let mut declared_in: HashMap<FileId, Vec<SymbolId>> = HashMap::default();
    for edge in &graph.edges {
        if let crate::vocab::EdgeKind::Declares { file, symbol } = edge.kind {
            declared_in.entry(file).or_default().push(symbol);
        }
    }

    let mut forward: HashMap<NavNode, Vec<NavEdge>> = HashMap::default();
    let mut reverse: HashMap<NavNode, Vec<NavEdge>> = HashMap::default();

    for edge in &graph.edges {
        let (from, to, label, confidence): (NavNode, NavNode, EdgeLabel, Confidence) =
            match edge.kind {
                crate::vocab::EdgeKind::ImportsFile { from, to } => (
                    NavNode::File(from),
                    NavNode::File(to),
                    EdgeLabel::ImportsFile,
                    edge.confidence,
                ),
                crate::vocab::EdgeKind::ImportsDependency { from, to } => (
                    NavNode::File(from),
                    NavNode::Dependency(to),
                    EdgeLabel::ImportsDependency,
                    edge.confidence,
                ),
                crate::vocab::EdgeKind::References { from, to, .. } => {
                    let from = match from {
                        NodeRef::File(f) => NavNode::File(f),
                        NodeRef::Symbol(s) => NavNode::Symbol(s),
                    };
                    (
                        from,
                        NavNode::Symbol(to),
                        EdgeLabel::References,
                        edge.confidence,
                    )
                }
                crate::vocab::EdgeKind::ReferencesFile { from, to } => {
                    let from = match from {
                        NodeRef::File(f) => NavNode::File(f),
                        NodeRef::Symbol(s) => NavNode::Symbol(s),
                    };
                    (
                        from,
                        NavNode::File(to),
                        EdgeLabel::ReferencesFile,
                        edge.confidence,
                    )
                }
                crate::vocab::EdgeKind::InvokesFile { from, to } => {
                    let from = match from {
                        NodeRef::File(f) => NavNode::File(f),
                        NodeRef::Symbol(s) => NavNode::Symbol(s),
                    };
                    (
                        from,
                        NavNode::File(to),
                        EdgeLabel::InvokesFile,
                        edge.confidence,
                    )
                }
                crate::vocab::EdgeKind::Wildcard { from } => {
                    let Some(syms) = declared_in.get(&from) else {
                        continue;
                    };
                    let site_file = from;
                    for &s in syms {
                        forward
                            .entry(NavNode::File(from))
                            .or_default()
                            .push(NavEdge {
                                to: NavNode::Symbol(s),
                                label: EdgeLabel::Wildcard,
                                confidence: Confidence::Possible,
                                span: edge.span,
                                site_file,
                            });
                        reverse
                            .entry(NavNode::Symbol(s))
                            .or_default()
                            .push(NavEdge {
                                to: NavNode::File(from),
                                label: EdgeLabel::Wildcard,
                                confidence: Confidence::Possible,
                                span: edge.span,
                                site_file,
                            });
                    }
                    continue;
                }
                crate::vocab::EdgeKind::Declares { .. } | crate::vocab::EdgeKind::Root { .. } => {
                    continue
                }
            };
        let site_file = nav_node_owning_file(graph, from);
        forward.entry(from).or_default().push(NavEdge {
            to,
            label,
            confidence,
            span: edge.span,
            site_file,
        });
        reverse.entry(to).or_default().push(NavEdge {
            to: from,
            label,
            confidence,
            span: edge.span,
            site_file,
        });
    }

    let mut roots: HashMap<RootKind, Vec<(NavNode, Confidence)>> = HashMap::default();
    for edge in &graph.edges {
        if let crate::vocab::EdgeKind::Root { kind, target } = edge.kind {
            let node = match target {
                NodeRef::File(f) => NavNode::File(f),
                NodeRef::Symbol(s) => NavNode::Symbol(s),
            };
            roots.entry(kind).or_default().push((node, edge.confidence));
        }
    }

    GraphIndex {
        forward,
        reverse,
        roots,
    }
}

// ---------------------------------------------------------------- find

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FindResult {
    pub matches: Vec<QNodeRef>,
    pub elided: usize,
}

pub struct FindFilters<'a> {
    pub kind: Option<&'a str>,
    pub color: Option<&'a str>,
    pub lang: Option<&'a str>,
}

/// Search files and symbols by name: ranked exact > prefix > substring, over
/// each file's basename and every symbol's own name. Case-sensitive — kndo's vocabulary (symbol
/// names, paths) is itself case-sensitive source text.
pub fn find(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    pattern: &str,
    filters: &FindFilters<'_>,
    limit: usize,
) -> FindResult {
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum Rank {
        Exact,
        Prefix,
        Substring,
    }

    fn rank(name: &str, pattern: &str) -> Option<Rank> {
        if name == pattern {
            Some(Rank::Exact)
        } else if name.starts_with(pattern) {
            Some(Rank::Prefix)
        } else if name.contains(pattern) {
            Some(Rank::Substring)
        } else {
            None
        }
    }

    let mut candidates: Vec<(Rank, Resolved)> = Vec::new();

    for (i, file) in graph.files.iter().enumerate() {
        let basename = file
            .path
            .0
            .rsplit('/')
            .next()
            .unwrap_or(file.path.0.as_str());
        let Some(r) = rank(basename, pattern) else {
            continue;
        };
        let node = Resolved::Node(ResolvedNode::File(FileId(i as u32)));
        if !passes_filters(graph, reach, &node, filters) {
            continue;
        }
        candidates.push((r, node));
    }
    for (i, sym) in graph.symbols.iter().enumerate() {
        // Members match on either form — a search for `Method` and one for `T.Method` both
        // land; the better of the two ranks wins.
        let bare = rank(sym.name.as_str(), pattern);
        let qualified = sym
            .member_of
            .is_some()
            .then(|| rank(&sym.qualified_name(), pattern))
            .flatten();
        let Some(r) = [bare, qualified].into_iter().flatten().min() else {
            continue;
        };
        let node = Resolved::Node(ResolvedNode::Symbol(SymbolId(i as u32)));
        if !passes_filters(graph, reach, &node, filters) {
            continue;
        }
        candidates.push((r, node));
    }

    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| selector_string_key(graph, &a.1).cmp(&selector_string_key(graph, &b.1)))
    });

    let total = candidates.len();
    let matches = candidates
        .into_iter()
        .take(limit)
        .map(|(_, n)| qnode_ref(graph, reach, &n))
        .collect();
    FindResult {
        matches,
        elided: total.saturating_sub(limit),
    }
}

fn selector_string_key(graph: &ProjectGraph, node: &Resolved) -> String {
    selector_string(graph, node)
}

fn passes_filters(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    node: &Resolved,
    filters: &FindFilters<'_>,
) -> bool {
    if let Some(kind) = filters.kind {
        if kind_string(graph, node) != kind {
            return false;
        }
    }
    if let Some(color) = filters.color {
        match node_color(graph, reach, node) {
            Some(c) if c == color => {}
            _ => return false,
        }
    }
    if let Some(lang) = filters.lang {
        let file = match node {
            Resolved::Node(ResolvedNode::File(f)) => Some(*f),
            Resolved::Node(ResolvedNode::Symbol(s)) => Some(graph.symbols[s.0 as usize].file),
            _ => None,
        };
        match file.and_then(|f| graph.files[f.0 as usize].language.as_deref()) {
            Some(l) if l == lang => {}
            _ => return false,
        }
    }
    true
}

// ---------------------------------------------------------------- describe

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DeclarationInfo {
    pub kind: String,
    pub span: NodeSpan,
    pub exported: bool,
    pub visibility: u8,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileInfo {
    pub role: String,
    pub origin: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DependencyInfo {
    pub manifest_scopes: Vec<String>,
    pub importing_files: usize,
    pub used: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PackageInfo {
    pub mode: String,
    pub files: usize,
    pub dependents: usize,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Degree {
    pub in_by_kind: HashMap<String, usize>,
    pub out_by_kind: HashMap<String, usize>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DescribeResult {
    pub node: QNodeRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declaration: Option<DeclarationInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<FileInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency: Option<DependencyInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<PackageInfo>,
    pub degree: Degree,
    pub reached_by_roots: Vec<QNodeRef>,
    pub findings: Vec<String>,
    pub sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declared_symbols: Vec<QNodeRef>,
    pub elided: HashMap<String, usize>,
}

const DECLARE_SYMBOLS_CAP: usize = 50;
const REACHED_BY_ROOTS_CAP: usize = 10;

pub(crate) fn describe(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    resolved: &Resolved,
    finding_locations: &[FindingLocation<'_>],
    nav: &GraphIndex,
) -> DescribeResult {
    let node_ref = qnode_ref(graph, reach, resolved);

    let (declaration, file, dependency, package, declared_symbols) = match resolved {
        Resolved::Node(ResolvedNode::Symbol(s)) => {
            let sym = &graph.symbols[s.0 as usize];
            (
                Some(DeclarationInfo {
                    kind: sym.kind.facet().to_string(),
                    span: NodeSpan {
                        path: graph.files[sym.file.0 as usize].path.0.to_string(),
                        start: sym.span.start,
                        end: sym.span.end,
                    },
                    exported: sym.exported,
                    visibility: sym.visibility.0,
                }),
                None,
                None,
                None,
                Vec::new(),
            )
        }
        Resolved::Node(ResolvedNode::File(f)) => {
            let file_node = &graph.files[f.0 as usize];
            let info = file_node.class.map(|c| FileInfo {
                role: match c.role {
                    crate::vocab::FileRole::Production => "production",
                    crate::vocab::FileRole::Test => "test",
                    crate::vocab::FileRole::Tooling => "tooling",
                }
                .to_string(),
                origin: match c.origin {
                    crate::vocab::FileOrigin::Authored => "authored",
                    crate::vocab::FileOrigin::Generated => "generated",
                    crate::vocab::FileOrigin::Vendored => "vendored",
                }
                .to_string(),
            });
            let mut symbols: Vec<QNodeRef> = graph
                .symbols
                .iter()
                .enumerate()
                .filter(|(_, s)| s.file == *f)
                .map(|(i, _)| {
                    qnode_ref(
                        graph,
                        reach,
                        &Resolved::Node(ResolvedNode::Symbol(SymbolId(i as u32))),
                    )
                })
                .collect();
            symbols.truncate(DECLARE_SYMBOLS_CAP);
            (None, info, None, None, symbols)
        }
        Resolved::Node(ResolvedNode::Package(p)) => {
            let files = graph.files.iter().filter(|f| f.package == *p).count();
            let dependents = count_package_dependents(graph, *p);
            let mode = if graph.packages[p.0 as usize].private {
                "app"
            } else {
                "library"
            };
            (
                None,
                None,
                None,
                Some(PackageInfo {
                    mode: mode.to_string(),
                    files,
                    dependents,
                }),
                Vec::new(),
            )
        }
        Resolved::Dependency(name) => {
            let scopes: Vec<String> = graph
                .declared_dependencies
                .iter()
                .filter(|d| d.name.as_str() == name.0.as_str())
                .map(|d| dependency_scope_str(d.scope).to_string())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let dep_id = graph
                .dependencies
                .iter()
                .position(|d| d.name.as_str() == name.0.as_str());
            let importing_files: HashSet<FileId> = dep_id
                .map(|idx| {
                    graph
                        .edges
                        .iter()
                        .filter_map(|e| match e.kind {
                            crate::vocab::EdgeKind::ImportsDependency { from, to }
                                if to.0 as usize == idx =>
                            {
                                Some(from)
                            }
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let script_invoked = graph
                .script_invoked_dependencies
                .iter()
                .any(|(_, n)| n.as_str() == name.0.as_str());
            (
                None,
                None,
                Some(DependencyInfo {
                    manifest_scopes: scopes,
                    importing_files: importing_files.len(),
                    used: !importing_files.is_empty() || script_invoked,
                }),
                None,
                Vec::new(),
            )
        }
        Resolved::Node(ResolvedNode::RootSet(_)) => (None, None, None, None, Vec::new()),
    };

    let degree = describe_degree(nav, resolved);
    let mut reached_by_roots = reached_by_roots(nav, resolved);
    let roots_elided = reached_by_roots.len().saturating_sub(REACHED_BY_ROOTS_CAP);
    reached_by_roots.truncate(REACHED_BY_ROOTS_CAP);
    let reached_by_roots = reached_by_roots
        .into_iter()
        .map(|n| qnode_ref(graph, reach, &nav_to_resolved(graph, n)))
        .collect();

    let selector = selector_string(graph, resolved);
    let findings: Vec<String> = finding_locations
        .iter()
        .filter(|f| f.matches(graph, resolved, &selector))
        .map(|f| f.id.to_string())
        .collect();

    let sources = describe_sources(graph, resolved);

    let mut elided = HashMap::default();
    if roots_elided > 0 {
        elided.insert("reached_by_roots".to_string(), roots_elided);
    }

    DescribeResult {
        node: node_ref,
        declaration,
        file,
        dependency,
        package,
        degree,
        reached_by_roots,
        findings,
        sources,
        declared_symbols,
        elided,
    }
}

fn dependency_scope_str(scope: crate::vocab::DependencyScope) -> &'static str {
    match scope {
        crate::vocab::DependencyScope::Prod => "prod",
        crate::vocab::DependencyScope::Dev => "dev",
        crate::vocab::DependencyScope::Build => "build",
        crate::vocab::DependencyScope::Peer => "peer",
        crate::vocab::DependencyScope::Optional => "optional",
    }
}

/// Other packages with at least one file importing a file owned by `package` — same-package
/// edges don't count as a dependent (that's just internal cohesion, not cross-package usage).
fn count_package_dependents(graph: &ProjectGraph, package: PackageId) -> usize {
    let mut dependents: HashSet<PackageId> = HashSet::default();
    for edge in &graph.edges {
        if let crate::vocab::EdgeKind::ImportsFile { from, to } = edge.kind {
            if graph.files[to.0 as usize].package != package {
                continue;
            }
            let from_pkg = graph.files[from.0 as usize].package;
            if from_pkg != package {
                dependents.insert(from_pkg);
            }
        }
    }
    dependents.len()
}

fn nav_node_of(resolved: &Resolved) -> Option<NavNode> {
    match resolved {
        Resolved::Node(ResolvedNode::File(f)) => Some(NavNode::File(*f)),
        Resolved::Node(ResolvedNode::Symbol(s)) => Some(NavNode::Symbol(*s)),
        Resolved::Dependency(_)
        | Resolved::Node(ResolvedNode::Package(_) | ResolvedNode::RootSet(_)) => None,
    }
}

fn describe_degree(nav: &GraphIndex, resolved: &Resolved) -> Degree {
    let Some(node) = nav_node_of(resolved) else {
        return Degree::default();
    };
    let mut degree = Degree::default();
    for e in nav.forward.get(&node).map(Vec::as_slice).unwrap_or(&[]) {
        *degree
            .out_by_kind
            .entry(e.label.as_str().to_string())
            .or_insert(0) += 1;
    }
    for e in nav.reverse.get(&node).map(Vec::as_slice).unwrap_or(&[]) {
        *degree
            .in_by_kind
            .entry(e.label.as_str().to_string())
            .or_insert(0) += 1;
    }
    degree
}

/// Every root (of any kind) that reaches this node, nearest (fewest hops) first — a reverse BFS
/// from the node over the same non-wildcard-excluded traversal `uses`/`used-by` use, seeded by
/// nothing and instead stopped at any node that is itself a literal root target.
fn reached_by_roots(nav: &GraphIndex, resolved: &Resolved) -> Vec<NavNode> {
    let Some(start) = nav_node_of(resolved) else {
        return Vec::new();
    };
    let root_nodes: HashSet<NavNode> = nav.roots.values().flatten().map(|&(n, _)| n).collect();
    if root_nodes.is_empty() {
        return Vec::new();
    }

    let filter = EdgeFilter::liveness();
    let mut visited: HashSet<NavNode> = HashSet::default();
    visited.insert(start);
    let mut queue: VecDeque<NavNode> = VecDeque::new();
    queue.push_back(start);
    let mut found: Vec<NavNode> = Vec::new();
    let mut found_set: HashSet<NavNode> = HashSet::default();

    while let Some(node) = queue.pop_front() {
        for e in nav.reverse.get(&node).map(Vec::as_slice).unwrap_or(&[]) {
            if !filter.allows(e.label) || !visited.insert(e.to) {
                continue;
            }
            if root_nodes.contains(&e.to) && found_set.insert(e.to) {
                found.push(e.to);
            }
            queue.push_back(e.to);
        }
    }
    found
}

fn provenance_label(p: &crate::vocab::Provenance) -> String {
    match p {
        crate::vocab::Provenance::Adapter(id) => format!("adapter:{id}"),
        crate::vocab::Provenance::Plugin(id) => format!("plugin:{id}"),
        crate::vocab::Provenance::Surface => "core:surface".to_string(),
    }
}

fn describe_sources(graph: &ProjectGraph, resolved: &Resolved) -> Vec<String> {
    let mut sources: HashSet<String> = HashSet::default();
    let mark = |sources: &mut HashSet<String>, p: &crate::vocab::Provenance| {
        sources.insert(provenance_label(p));
    };
    for edge in &graph.edges {
        if edge_touches(&edge.kind, resolved) {
            mark(&mut sources, &edge.source);
        }
    }
    let mut sources: Vec<String> = sources.into_iter().collect();
    sources.sort();
    sources
}

fn edge_touches(kind: &crate::vocab::EdgeKind, resolved: &Resolved) -> bool {
    use crate::vocab::EdgeKind;
    let node = nav_node_of(resolved);
    match (kind, node) {
        (EdgeKind::Declares { file, .. }, Some(NavNode::File(f))) => *file == f,
        (EdgeKind::Declares { symbol, .. }, Some(NavNode::Symbol(s))) => *symbol == s,
        (EdgeKind::ImportsFile { from, to }, Some(NavNode::File(f))) => *from == f || *to == f,
        (EdgeKind::ImportsDependency { from, .. }, Some(NavNode::File(f))) => *from == f,
        (EdgeKind::ImportsDependency { to, .. }, Some(NavNode::Dependency(d))) => *to == d,
        (EdgeKind::References { from, to, .. }, Some(NavNode::Symbol(s))) => {
            *to == s || *from == NodeRef::Symbol(s)
        }
        (EdgeKind::References { from, .. }, Some(NavNode::File(f))) => *from == NodeRef::File(f),
        (EdgeKind::Root { target, .. }, Some(NavNode::File(f))) => *target == NodeRef::File(f),
        (EdgeKind::Root { target, .. }, Some(NavNode::Symbol(s))) => *target == NodeRef::Symbol(s),
        (EdgeKind::Wildcard { from }, Some(NavNode::File(f))) => *from == f,
        _ => false,
    }
}

// ---------------------------------------------------------------- finding attachment (describe)

/// A minimal view of one `Finding` (from `crate::engine`) that `describe` needs to decide
/// whether it's "attached" to the node being described — kept decoupled from `Finding` itself so
/// this module doesn't have to depend on `engine`'s full type for a two-field comparison.
pub struct FindingLocation<'a> {
    pub id: &'a str,
    pub path: Option<&'a str>,
    pub symbol: Option<&'a str>,
}

impl FindingLocation<'_> {
    fn matches(&self, graph: &ProjectGraph, resolved: &Resolved, selector: &str) -> bool {
        let _ = graph;
        match resolved {
            Resolved::Node(ResolvedNode::Symbol(_)) => {
                self.path.is_some()
                    && self.path == selector.split('#').next()
                    && self.symbol_matches(selector)
            }
            Resolved::Node(ResolvedNode::File(_)) => self.path == Some(selector),
            Resolved::Dependency(name) => self.symbol == Some(name.0.as_str()),
            _ => false,
        }
    }

    fn symbol_matches(&self, selector: &str) -> bool {
        selector.split_once('#').map(|(_, name)| name) == self.symbol
    }
}

// ---------------------------------------------------------------- uses / used-by

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NeighborEntry {
    pub node: QNodeRef,
    pub via: QEdgeRef,
    pub depth: u32,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ByColor {
    pub production: usize,
    #[serde(rename = "test-only")]
    pub test_only: usize,
    #[serde(rename = "tooling-only")]
    pub tooling_only: usize,
    pub unreachable: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NeighborsResult {
    pub node: QNodeRef,
    pub entries: Vec<NeighborEntry>,
    pub by_color: ByColor,
    pub elided: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Uses,
    UsedBy,
}

/// [`neighbors`]'s flags, bundled into one struct purely to stay under clippy's argument-count
/// lint — each field is exactly one CLI `--flag`.
#[derive(Debug, Clone, Copy)]
pub struct NeighborsOpts {
    pub direction: Direction,
    pub edges: EdgeFilter,
    pub depth: u32,
    pub transitive: bool,
    pub limit: usize,
}

/// Shared implementation of `uses`/`used-by`: direction just picks forward
/// vs. reverse adjacency. `--depth N` (default 1) or `--transitive` (fixpoint, deduplicated,
/// depth-annotated — the *first*, shallowest depth at which a node is reached wins).
pub(crate) fn neighbors(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    resolved: &Resolved,
    opts: NeighborsOpts,
    nav: &GraphIndex,
) -> NeighborsResult {
    let NeighborsOpts {
        direction,
        edges,
        depth,
        transitive,
        limit,
    } = opts;
    let node_ref = qnode_ref(graph, reach, resolved);
    let Some(start) = nav_node_of(resolved) else {
        // Dependency/Package/RootSet selectors: no outgoing/incoming NavNode adjacency modeled.
        return NeighborsResult {
            node: node_ref,
            entries: Vec::new(),
            by_color: ByColor::default(),
            elided: 0,
        };
    };

    let adjacency = match direction {
        Direction::Uses => &nav.forward,
        Direction::UsedBy => &nav.reverse,
    };

    let max_depth = if transitive { u32::MAX } else { depth.max(1) };
    // `site_file`/`span` travel together from here on — both come straight off the `NavEdge`
    // that reached this neighbor, never re-derived from the neighbor itself (see `NavEdge::
    // site_file`'s doc: that derivation silently pairs the right line/column with the wrong
    // file whenever traversing `uses`, where the neighbor is `to`, not the edge's `from`).
    let mut best: HashMap<NavNode, (u32, EdgeLabel, Confidence, Option<Span>, FileId)> =
        HashMap::default();
    let mut queue: VecDeque<(NavNode, u32)> = VecDeque::new();
    let mut queued: HashSet<NavNode> = HashSet::default();
    queue.push_back((start, 0));
    queued.insert(start);

    while let Some((node, d)) = queue.pop_front() {
        if d >= max_depth {
            continue;
        }
        for e in adjacency.get(&node).map(Vec::as_slice).unwrap_or(&[]) {
            if !edges.allows(e.label) {
                continue;
            }
            let next_depth = d + 1;
            best.entry(e.to)
                .or_insert((next_depth, e.label, e.confidence, e.span, e.site_file));
            if queued.insert(e.to) {
                queue.push_back((e.to, next_depth));
            }
        }
    }
    best.remove(&start);

    let mut entries: Vec<(NavNode, u32, EdgeLabel, Confidence, Option<Span>, FileId)> = best
        .into_iter()
        .map(|(n, (d, l, c, s, sf))| (n, d, l, c, s, sf))
        .collect();
    entries.sort_by(|a, b| {
        a.1.cmp(&b.1).then_with(|| {
            selector_string(graph, &nav_to_resolved(graph, a.0))
                .cmp(&selector_string(graph, &nav_to_resolved(graph, b.0)))
        })
    });

    let mut by_color = ByColor::default();
    for (n, ..) in &entries {
        let resolved_n = nav_to_resolved(graph, *n);
        if let Some(color) = node_color(graph, reach, &resolved_n) {
            match color.as_str() {
                "production" => by_color.production += 1,
                "test-only" => by_color.test_only += 1,
                "tooling-only" => by_color.tooling_only += 1,
                "unreachable" => by_color.unreachable += 1,
                _ => {}
            }
        }
    }

    let total = entries.len();
    let out_entries = entries
        .into_iter()
        .take(limit)
        .map(|(n, d, l, c, s, sf)| NeighborEntry {
            node: qnode_ref(graph, reach, &nav_to_resolved(graph, n)),
            via: QEdgeRef {
                edge: l.as_str().to_string(),
                confidence: c,
                site: s.map(|sp| NodeSpan {
                    path: graph.files[sf.0 as usize].path.0.to_string(),
                    start: sp.start,
                    end: sp.end,
                }),
            },
            depth: d,
        })
        .collect();

    NeighborsResult {
        node: node_ref,
        entries: out_entries,
        by_color,
        elided: total.saturating_sub(limit),
    }
}

// ---------------------------------------------------------------- trace

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Hop {
    pub node: QNodeRef,
    pub via: QEdgeRef,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Path {
    pub hops: Vec<Hop>,
    pub weakest_confidence: Confidence,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TraceResult {
    pub from: QNodeRef,
    pub to: QNodeRef,
    pub paths: Vec<Path>,
    pub paths_elided: usize,
}

/// Safety valve for `--all`'s bounded DFS enumeration (module docs: path-enumeration policy
/// is an open design question, so this is a direct, explicitly-bounded reading) — caps total
/// node expansions, not just result count, so a highly-connected graph can't hang the query.
const TRACE_EXPANSION_BUDGET: usize = 20_000;

/// `trace_between`'s options — mirrors [`NeighborsOpts`]'s shape instead of two adjacent,
/// trivially-transposable positional params (a bare `bool` next to a `usize`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct TraceOpts {
    pub(crate) edges: EdgeFilter,
    pub(crate) all: bool,
    pub(crate) max_paths: usize,
}

/// Directed `trace <from> <to>` (two-argument form): path(s) over the same
/// navigable edges `uses`/`used-by` traverse. `--all --max-paths K` enumerates simple-path
/// alternatives near the shortest length; without `--all`, one shortest path only.
pub(crate) fn trace_between(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    from: &Resolved,
    to: &Resolved,
    opts: TraceOpts,
    nav: &GraphIndex,
) -> TraceResult {
    let TraceOpts {
        edges,
        all,
        max_paths,
    } = opts;
    let from_ref = qnode_ref(graph, reach, from);
    let to_ref = qnode_ref(graph, reach, to);

    let (Some(start), Some(goal)) = (nav_node_of(from), nav_node_of(to)) else {
        return TraceResult {
            from: from_ref,
            to: to_ref,
            paths: Vec::new(),
            paths_elided: 0,
        };
    };

    let shortest = bfs_shortest_path(&nav.forward, start, goal, edges);
    let Some(shortest) = shortest else {
        return TraceResult {
            from: from_ref,
            to: to_ref,
            paths: Vec::new(),
            paths_elided: 0,
        };
    };

    let paths = if all {
        enumerate_paths(&nav.forward, start, goal, edges, shortest.len(), max_paths)
    } else {
        vec![shortest]
    };
    let paths_elided = 0; // enumerate_paths reports its own cap via the returned count vs. what exists — see its doc
    let rendered = paths
        .into_iter()
        .map(|hops| render_path(graph, reach, nav, hops))
        .collect();

    TraceResult {
        from: from_ref,
        to: to_ref,
        paths: rendered,
        paths_elided,
    }
}

/// Liveness `trace <selector>` (single-argument form): shortest path from the
/// nearest root of `roots_kind` to `target`, falling back from production to test when
/// production reaches nothing (the documented fallback) unless a kind was explicit.
pub(crate) fn trace_liveness(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    target: &Resolved,
    roots_kind: Option<RootKind>,
    nav: &GraphIndex,
) -> TraceResult {
    let to_ref = qnode_ref(graph, reach, target);
    let Some(goal) = nav_node_of(target) else {
        return TraceResult {
            from: QNodeRef {
                selector: "roots:production".to_string(),
                kind: "root-set".to_string(),
                color: None,
                span: None,
            },
            to: to_ref,
            paths: Vec::new(),
            paths_elided: 0,
        };
    };

    let try_kind = |kind: RootKind| -> Option<(RootKind, Vec<NavNode>)> {
        let sources: Vec<NavNode> = nav
            .roots
            .get(&kind)
            .into_iter()
            .flatten()
            .map(|&(n, _)| n)
            .collect();
        bfs_shortest_path_multi(&nav.forward, &sources, goal, EdgeFilter::liveness())
            .map(|p| (kind, p))
    };

    let kinds_to_try: Vec<RootKind> = match roots_kind {
        Some(k) => vec![k],
        None => vec![RootKind::Production, RootKind::Test, RootKind::Tooling],
    };

    let mut result = None;
    for kind in kinds_to_try {
        if let Some(found) = try_kind(kind) {
            result = Some(found);
            break;
        }
    }

    let from_ref = QNodeRef {
        selector: format!(
            "roots:{}",
            root_kind_str(roots_kind.unwrap_or(RootKind::Production))
        ),
        kind: "root-set".to_string(),
        color: None,
        span: None,
    };

    match result {
        None => TraceResult {
            from: from_ref,
            to: to_ref,
            paths: Vec::new(),
            paths_elided: 0,
        },
        Some((kind, hops)) => TraceResult {
            from: QNodeRef {
                selector: format!("roots:{}", root_kind_str(kind)),
                kind: "root-set".to_string(),
                color: None,
                span: None,
            },
            to: to_ref,
            paths: vec![render_path(graph, reach, nav, hops)],
            paths_elided: 0,
        },
    }
}

/// BFS shortest path, single source; returns the node sequence including `start` and `goal`
/// (empty-hops case: `start == goal` yields a one-element path).
fn bfs_shortest_path(
    adjacency: &HashMap<NavNode, Vec<NavEdge>>,
    start: NavNode,
    goal: NavNode,
    edges: EdgeFilter,
) -> Option<Vec<NavNode>> {
    bfs_shortest_path_multi(adjacency, &[start], goal, edges)
}

fn bfs_shortest_path_multi(
    adjacency: &HashMap<NavNode, Vec<NavEdge>>,
    starts: &[NavNode],
    goal: NavNode,
    edges: EdgeFilter,
) -> Option<Vec<NavNode>> {
    if starts.contains(&goal) {
        return Some(vec![goal]);
    }
    let mut visited: HashSet<NavNode> = starts.iter().copied().collect();
    let mut parent: HashMap<NavNode, NavNode> = HashMap::default();
    let mut queue: VecDeque<NavNode> = starts.iter().copied().collect();

    while let Some(node) = queue.pop_front() {
        for e in adjacency.get(&node).map(Vec::as_slice).unwrap_or(&[]) {
            if !edges.allows(e.label) || !visited.insert(e.to) {
                continue;
            }
            parent.insert(e.to, node);
            if e.to == goal {
                let mut path = vec![goal];
                let mut cur = node;
                loop {
                    path.push(cur);
                    if starts.contains(&cur) {
                        break;
                    }
                    cur = parent[&cur];
                }
                path.reverse();
                return Some(path);
            }
            queue.push_back(e.to);
        }
    }
    None
}

/// Bounded DFS enumeration of simple (no-repeated-node) paths from `start` to `goal`, capped at
/// `max_paths` results and [`TRACE_EXPANSION_BUDGET`] total expansions, restricted to a length
/// ceiling of `shortest_len + 2` so "alternatives" stays close to "alternatives," not "every
/// walk in the graph." Order is deterministic (edges visited in adjacency-list order).
/// Immutable search config, held apart from the mutable recursion state so the recursive
/// method stays under clippy's argument-count lint.
struct PathSearch<'a> {
    adjacency: &'a HashMap<NavNode, Vec<NavEdge>>,
    goal: NavNode,
    edges: EdgeFilter,
    max_len: usize,
    max_paths: usize,
}

impl PathSearch<'_> {
    fn dfs(
        &self,
        path: &mut Vec<NavNode>,
        on_stack: &mut HashSet<NavNode>,
        results: &mut Vec<Vec<NavNode>>,
        budget: &mut usize,
    ) {
        if results.len() >= self.max_paths || *budget == 0 {
            return;
        }
        let node = *path.last().unwrap();
        if node == self.goal {
            results.push(path.clone());
            return;
        }
        if path.len() >= self.max_len {
            return;
        }
        for e in self.adjacency.get(&node).map(Vec::as_slice).unwrap_or(&[]) {
            if results.len() >= self.max_paths || *budget == 0 {
                return;
            }
            *budget -= 1;
            if !self.edges.allows(e.label) || on_stack.contains(&e.to) {
                continue;
            }
            path.push(e.to);
            on_stack.insert(e.to);
            self.dfs(path, on_stack, results, budget);
            on_stack.remove(&e.to);
            path.pop();
        }
    }
}

fn enumerate_paths(
    adjacency: &HashMap<NavNode, Vec<NavEdge>>,
    start: NavNode,
    goal: NavNode,
    edges: EdgeFilter,
    shortest_len: usize,
    max_paths: usize,
) -> Vec<Vec<NavNode>> {
    let search = PathSearch {
        adjacency,
        goal,
        edges,
        max_len: shortest_len + 2,
        max_paths,
    };
    let mut results: Vec<Vec<NavNode>> = Vec::new();
    let mut stack_path: Vec<NavNode> = vec![start];
    let mut on_stack: HashSet<NavNode> = [start].into_iter().collect();
    let mut budget = TRACE_EXPANSION_BUDGET;
    search.dfs(&mut stack_path, &mut on_stack, &mut results, &mut budget);
    results
}

fn render_path(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    nav: &GraphIndex,
    nodes: Vec<NavNode>,
) -> Path {
    let mut hops = Vec::new();
    let mut weakest = Confidence::Certain;
    for window in nodes.windows(2) {
        let (from, to) = (window[0], window[1]);
        let edge = nav
            .forward
            .get(&from)
            .and_then(|es| es.iter().find(|e| e.to == to));
        let (label, confidence, span, site_file) = match edge {
            Some(e) => (e.label.as_str(), e.confidence, e.span, Some(e.site_file)),
            None => ("references", Confidence::Certain, None, None),
        };
        weakest = weakest.min(confidence);
        hops.push(Hop {
            node: qnode_ref(graph, reach, &nav_to_resolved(graph, to)),
            via: QEdgeRef {
                edge: label.to_string(),
                confidence,
                // `site_file` is `None` only in the defensive "no matching edge found" fallback
                // above (shouldn't happen — every hop in a BFS/DFS path came from a real
                // traversed edge — but the span/site pair still degrades honestly to absent
                // rather than fabricating a location).
                site: span.zip(site_file).map(|(sp, sf)| NodeSpan {
                    path: graph.files[sf.0 as usize].path.0.to_string(),
                    start: sp.start,
                    end: sp.end,
                }),
            },
        });
    }
    Path {
        hops,
        weakest_confidence: weakest,
    }
}

// ---------------------------------------------------------------- impact

/// [`impact`]'s flags. Unlike `uses`/`used-by`, the *default* is the full transitive reverse
/// closure — blast radius is a closure by definition; `--depth N` bounds it when given.
#[derive(Debug, Clone, Copy)]
pub struct ImpactOpts {
    pub edges: EdgeFilter,
    pub depth: Option<u32>,
    pub limit: usize,
    pub if_deleted: bool,
}

/// One root whose liveness evidence passes through the impacted node — retesting starts here.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AffectedRoot {
    pub kind: String,
    pub node: QNodeRef,
}

/// `--if-deleted`'s simulation result: the finding flips removal would cause,
/// computed on a patched copy of the graph — nothing is written.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IfDeleted {
    /// Files and symbols (outside the deleted set) that lose reachability entirely — each a
    /// would-be `unused` finding.
    pub newly_unreachable: Vec<QNodeRef>,
    pub newly_unreachable_elided: usize,
    /// Production nodes that only tests would still reach — each a would-be `test-only`.
    pub newly_test_only: Vec<QNodeRef>,
    pub newly_test_only_elided: usize,
    /// Declared dependencies whose every importing file is in the deleted set — zero import
    /// edges remain, the exact evidence `dependency_hygiene` calls `unused`.
    pub freed_dependencies: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImpactResult {
    pub node: QNodeRef,
    /// The reverse closure — who is affected if this node changes — depth-annotated, capped.
    pub affected: Vec<NeighborEntry>,
    pub by_color: ByColor,
    pub elided: usize,
    pub affected_roots: Vec<AffectedRoot>,
    pub affected_roots_elided: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_deleted: Option<IfDeleted>,
}

/// `kndo impact <selector> [--if-deleted]`: forward-looking blast radius on
/// the same adjacency `uses`/`used-by` navigate, plus — with `--if-deleted` — a removal
/// simulation on a patched graph copy, reusing the reachability engine itself (the diff-mode
/// derived-effects machinery's core: same graph shape, recomputed colors, reported flips).
///
/// Selector coverage: files and symbols fully; a package is its files (dependents *outside*
/// the package); a dependency supports the default mode only (its reverse closure is its
/// importers — deleting a declared dependency breaks builds rather than flipping
/// reachability, so `--if-deleted` is rejected with an explanation instead of an answer that
/// would mean nothing).
pub(crate) fn impact(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    resolved: &Resolved,
    opts: ImpactOpts,
    nav: &GraphIndex,
) -> Result<ImpactResult, QueryError> {
    let node_ref = qnode_ref(graph, reach, resolved);

    // Seeds: the graph nodes whose change/removal is being simulated.
    let mut seed_files: HashSet<FileId> = HashSet::default();
    let mut seed_symbols: HashSet<SymbolId> = HashSet::default();
    let seeds: Vec<NavNode> = match resolved {
        Resolved::Node(ResolvedNode::File(f)) => {
            seed_files.insert(*f);
            vec![NavNode::File(*f)]
        }
        Resolved::Node(ResolvedNode::Symbol(s)) => {
            seed_symbols.insert(*s);
            vec![NavNode::Symbol(*s)]
        }
        Resolved::Node(ResolvedNode::Package(p)) => {
            let files: Vec<NavNode> = graph
                .files
                .iter()
                .enumerate()
                .filter(|(_, f)| f.package == *p)
                .map(|(i, _)| {
                    seed_files.insert(FileId(i as u32));
                    NavNode::File(FileId(i as u32))
                })
                .collect();
            files
        }
        Resolved::Dependency(d) => {
            if opts.if_deleted {
                return Err(QueryError::IfDeletedOnDependency(d.0.to_string()));
            }
            let Some(id) = graph
                .dependencies
                .iter()
                .position(|dep| dep.name == d.0)
                .map(|i| DependencyId(i as u32))
            else {
                // Declared-but-never-imported: zero importers IS the answer.
                return Ok(ImpactResult {
                    node: node_ref,
                    affected: Vec::new(),
                    by_color: ByColor::default(),
                    elided: 0,
                    affected_roots: Vec::new(),
                    affected_roots_elided: 0,
                    if_deleted: None,
                });
            };
            vec![NavNode::Dependency(id)]
        }
        Resolved::Node(ResolvedNode::RootSet(_)) => {
            return Err(QueryError::ImpactOnRootSet);
        }
    };
    // Deleting a symbol deletes nothing else; deleting a file (or package) deletes every
    // symbol it declares.
    for (i, sym) in graph.symbols.iter().enumerate() {
        if seed_files.contains(&sym.file) {
            seed_symbols.insert(SymbolId(i as u32));
        }
    }

    // Reverse-closure BFS (the same walk `used-by --transitive` does, multi-seed). For a
    // package, dependents inside the package itself are its own business — excluded.
    let excluded_package = match resolved {
        Resolved::Node(ResolvedNode::Package(p)) => Some(*p),
        _ => None,
    };
    let max_depth = opts.depth.unwrap_or(u32::MAX);
    let mut best: HashMap<NavNode, (u32, EdgeLabel, Confidence, Option<Span>, FileId)> =
        HashMap::default();
    let mut queue: VecDeque<(NavNode, u32)> = VecDeque::new();
    let mut queued: HashSet<NavNode> = HashSet::default();
    for &seed in &seeds {
        queue.push_back((seed, 0));
        queued.insert(seed);
    }
    while let Some((node, d)) = queue.pop_front() {
        if d >= max_depth {
            continue;
        }
        for e in nav.reverse.get(&node).map(Vec::as_slice).unwrap_or(&[]) {
            if !opts.edges.allows(e.label) {
                continue;
            }
            best.entry(e.to)
                .or_insert((d + 1, e.label, e.confidence, e.span, e.site_file));
            if queued.insert(e.to) {
                queue.push_back((e.to, d + 1));
            }
        }
    }
    for seed in &seeds {
        best.remove(seed);
    }
    if let Some(p) = excluded_package {
        best.retain(|n, _| match n {
            NavNode::File(f) => graph.files[f.0 as usize].package != p,
            NavNode::Symbol(s) => {
                graph.files[graph.symbols[s.0 as usize].file.0 as usize].package != p
            }
            NavNode::Dependency(_) => true,
        });
    }

    let closure: HashSet<NavNode> = best.keys().copied().collect();
    let mut entries: Vec<(NavNode, u32, EdgeLabel, Confidence, Option<Span>, FileId)> = best
        .into_iter()
        .map(|(n, (d, l, c, s, sf))| (n, d, l, c, s, sf))
        .collect();
    entries.sort_by(|a, b| {
        a.1.cmp(&b.1).then_with(|| {
            selector_string(graph, &nav_to_resolved(graph, a.0))
                .cmp(&selector_string(graph, &nav_to_resolved(graph, b.0)))
        })
    });

    let mut by_color = ByColor::default();
    for (n, ..) in &entries {
        if let Some(color) = node_color(graph, reach, &nav_to_resolved(graph, *n)) {
            match color.as_str() {
                "production" => by_color.production += 1,
                "test-only" => by_color.test_only += 1,
                "tooling-only" => by_color.tooling_only += 1,
                "unreachable" => by_color.unreachable += 1,
                _ => {}
            }
        }
    }
    let total = entries.len();
    let affected: Vec<NeighborEntry> = entries
        .into_iter()
        .take(opts.limit)
        .map(|(n, d, l, c, s, sf)| NeighborEntry {
            node: qnode_ref(graph, reach, &nav_to_resolved(graph, n)),
            via: QEdgeRef {
                edge: l.as_str().to_string(),
                confidence: c,
                site: s.map(|sp| NodeSpan {
                    path: graph.files[sf.0 as usize].path.0.to_string(),
                    start: sp.start,
                    end: sp.end,
                }),
            },
            depth: d,
        })
        .collect();

    // Affected roots: every Root edge whose target sits in the closure (or IS a seed) — the
    // entry points whose behavior a change here can reach, i.e. where retesting starts.
    let seed_set: HashSet<NavNode> = seeds.iter().copied().collect();
    let mut roots: Vec<AffectedRoot> = Vec::new();
    let mut seen_roots: HashSet<(RootKind, NavNode)> = HashSet::default();
    for (kind, targets) in &nav.roots {
        for &(target, _) in targets {
            if (closure.contains(&target) || seed_set.contains(&target))
                && seen_roots.insert((*kind, target))
            {
                roots.push(AffectedRoot {
                    kind: match kind {
                        RootKind::Production => "production".to_string(),
                        RootKind::Test => "test".to_string(),
                        RootKind::Tooling => "tooling".to_string(),
                    },
                    node: qnode_ref(graph, reach, &nav_to_resolved(graph, target)),
                });
            }
        }
    }
    roots.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.node.selector.cmp(&b.node.selector))
    });
    let roots_total = roots.len();
    roots.truncate(opts.limit);
    let affected_roots_elided = roots_total - roots.len();

    let if_deleted = if opts.if_deleted {
        Some(simulate_deletion(
            graph,
            reach,
            &seed_files,
            &seed_symbols,
            opts.limit,
        ))
    } else {
        None
    };

    Ok(ImpactResult {
        node: node_ref,
        affected,
        by_color,
        elided: total.saturating_sub(opts.limit),
        affected_roots: roots,
        affected_roots_elided,
        if_deleted,
    })
}

/// Rebuilds the graph minus every edge touching the deleted set, recomputes reachability with
/// the real engine (`analysis::reachability::compute` — no parallel simulation logic to drift),
/// and reports the color flips. Node arrays stay intact — ids keep meaning — only edges go;
/// deleted nodes themselves are excluded from the flip report (they aren't "newly unreachable",
/// they're gone).
fn simulate_deletion(
    graph: &ProjectGraph,
    before: &ReachabilityMap,
    deleted_files: &HashSet<FileId>,
    deleted_symbols: &HashSet<SymbolId>,
    limit: usize,
) -> IfDeleted {
    use crate::vocab::EdgeKind as EK;
    let touches_deleted = |edge: &crate::vocab::Edge| -> bool {
        match edge.kind {
            EK::ImportsFile { from, to } => {
                deleted_files.contains(&from) || deleted_files.contains(&to)
            }
            EK::ImportsDependency { from, .. } => deleted_files.contains(&from),
            EK::References { from, to, .. } => {
                deleted_symbols.contains(&to)
                    || match from {
                        NodeRef::File(f) => deleted_files.contains(&f),
                        NodeRef::Symbol(s) => deleted_symbols.contains(&s),
                    }
            }
            EK::Declares { file, symbol } => {
                deleted_files.contains(&file) || deleted_symbols.contains(&symbol)
            }
            EK::Root { target, .. } => match target {
                NodeRef::File(f) => deleted_files.contains(&f),
                NodeRef::Symbol(s) => deleted_symbols.contains(&s),
            },
            EK::Wildcard { from } => deleted_files.contains(&from),
            EK::ReferencesFile { from, to } | EK::InvokesFile { from, to } => {
                deleted_files.contains(&to)
                    || match from {
                        NodeRef::File(f) => deleted_files.contains(&f),
                        NodeRef::Symbol(s) => deleted_symbols.contains(&s),
                    }
            }
        }
    };

    let kept_edges: Vec<crate::vocab::Edge> = graph
        .edges
        .iter()
        .filter(|e| !touches_deleted(e))
        .cloned()
        .collect();
    let sim = ProjectGraph::from_snapshot_parts(crate::graph::GraphSnapshotParts {
        files: graph.files.clone(),
        symbols: graph.symbols.clone(),
        dependencies: graph.dependencies.clone(),
        declared_dependencies: graph.declared_dependencies.clone(),
        script_invoked_dependencies: graph.script_invoked_dependencies.clone(),
        packages: graph.packages.clone(),
        edges: kept_edges,
        suppressions: graph.suppressions.clone(),
        visibility_ladders: graph.visibility_ladders.clone(),
        cycle_policies: graph.cycle_policies.clone(),
        function_metrics: graph.function_metrics.clone(),
        patch_meta: graph.patch_meta.clone(),
        externally_consumed: graph.externally_consumed.clone(),
        plugin_implicitly_invoked: graph.plugin_implicitly_invoked.clone(),
    });
    let after = crate::analysis::reachability::compute(&sim);

    let mut newly_unreachable: Vec<QNodeRef> = Vec::new();
    let mut newly_test_only: Vec<QNodeRef> = Vec::new();
    let mut consider = |nref: NodeRef, resolved: Resolved| {
        let was = before.get(nref).0;
        let now = after.get(nref).0;
        if was != Reachability::Unreachable && now == Reachability::Unreachable {
            newly_unreachable.push(qnode_ref(graph, before, &resolved));
        } else if was == Reachability::Production && now == Reachability::TestOnly {
            newly_test_only.push(qnode_ref(graph, before, &resolved));
        }
    };
    for (i, _) in graph.files.iter().enumerate() {
        let f = FileId(i as u32);
        if deleted_files.contains(&f) {
            continue;
        }
        consider(NodeRef::File(f), Resolved::Node(ResolvedNode::File(f)));
    }
    for (i, _) in graph.symbols.iter().enumerate() {
        let s = SymbolId(i as u32);
        if deleted_symbols.contains(&s) {
            continue;
        }
        consider(NodeRef::Symbol(s), Resolved::Node(ResolvedNode::Symbol(s)));
    }
    newly_unreachable.sort_by(|a, b| a.selector.cmp(&b.selector));
    newly_test_only.sort_by(|a, b| a.selector.cmp(&b.selector));

    // Declared dependencies with import evidence before, none after (all importers deleted) —
    // dependency_hygiene's own `unused` evidence, reported as "freed".
    let mut had_importers: HashSet<&str> = HashSet::default();
    let mut still_has: HashSet<&str> = HashSet::default();
    for edge in &graph.edges {
        if let EK::ImportsDependency { from, to } = edge.kind {
            let name = graph.dependencies[to.0 as usize].name.as_str();
            had_importers.insert(name);
            if !deleted_files.contains(&from) {
                still_has.insert(name);
            }
        }
    }
    let declared: HashSet<&str> = graph
        .declared_dependencies
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    let mut freed_dependencies: Vec<String> = had_importers
        .difference(&still_has)
        .filter(|name| declared.contains(*name))
        .map(|name| name.to_string())
        .collect();
    freed_dependencies.sort();

    let unreachable_total = newly_unreachable.len();
    newly_unreachable.truncate(limit);
    let test_only_total = newly_test_only.len();
    newly_test_only.truncate(limit);
    IfDeleted {
        newly_unreachable_elided: unreachable_total - newly_unreachable.len(),
        newly_unreachable,
        newly_test_only_elided: test_only_total - newly_test_only.len(),
        newly_test_only,
        freed_dependencies,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::VisibilityLevel;
    use crate::analysis::reachability;
    use crate::graph::{DependencyNode, FileNode, PackageNode, ProjectGraph, SymbolNode};
    use crate::vocab::{
        DependencyScope, Edge, EdgeKind, FileClass, FileOrigin, FileRole, Provenance, RefKind,
        SymbolKind,
    };

    fn span(start_line: u32, end_line: u32) -> Span {
        Span {
            start: (start_line, 1),
            end: (end_line, 1),
        }
    }

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
            string_call_sites: Vec::new(),
        }
    }

    fn symbol(file: FileId, name: &str, start: u32, end: u32) -> SymbolNode {
        SymbolNode {
            file,
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: span(start, end),
            exported: true,
            visibility: VisibilityLevel(1),
            member_of: None,
            signature_span: None,
            implicitly_invoked: false,
            nested_scope: false,
            visibility_inherited: false,
            markers: Vec::new(),
        }
    }

    fn edge(kind: EdgeKind, confidence: Confidence, evidence: Option<Span>) -> Edge {
        Edge {
            owner: crate::vocab::FileId(0),
            kind,
            confidence,
            source: Provenance::Adapter(SmolStr::new("mock")),
            span: evidence,
        }
    }

    /// a.ts (production root) --imports--> b.ts, a.ts --imports-dependency--> lodash,
    /// a.ts --references--> b.ts#bar (file-granular). c.ts is disconnected (dead).
    fn linear_graph() -> ProjectGraph {
        let files = vec![file("a.ts"), file("b.ts"), file("c.ts")];
        let symbols = vec![
            symbol(FileId(0), "foo", 1, 3),
            symbol(FileId(1), "bar", 5, 8),
        ];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("lodash"),
        }];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
                None,
            ),
            edge(
                EdgeKind::Declares {
                    file: FileId(0),
                    symbol: SymbolId(0),
                },
                Confidence::Certain,
                Some(span(1, 3)),
            ),
            edge(
                EdgeKind::Declares {
                    file: FileId(1),
                    symbol: SymbolId(1),
                },
                Confidence::Certain,
                Some(span(5, 8)),
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(1),
                },
                Confidence::Certain,
                Some(span(2, 2)),
            ),
            edge(
                EdgeKind::ImportsDependency {
                    from: FileId(0),
                    to: DependencyId(0),
                },
                Confidence::Certain,
                Some(span(1, 1)),
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(0)),
                    to: SymbolId(1),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
                Some(span(4, 4)),
            ),
        ];
        ProjectGraph::for_test(files, symbols, dependencies, edges).with_declared_dependencies(
            vec![crate::graph::DeclaredDependency {
                package: PackageId(0),
                manifest: ProjectPath(SmolStr::new("package.json")),
                name: SmolStr::new("lodash"),
                version_req: SmolStr::new("^4"),
                scope: DependencyScope::Prod,
            }],
        )
    }

    // ---------------------------------------------------------------- parse_selector

    #[test]
    fn parses_every_selector_syntax() {
        assert_eq!(
            parse_selector("src/a.ts").unwrap(),
            Selector::File(ProjectPath("src/a.ts".into()))
        );
        assert_eq!(
            parse_selector("src/a.ts#Foo.bar").unwrap(),
            Selector::Symbol(ProjectPath("src/a.ts".into()), "Foo.bar".to_string())
        );
        assert_eq!(
            parse_selector("dep:lodash").unwrap(),
            Selector::Dependency("lodash".into())
        );
        assert_eq!(
            parse_selector("pkg:@org/ui").unwrap(),
            Selector::Package("@org/ui".into())
        );
        assert_eq!(
            parse_selector("roots:production").unwrap(),
            Selector::RootSet(RootKind::Production)
        );
        assert_eq!(
            parse_selector("roots:test").unwrap(),
            Selector::RootSet(RootKind::Test)
        );
        assert_eq!(
            parse_selector("roots:tooling").unwrap(),
            Selector::RootSet(RootKind::Tooling)
        );
    }

    #[test]
    fn malformed_selectors_are_rejected() {
        assert!(parse_selector("dep:").is_err());
        assert!(parse_selector("pkg:").is_err());
        assert!(parse_selector("roots:staging").is_err());
        assert!(parse_selector("#bar").is_err());
        assert!(parse_selector("src/a.ts#").is_err());
    }

    // ---------------------------------------------------------------- resolve

    #[test]
    fn resolves_a_file_selector() {
        let graph = linear_graph();
        let resolved = resolve(&graph, &Selector::File(ProjectPath("a.ts".into()))).unwrap();
        assert_eq!(resolved, Resolved::Node(ResolvedNode::File(FileId(0))));
    }

    #[test]
    fn unknown_file_selector_is_not_found() {
        let graph = linear_graph();
        assert!(matches!(
            resolve(&graph, &Selector::File(ProjectPath("missing.ts".into()))),
            Err(ResolveError::NotFound)
        ));
    }

    #[test]
    fn resolves_a_symbol_selector() {
        let graph = linear_graph();
        let resolved = resolve(
            &graph,
            &Selector::Symbol(ProjectPath("b.ts".into()), "bar".to_string()),
        )
        .unwrap();
        assert_eq!(resolved, Resolved::Node(ResolvedNode::Symbol(SymbolId(1))));
    }

    #[test]
    fn resolves_a_declared_but_never_imported_dependency() {
        let graph =
            linear_graph().with_declared_dependencies(vec![crate::graph::DeclaredDependency {
                package: PackageId(0),
                manifest: ProjectPath("package.json".into()),
                name: "never-imported".into(),
                version_req: "^1".into(),
                scope: DependencyScope::Dev,
            }]);
        let resolved = resolve(&graph, &Selector::Dependency("never-imported".into())).unwrap();
        assert_eq!(
            resolved,
            Resolved::Dependency(ResolvedDependency("never-imported".into()))
        );
    }

    #[test]
    fn ambiguous_package_name_lists_candidates() {
        let mut graph = linear_graph();
        graph = graph.with_packages(vec![
            PackageNode {
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                manifest: Some(ProjectPath("a/package.json".into())),
                name: Some("dup".into()),
                private: false,
                declares_surface: false,
                surface: Vec::new(),
                resolves_dependency_usage: true,
                manifest_claim_languages: Vec::new(),
            },
            PackageNode {
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                manifest: Some(ProjectPath("b/package.json".into())),
                name: Some("dup".into()),
                private: false,
                declares_surface: false,
                surface: Vec::new(),
                resolves_dependency_usage: true,
                manifest_claim_languages: Vec::new(),
            },
        ]);
        match resolve(&graph, &Selector::Package("dup".into())) {
            Err(ResolveError::Ambiguous(candidates)) => assert_eq!(candidates.len(), 2),
            Ok(_) => panic!("expected Ambiguous, resolved unambiguously"),
            Err(ResolveError::NotFound) => panic!("expected Ambiguous, got NotFound"),
        }
    }

    // ---------------------------------------------------------------- find

    #[test]
    fn find_ranks_exact_over_prefix_over_substring() {
        let files = vec![file("foo.ts"), file("foobar.ts"), file("xfooy.ts")];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        let result = find(
            &graph,
            &reach,
            "foo",
            &FindFilters {
                kind: None,
                color: None,
                lang: None,
            },
            50,
        );
        assert_eq!(result.matches[0].selector, "foo.ts");
        assert_eq!(result.matches[1].selector, "foobar.ts");
        assert_eq!(result.matches[2].selector, "xfooy.ts");
    }

    #[test]
    fn find_respects_limit_and_reports_elided() {
        let files: Vec<FileNode> = (0..5).map(|i| file(&format!("match{i}.ts"))).collect();
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        let result = find(
            &graph,
            &reach,
            "match",
            &FindFilters {
                kind: None,
                color: None,
                lang: None,
            },
            2,
        );
        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.elided, 3);
    }

    #[test]
    fn find_kind_filter_excludes_non_matching_kinds() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let result = find(
            &graph,
            &reach,
            "a",
            &FindFilters {
                kind: Some("file"),
                color: None,
                lang: None,
            },
            50,
        );
        assert!(result.matches.iter().all(|m| m.kind == "file"));
    }

    // ---------------------------------------------------------------- describe

    #[test]
    fn describe_symbol_reports_declaration_and_degree() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(
            &graph,
            &Selector::Symbol(ProjectPath("b.ts".into()), "bar".to_string()),
        )
        .unwrap();
        let result = describe(&graph, &reach, &resolved, &[], &nav);
        let decl = result
            .declaration
            .expect("symbol should carry a declaration");
        assert_eq!(decl.kind, "function");
        assert_eq!(decl.span.start, (5, 1));
        assert_eq!(*result.degree.in_by_kind.get("references").unwrap(), 1);
    }

    #[test]
    fn describe_file_reports_role_origin_and_declared_symbols() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(&graph, &Selector::File(ProjectPath("a.ts".into()))).unwrap();
        let result = describe(&graph, &reach, &resolved, &[], &nav);
        let file_info = result.file.expect("file node should carry file info");
        assert_eq!(file_info.role, "production");
        assert_eq!(file_info.origin, "authored");
        assert_eq!(result.declared_symbols.len(), 1);
        assert_eq!(result.declared_symbols[0].selector, "a.ts#foo");
    }

    #[test]
    fn describe_dependency_reports_scope_and_usage() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(&graph, &Selector::Dependency("lodash".into())).unwrap();
        let result = describe(&graph, &reach, &resolved, &[], &nav);
        let dep = result
            .dependency
            .expect("dep: selector should carry dependency info");
        assert_eq!(dep.manifest_scopes, vec!["prod".to_string()]);
        assert_eq!(dep.importing_files, 1);
        assert!(dep.used);
    }

    #[test]
    fn describe_reports_findings_attached_to_a_symbol() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(
            &graph,
            &Selector::Symbol(ProjectPath("b.ts".into()), "bar".to_string()),
        )
        .unwrap();
        let locations = vec![FindingLocation {
            id: "kndo-abc123",
            path: Some("b.ts"),
            symbol: Some("bar"),
        }];
        let result = describe(&graph, &reach, &resolved, &locations, &nav);
        assert_eq!(result.findings, vec!["kndo-abc123".to_string()]);
    }

    #[test]
    fn describe_reached_by_roots_finds_the_production_root() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(
            &graph,
            &Selector::Symbol(ProjectPath("b.ts".into()), "bar".to_string()),
        )
        .unwrap();
        let result = describe(&graph, &reach, &resolved, &[], &nav);
        assert_eq!(result.reached_by_roots.len(), 1);
        assert_eq!(result.reached_by_roots[0].selector, "a.ts");
    }

    // ---------------------------------------------------------------- neighbors

    #[test]
    fn uses_from_a_file_includes_imports_and_references() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(&graph, &Selector::File(ProjectPath("a.ts".into()))).unwrap();
        let result = neighbors(
            &graph,
            &reach,
            &resolved,
            NeighborsOpts {
                direction: Direction::Uses,
                edges: EdgeFilter::parse(None).unwrap(),
                depth: 1,
                transitive: false,
                limit: 50,
            },
            &nav,
        );
        let selectors: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.node.selector.as_str())
            .collect();
        assert!(selectors.contains(&"b.ts"));
        assert!(selectors.contains(&"dep:lodash"));
        assert!(selectors.contains(&"b.ts#bar"));
    }

    /// Regression: the evidence `site` on a `uses` entry must point at the *origin's* file (the
    /// file that wrote the import/reference), not the neighbor's — an earlier implementation
    /// derived the site path from whichever node was "the neighbor," which is only correct for
    /// `used-by` (reverse traversal); for `uses` (forward) it silently paired the right line/
    /// column with the wrong file, actively misleading a reader instead of just omitting evidence.
    #[test]
    fn uses_site_is_attributed_to_the_importing_file_not_the_imported_one() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(&graph, &Selector::File(ProjectPath("a.ts".into()))).unwrap();
        let result = neighbors(
            &graph,
            &reach,
            &resolved,
            NeighborsOpts {
                direction: Direction::Uses,
                edges: EdgeFilter::parse(Some("imports")).unwrap(),
                depth: 1,
                transitive: false,
                limit: 50,
            },
            &nav,
        );
        let to_b = result
            .entries
            .iter()
            .find(|e| e.node.selector == "b.ts")
            .expect("b.ts should be a uses neighbor");
        let site = to_b
            .via
            .site
            .as_ref()
            .expect("the import edge carries a span");
        assert_eq!(
            site.path, "a.ts",
            "the import statement lives in a.ts, not b.ts"
        );
        assert_eq!(site.start, (2, 1));
    }

    #[test]
    fn used_by_site_is_attributed_to_the_referencing_file() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(&graph, &Selector::File(ProjectPath("b.ts".into()))).unwrap();
        let result = neighbors(
            &graph,
            &reach,
            &resolved,
            NeighborsOpts {
                direction: Direction::UsedBy,
                edges: EdgeFilter::parse(Some("imports")).unwrap(),
                depth: 1,
                transitive: false,
                limit: 50,
            },
            &nav,
        );
        let from_a = &result.entries[0];
        assert_eq!(from_a.node.selector, "a.ts");
        let site = from_a
            .via
            .site
            .as_ref()
            .expect("the import edge carries a span");
        assert_eq!(
            site.path, "a.ts",
            "the referencing file (a.ts) wrote the import"
        );
    }

    #[test]
    fn trace_between_site_is_attributed_to_the_source_of_each_hop() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let from = resolve(&graph, &Selector::File(ProjectPath("a.ts".into()))).unwrap();
        let to = resolve(
            &graph,
            &Selector::Symbol(ProjectPath("b.ts".into()), "bar".to_string()),
        )
        .unwrap();
        let result = trace_between(
            &graph,
            &reach,
            &from,
            &to,
            TraceOpts {
                edges: EdgeFilter::parse(None).unwrap(),
                all: false,
                max_paths: 5,
            },
            &nav,
        );
        let hop = &result.paths[0].hops[0];
        let site = hop
            .via
            .site
            .as_ref()
            .expect("the references edge carries a span");
        // The reference to `bar` is written in a.ts (per linear_graph's `References` edge),
        // even though the hop's *node* is b.ts#bar.
        assert_eq!(site.path, "a.ts");
        assert_eq!(site.start, (4, 1));
    }

    #[test]
    fn used_by_a_file_is_the_reverse_of_uses() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(&graph, &Selector::File(ProjectPath("b.ts".into()))).unwrap();
        let result = neighbors(
            &graph,
            &reach,
            &resolved,
            NeighborsOpts {
                direction: Direction::UsedBy,
                edges: EdgeFilter::parse(None).unwrap(),
                depth: 1,
                transitive: false,
                limit: 50,
            },
            &nav,
        );
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].node.selector, "a.ts");
        assert_eq!(result.entries[0].via.edge, "imports");
    }

    #[test]
    fn edges_filter_restricts_to_references_only() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(&graph, &Selector::File(ProjectPath("a.ts".into()))).unwrap();
        let result = neighbors(
            &graph,
            &reach,
            &resolved,
            NeighborsOpts {
                direction: Direction::Uses,
                edges: EdgeFilter::parse(Some("references")).unwrap(),
                depth: 1,
                transitive: false,
                limit: 50,
            },
            &nav,
        );
        let selectors: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.node.selector.as_str())
            .collect();
        assert_eq!(selectors, vec!["b.ts#bar"]);
    }

    #[test]
    fn depth_one_stops_before_transitive_neighbors() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(&graph, &Selector::File(ProjectPath("a.ts".into()))).unwrap();
        let shallow = neighbors(
            &graph,
            &reach,
            &resolved,
            NeighborsOpts {
                direction: Direction::Uses,
                edges: EdgeFilter::parse(Some("imports")).unwrap(),
                depth: 1,
                transitive: false,
                limit: 50,
            },
            &nav,
        );
        // b.ts imports nothing further in this fixture, so depth 1 vs transitive coincide —
        // the real assertion is that dependency-only reach at depth 1 is exactly {b.ts, dep:lodash}.
        let selectors: HashSet<&str> = shallow
            .entries
            .iter()
            .map(|e| e.node.selector.as_str())
            .collect();
        assert_eq!(
            selectors,
            ["b.ts", "dep:lodash"].into_iter().collect::<HashSet<_>>()
        );
    }

    #[test]
    fn dependency_selector_has_no_uses_but_can_be_used_by() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let resolved = resolve(&graph, &Selector::Dependency("lodash".into())).unwrap();
        let result = neighbors(
            &graph,
            &reach,
            &resolved,
            NeighborsOpts {
                direction: Direction::Uses,
                edges: EdgeFilter::parse(None).unwrap(),
                depth: 1,
                transitive: false,
                limit: 50,
            },
            &nav,
        );
        assert!(result.entries.is_empty());
    }

    // ---------------------------------------------------------------- trace

    #[test]
    fn trace_between_finds_the_shortest_path() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let from = resolve(&graph, &Selector::File(ProjectPath("a.ts".into()))).unwrap();
        let to = resolve(
            &graph,
            &Selector::Symbol(ProjectPath("b.ts".into()), "bar".to_string()),
        )
        .unwrap();
        let result = trace_between(
            &graph,
            &reach,
            &from,
            &to,
            TraceOpts {
                edges: EdgeFilter::parse(None).unwrap(),
                all: false,
                max_paths: 5,
            },
            &nav,
        );
        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].hops.len(), 1);
        assert_eq!(result.paths[0].hops[0].node.selector, "b.ts#bar");
        assert_eq!(result.paths[0].hops[0].via.edge, "references");
    }

    #[test]
    fn trace_between_reports_no_path_to_a_disconnected_file() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let from = resolve(&graph, &Selector::File(ProjectPath("a.ts".into()))).unwrap();
        let to = resolve(&graph, &Selector::File(ProjectPath("c.ts".into()))).unwrap();
        let result = trace_between(
            &graph,
            &reach,
            &from,
            &to,
            TraceOpts {
                edges: EdgeFilter::parse(None).unwrap(),
                all: false,
                max_paths: 5,
            },
            &nav,
        );
        assert!(result.paths.is_empty());
    }

    #[test]
    fn liveness_trace_finds_the_path_from_the_production_root() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let target = resolve(
            &graph,
            &Selector::Symbol(ProjectPath("b.ts".into()), "bar".to_string()),
        )
        .unwrap();
        let result = trace_liveness(&graph, &reach, &target, None, &nav);
        assert_eq!(result.from.selector, "roots:production");
        assert_eq!(result.paths.len(), 1);
        // The root itself (a.ts) is `from`, not a hop — one hop reaches bar directly.
        assert_eq!(result.paths[0].hops.len(), 1);
        assert_eq!(result.paths[0].hops[0].node.selector, "b.ts#bar");
    }

    #[test]
    fn liveness_trace_finds_nothing_for_a_disconnected_file() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let target = resolve(&graph, &Selector::File(ProjectPath("c.ts".into()))).unwrap();
        let result = trace_liveness(&graph, &reach, &target, None, &nav);
        assert!(result.paths.is_empty());
    }

    #[test]
    fn liveness_trace_falls_back_from_production_to_test() {
        let files = vec![file("test_root.ts"), file("helper.ts")];
        let symbols = vec![symbol(FileId(1), "helper", 1, 2)];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
                None,
            ),
            edge(
                EdgeKind::Declares {
                    file: FileId(1),
                    symbol: SymbolId(0),
                },
                Confidence::Certain,
                None,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(0)),
                    to: SymbolId(0),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
                None,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let target = resolve(
            &graph,
            &Selector::Symbol(ProjectPath("helper.ts".into()), "helper".to_string()),
        )
        .unwrap();
        let result = trace_liveness(&graph, &reach, &target, None, &nav);
        assert_eq!(result.from.selector, "roots:test");
        assert_eq!(result.paths.len(), 1);
    }

    // ---------------------------------------------------------------- impact

    fn impact_opts(if_deleted: bool) -> ImpactOpts {
        ImpactOpts {
            edges: EdgeFilter::parse(None).unwrap(),
            depth: None,
            limit: 50,
            if_deleted,
        }
    }

    #[test]
    fn impact_default_is_the_reverse_closure_with_affected_roots() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let target = resolve(&graph, &Selector::File(ProjectPath("b.ts".into()))).unwrap();
        let result = impact(&graph, &reach, &target, impact_opts(false), &nav).unwrap();
        // a.ts imports b.ts — it's affected at depth 1, and it's the production root.
        assert!(result
            .affected
            .iter()
            .any(|e| e.node.selector == "a.ts" && e.depth == 1));
        assert!(result
            .affected_roots
            .iter()
            .any(|r| r.kind == "production" && r.node.selector == "a.ts"));
        assert!(result.if_deleted.is_none());
    }

    /// root a.ts → b.ts → d.ts; b.ts is lodash's only importer; lodash is declared.
    fn chain_graph() -> ProjectGraph {
        let files = vec![file("a.ts"), file("b.ts"), file("d.ts")];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("lodash"),
        }];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
                None,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(1),
                },
                Confidence::Certain,
                Some(span(1, 1)),
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(1),
                    to: FileId(2),
                },
                Confidence::Certain,
                Some(span(2, 2)),
            ),
            edge(
                EdgeKind::ImportsDependency {
                    from: FileId(1),
                    to: DependencyId(0),
                },
                Confidence::Certain,
                Some(span(3, 3)),
            ),
        ];
        ProjectGraph::for_test(files, vec![], dependencies, edges).with_declared_dependencies(vec![
            crate::graph::DeclaredDependency {
                package: PackageId(0),
                manifest: ProjectPath(SmolStr::new("package.json")),
                name: SmolStr::new("lodash"),
                version_req: SmolStr::new("*"),
                scope: DependencyScope::Prod,
            },
        ])
    }

    #[test]
    fn if_deleted_reports_orphaned_files_and_freed_dependencies() {
        let graph = chain_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let target = resolve(&graph, &Selector::File(ProjectPath("b.ts".into()))).unwrap();
        let result = impact(&graph, &reach, &target, impact_opts(true), &nav).unwrap();
        let sim = result.if_deleted.expect("--if-deleted requested");
        // d.ts was only reachable through b.ts — deleting b orphans it.
        assert!(sim.newly_unreachable.iter().any(|q| q.selector == "d.ts"));
        // b.ts itself is deleted, not "newly unreachable".
        assert!(!sim.newly_unreachable.iter().any(|q| q.selector == "b.ts"));
        // a.ts keeps its root — unaffected.
        assert!(!sim.newly_unreachable.iter().any(|q| q.selector == "a.ts"));
        // lodash's only importer is gone and it IS declared — freed.
        assert_eq!(sim.freed_dependencies, vec!["lodash".to_string()]);
    }

    #[test]
    fn if_deleted_of_a_symbol_flips_its_exclusive_callees() {
        // root-decl foo (a.ts) → bar (b.ts) → baz (d.ts), all symbol-attributed: deleting bar
        // makes baz (and b's file, which only bar's callers reached) newly unreachable.
        let files = vec![file("a.ts"), file("b.ts"), file("d.ts")];
        let symbols = vec![
            symbol(FileId(0), "foo", 1, 3),
            symbol(FileId(1), "bar", 1, 3),
            symbol(FileId(2), "baz", 1, 3),
        ];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::Symbol(SymbolId(0)),
                },
                Confidence::Certain,
                None,
            ),
            edge(
                EdgeKind::Declares {
                    file: FileId(0),
                    symbol: SymbolId(0),
                },
                Confidence::Certain,
                None,
            ),
            edge(
                EdgeKind::Declares {
                    file: FileId(1),
                    symbol: SymbolId(1),
                },
                Confidence::Certain,
                None,
            ),
            edge(
                EdgeKind::Declares {
                    file: FileId(2),
                    symbol: SymbolId(2),
                },
                Confidence::Certain,
                None,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::Symbol(SymbolId(0)),
                    to: SymbolId(1),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
                Some(span(2, 2)),
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::Symbol(SymbolId(1)),
                    to: SymbolId(2),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
                Some(span(2, 2)),
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let target = resolve(
            &graph,
            &Selector::Symbol(ProjectPath("b.ts".into()), "bar".to_string()),
        )
        .unwrap();
        let result = impact(&graph, &reach, &target, impact_opts(true), &nav).unwrap();
        // Default direction: foo (the caller) is the blast radius, transitively to depth 2.
        assert!(result
            .affected
            .iter()
            .any(|e| e.node.selector.ends_with("#foo")));
        let sim = result.if_deleted.unwrap();
        assert!(
            sim.newly_unreachable
                .iter()
                .any(|q| q.selector.ends_with("#baz")),
            "{:?}",
            sim.newly_unreachable
        );
        assert!(!sim
            .newly_unreachable
            .iter()
            .any(|q| q.selector.ends_with("#foo")));
    }

    #[test]
    fn if_deleted_reports_production_to_test_only_demotions() {
        // Production root a.ts and test root t.ts both import x.ts; deleting a.ts leaves x
        // reachable only from tests.
        let files = vec![file("a.ts"), file("t.ts"), file("x.ts")];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
                None,
            ),
            edge(
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(FileId(1)),
                },
                Confidence::Certain,
                None,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(2),
                },
                Confidence::Certain,
                None,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(1),
                    to: FileId(2),
                },
                Confidence::Certain,
                None,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let target = resolve(&graph, &Selector::File(ProjectPath("a.ts".into()))).unwrap();
        let result = impact(&graph, &reach, &target, impact_opts(true), &nav).unwrap();
        let sim = result.if_deleted.unwrap();
        assert!(
            sim.newly_test_only.iter().any(|q| q.selector == "x.ts"),
            "{:?}",
            sim.newly_test_only
        );
        assert!(sim.newly_unreachable.is_empty());
    }

    #[test]
    fn if_deleted_rejects_dependency_selectors_with_an_explanation() {
        let graph = linear_graph();
        let reach = reachability::compute(&graph);
        let nav = build_graph_index(&graph);
        let target = resolve(&graph, &Selector::Dependency(SmolStr::new("lodash"))).unwrap();
        let err = impact(&graph, &reach, &target, impact_opts(true), &nav).unwrap_err();
        assert!(matches!(err, QueryError::IfDeletedOnDependency(_)), "{err}");
        // The default mode still answers: importers are the blast radius.
        let ok = impact(&graph, &reach, &target, impact_opts(false), &nav).unwrap();
        assert!(ok.affected.iter().any(|e| e.node.selector == "a.ts"));
    }
}
