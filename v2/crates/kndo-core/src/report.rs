//! The one output type. No wall-clock fields by design — reports are compared
//! byte-for-byte by the equivalence gates, and run metadata that varies per run
//! (timings, timestamps) joins at the frontend edge when a milestone needs it.

use crate::analysis::Abstention;
use crate::plugin::PluginContribution;
use crate::suppress::SuppressedSummary;
use kndo_contract::evidence::DiagnosticLevel;
use kndo_contract::finding::Finding;
use kndo_contract::vocab::ProjectPath;
use serde::Serialize;
use smol_str::SmolStr;

pub const SCHEMA: &str = "kndo-v2/m5";

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AdapterRun {
    pub id: SmolStr,
    pub files: u32,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RunInfo {
    /// Stamped as a CONST in the generated schema, from the same `SCHEMA` the report
    /// writes — a report from any other envelope version fails validation instead of
    /// drifting through.
    #[cfg_attr(feature = "schema", schemars(schema_with = "schema_version_const"))]
    pub schema: &'static str,
    pub files_discovered: u32,
    pub files_claimed: u32,
    pub adapters: Vec<AdapterRun>,
}

#[cfg(feature = "schema")]
fn schema_version_const(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({ "const": SCHEMA })
}

#[derive(Debug, Serialize)]
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
    pub plugins: Vec<PluginContribution>,
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
