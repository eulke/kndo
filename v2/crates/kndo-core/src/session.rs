//! Session/Snapshot: the engine produces immutable values. `Session` owns
//! configuration (and, as milestones land, the cache lock and effects); `Snapshot` is
//! the result — queries, reports and gates run on it. `analyze` returns
//! `Result<Snapshot, Refusal>`: the run either happened or was refused, and the
//! refusal reappears inside [`RunOutcome`] so `exit_code` covers that path too.

use crate::analysis::{Abstention, Duplicate, InternalOnly, TestOnly, Untested, Unused, run_all};
use crate::cache::EvidenceCache;
use crate::conduct::Contribution;
use crate::graph::Graph;
use crate::report::{ExtensionRun, REPORT_SCHEMA, Report, ReportDiagnostic, RunInfo};
use crate::{discover, extract};
use kndo_contract::evidence::DiagnosticLevel;
use kndo_contract::extension::Extension;
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::vocab::ProjectPath;
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Threads {
    Auto,
    Count(usize),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub threads: Threads,
    pub use_cache: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            threads: Threads::Auto,
            use_cache: true,
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Full,
}

/// The run could not happen at all. Everything less than this degrades into
/// diagnostics inside a successful run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("project root not found: {0}")]
    RootNotFound(PathBuf),
    #[error("could not configure the thread pool: {0}")]
    ThreadPool(String),
}

pub struct Session {
    root: PathBuf,
    config: Config,
    extensions: Vec<Box<dyn Extension>>,
    composition_diagnostics: Vec<crate::report::ReportDiagnostic>,
}

/// Wall-clock per pipeline phase. Lives BESIDE the report, never inside it: the
/// [`Report`] is compared byte-for-byte by the equivalence gates and diffed by users,
/// so run-varying metadata stays out of the envelope — frontends render these
/// (verbose/human output, serve's own metadata channel) and the bench harness measures
/// around `analyze()` with its per-machine baseline.
///
/// On a surgically patched run, re-extraction of the changed files happens inside the
/// patch, so `extract` reads zero and that work lands in `assemble`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhaseTimings {
    pub discover: Duration,
    pub claim: Duration,
    pub extract: Duration,
    pub assemble: Duration,
    pub analyze: Duration,
}

impl PhaseTimings {
    pub fn total(&self) -> Duration {
        self.discover + self.claim + self.extract + self.assemble + self.analyze
    }
}

pub struct Snapshot {
    pub graph: Graph,
    /// Current findings, post-suppression — plugin findings included, under their
    /// namespaced categories. The baseline split (new vs known) is the report's and
    /// the gate's view; the snapshot keeps the whole truth.
    pub findings: Vec<Finding>,
    pub abstained: Vec<Abstention>,
    pub suppressed: crate::suppress::SuppressedSummary,
    /// What each active plugin asserted, in registration order — always reported,
    /// even when everything applied cleanly.
    pub contributions: Vec<Contribution>,
    pub timings: PhaseTimings,
    baseline: Option<Vec<Finding>>,
    pragma_problems: Vec<crate::suppress::PragmaProblem>,
    composition_diagnostics: Vec<ReportDiagnostic>,
    files_discovered: u32,
}

impl Snapshot {
    /// Findings the baseline does not already carry — what the gate counts and the
    /// report lists.
    pub fn new_findings(&self) -> impl Iterator<Item = &Finding> {
        let known: std::collections::BTreeSet<&str> = self
            .baseline
            .iter()
            .flatten()
            .map(|f| f.id.as_str())
            .collect();
        self.findings
            .iter()
            .filter(move |f| !known.contains(f.id.as_str()))
    }
}

pub struct GatePolicy {
    pub fail_on: Option<Severity>,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    Pass,
    FailFindings {
        at_or_above: u32,
    },
    /// The run never happened; `exit_code` covers this path too, so a frontend maps
    /// `analyze()`'s `Err` through here instead of inventing its own code.
    Refused(Refusal),
}

impl RunOutcome {
    pub fn exit_code(&self) -> i32 {
        match self {
            RunOutcome::Pass => 0,
            RunOutcome::FailFindings { .. } => 1,
            RunOutcome::Refused(_) => 2,
        }
    }
}

