//! Native plugins: engine extensions that see the assembled world and contribute
//! liveness (roots the language cannot know — framework routes, DI wiring),
//! advisory findings under namespaced categories, and ingested evidence
//! (coverage). v1's containment model carries whole: plugin findings are
//! namespaced and advisory (the gate never counts them), contributions are
//! reported in full — every root applied, every miss described, every budget cut
//! visible — and identity is the coordinate. External authors reach this same
//! shape through the WASM ABI; this trait is the native half built-ins and
//! embedders use.

use crate::graph::{Graph, GraphFile};
use kndo_contract::evidence::{Root, RootKind, RootTarget};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::{Subject, SymbolSelector};
use kndo_contract::vocab::{Category, Confidence, ProjectPath};
use serde::Serialize;
use smol_str::SmolStr;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// `kndo:` is the built-in namespace: an external component carrying it is
/// rejected at load, which is what makes `dependencies: ["kndo:express"]`
/// unambiguous from any source.
pub fn is_reserved_coordinate(coordinate: &str) -> bool {
    coordinate.starts_with("kndo:")
}

/// One machine-checkable activation predicate — cheap, evaluated against what the
/// run already discovered, never by running plugin code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationRule {
    /// At least one discovered file matches this glob (e.g. `next.config.*`).
    FileExists(SmolStr),
    /// Some discovered manifest declares a dependency with this name, in any
    /// section — as reported by the claiming adapters through
    /// `LanguageAdapter::manifest_dependencies`, the one manifest pipeline.
    ManifestDependency(SmolStr),
}

/// When a plugin runs. `Always` is speakable on purpose: "always on and cheap"
/// (a coverage ingester) is a real posture, not an exemption with a paragraph of
/// justification. An empty rule list is the OTHER deliberate extreme — a plugin
/// that can never self-activate, reachable only through another plugin's
/// `dependencies` (a company framework whose users never depend on it directly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Activation {
    Always,
    AnyRule(Vec<ActivationRule>),
}

/// One rule a plugin may emit findings under; the suffix of the namespaced
/// category `plugin:<coordinate>/<rule>`. A finding under an undeclared rule is
/// dropped and recorded — declaration is the contract, not decoration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDescriptor {
    pub name: SmolStr,
    pub description: SmolStr,
}

/// What a plugin IS, as data — the same posture as `AdapterSpec`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpec {
    coordinate: SmolStr,
    version: u32,
    activation: Activation,
    dependencies: Vec<SmolStr>,
    /// Globs over discovered files this plugin may read through [`ContentView`];
    /// a path outside them reads as absent.
    requested_file_access: Vec<SmolStr>,
    rules: Vec<RuleDescriptor>,
}

impl PluginSpec {
    pub fn builder(coordinate: &'static str, version: u32) -> PluginSpecBuilder {
        PluginSpecBuilder {
            spec: PluginSpec {
                coordinate: SmolStr::new_static(coordinate),
                version,
                activation: Activation::AnyRule(Vec::new()),
                dependencies: Vec::new(),
                requested_file_access: Vec::new(),
                rules: Vec::new(),
            },
        }
    }

    pub fn coordinate(&self) -> &str {
        &self.coordinate
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn activation(&self) -> &Activation {
        &self.activation
    }

    pub fn dependencies(&self) -> &[SmolStr] {
        &self.dependencies
    }

    pub fn requested_file_access(&self) -> &[SmolStr] {
        &self.requested_file_access
    }

    pub fn rules(&self) -> &[RuleDescriptor] {
        &self.rules
    }
}

pub struct PluginSpecBuilder {
    spec: PluginSpec,
}

impl PluginSpecBuilder {
    /// Omitted ⇒ `AnyRule([])`: never self-activates, dependency-reachable only.
    pub fn activation(mut self, activation: Activation) -> Self {
        self.spec.activation = activation;
        self
    }

