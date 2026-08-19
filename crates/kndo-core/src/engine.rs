//! The `Engine` facade — the only surface frontends may touch (contracts §5).
//!
//! Separation rules, enforced by dependency direction: the core contains no terminal concerns
//! (no ANSI, no TTY detection, no exit codes, no stdout) — it returns data and never prints;
//! frontends contain no analysis concerns — they cannot reach the graph, cache, or adapters
//! except through this type. A frontend that needs a new fact is a core PR adding it to
//! [`RunResult`], never a core import.
//!
//! Adapter *registration* is the **distribution layer's** concern (the `kndo` crate): the
//! core never knows which languages exist (RFC 0001 §2, the ignorance rule), and frontends
//! never compose the product — they call `kndo::open`, which passes the registry in here.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::adapter::{Diagnostic, DiagnosticLevel, LanguageAdapter, ProjectPath, Span};
use crate::analysis;
use crate::gitutil;
use crate::graph;
use crate::vocab::Confidence;

/// The set of paths that differ (added, removed, or content-changed) between two graphs' file
/// sets — "the change set," in RFC 0004 §6's terms, at file granularity. Feeds diff mode's
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

    let mut touched = HashSet::new();
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

/// Mirrors the output schema's `schema_version` (contracts/output-schema.md).
pub const SCHEMA_VERSION: &str = "1.0.0";

/// The product version — every crate shares `version.workspace = true`, so kndo-core's own
/// `CARGO_PKG_VERSION` is the same string the distribution crate and CLI would report.
pub const KNDO_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct ConfigOverrides {
    /// `--no-cache` (RFC 0004 §4): disables the facts cache entirely for this run. Defaults to
    /// `true` — the correctness gate is that this must never change *findings*, only whether
    /// the run was warm.
    pub use_cache: bool,
}

impl Default for ConfigOverrides {
    fn default() -> Self {
        ConfigOverrides { use_cache: true }
    }
}

#[derive(Debug)]
pub enum EngineError {
    ProjectRootNotFound(PathBuf),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::ProjectRootNotFound(p) => {
                write!(
                    f,
                    "project root does not exist or is not a directory: {}",
                    p.display()
                )
            }
        }
    }
}

impl std::error::Error for EngineError {}

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

/// `kndo baseline`'s two modes (RFC 0006 §6, contracts §5's `Engine::baseline`): `Create`
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

/// One registered adapter, as `kndo doctor` reports it (RFC 0006 §2's "what was detected:
/// adapters…") — static descriptor info, not tied to any particular run.
#[derive(Debug, Clone)]
pub struct DoctorAdapterInfo {
    pub id: String,
    pub grammar_version: String,
    pub file_globs: Vec<String>,
    pub manifest_globs: Vec<String>,
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
    pub graph_snapshot_present: bool,
    pub graph_snapshot_bytes: u64,
}

/// `kndo doctor` (RFC 0006 §2, contracts §5's `Engine::doctor`): everything detected about this
/// project, without running a check — read-only and instant, so it stays useful for debugging a
/// setup that itself might be slow or broken. `plugins` is always empty: the plugin system is
/// internal-only pre-1.0 (RFC 0003 §6) and `Engine` doesn't wire any in yet — an honest gap, not
/// an omission to paper over with a placeholder.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub project_root: String,
    pub adapters: Vec<DoctorAdapterInfo>,
    pub cache_enabled: bool,
    pub cache: Option<DoctorCacheInfo>,
    pub baseline_present: bool,
    pub baseline_entries: usize,
}

/// A finding's severity (contracts/output-schema.md §2) — RFC 0005 assigns one per category as
/// a fixed default; `--strict` promotion isn't implemented yet, so this is always the default.
/// Declaration order doubles as sort/triage order: worst first (RFC 0009 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// Where a finding landed (contracts/output-schema.md §2). Every field is optional because not
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

/// A finding's place in a diff-mode delta (contracts/output-schema.md §2, RFC 0004 §6). `None`
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
/// RFC 0004 §6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum DeltaOrigin {
    Introduced,
    Derived,
}

