//! The one output type. No wall-clock fields by design — reports are compared
//! byte-for-byte by the equivalence gates, and run metadata that varies per run
//! (timings, timestamps) joins at the frontend edge when a milestone needs it.

use crate::analysis::Abstention;
use crate::health::Health;
use crate::plugin::Contribution;
use crate::suppress::SuppressedSummary;
use kndo_contract::evidence::DiagnosticLevel;
use kndo_contract::finding::Finding;
use kndo_contract::vocab::ProjectPath;
use serde::Serialize;
use smol_str::SmolStr;

pub const REPORT_SCHEMA: &str = "kndo-v2/m6";

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PluginRun {
    pub id: SmolStr,
    pub files: u32,
    /// The judgment capabilities this extension declared — the one-row answer
    /// to "why does kndo (not) report X for this language". What a unit of
    /// this ecosystem publishes: under `exports`, every exported declaration is
    /// the outside world's and `internal-only`'s Exported rung never fires;
    /// under `entries`, only what an entry exports is.
    pub published_surface: kndo_contract::plugin::PublishedSurface,
    /// Whether the language calls import cycles a hazard (`cyclic` reads it);
    /// `tolerated` is why a cycle-free-by-compiler language reports none.
    pub import_cycles: kndo_contract::plugin::CycleTolerance,
    /// Whether the manifests this extension reads have dependency sections
    /// (`scoped`) or one flat requirement list (`unscoped`) — under `unscoped`,
    /// `test-only` never fires on a dependency: there is no section to move it to.
    pub dependency_scoping: kndo_contract::plugin::DependencyScoping,
    /// How this extension's import specifiers name a declared dependency —
    /// `underivable` is why no dependency finding can exist for its manifests.
    pub dependency_identity: kndo_contract::plugin::DependencyIdentity,
    /// The reaches this language can spell, narrowest first, each under the
    /// word this language uses for it (`internal-only` reads both: the rungs
    /// to know a narrower one exists, the word to say so). Empty means the
    /// language states no ladder and that analysis stays silent for its files.
    #[serde(
        default,
        skip_serializing_if = "kndo_contract::plugin::Ladder::is_empty"
    )]
    pub ladder: kndo_contract::plugin::Ladder,
}

/// What this report's `findings`/`fixed` split was computed against. `full` is a
/// tree against its baseline file (all current findings when none exists); the
/// diff modes compare two TREES — base never includes the baseline file, so a
/// baselined finding reintroduced by a change reads as new debt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Mode {
    /// The whole tree, split against the on-disk baseline when one exists.
    Full,
    /// What `git commit` would commit (the index) against HEAD.
    Staged,
    /// The worktree against the merge-base with a named ref.
    Diff,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RunInfo {
    /// Stamped as a CONST in the generated schema, from the same `REPORT_SCHEMA` the report
    /// writes — a report from any other envelope version fails validation instead of
    /// drifting through.
    #[cfg_attr(feature = "schema", schemars(schema_with = "schema_version_const"))]
    pub schema: &'static str,
    pub mode: Mode,
    /// The category narrowing this run was asked for, verbatim — absent when the
    /// run judged everything. What a narrowed run does not list, it did not judge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<crate::session::Categories>,
    pub files_discovered: u32,
    pub files_claimed: u32,
    pub extensions: Vec<PluginRun>,
}

#[cfg(feature = "schema")]
fn schema_version_const(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({ "const": REPORT_SCHEMA })
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReportDiagnostic {
    pub path: ProjectPath,
    pub level: DiagnosticLevel,
    pub message: String,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Report {
    pub run: RunInfo,
    /// The project-level aggregate over the CURRENT tree — the baseline hides
    /// findings from the listing below, never from health. Absent when
    /// reachability itself abstained (the abstention entry says why).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<Health>,
    /// In the diff modes, the BASE tree's health — a pure function of the tree
    /// the invocation pinned, never cross-run state. Absent in `full` mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_health: Option<Health>,
    /// New relative to the baseline; all current findings when none exists.
    pub findings: Vec<Finding>,
    /// Baseline entries the current run no longer produces.
    pub fixed: Vec<Finding>,
    /// Current findings the baseline already carries — counted, not repeated.
    pub baselined: u32,
    pub abstained: Vec<Abstention>,
    pub suppressed: SuppressedSummary,
    /// One entry per ACTIVE plugin, in registration order — what it asserted, what
    /// missed, whether its content budget cut. Always present, so a plugin that
    /// contributed nothing is visibly distinct from a plugin that never ran.
    /// JSON key `plugins` is envelope contract (schema `kndo-v2/m6`) — the key
    /// predates the conduct renaming and stays; the TYPE carries the new name.
    pub plugins: Vec<Contribution>,
    pub diagnostics: Vec<ReportDiagnostic>,
}

impl Report {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report serializes")
    }
}

/// The JSON schema of the envelope, derived from the types — the committed
/// `schemas/report.schema.json` is generated from this, never written by hand.
#[cfg(feature = "schema")]
pub fn report_schema() -> String {
    let schema = schemars::schema_for!(Report);
    let mut json = serde_json::to_string_pretty(&schema).expect("schema serializes");
    json.push('\n');
    json
}
