//! **`internal/` has exactly one point of contact with the rest of the codebase: `CLAUDE.md`.**
//!
//! `internal/` is a maintainers-only design-doc tree; every public contract is legible from the
//! code and its doc comments alone. Nothing outside `internal/` may cite it — no `internal/...`
//! path, no `RFC 00NN`/`ADR 00NN` number from the retired pre-consolidation numbering, no `§`
//! section sign — except the one line in `CLAUDE.md` that tells an agent the tree exists. A
//! citation anywhere else means code (or the public manual) has started depending on a document
//! only maintainers can see, which is the drift this gate exists to catch before it spreads.
//!
//! Deliberately `RFC 00\d\d`/`ADR 00\d\d`, not a bare `RFC \d+`: this project's own retired
//! numbering never went past `RFC 0018`/`ADR 0007`, so the zero-padded form is unambiguous,
//! while a real external standard (`RFC 3339`, `RFC 5322`) is never a false failure here.
//!
//! A handful of files legitimately contain the substring `internal/` for a reason that has
//! nothing to do with this project's doc tree — Go's own compiler-enforced `internal/` package
//! boundary, Node's `#internal/*` subpath-imports convention, Kotlin's `internal` visibility
//! keyword — and are listed in [`INTERNAL_SUBSTRING_ALLOWLIST`]. A new false positive there is
//! fixed by adding to that list, never by weakening the scan.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo has two ancestors up to the workspace root")
        .to_path_buf()
}

/// Files where `internal/` is a real language convention, not a citation of this project's own
/// design-doc tree. See the module doc for why each language is on this list.
const INTERNAL_SUBSTRING_ALLOWLIST: &[&str] = &[
    "crates/kndo-adapter-go/src/extraction.rs",
    "crates/kndo-adapter-go/src/lib.rs",
    "crates/kndo-adapter-go/src/manifest.rs",
    "crates/kndo-adapter-go/src/stdlib.txt",
    "crates/kndo-adapter-go/tests/fixtures/qualified-package-name/project/main.go",
    "crates/kndo-adapter-go/tests/fixtures/internal-only-unit/project/main.go",
    "crates/kndo-adapter-go/tests/fixtures/internal-only-unit/project/internal/util/helper.go",
    "crates/kndo-adapter-go/tests/fixtures/internal-only-unit/expected.json",
    "crates/kndo-adapter-go/tests/fixtures/internal-package/project/internal/util/helper.go",
    "crates/kndo-adapter-go/tests/fixtures/internal-package/expected.json",
    "crates/kndo-adapter-go/tests/fixtures/qualified-package-name/project/internal/v2/yaml.go",
    "crates/kndo/tests/fixtures/go-module-with-nested-example/project/main.go",
    "crates/kndo/tests/fixtures/go-module-with-nested-example/expected.json",
    "crates/kndo-adapter-js/src/resolution.rs",
    "crates/kndo-adapter-kotlin/src/parsing.rs",
    "crates/kndo-core/src/adapter.rs",
    "crates/kndo-core/src/analysis/deep_import.rs",
    "xtask/src/main.rs",
    "xtask/tests/release_channels.rs",
    "docs/src/languages.md",
    "docs/src/rules.md",
];

/// Every file under the repository, skipping build output, vendored trees, and the top-level
/// `internal/` design-doc tree itself — unconstrained by design, since this gate is about
/// everything ELSE never citing it. Checked by relative path from the root, not bare basename,
/// so a directory named `internal` nested inside a Go test fixture (real Go source, not the doc
/// tree) is never accidentally skipped along with it.
fn all_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let rel_path = rel.join(&name);
            if path.is_dir() {
                let name = name.to_string_lossy();
                if matches!(name.as_ref(), "target" | ".git" | "node_modules") {
                    continue;
                }
                if rel_path == Path::new("internal") {
                    continue;
                }
                walk(&path, &rel_path, out);
            } else {
                out.push(rel_path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, Path::new(""), &mut out);
    out.sort();
    out
}

/// Whether `line` contains `<prefix> ` followed by exactly two more ASCII digits — the
/// zero-padded citation shape, e.g. `has_zero_padded_citation(line, "RFC 00")` matches
/// `RFC 0007` but not `RFC 3339`.
fn has_zero_padded_citation(line: &str, prefix: &str) -> bool {
    for (idx, _) in line.match_indices(prefix) {
        let rest = &line[idx + prefix.len()..];
        let mut digits = rest.chars();
        if digits.next().is_some_and(|c| c.is_ascii_digit())
            && digits.next().is_some_and(|c| c.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

fn cites_retired_numbering_or_section(line: &str) -> bool {
    has_zero_padded_citation(line, "RFC 00")
        || has_zero_padded_citation(line, "ADR 00")
        || line.contains('§')
}

/// This file's own path, relative to the repository root. Its source necessarily contains every
/// pattern it scans for — that's how it recognizes them — so it is the one file besides
/// `CLAUDE.md` exempt from its own check, the same way a linter's rule-definition source is
/// exempt from the rule it implements.
const SELF: &str = "crates/kndo/tests/internal_boundary.rs";

#[test]
fn internal_has_exactly_one_point_of_contact() {
    let root = repo_root();
    let files = all_files(&root);

    // Not an assertion about a number — about the walk having found anything at all. A walk
    // that silently matched nothing would make every assertion below vacuously true, which is
    // the one way a gate like this passes while proving nothing.
    assert!(
        files.len() > 200,
        "only {} files found under the repository root; the walk went stale",
        files.len()
    );

    let mut internal_hits = Vec::new();
    let mut citation_hits = Vec::new();
    let mut claude_md_internal_count = 0usize;

    for rel in &files {
        let display = rel.display().to_string();
        if display == SELF {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue; // not UTF-8: nothing to read, nothing to cite
        };

        let is_claude_md = display == "CLAUDE.md";
        let internal_allowed = INTERNAL_SUBSTRING_ALLOWLIST.contains(&display.as_str());

        for (i, line) in text.lines().enumerate() {
            if line.contains("internal/") {
                if is_claude_md {
                    claude_md_internal_count += 1;
                } else if !internal_allowed {
                    internal_hits.push(format!("{display}:{}: {}", i + 1, line.trim()));
                }
            }
            if cites_retired_numbering_or_section(line) {
                citation_hits.push(format!("{display}:{}: {}", i + 1, line.trim()));
            }
        }
    }

    assert!(
        citation_hits.is_empty(),
        "code/docs cite a retired RFC/ADR number or a `§` section sign outside internal/ \
         ({} hit(s)) — state the rule directly instead of pointing at a document only \
         maintainers can see:\n  {}",
        citation_hits.len(),
        citation_hits.join("\n  ")
    );

    assert!(
        internal_hits.is_empty(),
        "`internal/` is cited outside internal/, CLAUDE.md, and the allowlist ({} hit(s)) — \
         if this is a real language convention (Go's own internal/, Node's #internal/*, \
         Kotlin's internal keyword) add the file to INTERNAL_SUBSTRING_ALLOWLIST in this test; \
         otherwise state the fact directly instead of citing the doc tree:\n  {}",
        internal_hits.len(),
        internal_hits.join("\n  ")
    );

    assert_eq!(
        claude_md_internal_count, 1,
        "CLAUDE.md must mention `internal/` exactly once — the single point of contact that \
         lets an agent know the tree exists and nothing more. Found {claude_md_internal_count}."
    );
}
