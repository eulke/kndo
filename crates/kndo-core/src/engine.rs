//! The `Engine` facade — the only surface frontends may touch.
//!
//! Separation rules, enforced by dependency direction: the core contains no terminal concerns
//! (no ANSI, no TTY detection, no exit codes, no stdout) — it returns data and never prints;
//! frontends contain no analysis concerns — they cannot reach the graph, cache, or adapters
//! except through this type. A frontend that needs a new fact is a core PR adding it to
//! [`RunResult`], never a core import.
//!
//! Adapter *registration* is the **distribution layer's** concern (the `kndo` crate): the
//! core never knows which languages exist (the ignorance rule), and frontends
//! never compose the product — they call `kndo::open`, which passes the registry in here.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::adapter::{Diagnostic, DiagnosticLevel, LanguageAdapter, ProjectPath, Span};
use crate::analysis;
use crate::discovery;
use crate::gitutil;
use crate::graph;
use crate::query_envelope::{self, QueryRequest, QueryResult};
use crate::vocab::Confidence;

/// The set of paths that differ (added, removed, or content-changed) between two graphs' file
/// sets — "the change set," in the terms, at file granularity. Feeds diff mode's
/// `delta_origin`: a new finding whose path is in this set is `Introduced` (inside the change
/// itself); otherwise it's `Derived` (flipped at a distance by untouched code).
fn touched_paths(before: &graph::ProjectGraph, after: &graph::ProjectGraph) -> HashSet<String> {
    let before_hashes: HashMap<&str, [u8; 32]> = before
        .files
        .iter()
        .map(|f| (f.path.0.as_str(), f.content_hash))
        .collect();
    let after_hashes: HashMap<&str, [u8; 32]> = after
        .files
        .iter()
        .map(|f| (f.path.0.as_str(), f.content_hash))
        .collect();

    let mut touched = HashSet::default();
    for (path, hash) in &after_hashes {
        if before_hashes.get(path) != Some(hash) {
            touched.insert((*path).to_string()); // added or content-modified
        }
    }
    for path in before_hashes.keys() {
        if !after_hashes.contains_key(path) {
            touched.insert((*path).to_string()); // removed
        }
    }
    touched
}

/// The coverage freshness gate: a coverage report modified longer ago than this is ignored with
/// a diagnostic — stale certainty is worse than absence. The default; `[plugins.<id>] max-age`
/// in `kndo.toml` overrides it per plugin.
const MAX_COVERAGE_AGE_DAYS: u64 = 7;

/// Expand one report pattern to `(absolute path, project-relative display path)` matches.
/// A pattern with no glob metacharacters is stat'd literally (the common well-known-path
/// case, zero-cost); a glob walks only the directories its literal components pin down —
/// the same `glob::glob` mechanism activation's `FileExists` rules use, so activation and
/// ingestion see the same files by the same rules. Matches come back sorted (deterministic
/// provenance order).
fn expand_report_pattern(root: &Path, pattern: &str) -> Vec<(std::path::PathBuf, String)> {
    let relify = |abs: &Path| {
        abs.strip_prefix(root)
            .unwrap_or(abs)
            .to_string_lossy()
            .replace('\\', "/")
    };
    if !pattern.contains(['*', '?', '[']) {
        let path = root.join(pattern);
        return if path.is_file() {
            vec![(path, pattern.to_string())]
        } else {
            Vec::new()
        };
    }
    let full = root.join(pattern);
    let Ok(paths) = glob::glob(&full.to_string_lossy()) else {
        return Vec::new(); // validated at config-parse time; a bad descriptor glob is inert
    };
    let mut matches: Vec<(std::path::PathBuf, String)> = paths
        .flatten()
        .filter(|p| p.is_file())
        .map(|p| {
            let rel = relify(&p);
            (p, rel)
        })
        .collect();
    matches.sort_by(|a, b| a.1.cmp(&b.1));
    matches
}

/// The second, graph-guided rebase pass: land report keys qualified by a *package name*
/// rather than a location — Go coverprofiles record module-qualified paths
/// (`github.com/x/y/pkg/file.go`), and the mapping from module name to directory lives in
/// the graph's package table (neutral vocabulary the manifest adapters fill; no ecosystem
/// knowledge here). Guarded so it can never mis-attribute: only keys matching *no* graph
/// file are candidates, and a candidate moves only when exactly one `name/ → dir/` mapping
/// produces a path that *does* match a graph file — anything else stays verbatim and
/// matches nothing (degrade to silence, never to a wrong file).
fn rebase_by_packages(map: &mut crate::coverage::CoverageMap, graph: &crate::graph::ProjectGraph) {
    if map.is_empty() {
        return;
    }
    let mappings: Vec<(String, String)> = graph
        .packages
        .iter()
        .filter_map(|package| {
            let name = package.name.as_ref()?;
            let manifest = package.manifest.as_ref()?;
            let dir = match manifest.0.rfind('/') {
                Some(idx) => format!("{}/", &manifest.0[..idx]),
                None => String::new(),
            };
            Some((format!("{name}/"), dir))
        })
        .collect();
    if mappings.is_empty() {
        return;
    }
    let graph_files: rustc_hash::FxHashSet<&str> =
        graph.files.iter().map(|f| f.path.0.as_str()).collect();
    let moves: Vec<(String, String)> = map
        .files
        .keys()
        .filter(|key| !graph_files.contains(key.0.as_str()))
        .filter_map(|key| {
            let mut resolved: Option<String> = None;
            for (prefix, dir) in &mappings {
                if let Some(rest) = key.0.strip_prefix(prefix.as_str()) {
                    let candidate = format!("{dir}{rest}");
                    if graph_files.contains(candidate.as_str()) {
                        if resolved.is_some() {
                            return None; // ambiguous — leave the key alone
                        }
                        resolved = Some(candidate);
                    }
                }
            }
            resolved.map(|to| (key.0.to_string(), to))
        })
        .collect();
    // Whole keys as "prefixes": strip leaves the empty remainder, so each pair is an
    // exact one-key move through the same merge-on-collision core.
    map.rebase_prefixes(&moves);
}

impl Drop for Engine {
    /// The persist sequencing: frontends drop the engine after printing, so the deferred
    /// snapshot write completes "after results are printed, before exit".
    fn drop(&mut self) {
        self.join_persist();
    }
}

/// Where full-mode runs remember their last health score (`.kndo/health.json`) so the next
/// run can report the trend (the output schema's `previous`). Diff modes never touch it — their
/// `previous` is the computed "before" side.
fn health_snapshot_path(root: &Path) -> PathBuf {
    root.join(".kndo").join("health.json")
}

