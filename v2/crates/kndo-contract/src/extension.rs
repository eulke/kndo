//! The unified extension surface — ONE species. An extension declares everything
//! it does in an [`ExtensionSpec`] and implements the hooks for the capabilities
//! it declared; the engine routes by the spec and never invokes an undeclared
//! hook. Claims gate the extraction cluster; activation gates conduct and
//! ingestion — gathering evidence is ungated fact-collection, emitting judgment
//! is gated. Phase discipline lives in the signatures: an extraction hook cannot
//! name the graph because no parameter provides one.

use crate::adapter::{PackageEntry, ProjectRoot, Resolution, ResolveContext, SourceFile};
use crate::evidence::{CoverageRecords, EvidenceSink, EvidenceStreams, RootKind};
use crate::finding::Severity;
use crate::vocab::{Confidence, ProjectPath};
use serde::Serialize;
use smol_str::SmolStr;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// One machine-checkable activation predicate — cheap, evaluated against what the
/// run already discovered, never by running extension code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ActivationRule {
    /// At least one discovered file matches this glob (e.g. `next.config.*`).
    FileExists(SmolStr),
    /// Some discovered manifest declares a dependency with this name, in any
    /// section — as reported by the claiming extensions through
    /// [`Extension::manifest_dependencies`], the one manifest pipeline.
    ManifestDependency(SmolStr),
}

/// When an extension's CONDUCT and INGESTION run (extraction is gated by claims
/// alone). `Always` is speakable on purpose: "always on and cheap" (a coverage
/// ingester) is a real posture, not an exemption with a paragraph of
/// justification. An empty rule list is the OTHER deliberate extreme — an
/// extension that can never self-activate, reachable only through another
/// extension's `dependencies` (a company framework whose users never depend on
/// it directly).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Activation {
    Always,
    AnyRule(Vec<ActivationRule>),
}

/// One rule an extension may emit findings under; the suffix of the namespaced
/// advisory category. A finding under an undeclared rule is dropped and recorded
/// — declaration is the contract, not decoration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleDescriptor {
    pub name: SmolStr,
    pub description: SmolStr,
}

/// Whether this extension contributes to graph assembly through
/// [`Extension::contribute_roots`]. Load-bearing, not a hint: any ACTIVE
/// graph-mutating extension bypasses the persisted graph cache for the run — the
/// surgical patch never re-invokes conduct hooks, so it can never safely reuse a
/// graph one influenced. An enum rather than a bool so the decision reads at the
/// call site, and an argument of [`ExtensionSpecBuilder::conduct`] rather than a
/// defaulted field so declaring conduct without deciding it does not compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutatesGraph {
    Yes,
    No,
}

impl MutatesGraph {
    pub fn as_bool(self) -> bool {
        matches!(self, MutatesGraph::Yes)
    }
}

/// `kndo:` is the built-in namespace: an external component carrying it is
/// rejected at load, which is what makes `dependencies: ["kndo:express"]`
/// unambiguous from any source.
pub fn is_reserved_coordinate(coordinate: &str) -> bool {
    coordinate.starts_with("kndo:")
}

/// What an extension IS, as data — the one manifest for every capability. Fields
/// come in three clusters with one gate each: extraction (gated by `claims`),
/// conduct (gated by `activation` + `mutates_graph`), ingestion (gated by
/// `activation` + `reads_reports`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtensionSpec {
    coordinate: SmolStr,
    version: u32,
    // -- extraction --
    suffixes: Vec<SmolStr>,
    claims: Vec<SmolStr>,
    emits: EvidenceStreams,
    manifests: Vec<SmolStr>,
    // -- conduct --
    /// Whether this spec went through the conduct stage at all. Data, not
    /// inference: an empty-but-conducting spec (an always-on ingester before its
    /// paths are declared) still owes the report a contribution row, and an
    /// extraction-only spec never appears in the round — a distinction field
    /// values alone cannot draw.
    conducts: bool,
    activation: Activation,
    mutates_graph: bool,
    dependencies: Vec<SmolStr>,
    requested_file_access: Vec<SmolStr>,
    rules: Vec<RuleDescriptor>,
    // -- ingestion --
    reads_reports: Vec<SmolStr>,
}

