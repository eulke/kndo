//! Analyses weigh evidence and return verdicts; the engine derives abstention before
//! any analysis runs — from the pairing rule (an analysis NAMES the streams it weighs
//! in [`Analysis::requires`]; files whose claiming adapter did not declare them form
//! its unmeasured set) and from each analysis's own whole-run precondition
//! ([`Analysis::abstains`]). Never an `if adapter == …`, never a per-analysis flag.
//!
//! Reachability is computed once, per root color, and shared: every analysis reads
//! the same [`Reachability`] instead of building its own.

mod crap;
mod cyclic;
mod dependency;
mod duplicate;
mod internal_only;
mod private_type_leak;
mod test_only;
mod undeclared;
mod unresolved;
mod untested;
mod unused;
mod version_skew;

pub use crap::{CRAP_THRESHOLD, Crap};
pub use cyclic::Cyclic;
pub use duplicate::Duplicate;
pub use internal_only::InternalOnly;
pub use private_type_leak::PrivateTypeLeak;
pub use test_only::TestOnly;
pub use undeclared::Undeclared;
pub use unresolved::Unresolved;
pub use untested::Untested;
pub use unused::Unused;
pub use version_skew::VersionSkew;

use crate::graph::Graph;
use kndo_contract::evidence::{EvidenceStream, RootKind, RootTarget};
use kndo_contract::finding::{Finding, sort_findings};
use kndo_contract::vocab::Category;
use kndo_contract::vocab::ProjectPath;
use serde::Serialize;
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Which root colors reach each file: seeded by the file's own roots of that kind
/// (extraction evidence and manifest anchors alike), propagated over resolved import
/// edges. Computed once per run; every analysis reads the same answer.
pub struct Reachability {
    production: Vec<bool>,
    test: Vec<bool>,
    tooling: Vec<bool>,
}

impl Reachability {
    pub fn compute(graph: &Graph) -> Self {
        Reachability {
            production: flood(graph, RootKind::Production),
            test: flood(graph, RootKind::Test),
            tooling: flood(graph, RootKind::Tooling),
        }
    }

    pub fn by(&self, kind: RootKind) -> &[bool] {
        match kind {
            RootKind::Production => &self.production,
            RootKind::Test => &self.test,
            RootKind::Tooling => &self.tooling,
        }
    }

    /// Reached by any color at all.
    pub fn any(&self, file: usize) -> bool {
        self.production[file] || self.test[file] || self.tooling[file]
    }
}

impl RunContext<'_> {
    /// What the extension claiming `coordinate` declared about its language.
    pub fn capabilities_of(&self, coordinate: &smol_str::SmolStr) -> Option<&DeclaredCapabilities> {
        self.capabilities
            .iter()
            .find(|(c, _)| c == coordinate)
            .map(|(_, caps)| caps)
    }
}

/// Does this file itself carry a root of `kind` — its own evidence, dispatch
/// or an anchor?
pub fn has_root_of(graph: &Graph, file: usize, kind: RootKind) -> bool {
    graph.files[file].roots().any(|r| r.kind == kind)
}

/// Is this file a test as a whole — a whole-file Test root, whoever said it?
/// A production file with an inline test module carries a declaration-targeted
/// Test root and is NOT one: its imports serve production.
pub fn is_test_file(graph: &Graph, file: usize) -> bool {
    graph.files[file]
        .roots()
        .any(|r| r.kind == RootKind::Test && matches!(r.target, RootTarget::WholeFile))
}

fn flood(graph: &Graph, kind: RootKind) -> Vec<bool> {
    let n = graph.files.len();
    let mut reached = vec![false; n];
    let mut queue: Vec<usize> = (0..n).filter(|&i| has_root_of(graph, i, kind)).collect();
    for &i in &queue {
        reached[i] = true;
    }
    while let Some(i) = queue.pop() {
        // Unit mates are edges like imports: reaching one file of a shared-scope
        // unit reaches what its names can see.
        let f = &graph.files[i];
        for &t in f.imports.iter().chain(&f.sees) {
            let t = t as usize;
            if !reached[t] {
                reached[t] = true;
                queue.push(t);
            }
        }
    }
    reached
}

/// Everything one run shares across analyses; each analysis reads what it needs.
pub struct RunContext<'a> {
    pub graph: &'a Graph,
    pub reach: Reachability,
    /// The navigation index — the keep rules' one home, shared between the
    /// `unused` judgment and the query verbs.
    pub index: crate::navigate::Index,
    pub coverage: Option<crate::coverage::Coverage>,
    /// What each extension declared about its language, by coordinate — the
    /// ONE per-adapter input. An analysis asks it the question it needs
    /// ([`RunContext::capabilities_of`]) rather than receiving a list per
    /// capability, so a new language fact reaches every analysis without
    /// moving a signature.
    pub capabilities: &'a [(smol_str::SmolStr, DeclaredCapabilities)],
    /// Parallel to `Graph::manifest_declarations`: why each manifest's
    /// dependency usage goes unjudged this run, `None` where it is judged —
    /// derived once, read by every dependency-subject verdict.
    pub manifests: Vec<Option<AbstentionReason>>,
}

