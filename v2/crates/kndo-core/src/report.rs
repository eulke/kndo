//! The one output type. No wall-clock fields by design — reports are compared
//! byte-for-byte by the equivalence gates, and run metadata that varies per run
//! (timings, timestamps) joins at the frontend edge when a milestone needs it.

use crate::analysis::Abstention;
use crate::suppress::SuppressedSummary;
use kndo_contract::evidence::DiagnosticLevel;
use kndo_contract::finding::Finding;
use kndo_contract::vocab::ProjectPath;
use serde::Serialize;
use smol_str::SmolStr;

pub const SCHEMA: &str = "kndo-v2/m3";

#[derive(Serialize)]
pub struct AdapterRun {
    pub id: SmolStr,
    pub files: u32,
}

#[derive(Serialize)]
pub struct RunInfo {
    pub schema: &'static str,
    pub files_discovered: u32,
    pub files_claimed: u32,
    pub adapters: Vec<AdapterRun>,
}

#[derive(Serialize)]
pub struct ReportDiagnostic {
    pub path: ProjectPath,
    pub level: DiagnosticLevel,
    pub message: String,
}

#[derive(Serialize)]
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
    pub diagnostics: Vec<ReportDiagnostic>,
}

impl Report {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report serializes")
    }
}
