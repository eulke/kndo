//! Shared conformance harness: every adapter's
//! fixture suite runs through this one runner, so "the adapter passes conformance" means the
//! same thing everywhere — first-party today, third-party later. A fixture is a directory:
//! `project/` (a miniature real codebase, extracted and resolved by real adapters — not
//! `MockAdapter`) plus `expected.json` (the findings it must produce).
//!
//! `expected.json` deliberately projects each finding onto four fields — `category`,
//! `subject_kind`, `path`, `symbol` — and nothing else. `id` is excluded (it's a hash, not
//! something a fixture author should transcribe by hand), `range`/`message`/`confidence` are
//! excluded because pinning them would make every fixture brittle to wording and span changes
//! that don't represent a real regression. What's asserted is exactly what a fixture exists to
//! prove: *this* category of finding fired on *this* subject, and nothing else did.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapter::LanguageAdapter;
use crate::engine::{ConfigOverrides, Engine, RunMode};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct FixtureFinding {
    pub category: String,
    pub subject_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ExpectedFile {
    findings: Vec<FixtureFinding>,
}

/// A fixture's actual findings didn't match `expected.json`.
#[derive(Debug)]
pub struct ConformanceMismatch {
    pub missing: Vec<FixtureFinding>,
    pub unexpected: Vec<FixtureFinding>,
}

impl fmt::Display for ConformanceMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.missing.is_empty() {
            writeln!(f, "expected but not produced:")?;
            for finding in &self.missing {
                writeln!(f, "  - {finding:?}")?;
            }
        }
        if !self.unexpected.is_empty() {
            writeln!(f, "produced but not expected:")?;
            for finding in &self.unexpected {
                writeln!(f, "  - {finding:?}")?;
            }
        }
        Ok(())
    }
}

/// One fixture's setup/read failure — distinct from [`ConformanceMismatch`] (a fixture that
/// ran and disagreed) because these mean the fixture itself is malformed, not that the adapter
/// is wrong.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ConformanceError(pub String);

/// [`run_fixture`]/[`run_fixture_with`]'s success outcome — a fixture that ran and either
/// matched `expected.json` or didn't. Was a nested `Result<Result<(), ConformanceMismatch>,
/// ConformanceError>`; the two failure modes (a malformed fixture vs. a fixture that ran and
/// disagreed) are still distinct, just as one flat enum instead of Result-in-Result.
#[derive(Debug)]
pub enum ConformanceVerdict {
    Pass,
    Mismatch(ConformanceMismatch),
}

