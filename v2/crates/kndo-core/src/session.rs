//! Session/Snapshot: the engine produces immutable values. `Session` owns
//! configuration (and, as milestones land, the cache lock and effects); `Snapshot` is
//! the result — queries, reports and gates run on it. `analyze` returns
//! `Result<Snapshot, Refusal>`: the run either happened or was refused, and the
//! refusal reappears inside [`RunOutcome`] so `exit_code` covers that path too.

use crate::analysis::{
    Abstention, Crap, Cyclic, Duplicate, InternalOnly, PrivateTypeLeak, TestOnly, Undeclared,
    Unresolved, Untested, Unused, VersionSkew, run_all,
};
use crate::cache::EvidenceCache;
use crate::conduct::Contribution;
use crate::graph::Graph;
use crate::report::{ExtensionRun, REPORT_SCHEMA, Report, ReportDiagnostic, RunInfo};
use crate::{discover, extract};
use kndo_contract::evidence::DiagnosticLevel;
use kndo_contract::extension::Extension;
use kndo_contract::finding::{Finding, LineSpan, Severity};
use kndo_contract::subject::Subject;
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
    pub categories: Categories,
    /// `crap`'s line: a function scoring at or above it is a finding. The
    /// metric's own 30 by default (`CRAP_THRESHOLD`).
    pub crap_threshold: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            threads: Threads::Auto,
            use_cache: true,
            categories: Categories::All,
            crap_threshold: crate::analysis::CRAP_THRESHOLD,
        }
    }
}

/// Which categories this run JUDGES — never a display filter. An unselected
/// category's analysis does not run: it leaves the `judged` set (so its
/// suppressions cannot read as stale), produces no abstention, and the report's
/// `run.selection` says the narrowing was asked for. Health follows judgment —
/// skip `unused` and health is absent, not padded. Plugin findings pass the same
/// test by category name; a plugin's [`Contribution`] still records what it
/// asserted, because activity and selection are different facts.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Categories {
    #[default]
    All,
    /// Judge only these.
    Only(Vec<kndo_contract::vocab::Category>),
    /// Judge everything except these.
    Skip(Vec<kndo_contract::vocab::Category>),
}

