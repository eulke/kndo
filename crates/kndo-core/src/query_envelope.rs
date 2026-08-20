//! Query envelopes (contracts/output-schema.md §8, RFC 0007 §4.7) — request/response shapes for
//! `Engine::query`, batching, and rendering. The verb algorithms themselves live in
//! [`crate::query`]; this module is the request-dispatch and JSON-shape layer around them,
//! mirroring how `engine.rs` is the envelope layer around `analysis::run_all`.
//!
//! Every navigation verb is batched by construction (RFC 0007 §4.7 tenet 1): `selectors` always
//! carries one-or-more entries and `results` aligns 1:1 with it in argument order, except
//! `trace`'s pair-batched form (`flags.pairs`), which aligns with `pairs` instead — the one case
//! output-schema §8 itself calls out as different.

use crate::adapter::{Diagnostic, DiagnosticLevel};
use crate::analysis::reachability::{self, ReachabilityMap};
use crate::engine::Finding;
use crate::graph::ProjectGraph;
use crate::query::{
    self, Direction, EdgeFilter, FindFilters, FindingLocation, NeighborsOpts, ResolveError,
    Resolved, Selector,
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
}

impl Verb {
    pub fn as_str(self) -> &'static str {
        match self {
            Verb::Find => "find",
            Verb::Describe => "describe",
            Verb::Uses => "uses",
            Verb::UsedBy => "used-by",
            Verb::Trace => "trace",
        }
    }

    pub fn parse(raw: &str) -> Option<Verb> {
        match raw {
            "find" => Some(Verb::Find),
            "describe" => Some(Verb::Describe),
            "uses" => Some(Verb::Uses),
            "used-by" => Some(Verb::UsedBy),
            "trace" => Some(Verb::Trace),
            _ => None,
        }
    }
}

/// Every verb-specific `--flag` in one place (RFC 0007 §4) — irrelevant flags for a given verb
/// are simply ignored rather than rejected, so a `kndo query` request can carry a superset
/// without per-verb validation ceremony.
#[derive(Debug, Clone, Default)]
pub struct QueryFlags {
    pub kind: Option<String>,
    pub color: Option<String>,
    pub lang: Option<String>,
    pub depth: Option<u32>,
    pub transitive: bool,
    pub edges: Option<String>,
    pub all: bool,
    pub max_paths: Option<usize>,
    pub roots: Option<String>,
    /// `trace`'s batched form (RFC 0007 §4.7's own example): independent directed traces, one
    /// per pair, `results` aligning with this list instead of `selectors` when non-empty.
    pub pairs: Vec<(String, String)>,
    pub limit: Option<usize>,
}

pub struct QueryRequest {
    /// Query-mode echo (RFC 0007 §4.7) — `None` outside `kndo query`.
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
/// failure (output-schema §8: "a failed selector yields an inline `{status, …}` entry without
/// failing its siblings").
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum ResultEntry {
    Find(query::FindResult),
    Describe(Box<query::DescribeResult>),
    Neighbors(query::NeighborsResult),
    Trace(query::TraceResult),
    Failed {
        status: &'static str,
        selector: String,
        message: String,
    },
}

impl ResultEntry {
    fn status(&self) -> Status {
        match self {
            ResultEntry::Failed { status, .. } if *status == "not-found" => Status::NotFound,
            ResultEntry::Failed { .. } => Status::Error,
            _ => Status::Ok,
        }
    }
}

/// The typed form of the query envelope (output-schema §8) — [`Self::to_json_line`] nests it
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
    /// Worst status across every result entry (RFC 0007 §6: "the process exit code is the worst
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

/// The full `--format json` query envelope (output-schema §8) — also `cargo xtask gen-schema`'s
/// second root type. `run` is `Option` only so [`QueryResult::to_json_line`] can omit it on
/// every `kndo query` line after the first (§4.7: "run appearing only on the first line").
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

