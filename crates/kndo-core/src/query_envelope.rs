//! Query envelopes — request/response shapes for
//! `Engine::query`, batching, and rendering. The verb algorithms themselves live in
//! [`crate::query`]; this module is the request-dispatch and JSON-shape layer around them,
//! mirroring how `engine.rs` is the envelope layer around `analysis::run_all`.
//!
//! Every navigation verb is batched by construction: `selectors` always
//! carries one-or-more entries and `results` aligns 1:1 with it in argument order, except
//! `trace`'s pair-batched form (`flags.pairs`), which aligns with `pairs` instead — the one
//! deliberately different case.

use crate::adapter::{Diagnostic, DiagnosticLevel};
use crate::analysis::reachability::{self, ReachabilityMap};
use crate::engine::Finding;
use crate::graph::ProjectGraph;
use crate::query::{
    self, Direction, EdgeFilter, FindFilters, FindingLocation, ImpactOpts, NeighborsOpts,
    ResolveError, Resolved, Selector,
};
use crate::vocab::RootKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Verb {
    Find,
    Describe,
    Uses,
    UsedBy,
    Trace,
    Impact,
}

impl Verb {
    pub fn as_str(self) -> &'static str {
        match self {
            Verb::Find => "find",
            Verb::Describe => "describe",
            Verb::Uses => "uses",
            Verb::UsedBy => "used-by",
            Verb::Trace => "trace",
            Verb::Impact => "impact",
        }
    }

    pub fn parse(raw: &str) -> Option<Verb> {
        match raw {
            "find" => Some(Verb::Find),
            "describe" => Some(Verb::Describe),
            "uses" => Some(Verb::Uses),
            "used-by" => Some(Verb::UsedBy),
            "trace" => Some(Verb::Trace),
            "impact" => Some(Verb::Impact),
            _ => None,
        }
    }
}

/// Every verb-specific `--flag` in one place — irrelevant flags for a given verb
/// are simply ignored rather than rejected, so a `kndo query` request can carry a superset
/// without per-verb validation ceremony. `Deserialize` doubles as `kndo query`'s JSONL
/// per-line flags shape — a frontend parsing a `flags` object from stdin deserializes straight
/// into this type instead of maintaining a mirrored struct + a field-by-field `From` impl.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct QueryFlags {
    pub kind: Option<String>,
    pub color: Option<String>,
    pub lang: Option<String>,
    pub depth: Option<u32>,
    #[serde(default)]
    pub transitive: bool,
    pub edges: Option<String>,
    #[serde(default)]
    pub all: bool,
    pub max_paths: Option<usize>,
    pub roots: Option<String>,
    /// `trace`'s batched form (the own example): independent directed traces, one
    /// per pair, `results` aligning with this list instead of `selectors` when non-empty.
    #[serde(default)]
    pub pairs: Vec<(String, String)>,
    pub limit: Option<usize>,
    /// `impact --if-deleted`: simulate removal, report the finding flips.
    #[serde(default)]
    pub if_deleted: bool,
}

pub struct QueryRequest {
    /// Query-mode echo — `None` outside `kndo query`.
    pub id: Option<String>,
    pub verb: Verb,
    /// Patterns (`find`) or selectors (every other verb) — see module docs for `trace`'s
    /// exception.
    pub selectors: Vec<String>,
    pub flags: QueryFlags,
}

const DEFAULT_LIMIT: usize = 50;
const DEFAULT_MAX_PATHS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
    Ok,
    NotFound,
    Error,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::NotFound => "not-found",
            Status::Error => "error",
        }
    }
}

/// One `results[]` entry — either a verb-specific success shape or an inline per-selector
/// failure ("a failed selector yields an inline `{status, …}` entry without
/// failing its siblings").
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum ResultEntry {
    Find(query::FindResult),
    Describe(Box<query::DescribeResult>),
    Neighbors(query::NeighborsResult),
    Trace(query::TraceResult),
    Impact(Box<query::ImpactResult>),
    Failed {
        status: &'static str,
        selector: String,
        message: String,
    },
}