/// Typed form of the output-schema finding (grows field-by-field with the analyses in M1;
/// every field lands in the JSON schema first — that document is normative). Not yet present:
/// `related` (evidence chain), `evidence` (category-specific block), `sources`, `remediation`,
/// `rolled_up` — each needs infrastructure this milestone doesn't have (an evidence model,
/// computed remediation text) and is omitted rather than fabricated with a placeholder.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Finding {
    pub id: String,
    pub category: String,
    pub group: String,
    pub subject_kind: String,
    pub severity: Severity,
    pub confidence: Confidence,
    pub message: String,
    pub location: Location,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<Delta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_origin: Option<DeltaOrigin>,
}

/// One registered adapter's contribution (`run.adapters[]`, output-schema §1).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AdapterRunInfo {
    pub id: String,
    pub files: usize,
}

/// `baseline` summary (contracts/output-schema.md §1's `baseline` envelope field, RFC 0006 §6):
/// `acknowledged` counts baseline entries that still match a current finding (excluded from
/// `findings` and from `--fail-on`); `stale` counts entries that match nothing anymore — the
/// underlying issue was fixed, and `kndo baseline --update` would drop them.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BaselineSummary {
    pub acknowledged: usize,
    pub stale: usize,
}

/// Typed form of the output-schema envelope. JSON/SARIF/agent serializers live core-side so
/// every frontend emits byte-identical machine output; *human* rendering is frontend-owned
/// (RFC 0009). Flat here for ergonomic Rust consumption; [`RunResult::to_json`] nests it into
/// the schema's actual shape. Not yet present: `health`, `budget`, `suppressed` — neither
/// subsystem exists yet (health/CRAP scoring is M4; inline suppression pragmas need adapter-side
/// grammar work not yet done), so those fields are omitted rather than emitted empty/null.
/// Adding them later is additive (minor schema bump, RFC 0006 §4), not a breaking change.
#[derive(Debug, Default)]
pub struct RunResult {
    /// Full mode: every finding. Diff modes: only *new* findings (contracts/output-schema.md
    /// §1) — findings that disappeared belong in `fixed` below, not here.
    pub findings: Vec<Finding>,
    /// Diff modes only (RFC 0004 §6, output-schema §3): findings present in the "before" tree
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
    /// its very first run and is still, correctly, cold (RFC 0004 §2).
    pub cache_enabled: bool,
    pub cache_hits: u64,
    /// `None` when `.kndo/baseline.json` doesn't exist (RFC 0006 §6) — distinct from `Some`
    /// with zero counts, which means a baseline exists and is fully clean/reproducing.
    pub baseline: Option<BaselineSummary>,
}

impl RunResult {
    /// `"warm"` only when the cache was on *and* actually served something this run — an
    /// enabled-but-empty cache (first run ever, or every file changed) is honestly `"cold"`
    /// (RFC 0004 §2). Shared by every renderer (`to_json`, `to_agent_format`) so "what counts as
    /// warm" is defined exactly once.
    pub fn cache_status(&self) -> &'static str {
        if self.cache_enabled && self.cache_hits > 0 {
            "warm"
        } else {
            "cold"
        }
    }
}

/// Owned mirror of the JSON envelope's `run` object — not borrowed, unlike a hot-path type,
/// because this exists purely to be serialized (and, behind `schema`, to derive the JSON
/// Schema from): the one-time clone per `--format json` invocation is free by comparison.
#[derive(serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct RunInfo {
    mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_ref: Option<String>,
    started_at: String,
    duration_ms: u64,
    cache: &'static str,
    project_root: String,
    adapters: Vec<AdapterRunInfo>,
}

/// The full `--format json` envelope shape (contracts/output-schema.md §1) — also the schema
/// generator's root type (`cargo xtask gen-schema`, gated behind the `schema` feature): the
/// JSON Schema is derived from this struct, not maintained as a second hand-written document.
#[derive(serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
struct Envelope {
    schema_version: &'static str,
    kndo_version: &'static str,
    run: RunInfo,
    findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fixed: Vec<Finding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline: Option<BaselineSummary>,
    diagnostics: Vec<Diagnostic>,
}