fn load_health_snapshot(root: &Path) -> Option<crate::analysis::health::HealthSummary> {
    let bytes = std::fs::read(health_snapshot_path(root)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn store_health_snapshot(root: &Path, score: f64, grade: &str) {
    let summary = crate::analysis::health::HealthSummary {
        score,
        grade: grade.to_string(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&summary) {
        let path = health_snapshot_path(root);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, json); // best-effort: read-only checkouts stay silent
    }
}

/// Mirrors the output schema's `schema_version` (contracts/output-schema.md).
pub const SCHEMA_VERSION: &str = "1.0.0";

/// The product version — every crate shares `version.workspace = true`, so kndo-core's own
/// `CARGO_PKG_VERSION` is the same string the distribution crate and CLI would report.
pub const KNDO_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct ConfigOverrides {
    /// `--no-cache`: disables the facts cache entirely for this run. Defaults to
    /// `true` — the correctness gate is that this must never change *findings*, only whether
    /// the run was warm.
    pub use_cache: bool,
    /// `--threads N` > `KNDO_THREADS` env > default — resolved to a concrete
    /// value *before* reaching here (frontend concern, like the precedence chain itself);
    /// `None` means "physical cores," the stated default, not "unspecified." `Some(1)` is
    /// a first-class supported mode (determinism checks, debugging, noisy-neighbor CI runners).
    pub threads: Option<usize>,
    /// The report floor override: findings below this confidence tier are dropped from the
    /// report (never counted as suppressed — a floor is a display posture, not an
    /// acknowledgment). `None` defers to `kndo.toml [analysis] min-confidence`, and with
    /// neither set the floor is `Possible` — every tier reported, the historical behavior.
    /// The CLI passes `Some(Possible)` under `--verbose` so verbose always shows
    /// everything even when the project config raises the floor.
    pub min_confidence: Option<crate::vocab::Confidence>,
}

impl Default for ConfigOverrides {
    fn default() -> Self {
        ConfigOverrides {
            use_cache: true,
            threads: None,
            min_confidence: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("project root does not exist or is not a directory: {}", .0.display())]
    ProjectRootNotFound(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunMode {
    Full,
    Staged,
    Diff { base: String },
}

impl RunMode {
    fn as_str(&self) -> &'static str {
        match self {
            RunMode::Full => "full",
            RunMode::Staged => "staged",
            RunMode::Diff { .. } => "diff",
        }
    }

    fn base_ref(&self) -> Option<String> {
        match self {
            RunMode::Diff { base } => Some(base.clone()),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct CheckRequest {
    pub mode: RunMode,
}

/// `kndo baseline`'s two modes (see [`Engine::baseline`]): `Create`
/// refuses if `.kndo/baseline.json` already exists (a bare re-run can't tell intended growth
/// from intended shrinkage without a human reviewing the diff first); `Update` always
/// (re)writes a full snapshot from the current finding set — auto-dropping entries that no
/// longer reproduce, adding whatever's newly present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineOp {
    Create,
    Update,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineResult {
    Written {
        acknowledged: usize,
    },
    /// `Create` requested but a baseline is already there.
    AlreadyExists,
    WriteFailed(String),
}

/// One registered adapter, as `kndo doctor` reports it (the "what was detected:
/// adapters…") — static descriptor info, not tied to any particular run.
#[derive(Debug, Clone)]
pub struct DoctorAdapterInfo {
    pub id: String,
    pub grammar_version: String,
    pub file_globs: Vec<String>,
    pub manifest_globs: Vec<String>,
    /// Shown for the same reason `DoctorPluginInfo.activation` is: visible even
    /// though it's dormant for every compiled-in adapter today (empty = always-on), it's what
    /// gates a *globally installed* adapter (`kndo::global_adapter_candidates`, a separate
    /// call this report has no visibility into, same split `DoctorPluginInfo`'s own doc notes).
    pub activation: Vec<String>,
    /// Dependency coordinates — same contract as [`DoctorPluginInfo::dependencies`]: rendered
    /// so an activation chain is inspectable; whether each one was satisfied is the composition
    /// layer's report (`kndo::adapter_resolution`), not this struct's.
    pub dependencies: Vec<String>,
}

/// `kndo doctor`'s cache section — `None` when the cache is disabled entirely
/// (`ConfigOverrides.use_cache: false`), `Some` with a fresh, uncreated cache's stats
/// (everything zero, `writable` reflecting whether the directory itself could be created) when
/// enabled but never yet used.
#[derive(Debug, Clone, Copy)]
pub struct DoctorCacheInfo {
    pub writable: bool,
    pub facts_entries: usize,
    pub facts_bytes: u64,
    pub graph_snapshots: usize,
    pub graph_snapshot_bytes: u64,
}

/// One registered plugin, as `kndo doctor` reports it — static descriptor info, matching
/// [`DoctorAdapterInfo`]'s shape. `detection`/`activation`/`requested_file_access` are shown so
/// it's visible *why* a plugin would activate. Every plugin reaching this struct is already
/// part of the composed set `Engine` was built with — `PluginDescriptor.activation`
/// is evaluated earlier, only for globally installed plugins, by `kndo`'s composition layer
/// (`crates/kndo/src/lib.rs`'s `activation` module), before `Engine::open_with_plugins` is even
/// called; this report has no visibility into global candidates that were discovered and
/// *skipped* (`kndo::global_plugin_candidates` covers that, a separate call the CLI's `doctor`
/// command makes directly), only the final set
/// that actually made it into composition.
#[derive(Debug, Clone)]
pub struct DoctorPluginInfo {
    pub id: String,
    pub version: String,
    pub detection: Vec<String>,
    pub activation: Vec<String>,
    /// Dependency coordinates — rendered so an activation chain is inspectable; whether each
    /// one was satisfied is the composition layer's report (`kndo::plugin_resolution`), not
    /// this struct's.
    pub dependencies: Vec<String>,
    pub requested_file_access: Vec<String>,
    /// The rules this plugin may emit findings under, pre-rendered
    /// (`"<name> (<severity>): <description>"`) — what a component MAY assert, visible
    /// before it ever runs.
    pub rules: Vec<String>,
}

/// `kndo doctor`'s report: everything detected about this
/// project, without running a check — read-only and instant, so it stays useful for debugging a
/// setup that itself might be slow or broken.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub project_root: String,
    pub adapters: Vec<DoctorAdapterInfo>,
    pub plugins: Vec<DoctorPluginInfo>,
    pub cache_enabled: bool,
    pub cache: Option<DoctorCacheInfo>,
    /// The last recorded plugin round's per-plugin contribution counts (the audit
    /// record, read from the cache's sidecar) — empty when no round has been recorded (no
    /// cache, no plugins, or no run yet). The one field here that reflects a *past run*
    /// rather than static configuration; still a plain file read, keeping doctor instant.
    pub plugin_contributions: Vec<crate::plugin::PluginContribution>,
    pub baseline_present: bool,
    pub baseline_entries: usize,
}

/// A finding's severity — each category carries one as
/// a fixed default; `--strict` promotion isn't implemented yet, so this is always the default.
/// Declaration order doubles as sort/triage order: worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// Where a finding points. Every field is optional because not
/// every subject has all of them: `version-skew`/`duplicate` findings span multiple manifests
/// or files, so no single `path` is *the* location — expressing that properly is the `related`
/// evidence chain, not yet implemented (deferred, not faked with an arbitrary first path).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Location {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<ProjectPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Span>,
    /// The finding's primary named subject — a code symbol's name for symbol-kind findings,
    /// but also a dependency's name for `undeclared`/`version-skew` (which have no code symbol
    /// at all): whatever single name a reader or the agent-format renderer would point at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

/// A finding's place in a diff-mode delta. `None`
/// in full mode — there is no "before" to compare against, so the concept doesn't apply, and
/// the field is omitted rather than forced to some default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Delta {
    New,
    Fixed,
}

/// Only set on `Delta::New` findings: does this finding sit *inside* the change set itself
/// (`Introduced` — dead on arrival, an agent or author can self-correct before committing) or
/// does it live in untouched code that flipped because of the change at a distance (`Derived`)?
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum DeltaOrigin {
    Introduced,
    Derived,
}

/// One entry of a finding's evidence chain (the schema's `related` — what `kndo
/// explain` renders): a concrete location plus its role in the story. Populated by
/// `cyclic` ("a shortest cycle path in `related` as the evidence chain"); other
/// analyses adopt it as they gain evidence models — never fabricated as a placeholder.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RelatedLocation {
    /// What this location contributes: "cycle-hop", "cause", …
    pub role: String,
    pub path: crate::adapter::ProjectPath,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<crate::adapter::Span>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Typed form of the output-schema finding (every field lands in the JSON schema
/// first — that document is normative). Not yet present:
/// `evidence` (category-specific block), `sources`, `remediation`, `rolled_up` — each needs
/// infrastructure that doesn't exist yet (computed remediation text) and is omitted
/// rather than fabricated with a placeholder.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Finding {
    pub id: String,
    pub category: crate::vocab::Category,
    pub group: crate::vocab::Group,
    pub subject_kind: crate::vocab::SubjectKind,
    pub severity: Severity,
    pub confidence: Confidence,
    pub message: String,
    pub location: Location,
    /// Evidence chain — empty for analyses that haven't adopted it yet.
    /// `default` isn't for deserialization (Finding is serialize-only) — it tells the schema
    /// generator the field is optional, matching the skip-when-empty serialization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RelatedLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<Delta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_origin: Option<DeltaOrigin>,
    /// The severity channel: `true` = this finding never influences exit codes or
    /// budgets, whatever its `severity` says — the state of every plugin-contributed finding
    /// (`plugin:` categories) without an explicit `[plugins.gate]` opt-in. Always `false` for
    /// core findings; serialized only when true (additive).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub advisory: bool,
}

/// The shared triage presentation order — severity (worst first), then path, then span start.
/// Every renderer that lists findings sorts with this, so human, agent, and any future
/// frontend agree on order.
pub fn sort_findings_for_display(findings: &mut [&Finding]) {
    fn path_key(f: &Finding) -> &str {
        f.location.path.as_ref().map(|p| p.0.as_str()).unwrap_or("")
    }
    fn span_key(f: &Finding) -> (u32, u32) {
        f.location.range.map(|r| r.start).unwrap_or((0, 0))
    }
    findings.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then_with(|| path_key(a).cmp(path_key(b)))
            .then_with(|| span_key(a).cmp(&span_key(b)))
    });
}

/// One registered adapter's contribution (`run.adapters[]`).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AdapterRunInfo {
    pub id: String,
    pub files: usize,
}

/// `baseline` summary (the `baseline` envelope field):
/// `acknowledged` counts baseline entries that still match a current finding (excluded from
/// `findings` and from `--fail-on`); `stale` counts entries that match nothing anymore — the
/// underlying issue was fixed, and `kndo baseline --update` would drop them.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BaselineSummary {
    pub acknowledged: usize,
    pub stale: usize,
}

/// `suppressed.inline`/`suppressed.config` — always present
/// (unlike `baseline`, which is `None` when the feature isn't adopted at all): both
/// mechanisms run on every check, so `{ inline: 0, config: 0 }` is a meaningful "nothing
/// suppressed," not an absent subsystem. `inline` counts pragma-matched findings;
/// `config` counts findings filtered by `kndo.toml`'s `[analysis].skip` and `[[rule]]`
/// entries (a finding covered by both counts as `inline` — pragmas run first).
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SuppressedSummary {
    pub inline: usize,
    pub config: usize,
}

/// Typed form of the output-schema envelope. JSON/SARIF/agent serializers live core-side so
/// every frontend emits byte-identical machine output; *human* rendering is frontend-owned.
/// Flat here for ergonomic Rust consumption; [`RunResult::to_json`] nests it into
/// the schema's actual shape. Not yet present: `budget` — the delta-budget gate subsystem
/// doesn't exist yet, so the field is omitted rather
/// than emitted empty/null. Adding it later is additive (minor schema bump), not
/// a breaking change.
#[derive(Debug, Default)]
pub struct RunResult {
    /// Full mode: every finding. Diff modes: only *new*
    /// findings — findings that disappeared belong in `fixed` below, not here.
    pub findings: Vec<Finding>,
    /// Diff modes only: findings present in the "before" tree
    /// but absent from "after," each carrying `delta: Fixed` and the *previous* location.
    /// Always empty in full mode.
    pub fixed: Vec<Finding>,
    /// Typed diagnostics (the schema's `diagnostics` array) — one representation everywhere,
    /// never parallel stringly-typed variants.
    pub diagnostics: Vec<Diagnostic>,
    pub files_discovered: usize,
    /// Files a registered adapter recognized (subset of `files_discovered`); `adapters` below
    /// is the per-language breakdown the schema actually wants (`run.adapters[].files`).
    pub files_claimed: usize,
    pub symbols: usize,
    pub dependencies: usize,
    pub edges: usize,
    pub mode: String,
    pub base_ref: Option<String>,
    pub started_at: String,
    pub duration_ms: u64,
    pub project_root: String,
    pub adapters: Vec<AdapterRunInfo>,
    /// Whether the cache was consulted at all (`--no-cache` ⇒ `false`) and how many things it
    /// actually served this run — facts entries plus, when the whole graph matched, one graph
    /// snapshot (`ProjectCache::hits() + ProjectCache::graph_hits()`) — the only honest way to
    /// know whether a run was warm: a freshly-`kndo init`ed project has the cache *enabled* on
    /// its very first run and is still, correctly, cold.
    pub cache_enabled: bool,
    pub cache_hits: u64,
    /// `None` when `.kndo/baseline.json` doesn't exist — distinct from `Some`
    /// with zero counts, which means a baseline exists and is fully clean/reproducing.
    pub baseline: Option<BaselineSummary>,
    /// Inline `kndo:allow` pragmas matched against this run's findings — always
    /// present, unlike `baseline`. In diff modes this reflects the "after" side only, mirroring
    /// how `baseline` is applied symmetrically but reported from "after" (see `run_diff`).
    pub suppressed: SuppressedSummary,
    /// Per-phase wall times, `(phase, µs)` in execution order (the `--verbose`
    /// block). Diff modes carry the "after" side's phases prefixed `after:` plus one
    /// `before-side` rollup. Deliberately NOT serialized into the JSON envelope: wall times
    /// are run metadata, and the determinism matrix compares envelopes byte-for-byte.
    pub timings: Vec<(String, u64)>,
    /// The health score. Full mode: the current tree, with
    /// `previous` from the last stored snapshot when the cache holds one. Diff modes: the
    /// "after" side, with `previous` computed from "before".
    /// `None` only when assembly itself failed.
    pub health: Option<crate::analysis::health::Health>,
}

/// `"warm"` only when the cache was on *and* actually served something this run — an
/// enabled-but-empty cache (first run ever, or every file changed) is honestly `"cold"`.
/// Shared by every renderer (`RunResult`'s `to_json`/`to_agent_format` and
/// `Engine::query`'s envelope alike) so "what counts as warm" is defined exactly once.
fn cache_status_str(enabled: bool, hits: u64) -> &'static str {
    if enabled && hits > 0 {
        "warm"
    } else {
        "cold"
    }
}

impl RunResult {
    pub fn cache_status(&self) -> &'static str {
        cache_status_str(self.cache_enabled, self.cache_hits)
    }
}

/// Owned mirror of the JSON envelope's `run` object — not borrowed, unlike a hot-path type,
/// because this exists purely to be serialized (and, behind `schema`, to derive the JSON
/// Schema from): the one-time clone per `--format json` invocation is free by comparison.
#[derive(serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct RunInfo<'a> {
    mode: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_ref: Option<&'a str>,
    started_at: &'a str,
    duration_ms: u64,
    cache: &'static str,
    project_root: &'a str,
    adapters: &'a [AdapterRunInfo],
}

/// The full `--format json` envelope shape — also the schema
/// generator's root type (`cargo xtask gen-schema`, gated behind the `schema` feature): the
/// JSON Schema is derived from this struct, not maintained as a second hand-written document.
/// Borrows the run's collections instead of cloning them: `to_json` on a 75k-finding result
/// was paying a full deep clone (every String in every Finding) purely to serialize — the
/// borrow makes serialization allocation-free on the input side. Schema output is unaffected
/// (schemars sees through references and slices to the same shapes).
#[derive(serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct Envelope<'a> {
    schema_version: &'static str,
    kndo_version: &'static str,
    run: RunInfo<'a>,
    findings: &'a [Finding],
    #[serde(default, skip_serializing_if = "<[Finding]>::is_empty")]
    fixed: &'a [Finding],
    #[serde(skip_serializing_if = "Option::is_none")]
    health: Option<&'a crate::analysis::health::Health>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline: Option<&'a BaselineSummary>,
    suppressed: SuppressedSummary,
    diagnostics: &'a [Diagnostic],
}

impl RunResult {
    fn to_envelope(&self) -> Envelope<'_> {
        Envelope {
            schema_version: SCHEMA_VERSION,
            kndo_version: KNDO_VERSION,
            run: RunInfo {
                mode: &self.mode,
                base_ref: self.base_ref.as_deref(),
                started_at: &self.started_at,
                duration_ms: self.duration_ms,
                cache: self.cache_status(),
                project_root: &self.project_root,
                adapters: &self.adapters,
            },
            findings: &self.findings,
            fixed: &self.fixed,
            health: self.health.as_ref(),
            baseline: self.baseline.as_ref(),
            suppressed: self.suppressed,
            diagnostics: &self.diagnostics,
        }
    }

    /// The `--format json` rendering — serialized core-side so
    /// every frontend emits byte-identical machine output.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.to_envelope())
            .unwrap_or_else(|e| format!("{{\"error\": \"failed to serialize output: {e}\"}}"))
    }

    /// The `--format agent` rendering — like JSON, serialized
    /// core-side so every frontend emits byte-identical agent text.
    pub fn to_agent_format(&self) -> String {
        crate::agent_format::render(self)
    }

    /// The `--format sarif` rendering — SARIF 2.1.0,
    /// serialized core-side like every machine format.
    pub fn to_sarif(&self) -> String {
        crate::sarif::render(self)
    }
}

/// The `--format json` envelope's JSON Schema, derived from [`Envelope`] itself — never a
/// second hand-written document (the derived schema is normative). Dev-time
/// only: regenerate the committed copy with `cargo xtask gen-schema`.
#[cfg(feature = "schema")]
pub fn json_schema() -> schemars::Schema {
    schemars::schema_for!(Envelope)
}

/// One global rayon pool per process — `--threads N` > `KNDO_THREADS` env >
/// physical cores, already resolved into `threads` by the frontend before it ever reaches here.
/// rayon's global pool can only be *built* once per process; a second `Engine::open` call
/// (embedders opening more than one engine, or many tests sharing one test binary) hits
/// `build_global`'s "already initialized" error, silently ignored — whichever call came first
/// wins the thread count for the rest of the process. This can never threaten
/// determinism: thread count only ever changes *scheduling*, never which bytes come out.
/// The `min-confidence` report floor. `stale` is exempt — the "your suppressions are
/// dead" audit must not disappear behind a floor the audited pragmas cannot influence —
/// and a `Possible` floor is the identity (nothing sits below the lowest tier).
fn apply_confidence_floor(findings: Vec<Finding>, floor: crate::vocab::Confidence) -> Vec<Finding> {
    if floor == crate::vocab::Confidence::Possible {
        return findings;
    }
    findings
        .into_iter()
        .filter(|f| f.category == "stale" || f.confidence >= floor)
        .collect()
}

fn ensure_thread_pool(threads: Option<usize>) {
    let n = threads.unwrap_or_else(|| num_cpus::get_physical().max(1));
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build_global();
}

/// One tree's full analysis, as [`Engine::assemble_and_analyze`] returns it — graph plus
/// everything derived from it in that pass.
struct AnalyzedTree {
    graph: std::sync::Arc<graph::ProjectGraph>,
    findings: Vec<Finding>,
    diagnostics: Vec<Diagnostic>,
    suppressed: SuppressedSummary,
    health: crate::analysis::health::Health,
    /// `(phase, µs)` in execution order: assembly + coverage first, then every analysis phase.
    timings: Vec<(String, u64)>,
}

fn describe_rule(rule: &crate::plugin::RuleDescriptor) -> String {
    let severity = match rule.severity {
        crate::plugin::PluginSeverity::Error => "error",
        crate::plugin::PluginSeverity::Warning => "warning",
        crate::plugin::PluginSeverity::Info => "info",
    };
    format!("{} ({severity}): {}", rule.name, rule.description)
}

/// One plugin proto finding → an output [`Finding`] under the severity-channel
/// rules: no `[plugins.gate]` entry (or `"off"`) → advisory at the declared severity;
/// a gate entry → gate-eligible at `min(declared, configured)` — config can lower a rule's
/// declared severity, never raise it. The `plugin:`-namespaced category and `convention`
/// group are already assembled host-side (graph.rs's finding round); nothing here is
/// guest-controlled beyond message/confidence/target.
fn plugin_finding(
    proto: crate::plugin::ProtoFinding,
    gate: &crate::plugin_gate::PluginsGate,
) -> Finding {
    let (severity, advisory) = severity_channel(&proto, gate);
    let category = crate::vocab::Category::new(proto.category);
    let subject_kind = crate::vocab::SubjectKind::new(proto.subject_kind);
    Finding {
        id: crate::analysis::finding_id(crate::analysis::FindingIdParts {
            category: &category,
            subject_kind: &subject_kind,
            path: proto.path.0.as_str(),
            symbol_path: proto.symbol.as_deref().unwrap_or(""),
            discriminator: "",
        }),
        category,
        group: crate::vocab::Group::Convention,
        subject_kind,
        severity,
        confidence: proto.confidence,
        message: proto.message,
        location: Location {
            path: Some(proto.path),
            range: proto.span,
            symbol: proto.symbol.map(|s| s.to_string()),
            package: proto.package,
        },
        related: Vec::new(),
        delta: None,
        delta_origin: None,
        advisory,
    }
}