impl ResultEntry {
    /// "1 | selector/path not found (`find` with zero hits, `trace` with no
    /// path)" — only these two verbs turn an empty-but-successful computation into `not-found`;
    /// `describe`/`uses`/`used-by` resolving to zero neighbors is a legitimate `ok` answer (e.g.
    /// `used-by` on a genuinely-unused symbol correctly returns nothing — that IS the answer,
    /// not a failure to compute one).
    fn status(&self) -> Status {
        match self {
            ResultEntry::Failed { status, .. } if *status == "not-found" => Status::NotFound,
            ResultEntry::Failed { .. } => Status::Error,
            ResultEntry::Find(r) if r.matches.is_empty() => Status::NotFound,
            ResultEntry::Trace(r) if r.paths.is_empty() => Status::NotFound,
            _ => Status::Ok,
        }
    }
}

/// The typed form of the query envelope — [`Self::to_json_line`] nests it
/// into the schema's actual shape, the same split `RunResult`/`Envelope` use.
pub struct QueryResult {
    pub verb: Verb,
    pub selectors: Vec<String>,
    pub id: Option<String>,
    pub cache: &'static str,
    pub duration_ms: u64,
    pub results: Vec<ResultEntry>,
    pub diagnostics: Vec<Diagnostic>,
}

impl QueryResult {
    /// Worst status across every result entry ("the process exit code is the worst
    /// individual status … so single-question scripting semantics survive batching unchanged").
    pub fn status(&self) -> &'static str {
        self.results
            .iter()
            .map(ResultEntry::status)
            .max()
            .unwrap_or(Status::Error) // zero results is itself a failure to answer anything
            .as_str()
    }
}

#[derive(serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct QueryEcho {
    verb: &'static str,
    selectors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct RunEcho {
    cache: &'static str,
    duration_ms: u64,
}

/// The full `--format json` query envelope — also `cargo xtask gen-schema`'s
/// second root type. `run` is `Option` only so [`QueryResult::to_json_line`] can omit it on
/// every `kndo query` line after the first ("run appearing only on the first line").
#[derive(serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QueryJsonEnvelope {
    schema_version: &'static str,
    kndo_version: &'static str,
    query: QueryEcho,
    #[serde(skip_serializing_if = "Option::is_none")]
    run: Option<RunEcho>,
    status: &'static str,
    results: Vec<ResultEntry>,
    diagnostics: Vec<Diagnostic>,
}

impl QueryResult {
    fn to_envelope(&self, include_run: bool) -> QueryJsonEnvelope {
        QueryJsonEnvelope {
            schema_version: crate::engine::SCHEMA_VERSION,
            kndo_version: crate::engine::KNDO_VERSION,
            query: QueryEcho {
                verb: self.verb.as_str(),
                selectors: self.selectors.clone(),
                id: self.id.clone(),
            },
            run: include_run.then_some(RunEcho {
                cache: self.cache,
                duration_ms: self.duration_ms,
            }),
            status: self.status(),
            results: self.results.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }

    /// The single-query `--format json` rendering — always includes `run`.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.to_envelope(true))
            .unwrap_or_else(|e| format!("{{\"error\": \"failed to serialize output: {e}\"}}"))
    }

    /// One `kndo query` JSONL line: compact (no pretty-printing — JSONL is
    /// one-object-per-line by construction), `run` included only when `include_run` is set
    /// (the caller passes `true` for exactly the first line of a batch).
    pub fn to_json_line(&self, include_run: bool) -> String {
        serde_json::to_string(&self.to_envelope(include_run))
            .unwrap_or_else(|e| format!("{{\"error\": \"failed to serialize output: {e}\"}}"))
    }

    pub fn to_agent_format(&self) -> String {
        crate::agent_format::render_query(self)
    }
}

#[cfg(feature = "schema")]
pub fn json_schema() -> schemars::Schema {
    schemars::schema_for!(QueryJsonEnvelope)
}