/// Every subdirectory of `root` that looks like a fixture (has `project/` and `expected.json`),
/// sorted for deterministic test output.
pub fn discover_fixtures(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("expected.json").is_file() && path.join("project").is_dir() {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// The discover → run-each → collect → format shape every adapter's own conformance test
/// repeats verbatim — the only thing that varies between adapters is which adapters/plugins
/// `run` constructs per fixture. `run` typically closes over one `run_fixture(fixture, vec![Box::new(MyAdapter)])`
/// call (or `run_fixture_with` when the fixture needs a plugin too). Returns `Err(message)`
/// ready to hand to `panic!` at the call site, rather than panicking here, so a failure's
/// backtrace still points at the adapter's own `#[test]` fn.
pub fn run_fixture_dir(
    root: &Path,
    mut run: impl FnMut(&Path) -> Result<ConformanceVerdict, ConformanceError>,
) -> Result<(), String> {
    let fixtures = discover_fixtures(root);
    if fixtures.is_empty() {
        return Err(format!(
            "no conformance fixtures found under {}",
            root.display()
        ));
    }

    let mut failures = Vec::new();
    for fixture in &fixtures {
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        match run(fixture) {
            Ok(ConformanceVerdict::Pass) => {}
            Ok(ConformanceVerdict::Mismatch(mismatch)) => {
                failures.push(format!("{name}:\n{mismatch}"))
            }
            Err(err) => failures.push(format!("{name}: {err}")),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "conformance fixture failures:\n\n{}",
            failures.join("\n")
        ))
    }
}

/// Runs one fixture end to end — real discovery, real adapters, the real `Engine` — and diffs
/// the projected actual findings against `expected.json`.
pub fn run_fixture(
    fixture_dir: &Path,
    adapters: Vec<Box<dyn LanguageAdapter>>,
) -> Result<ConformanceVerdict, ConformanceError> {
    run_fixture_with(fixture_dir, adapters, vec![])
}

/// [`run_fixture`] plus a plugin set — for fixtures whose expectations need one (the crap
/// fixtures ship an lcov report, and the coverage ingesters are composition, not core:
/// `Engine::open` registers no plugins, so the adapter's test crate passes the ingester in,
/// exactly the way the product's own composition layer does).
// The shape this shares with `query_envelope::finding_locations` is
// `findings.iter().map(|f| …).collect()` — an idiom, not a decision. This projects findings
// into the golden comparison shape (owned, ordered, category + subject); that one builds a
// borrowed query index (id + path + symbol). Nothing is decided in both places.
// kndo:allow duplicate the shared shape is an iterator idiom, not a shared decision
pub fn run_fixture_with(
    fixture_dir: &Path,
    adapters: Vec<Box<dyn LanguageAdapter>>,
    plugins: Vec<Box<dyn crate::plugin::Plugin>>,
) -> Result<ConformanceVerdict, ConformanceError> {
    let expected_path = fixture_dir.join("expected.json");
    let expected_text = fs::read_to_string(&expected_path)
        .map_err(|e| ConformanceError(format!("reading {}: {e}", expected_path.display())))?;
    let expected: ExpectedFile = serde_json::from_str(&expected_text)
        .map_err(|e| ConformanceError(format!("parsing {}: {e}", expected_path.display())))?;

    // Fixtures are checked-in golden inputs, not throwaway project dirs — caching would leave
    // a `.kndo/` behind inside version-controlled fixture directories on every test run, for
    // zero benefit (fixtures are tiny and each runs once).
    let project_dir = fixture_dir.join("project");
    let overrides = ConfigOverrides {
        use_cache: false,
        ..ConfigOverrides::default()
    };
    let mut engine = Engine::open_with_plugins(&project_dir, overrides, adapters, plugins)
        .map_err(|e| ConformanceError(format!("opening {}: {e}", project_dir.display())))?;
    let result = engine.check(RunMode::Full);

    let actual: BTreeSet<FixtureFinding> = result
        .findings
        .iter()
        .map(|f| FixtureFinding {
            category: f.category.to_string(),
            subject_kind: f.subject_kind.to_string(),
            path: f.location.path.as_ref().map(|p| p.0.to_string()),
            symbol: f.location.symbol.clone(),
        })
        .collect();
    let expected_set: BTreeSet<FixtureFinding> = expected.findings.into_iter().collect();

    let missing: Vec<_> = expected_set.difference(&actual).cloned().collect();
    let unexpected: Vec<_> = actual.difference(&expected_set).cloned().collect();
    if missing.is_empty() && unexpected.is_empty() {
        Ok(ConformanceVerdict::Pass)
    } else {
        Ok(ConformanceVerdict::Mismatch(ConformanceMismatch {
            missing,
            unexpected,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_fixtures_finds_only_well_formed_directories() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("good/project")).unwrap();
        fs::write(
            root.path().join("good/expected.json"),
            r#"{"findings": []}"#,
        )
        .unwrap();
        fs::create_dir_all(root.path().join("missing-expected/project")).unwrap();
        fs::create_dir_all(root.path().join("missing-project")).unwrap();
        fs::write(
            root.path().join("missing-project/expected.json"),
            r#"{"findings": []}"#,
        )
        .unwrap();
        fs::write(root.path().join("not-a-fixture.txt"), "").unwrap();

        let found = discover_fixtures(root.path());
        assert_eq!(found, vec![root.path().join("good")]);
    }

    #[test]
    fn run_fixture_reports_clean_on_a_perfect_match() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("project")).unwrap();
        fs::write(dir.path().join("expected.json"), r#"{"findings": []}"#).unwrap();

        let outcome = run_fixture(dir.path(), vec![]).unwrap();
        assert!(matches!(outcome, ConformanceVerdict::Pass));
    }

    #[test]
    fn run_fixture_reports_a_mismatch_when_expected_findings_never_fire() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("project")).unwrap();
        fs::write(
            dir.path().join("expected.json"),
            r#"{"findings": [{"category": "unused", "subject_kind": "file", "path": "ghost.ts"}]}"#,
        )
        .unwrap();

        let outcome = run_fixture(dir.path(), vec![]).unwrap();
        let ConformanceVerdict::Mismatch(mismatch) = outcome else {
            panic!("expected a mismatch, got {outcome:?}");
        };
        assert_eq!(mismatch.missing.len(), 1);
        assert!(mismatch.unexpected.is_empty());
    }

    #[test]
    fn run_fixture_errors_on_a_malformed_expected_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("project")).unwrap();
        fs::write(dir.path().join("expected.json"), "not json").unwrap();

        assert!(run_fixture(dir.path(), vec![]).is_err());
    }
}