/// The mapping: `(severity, advisory)` for one proto finding under the gate config.
fn severity_channel(
    proto: &crate::plugin::ProtoFinding,
    gate: &crate::plugin_gate::PluginsGate,
) -> (Severity, bool) {
    let declared = declared_severity(proto.severity);
    match gate.resolve(&proto.plugin_id, &proto.rule) {
        None | Some(crate::plugin_gate::GateLevel::Off) => (declared, true),
        // Severity's declared order is worst-first, so `max` picks the LESS severe of the
        // two — the "lower, never raise" cap.
        Some(crate::plugin_gate::GateLevel::At(cap)) => (declared.max(cap), false),
    }
}

fn declared_severity(severity: crate::plugin::PluginSeverity) -> Severity {
    match severity {
        crate::plugin::PluginSeverity::Error => Severity::Error,
        crate::plugin::PluginSeverity::Warning => Severity::Warning,
        crate::plugin::PluginSeverity::Info => Severity::Info,
    }
}

/// Synchronous and single-instance-per-project (the cache lock); a serving
/// frontend wraps it in its own concurrency model.
pub struct Engine {
    root: PathBuf,
    adapters: Vec<Box<dyn LanguageAdapter>>,
    plugins: Vec<Box<dyn crate::plugin::Plugin>>,
    cache: Option<crate::cache::ProjectCache>,
    cache_enabled: bool,
    /// The in-flight background snapshot write (persist off the critical path)
    /// — spawned right after assembly so serialization overlaps with analysis and rendering,
    /// joined before the next assembly and on drop (frontends drop the engine after
    /// printing, which is exactly "written after results are printed, before
    /// exit"). Crash-safety is the writer's temp-file + rename; a killed process loses only
    /// cache warmth.
    pending_persist: Option<std::thread::JoinHandle<()>>,
    /// `kndo.toml`, read and parsed once at open (`[plugins.gate]` included).
    config: crate::config::KndoConfig,
    /// Problems reading it — surfaced as run diagnostics, never a failed open.
    config_problems: Vec<String>,
    /// `config` merged under this open's `ConfigOverrides` — the one resolution, computed
    /// once ([`crate::config::KndoConfig::resolve`]) and read everywhere a knob's final
    /// value is needed.
    effective: crate::config::EffectiveConfig,
}

impl Engine {
    /// `adapters` is the registered language set, composed by the distribution layer (the
    /// `kndo` crate) — compiled-in first-party adapters today, WASM-bridged third-party
    /// adapters later. The core never selects or knows about them beyond the
    /// trait; embedders and tests may pass a custom set directly.
    pub fn open(
        root: &Path,
        overrides: ConfigOverrides,
        adapters: Vec<Box<dyn LanguageAdapter>>,
    ) -> Result<Engine, EngineError> {
        // No plugins — not even coverage ingesters: core registers nothing it doesn't
        // define (the ignorance rule covers report formats too). The product's built-in
        // set, coverage ingesters included, is composed by the `kndo` crate's
        // `default_plugins()` and arrives through `open_with_plugins`, exactly like
        // adapters do.
        Engine::open_with_plugins(root, overrides, adapters, vec![])
    }