impl Session {
    /// One list, one door: registration order is claim priority among claiming
    /// extensions, and — among conduct-declaring ones — coverage-ingestion
    /// precedence and contribution order alike.
    pub fn open(
        root: impl Into<PathBuf>,
        config: Config,
        extensions: Vec<Box<dyn Extension>>,
    ) -> Result<Session, Refusal> {
        let root = root.into();
        if !root.is_dir() {
            return Err(Refusal::RootNotFound(root));
        }
        Ok(Session {
            root,
            config,
            extensions,
            composition_diagnostics: Vec::new(),
        })
    }

    /// Diagnostics from assembling this session's composition — a component that
    /// failed to load, a rejected coordinate. They ride every report the session
    /// produces: an opted-in component that silently vanished would be the one
    /// failure the honesty channel exists to prevent.
    pub fn with_composition_diagnostics(
        mut self,
        diagnostics: Vec<crate::report::ReportDiagnostic>,
    ) -> Self {
        self.composition_diagnostics = diagnostics;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Everything that could change how the same tree assembles: the contract
    /// fingerprint, the graph semantics, and the full adapter set as data.
    fn graph_cache_key(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(&kndo_contract::contract_fingerprint());
        h.update(&crate::graph::GRAPH_SEMANTICS_VERSION.to_le_bytes());
        for extension in &self.extensions {
            let spec = serde_json::to_string(extension.spec()).unwrap_or_default();
            h.update(&(spec.len() as u32).to_le_bytes());
            h.update(spec.as_bytes());
        }
        *h.finalize().as_bytes()
    }

    pub fn analyze(&self, _mode: RunMode) -> Result<Snapshot, Refusal> {
        let threads = match self.config.threads {
            Threads::Auto => 0,
            Threads::Count(n) => n.max(1),
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .map_err(|e| Refusal::ThreadPool(e.to_string()))?;
        Ok(pool.install(|| self.pipeline()))
    }

    fn pipeline(&self) -> Snapshot {
        let mut timings = PhaseTimings::default();
        let timed = |slot: &mut Duration, f: &mut dyn FnMut()| {
            let t = Instant::now();
            f();
            *slot = t.elapsed();
        };

        let mut files = Vec::new();
        timed(&mut timings.discover, &mut || {
            files = discover::discover(&self.root);
        });

        let mut claims = Vec::new();
        timed(&mut timings.claim, &mut || {
            claims = extract::claim(&files, &self.extensions);
        });

        // Activation is decided before the graph exists — its inputs are what
        // discovery and the manifest pass already know — because the cache decision
        // hangs on it: any ACTIVE graph-mutating plugin bypasses the persisted graph
        // entirely (the surgical patch never re-invokes plugin hooks, so it could
        // never safely reuse a graph one influenced). The evidence cache stays on:
        // per-file evidence is plugin-independent.
        let discovered_paths: BTreeSet<ProjectPath> =
            files.iter().map(|f| f.path.clone()).collect();
        let mut manifest_dependencies: BTreeSet<SmolStr> = BTreeSet::new();
        crate::graph::for_each_manifest(&files, &self.extensions, |extension, manifest| {
            manifest_dependencies.extend(extension.manifest_dependencies(&manifest));
        });
        let active =
            crate::conduct::activate(&self.extensions, &discovered_paths, &manifest_dependencies);
        let plugins_mutate = active
            .iter()
            .any(|(ix, _)| self.extensions[*ix].spec().mutates_graph());

        let fingerprint = kndo_contract::contract_fingerprint();
        let cache_root = self.config.use_cache.then(|| self.root.join(".kndo/cache"));
        let cache =
            EvidenceCache::new(cache_root.as_ref().map(|r| r.join("evidence")), fingerprint);
        let graph_cache = (!plugins_mutate)
            .then(|| crate::cache::GraphCache::new(cache_root, self.graph_cache_key()));

        // The surgical path first: a persisted graph patched in place when only file
        // contents moved. Re-extraction of the changed files happens inside `patch`,
        // so on a patched run the extract phase reads as zero and its work is folded
        // into `assemble`.
        let assemble_start = Instant::now();
        let patched = graph_cache.as_ref().and_then(|gc| gc.load()).and_then(|p| {
            crate::graph::patch(
                p.graph,
                p.manifest_state,
                &files,
                &claims,
                &self.extensions,
                &cache,
            )
        });
        let mut graph = match patched {
            Some(graph) => {
                timings.assemble = assemble_start.elapsed();
                graph
            }
            None => {
                let mut evidence = Vec::new();
                timed(&mut timings.extract, &mut || {
                    evidence = extract::extract(&files, &claims, &self.extensions, &cache);
                });
                let assemble_start = Instant::now();
                let graph = crate::graph::assemble(&files, &claims, evidence, &self.extensions);
                timings.assemble = assemble_start.elapsed();
                graph
            }
        };
        // Stored before the plugin round on purpose: the persisted graph is always
        // plugin-free, and when a mutating plugin is active nothing is stored at all.
        if let Some(gc) = &graph_cache {
            let persisted = crate::cache::PersistedGraph {
                manifest_state: crate::graph::manifest_state(&files, &self.extensions),
                graph,
            };
            gc.store(&persisted);
            graph = persisted.graph;
        }

        let analyze_start = Instant::now();
        let contents: BTreeMap<_, _> = files
            .iter()
            .map(|f| (f.path.clone(), f.content.as_slice()))
            .collect();
        let round =
            crate::conduct::run_round(&self.extensions, &active, &mut graph, &self.root, &contents);
        let narrowables: Vec<(SmolStr, Vec<SmolStr>)> = self
            .extensions
            .iter()
            .map(|e| {
                (
                    SmolStr::new(e.spec().coordinate()),
                    e.spec().narrowable_scopes().to_vec(),
                )
            })
            .collect();
        let mut outcome = run_all(
            &graph,
            round.coverage,
            &narrowables,
            &[&Unused, &InternalOnly, &TestOnly, &Untested, &Duplicate],
        );
        // Plugin findings ride the same suppression pass — a `kndo:allow
        // ext:<coordinate>/<rule>` pragma reaches them like any category — and
        // `apply` owns the canonical final sort.
        outcome.findings.extend(round.findings);
        let (findings, suppressed) = crate::suppress::apply(
            &graph,
            &contents,
            &outcome.abstained,
            &outcome.judged,
            outcome.findings,
        );
        timings.analyze = analyze_start.elapsed();

        let mut composition = self.composition_diagnostics.clone();
        let baseline = self.read_baseline(&mut composition);
        Snapshot {
            graph,
            findings,
            abstained: outcome.abstained,
            suppressed: suppressed.summary,
            contributions: round.contributions,
            pragma_problems: suppressed.problems,
            composition_diagnostics: composition,
            timings,
            baseline,
            files_discovered: files.len() as u32,
        }
    }

    fn baseline_path(&self) -> PathBuf {
        self.root.join(".kndo/baseline.json")
    }

    /// A missing baseline is none; an unreadable or wrong-schema one is none PLUS a
    /// diagnostic — the run never fails on its own memory, but a baseline that
    /// silently stopped applying would make every finding "new" with no explanation.
    fn read_baseline(&self, problems: &mut Vec<ReportDiagnostic>) -> Option<Vec<Finding>> {
        let path = self.baseline_path();
        let bytes = std::fs::read(&path).ok()?;
        match serde_json::from_slice::<BaselineFile>(&bytes) {
            Ok(b) if b.schema == BASELINE_SCHEMA => Some(b.findings),
            Ok(b) => {
                problems.push(ReportDiagnostic {
                    path: ProjectPath::new(".kndo/baseline.json"),
                    level: DiagnosticLevel::Warn,
                    message: format!(
                        "baseline schema `{}` is not `{BASELINE_SCHEMA}` — treated as absent; \
                         re-run `kndo baseline`",
                        b.schema
                    ),
                });
                None
            }
            Err(e) => {
                // A v1 baseline (`schema_version`) is another product's memory
                // living at the shared path — absent, not corrupt: warning about
                // a file v1 rightfully owns would be noise in every migrating
                // repo. Anything else unreadable is loud.
                let v1 = serde_json::from_slice::<serde_json::Value>(&bytes)
                    .is_ok_and(|v| v.get("schema_version").is_some());
                if !v1 {
                    problems.push(ReportDiagnostic {
                        path: ProjectPath::new(".kndo/baseline.json"),
                        level: DiagnosticLevel::Warn,
                        message: format!("baseline unreadable ({e}) — treated as absent"),
                    });
                }
                None
            }
        }
    }

    /// The one baseline effect: accept the snapshot's current findings as known.
    /// Written sorted, pretty, and whole — a baseline is a reviewed artifact users
    /// commit, not a cache.
    pub fn write_baseline(&self, snap: &Snapshot) -> std::io::Result<()> {
        let path = self.baseline_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = BaselineFile {
            schema: BASELINE_SCHEMA.into(),
            findings: snap.findings.clone(),
        };
        let json = serde_json::to_string_pretty(&file).expect("findings serialize");
        std::fs::write(path, json)
    }
}

impl Snapshot {
    pub fn report(&self) -> Report {
        let mut per_extension: BTreeMap<SmolStr, u32> = BTreeMap::new();
        for f in &self.graph.files {
            *per_extension.entry(f.adapter.clone()).or_insert(0) += 1;
        }
        let mut diagnostics: Vec<ReportDiagnostic> = self
            .graph
            .files
            .iter()
            .flat_map(|f| {
                f.evidence.diagnostics.iter().map(|d| ReportDiagnostic {
                    path: f.path.clone(),
                    level: d.level,
                    message: d.message.clone(),
                })
            })
            .chain(self.pragma_problems.iter().map(|p| ReportDiagnostic {
                path: p.path.clone(),
                level: p.level,
                message: p.message.clone(),
            }))
            .chain(self.composition_diagnostics.iter().cloned())
            .collect();
        diagnostics.sort_by(|a, b| (&a.path, &a.message).cmp(&(&b.path, &b.message)));

        let current: std::collections::BTreeSet<&str> =
            self.findings.iter().map(|f| f.id.as_str()).collect();
        let fixed: Vec<Finding> = self
            .baseline
            .iter()
            .flatten()
            .filter(|f| !current.contains(f.id.as_str()))
            .cloned()
            .collect();
        let findings: Vec<Finding> = self.new_findings().cloned().collect();
        let baselined = (self.findings.len() - findings.len()) as u32;

        Report {
            run: RunInfo {
                schema: REPORT_SCHEMA,
                files_discovered: self.files_discovered,
                files_claimed: self.graph.files.len() as u32,
                extensions: per_extension
                    .into_iter()
                    .map(|(id, files)| ExtensionRun { id, files })
                    .collect(),
            },
            findings,
            fixed,
            baselined,
            abstained: self.abstained.clone(),
            suppressed: self.suppressed.clone(),
            plugins: self.contributions.clone(),
            diagnostics,
        }
    }

    /// Counts NEW findings only — the baseline's whole purpose is that known findings
    /// hold no gate hostage — and never a plugin finding: those are advisory by
    /// containment (a plugin cannot construct a gate-eligible finding), visible in
    /// the report, powerless over the exit code.
    pub fn gate(&self, policy: &GatePolicy) -> RunOutcome {
        let Some(floor) = policy.fail_on else {
            return RunOutcome::Pass;
        };
        let at_or_above = self
            .new_findings()
            .filter(|f| !f.category.is_extension() && f.severity.at_least(floor))
            .count() as u32;
        if at_or_above == 0 {
            RunOutcome::Pass
        } else {
            RunOutcome::FailFindings { at_or_above }
        }
    }
}

/// The baseline's persisted shape, versioned like the report envelope: a serde
/// change to `Finding` must announce itself, not silently void the memory.
const BASELINE_SCHEMA: &str = "kndo-baseline/1";

#[derive(serde::Serialize, serde::Deserialize)]
struct BaselineFile {
    schema: String,
    findings: Vec<Finding>,
}