pub struct AnalysisContext<'a> {
    pub run: &'a RunContext<'a>,
    /// Per-file, for THIS analysis: did the claiming adapter declare every stream it
    /// requires? Unmeasured files must produce no findings.
    pub measured: &'a [bool],
    /// Where an analysis records the parts of the run it declined to judge —
    /// drained by the engine into the run's abstentions under this analysis's
    /// category, so a partial silence is as legible as a whole-run one.
    abstentions: std::cell::RefCell<Vec<(AbstentionReason, AbstentionScope)>>,
}

impl<'a> AnalysisContext<'a> {
    pub fn graph(&self) -> &'a Graph {
        self.run.graph
    }

    pub fn abstain(&self, reason: AbstentionReason, scope: AbstentionScope) {
        self.abstentions.borrow_mut().push((reason, scope));
    }
}

pub trait Analysis: Sync {
    fn id(&self) -> &'static str;
    fn category(&self) -> Category;
    fn requires(&self) -> &'static [EvidenceStream] {
        &[]
    }
    /// A precondition on the run as a whole. `Some` means this run cannot be judged
    /// at all — the engine records the abstention and never calls [`Analysis::run`].
    /// Degrade-toward-keep-alive at analysis scale: silence over accusation.
    fn abstains(&self, _run: &RunContext<'_>) -> Option<AbstentionReason> {
        None
    }
    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum AbstentionScope {
    WholeRun,
    Files {
        unmeasured: u32,
    },
    /// Dependency subjects only: this many declaring manifests went unjudged,
    /// every file still judged as usual.
    Manifests {
        unjudged: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum AbstentionReason {
    StreamsNotDeclared {
        missing: Vec<EvidenceStream>,
    },
    /// No root anchors anything in the whole graph. Reachability judged from zero
    /// roots would accuse every file at once; that is a missing-evidence condition,
    /// not a verdict.
    NoRootsAnywhere,
    /// No test root anchors anything: with zero test evidence, "tests never reach
    /// this" describes every declaration equally and accuses none.
    NoTestRootsAnywhere,
    /// The manifest's claiming extension declares no spelling that derives a
    /// package from an import specifier (`DependencyIdentity::Underivable`), so
    /// "nothing imports this dependency" cannot be told from "the imports spell
    /// it differently" — JVM coordinates, Swift products, Python distributions.
    SpecifierIdentityUnderivable,
    /// Files no extension claims sit inside the package with a suffix the
    /// claiming extension declares can import
    /// (`ExtensionSpec::dependency_importers`) — a `.vue` component, an `.html`
    /// page — so an import of the dependency may exist where nothing can see it.
    UnclaimedImporters {
        suffixes: Vec<SmolStr>,
    },
    /// No root reaches any file the package owns: dead or apparatus, its
    /// dependencies are moot and the file findings already say so.
    NothingReachesOwnedFiles,
    /// Every file the package owns is a test: a fixture package, whose
    /// dependencies serve the tests by construction.
    OwnedFilesAreTests,
    /// No coverage report was ingested this run: a verdict with a measured
    /// coverage factor cannot be reached for any function at once.
    NoCoverageIngested,
    /// The ingested report never instrumented these files: their functions'
    /// coverage is unknown, not zero.
    NoCoverageRecord,
}

impl fmt::Display for AbstentionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AbstentionReason::StreamsNotDeclared { missing } => {
                write!(f, "required evidence streams not declared: {missing:?}")
            }
            AbstentionReason::NoRootsAnywhere => {
                write!(f, "no root anchors any file in this graph")
            }
            AbstentionReason::NoTestRootsAnywhere => {
                write!(f, "no test root anchors any file in this graph")
            }
            AbstentionReason::SpecifierIdentityUnderivable => {
                write!(
                    f,
                    "the claiming extension derives no package identity from import specifiers"
                )
            }
            AbstentionReason::UnclaimedImporters { suffixes } => {
                write!(f, "files nothing claims could import: ")?;
                for (i, suffix) in suffixes.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, ".{suffix}")?;
                }
                Ok(())
            }
            AbstentionReason::NothingReachesOwnedFiles => {
                write!(f, "no root reaches any file the package owns")
            }
            AbstentionReason::OwnedFilesAreTests => {
                write!(f, "every file the package owns is a test")
            }
            AbstentionReason::NoCoverageIngested => {
                write!(f, "no coverage report ingested this run")
            }
            AbstentionReason::NoCoverageRecord => {
                write!(f, "the coverage report never instrumented these files")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Abstention {
    pub category: Category,
    pub reason: AbstentionReason,
    pub scope: AbstentionScope,
}

/// What one run's analyses produced — and which categories actually JUDGED, so
/// suppression can tell a stale allow from an allow over an un-judged category.
pub struct AnalysisOutcome {
    pub findings: Vec<Finding>,
    pub abstained: Vec<Abstention>,
    /// Categories whose analysis ran this run (not whole-run-abstained). A category
    /// absent here — abstained, or no analysis ships for it yet — is un-judged, and
    /// nothing about it (a suppression included) may be called stale.
    pub judged: std::collections::BTreeSet<Category>,
    /// The dependency declarations the usage judgment counted, by declaring
    /// manifest — health's dependency universe. Empty when `unused` never ran.
    pub dependency_universe: BTreeMap<ProjectPath, BTreeSet<SmolStr>>,
}

/// The judgment capabilities one extension declared, carried spec → analyses →
/// report. One shape, so "why does kndo (not) report X for this language" has
/// one answer in the run and the same one in the envelope.
///
/// The default is an extension that declared NOTHING, and every field's own
/// default is silence — which is what an unregistered coordinate degrades to
/// wherever a lookup misses.
#[derive(Debug, Clone, Default)]
pub struct DeclaredCapabilities {
    pub narrowable_scopes: Vec<smol_str::SmolStr>,
    pub export_narrowing: kndo_contract::extension::ExportNarrowing,
    pub import_cycles: kndo_contract::extension::CycleTolerance,
    pub dependency_scoping: kndo_contract::extension::DependencyScoping,
    pub dependency_identity: kndo_contract::extension::DependencyIdentity,
    /// The reaches this language can spell, narrowest first, each under this
    /// language's own word for it — see [`kndo_contract::extension::Step`].
    pub ladder: Vec<kndo_contract::extension::Step>,
    /// How far one of this language's namespaces reaches across the project's
    /// units — see [`kndo_contract::extension::NamespaceSpan`].
    pub namespace_span: kndo_contract::extension::NamespaceSpan,
}

pub fn run_all(
    graph: &Graph,
    coverage: Option<crate::coverage::Coverage>,
    capabilities: &[(smol_str::SmolStr, DeclaredCapabilities)],
    analyses: &[&dyn Analysis],
) -> AnalysisOutcome {
    let reach = Reachability::compute(graph);
    let index = crate::navigate::Index::build(graph, &reach, capabilities);
    let manifests = dependency::eligibility(graph, &reach);
    let run = RunContext {
        graph,
        reach,
        index,
        coverage,
        capabilities,
        manifests,
    };
    let mut findings = Vec::new();
    let mut abstained = Vec::new();
    let mut judged = std::collections::BTreeSet::new();

    for analysis in analyses {
        if let Some(reason) = analysis.abstains(&run) {
            abstained.push(Abstention {
                category: analysis.category(),
                reason,
                scope: AbstentionScope::WholeRun,
            });
            continue;
        }
        let requires = analysis.requires();
        let measured: Vec<bool> = graph
            .files
            .iter()
            .map(|f| requires.iter().all(|s| f.evidence.declared.contains(*s)))
            .collect();
        let unmeasured = measured.iter().filter(|m| !**m).count() as u32;
        if unmeasured > 0 {
            let missing: Vec<EvidenceStream> = requires.to_vec();
            let scope = if unmeasured as usize == graph.files.len() {
                AbstentionScope::WholeRun
            } else {
                AbstentionScope::Files { unmeasured }
            };
            abstained.push(Abstention {
                category: analysis.category(),
                reason: AbstentionReason::StreamsNotDeclared { missing },
                scope,
            });
            if unmeasured as usize == graph.files.len() {
                // Nothing is measured — the analysis has nothing to run on.
                continue;
            }
        }
        let cx = AnalysisContext {
            run: &run,
            measured: &measured,
            abstentions: Default::default(),
        };
        findings.extend(analysis.run(&cx));
        for (reason, scope) in cx.abstentions.into_inner() {
            abstained.push(Abstention {
                category: analysis.category(),
                reason,
                scope,
            });
        }
        judged.insert(analysis.category());
    }

    let dependency_universe = if judged.contains(&Category::UNUSED) {
        dependency::universe(&run)
    } else {
        BTreeMap::new()
    };
    sort_findings(&mut findings);
    AnalysisOutcome {
        findings,
        abstained,
        judged,
        dependency_universe,
    }
}