    /// Same as [`Self::open`], additionally taking the registered plugin set —
    /// compiled-in first-party plugins (the `kndo` crate's `default_plugins()`, coverage
    /// ingesters included) and WASM-bridged third-party plugins alike. [`Self::open`] itself
    /// registers none: plugins are composition, not core.
    pub fn open_with_plugins(
        root: &Path,
        overrides: ConfigOverrides,
        adapters: Vec<Box<dyn LanguageAdapter>>,
        plugins: Vec<Box<dyn crate::plugin::Plugin>>,
    ) -> Result<Engine, EngineError> {
        if !root.is_dir() {
            return Err(EngineError::ProjectRootNotFound(root.to_path_buf()));
        }
        let (config, config_problems) = crate::config::KndoConfig::load(root);
        let effective = config.resolve(&overrides);
        ensure_thread_pool(effective.threads);
        let cache = overrides
            .use_cache
            .then(|| crate::cache::ProjectCache::open(root));
        Ok(Engine {
            root: root.to_path_buf(),
            adapters,
            plugins,
            cache,
            cache_enabled: overrides.use_cache,
            pending_persist: None,
            config,
            config_problems,
            effective,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `kndo doctor`. Deliberately does not assemble or analyze
    /// anything — every field comes from static descriptors, a cache-directory stat walk, and a
    /// baseline-file read, so this stays fast and side-effect-free even when the project itself
    /// would be slow or broken to check.
    pub fn doctor(&self) -> DoctorReport {
        let adapters = self
            .adapters
            .iter()
            .map(|a| {
                let d = a.descriptor();
                DoctorAdapterInfo {
                    id: d.id.to_string(),
                    grammar_version: d.grammar_version.to_string(),
                    file_globs: d.file_globs.iter().map(|g| g.to_string()).collect(),
                    manifest_globs: d.manifest_globs.iter().map(|g| g.to_string()).collect(),
                    activation: d.activation.iter().map(|r| r.describe()).collect(),
                    dependencies: d.dependencies.iter().map(|c| c.to_string()).collect(),
                }
            })
            .collect();

        let plugins = self
            .plugins
            .iter()
            .map(|p| {
                let d = p.descriptor();
                DoctorPluginInfo {
                    id: d.id.to_string(),
                    version: d.version.to_string(),
                    detection: d.detection.iter().map(|s| s.to_string()).collect(),
                    activation: d.activation.iter().map(|r| r.describe()).collect(),
                    dependencies: d.dependencies.iter().map(|c| c.to_string()).collect(),
                    requested_file_access: d
                        .requested_file_access
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                    rules: p.rules().iter().map(describe_rule).collect(),
                }
            })
            .collect();

        let cache = self.cache.as_ref().map(|c| {
            let stats = c.stats();
            DoctorCacheInfo {
                writable: stats.writable,
                facts_entries: stats.facts_entries,
                facts_bytes: stats.facts_bytes,
                graph_snapshots: stats.graph_snapshots,
                graph_snapshot_bytes: stats.graph_snapshot_bytes,
            }
        });

        let baseline_entries = crate::baseline::load(&self.root);
        DoctorReport {
            project_root: self.root.display().to_string(),
            adapters,
            plugins,
            cache_enabled: self.cache_enabled,
            cache,
            plugin_contributions: self
                .cache
                .as_ref()
                .and_then(|c| c.plugin_contributions())
                .unwrap_or_default(),
            baseline_present: baseline_entries.is_some(),
            baseline_entries: baseline_entries.map(|e| e.len()).unwrap_or(0),
        }
    }

    /// Full mode reports every current finding; `--staged`/`--diff <ref>` report the
    /// derived-effects delta instead — see [`Self::run_diff`].
    pub fn check(&mut self, req: CheckRequest) -> RunResult {
        let start = Instant::now();
        let started_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mode = req.mode.as_str().to_string();
        let base_ref = req.mode.base_ref();
        let project_root = self.root.display().to_string();

        let outcome = match &req.mode {
            RunMode::Full => {
                let root = self.root.clone();
                let mut raw = self.run_analysis_at(&root);
                // Trend (the "trend vs previous snapshots"): the last full run's
                // score, stored beside the baseline in `.kndo/` — best-effort on read-only
                // checkouts, and deliberately not in the prunable cache directory.
                if let Some(health) = &mut raw.health {
                    health.previous = load_health_snapshot(&self.root);
                    store_health_snapshot(&self.root, health.score, &health.grade);
                }
                let (findings, baseline) = self.apply_baseline(raw.findings);
                RunResult {
                    findings,
                    baseline,
                    ..raw
                }
            }
            RunMode::Staged | RunMode::Diff { .. } => self.run_diff(&req.mode),
        };

        if let Some(cache) = &self.cache {
            cache.prune(crate::cache::DEFAULT_CAP_BYTES);
        }

        RunResult {
            mode,
            base_ref,
            started_at,
            duration_ms: start.elapsed().as_millis() as u64,
            project_root,
            cache_enabled: self.cache_enabled,
            cache_hits: self
                .cache
                .as_ref()
                .map(|c| c.hits() + c.graph_hits())
                .unwrap_or(0),
            ..outcome
        }
    }

    /// The derived-effects delta: assemble the graph at two tree states and report
    /// `(findings_after − findings_before) ∪ (findings_before − findings_after)`, each finding
    /// tagged `delta: New|Fixed` (and, for `New`, `delta_origin: Introduced|Derived` — whether
    /// it sits inside a *touched* file or was flipped at a distance in untouched
    /// code). Both sides run through baseline filtering symmetrically before the
    /// diff, so an acknowledged issue never surfaces as new or fixed on either side.
    ///
    /// Tree states, a deliberate choice of git
    /// semantics: `--staged`'s "after" is the **index**, not the raw working
    /// tree — exactly what would be committed, excluding further unstaged edits on top (the
    /// pre-commit use case `kndo init --hook` installs wants precisely this). `--staged`'s
    /// "before" is `HEAD`. `--diff <ref>`'s "before" is `merge-base(<ref>, HEAD)`; "after" is
    /// the real working tree as-is (uncommitted changes included) — it's just `self.root`.
    ///
    /// Git-side tree states are read **in memory** (`discovery::TreeSource::GitTree`:
    /// `ls-tree` + `cat-file --batch`), never checked out to temp directories — materializing
    /// whole trees was measured as diff mode's dominant cost (~0.5 s of file-creation syscalls
    /// per tree at 5k files, ×2 for `--staged`, plus cleanup) and scaled with repo size instead
    /// of change size. When `self.root` is a subdirectory of the repository, tree paths are
    /// scoped and re-relativized to it (`prefix`), so both diff sides and full mode agree on
    /// the same project-relative paths.
    fn run_diff(&mut self, mode: &RunMode) -> RunResult {
        let git_root = match gitutil::repo_root(&self.root) {
            Ok(r) => r,
            Err(e) => {
                return Self::git_failure(
                    "--staged/--diff need a git repository — run inside one, or drop the flag for a full scan",
                    e,
                )
            }
        };
        // Canonicalize both sides before computing the prefix: `repo_root` comes back
        // canonicalized from git, while `self.root` is whatever the frontend passed.
        let canonical_root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let prefix = canonical_root
            .strip_prefix(&git_root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        let before_treeish = match mode {
            RunMode::Staged => gitutil::rev_parse(&git_root, "HEAD"),
            RunMode::Diff { base } => gitutil::merge_base(&git_root, base, "HEAD"),
            RunMode::Full => unreachable!("run_diff is only called for Staged/Diff"),
        };
        let before_treeish = match before_treeish {
            Ok(t) => t,
            Err(e) => {
                let context = match mode {
                    RunMode::Diff { base } => format!(
                        "the diff base `{base}` does not resolve — check the ref name, or fetch it first (`git fetch origin {base}`)"
                    ),
                    _ => "HEAD does not resolve — the repository may have no commits yet; commit once, or drop --staged for a full scan".to_string(),
                };
                return Self::git_failure(&context, e);
            }
        };

        // `--staged`'s "after" is the index as a tree object (`write-tree` — the one
        // object-database write diff mode performs; it never touches the real index or working
        // tree). `--diff`'s "after" is the working tree itself. Owned locals (not borrows of
        // `self`) because `assemble_and_analyze` needs `&mut self` right after.
        let after_treeish: Option<String> =
            match mode {
                RunMode::Staged => match gitutil::write_tree(&git_root) {
                    Ok(t) => Some(t),
                    Err(e) => return Self::git_failure(
                        "cannot snapshot the git index for --staged — check repository permissions",
                        e,
                    ),
                },
                RunMode::Diff { .. } => None,
                RunMode::Full => unreachable!("run_diff is only called for Staged/Diff"),
            };
        let work_root = self.root.clone();

        let before_source = discovery::TreeSource::GitTree {
            repo_root: &git_root,
            treeish: &before_treeish,
            prefix: &prefix,
        };
        let after_source = match &after_treeish {
            Some(treeish) => discovery::TreeSource::GitTree {
                repo_root: &git_root,
                treeish,
                prefix: &prefix,
            },
            None => discovery::TreeSource::Directory(&work_root),
        };

        let before = match self.assemble_and_analyze(&before_source) {
            Ok(t) => t,
            Err(d) => {
                return RunResult {
                    diagnostics: vec![d],
                    ..RunResult::default()
                }
            }
        };
        let after = match self.assemble_and_analyze(&after_source) {
            Ok(t) => t,
            Err(d) => {
                return RunResult {
                    diagnostics: vec![d],
                    ..RunResult::default()
                }
            }
        };
        let before_total_us: u64 = before.timings.iter().map(|(_, us)| us).sum();
        let mut diff_timings: Vec<(String, u64)> = after
            .timings
            .iter()
            .map(|(phase, us)| (format!("after:{phase}"), *us))
            .collect();
        diff_timings.push(("before-side".to_string(), before_total_us));
        let (before_graph, before_findings, before_diagnostics, before_health) = (
            before.graph,
            before.findings,
            before.diagnostics,
            before.health,
        );
        let (after_graph, after_findings, after_diagnostics, after_suppressed, after_health) = (
            after.graph,
            after.findings,
            after.diagnostics,
            after.suppressed,
            after.health,
        );

        let (before_findings, _) = self.apply_baseline(before_findings);
        let (after_findings, baseline) = self.apply_baseline(after_findings);

        let touched = touched_paths(&before_graph, &after_graph);
        let before_ids: HashSet<String> = before_findings.iter().map(|f| f.id.clone()).collect();
        let after_ids: HashSet<String> = after_findings.iter().map(|f| f.id.clone()).collect();

        let new_findings: Vec<Finding> = after_findings
            .into_iter()
            .filter(|f| !before_ids.contains(f.id.as_str()))
            .map(|mut f| {
                let origin = match &f.location.path {
                    Some(p) if touched.contains(p.0.as_str()) => DeltaOrigin::Introduced,
                    _ => DeltaOrigin::Derived,
                };
                f.delta = Some(Delta::New);
                f.delta_origin = Some(origin);
                f
            })
            .collect();
        let fixed_findings: Vec<Finding> = before_findings
            .into_iter()
            .filter(|f| !after_ids.contains(f.id.as_str()))
            .map(|mut f| {
                f.delta = Some(Delta::Fixed);
                f
            })
            .collect();

        let adapters = self
            .adapters
            .iter()
            .map(|a| {
                let id = a.descriptor().id;
                let files = after_graph
                    .files
                    .iter()
                    .filter(|f| f.language.as_deref() == Some(id.as_str()))
                    .count();
                AdapterRunInfo {
                    id: id.to_string(),
                    files,
                }
            })
            .collect();

        let mut diagnostics = after_diagnostics;
        diagnostics.extend(before_diagnostics);

        RunResult {
            files_discovered: after_graph.files.len(),
            files_claimed: after_graph
                .files
                .iter()
                .filter(|f| f.language.is_some())
                .count(),
            symbols: after_graph.symbols.len(),
            dependencies: after_graph.dependencies.len(),
            edges: after_graph.edges.len(),
            diagnostics,
            findings: new_findings,
            fixed: fixed_findings,
            adapters,
            baseline,
            suppressed: after_suppressed,
            health: {
                let mut health = after_health;
                health.previous = Some(crate::analysis::health::HealthSummary {
                    score: before_health.score,
                    grade: before_health.grade,
                });
                Some(health)
            },
            timings: diff_timings,
            ..RunResult::default()
        }
    }

    /// A diff-mode git failure is an **error**, not a degradation:
    /// the user explicitly asked for `--staged`/`--diff`, the analysis never ran, and an empty
    /// result at exit 0 would fail open in CI (a typo'd base ref silently passing the
    /// gate). Problem + probable cause + next command.
    fn git_failure(context: &str, e: gitutil::GitError) -> RunResult {
        RunResult {
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Error,
                path: None,
                message: format!("{context} ({e})"),
                span: None,
            }],
            ..RunResult::default()
        }
    }

    /// `kndo baseline [--update]`. Snapshots the complete, current
    /// finding set — bypassing whatever baseline already exists, since the whole point is
    /// capturing what "acknowledged" means right now, not what's left after an old baseline
    /// already filtered it down.
    pub fn baseline(&mut self, op: BaselineOp) -> BaselineResult {
        if op == BaselineOp::Create && crate::baseline::exists(&self.root) {
            return BaselineResult::AlreadyExists;
        }
        let root = self.root.clone();
        let findings = self.run_analysis_at(&root).findings;
        let entries: Vec<crate::baseline::BaselineEntry> = findings
            .iter()
            .map(crate::baseline::BaselineEntry::from)
            .collect();
        let acknowledged = entries.len();
        match crate::baseline::save(&self.root, &entries) {
            Ok(()) => BaselineResult::Written { acknowledged },
            Err(e) => BaselineResult::WriteFailed(e.to_string()),
        }
    }

    /// One navigation query: assembles/warms the
    /// graph exactly like full-mode `check`, then dispatches to the requested verb. Read-only —
    /// never touches findings, the baseline, or anything beyond what assembly's own cache
    /// read/write already does.
    pub fn query(&mut self, req: QueryRequest) -> QueryResult {
        self.query_batch(vec![req])
            .into_iter()
            .next()
            .expect("query_batch returns exactly one result per request")
    }

    /// `kndo query`'s batching entry point: assembles/warms the graph exactly
    /// **once** for the whole batch — the amortization batching exists for —
    /// then answers every request against that one shared snapshot. One request failing (bad
    /// selector, no path) never drops the others; every request sees the same graph, so answers
    /// stay mutually consistent (no torn reads across a batch).
    pub fn query_batch(&mut self, requests: Vec<QueryRequest>) -> Vec<QueryResult> {
        let start = Instant::now();
        let root = self.root.clone();
        let source = discovery::TreeSource::Directory(&root);
        let analyzed = match self.assemble_and_analyze(&source) {
            Ok(t) => t,
            Err(d) => {
                return requests
                    .into_iter()
                    .map(|req| query_envelope::build_failure(req, d.message.clone()))
                    .collect()
            }
        };
        let (graph, findings) = (analyzed.graph, analyzed.findings);
        let reach = query_envelope::compute_reachability(&graph);
        let findings_owned = findings; // keep the Vec<Finding> alive across the borrow below
        let locations = query_envelope::finding_locations(&findings_owned);
        let cache = cache_status_str(
            self.cache_enabled,
            self.cache
                .as_ref()
                .map(|c| c.hits() + c.graph_hits())
                .unwrap_or(0),
        );
        let duration_ms = start.elapsed().as_millis() as u64;

        requests
            .into_iter()
            .map(|req| query_envelope::run(&graph, &reach, &locations, req, cache, duration_ms))
            .collect()
    }

    fn join_persist(&mut self) {
        if let Some(handle) = self.pending_persist.take() {
            let _ = handle.join();
        }
    }

    /// The lowest-level shared step: assemble the graph from an arbitrary tree source (the
    /// real project directory for full mode; in-memory git trees for diff modes' "before" and
    /// `--staged`'s "after") and run every analysis over it. `self.cache` is still the *real*
    /// project's `.kndo/cache/` regardless of source — the facts layer keys purely by content
    /// hash, so it's fully shared across trees; the graph-snapshot layer's key folds in the
    /// whole file set, so a differing tree just misses cleanly rather than colliding with the
    /// real project's own cached graph.
    fn assemble_and_analyze(
        &mut self,
        source: &discovery::TreeSource<'_>,
    ) -> Result<AnalyzedTree, Diagnostic> {
        self.join_persist(); // at most one background writer in flight
        let assemble_start = Instant::now();
        match graph::assemble_from_source(
            source,
            &self.adapters,
            &self.plugins,
            self.cache.as_ref(),
        ) {
            Ok(graph::AssembledGraph {
                graph: g,
                discovery_diagnostics,
                extraction_diagnostics,
                plugin_diagnostics,
                plugin_findings,
                finding_diagnostics,
                pending_snapshot,
                timings: assembly_timings,
            }) => {
                let mut timings = vec![(
                    "assemble".to_string(),
                    assemble_start.elapsed().as_micros() as u64,
                )];
                timings.extend(
                    assembly_timings
                        .into_iter()
                        .map(|(phase, us)| (format!("assemble:{phase}"), us)),
                );
                // Surface-member closure (graph.rs docs): idempotent strip-and-recompute at
                // this single choke point — cold, patch, and warm snapshot paths all analyze
                // the same derived surface. A snapshot may persist the edges; the strip-first
                // recompute makes that carryover irrelevant.
                let mut g = g;
                let closure_start = Instant::now();
                crate::graph::recompute_surface_closure(&mut g);
                timings.push((
                    "surface-closure".to_string(),
                    closure_start.elapsed().as_micros() as u64,
                ));
                let g = std::sync::Arc::new(g);
                if let Some(writer) = pending_snapshot {
                    // Extraction + manifest diagnostics and the plugin round's own, in the
                    // snapshot's two partitions — exactly what a
                    // warm path replays; discovery diagnostics stay fresh per walk.
                    let graph_for_writer = std::sync::Arc::clone(&g);
                    let diagnostics_for_writer = extraction_diagnostics.clone();
                    let plugin_diagnostics_for_writer = plugin_diagnostics.clone();
                    self.pending_persist = Some(std::thread::spawn(move || {
                        writer.write(
                            &graph_for_writer,
                            &diagnostics_for_writer,
                            &plugin_diagnostics_for_writer,
                        );
                    }));
                }
                let mut diagnostics = discovery_diagnostics;
                diagnostics.extend(extraction_diagnostics);
                diagnostics.extend(plugin_diagnostics);
                diagnostics.extend(finding_diagnostics);
                diagnostics.extend(self.config_problems.iter().map(|p| Diagnostic {
                    level: DiagnosticLevel::Warn,
                    path: None,
                    message: p.clone(),
                    span: None,
                }));
                let coverage_start = Instant::now();
                let coverage = self.ingest_coverage(&g, &mut diagnostics);
                timings.push((
                    "coverage-ingest".to_string(),
                    coverage_start.elapsed().as_micros() as u64,
                ));
                let outcome = analysis::run_all(&g, &coverage, &self.effective.tuning);
                let (mut findings, analysis_diagnostics, health) =
                    (outcome.findings, outcome.diagnostics, outcome.health);
                timings.extend(
                    outcome
                        .timings
                        .into_iter()
                        .map(|(phase, us)| (phase.to_string(), us)),
                );
                diagnostics.extend(analysis_diagnostics);
                // Plugin findings join AFTER run_all (health is computed inside it,
                // so the score structurally cannot see them) and BEFORE suppression,
                // so inline `kndo:allow plugin:...` pragmas apply uniformly. Baseline is
                // applied later by check(), uniformly too. Re-sorted by id — the same
                // deterministic order run_all itself guarantees.
                findings.extend(
                    plugin_findings
                        .into_iter()
                        .map(|p| plugin_finding(p, &self.config.plugins_gate)),
                );
                findings.sort_unstable_by(|a, b| a.id.cmp(&b.id));
                // The dynamic half of suppression category validation (module docs of
                // `suppression`): every `plugin:<coordinate>/<rule>` category the active
                // plugins declare, whether or not the rule emitted anything this run.
                let plugin_categories: rustc_hash::FxHashSet<String> = self
                    .plugins
                    .iter()
                    .flat_map(|p| {
                        let id = p.descriptor().id;
                        p.rules()
                            .into_iter()
                            .filter(|r| crate::plugin::is_valid_rule_name(&r.name))
                            .map(move |r| format!("plugin:{id}/{}", r.name))
                    })
                    .collect();
                let (findings, mut suppressed) =
                    crate::suppression::apply(&g, findings, &plugin_categories);
                // Config suppression runs strictly AFTER pragmas: staleness was judged
                // against the complete finding set, so a pragma covering a config-skipped
                // finding stays honestly non-stale, and a finding covered by both counts
                // as inline (config never saw it). Then the min-confidence floor — a
                // display posture, not an acknowledgment, so it is dropped, not counted.
                let (findings, config_suppressed) = self.config.filter_findings(findings);
                suppressed.config = config_suppressed;
                let findings =
                    apply_confidence_floor(findings, self.effective.min_confidence_floor);
                Ok(AnalyzedTree {
                    graph: g,
                    findings,
                    diagnostics,
                    suppressed,
                    health,
                    timings,
                })
            }
            Err(crate::discovery::DiscoveryError::Root(e)) => Err(Diagnostic {
                level: DiagnosticLevel::Warn,
                path: None,
                message: format!(
                    "cannot read the project tree: {e} — check the path and permissions"
                ),
                span: None,
            }),
        }
    }

    /// Locate and ingest coverage reports ("ingested, never measured") through each
    /// coverage plugin's report patterns — a `[plugins.<id>] report` entry in `kndo.toml`
    /// when present (explicit config *replaces* the built-in list, like every other knob),
    /// the descriptor's `requested_file_access` well-known paths otherwise; both accept
    /// globs (`packages/*/coverage/lcov.info`). Every match is freshness-checked against
    /// the plugin's effective max-age ([`MAX_COVERAGE_AGE_DAYS`], or `[plugins.<id>]
    /// max-age`) — a stale report gets one diagnostic and is ignored, per the ADR's "stale
    /// certainty is worse than absence". Re-read every run, never cached: a report's
    /// freshness varies independently of source content hashes.
    ///
    /// Reports are always read from the *real* project root, including for diff modes'
    /// git-tree sides — coverage describes the working tree's test run, and applying the same
    /// current report to both sides keeps a diff's `crap` delta about the *code* change, not
    /// about report drift (a deliberate approximation; the report predates the diff either
    /// way).
    fn ingest_coverage(
        &self,
        graph: &crate::graph::ProjectGraph,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> crate::coverage::CoverageMap {
        let mut sink = crate::coverage::CoverageSink::default();
        for plugin in &self.plugins {
            let descriptor = plugin.descriptor();
            let options = self.config.plugin_options_for(&descriptor.id);
            let configured = options.map(|o| o.report.as_slice()).unwrap_or(&[]);
            let patterns: Vec<String> = if configured.is_empty() {
                descriptor
                    .requested_file_access
                    .iter()
                    .map(|p| p.to_string())
                    .collect()
            } else {
                configured.to_vec()
            };
            let max_age_secs = options
                .and_then(|o| o.max_age)
                .map(|d| d.as_secs())
                .unwrap_or(MAX_COVERAGE_AGE_DAYS * 86_400);
            let mut seen = rustc_hash::FxHashSet::default();
            for pattern in &patterns {
                for (path, rel) in expand_report_pattern(&self.root, pattern) {
                    if !seen.insert(rel.clone()) {
                        continue; // one report matched by two patterns ingests once
                    }
                    let Ok(meta) = std::fs::metadata(&path) else {
                        continue; // no report at this path — silence, not a diagnostic
                    };
                    let age_secs = meta
                        .modified()
                        .ok()
                        .and_then(|m| m.elapsed().ok())
                        .map(|e| e.as_secs());
                    if let Some(secs) = age_secs {
                        if secs > max_age_secs {
                            let days = secs / 86_400;
                            let max_days = max_age_secs as f64 / 86_400.0;
                            diagnostics.push(Diagnostic {
                                level: DiagnosticLevel::Warn,
                                path: Some(ProjectPath(smol_str::SmolStr::new(&rel))),
                                message: format!(
                                    "coverage report {rel} ignored: {days} days old (max age \
                                     {max_days:.1} days) — regenerate it to restore \
                                     coverage-aware analysis"
                                ),
                                span: None,
                            });
                            continue;
                        }
                    }
                    match std::fs::read(&path) {
                        Ok(content) => {
                            let project_path = ProjectPath(smol_str::SmolStr::new(&rel));
                            plugin.ingest_coverage(&project_path, &content, &mut sink);
                            // Provenance is host-side: the host located the report and
                            // checked its freshness, so it records what was ingested and how
                            // old it was (surfaced by health's crap category).
                            sink.add_source(format!(
                                "{} {} ({}d old)",
                                descriptor.id,
                                rel,
                                age_secs.map(|s| s / 86_400).unwrap_or(0)
                            ));
                        }
                        Err(e) => diagnostics.push(Diagnostic {
                            level: DiagnosticLevel::Warn,
                            path: Some(ProjectPath(smol_str::SmolStr::new(&rel))),
                            message: format!("coverage report {rel} could not be read: {e}"),
                            span: None,
                        }),
                    }
                }
            }
        }
        let mut map = sink.into_map();
        // Coverage tools commonly record absolute paths; graph paths are project-relative.
        // Rebasing is host-side — the one layer that knows the root — so every format
        // plugin's output lands comparable (CoverageMap::rebase).
        map.rebase(&self.root);
        rebase_by_packages(&mut map, graph);
        map
    }

    /// Full-mode `RunResult` construction — assemble + analyze at `root`, plus the run counters
    /// (`files_discovered`, `symbols`, …) that only full mode reports directly (diff mode
    /// builds its own `RunResult` in [`Self::run_diff`], from the "after" side).
    fn run_analysis_at(&mut self, root: &Path) -> RunResult {
        match self.assemble_and_analyze(&discovery::TreeSource::Directory(root)) {
            Ok(AnalyzedTree {
                graph: g,
                findings,
                diagnostics,
                suppressed,
                health,
                timings,
            }) => {
                let adapters = self
                    .adapters
                    .iter()
                    .map(|a| {
                        let id = a.descriptor().id;
                        let files = g
                            .files
                            .iter()
                            .filter(|f| f.language.as_deref() == Some(id.as_str()))
                            .count();
                        AdapterRunInfo {
                            id: id.to_string(),
                            files,
                        }
                    })
                    .collect();
                RunResult {
                    files_discovered: g.files.len(),
                    files_claimed: g.files.iter().filter(|f| f.language.is_some()).count(),
                    symbols: g.symbols.len(),
                    dependencies: g.dependencies.len(),
                    edges: g.edges.len(),
                    diagnostics,
                    findings,
                    adapters,
                    suppressed,
                    health: Some(health),
                    timings,
                    ..RunResult::default()
                }
            }
            Err(d) => RunResult {
                diagnostics: vec![d],
                ..RunResult::default()
            },
        }
    }

    /// Partitions `findings` against `.kndo/baseline.json`: a matched entry is
    /// excluded from the returned findings (and so from `--fail-on`, which only ever sees what
    /// `check()` returns) and counted in the summary instead. `None` when no baseline file
    /// exists — distinct from `Some` with `acknowledged: 0`, a baseline that exists but matches
    /// nothing right now (everything it acknowledged got fixed).
    fn apply_baseline(&self, findings: Vec<Finding>) -> (Vec<Finding>, Option<BaselineSummary>) {
        let Some(entries) = crate::baseline::load(&self.root) else {
            return (findings, None);
        };
        let all_ids: HashSet<&str> = findings.iter().map(|f| f.id.as_str()).collect();
        let acknowledged = entries
            .iter()
            .filter(|e| all_ids.contains(e.id.as_str()))
            .count();
        let stale = entries.len() - acknowledged;

        let baselined_ids: HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        let kept = findings
            .into_iter()
            .filter(|f| !baselined_ids.contains(f.id.as_str()))
            .collect();
        (
            kept,
            Some(BaselineSummary {
                acknowledged,
                stale,
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocab::RefKind;
    use smol_str::SmolStr;

    /// True for the category-level skip diagnostics (`untested` with no test roots, `crap`
    /// with no ingested coverage) — expected noise in every diff-mode fixture below, since
    /// none of this module's mock projects declare test roots or ship a coverage report.
    /// Filtering them out keeps `diagnostics`-shape assertions meaningful for genuine
    /// regressions instead of forcing every diff test to know about finding categories it
    /// isn't testing.
    fn is_no_test_roots_diagnostic(d: &Diagnostic) -> bool {
        d.message.starts_with("untested: no test roots detected")
            || d.message.starts_with("crap: no coverage ingested")
    }

    #[test]
    fn display_sort_orders_by_severity_then_path_then_span() {
        fn f(severity: Severity, path: Option<&str>, line: u32) -> Finding {
            Finding {
                advisory: false,
                id: String::new(),
                category: "unused".into(),
                group: crate::vocab::Group::Waste,
                subject_kind: "function".into(),
                severity,
                confidence: crate::vocab::Confidence::Certain,
                message: String::new(),
                location: Location {
                    path: path.map(|p| crate::adapter::ProjectPath(SmolStr::new(p))),
                    range: (line > 0).then_some(crate::adapter::Span {
                        start: (line, 1),
                        end: (line, 2),
                    }),
                    ..Location::default()
                },
                related: Vec::new(),
                delta: None,
                delta_origin: None,
            }
        }
        let findings = [
            f(Severity::Info, Some("a.rs"), 5),
            f(Severity::Warning, Some("z.rs"), 1),
            f(Severity::Warning, Some("a.rs"), 9),
            f(Severity::Warning, Some("a.rs"), 2),
            f(Severity::Warning, None, 0), // pathless sorts as "" — first among warnings
        ];
        let mut refs: Vec<&Finding> = findings.iter().collect();
        sort_findings_for_display(&mut refs);
        let order: Vec<(Severity, &str, u32)> = refs
            .iter()
            .map(|f| {
                (
                    f.severity,
                    f.location.path.as_ref().map(|p| p.0.as_str()).unwrap_or(""),
                    f.location.range.map(|r| r.start.0).unwrap_or(0),
                )
            })
            .collect();
        assert_eq!(
            order,
            vec![
                (Severity::Warning, "", 0),
                (Severity::Warning, "a.rs", 2),
                (Severity::Warning, "a.rs", 9),
                (Severity::Warning, "z.rs", 1),
                (Severity::Info, "a.rs", 5),
            ]
        );
    }

    /// Bare-minimum adapter claiming `.mock` files — engine.rs can't depend on a real adapter
    /// crate (that would invert the layering the ignorance rule protects), but a cache-warmth
    /// test needs *something* to extract, or every file stays factless and nothing is ever
    /// cached.
    struct CacheMockAdapter;

    impl LanguageAdapter for CacheMockAdapter {
        fn descriptor(&self) -> crate::adapter::AdapterDescriptor {
            crate::adapter::AdapterDescriptor {
                activation: Vec::new(),
                dependencies: Vec::new(),
                id: SmolStr::new("mock"),
                facts_schema_version: 1,
                file_globs: vec![SmolStr::new("**/*.mock")],
                manifest_globs: vec![],
                grammar_version: SmolStr::new("mock"),
                visibility_ladder: vec![
                    crate::adapter::VisibilityRung {
                        scope: crate::adapter::VisibilityScope::Unit,
                        label: SmolStr::new("private"),
                        surface_transitive: false,
                    },
                    crate::adapter::VisibilityRung {
                        scope: crate::adapter::VisibilityScope::Public,
                        label: SmolStr::new("exported"),
                        surface_transitive: true,
                    },
                ],
                cycle_policy: crate::adapter::CyclePolicy {
                    file_cycles: crate::adapter::CycleTolerance::Hazard,
                    package_cycles: crate::adapter::CycleTolerance::Hazard,
                },
                resolves_dependency_usage: true,
                package_test_dirs: Vec::new(),
            }
        }
        fn claim(&self, path: &ProjectPath) -> Option<crate::adapter::FileClaim> {
            path.0
                .ends_with(".mock")
                .then(|| crate::adapter::FileClaim {
                    language: SmolStr::new("mock"),
                    class: Default::default(),
                })
        }
        fn claim_manifest(&self, _path: &ProjectPath) -> bool {
            false
        }
        fn extract(&self, _file: &crate::adapter::SourceFile<'_>) -> crate::adapter::FileFacts {
            crate::adapter::FileFacts::default()
        }
        fn extract_manifest(
            &self,
            _file: &crate::adapter::SourceFile<'_>,
            _ctx: &crate::adapter::ResolveCtx<'_>,
        ) -> crate::adapter::ManifestFacts {
            crate::adapter::ManifestFacts::default()
        }
        fn resolve(
            &self,
            _spec: &crate::adapter::ImportSpec,
            _ctx: &crate::adapter::ResolveCtx<'_>,
        ) -> crate::adapter::Resolution {
            crate::adapter::Resolution::Unresolved
        }
    }

    /// A second mock, richer than [`CacheMockAdapter`]: understands `root-file` (a whole-file
    /// production root), `import ./sibling.dmock` (an `ImportsFile` edge),
    /// `suppress-file <category>` (a File-scope `RawSuppression`), `decl <name>` (an exported
    /// Function declaration), and `ref <name>` (a file-granular reference) — enough to drive
    /// `unused` (file-level), inline suppression, and navigation-query tests alike.
    struct DiffMockAdapter;

    impl LanguageAdapter for DiffMockAdapter {
        fn descriptor(&self) -> crate::adapter::AdapterDescriptor {
            crate::adapter::AdapterDescriptor {
                activation: Vec::new(),
                dependencies: Vec::new(),
                id: SmolStr::new("dmock"),
                facts_schema_version: 1,
                file_globs: vec![SmolStr::new("**/*.dmock")],
                manifest_globs: vec![],
                grammar_version: SmolStr::new("dmock"),
                visibility_ladder: vec![
                    crate::adapter::VisibilityRung {
                        scope: crate::adapter::VisibilityScope::Unit,
                        label: SmolStr::new("private"),
                        surface_transitive: false,
                    },
                    crate::adapter::VisibilityRung {
                        scope: crate::adapter::VisibilityScope::Public,
                        label: SmolStr::new("exported"),
                        surface_transitive: true,
                    },
                ],
                cycle_policy: crate::adapter::CyclePolicy {
                    file_cycles: crate::adapter::CycleTolerance::Hazard,
                    package_cycles: crate::adapter::CycleTolerance::Hazard,
                },
                resolves_dependency_usage: true,
                package_test_dirs: Vec::new(),
            }
        }
        fn claim(&self, path: &ProjectPath) -> Option<crate::adapter::FileClaim> {
            path.0
                .ends_with(".dmock")
                .then(|| crate::adapter::FileClaim {
                    language: SmolStr::new("dmock"),
                    class: Default::default(),
                })
        }
        fn claim_manifest(&self, _path: &ProjectPath) -> bool {
            false
        }
        fn extract(&self, file: &crate::adapter::SourceFile<'_>) -> crate::adapter::FileFacts {
            let text = std::str::from_utf8(file.content).unwrap_or("");
            let mut facts = crate::adapter::FileFacts::default();
            for line in text.lines() {
                if line == "root-file" {
                    facts.roots.push(crate::adapter::RawRoot {
                        kind: crate::vocab::RootKind::Production,
                        target: crate::adapter::RawRootTarget::WholeFile,
                        confidence: Confidence::Certain,
                    });
                } else if let Some(category) = line.strip_prefix("suppress-file ") {
                    facts.suppressions.push(crate::adapter::RawSuppression {
                        span: Span::default(),
                        category: SmolStr::new(category),
                        subject: None,
                        reason: None,
                        scope: crate::adapter::SuppressionScope::File,
                    });
                } else if let Some(spec) = line.strip_prefix("import ") {
                    facts.imports.push(crate::adapter::RawImport {
                        specifier: SmolStr::new(spec),
                        kind: crate::adapter::ImportKind::Relative,
                        span: Span::default(),
                        side_effect_only: true,
                        type_only: false,
                        confidence: Confidence::Certain,
                        bindings: vec![],
                        reexported: false,
                        opaque_namespace_use: false,
                        module_names_visible: false,
                        local_alias: None,
                    });
                } else if let Some(name) = line.strip_prefix("decl ") {
                    facts.declarations.push(crate::adapter::Declaration {
                        name: SmolStr::new(name),
                        kind: crate::vocab::SymbolKind::Function,
                        span: Span::default(),
                        exported: true,
                        visibility: crate::adapter::VisibilityLevel(1),
                        member_of: None,
                        signature_span: None,
                        implicitly_invoked: false,
                        nested_scope: false,
                        visibility_inherited: false,
                    });
                } else if let Some(name) = line.strip_prefix("ref ") {
                    facts.references.push(crate::adapter::RawReference {
                        name: SmolStr::new(name),
                        scope_context: None,
                        span: Span::default(),
                        within: None,
                        kind: RefKind::Read,
                    });
                }
            }
            facts
        }
        fn extract_manifest(
            &self,
            _file: &crate::adapter::SourceFile<'_>,
            _ctx: &crate::adapter::ResolveCtx<'_>,
        ) -> crate::adapter::ManifestFacts {
            crate::adapter::ManifestFacts::default()
        }
        fn resolve(
            &self,
            spec: &crate::adapter::ImportSpec,
            ctx: &crate::adapter::ResolveCtx<'_>,
        ) -> crate::adapter::Resolution {
            let Some(rel) = spec.specifier.strip_prefix("./") else {
                return crate::adapter::Resolution::Unresolved;
            };
            let dir = spec.from.0.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let candidate = if dir.is_empty() {
                rel.to_string()
            } else {
                format!("{dir}/{rel}")
            };
            let path = ProjectPath(SmolStr::new(candidate));
            if ctx.contains(&path) {
                crate::adapter::Resolution::File(path, Confidence::Certain)
            } else {
                crate::adapter::Resolution::Unresolved
            }
        }
    }

    // ------------------------------------------ Plugin graph-mutation hooks

    /// Exercises all four graph-affecting hooks in one real `Engine::check` — proof the wiring
    /// (not just each analysis's own isolated exemption unit test) actually connects: a
    /// `classify_file` role/origin override, a `contribute_roots` root, a `contribute_edges`
    /// reference, and an `annotate_symbols` mark, each targeting a *different* declaration so
    /// the test can tell which hook did what — plus one untouched control declaration that must
    /// stay flagged, proving the plugin didn't just root everything.
    struct DemoPlugin;

    impl crate::plugin::Plugin for DemoPlugin {
        fn descriptor(&self) -> crate::plugin::PluginDescriptor {
            crate::plugin::PluginDescriptor {
                id: SmolStr::new("demo"),
                version: SmolStr::new("1"),
                detection: vec![],
                // Declares access to a companion config-like file outside the
                // language graph — `contribute_roots` below reads it to gate a fifth root.
                requested_file_access: vec![SmolStr::new("content.marker")],
                activation: vec![],
                dependencies: vec![],
            }
        }

        fn classify_file(
            &self,
            path: &ProjectPath,
            current: crate::vocab::FileClass,
        ) -> Option<crate::vocab::FileClass> {
            path.0
                .ends_with(".banner.dmock")
                .then_some(crate::vocab::FileClass {
                    role: current.role,
                    origin: crate::vocab::FileOrigin::Generated,
                })
        }

        fn contribute_roots(
            &self,
            _graph: &crate::plugin::GraphView<'_>,
            content: &crate::plugin::ContentView<'_>,
            out: &mut crate::plugin::RootSink,
        ) {
            out.add(
                crate::plugin::PluginTarget::symbol(
                    ProjectPath(SmolStr::new("handler.dmock")),
                    "rootedByPlugin",
                ),
                crate::vocab::RootKind::Production,
                Confidence::Probable,
            );
            // A real content-channel read gates a real root — proves the host
            // plumbing (glob scoping, budget-tracked read) actually reaches a graph-mutation
            // hook, not just that the type-checker accepts the new parameter.
            if content
                .read(&ProjectPath(SmolStr::new("content.marker")))
                .as_deref()
                == Some(b"promote".as_slice())
            {
                out.add(
                    crate::plugin::PluginTarget::symbol(
                        ProjectPath(SmolStr::new("handler.dmock")),
                        "contentGatedRoot",
                    ),
                    crate::vocab::RootKind::Production,
                    Confidence::Probable,
                );
            }
        }

        fn contribute_edges(
            &self,
            _graph: &crate::plugin::GraphView<'_>,
            _content: &crate::plugin::ContentView<'_>,
            out: &mut crate::plugin::EdgeSink,
        ) {
            out.add(
                crate::plugin::PluginTarget::file(ProjectPath(SmolStr::new("root.dmock"))),
                crate::plugin::PluginTarget::symbol(
                    ProjectPath(SmolStr::new("handler.dmock")),
                    "referencedByPlugin",
                ),
                RefKind::Call,
                Confidence::Probable,
            );
        }

        fn annotate_symbols(
            &self,
            _graph: &crate::plugin::GraphView<'_>,
            _content: &crate::plugin::ContentView<'_>,
            out: &mut crate::plugin::AnnotationSink,
        ) {
            out.mark_externally_consumed(
                ProjectPath(SmolStr::new("handler.dmock")),
                "almostInternalOnly",
            );
        }
    }

    #[test]
    fn plugin_graph_hooks_affect_a_real_check() {
        let dir = std::env::temp_dir().join("kndo-engine-test-plugin-hooks");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // `import ./handler` makes `handler.dmock` reachable *as a file* (an `ImportsFile`
        // edge from the already-rooted `root.dmock`) without reaching any of its individual
        // declarations — those need their own root/reference edge, which is exactly what
        // distinguishes the four scenarios below instead of collapsing them into one
        // file-level `unused` rollup.
        std::fs::write(
            dir.join("root.dmock"),
            "root-file\nimport ./handler.dmock\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("handler.dmock"),
            "decl rootedByPlugin\n\
             decl referencedByPlugin\n\
             decl almostInternalOnly\n\
             ref almostInternalOnly\n\
             decl trulyDead\n\
             decl contentGatedRoot\n",
        )
        .unwrap();
        std::fs::write(dir.join("noise.banner.dmock"), "decl bannerDecl\n").unwrap();
        // Content the plugin's contribute_roots reads through the host-mediated
        // channel to decide whether to root `contentGatedRoot` — not itself part of the
        // language graph (the mock adapter never claims `.marker` files).
        std::fs::write(dir.join("content.marker"), "promote").unwrap();

        // Baseline, no plugin: every one of the four declarations the plugin later rescues
        // must actually be flagged on its own — otherwise the assertions below would pass
        // vacuously regardless of whether the plugin wiring does anything at all.
        let mut baseline_engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let baseline = baseline_engine.check(CheckRequest {
            mode: RunMode::Full,
        });
        let baseline_unused: Vec<&str> = baseline
            .findings
            .iter()
            .filter(|f| f.category == "unused")
            .filter_map(|f| f.location.symbol.as_deref())
            .collect();
        assert!(
            baseline_unused.contains(&"rootedByPlugin"),
            "{baseline_unused:?}"
        );
        assert!(
            baseline_unused.contains(&"referencedByPlugin"),
            "{baseline_unused:?}"
        );
        assert!(
            baseline_unused.contains(&"contentGatedRoot"),
            "{baseline_unused:?}"
        );
        let baseline_unused_files: Vec<String> = baseline
            .findings
            .iter()
            .filter(|f| f.category == "unused" && f.subject_kind == "file")
            .filter_map(|f| f.location.path.as_ref())
            .map(|p| p.0.to_string())
            .collect();
        assert!(baseline_unused_files.contains(&"noise.banner.dmock".to_string()));
        let baseline_internal_only: Vec<&str> = baseline
            .findings
            .iter()
            .filter(|f| f.category == "internal-only")
            .filter_map(|f| f.location.symbol.as_deref())
            .collect();
        assert!(baseline_internal_only.contains(&"almostInternalOnly"));

        let adapters: Vec<Box<dyn LanguageAdapter>> = vec![Box::new(DiffMockAdapter)];
        let plugins: Vec<Box<dyn crate::plugin::Plugin>> = vec![Box::new(DemoPlugin)];
        let mut engine =
            Engine::open_with_plugins(&dir, ConfigOverrides::default(), adapters, plugins).unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });

        let unused_symbols: Vec<&str> = result
            .findings
            .iter()
            .filter(|f| f.category == "unused")
            .filter_map(|f| f.location.symbol.as_deref())
            .collect();
        assert!(
            !unused_symbols.contains(&"rootedByPlugin"),
            "contribute_roots should have kept this reachable: {unused_symbols:?}"
        );
        assert!(
            !unused_symbols.contains(&"referencedByPlugin"),
            "contribute_edges should have kept this reachable: {unused_symbols:?}"
        );
        assert!(
            !unused_symbols.contains(&"contentGatedRoot"),
            "the content-channel-gated root should have kept this reachable: {unused_symbols:?}"
        );
        assert!(
            unused_symbols.contains(&"trulyDead"),
            "the untouched control declaration must still be flagged: {unused_symbols:?}"
        );

        let unused_files: Vec<String> = result
            .findings
            .iter()
            .filter(|f| f.category == "unused" && f.subject_kind == "file")
            .filter_map(|f| f.location.path.as_ref())
            .map(|p| p.0.to_string())
            .collect();
        assert!(
            !unused_files.contains(&"noise.banner.dmock".to_string()),
            "classify_file's Generated override should exempt this file: {unused_files:?}"
        );

        let internal_only_symbols: Vec<&str> = result
            .findings
            .iter()
            .filter(|f| f.category == "internal-only")
            .filter_map(|f| f.location.symbol.as_deref())
            .collect();
        assert!(
            !internal_only_symbols.contains(&"almostInternalOnly"),
            "annotate_symbols should have exempted this: {internal_only_symbols:?}"
        );
    }

    #[test]
    fn doctor_reports_every_registered_plugin() {
        let dir = std::env::temp_dir().join("kndo-engine-test-doctor-plugins");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let engine = Engine::open_with_plugins(
            &dir,
            ConfigOverrides::default(),
            vec![],
            vec![Box::new(DemoPlugin)],
        )
        .unwrap();
        let report = engine.doctor();
        assert_eq!(report.plugins.len(), 1);
        assert_eq!(report.plugins[0].id, "demo");
        assert_eq!(report.plugins[0].version, "1");
        assert!(report.plugins[0].activation.is_empty());

        // The zero-plugin case is zero — callers who ask for no plugins get no plugins.
        let bare =
            Engine::open_with_plugins(&dir, ConfigOverrides::default(), vec![], vec![]).unwrap();
        assert!(bare.doctor().plugins.is_empty());
    }

    /// A throwaway git repo for diff-mode tests — local signing disabled for the same reason
    /// `gitutil`'s own test fixtures disable it (this sandbox signs every commit via an
    /// MCP-backed tool unrelated to what's under test, and it occasionally times out).
    fn git_repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kndo-engine-difftest-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "-q", "."]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "test"]);
        git(&["config", "commit.gpgsign", "false"]);
        dir
    }

    fn git_add_all_commit(dir: &std::path::Path, message: &str) {
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", message]);
    }

    #[test]
    fn a_second_check_on_the_same_engine_is_warm() {
        let dir = std::env::temp_dir().join("kndo-engine-test-warm");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.mock"), "hello").unwrap();

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(CacheMockAdapter)],
        )
        .unwrap();