    /// Coordinates of plugins this one needs running beside it. An active plugin
    /// activates its dependencies, transitively — the only path for a component
    /// whose framework is an INDIRECT dependency.
    pub fn dependencies(mut self, coordinates: &[&'static str]) -> Self {
        self.spec.dependencies = coordinates.iter().map(|c| SmolStr::new_static(c)).collect();
        self
    }

    pub fn requested_file_access(mut self, globs: &[&'static str]) -> Self {
        self.spec.requested_file_access = globs.iter().map(|g| SmolStr::new_static(g)).collect();
        self
    }

    pub fn rule(mut self, name: &'static str, description: &'static str) -> Self {
        self.spec.rules.push(RuleDescriptor {
            name: SmolStr::new_static(name),
            description: SmolStr::new_static(description),
        });
        self
    }

    pub fn build(self) -> PluginSpec {
        self.spec
    }
}

/// A plugin's own severity vocabulary — deliberately not `Severity`: the engine
/// maps it into the advisory channel, so a plugin can never construct a
/// gate-eligible finding directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSeverity {
    Error,
    Warning,
    Info,
}

impl PluginSeverity {
    fn advisory(self) -> Severity {
        match self {
            PluginSeverity::Error => Severity::Error,
            PluginSeverity::Warning => Severity::Warning,
            PluginSeverity::Info => Severity::Info,
        }
    }
}

/// What a contribution points at. A target that resolves to nothing is a silent
/// no-op in the graph and a described line in the contribution — the author
/// debugging "contributed 0 roots" needs the why; the run never crashes on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginTarget {
    File(ProjectPath),
    Symbol { path: ProjectPath, name: SmolStr },
}

/// The graph as a plugin may see it: paths and membership, no internals.
pub struct GraphView<'a> {
    files: &'a [GraphFile],
}

impl<'a> GraphView<'a> {
    pub fn paths(&self) -> impl Iterator<Item = &'a ProjectPath> {
        self.files.iter().map(|f| &f.path)
    }

    pub fn contains(&self, path: &ProjectPath) -> bool {
        self.files.binary_search_by(|f| f.path.cmp(path)).is_ok()
    }
}

/// Budgeted, glob-scoped reads over the run's already-read file contents — no
/// second disk walk. Every miss (no glob match, unknown path, budget cut) is the
/// same `None`; the cut itself is reported on the contribution, never silent.
pub struct ContentView<'a> {
    contents: &'a BTreeMap<ProjectPath, &'a [u8]>,
    globs: Vec<globset::GlobMatcher>,
    budget: RefCell<ContentBudget>,
}

pub const CONTENT_MAX_FILES: usize = 200;
pub const CONTENT_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
struct ContentBudget {
    files: usize,
    bytes: usize,
    seen: BTreeSet<ProjectPath>,
    cut_off: bool,
}

impl<'a> ContentView<'a> {
    fn new(contents: &'a BTreeMap<ProjectPath, &'a [u8]>, spec: &PluginSpec) -> Self {
        // A malformed glob is simply never satisfied — declared input degrades,
        // the run never aborts on it.
        let globs = spec
            .requested_file_access
            .iter()
            .filter_map(|g| globset::Glob::new(g).ok())
            .map(|g| g.compile_matcher())
            .collect();
        ContentView {
            contents,
            globs,
            budget: RefCell::new(ContentBudget::default()),
        }
    }

    pub fn read(&self, path: &ProjectPath) -> Option<&'a [u8]> {
        if !self.globs.iter().any(|g| g.is_match(path.as_str())) {
            return None;
        }
        let bytes = *self.contents.get(path)?;
        let mut budget = self.budget.borrow_mut();
        if budget.seen.contains(path) {
            // A charged path re-reads for free, cutoff or not: the budget is
            // about new reads, not about punishing a second look.
            return Some(bytes);
        }
        if budget.cut_off {
            return None;
        }
        budget.files += 1;
        budget.bytes += bytes.len();
        budget.seen.insert(path.clone());
        if budget.files > CONTENT_MAX_FILES || budget.bytes > CONTENT_MAX_BYTES {
            budget.cut_off = true;
            return None;
        }
        Some(bytes)
    }

    fn was_cut(&self) -> bool {
        self.budget.borrow().cut_off
    }
}

