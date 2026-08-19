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

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::adapter::{Diagnostic, DiagnosticLevel, LanguageAdapter, ProjectPath, Span};
use crate::analysis;
use crate::graph;
use crate::vocab::Confidence;

/// Mirrors the output schema's `schema_version` (contracts/output-schema.md).
pub const SCHEMA_VERSION: &str = "1.0.0";

/// The product version — every crate shares `version.workspace = true`, so kndo-core's own
/// `CARGO_PKG_VERSION` is the same string the distribution crate and CLI would report.
pub const KNDO_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default)]
pub struct ConfigOverrides {}

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

/// Typed form of the output-schema finding (grows field-by-field with the analyses in M1;
/// every field lands in the JSON schema first — that document is normative). Not yet present:
/// `related` (evidence chain), `evidence` (category-specific block), `sources`, `remediation`,
/// `rolled_up`, `delta`/`delta_origin` — each needs infrastructure this milestone doesn't have
/// (an evidence model, computed remediation text, diff mode) and is omitted rather than
/// fabricated with a placeholder.
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
}

/// One registered adapter's contribution (`run.adapters[]`, output-schema §1).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AdapterRunInfo {
    pub id: String,
    pub files: usize,
}

/// Typed form of the output-schema envelope. JSON/SARIF/agent serializers live core-side so
/// every frontend emits byte-identical machine output; *human* rendering is frontend-owned
/// (RFC 0009). Flat here for ergonomic Rust consumption; [`RunResult::to_json`] nests it into
/// the schema's actual shape. Not yet present: `health`, `budget`, `baseline`, `suppressed` —
/// none of those subsystems exist yet (health/CRAP scoring is M4; baseline, suppressions, and
/// diff-mode budgets are M2), so the fields are omitted rather than emitted empty/null. Adding
/// them later is additive (minor schema bump, RFC 0006 §4), not a breaking change.
#[derive(Debug, Default)]
pub struct RunResult {
    pub findings: Vec<Finding>,
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
                cache: "cold", // no cache exists yet (RFC 0004 lands M2) — every run is cold
                project_root: self.project_root.clone(),
                adapters: self.adapters.clone(),
            },
            findings: self.findings.clone(),
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
}

impl Engine {
    /// `adapters` is the registered language set, composed by the distribution layer (the
    /// `kndo` crate) — compiled-in first-party adapters today, WASM-bridged third-party
    /// adapters later (ADR 0003). The core never selects or knows about them beyond the
    /// trait; embedders and tests may pass a custom set directly.
    pub fn open(
        root: &Path,
        _overrides: ConfigOverrides,
        adapters: Vec<Box<dyn LanguageAdapter>>,
    ) -> Result<Engine, EngineError> {
        if !root.is_dir() {
            return Err(EngineError::ProjectRootNotFound(root.to_path_buf()));
        }
        Ok(Engine {
            root: root.to_path_buf(),
            adapters,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `--staged`/`--diff` scoping (`RunMode`) isn't implemented yet — every mode walks the
    /// full tree until git-index/merge-base scoping lands (M2); the requested mode is still
    /// echoed into the result honestly (`run.mode`), it just doesn't change behavior yet.
    pub fn check(&mut self, req: CheckRequest) -> RunResult {
        let start = Instant::now();
        let started_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mode = req.mode.as_str().to_string();
        let base_ref = req.mode.base_ref();
        let project_root = self.root.display().to_string();

        let outcome = match graph::assemble(&self.root, &self.adapters) {
            Ok((g, diagnostics)) => {
                let findings = analysis::run_all(&g);
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
            Err(crate::discovery::DiscoveryError::Root(e)) => RunResult {
                diagnostics: vec![Diagnostic {
                    level: DiagnosticLevel::Warn,
                    path: None,
                    message: format!(
                        "cannot walk the project root: {e} — check the path and permissions"
                    ),
                    span: None,
                }],
                ..RunResult::default()
            },
        };

        RunResult {
            mode,
            base_ref,
            started_at,
            duration_ms: start.elapsed().as_millis() as u64,
            project_root,
            ..outcome
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        };
        let json = serde_json::to_string(&finding).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["location"], serde_json::json!({}));
        assert_eq!(value["severity"], "warning");
        assert_eq!(value["confidence"], "certain");
    }
}