        let first = engine.check(CheckRequest {
            mode: RunMode::Full,
        });
        assert!(first.cache_enabled);
        assert_eq!(first.cache_hits, 0); // nothing cached yet — the whole run is a miss
        assert!(first.to_json().contains("\"cache\": \"cold\""));

        let second = engine.check(CheckRequest {
            mode: RunMode::Full,
        });
        assert_eq!(second.cache_hits, 1);
        assert!(second.to_json().contains("\"cache\": \"warm\""));

        // `--no-cache` must never change *findings*, only warmth.
        assert_eq!(first.files_claimed, second.files_claimed);
        assert_eq!(first.symbols, second.symbols);
    }

    #[test]
    fn no_cache_override_reports_cold_even_after_a_prior_warm_engine() {
        let dir = std::env::temp_dir().join("kndo-engine-test-no-cache");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.mock"), "hello").unwrap();

        // Warm the on-disk cache with one engine…
        Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(CacheMockAdapter)],
        )
        .unwrap()
        .check(CheckRequest {
            mode: RunMode::Full,
        });

        // …then open a fresh engine with the cache disabled: it must never report warm, even
        // though the disk cache is populated and would otherwise hit.
        let mut uncached = Engine::open(
            &dir,
            ConfigOverrides {
                use_cache: false,
                threads: None,
                min_confidence: None,
            },
            vec![Box::new(CacheMockAdapter)],
        )
        .unwrap();
        let result = uncached.check(CheckRequest {
            mode: RunMode::Full,
        });
        assert!(!result.cache_enabled);
        assert_eq!(result.cache_hits, 0);
        assert!(result.to_json().contains("\"cache\": \"cold\""));
    }

    #[test]
    fn open_rejects_missing_root() {
        let err = Engine::open(
            Path::new("/definitely/not/a/dir"),
            ConfigOverrides::default(),
            vec![],
        );
        assert!(err.is_err());
    }

    #[test]
    fn check_on_empty_project_is_clean() {
        let dir = std::env::temp_dir().join("kndo-engine-test-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut engine = Engine::open(&dir, ConfigOverrides::default(), vec![]).unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });
        assert!(result.findings.is_empty());
        assert_eq!(result.files_discovered, 0);
    }

    #[test]
    fn check_counts_discovered_files_with_no_adapters_registered() {
        let dir = std::env::temp_dir().join("kndo-engine-test-files");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ts"), "export const a = 1;").unwrap();
        std::fs::write(dir.join("b.ts"), "export const b = 2;").unwrap();

        let mut engine = Engine::open(&dir, ConfigOverrides::default(), vec![]).unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });
        assert_eq!(result.files_discovered, 2);
        // No adapters registered in this test — files exist as nodes but nothing claims them.
        assert_eq!(result.files_claimed, 0);
    }

    #[test]
    fn full_mode_populates_phase_timings_and_json_omits_them() {
        let dir = std::env::temp_dir().join("kndo-engine-test-timings");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut engine = Engine::open(&dir, ConfigOverrides::default(), vec![]).unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });
        let phases: Vec<&str> = result.timings.iter().map(|(p, _)| p.as_str()).collect();
        assert!(phases.contains(&"assemble"));
        assert!(phases.contains(&"reachability"));
        assert!(phases.contains(&"health"));
        // Wall times are run metadata, never envelope content (determinism matrix compares
        // envelopes byte-for-byte).
        let v: serde_json::Value = serde_json::from_str(&result.to_json()).unwrap();
        assert!(v.get("timings").is_none());
    }

    #[test]
    fn to_json_produces_the_envelope_shape() {
        let dir = std::env::temp_dir().join("kndo-engine-test-json");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut engine = Engine::open(&dir, ConfigOverrides::default(), vec![]).unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });
        let json = result.to_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["schema_version"], SCHEMA_VERSION);
        assert_eq!(value["kndo_version"], KNDO_VERSION);
        assert_eq!(value["run"]["mode"], "full");
        assert_eq!(value["run"]["cache"], "cold");
        assert!(value["run"]["base_ref"].is_null()); // skipped, not just null-valued
        assert!(value["run"]["duration_ms"].is_u64());
        assert!(value["findings"].is_array());
        assert!(value["diagnostics"].is_array());
        // Health is present in full mode, with the documented shape.
        assert!(value["health"]["score"].is_number());
        assert!(value["health"]["grade"].is_string());
        assert!(value["health"]["categories"].is_array());
        // Not yet implemented subsystems must be absent, not fabricated as empty/null.
        assert!(value.get("budget").is_none());
        assert!(value.get("baseline").is_none());
        // Unlike baseline, suppressed is always present — inline pragma matching runs on
        // every check, so "nothing suppressed" is a meaningful zeroed result, not an absent
        // subsystem.
        assert_eq!(
            value["suppressed"],
            serde_json::json!({"inline": 0, "config": 0})
        );
    }

    #[test]
    fn to_json_omits_absent_finding_location_fields() {
        let dir = std::env::temp_dir().join("kndo-engine-test-json-location");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let finding = Finding {
            advisory: false,
            id: "kndo-000000000000".to_string(),
            category: crate::vocab::Category::VERSION_SKEW,
            group: crate::vocab::Group::Defect,
            subject_kind: crate::vocab::SubjectKind::DEPENDENCY,
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: "example".to_string(),
            location: Location::default(),
            related: Vec::new(),
            delta: None,
            delta_origin: None,
        };
        let json = serde_json::to_string(&finding).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["location"], serde_json::json!({}));
        assert_eq!(value["severity"], "warning");
        assert_eq!(value["confidence"], "certain");
    }

    fn git_rev_parse(dir: &std::path::Path, refname: &str) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", refname])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn finding_path(f: &Finding) -> &str {
        f.location.path.as_ref().map(|p| p.0.as_str()).unwrap_or("")
    }

    #[test]
    fn diff_mode_reports_introduced_and_derived_new_findings_plus_fixed() {
        let dir = git_repo("delta-basic");
        std::fs::write(dir.join("root.dmock"), "root-file\nimport ./b.dmock\n").unwrap();
        std::fs::write(dir.join("b.dmock"), "").unwrap();
        std::fs::write(dir.join("orphan.dmock"), "").unwrap(); // already dead at the base
        git_add_all_commit(&dir, "base");
        let base_sha = git_rev_parse(&dir, "HEAD");

        // Uncommitted working-tree changes: root.dmock stops importing b.dmock (b.dmock goes
        // dead — "derived", since b.dmock itself isn't the touched file), starts importing
        // orphan.dmock instead (orphan.dmock comes alive — "fixed"), and a brand new dead file
        // shows up ("introduced" — it's the touched file itself).
        std::fs::write(dir.join("root.dmock"), "root-file\nimport ./orphan.dmock\n").unwrap();
        std::fs::write(dir.join("c.dmock"), "").unwrap();

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Diff { base: base_sha },
        });

        assert!(
            result.diagnostics.iter().all(is_no_test_roots_diagnostic),
            "{:?}",
            result.diagnostics
        );
        assert_eq!(result.mode, "diff");

        let new_b = result
            .findings
            .iter()
            .find(|f| finding_path(f) == "b.dmock")
            .expect("b.dmock should be a new finding");
        assert_eq!(new_b.delta, Some(Delta::New));
        assert_eq!(new_b.delta_origin, Some(DeltaOrigin::Derived));

        let new_c = result
            .findings
            .iter()
            .find(|f| finding_path(f) == "c.dmock")
            .expect("c.dmock should be a new finding");
        assert_eq!(new_c.delta, Some(Delta::New));
        assert_eq!(new_c.delta_origin, Some(DeltaOrigin::Introduced));

        assert_eq!(result.findings.len(), 2, "{:?}", result.findings);

        let fixed_orphan = result
            .fixed
            .iter()
            .find(|f| finding_path(f) == "orphan.dmock")
            .expect("orphan.dmock should be fixed");
        assert_eq!(fixed_orphan.delta, Some(Delta::Fixed));
        assert_eq!(result.fixed.len(), 1, "{:?}", result.fixed);
    }

    #[test]
    fn staged_mode_uses_the_index_not_the_raw_working_tree() {
        let dir = git_repo("staged-index");
        std::fs::write(dir.join("root.dmock"), "root-file\n").unwrap();
        git_add_all_commit(&dir, "base");

        // Stage a new dead file, then make a further UNSTAGED edit to root.dmock — that
        // unstaged edit must not affect the "after" side, which is exactly the index.
        std::fs::write(dir.join("staged.dmock"), "").unwrap();
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["add", "staged.dmock"])
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(
            dir.join("root.dmock"),
            "root-file\nimport ./unstaged.dmock\n",
        )
        .unwrap();
        // `unstaged.dmock` doesn't even exist on disk as a tracked/staged file — if `--staged`
        // leaked the raw working tree in, this import would resolve to nothing new; the real
        // assertion is that `staged.dmock` (and only it) shows up as new.

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Staged,
        });

        assert!(
            result.diagnostics.iter().all(is_no_test_roots_diagnostic),
            "{:?}",
            result.diagnostics
        );
        assert_eq!(result.findings.len(), 1, "{:?}", result.findings);
        assert_eq!(finding_path(&result.findings[0]), "staged.dmock");
    }

    #[test]
    fn diff_mode_outside_a_git_repo_is_an_error_diagnostic_not_a_panic() {
        let dir = std::env::temp_dir().join("kndo-engine-difftest-no-git");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.dmock"), "").unwrap();

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Staged,
        });

        assert!(result.findings.is_empty());
        let err = &result.diagnostics[0];
        assert_eq!(err.level, crate::adapter::DiagnosticLevel::Error);
        assert!(
            err.message.contains("need a git repository"),
            "problem + next step: {}",
            err.message
        );
    }

    /// Running diff mode from a subdirectory of the repo scopes both sides to that
    /// subdirectory with matching project-relative paths — if the "before" side covered
    /// the whole repo (repo-relative paths) while `--diff`'s "after" covered the
    /// subdirectory (subdir-relative paths), paths would never align and everything outside
    /// the subdir would appear removed.
    #[test]
    fn diff_mode_from_a_subdirectory_scopes_both_sides_to_it() {
        let dir = git_repo("subdir-scope");
        std::fs::create_dir_all(dir.join("pkg")).unwrap();
        std::fs::write(dir.join("outside.dmock"), "").unwrap(); // dead, but OUTSIDE the scope
        std::fs::write(
            dir.join("pkg/root.dmock"),
            "root-file\nimport ./used.dmock\n",
        )
        .unwrap();
        std::fs::write(dir.join("pkg/used.dmock"), "").unwrap();
        git_add_all_commit(&dir, "base");
        let base_sha = git_rev_parse(&dir, "HEAD");

        // Working-tree change inside pkg only: stop importing used.dmock.
        std::fs::write(dir.join("pkg/root.dmock"), "root-file\n").unwrap();

        let mut engine = Engine::open(
            &dir.join("pkg"),
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Diff { base: base_sha },
        });

        assert!(
            result.diagnostics.iter().all(is_no_test_roots_diagnostic),
            "{:?}",
            result.diagnostics
        );
        // Exactly one derived new finding, with a pkg-relative path — and nothing about
        // outside.dmock on either side (it is out of scope, not "removed").
        assert_eq!(result.findings.len(), 1, "{:?}", result.findings);
        assert_eq!(finding_path(&result.findings[0]), "used.dmock");
        assert_eq!(result.findings[0].delta_origin, Some(DeltaOrigin::Derived));
        assert!(result.fixed.is_empty(), "{:?}", result.fixed);
    }

    #[test]
    fn baseline_applies_symmetrically_in_diff_mode() {
        let dir = git_repo("delta-baseline");
        std::fs::write(dir.join("root.dmock"), "root-file\n").unwrap();
        std::fs::write(dir.join("orphan.dmock"), "").unwrap();
        git_add_all_commit(&dir, "base");
        let base_sha = git_rev_parse(&dir, "HEAD");

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        // Baseline acknowledges orphan.dmock's finding while it's still present on both sides.
        let baseline_result = engine.baseline(BaselineOp::Create);
        assert!(matches!(
            baseline_result,
            BaselineResult::Written { acknowledged: 1 }
        ));

        // Add a second, unacknowledged dead file — the acknowledged one must not resurface as
        // new or fixed on either side of the diff.
        std::fs::write(dir.join("also-dead.dmock"), "").unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Diff { base: base_sha },
        });

        assert!(
            result.diagnostics.iter().all(is_no_test_roots_diagnostic),
            "{:?}",
            result.diagnostics
        );
        assert!(!result
            .findings
            .iter()
            .any(|f| finding_path(f) == "orphan.dmock"));
        assert!(!result
            .fixed
            .iter()
            .any(|f| finding_path(f) == "orphan.dmock"));
        assert_eq!(result.findings.len(), 1);
        assert_eq!(finding_path(&result.findings[0]), "also-dead.dmock");
    }

    #[test]
    fn inline_suppression_hides_a_finding_from_full_mode_but_still_counts_it() {
        let dir = std::env::temp_dir().join("kndo-engine-test-suppress-full");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("orphan.dmock"), "suppress-file unused\n").unwrap();

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });

        assert!(
            !result
                .findings
                .iter()
                .any(|f| finding_path(f) == "orphan.dmock"),
            "{:?}",
            result.findings
        );
        assert_eq!(result.suppressed.inline, 1);
        assert_eq!(result.suppressed.config, 0);
    }

    #[test]
    fn config_skip_hides_a_finding_and_counts_it_as_config_suppressed() {
        let dir = std::env::temp_dir().join("kndo-engine-test-config-skip");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("orphan.dmock"), "").unwrap();
        std::fs::write(dir.join("kndo.toml"), "[analysis]\nskip = [\"unused\"]\n").unwrap();

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });

        assert!(
            !result.findings.iter().any(|f| f.category == "unused"),
            "{:?}",
            result.findings
        );
        assert_eq!(result.suppressed.config, 1);
        assert_eq!(result.suppressed.inline, 0);
    }

    #[test]
    fn a_pragma_under_a_config_skip_counts_inline_and_never_goes_stale() {
        // The ordering guarantee: pragmas run first, so a finding covered by BOTH
        // mechanisms counts as inline (config never sees it) and the pragma stays
        // honestly non-stale — deleting the config entry could never flicker it.
        let dir = std::env::temp_dir().join("kndo-engine-test-config-plus-pragma");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("orphan.dmock"), "suppress-file unused\n").unwrap();
        std::fs::write(dir.join("kndo.toml"), "[analysis]\nskip = [\"unused\"]\n").unwrap();

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });

        assert!(
            !result.findings.iter().any(|f| f.category == "stale"),
            "the pragma is doing its job — config must not steal the match: {:?}",
            result.findings
        );
        assert_eq!(result.suppressed.inline, 1);
        assert_eq!(result.suppressed.config, 0);
    }

    #[test]
    fn a_path_rule_scopes_its_skip_to_matching_paths() {
        let dir = std::env::temp_dir().join("kndo-engine-test-config-rule");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("gen")).unwrap();
        std::fs::write(dir.join("orphan.dmock"), "").unwrap();
        std::fs::write(dir.join("gen/tool.dmock"), "").unwrap();
        std::fs::write(
            dir.join("kndo.toml"),
            "[[rule]]\npaths = [\"gen/**\"]\nskip = [\"unused\"]\n",
        )
        .unwrap();

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });

        assert!(
            result
                .findings
                .iter()
                .any(|f| finding_path(f) == "orphan.dmock"),
            "outside the rule's paths the finding stands: {:?}",
            result.findings
        );
        assert!(!result
            .findings
            .iter()
            .any(|f| finding_path(f) == "gen/tool.dmock"));
        assert_eq!(result.suppressed.config, 1);
    }

    #[test]
    fn the_confidence_floor_drops_lower_tiers_but_never_stale() {
        let mk = |category: &str, confidence: Confidence| Finding {
            advisory: false,
            id: format!("{category}-{confidence:?}"),
            category: crate::vocab::Category::new(category),
            group: crate::vocab::Group::Waste,
            subject_kind: crate::vocab::SubjectKind::new("function"),
            severity: Severity::Info,
            confidence,
            message: String::new(),
            location: Location {
                path: None,
                range: None,
                symbol: None,
                package: None,
            },
            related: Vec::new(),
            delta: None,
            delta_origin: None,
        };
        let findings = vec![
            mk("unused", Confidence::Possible),
            mk("unused", Confidence::Probable),
            mk("stale", Confidence::Possible),
        ];
        // Possible is the identity floor — the report-everything default.
        assert_eq!(
            apply_confidence_floor(findings.clone(), Confidence::Possible).len(),
            3
        );
        let kept = apply_confidence_floor(findings, Confidence::Probable);
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().any(|f| f.category == "stale"));
        assert!(kept
            .iter()
            .any(|f| f.category == "unused" && f.confidence == Confidence::Probable));
    }

    #[test]
    fn a_matchless_pragma_surfaces_as_a_stale_finding_in_full_mode() {
        let dir = std::env::temp_dir().join("kndo-engine-test-stale-full");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A root file (never `unused`) acknowledging a category it will never produce: the
        // pragma suppresses nothing, so the `stale` rule flags the pragma itself.
        std::fs::write(
            dir.join("root.dmock"),
            "root-file\nsuppress-file version-skew\n",
        )
        .unwrap();

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });

        let stale: Vec<_> = result
            .findings
            .iter()
            .filter(|f| f.category == "stale")
            .collect();
        assert_eq!(stale.len(), 1, "{:?}", result.findings);
        assert_eq!(stale[0].group, crate::vocab::Group::Hygiene);
        assert_eq!(stale[0].subject_kind, "suppression");
        assert_eq!(stale[0].severity, Severity::Info);
        assert_eq!(finding_path(stale[0]), "root.dmock");
        assert!(stale[0].message.contains("version-skew"));
        assert_eq!(result.suppressed.inline, 0);
    }

    #[test]
    fn a_bad_diff_base_is_an_error_level_diagnostic_never_a_clean_empty_pass() {
        // A Warn + empty result would fail open — a typo'd base ref in CI
        // would read as zero findings at exit 0. This is the exit-2 tier, signaled
        // through the one channel every format carries (an error-level diagnostic).
        let dir = git_repo("bad-diff-base");
        std::fs::write(
            dir.join("root.dmock"),
            "root-file
",
        )
        .unwrap();
        git_add_all_commit(&dir, "base");

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Diff {
                base: "no-such-ref".to_string(),
            },
        });

        assert!(result.findings.is_empty());
        let err = result
            .diagnostics
            .iter()
            .find(|d| d.level == crate::adapter::DiagnosticLevel::Error)
            .expect("an error-level diagnostic");
        assert!(err.message.contains("no-such-ref"), "{}", err.message);
        assert!(
            err.message.contains("check the ref name"),
            "problem + next step: {}",
            err.message
        );
    }

    #[test]
    fn a_suppression_present_on_both_sides_of_a_diff_never_surfaces_as_new_or_fixed() {
        let dir = git_repo("delta-suppressed");
        std::fs::write(dir.join("root.dmock"), "root-file\n").unwrap();
        std::fs::write(dir.join("orphan.dmock"), "suppress-file unused\n").unwrap();
        git_add_all_commit(&dir, "base");
        let base_sha = git_rev_parse(&dir, "HEAD");

        // Touch an unrelated file so the diff isn't a total no-op.
        std::fs::write(dir.join("also-dead.dmock"), "").unwrap();

        let mut engine = Engine::open(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Diff { base: base_sha },
        });

        assert!(
            result.diagnostics.iter().all(is_no_test_roots_diagnostic),
            "{:?}",
            result.diagnostics
        );
        assert!(!result
            .findings
            .iter()
            .any(|f| finding_path(f) == "orphan.dmock"));
        assert!(!result
            .fixed
            .iter()
            .any(|f| finding_path(f) == "orphan.dmock"));
        assert_eq!(result.findings.len(), 1);
        assert_eq!(finding_path(&result.findings[0]), "also-dead.dmock");
        // Reported from the "after" side, mirroring baseline's convention.
        assert_eq!(result.suppressed.inline, 1);
    }

    fn query_engine(dir: &std::path::Path) -> Engine {
        Engine::open(
            dir,
            ConfigOverrides::default(),
            vec![Box::new(DiffMockAdapter)],
        )
        .unwrap()
    }

    #[test]
    fn query_find_locates_a_declared_symbol() {
        let dir = std::env::temp_dir().join("kndo-engine-query-find");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("root.dmock"), "root-file\ndecl handler\n").unwrap();

        let mut engine = query_engine(&dir);
        let result = engine.query(crate::query_envelope::QueryRequest {
            id: None,
            verb: crate::query_envelope::Verb::Find,
            selectors: vec!["handler".to_string()],
            flags: crate::query_envelope::QueryFlags::default(),
        });
        assert_eq!(result.status(), "ok");
        let crate::query_envelope::ResultEntry::Find(found) = &result.results[0] else {
            panic!("expected a Find result");
        };
        assert_eq!(found.matches[0].selector, "root.dmock#handler");
    }

    #[test]
    fn query_describe_reports_a_not_found_selector() {
        let dir = std::env::temp_dir().join("kndo-engine-query-not-found");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("root.dmock"), "root-file\n").unwrap();

        let mut engine = query_engine(&dir);
        let result = engine.query(crate::query_envelope::QueryRequest {
            id: Some("q1".to_string()),
            verb: crate::query_envelope::Verb::Describe,
            selectors: vec!["missing.dmock".to_string()],
            flags: crate::query_envelope::QueryFlags::default(),
        });
        assert_eq!(result.status(), "not-found");
        assert!(matches!(
            &result.results[0],
            crate::query_envelope::ResultEntry::Failed {
                status: "not-found",
                ..
            }
        ));
    }

    #[test]
    fn query_used_by_finds_the_importing_file() {
        let dir = std::env::temp_dir().join("kndo-engine-query-used-by");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("root.dmock"), "root-file\nimport ./lib.dmock\n").unwrap();
        std::fs::write(dir.join("lib.dmock"), "").unwrap();

        let mut engine = query_engine(&dir);
        let result = engine.query(crate::query_envelope::QueryRequest {
            id: None,
            verb: crate::query_envelope::Verb::UsedBy,
            selectors: vec!["lib.dmock".to_string()],
            flags: crate::query_envelope::QueryFlags::default(),
        });
        let crate::query_envelope::ResultEntry::Neighbors(n) = &result.results[0] else {
            panic!("expected a Neighbors result");
        };
        assert_eq!(n.entries.len(), 1);
        assert_eq!(n.entries[0].node.selector, "root.dmock");
    }

    #[test]
    fn query_trace_finds_the_liveness_path() {
        let dir = std::env::temp_dir().join("kndo-engine-query-trace");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The mock adapter never populates import bindings, so a cross-file `ref` can't resolve
        // (matches the real js-ts adapter's own binding-driven cross-file resolution — this is
        // a same-file reference instead, which the mock's `symbol_by_name_per_file` fallback can
        // resolve on its own).
        std::fs::write(dir.join("root.dmock"), "root-file\ndecl bar\nref bar\n").unwrap();

        let mut engine = query_engine(&dir);
        let result = engine.query(crate::query_envelope::QueryRequest {
            id: None,
            verb: crate::query_envelope::Verb::Trace,
            selectors: vec!["root.dmock#bar".to_string()],
            flags: crate::query_envelope::QueryFlags::default(),
        });
        let crate::query_envelope::ResultEntry::Trace(t) = &result.results[0] else {
            panic!("expected a Trace result");
        };
        assert_eq!(t.from.selector, "roots:production");
        assert_eq!(t.paths.len(), 1);
    }

    #[test]
    fn query_batch_shares_one_graph_load_and_aligns_results_with_requests() {
        let dir = std::env::temp_dir().join("kndo-engine-query-batch");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("root.dmock"), "root-file\ndecl foo\ndecl bar\n").unwrap();

        let mut engine = query_engine(&dir);
        let results = engine.query_batch(vec![
            crate::query_envelope::QueryRequest {
                id: Some("q1".to_string()),
                verb: crate::query_envelope::Verb::Find,
                selectors: vec!["foo".to_string()],
                flags: crate::query_envelope::QueryFlags::default(),
            },
            crate::query_envelope::QueryRequest {
                id: Some("q2".to_string()),
                verb: crate::query_envelope::Verb::Find,
                selectors: vec!["bar".to_string()],
                flags: crate::query_envelope::QueryFlags::default(),
            },
        ]);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id.as_deref(), Some("q1"));
        assert_eq!(results[1].id.as_deref(), Some("q2"));
        let crate::query_envelope::ResultEntry::Find(f1) = &results[0].results[0] else {
            panic!("expected Find");
        };
        assert_eq!(f1.matches[0].selector, "root.dmock#foo");
        let crate::query_envelope::ResultEntry::Find(f2) = &results[1].results[0] else {
            panic!("expected Find");
        };
        assert_eq!(f2.matches[0].selector, "root.dmock#bar");
    }

    /// Minimal coverage ingester for host-side tests — core ships no format parsers
    /// (they live in `kndo-plugin-coverage`), so ingestion tests bring their own.
    /// Format: one `path line hits` triple per line.
    struct MockCoverageIngester;
    impl crate::plugin::Plugin for MockCoverageIngester {
        fn descriptor(&self) -> crate::plugin::PluginDescriptor {
            crate::plugin::PluginDescriptor {
                id: SmolStr::new("kndo:coverage-mock"),
                version: SmolStr::new("1"),
                detection: vec![],
                requested_file_access: vec![SmolStr::new("lcov.info")],
                activation: vec![],
                dependencies: vec![],
            }
        }
        fn mutates_graph(&self) -> bool {
            false
        }
        fn ingest_coverage(
            &self,
            _path: &crate::adapter::ProjectPath,
            content: &[u8],
            out: &mut crate::coverage::CoverageSink,
        ) {
            for line in String::from_utf8_lossy(content).lines() {
                let mut parts = line.split_whitespace();
                if let (Some(path), Some(l), Some(h)) = (parts.next(), parts.next(), parts.next()) {
                    if let (Ok(l), Ok(h)) = (l.parse(), h.parse()) {
                        out.add_line(crate::adapter::ProjectPath(SmolStr::new(path)), l, h);
                    }
                }
            }
        }
    }

    fn backdate(path: &Path, days: u64) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(
            std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400),
        )
        .unwrap();
    }

    #[test]
    fn expand_report_pattern_stats_literals_and_walks_globs_sorted() {
        let dir = std::env::temp_dir().join("kndo-engine-test-expand-report");
        let _ = std::fs::remove_dir_all(&dir);
        for package in ["b", "a"] {
            let cov = dir.join("packages").join(package).join("coverage");
            std::fs::create_dir_all(&cov).unwrap();
            std::fs::write(cov.join("lcov.info"), "x").unwrap();
        }
        assert!(expand_report_pattern(&dir, "lcov.info").is_empty());
        let matches = expand_report_pattern(&dir, "packages/*/coverage/lcov.info");
        let rels: Vec<&str> = matches.iter().map(|(_, rel)| rel.as_str()).collect();
        assert_eq!(
            rels,
            vec![
                "packages/a/coverage/lcov.info",
                "packages/b/coverage/lcov.info"
            ]
        );
        let literal = expand_report_pattern(&dir, "packages/a/coverage/lcov.info");
        assert_eq!(literal.len(), 1);
    }

    #[test]
    fn rebase_by_packages_moves_only_unmatched_keys_with_a_unique_target() {
        use crate::coverage::CoverageSink;
        let mut graph = crate::graph::ProjectGraph::default();
        graph.packages.push(crate::graph::PackageNode {
            manifest: Some(crate::adapter::ProjectPath(SmolStr::new("go.mod"))),
            name: Some(SmolStr::new("github.com/x/y")),
            private: false,
            declares_surface: false,
            surface: vec![],
            workspace_entry: None,
            targets: vec![],
            executables: vec![],
            resolves_dependency_usage: false,
        });
        let file = |path: &str| crate::graph::FileNode {
            path: crate::adapter::ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: None,
            class: None,
            package: crate::vocab::PackageId(0),
            unit: None,
            test_spans: vec![],
            string_call_sites: vec![],
        };
        graph.files.push(file("pkg/a.go"));
        graph.files.push(file("github.com/x/y/pkg/b.go")); // pathological: matches as-is
        let mut sink = CoverageSink::default();
        for key in [
            "github.com/x/y/pkg/a.go",     // unmatched, unique target -> moves
            "github.com/x/y/pkg/b.go",     // matches the graph verbatim -> stays
            "github.com/other/z/pkg/c.go", // no mapping applies -> stays
        ] {
            sink.add_line(crate::adapter::ProjectPath(SmolStr::new(key)), 1, 1);
        }
        let mut map = sink.into_map();
        rebase_by_packages(&mut map, &graph);
        let has = |path: &str| {
            map.function_coverage(
                &crate::adapter::ProjectPath(SmolStr::new(path)),
                crate::adapter::Span {
                    start: (1, 1),
                    end: (5, 1),
                },
            )
            .is_some()
        };
        assert!(
            has("pkg/a.go"),
            "module-qualified key lands on the graph file"
        );
        assert!(
            has("github.com/x/y/pkg/b.go"),
            "a key already matching a graph file is never rewritten"
        );
        assert!(
            has("github.com/other/z/pkg/c.go"),
            "unmapped keys stay verbatim"
        );
    }

    #[test]
    fn configured_report_glob_replaces_well_known_paths_and_finds_monorepo_reports() {
        let dir = std::env::temp_dir().join("kndo-engine-test-cov-glob");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.mock"), "decl covered\n").unwrap();
        // Reports are normally gitignored — glob expansion must not depend on discovery.
        std::fs::write(dir.join(".gitignore"), "coverage/\n").unwrap();
        for package in ["a", "b"] {
            let cov = dir.join("packages").join(package).join("coverage");
            std::fs::create_dir_all(&cov).unwrap();
            std::fs::write(cov.join("lcov.info"), "a.mock 1 1\n").unwrap();
        }
        // A stale report at the well-known path: replace semantics means it is never
        // visited — no freshness warning about it may appear.
        std::fs::write(dir.join("lcov.info"), "a.mock 1 1\n").unwrap();
        backdate(&dir.join("lcov.info"), 30);
        std::fs::write(
            dir.join("kndo.toml"),
            "[plugins.coverage-mock]\nreport = \"packages/*/coverage/lcov.info\"\n",
        )
        .unwrap();
        let mut engine = Engine::open_with_plugins(
            &dir,
            ConfigOverrides::default(),
            vec![Box::new(CacheMockAdapter)],
            vec![Box::new(MockCoverageIngester)],
        )
        .unwrap();
        let result = engine.check(CheckRequest {
            mode: RunMode::Full,
        });
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.message.starts_with("crap: no coverage ingested")),
            "the glob-configured monorepo reports must be ingested: {:?}",
            result.diagnostics
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("ignored")),
            "the stale well-known report is replaced, not visited: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn max_age_gates_by_default_and_is_overridable_per_plugin() {
        let stale_days = 10;
        let fixture = |name: &str, config: &str| {
            let dir = std::env::temp_dir().join(format!("kndo-engine-test-maxage-{name}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("a.mock"), "decl covered\n").unwrap();
            std::fs::write(dir.join("lcov.info"), "a.mock 1 1\n").unwrap();
            backdate(&dir.join("lcov.info"), stale_days);
            if !config.is_empty() {
                std::fs::write(dir.join("kndo.toml"), config).unwrap();
            }
            let mut engine = Engine::open_with_plugins(
                &dir,
                ConfigOverrides::default(),
                vec![Box::new(CacheMockAdapter)],
                vec![Box::new(MockCoverageIngester)],
            )
            .unwrap();
            engine.check(CheckRequest {
                mode: RunMode::Full,
            })
        };
        let default = fixture("default", "");
        assert!(
            default
                .diagnostics
                .iter()
                .any(|d| d.message.contains("ignored: 10 days old")),
            "{:?}",
            default.diagnostics
        );
        let widened = fixture("widened", "[plugins.coverage-mock]\nmax-age = \"30d\"\n");
        assert!(
            !widened
                .diagnostics
                .iter()
                .any(|d| d.message.contains("ignored")),
            "{:?}",
            widened.diagnostics
        );
        assert!(
            !widened
                .diagnostics
                .iter()
                .any(|d| d.message.starts_with("crap: no coverage ingested")),
            "a widened max-age ingests the stale-by-default report: {:?}",
            widened.diagnostics
        );
    }
}