/// Root-relative reads of RUN OUTPUT the discovery walk deliberately never sees
/// (a coverage file is gitignored build product). Size-capped; misses are absent.
pub struct WellKnown<'a> {
    root: &'a std::path::Path,
}

/// One well-known file may not exceed the whole content budget.
const WELL_KNOWN_MAX_BYTES: u64 = CONTENT_MAX_BYTES as u64;

impl WellKnown<'_> {
    pub fn read(&self, relative: &str) -> Option<String> {
        let path = self.root.join(relative);
        let meta = std::fs::metadata(&path).ok()?;
        if !meta.is_file() || meta.len() > WELL_KNOWN_MAX_BYTES {
            return None;
        }
        std::fs::read_to_string(path).ok()
    }
}

/// The write side of one plugin's run.
#[derive(Default)]
pub struct PluginSink {
    roots: Vec<(PluginTarget, RootKind, Confidence)>,
    findings: Vec<(SmolStr, PluginSeverity, PluginTarget, String)>,
}

impl PluginSink {
    /// Anchor liveness the language cannot see: a route file, a DI-registered
    /// symbol. Applied to the graph only from plugins that declare
    /// `mutates_graph() == true` — the declaration is self-enforcing, because the
    /// hook that fills this is only invoked on those.
    pub fn root(&mut self, target: PluginTarget, kind: RootKind, confidence: Confidence) {
        self.roots.push((target, kind, confidence));
    }

    /// An advisory finding under one of the spec's declared rules; undeclared
    /// rules drop with a described line on the contribution.
    pub fn finding(
        &mut self,
        rule: &str,
        severity: PluginSeverity,
        target: PluginTarget,
        message: impl Into<String>,
    ) {
        self.findings
            .push((SmolStr::new(rule), severity, target, message.into()));
    }
}

/// What one plugin asserted this run — the observable half of the containment
/// model: a plugin can only lie about graph facts, and here is exactly what facts
/// it asserted, what missed, and whether its content budget cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PluginContribution {
    pub coordinate: SmolStr,
    pub roots: u32,
    pub findings: u32,
    /// Contributions whose targets resolved to nothing, one described line each.
    pub dropped: Vec<String>,
    pub content_budget_cut: bool,
}

pub trait Plugin: Send + Sync {
    fn spec(&self) -> &PluginSpec;

    /// Whether this plugin contributes to graph assembly (`contribute_roots`).
    /// Load-bearing, not a hint: any ACTIVE graph-mutating plugin bypasses the
    /// persisted graph cache for the run — the surgical patch never re-invokes
    /// plugin hooks, so it can never safely reuse a graph one influenced. An
    /// ingester-only plugin must return `false`, or its mere presence turns the
    /// cache off product-wide. No default: forgetting this is a compile error,
    /// never a silent product-wide loss of incremental speed.
    fn mutates_graph(&self) -> bool;

    /// Liveness the language cannot know. Called only when
    /// `mutates_graph() == true`.
    fn contribute_roots(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut PluginSink,
    ) {
        let _ = (graph, content, out);
    }

    /// Advisory findings under the spec's declared rules. Called on every active
    /// plugin.
    fn report_findings(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut PluginSink,
    ) {
        let _ = (graph, content, out);
    }

    /// Ingested run output (coverage), read through the well-known channel plus
    /// the run's file contents (line tables need the sources). First active
    /// plugin to answer wins, in registration order.
    fn ingest_coverage(
        &self,
        well_known: &WellKnown<'_>,
        contents: &BTreeMap<ProjectPath, &[u8]>,
    ) -> Option<kndo_coverage::Coverage> {
        let _ = (well_known, contents);
        None
    }
}