impl RunResult {
    fn to_envelope(&self) -> Envelope {
        Envelope {
            schema_version: SCHEMA_VERSION,
            kndo_version: KNDO_VERSION,
            run: RunInfo {
                mode: self.mode.clone(),
                base_ref: self.base_ref.clone(),
                started_at: self.started_at.clone(),
                duration_ms: self.duration_ms,
                cache: self.cache_status(),
                project_root: self.project_root.clone(),
                adapters: self.adapters.clone(),
            },
            findings: self.findings.clone(),
            fixed: self.fixed.clone(),
            baseline: self.baseline.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }

    /// The `--format json` rendering (contracts/output-schema.md §1) — serialized core-side so
    /// every frontend emits byte-identical machine output (RFC 0001 §2, contracts §5).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.to_envelope())
            .unwrap_or_else(|e| format!("{{\"error\": \"failed to serialize output: {e}\"}}"))
    }

    /// The `--format agent` rendering (contracts/output-schema.md §9) — like JSON, serialized
    /// core-side so every frontend emits byte-identical agent text.
    pub fn to_agent_format(&self) -> String {
        crate::agent_format::render(self)
    }
}

/// The `--format json` envelope's JSON Schema, derived from [`Envelope`] itself — never a
/// second hand-written document (contracts/output-schema.md's normative promise). Dev-time
/// only: regenerate the committed copy with `cargo xtask gen-schema`.
#[cfg(feature = "schema")]
pub fn json_schema() -> schemars::Schema {
    schemars::schema_for!(Envelope)
}

/// Synchronous and single-instance-per-project (the cache lock, RFC 0004 §7); a serving
/// frontend wraps it in its own concurrency model.
pub struct Engine {
    root: PathBuf,
    adapters: Vec<Box<dyn LanguageAdapter>>,
    cache: Option<crate::cache::ProjectCache>,
    cache_enabled: bool,
}

impl Engine {
    /// `adapters` is the registered language set, composed by the distribution layer (the
    /// `kndo` crate) — compiled-in first-party adapters today, WASM-bridged third-party
    /// adapters later (ADR 0003). The core never selects or knows about them beyond the
    /// trait; embedders and tests may pass a custom set directly.
    pub fn open(
        root: &Path,
        overrides: ConfigOverrides,
        adapters: Vec<Box<dyn LanguageAdapter>>,
    ) -> Result<Engine, EngineError> {
        if !root.is_dir() {
            return Err(EngineError::ProjectRootNotFound(root.to_path_buf()));
        }
        let cache = overrides
            .use_cache
            .then(|| crate::cache::ProjectCache::open(root));
        Ok(Engine {
            root: root.to_path_buf(),
            adapters,
            cache,
            cache_enabled: overrides.use_cache,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `kndo doctor` (RFC 0006 §2, contracts §5). Deliberately does not assemble or analyze
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
                }
            })
            .collect();

        let cache = self.cache.as_ref().map(|c| {
            let stats = c.stats();
            DoctorCacheInfo {
                writable: stats.writable,
                facts_entries: stats.facts_entries,
                facts_bytes: stats.facts_bytes,
                graph_snapshot_present: stats.graph_snapshot_present,
                graph_snapshot_bytes: stats.graph_snapshot_bytes,
            }
        });

