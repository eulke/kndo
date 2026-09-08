//! The conduct-and-ingestion round: activation, the dependency closure, and the
//! containment model over the CONDUCT-DECLARING subset of the session's
//! extensions — liveness roots the language cannot know, advisory findings under
//! namespaced categories (the gate never counts them), ingested coverage.
//! Contributions are reported in full — every root applied, every miss
//! described, every budget cut visible — and identity is the coordinate.

use crate::graph::{Graph, GraphFile};
use kndo_contract::evidence::{Root, RootTarget};
use kndo_contract::finding::Finding;
use kndo_contract::subject::Subject;
use kndo_contract::vocab::{Category, ProjectPath};
use serde::Serialize;
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};

/// The conduct vocabulary is contract vocabulary (`kndo-contract`'s extension
/// module — the one door); re-exported here where the engine that enforces it
/// lives.
pub use kndo_contract::plugin::{
    Activation, ActivationRule, CONTENT_MAX_BYTES, CONTENT_MAX_FILES, ContentView, DeclaredSymbol,
    GraphAccess, Plugin, PluginSeverity, PluginSink, PluginTarget, RuleDescriptor,
};

pub use kndo_contract::plugin::is_reserved_coordinate;

/// The graph as a plugin may see it: paths and membership, no internals. The
/// engine-side implementation of the contract's [`GraphAccess`].
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

impl GraphAccess for GraphView<'_> {
    fn paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_> {
        Box::new(GraphView::paths(self))
    }

    fn contains(&self, path: &ProjectPath) -> bool {
        GraphView::contains(self, path)
    }

    fn declarations(&self) -> Box<dyn Iterator<Item = DeclaredSymbol<'_>> + '_> {
        Box::new(
            self.files
                .iter()
                .flat_map(|f| DeclaredSymbol::of_file(&f.path, &f.evidence.declarations)),
        )
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
        // `relative` is wire data when the extension is a loaded component: an
        // absolute path would REPLACE the root in `join`, and `..` would climb out
        // of it — both must stay unreadable, not merely undocumented.
        let candidate = std::path::Path::new(relative);
        if candidate.is_absolute()
            || candidate
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return None;
        }
        let path = self.root.join(relative);
        let meta = std::fs::metadata(&path).ok()?;
        if !meta.is_file() || meta.len() > WELL_KNOWN_MAX_BYTES {
            return None;
        }
        std::fs::read_to_string(path).ok()
    }
}

/// What one plugin asserted this run — the observable half of the containment
/// model: a plugin can only lie about graph facts, and here is exactly what facts
/// it asserted, what missed, and whether its content budget cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Contribution {
    pub coordinate: SmolStr,
    pub roots: u32,
    pub findings: u32,
    /// Contributions whose targets resolved to nothing, one described line each.
    pub dropped: Vec<String>,
    pub content_budget_cut: bool,
}

/// Why each active extension is running — decided once, carried, never re-derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationReason {
    AlwaysOn,
    RuleMatched(ActivationRule),
    /// The named ACTIVE plugin lists this one in `dependencies` — possibly
    /// transitively; the only path for an indirect-framework plugin.
    DependencyOf(SmolStr),
}