/// Resolves and dispatches one [`QueryRequest`] against an already-assembled graph — the shared
/// entry point `Engine::query` (single request) and `Engine::query_batch` (`kndo query`'s JSONL
/// loop, one shared graph load) both call, so cache revalidation happens exactly once per
/// process regardless of how many requests are answered (the batching tenet).
#[allow(clippy::too_many_arguments)] // one shared snapshot's worth of borrows, all read-only
pub(crate) fn run(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    nav: &query::GraphIndex,
    finding_locations: &[FindingLocation<'_>],
    coverage: &crate::coverage::CoverageMap,
    req: QueryRequest,
    cache: &'static str,
    duration_ms: u64,
) -> QueryResult {
    let limit = req.flags.limit.unwrap_or(DEFAULT_LIMIT);
    let results = match req.verb {
        Verb::Find => find_entries(graph, reach, &req.selectors, &req.flags, limit),
        Verb::Describe => describe_entries(
            graph,
            reach,
            nav,
            finding_locations,
            coverage,
            &req.selectors,
        ),
        Verb::Uses => neighbor_entries(
            graph,
            reach,
            nav,
            &req.selectors,
            &req.flags,
            Direction::Uses,
            limit,
        ),
        Verb::UsedBy => neighbor_entries(
            graph,
            reach,
            nav,
            &req.selectors,
            &req.flags,
            Direction::UsedBy,
            limit,
        ),
        Verb::Trace => trace_entries(graph, reach, nav, &req.selectors, &req.flags),
        Verb::Impact => impact_entries(graph, reach, nav, &req.selectors, &req.flags, limit),
    };
    QueryResult {
        verb: req.verb,
        selectors: req.selectors,
        id: req.id,
        cache,
        duration_ms,
        results,
        diagnostics: Vec::new(),
    }
}

fn find_entries(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    patterns: &[String],
    flags: &QueryFlags,
    limit: usize,
) -> Vec<ResultEntry> {
    let filters = FindFilters {
        kind: flags.kind.as_deref(),
        color: flags.color.as_deref(),
        lang: flags.lang.as_deref(),
    };
    patterns
        .iter()
        .map(|p| ResultEntry::Find(query::find(graph, reach, p, &filters, limit)))
        .collect()
}

/// A small, `Copy`-cheap error carrying exactly what an inline `results[]` failure
/// needs — kept separate from [`ResultEntry`] itself so `resolve_selector`'s `Err`
/// stays small (clippy's `result_large_err`: `ResultEntry`'s successful variants, especially
/// `Trace`, are hundreds of bytes).
struct QueryFailure {
    status: &'static str,
    selector: String,
    message: String,
}

impl From<QueryFailure> for ResultEntry {
    fn from(f: QueryFailure) -> Self {
        ResultEntry::Failed {
            status: f.status,
            selector: f.selector,
            message: f.message,
        }
    }
}

fn resolve_selector(graph: &ProjectGraph, raw: &str) -> Result<Resolved, QueryFailure> {
    let selector = parse_selector_entry(raw)?;
    query::resolve(graph, &selector).map_err(|e| match e {
        ResolveError::NotFound => QueryFailure {
            status: "not-found",
            selector: raw.to_string(),
            message: format!("no node matches selector `{raw}`"),
        },
        ResolveError::Ambiguous(candidates) => QueryFailure {
            status: "error",
            selector: raw.to_string(),
            message: format!(
                "ambiguous selector `{raw}` — candidates: {}",
                candidates.join(", ")
            ),
        },
    })
}

fn parse_selector_entry(raw: &str) -> Result<Selector, QueryFailure> {
    query::parse_selector(raw).map_err(|e| QueryFailure {
        status: "error",
        selector: raw.to_string(),
        message: e.to_string(),
    })
}

fn describe_entries(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    nav: &query::GraphIndex,
    finding_locations: &[FindingLocation<'_>],
    coverage: &crate::coverage::CoverageMap,
    selectors: &[String],
) -> Vec<ResultEntry> {
    resolved_entries(graph, selectors, |_, resolved| {
        ResultEntry::Describe(Box::new(query::describe(
            graph,
            reach,
            resolved,
            finding_locations,
            coverage,
            nav,
        )))
    })
}

/// One entry per selector, in argument order: resolve it and hand the resolution to `entry`,
/// or turn a resolution failure into that selector's own `Failed` entry. The envelope's
/// `status` is per-result, so one unresolvable selector never sinks the batch — a rule every
/// verb follows, and one each of them used to spell out.
fn resolved_entries(
    graph: &ProjectGraph,
    selectors: &[String],
    entry: impl Fn(&str, &query::Resolved) -> ResultEntry,
) -> Vec<ResultEntry> {
    selectors
        .iter()
        .map(|raw| match resolve_selector(graph, raw) {
            Ok(resolved) => entry(raw, &resolved),
            Err(failed) => failed.into(),
        })
        .collect()
}

/// Every selector in the batch failed the same way. A malformed `--edges` filter is an error
/// about the *request*, not about any one selector, so each entry carries the identical
/// message rather than the batch failing as a whole — the envelope's `status` is per-result.
fn failed_batch(selectors: &[String], message: &str) -> Vec<ResultEntry> {
    selectors
        .iter()
        .map(|s| ResultEntry::Failed {
            status: "error",
            selector: s.clone(),
            message: message.to_string(),
        })
        .collect()
}

fn neighbor_entries(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    nav: &query::GraphIndex,
    selectors: &[String],
    flags: &QueryFlags,
    direction: Direction,
    limit: usize,
) -> Vec<ResultEntry> {
    let edges = match EdgeFilter::parse(flags.edges.as_deref()) {
        Ok(e) => e,
        Err(err) => return failed_batch(selectors, &err.to_string()),
    };
    resolved_entries(graph, selectors, |_, resolved| {
        ResultEntry::Neighbors(query::neighbors(
            graph,
            reach,
            resolved,
            NeighborsOpts {
                direction,
                edges,
                depth: flags.depth.unwrap_or(1),
                transitive: flags.transitive,
                limit,
            },
            nav,
        ))
    })
}

// What `impact_entries` and `neighbor_entries` share is now `failed_batch` and
// `resolved_entries`. What is left is each verb's own options struct and result variant —
// plus `impact`'s extra failure arm, which no other verb has. Sharing further would mean one
// verb's entry builder knowing the other's options.
// kndo:allow duplicate the shared halves are failed_batch and resolved_entries
fn impact_entries(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    nav: &query::GraphIndex,
    selectors: &[String],
    flags: &QueryFlags,
    limit: usize,
) -> Vec<ResultEntry> {
    let edges = match EdgeFilter::parse(flags.edges.as_deref()) {
        Ok(e) => e,
        Err(err) => return failed_batch(selectors, &err.to_string()),
    };
    resolved_entries(graph, selectors, |raw, resolved| {
        // `impact` is the one verb that can refuse a selector it resolved fine — `--if-deleted`
        // on a dependency, `impact` on a root set — so it needs the raw text for its own
        // failure entry.
        match query::impact(
            graph,
            reach,
            resolved,
            ImpactOpts {
                edges,
                depth: flags.depth,
                limit,
                if_deleted: flags.if_deleted,
            },
            nav,
        ) {
            Ok(result) => ResultEntry::Impact(Box::new(result)),
            Err(err) => ResultEntry::Failed {
                status: "error",
                selector: raw.to_string(),
                message: err.to_string(),
            },
        }
    })
}

fn trace_entries(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    nav: &query::GraphIndex,
    selectors: &[String],
    flags: &QueryFlags,
) -> Vec<ResultEntry> {
    let edges = match EdgeFilter::parse(None) {
        Ok(e) => e,
        Err(_) => unreachable!("EdgeFilter::parse(None) always succeeds"),
    };
    let max_paths = flags.max_paths.unwrap_or(DEFAULT_MAX_PATHS);
    let opts = query::TraceOpts {
        edges,
        all: flags.all,
        max_paths,
    };

    if !flags.pairs.is_empty() {
        return flags
            .pairs
            .iter()
            .map(|(a, b)| trace_pair(graph, reach, nav, a, b, opts))
            .collect();
    }

    match selectors.len() {
        1 => {
            let roots_kind = match flags.roots.as_deref() {
                None | Some("production") => None, // None = try production, fall back
                Some("test") => Some(RootKind::Test),
                Some("tooling") => Some(RootKind::Tooling),
                Some(other) => {
                    return vec![ResultEntry::Failed {
                        status: "error",
                        selector: selectors[0].clone(),
                        message: format!("unknown --roots `{other}` (production, test, tooling)"),
                    }]
                }
            };
            match resolve_selector(graph, &selectors[0]) {
                Ok(target) => vec![ResultEntry::Trace(query::trace_liveness(
                    graph, reach, &target, roots_kind, nav,
                ))],
                Err(failed) => vec![failed.into()],
            }
        }
        2 => vec![trace_pair(
            graph,
            reach,
            nav,
            &selectors[0],
            &selectors[1],
            opts,
        )],
        n => vec![ResultEntry::Failed {
            status: "error",
            selector: selectors.join(", "),
            message: format!("trace takes 1 (liveness) or 2 (directed) selectors, got {n}"),
        }],
    }
}

fn trace_pair(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    nav: &query::GraphIndex,
    from_raw: &str,
    to_raw: &str,
    opts: query::TraceOpts,
) -> ResultEntry {
    let from = match resolve_selector(graph, from_raw) {
        Ok(r) => r,
        Err(failed) => return failed.into(),
    };
    let to = match resolve_selector(graph, to_raw) {
        Ok(r) => r,
        Err(failed) => return failed.into(),
    };
    ResultEntry::Trace(query::trace_between(graph, reach, &from, &to, opts, nav))
}

/// Builds the `describe`-time finding-attachment view from a plain finding list — always the
/// *current*, suppression-applied set (matching what `check` would show), never baseline-
/// filtered (an agent asking "what's attached here" wants the full current picture, baseline
/// acknowledgment or not).
pub(crate) fn finding_locations(findings: &[Finding]) -> Vec<FindingLocation<'_>> {
    findings
        .iter()
        .map(|f| FindingLocation {
            id: f.id.as_str(),
            path: f.location.path.as_ref().map(|p| p.0.as_str()),
            symbol: f.location.symbol.as_deref(),
            category: f.category.as_str(),
            related: &f.related,
        })
        .collect()
}

/// The navigation verbs' reachability — seeded with the same project-declared entry points
/// `check` uses, so `kndo used-by` and `kndo check` can never disagree about a symbol's color
/// (one source per concept: the rules live in config, the derivation in `reachability`).
pub(crate) fn compute_reachability(
    graph: &ProjectGraph,
    rules: &[crate::config::ExternallyInvokedRule],
) -> ReachabilityMap {
    let declared = reachability::externally_invoked_symbols(graph, rules);
    reachability::compute_with_roots(graph, &declared)
}

/// A build failure (project root broken, unreadable tree) degrades to a single `error` result
/// entry plus a diagnostic — never a panic (same contract as `check`'s `RunMode` dispatch).
///
/// `cache` is passed in rather than assumed: the failure says nothing about whether the cache
/// was consulted, and hardcoding `"cold"` here would report an empty cache to a caller who had
/// switched the cache off — the same conflation `cache_status_str` exists to prevent.
pub(crate) fn build_failure(
    req: QueryRequest,
    message: String,
    cache: &'static str,
) -> QueryResult {
    QueryResult {
        verb: req.verb,
        selectors: req.selectors.clone(),
        id: req.id,
        cache,
        duration_ms: 0,
        results: req
            .selectors
            .iter()
            .map(|s| ResultEntry::Failed {
                status: "error",
                selector: s.clone(),
                message: message.clone(),
            })
            .collect(),
        diagnostics: vec![Diagnostic {
            level: DiagnosticLevel::Warn,
            path: None,
            message,
            span: None,
        }],
    }
}
