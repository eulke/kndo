//! The guard behind `CLAUDE.md`'s "never a hand-rolled name under `std::env::temp_dir()`".
//!
//! It was a written rule with nothing enforcing it, and the workspace had drifted to 69
//! violations across 27 files — including `graph/tests.rs`, where the migration to `tempfile`
//! had been done for the project directory, documented with the race it fixed, and then
//! abandoned halfway: the *cache* directory three lines below stayed hand-built.
//!
//! Two properties make the hand-rolled form worse than it looks. The names were fixed string
//! literals, so uniqueness was a hand-maintained invariant with nothing checking it; and the
//! cleanup was `remove_dir_all` **on entry**, so a collision did not merely share a directory —
//! the second test to start deleted the first one's fixture while it was still being read.
//! `tempfile::TempDir` is unique by construction and deleted on drop, unwind included.

use std::path::Path;

#[test]
fn no_source_file_builds_its_own_directory_under_the_system_temp_dir() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf();

    // Split so this file is not its own first offender — the alternative, skipping by
    // filename, is the kind of exemption that quietly grows.
    let needle = concat!("env", "::temp_dir");
    let mut offenders = Vec::new();
    let mut scanned = 0usize;
    walk(&root, &mut |path| {
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            return;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        scanned += 1;
        for (i, line) in text.lines().enumerate() {
            if line.contains(needle) && !line.trim_start().starts_with("//") {
                let rel = path.strip_prefix(&root).unwrap_or(path);
                offenders.push(format!("{}:{}: {}", rel.display(), i + 1, line.trim()));
            }
        }
    });

    assert!(
        scanned > 100,
        "the walk found only {scanned} Rust files — it is not reaching the sources, so a green \
         result here would prove nothing"
    );
    assert!(
        offenders.is_empty(),
        "hand-rolled temp directories (use `tempfile::tempdir()`, or \
         `kndo_core::testkit::fixture::project` for a project tree):\n{}",
        offenders.join("\n")
    );
}

/// Skips `target/` and `.git/`: build artifacts vendor other people's sources, and this rule is
/// about ours.
fn walk(dir: &Path, visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name == ".git" || name == "node_modules" {
                continue;
            }
            walk(&path, visit);
        } else {
            visit(&path);
        }
    }
}
