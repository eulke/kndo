//! `.kndo/baseline.json` — acknowledged findings. Deliberately not part
//! of `cache.rs`: everything under `.kndo/cache/` is disposable and gitignored, but this one
//! file is *committed* — a human reviews its diff in a PR, which is the whole adoption-path
//! point (acknowledge a legacy repo's existing debt on day one, then ratchet it down over time,
//! never regrow it silently). Plain, pretty-printed JSON, not the cache's binary formats: a
//! reviewer has to be able to read the diff.

use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::Finding;

fn path(root: &Path) -> PathBuf {
    root.join(".kndo").join("baseline.json")
}

pub(crate) fn exists(root: &Path) -> bool {
    path(root).is_file()
}

/// One acknowledged finding. Matching against current findings is by `id` alone (stable,
/// content-anchored); the rest of the fields exist only so a
/// human reading `git diff .kndo/baseline.json` can tell *what* changed without cross-
/// referencing ids against a report.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BaselineEntry {
    pub(crate) id: String,
    pub(crate) category: String,
    pub(crate) subject_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) symbol: Option<String>,
}

impl From<&Finding> for BaselineEntry {
    fn from(f: &Finding) -> Self {
        BaselineEntry {
            id: f.id.clone(),
            category: f.category.clone(),
            subject_kind: f.subject_kind.clone(),
            path: f.location.path.as_ref().map(|p| p.0.to_string()),
            symbol: f.location.symbol.clone(),
        }
    }
}

/// Bumped only if this on-disk shape itself changes; unrelated to any cache format version —
/// this file is committed and hand-editable, so a version mismatch is a diagnostic-worthy
/// condition for a future migration path, never a silent-rebuild-and-move-on cache miss.
const BASELINE_FILE_VERSION: u32 = 1;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct BaselineFile {
    schema_version: u32,
    entries: Vec<BaselineEntry>,
}

/// `None` when no baseline exists yet, or the file is unreadable/malformed — both are "there is
/// no baseline to apply," never a hard error: a corrupt `baseline.json` degrading to "nothing is
/// acknowledged" (every finding reported) is the fail-safe direction, never the reverse.
pub(crate) fn load(root: &Path) -> Option<Vec<BaselineEntry>> {
    let text = fs::read_to_string(path(root)).ok()?;
    let file: BaselineFile = serde_json::from_str(&text).ok()?;
    Some(file.entries)
}

pub(crate) fn save(root: &Path, entries: &[BaselineEntry]) -> std::io::Result<()> {
    let file = BaselineFile {
        schema_version: BASELINE_FILE_VERSION,
        entries: entries.to_vec(),
    };
    let json = serde_json::to_string_pretty(&file)
        .expect("BaselineFile serialization is infallible (no maps, no non-finite floats)");
    if let Some(dir) = path(root).parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path(root), json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kndo-baseline-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(id: &str) -> BaselineEntry {
        BaselineEntry {
            id: id.to_string(),
            category: "unused".to_string(),
            subject_kind: "function".to_string(),
            path: Some("src/a.ts".to_string()),
            symbol: Some("dead".to_string()),
        }
    }

    #[test]
    fn missing_file_is_none_not_an_error() {
        let dir = tmp("missing");
        assert!(load(&dir).is_none());
        assert!(!exists(&dir));
    }

    #[test]
    fn round_trips_entries() {
        let dir = tmp("roundtrip");
        save(&dir, &[entry("kndo-a"), entry("kndo-b")]).unwrap();
        assert!(exists(&dir));
        let loaded = load(&dir).unwrap();
        assert_eq!(loaded, vec![entry("kndo-a"), entry("kndo-b")]);
    }

    #[test]
    fn empty_baseline_round_trips_as_some_empty_not_none() {
        let dir = tmp("empty");
        save(&dir, &[]).unwrap();
        assert_eq!(load(&dir), Some(vec![]));
    }

    #[test]
    fn corrupt_file_degrades_to_none_not_a_panic() {
        let dir = tmp("corrupt");
        fs::create_dir_all(dir.join(".kndo")).unwrap();
        fs::write(path(&dir), b"not json at all").unwrap();
        assert!(load(&dir).is_none());
    }

    #[test]
    fn file_is_human_readable_pretty_json() {
        let dir = tmp("pretty");
        save(&dir, &[entry("kndo-a")]).unwrap();
        let text = fs::read_to_string(path(&dir)).unwrap();
        assert!(text.contains('\n')); // pretty-printed, not a single minified line
        assert!(text.contains("\"kndo-a\""));
    }
}