    /// One `kndo query` JSONL line (RFC 0007 §4.7): compact (no pretty-printing — JSONL is
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
/// process regardless of how many requests are answered (RFC 0007 §4.7 tenet 1).
pub(crate) fn run(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    finding_locations: &[FindingLocation<'_>],
    req: QueryRequest,
    cache: &'static str,
    duration_ms: u64,
) -> QueryResult {
    let limit = req.flags.limit.unwrap_or(DEFAULT_LIMIT);
    let results = match req.verb {
        Verb::Find => find_entries(graph, reach, &req.selectors, &req.flags, limit),
        Verb::Describe => describe_entries(graph, reach, finding_locations, &req.selectors),
        Verb::Uses => neighbor_entries(
            graph,
            reach,
            &req.selectors,
            &req.flags,
            Direction::Uses,
            limit,
        ),
        Verb::UsedBy => neighbor_entries(
            graph,
            reach,
            &req.selectors,
            &req.flags,
            Direction::UsedBy,
            limit,
        ),
        Verb::Trace => trace_entries(graph, reach, &req.selectors, &req.flags),
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

/// A small, `Copy`-cheap error carrying exactly what an inline `results[]` failure needs
/// (output-schema §8) — kept separate from [`ResultEntry`] itself so `resolve_selector`'s `Err`
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
    query::parse_selector(raw).map_err(|message| QueryFailure {
        status: "error",
        selector: raw.to_string(),
        message,
    })
}

fn describe_entries(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    finding_locations: &[FindingLocation<'_>],
    selectors: &[String],
) -> Vec<ResultEntry> {
    selectors
        .iter()
        .map(|raw| match resolve_selector(graph, raw) {
            Ok(resolved) => ResultEntry::Describe(Box::new(query::describe(
                graph,
                reach,
                &resolved,
                finding_locations,
            ))),
            Err(failed) => failed.into(),
        })
        .collect()
}

fn neighbor_entries(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    selectors: &[String],
    flags: &QueryFlags,
    direction: Direction,
    limit: usize,
) -> Vec<ResultEntry> {
    let edges = match EdgeFilter::parse(flags.edges.as_deref()) {
        Ok(e) => e,
        Err(message) => {
            return selectors
                .iter()
                .map(|s| ResultEntry::Failed {
                    status: "error",
                    selector: s.clone(),
                    message: message.clone(),
                })
                .collect()
        }
    };
    selectors
        .iter()
        .map(|raw| match resolve_selector(graph, raw) {
            Ok(resolved) => ResultEntry::Neighbors(query::neighbors(
                graph,
                reach,
                &resolved,
                NeighborsOpts {
                    direction,
                    edges,
                    depth: flags.depth.unwrap_or(1),
                    transitive: flags.transitive,
                    limit,
                },
            )),
            Err(failed) => failed.into(),
        })
        .collect()
}

fn trace_entries(
    graph: &ProjectGraph,
    reach: &ReachabilityMap,
    selectors: &[String],
    flags: &QueryFlags,
) -> Vec<ResultEntry> {
    let edges = match EdgeFilter::parse(None) {
        Ok(e) => e,
        Err(_) => unreachable!("EdgeFilter::parse(None) always succeeds"),
    };
    let max_paths = flags.max_paths.unwrap_or(DEFAULT_MAX_PATHS);

    if !flags.pairs.is_empty() {
        return flags
            .pairs
            .iter()
            .map(|(a, b)| trace_pair(graph, reach, a, b, edges, flags.all, max_paths))
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
                    graph, reach, &target, roots_kind,
                ))],
                Err(failed) => vec![failed.into()],
            }
        }
        2 => vec![trace_pair(
            graph,
            reach,
            &selectors[0],
            &selectors[1],
            edges,
            flags.all,
            max_paths,
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
    from_raw: &str,
    to_raw: &str,
    edges: EdgeFilter,
    all: bool,
    max_paths: usize,
) -> ResultEntry {
    let from = match resolve_selector(graph, from_raw) {
        Ok(r) => r,
        Err(failed) => return failed.into(),
    };
    let to = match resolve_selector(graph, to_raw) {
        Ok(r) => r,
        Err(failed) => return failed.into(),
    };
    ResultEntry::Trace(query::trace_between(
        graph, reach, &from, &to, edges, all, max_paths,
    ))
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
        })
        .collect()
}

pub(crate) fn compute_reachability(graph: &ProjectGraph) -> ReachabilityMap {
    reachability::compute(graph)
}

/// A cold-build failure (no cache, project root broken) degrades to a single `error` result
/// entry plus a diagnostic — never a panic (same contract as `check`'s `RunMode` dispatch).
pub(crate) fn build_failure(req: QueryRequest, message: String) -> QueryResult {
    QueryResult {
        verb: req.verb,
        selectors: req.selectors.clone(),
        id: req.id,
        cache: "cold",
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
