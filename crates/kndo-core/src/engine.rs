//! The `Engine` facade — the only surface frontends may touch (contracts §5).
//!
//! Separation rules, enforced by dependency direction: the core contains no terminal concerns
//! (no ANSI, no TTY detection, no exit codes, no stdout) — it returns data and never prints;
//! frontends contain no analysis concerns — they cannot reach the graph, cache, or adapters
//! except through this type. A frontend that needs a new fact is a core PR adding it to
//! [`RunResult`], never a core import.
//!
//! Adapter *registration* is a frontend concern too: the core never knows which languages
//! exist (RFC 0001 §2, the ignorance rule) — `kndo-cli` composes `Engine::open` with the
//! first-party adapters it links in.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::adapter::{Diagnostic, DiagnosticLevel, LanguageAdapter};
use crate::graph;

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
    /// Typed diagnostics (the schema's `diagnostics` array) — one representation everywhere,
    /// never parallel stringly-typed variants.
    pub diagnostics: Vec<Diagnostic>,
    pub files_discovered: usize,
    /// Files a registered adapter recognized (subset of `files_discovered`). Stands in for
    /// `run.adapters[].files` (output-schema §1) until per-adapter breakdown lands.
    pub files_claimed: usize,
    pub symbols: usize,
    pub dependencies: usize,
    pub edges: usize,
}

/// Synchronous and single-instance-per-project (the cache lock, RFC 0004 §7); a serving
/// frontend wraps it in its own concurrency model.
pub struct Engine {
    root: PathBuf,
    adapters: Vec<Box<dyn LanguageAdapter>>,
}

impl Engine {
    /// `adapters` is the frontend's registered language set — compiled-in first-party
    /// adapters today, WASM-bridged third-party adapters later (ADR 0003). The core never
    /// selects or knows about them beyond the trait.
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

    /// M1 skeleton: analyses land behind this signature next. Graph assembly (discovery →
    /// claim → extract → resolve → link) is wired; `--staged`/`--diff` scoping (`RunMode`)
    /// is not yet — every mode walks the full tree until git-index/merge-base scoping lands.
    pub fn check(&mut self, _req: CheckRequest) -> RunResult {
        match graph::assemble(&self.root, &self.adapters) {
            Ok((g, diagnostics)) => RunResult {
                files_discovered: g.files.len(),
                files_claimed: g.files.iter().filter(|f| f.language.is_some()).count(),
                symbols: g.symbols.len(),
                dependencies: g.dependencies.len(),
                edges: g.edges.len(),
                diagnostics,
                findings: Vec::new(),
            },
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
}