impl ExtensionSpec {
    /// Stage one of the two-stage builder: identity and the extraction cluster.
    /// The conduct and ingestion methods do not exist here — they live on the
    /// builder [`ExtensionSpecBuilder::conduct`] returns, which demands the two
    /// gates as arguments. Forgetting a gate is not a panic; it does not compile.
    pub fn builder(coordinate: &'static str, version: u32) -> ExtensionSpecBuilder {
        ExtensionSpecBuilder {
            spec: ExtensionSpec {
                coordinate: SmolStr::new_static(coordinate),
                version,
                suffixes: Vec::new(),
                claims: Vec::new(),
                emits: EvidenceStreams::none(),
                manifests: Vec::new(),
                // Inert neutrals for an extraction-only extension: activation
                // gates only conduct and ingestion, and with no conduct declared
                // there is nothing for these to gate.
                conducts: false,
                activation: Activation::Always,
                mutates_graph: false,
                dependencies: Vec::new(),
                requested_file_access: Vec::new(),
                rules: Vec::new(),
                reads_reports: Vec::new(),
            },
        }
    }

    pub fn coordinate(&self) -> &str {
        &self.coordinate
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// File suffixes this extension speaks (no leading dot), in
    /// resolution-candidate priority order. Analyses and resolution read this
    /// one list; claims derive from it at build time. Named for what it holds —
    /// "extension" already means the species.
    pub fn suffixes(&self) -> &[SmolStr] {
        &self.suffixes
    }

    pub fn claims(&self) -> &[SmolStr] {
        &self.claims
    }

    pub fn emits(&self) -> &EvidenceStreams {
        &self.emits
    }

    pub fn manifests(&self) -> &[SmolStr] {
        &self.manifests
    }

    /// Whether this spec declares conduct or ingestion at all — the engine's
    /// round runs over exactly the extensions for which this is true, and only
    /// those appear as contributions in the report.
    pub fn declares_conduct(&self) -> bool {
        self.conducts
    }

    pub fn activation(&self) -> &Activation {
        &self.activation
    }

    pub fn mutates_graph(&self) -> bool {
        self.mutates_graph
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

    /// Root-relative report paths the engine reads through its well-known channel
    /// (run output is gitignored — discovery never sees it) and pushes to
    /// [`Extension::ingest`], in declaration order.
    pub fn reads_reports(&self) -> &[SmolStr] {
        &self.reads_reports
    }
}

/// The owned-parts constructor for the wire boundary: a LOADED component's spec
/// arrives as data, not statics, so the builder's `&'static str` economy cannot
/// apply — and a parts struct rather than a parameter list, so no two same-typed
/// fields can swap silently. `conducts` is the loader's statement of which side
/// of the door the component's world sits on until the worlds unify.
#[derive(Debug, Default)]
pub struct ExtensionSpecParts {
    pub coordinate: SmolStr,
    pub version: u32,
    pub suffixes: Vec<SmolStr>,
    pub claims: Vec<SmolStr>,
    pub emits: EvidenceStreams,
    pub manifests: Vec<SmolStr>,
    pub conducts: bool,
    pub activation: Activation,
    pub mutates_graph: bool,
    pub dependencies: Vec<SmolStr>,
    pub requested_file_access: Vec<SmolStr>,
    pub rules: Vec<RuleDescriptor>,
    pub reads_reports: Vec<SmolStr>,
}

impl Default for Activation {
    /// The inert neutral (see [`ExtensionSpec::builder`]); a conducting spec
    /// assembled from wire parts carries the activation its component declared.
    fn default() -> Self {
        Activation::Always
    }
}

impl Default for EvidenceStreams {
    fn default() -> Self {
        EvidenceStreams::none()
    }
}

impl From<ExtensionSpecParts> for ExtensionSpec {
    fn from(parts: ExtensionSpecParts) -> ExtensionSpec {
        ExtensionSpec {
            coordinate: parts.coordinate,
            version: parts.version,
            suffixes: parts.suffixes,
            claims: parts.claims,
            emits: parts.emits,
            manifests: parts.manifests,
            conducts: parts.conducts,
            activation: parts.activation,
            mutates_graph: parts.mutates_graph,
            dependencies: parts.dependencies,
            requested_file_access: parts.requested_file_access,
            rules: parts.rules,
            reads_reports: parts.reads_reports,
        }
    }
}

/// Declaring suffixes IS claiming them: each declared suffix derives its
/// `**/*.<ext>` claim glob, in declaration order. The one spelling of the rule,
/// called by every spec builder that speaks extensions.
pub(crate) fn declare_suffixes(
    suffixes: &mut Vec<SmolStr>,
    claims: &mut Vec<SmolStr>,
    declared: &[&'static str],
) {
    for ext in declared {
        suffixes.push(SmolStr::new_static(ext));
        claims.push(SmolStr::from(format!("**/*.{ext}")));
    }
}

/// Identity + extraction. See [`ExtensionSpec::builder`].
pub struct ExtensionSpecBuilder {
    spec: ExtensionSpec,
}

impl ExtensionSpecBuilder {
    /// Declare the file suffixes this extension speaks (no leading dot), in
    /// resolution-candidate priority order — see [`declare_suffixes`]; `claims`
    /// stays for patterns that are not extension-shaped.
    pub fn suffixes(mut self, suffixes: &[&'static str]) -> Self {
        declare_suffixes(&mut self.spec.suffixes, &mut self.spec.claims, suffixes);
        self
    }

    pub fn claims(mut self, globs: &[&'static str]) -> Self {
        self.spec
            .claims
            .extend(globs.iter().map(|g| SmolStr::new_static(g)));
        self
    }

    /// Omitted ⇒ `EvidenceStreams::none()` — the default-compatibility rule.
    pub fn emits(mut self, streams: EvidenceStreams) -> Self {
        self.spec.emits = streams;
        self
    }

    /// Omitted ⇒ no manifests consulted and [`Extension::roots`] never called —
    /// the default-compatibility rule.
    pub fn manifests(mut self, globs: &[&'static str]) -> Self {
        self.spec.manifests = globs.iter().map(|g| SmolStr::new_static(g)).collect();
        self
    }

    /// The key to stage two. Declaring any conduct or ingestion — rules,
    /// dependencies, content access, report paths — requires deciding its two
    /// gates first, as arguments, not as defaults: a framework extension that
    /// fell into `Always` would fire roots on every project on the planet, and a
    /// forgotten `MutatesGraph` answer either breaks incremental analysis for
    /// everyone or corrupts it. The dependency-only posture is written by hand:
    /// `Activation::AnyRule(vec![])`.
    pub fn conduct(
        mut self,
        activation: Activation,
        mutates_graph: MutatesGraph,
    ) -> ConductBuilder {
        self.spec.conducts = true;
        self.spec.activation = activation;
        self.spec.mutates_graph = mutates_graph.as_bool();
        ConductBuilder { spec: self.spec }
    }

    pub fn build(self) -> ExtensionSpec {
        self.spec
    }
}

/// Stage two: conduct and ingestion, reachable only through
/// [`ExtensionSpecBuilder::conduct`].
pub struct ConductBuilder {
    spec: ExtensionSpec,
}

impl ConductBuilder {
    pub fn rule(mut self, name: &'static str, description: &'static str) -> Self {
        assert!(
            !name.contains('/'),
            "rule names must not contain '/': coordinates legally do, so a slash \
             here would let two (coordinate, rule) pairs spell one category"
        );
        self.spec.rules.push(RuleDescriptor {
            name: SmolStr::new_static(name),
            description: SmolStr::new_static(description),
        });
        self
    }

    /// Coordinates of extensions this one needs running beside it. An active
    /// extension activates its dependencies' conduct and ingestion, transitively
    /// — the only path for a component whose framework is an INDIRECT dependency.
    pub fn dependencies(mut self, coordinates: &[&'static str]) -> Self {
        self.spec.dependencies = coordinates.iter().map(|c| SmolStr::new_static(c)).collect();
        self
    }

    /// Globs over discovered files this extension may read through
    /// [`ContentView`]; a path outside them reads as absent.
    pub fn requested_file_access(mut self, globs: &[&'static str]) -> Self {
        self.spec.requested_file_access = globs.iter().map(|g| SmolStr::new_static(g)).collect();
        self
    }

    /// Root-relative report paths for [`Extension::ingest`], tried in order.
    pub fn reads_reports(mut self, paths: &[&'static str]) -> Self {
        self.spec.reads_reports = paths.iter().map(|p| SmolStr::new_static(p)).collect();
        self
    }

    pub fn build(self) -> ExtensionSpec {
        self.spec
    }
}

/// An extension's own severity vocabulary — deliberately not [`Severity`]: the
/// engine maps it into the advisory channel, so an extension can never construct
/// a gate-eligible finding directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConductSeverity {
    Error,
    Warning,
    Info,
}

impl ConductSeverity {
    /// The advisory mapping the engine applies; findings so mapped ride the
    /// namespaced categories the gate never counts.
    pub fn advisory(self) -> Severity {
        match self {
            ConductSeverity::Error => Severity::Error,
            ConductSeverity::Warning => Severity::Warning,
            ConductSeverity::Info => Severity::Info,
        }
    }
}

/// What a conduct contribution points at. A target that resolves to nothing is a
/// silent no-op in the graph and a described line in the contribution — the
/// author debugging "contributed 0 roots" needs the why; the run never crashes
/// on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConductTarget {
    File(ProjectPath),
    Symbol { path: ProjectPath, name: SmolStr },
}

/// The graph as conduct hooks may see it: paths and membership, no internals —
/// the surface the WASM boundary already proved sufficient. The engine
/// implements it; extensions only consume it.
pub trait GraphAccess {
    /// Every path in the assembled graph, in path order.
    fn paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_>;

    /// Membership without the full list.
    fn contains(&self, path: &ProjectPath) -> bool;
}

/// The write side of one extension's conduct round.
/// One liveness anchor a conduct round contributed — the wire record's native
/// twin, so consumers name fields instead of destructuring positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributedRoot {
    pub target: ConductTarget,
    pub kind: RootKind,
    pub confidence: Confidence,
}

/// One advisory finding a conduct round contributed. `confidence` is the
/// extension's own claim — a fact parsed from a lockfile is `Certain`, a
/// heuristic is `Possible`; the engine carries it into the finding verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributedFinding {
    pub rule: SmolStr,
    pub severity: ConductSeverity,
    pub target: ConductTarget,
    pub confidence: Confidence,
    pub message: String,
}

#[derive(Default)]
pub struct ConductSink {
    roots: Vec<ContributedRoot>,
    findings: Vec<ContributedFinding>,
    notes: Vec<String>,
}

impl ConductSink {
    /// Anchor liveness the language cannot see: a route file, a DI-registered
    /// symbol. Applied to the graph only from extensions whose spec declares
    /// `mutates_graph` — the declaration is self-enforcing, because the hook
    /// that fills this is only invoked on those; anything smuggled through the
    /// shared sink drops with a described line.
    pub fn root(&mut self, target: ConductTarget, kind: RootKind, confidence: Confidence) {
        self.roots.push(ContributedRoot {
            target,
            kind,
            confidence,
        });
    }

    /// An advisory finding under one of the spec's declared rules; undeclared
    /// rules drop with a described line on the contribution.
    pub fn finding(
        &mut self,
        rule: &str,
        severity: ConductSeverity,
        target: ConductTarget,
        confidence: Confidence,
        message: impl Into<String>,
    ) {
        self.findings.push(ContributedFinding {
            rule: SmolStr::new(rule),
            severity,
            target,
            confidence,
            message: message.into(),
        });
    }

    /// A bridge-level honesty line: something went wrong OUTSIDE the guest's
    /// declared surface (a trap mid-conduct, a violated phase gate) and the
    /// contribution must say so — a vanished call and a clean empty round must
    /// never look alike. Lands in the contribution's dropped list.
    pub fn note(&mut self, line: impl Into<String>) {
        self.notes.push(line.into());
    }

    /// Everything the round wrote, for the engine to judge: roots, findings,
    /// then bridge notes, each in emission order.
    pub fn into_parts(self) -> (Vec<ContributedRoot>, Vec<ContributedFinding>, Vec<String>) {
        (self.roots, self.findings, self.notes)
    }
}

/// Scoped content reads as conduct hooks see them — the same surface natively
/// (backed by [`ContentView`]) and across the WASM boundary (backed by the
/// host's prefetched snapshot), so an extension's conduct code is written once
/// against one shape. Every miss is `None`; the budget lives behind the
/// implementation, and its cut is reported on the contribution.
pub trait ContentAccess {
    /// The discovered paths the scope admits, in path order — names only,
    /// nothing charged.
    fn readable_paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_>;

    fn read(&self, path: &ProjectPath) -> Option<&[u8]>;
}

pub const CONTENT_MAX_FILES: usize = 200;
pub const CONTENT_MAX_BYTES: usize = 8 * 1024 * 1024;

/// Budgeted, glob-scoped reads over the run's already-read file contents — no
/// second disk walk. Every miss (no glob match, unknown path, budget cut) is the
/// same `None`; the cut itself is reported on the contribution, never silent.
pub struct ContentView<'a> {
    contents: &'a BTreeMap<ProjectPath, &'a [u8]>,
    globs: Vec<globset::GlobMatcher>,
    budget: RefCell<ContentBudget>,
}

#[derive(Default)]
struct ContentBudget {
    files: usize,
    bytes: usize,
    seen: BTreeSet<ProjectPath>,
    cut_off: bool,
}

impl<'a> ContentView<'a> {
    /// A view scoped to `access` (an extension spec's `requested_file_access`).
    /// A malformed glob is simply never satisfied — declared input degrades, the
    /// run never aborts on it.
    pub fn new(contents: &'a BTreeMap<ProjectPath, &'a [u8]>, access: &[SmolStr]) -> Self {
        let globs = access
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

    /// The discovered paths this view's globs admit, in path order — names only,
    /// nothing charged. What a prefetching consumer (the WASM bridge's
    /// before-instantiation snapshot) walks so it never re-implements the glob
    /// scope; reading each is still [`ContentView::read`], budget and all.
    pub fn readable_paths(&self) -> impl Iterator<Item = &'a ProjectPath> + '_ {
        self.contents
            .keys()
            .filter(|p| self.globs.iter().any(|g| g.is_match(p.as_str())))
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

    /// Whether the budget cut a read this round — reported on the contribution.
    pub fn budget_cut(&self) -> bool {
        self.budget.borrow().cut_off
    }
}

impl ContentAccess for ContentView<'_> {
    fn readable_paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_> {
        Box::new(ContentView::readable_paths(self))
    }

    fn read(&self, path: &ProjectPath) -> Option<&[u8]> {
        ContentView::read(self, path)
    }
}

/// The one door. Every hook has an abstaining default; [`Extension::spec`] is the
/// only obligation. The engine invokes a hook only when the spec declares its
/// capability: extraction hooks for claimed files, manifest hooks for declared
/// manifest globs, conduct hooks under activation (+ `mutates_graph` for roots),
/// ingestion under activation for declared report paths.
pub trait Extension: Send + Sync {
    fn spec(&self) -> &ExtensionSpec;

    // ---- extraction: per file, pure in (bytes, spec version); gated by claims ----

    /// Extract one claimed file's evidence. Never fails: extraction degrades
    /// through the sink's diagnostics.
    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let _ = (file, out);
    }

    /// Resolve an import specifier written in `from` against the project.
    /// `Unresolved` is the keep-alive default for anything the extension cannot
    /// place.
    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        let _ = (from, specifier, cx);
        Resolution::Unresolved
    }

    /// The roots one manifest declares — transcription of a project FACT, which
    /// is why claims gate it and activation does not. Called for every discovered
    /// file matching the spec's manifest globs. Unparseable or dangling entries
    /// degrade to absence: a root that anchors nothing accuses nothing.
    fn roots(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<ProjectRoot> {
        let _ = (manifest, cx);
        Vec::new()
    }

    /// The packages one manifest declares, fed back to every extension's
    /// `resolve` through [`ResolveContext::package`].
    fn packages(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
        let _ = (manifest, cx);
        Vec::new()
    }

    /// The dependency NAMES one manifest declares, every section alike — what
    /// activation's `ManifestDependency` rules evaluate against, through the same
    /// discovered-manifest pipeline `roots` and `packages` ride.
    fn manifest_dependencies(&self, manifest: &SourceFile<'_>) -> Vec<SmolStr> {
        let _ = manifest;
        Vec::new()
    }

    /// The files whose names `path` SEES with no import naming them — the rest
    /// of its shared name scope, in the languages where that scope is bigger
    /// than the file (every non-test sibling of a Go file's package; a test
    /// file sees the whole package). Directional, deliberately: a test sees
    /// `src/main`, never the reverse. The engine draws one reachability edge
    /// per seen file and pools references over this sight. Depends only on
    /// `path` and the file SET, never on content — which is what lets a
    /// persisted graph trust it while only contents change. The default — sees
    /// nothing beyond itself — reproduces pre-capability behavior.
    fn sees(&self, path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
        let _ = (path, cx);
        Vec::new()
    }

    /// The files a `Scoped { scope }` declaration at `path` can legally be seen
    /// FROM — the region behind the adapter's own scope word, enumerated from
    /// paths and manifests only, never contents (the `sees` stability class: a
    /// persisted graph trusts it while contents change). `None` = this adapter
    /// cannot bound that token — the declaration is treated exactly as Exported,
    /// keep-alive. The default answers nothing, reproducing pre-capability
    /// behavior; `unused` (and `internal-only` when it lands) are the consumers,
    /// and the Kotlin `internal` fixtures the conformance case.
    fn seen_from(
        &self,
        path: &ProjectPath,
        scope: &str,
        cx: &ResolveContext<'_>,
    ) -> Option<Vec<ProjectPath>> {
        let _ = (path, scope, cx);
        None
    }

    // ---- conduct: post-graph, once; gated by activation ----

    /// Liveness the language cannot know. Invoked only when the spec declares
    /// `mutates_graph`.
    fn contribute_roots(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        let _ = (graph, content, out);
    }

    /// Advisory findings under the spec's declared rules.
    fn report_findings(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        let _ = (graph, content, out);
    }

    // ---- ingestion: pure; gated by activation + reads_reports ----

    /// Parse one report the engine located through the spec's `reads_reports`
    /// paths and read for you: what the report STATES, never a line table —
    /// mapping records onto the project is the engine's, uniformly for every
    /// ingester. `None` when the bytes are not this extension's format.
    fn ingest(&self, report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        let _ = (report_path, content);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_stage_builder_produces_the_declared_spec() {
        let extraction_only = ExtensionSpec::builder("kndo:kmini", 3)
            .suffixes(&["kmini"])
            .claims(&["**/legacy.km"])
            .manifests(&["kmini.toml"])
            .build();
        assert_eq!(extraction_only.coordinate(), "kndo:kmini");
        assert_eq!(extraction_only.version(), 3);
        assert_eq!(extraction_only.claims(), ["**/*.kmini", "**/legacy.km"]);
        assert_eq!(extraction_only.manifests(), ["kmini.toml"]);
        // The inert neutrals: nothing declared for them to gate.
        assert_eq!(extraction_only.activation(), &Activation::Always);
        assert!(!extraction_only.mutates_graph());
        assert!(extraction_only.rules().is_empty());
        assert!(extraction_only.reads_reports().is_empty());

        let conduct = ExtensionSpec::builder("acme:framework", 1)
            .conduct(
                Activation::AnyRule(vec![ActivationRule::ManifestDependency(
                    SmolStr::new_static("acme-framework"),
                )]),
                MutatesGraph::Yes,
            )
            .rule("routes", "route files the framework wires")
            .dependencies(&["kndo:express"])
            .requested_file_access(&["routes/**"])
            .build();
        assert!(conduct.mutates_graph());
        assert_eq!(conduct.dependencies(), ["kndo:express"]);
        assert_eq!(conduct.rules().len(), 1);

        let ingester = ExtensionSpec::builder("kndo:coverage-lcov", 1)
            .conduct(Activation::Always, MutatesGraph::No)
            .reads_reports(&["coverage/lcov.info", "lcov.info"])
            .build();
        assert!(!ingester.mutates_graph());
        assert_eq!(
            ingester.reads_reports(),
            ["coverage/lcov.info", "lcov.info"]
        );
    }

    #[test]
    fn every_hook_defaults_to_abstention() {
        struct Bare(ExtensionSpec);
        impl Extension for Bare {
            fn spec(&self) -> &ExtensionSpec {
                &self.0
            }
        }
        struct NoGraph;
        impl GraphAccess for NoGraph {
            fn paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_> {
                Box::new(std::iter::empty())
            }
            fn contains(&self, _: &ProjectPath) -> bool {
                false
            }
        }

        let bare = Bare(ExtensionSpec::builder("demo:bare", 1).build());
        let cx_files = BTreeSet::new();
        let cx = ResolveContext::new(&cx_files);
        let from = ProjectPath::new("a.js");
        assert_eq!(bare.resolve(&from, "./b", &cx), Resolution::Unresolved);
        assert!(bare.sees(&from, &cx).is_empty());
        assert_eq!(bare.ingest("coverage/lcov.info", b"TN:"), None);

        let contents = BTreeMap::new();
        let view = ContentView::new(&contents, bare.spec().requested_file_access());
        let mut sink = ConductSink::default();
        bare.contribute_roots(&NoGraph, &view, &mut sink);
        bare.report_findings(&NoGraph, &view, &mut sink);
        let (roots, findings, _) = sink.into_parts();
        assert!(roots.is_empty() && findings.is_empty() && !view.budget_cut());
    }

    #[test]
    fn content_view_scopes_reads_to_the_declared_globs() {
        let a = ProjectPath::new("routes/web.rb");
        let b = ProjectPath::new("secrets.env");
        let mut contents: BTreeMap<ProjectPath, &[u8]> = BTreeMap::new();
        contents.insert(a.clone(), b"root 'x'");
        contents.insert(b.clone(), b"KEY=1");
        let access = [SmolStr::new_static("routes/**")];
        let view = ContentView::new(&contents, &access);
        assert_eq!(view.read(&a), Some(b"root 'x'".as_slice()));
        assert_eq!(view.read(&b), None);
        assert_eq!(view.readable_paths().collect::<Vec<_>>(), [&a]);
    }
}