/// Why each active plugin is running — decided once, carried, never re-derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationReason {
    AlwaysOn,
    RuleMatched(ActivationRule),
    /// The named ACTIVE plugin lists this one in `dependencies` — possibly
    /// transitively; the only path for an indirect-framework plugin.
    DependencyOf(SmolStr),
}

/// Evaluate activation over what the run discovered: file paths for `FileExists`,
/// adapter-reported dependency names for `ManifestDependency`, then the
/// dependency closure — an active plugin activates what it depends on, whether or
/// not those rules matched.
pub fn activate(
    plugins: &[Box<dyn Plugin>],
    discovered_paths: &BTreeSet<ProjectPath>,
    manifest_dependencies: &BTreeSet<SmolStr>,
) -> Vec<(usize, ActivationReason)> {
    let mut active: Vec<(usize, ActivationReason)> = Vec::new();
    let mut is_active = vec![false; plugins.len()];
    for (ix, plugin) in plugins.iter().enumerate() {
        let reason = match plugin.spec().activation() {
            Activation::Always => Some(ActivationReason::AlwaysOn),
            Activation::AnyRule(rules) => rules
                .iter()
                .find(|rule| rule_matches(rule, discovered_paths, manifest_dependencies))
                .map(|rule| ActivationReason::RuleMatched(rule.clone())),
        };
        if let Some(reason) = reason {
            is_active[ix] = true;
            active.push((ix, reason));
        }
    }
    // Dependency closure, deterministic: scan until fixpoint in coordinate order.
    loop {
        let mut grew = false;
        for (ix, plugin) in plugins.iter().enumerate() {
            if !is_active[ix] {
                continue;
            }
            for dep in plugin.spec().dependencies() {
                if let Some(dep_ix) = plugins
                    .iter()
                    .position(|p| p.spec().coordinate() == dep.as_str())
                    && !is_active[dep_ix]
                {
                    is_active[dep_ix] = true;
                    active.push((
                        dep_ix,
                        ActivationReason::DependencyOf(SmolStr::new(plugin.spec().coordinate())),
                    ));
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    active.sort_by_key(|(ix, _)| *ix);
    active
}

fn rule_matches(
    rule: &ActivationRule,
    discovered_paths: &BTreeSet<ProjectPath>,
    manifest_dependencies: &BTreeSet<SmolStr>,
) -> bool {
    match rule {
        ActivationRule::FileExists(glob) => globset::Glob::new(glob)
            .ok()
            .map(|g| g.compile_matcher())
            .is_some_and(|m| discovered_paths.iter().any(|p| m.is_match(p.as_str()))),
        ActivationRule::ManifestDependency(name) => manifest_dependencies.contains(name),
    }
}

/// Everything one plugin round produced, ready for the session to fold in.
pub struct PluginRound {
    pub contributions: Vec<PluginContribution>,
    pub coverage: Option<kndo_coverage::Coverage>,
    pub findings: Vec<Finding>,
}

/// Run every active plugin over the assembled graph: coverage first-answer-wins,
/// roots applied onto `anchored` (target misses drop with a description), advisory
/// findings mapped under their namespaced categories.
pub fn run_round(
    plugins: &[Box<dyn Plugin>],
    active: &[(usize, ActivationReason)],
    graph: &mut Graph,
    root: &std::path::Path,
    contents: &BTreeMap<ProjectPath, &[u8]>,
) -> PluginRound {
    let mut contributions = Vec::new();
    let mut coverage = None;
    let mut findings = Vec::new();
    let well_known = WellKnown { root };

    for &(ix, _) in active {
        let plugin = &plugins[ix];
        let spec = plugin.spec();
        let mut sink = PluginSink::default();
        let mut dropped = Vec::new();

        if coverage.is_none() {
            coverage = plugin.ingest_coverage(&well_known, contents);
        }

        let content = ContentView::new(contents, spec);
        {
            let view = GraphView {
                files: &graph.files,
            };
            if plugin.mutates_graph() {
                plugin.contribute_roots(&view, &content, &mut sink);
            }
            plugin.report_findings(&view, &content, &mut sink);
        }

        let mut applied_roots = 0u32;
        for (target, kind, confidence) in sink.roots {
            // The sink is shared between hooks, so a plugin that declared
            // `mutates_graph() == false` can still CALL `root()` from
            // `report_findings` — those drop with a described line instead of
            // silently mutating a graph the cache was told is plugin-free.
            if !plugin.mutates_graph() {
                dropped.push(format!(
                    "root refused: {} — the plugin declares mutates_graph() == false",
                    describe_target(&target)
                ));
                continue;
            }
            match resolve_target(graph, &target) {
                Some((file_ix, root_target)) => {
                    graph.files[file_ix].anchored.push(Root {
                        target: root_target,
                        kind,
                        confidence,
                    });
                    applied_roots += 1;
                }
                None => dropped.push(format!(
                    "root not applied: {} does not resolve in the graph",
                    describe_target(&target)
                )),
            }
        }

        let mut applied_findings = 0u32;
        for (rule, severity, target, message) in sink.findings {
            if !spec.rules().iter().any(|r| r.name == rule) {
                dropped.push(format!("finding under undeclared rule `{rule}`"));
                continue;
            }
            let Some(subject) = target_subject(graph, &target) else {
                dropped.push(format!(
                    "finding not applied: {} does not resolve in the graph",
                    describe_target(&target)
                ));
                continue;
            };
            findings.push(Finding::new(
                Category::plugin(spec.coordinate(), &rule),
                severity.advisory(),
                Confidence::Probable,
                subject,
                "",
                message,
            ));
            applied_findings += 1;
        }

        contributions.push(PluginContribution {
            coordinate: SmolStr::new(spec.coordinate()),
            roots: applied_roots,
            findings: applied_findings,
            dropped,
            content_budget_cut: content.was_cut(),
        });
    }

    PluginRound {
        contributions,
        coverage,
        findings,
    }
}

fn file_index(graph: &Graph, path: &ProjectPath) -> Option<usize> {
    graph.files.binary_search_by(|f| f.path.cmp(path)).ok()
}

fn resolve_target(graph: &Graph, target: &PluginTarget) -> Option<(usize, RootTarget)> {
    match target {
        PluginTarget::File(path) => Some((file_index(graph, path)?, RootTarget::WholeFile)),
        PluginTarget::Symbol { path, name } => {
            let ix = file_index(graph, path)?;
            let (id, _) = graph.files[ix]
                .evidence
                .declarations_with_ids()
                .find(|(_, d)| d.name == name.as_str())?;
            Some((ix, RootTarget::Declaration(id)))
        }
    }
}

fn target_subject(graph: &Graph, target: &PluginTarget) -> Option<Subject> {
    match target {
        PluginTarget::File(path) => {
            file_index(graph, path)?;
            Some(Subject::File { path: path.clone() })
        }
        PluginTarget::Symbol { path, name } => {
            let ix = file_index(graph, path)?;
            let decl = graph.files[ix]
                .evidence
                .declarations
                .iter()
                .find(|d| d.name == name.as_str())?;
            Some(Subject::Symbol {
                path: path.clone(),
                selector: match decl.owner {
                    Some(owner) => SymbolSelector::Member {
                        owner: graph.files[ix].evidence.declarations[owner.index()]
                            .name
                            .clone(),
                        name: decl.name.clone(),
                    },
                    None => SymbolSelector::Free(decl.name.clone()),
                },
                span: decl.span,
            })
        }
    }
}

/// Neutral spelling of a target for dropped-contribution lines — each drop site
/// states its own reason beside it.
fn describe_target(target: &PluginTarget) -> String {
    match target {
        PluginTarget::File(path) => format!("file {}", path.as_str()),
        PluginTarget::Symbol { path, name } => {
            format!("symbol {name} in {}", path.as_str())
        }
    }
}
