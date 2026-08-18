//! The `Engine` facade — the only surface frontends may touch (contracts §5).
//!
//! Separation rules, enforced by dependency direction: the core contains no terminal concerns
//! (no ANSI, no TTY detection, no exit codes, no stdout) — it returns data and never prints;
//! frontends contain no analysis concerns — they cannot reach the graph, cache, or adapters
//! except through this type. A frontend that needs a new fact is a core PR adding it to
//! [`RunResult`], never a core import.

use std::fmt;
use std::path::{Path, PathBuf};

/// Mirrors the output schema's `schema_version` (contracts/output-schema.md).
pub const SCHEMA_VERSION: &str = "1.0.0";

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
                write!(f, "project root does not exist or is not a directory: {}", p.display())
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

#[derive(Debug)]
pub struct CheckRequest {
    pub mode: RunMode,
}

/// Typed form of the output-schema finding (grows field-by-field with the analyses in M1;
/// every field lands in the JSON schema first — that document is normative).
#[derive(Debug, Clone)]
pub struct Finding {
    pub id: String,
    pub category: String,
    pub group: String,
    pub subject_kind: String,
    pub message: String,
}

/// Typed form of the output-schema envelope. JSON/SARIF/agent serializers live core-side so
/// every frontend emits byte-identical machine output; *human* rendering is frontend-owned
/// (RFC 0009).
#[derive(Debug, Default)]
pub struct RunResult {
    pub findings: Vec<Finding>,
    pub diagnostics: Vec<String>,
}

/// Synchronous and single-instance-per-project (the cache lock, RFC 0004 §7); a serving
/// frontend wraps it in its own concurrency model.
pub struct Engine {
    root: PathBuf,
}

impl Engine {
    pub fn open(root: &Path, _overrides: ConfigOverrides) -> Result<Engine, EngineError> {
        if !root.is_dir() {
            return Err(EngineError::ProjectRootNotFound(root.to_path_buf()));
        }
        Ok(Engine { root: root.to_path_buf() })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// M1 skeleton: discovery/extraction/graph/analyses land behind this signature. The
    /// signature is the contract; the emptiness is temporary.
    pub fn check(&mut self, _req: CheckRequest) -> RunResult {
        RunResult::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_rejects_missing_root() {
        let err = Engine::open(Path::new("/definitely/not/a/dir"), ConfigOverrides::default());
        assert!(err.is_err());
    }

    #[test]
    fn check_on_empty_project_is_clean() {
        let dir = std::env::temp_dir().join("kndo-engine-test");
        std::fs::create_dir_all(&dir).unwrap();
        let mut engine = Engine::open(&dir, ConfigOverrides::default()).unwrap();
        let result = engine.check(CheckRequest { mode: RunMode::Full });
        assert!(result.findings.is_empty());
    }
}