impl Categories {
    pub fn includes(&self, category: &kndo_contract::vocab::Category) -> bool {
        match self {
            Categories::All => true,
            Categories::Only(selected) => selected.contains(category),
            Categories::Skip(skipped) => !skipped.contains(category),
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

/// The judgment capabilities one extension declared, carried spec → report.
#[derive(Debug, Clone)]
pub struct DeclaredCapabilities {
    pub narrowable_scopes: Vec<SmolStr>,
    pub export_narrowing: kndo_contract::extension::ExportNarrowing,
    pub import_cycles: kndo_contract::extension::CycleTolerance,
    pub dependency_scoping: kndo_contract::extension::DependencyScoping,
    pub dependency_identity: kndo_contract::extension::DependencyIdentity,
}

pub struct Snapshot {
    pub graph: Graph,
    /// Current findings, post-suppression — plugin findings included, under their
    /// namespaced categories. The baseline split (new vs known) is the report's and
    /// the gate's view; the snapshot keeps the whole truth.
    pub findings: Vec<Finding>,
    pub abstained: Vec<Abstention>,
    /// Categories whose analysis actually ran — health refuses to measure when
    /// reachability itself is absent from this set.
    pub judged: std::collections::BTreeSet<kndo_contract::vocab::Category>,
    /// The dependency declarations the usage judgment counted, by manifest —
    /// the part of health's universe the graph alone cannot state.
    dependency_universe: BTreeMap<ProjectPath, BTreeSet<SmolStr>>,
    pub suppressed: crate::suppress::SuppressedSummary,
    /// What each active plugin asserted, in registration order — always reported,
    /// even when everything applied cleanly.
    pub contributions: Vec<Contribution>,
    pub timings: PhaseTimings,
    /// Per extension coordinate, the judgment capabilities its spec declared —
    /// carried from composition to the report's `extensions` rows, where they
    /// answer "why does kndo (not) report X for this language".
    capabilities: Vec<(SmolStr, DeclaredCapabilities)>,
    baseline: Option<Vec<Finding>>,
    mode: crate::report::Mode,
    base_health: Option<crate::health::Health>,
    categories: Categories,
    line_index: BTreeMap<ProjectPath, Vec<u32>>,
    pragma_problems: Vec<crate::suppress::PragmaProblem>,
    composition_diagnostics: Vec<ReportDiagnostic>,
    files_discovered: u32,
    /// Reachability + navigation index, built on the first query and held for
    /// the snapshot's lifetime — a pure function of the graph, so a holder
    /// answering many queries (serve) pays the build once, and a single-query
    /// holder (the CLI) pays exactly what it always did.
    pub(crate) navigation:
        std::sync::OnceLock<(crate::analysis::Reachability, crate::navigate::Index)>,
}

/// Byte offsets where each line begins; line N (1-based) starts at `[N-1]`.
fn line_starts(content: &[u8]) -> Vec<u32> {
    let mut starts = vec![0u32];
    for (i, b) in content.iter().enumerate() {
        if *b == b'\n' {
            starts.push(i as u32 + 1);
        }
    }
    starts
}

fn line_of(starts: &[u32], offset: u32) -> u32 {
    starts.partition_point(|s| *s <= offset) as u32
}

/// Resolve each spanned subject's byte span to its 1-based inclusive line range.
/// The span's `end` is exclusive, so the range's last byte decides `end`'s line
/// (an empty span sits on its start line).
fn fill_lines(mut findings: Vec<Finding>, index: &BTreeMap<ProjectPath, Vec<u32>>) -> Vec<Finding> {
    for f in &mut findings {
        let span = match &f.subject {
            Subject::Symbol { span, .. }
            | Subject::Import { span, .. }
            | Subject::Suppression { span, .. } => *span,
            _ => continue,
        };
        if let Some(starts) = index.get(f.subject.path()) {
            f.lines = Some(LineSpan {
                start: line_of(starts, span.start),
                end: line_of(starts, span.end.saturating_sub(1).max(span.start)),
            });
        }
    }
    findings
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

    /// Byte-to-line resolution for this snapshot's tree, as computed at analyze
    /// time — the query verbs speak `path:line` from it.
    pub(crate) fn line_index(&self) -> &BTreeMap<ProjectPath, Vec<u32>> {
        &self.line_index
    }

    /// Turn this snapshot into a diff against another tree's snapshot: the base's
    /// findings replace the baseline file as the comparison set (a tree-vs-tree
    /// split never consults the baseline — a baselined finding this change
    /// reintroduces is new debt), and the base's health rides along so the report
    /// can say which way the change moves it — a pure function of the two trees
    /// the invocation pinned, never cross-run state.
    pub fn against(&mut self, base: &Snapshot, mode: crate::report::Mode) {
        self.baseline = Some(base.findings.clone());
        let universe = base.universe();
        self.base_health = crate::health::Health::measure(&base.findings, &universe, &base.judged)
            .map(|mut h| {
                h.partition(&base.graph, &base.findings, &universe);
                h
            });
        self.mode = mode;
    }
}

impl Snapshot {
    /// The health universe: every declaration plus every claimed file, plus the
    /// dependency declarations this run's usage judgment counted.
    fn universe(&self) -> crate::health::Universe {
        crate::health::Universe::of(&self.graph, self.dependency_universe.clone())
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

    /// The composition this session runs, in registration order — introspection
    /// for `doctor`-shaped frontends; the specs are the extensions' own claims.
    pub fn extensions(&self) -> impl Iterator<Item = &kndo_contract::extension::ExtensionSpec> {
        self.extensions.iter().map(|e| e.spec())
    }

    /// What went sideways assembling the composition (a component that failed to
    /// load, a rejected coordinate) — the same entries every report carries.
    pub fn composition_diagnostics(&self) -> &[crate::report::ReportDiagnostic] {
        &self.composition_diagnostics
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
            let hidden =
                discover::HiddenOptIn::from_manifest_globs(self.extensions.iter().flat_map(|e| {
                    let spec = e.spec();
                    spec.manifests()
                        .iter()
                        .chain(spec.launchers())
                        .map(|g| g.as_str())
                }));
            files = discover::discover(&self.root, &hidden);
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
            manifest_dependencies.extend(
                extension
                    .manifest_dependencies(&manifest)
                    .into_iter()
                    .map(|d| d.name),
            );
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
        let cycle_hazards: Vec<SmolStr> = self
            .extensions
            .iter()
            .filter(|e| {
                e.spec().import_cycles() == kndo_contract::extension::CycleTolerance::Hazard
            })
            .map(|e| SmolStr::new(e.spec().coordinate()))
            .collect();
        let export_narrowables: Vec<SmolStr> = self
            .extensions
            .iter()
            .filter(|e| {
                e.spec().export_narrowing()
                    == kndo_contract::extension::ExportNarrowing::Expressible
            })
            .map(|e| SmolStr::new(e.spec().coordinate()))
            .collect();
        let crap = Crap {
            threshold: self.config.crap_threshold,
        };
        let all: [&dyn crate::analysis::Analysis; 11] = [
            &Cyclic,
            &PrivateTypeLeak,
            &Unused,
            &InternalOnly,
            &TestOnly,
            &Untested,
            &Duplicate,
            &Unresolved,
            &VersionSkew,
            &Undeclared,
            &crap,
        ];
        let selected: Vec<&dyn crate::analysis::Analysis> = all
            .into_iter()
            .filter(|a| self.config.categories.includes(&a.category()))
            .collect();
        let mut outcome = run_all(
            &graph,
            round.coverage,
            &narrowables,
            &export_narrowables,
            &cycle_hazards,
            &selected,
        );
        // Plugin findings ride the same suppression pass — a `kndo:allow
        // ext:<coordinate>/<rule>` pragma reaches them like any category — and
        // `apply` owns the canonical final sort. The selection reaches them by
        // category name like any analysis's.
        outcome.findings.extend(
            round
                .findings
                .into_iter()
                .filter(|f| self.config.categories.includes(&f.category)),
        );
        let (findings, suppressed) = crate::suppress::apply(
            &graph,
            &contents,
            &outcome.abstained,
            &outcome.judged,
            outcome.findings,
        );
        // Lines are resolved here, once, for every finding with a span — a pure
        // function of this run's file contents (already in memory for hashing), so
        // nothing is persisted and no cache format learns about lines.
        let line_index: BTreeMap<ProjectPath, Vec<u32>> = files
            .iter()
            .map(|f| (f.path.clone(), line_starts(&f.content)))
            .collect();
        let findings = fill_lines(findings, &line_index);
        timings.analyze = analyze_start.elapsed();
        let retained_line_index = line_index;

        let mut composition = self.composition_diagnostics.clone();
        let baseline = self.read_baseline(&mut composition);
        let capabilities = self
            .extensions
            .iter()
            .map(|e| {
                let s = e.spec();
                (
                    SmolStr::new(s.coordinate()),
                    DeclaredCapabilities {
                        narrowable_scopes: s.narrowable_scopes().to_vec(),
                        export_narrowing: s.export_narrowing(),
                        import_cycles: s.import_cycles(),
                        dependency_scoping: s.dependency_scoping(),
                        dependency_identity: s.dependency_identity(),
                    },
                )
            })
            .collect();
        Snapshot {
            graph,
            findings,
            capabilities,
            abstained: outcome.abstained,
            judged: outcome.judged,
            dependency_universe: outcome.dependency_universe,
            mode: crate::report::Mode::Full,
            base_health: None,
            categories: self.config.categories.clone(),
            line_index: retained_line_index,
            suppressed: suppressed.summary,
            contributions: round.contributions,
            pragma_problems: suppressed.problems,
            composition_diagnostics: composition,
            timings,
            baseline,
            files_discovered: files.len() as u32,
            navigation: std::sync::OnceLock::new(),
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

        let universe = self.universe();
        let health =
            crate::health::Health::measure(&self.findings, &universe, &self.judged).map(|mut h| {
                h.partition(&self.graph, &self.findings, &universe);
                h
            });

        Report {
            run: RunInfo {
                schema: REPORT_SCHEMA,
                mode: self.mode,
                selection: match &self.categories {
                    Categories::All => None,
                    narrowed => Some(narrowed.clone()),
                },
                files_discovered: self.files_discovered,
                files_claimed: self.graph.files.len() as u32,
                extensions: per_extension
                    .into_iter()
                    .map(|(id, files)| {
                        let caps = self
                            .capabilities
                            .iter()
                            .find(|(c, _)| *c == id)
                            .map(|(_, caps)| caps);
                        ExtensionRun {
                            id,
                            files,
                            narrowable_scopes: caps
                                .map(|c| c.narrowable_scopes.clone())
                                .unwrap_or_default(),
                            export_narrowing: caps.map(|c| c.export_narrowing).unwrap_or_default(),
                            import_cycles: caps.map(|c| c.import_cycles).unwrap_or_default(),
                            dependency_scoping: caps
                                .map(|c| c.dependency_scoping)
                                .unwrap_or_default(),
                            dependency_identity: caps
                                .map(|c| c.dependency_identity)
                                .unwrap_or_default(),
                        }
                    })
                    .collect(),
            },
            health,
            base_health: self.base_health.clone(),
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