        let baseline_entries = crate::baseline::load(&self.root);
        DoctorReport {
            project_root: self.root.display().to_string(),
            adapters,
            cache_enabled: self.cache_enabled,
            cache,
            baseline_present: baseline_entries.is_some(),
            baseline_entries: baseline_entries.map(|e| e.len()).unwrap_or(0),
        }
    }

    /// Full mode reports every current finding; `--staged`/`--diff <ref>` report the RFC 0004
    /// §6 derived-effects delta instead — see [`Self::run_diff`].
    pub fn check(&mut self, req: CheckRequest) -> RunResult {
        let start = Instant::now();
        let started_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mode = req.mode.as_str().to_string();
        let base_ref = req.mode.base_ref();
        let project_root = self.root.display().to_string();

        let outcome = match &req.mode {
            RunMode::Full => {
                let root = self.root.clone();
                let raw = self.run_analysis_at(&root);
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

    /// RFC 0004 §6's derived-effects delta: assemble the graph at two tree states and report
    /// `(findings_after − findings_before) ∪ (findings_before − findings_after)`, each finding
    /// tagged `delta: New|Fixed` (and, for `New`, `delta_origin: Introduced|Derived` — whether
    /// it sits inside a *touched* file or was flipped at a distance in untouched code, per the
    /// RFC's own example). Both sides run through baseline filtering symmetrically before the
    /// diff, so an acknowledged issue never surfaces as new or fixed on either side.
    ///
    /// Tree states, since the RFC's prose doesn't spell out exact git semantics and this is a
    /// deliberate reading of it: `--staged`'s "after" is the **index**, not the raw working
    /// tree — exactly what would be committed, excluding further unstaged edits on top (the
    /// pre-commit use case `kndo init --hook` installs wants precisely this). `--staged`'s
    /// "before" is `HEAD`. `--diff <ref>`'s "before" is `merge-base(<ref>, HEAD)`; "after" is
    /// the real working tree as-is (uncommitted changes included) — no materialization needed,
    /// it's just `self.root`.
    fn run_diff(&mut self, mode: &RunMode) -> RunResult {
        let git_root = match gitutil::repo_root(&self.root) {
            Ok(r) => r,
            Err(e) => return Self::git_failure(e),
        };

        let before_treeish = match mode {
            RunMode::Staged => gitutil::rev_parse(&git_root, "HEAD"),
            RunMode::Diff { base } => gitutil::merge_base(&git_root, base, "HEAD"),
            RunMode::Full => unreachable!("run_diff is only called for Staged/Diff"),
        };
        let before_treeish = match before_treeish {
            Ok(t) => t,
            Err(e) => return Self::git_failure(e),
        };
        let before_dir = match gitutil::materialize(&git_root, &before_treeish) {
            Ok(d) => d,
            Err(e) => return Self::git_failure(e),
        };

        // `--staged` needs a second materialization (the index); `--diff` reuses the real root.
        // Owned, not borrowed: `assemble_and_analyze` needs `&mut self` right after, which an
        // active borrow of `self.root` would block.
        let staged_after_dir;
        let after_root: PathBuf = match mode {
            RunMode::Staged => {
                let index_tree = match gitutil::write_tree(&git_root) {
                    Ok(t) => t,
                    Err(e) => return Self::git_failure(e),
                };
                staged_after_dir = match gitutil::materialize(&git_root, &index_tree) {
                    Ok(d) => d,
                    Err(e) => return Self::git_failure(e),
                };
                staged_after_dir.path().to_path_buf()
            }
            RunMode::Diff { .. } => self.root.clone(),
            RunMode::Full => unreachable!("run_diff is only called for Staged/Diff"),
        };

        let (before_graph, before_findings, before_diagnostics) =
            match self.assemble_and_analyze(before_dir.path()) {
                Ok(t) => t,
                Err(d) => {
                    return RunResult {
                        diagnostics: vec![d],
                        ..RunResult::default()
                    }
                }
            };
        let (after_graph, after_findings, after_diagnostics) =
            match self.assemble_and_analyze(&after_root) {
                Ok(t) => t,
                Err(d) => {
                    return RunResult {
                        diagnostics: vec![d],
                        ..RunResult::default()
                    }
                }
            };

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
            ..RunResult::default()
        }
    }

    fn git_failure(e: gitutil::GitError) -> RunResult {
        RunResult {
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Warn,
                path: None,
                message: format!("diff mode unavailable: {e}"),
                span: None,
            }],
            ..RunResult::default()
        }
    }

    /// `kndo baseline [--update]` (RFC 0006 §6, contracts §5). Snapshots the complete, current
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

    /// The lowest-level shared step: assemble the graph rooted at an arbitrary directory (the
    /// real project root for full mode; a git-materialized temp directory for diff modes'
    /// "before", and `--staged`'s "after") and run every analysis over it. `self.cache` is
    /// still the *real* project's `.kndo/cache/` regardless of `root` — the facts layer keys
    /// purely by content hash, so it's fully shared across trees; the graph-snapshot layer's
    /// key folds in the whole file set, so a differing tree just misses cleanly rather than
    /// colliding with the real project's own cached graph.
    fn assemble_and_analyze(
        &mut self,
        root: &Path,
    ) -> Result<(graph::ProjectGraph, Vec<Finding>, Vec<Diagnostic>), Diagnostic> {
        match graph::assemble_with_cache(root, &self.adapters, self.cache.as_ref()) {
            Ok((g, diagnostics)) => {
                let findings = analysis::run_all(&g);
                Ok((g, findings, diagnostics))
            }
            Err(crate::discovery::DiscoveryError::Root(e)) => Err(Diagnostic {
                level: DiagnosticLevel::Warn,
                path: None,
                message: format!(
                    "cannot walk the project root: {e} — check the path and permissions"
                ),
                span: None,
            }),
        }
    }

    /// Full-mode `RunResult` construction — assemble + analyze at `root`, plus the run counters
    /// (`files_discovered`, `symbols`, …) that only full mode reports directly (diff mode
    /// builds its own `RunResult` in [`Self::run_diff`], from the "after" side).
    fn run_analysis_at(&mut self, root: &Path) -> RunResult {
        match self.assemble_and_analyze(root) {
            Ok((g, findings, diagnostics)) => {
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
                    ..RunResult::default()
                }
            }
            Err(d) => RunResult {
                diagnostics: vec![d],
                ..RunResult::default()
            },
        }
    }

    /// Partitions `findings` against `.kndo/baseline.json` (RFC 0006 §6): a matched entry is
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
    use smol_str::SmolStr;

    /// Bare-minimum adapter claiming `.mock` files — engine.rs can't depend on a real adapter
    /// crate (that would invert the layering the ignorance rule protects), but a cache-warmth
    /// test needs *something* to extract, or every file stays factless and nothing is ever
    /// cached.
    struct CacheMockAdapter;

    impl LanguageAdapter for CacheMockAdapter {
        fn descriptor(&self) -> crate::adapter::AdapterDescriptor {
            crate::adapter::AdapterDescriptor {
                id: SmolStr::new("mock"),
                facts_schema_version: 1,
                file_globs: vec![SmolStr::new("**/*.mock")],
                manifest_globs: vec![],
                grammar_version: SmolStr::new("mock"),
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
    /// production root) and `import ./sibling.dmock` (an `ImportsFile` edge), which is enough
    /// to drive `unused` (file-level) — exactly what the diff-mode tests below need to produce
    /// real new/fixed findings across two tree states.
    struct DiffMockAdapter;

    impl LanguageAdapter for DiffMockAdapter {
        fn descriptor(&self) -> crate::adapter::AdapterDescriptor {
            crate::adapter::AdapterDescriptor {
                id: SmolStr::new("dmock"),
                facts_schema_version: 1,
                file_globs: vec![SmolStr::new("**/*.dmock")],
                manifest_globs: vec![],
                grammar_version: SmolStr::new("dmock"),
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
            ConfigOverrides { use_cache: false },
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
        // Not yet implemented subsystems must be absent, not fabricated as empty/null.
        assert!(value.get("health").is_none());
        assert!(value.get("budget").is_none());
        assert!(value.get("baseline").is_none());
    }

    #[test]
    fn to_json_omits_absent_finding_location_fields() {
        let dir = std::env::temp_dir().join("kndo-engine-test-json-location");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let finding = Finding {
            id: "kndo-000000000000".to_string(),
            category: "version-skew".to_string(),
            group: "defect".to_string(),
            subject_kind: "dependency".to_string(),
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: "example".to_string(),
            location: Location::default(),
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

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
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

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.findings.len(), 1, "{:?}", result.findings);
        assert_eq!(finding_path(&result.findings[0]), "staged.dmock");
    }

    #[test]
    fn diff_mode_outside_a_git_repo_degrades_to_a_diagnostic_not_a_panic() {
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
        assert!(!result.diagnostics.is_empty());
        assert!(result.diagnostics[0].message.contains("diff mode"));
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

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
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
}