/// Evaluate activation over what the run discovered: file paths for `FileExists`,
/// extension-reported dependency names for `ManifestDependency`, then the
/// dependency closure — an active extension activates what it depends on, whether
/// or not those rules matched. Only the CONDUCT-DECLARING subset participates:
/// activation gates judgment, and an extraction-only extension has none to gate —
/// it never appears in the round or as a contribution row.
pub fn activate(
    extensions: &[Box<dyn Plugin>],
    discovered: &[crate::discover::DiscoveredFile],
    declared_dependencies: &BTreeSet<SmolStr>,
) -> Vec<(usize, ActivationReason)> {
    let mut active: Vec<(usize, ActivationReason)> = Vec::new();
    let mut is_active = vec![false; extensions.len()];
    for (ix, extension) in extensions.iter().enumerate() {
        if !extension.spec().declares_conduct() {
            continue;
        }
        let reason = match extension.spec().activation() {
            Activation::Always => Some(ActivationReason::AlwaysOn),
            Activation::AnyRule(rules) => rules
                .iter()
                .find(|rule| rule_matches(rule, discovered, declared_dependencies))
                .map(|rule| ActivationReason::RuleMatched(rule.clone())),
        };
        if let Some(reason) = reason {
            is_active[ix] = true;
            active.push((ix, reason));
        }
    }
    // Dependency closure, deterministic: scan until fixpoint in the (sorted)
    // composition order the loader established.
    loop {
        let mut grew = false;
        for (ix, extension) in extensions.iter().enumerate() {
            if !is_active[ix] {
                continue;
            }
            for dep in extension.spec().dependencies() {
                if let Some(dep_ix) = extensions
                    .iter()
                    .position(|e| e.spec().coordinate() == dep.as_str())
                    && !is_active[dep_ix]
                    && extensions[dep_ix].spec().declares_conduct()
                {
                    is_active[dep_ix] = true;
                    active.push((
                        dep_ix,
                        ActivationReason::DependencyOf(SmolStr::new(extension.spec().coordinate())),
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
    discovered: &[crate::discover::DiscoveredFile],
    declared_dependencies: &BTreeSet<SmolStr>,
) -> bool {
    match rule {
        ActivationRule::FileExists(glob) => globset::Glob::new(glob)
            .ok()
            .map(|g| g.compile_matcher())
            .is_some_and(|m| discovered.iter().any(|f| m.is_match(f.path.as_str()))),
        ActivationRule::ManifestDependency(pattern) => declared_dependencies
            .iter()
            .any(|name| kndo_contract::plugin::matches_pattern(pattern, name)),
        // Before any file is parsed there is no import stream to ask, so the
        // gate reads the one thing every specifier leaves in the source: its
        // literal text. The stem is the pattern up to its first `*` — what an
        // `org.junit.*` rule and an `org.junit.jupiter.api.Test` import share
        // verbatim — and a hit anywhere in a discovered file opens the gate.
        // See `ActivationRule::FileImports` for why coarse is the contract.
        ActivationRule::FileImports(pattern) => {
            let stem = pattern.split('*').next().unwrap_or("");
            !stem.is_empty()
                && discovered
                    .iter()
                    .any(|f| f.content.windows(stem.len()).any(|w| w == stem.as_bytes()))
        }
    }
}

/// Everything one plugin round produced, ready for the session to fold in.
pub struct PluginRound {
    pub contributions: Vec<Contribution>,
    pub coverage: Option<crate::coverage::Coverage>,
    pub findings: Vec<Finding>,
}

/// Run every active extension over the assembled graph: coverage
/// first-answer-wins in registration order — the engine walks each spec's
/// `reads_reports` through the well-known channel, pushes the bytes to `ingest`,
/// and a report that parses but maps no file falls through to the next candidate
/// — roots applied onto `anchored` (target misses drop with a description), and
/// advisory findings mapped under their namespaced categories.
pub fn run_round(
    extensions: &[Box<dyn Plugin>],
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
        let extension = &extensions[ix];
        let spec = extension.spec();
        let mut sink = PluginSink::default();
        let mut dropped = Vec::new();

        if coverage.is_none() {
            coverage = spec
                .reads_reports()
                .iter()
                .filter_map(|path| well_known.read(path).map(|text| (path, text)))
                .filter_map(|(path, text)| extension.ingest(path, text.as_bytes()))
                .find_map(|records| crate::coverage::assemble(records, contents));
        }

        let content = ContentView::new(contents, spec.requested_file_access());
        {
            let view = GraphView {
                files: &graph.files,
            };
            if spec.mutates_graph() {
                extension.contribute_roots(&view, &content, &mut sink);
            }
            extension.report_findings(&view, &content, &mut sink);
        }

        let (sunk_roots, sunk_findings, notes) = sink.into_parts();
        // Bridge honesty lines (a trapped call, a violated gate) reach the
        // report through the same described-drop channel.
        dropped.extend(notes);
        let mut applied_roots = 0u32;
        for r in sunk_roots {
            let (target, kind, confidence) = (r.target, r.kind, r.confidence);
            // The sink is shared between hooks, so an extension whose spec
            // declares `mutates_graph == false` can still CALL `root()` from
            // `report_findings` — those drop with a described line instead of
            // silently mutating a graph the cache was told is plugin-free.
            if !spec.mutates_graph() {
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
        for f in sunk_findings {
            if !spec.rules().iter().any(|r| r.name == f.rule) {
                dropped.push(format!("finding under undeclared rule `{}`", f.rule));
                continue;
            }
            let Some(subject) = target_subject(graph, &f.target) else {
                dropped.push(format!(
                    "finding not applied: {} does not resolve in the graph",
                    describe_target(&f.target)
                ));
                continue;
            };
            // The message is the discriminator: two findings under one rule on one
            // subject are distinct exactly when they say different things. The
            // confidence is the extension's own claim, carried verbatim.
            findings.push(Finding::new(
                Category::extension(spec.coordinate(), &f.rule),
                f.severity.advisory(),
                f.confidence,
                subject,
                &f.message.clone(),
                f.message,
            ));
            applied_findings += 1;
        }

        contributions.push(Contribution {
            coordinate: SmolStr::new(spec.coordinate()),
            // A RULE PACK runs no code: its roots were derived in `Dispatch`,
            // from its rules as data, and counted there. Same row, same
            // meaning — what this extension asserted about this project.
            roots: applied_roots
                + graph
                    .pack_roots
                    .get(spec.coordinate())
                    .copied()
                    .unwrap_or(0),
            findings: applied_findings,
            dropped,
            content_budget_cut: content.budget_cut(),
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
            // The query contract's resolver, so a plugin names a symbol the
            // way a user does — and an ambiguous name resolves to nothing
            // rather than to whichever declaration came first.
            let id = crate::query::declaration_named(graph, ix, name).ok()??;
            Some(graph.files[ix].evidence.subject_of(path, id))
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
