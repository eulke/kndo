//! **Every relative link in the repository's Markdown resolves.**
//!
//! This gate is scoped to exactly what the evidence on this repository supports, not to the
//! broader approach of also treating a path-shaped token in prose as a `Possible` reference.
//! Measured here, that broader approach fails:
//!
//! * A path that approach would flag as moved appears in only two documents on this
//!   repository, and both are the ones explaining the gap itself — not stale pointers a
//!   prose-path check would need to catch.
//! * Backticked prose paths: **195 checked, 63 resolve to nothing, and essentially none is a
//!   defect** — they are examples from other repositories (`crates/searcher/src/sink.rs` is
//!   ripgrep's), invented illustrations (`com/foo/bar/Widget.java`), paths that exist in a
//!   *user's* project (`coverage/lcov.info`), and module-relative shorthand. An analysis
//!   firing on those would be 63 findings and no fix.
//! * Claiming `**/*.md` at all would make every document eligible for `unused`, and 15 of
//!   this repository's 74 are linked from nothing — `CLAUDE.md`, `CONTRIBUTING.md`, every
//!   adapter spec, several RFCs. All legitimate; all would be false, and `dogfood` would go
//!   red. Exempting them needs a `FileRole::Docs` that does not exist, which is a vocabulary
//!   change to make a component viable that has no measured case.
//!
//! What *is* real is the narrow half: a Markdown **link** is an author asserting that a path
//! resolves — the closest thing prose has to an import — and 189 of them exist here. Moving or
//! renaming a document silently breaks any link still pointing at the old path unless
//! something checks it — which is what this test is for.
//!
//! Prose is deliberately not checked. That is the measurement above, not an omission.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo has two ancestors up to the workspace root")
        .to_path_buf()
}

/// Every `.md` under the repository, skipping build output and vendored trees.
fn markdown_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !matches!(name.as_ref(), "target" | ".git" | "node_modules") {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|e| e == "md") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

/// `text` with every fenced block and inline code span blanked out, so what remains is prose
/// and real links.
///
/// Not cosmetic: a code span documenting a generic function signature like
/// `` `func F[T any](x T)` `` contains `](x T)`, and a scan that does not blank code spans
/// first reads that as a link to `x`. Code is quoted, not asserted — a path inside backticks
/// is an illustration, and the whole point of this gate is to fire only on what an author
/// actually claims resolves.
///
/// Replaces with spaces rather than removing, so byte offsets stay put and a span cannot be
/// joined to the text after it.
fn without_code(text: &str) -> String {
    let mut out: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < out.len() {
        if out[i] != '`' {
            i += 1;
            continue;
        }
        // A run of N backticks opens a span that only a run of exactly N backticks closes —
        // the rule for both inline spans and fenced blocks, which is why one pass handles
        // ```rust fences and ``literal `backtick` `` spans alike.
        let open = i;
        while i < out.len() && out[i] == '`' {
            i += 1;
        }
        let fence = i - open;
        let close = loop {
            match out[i..].iter().position(|&c| c == '`') {
                None => break None,
                Some(rel) => {
                    let at = i + rel;
                    let mut end = at;
                    while end < out.len() && out[end] == '`' {
                        end += 1;
                    }
                    if end - at == fence {
                        break Some((at, end));
                    }
                    i = end;
                }
            }
        };
        match close {
            // Blank the delimiters and everything between them.
            Some((_, end)) => {
                for c in &mut out[open..end] {
                    *c = ' ';
                }
                i = end;
            }
            // Unterminated: the rest of the document is inside it as far as any reader is
            // concerned, so nothing after this point is a claim.
            None => {
                for c in &mut out[open..] {
                    *c = ' ';
                }
                break;
            }
        }
    }
    out.into_iter().collect()
}

/// Every `](target)` in `text`, minus the ones no filesystem check applies to: absolute URLs,
/// `mailto:`, and same-document anchors. A `#fragment` is trimmed — this checks that the file
/// exists, not that a heading does; anchors drift for reasons a path check cannot judge.
fn relative_link_targets(text: &str) -> Vec<String> {
    let text = without_code(text);
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(open) = text[i..].find("](") {
        let start = i + open + 2;
        let Some(close) = text[start..].find(')') else {
            break;
        };
        let target = &text[start..start + close];
        i = start + close + 1;
        // A link whose target carries a title (`](path "Title")`) keeps the path half.
        let target = target.split_whitespace().next().unwrap_or_default();
        let target = target.split('#').next().unwrap_or_default();
        if target.is_empty()
            || target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("mailto:")
        {
            continue;
        }
        out.push(target.to_string());
    }
    out
}

#[test]
fn every_relative_markdown_link_resolves() {
    let root = repo_root();
    let mut dead: BTreeSet<String> = BTreeSet::new();
    let mut checked = 0usize;

    for file in markdown_files(&root) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue; // not UTF-8: nothing to read, nothing to claim
        };
        for target in relative_link_targets(&text) {
            checked += 1;
            if !file
                .parent()
                .expect("a file has a parent")
                .join(&target)
                .exists()
            {
                let shown = file.strip_prefix(&root).unwrap_or(&file).display();
                dead.insert(format!("{shown} -> {target}"));
            }
        }
    }

    // Not an assertion about a number — about the walk having found anything at all. A
    // scanner that matched nothing would make the real assertion vacuously true, which is the
    // one way a test like this passes while proving nothing.
    assert!(
        checked > 50,
        "only {checked} relative markdown links found; the scanner or the walk went stale"
    );
    assert!(
        dead.is_empty(),
        "markdown links pointing at files that do not exist ({} of {checked} checked):\n  {}\n\
         A link is the author asserting the path resolves. Moving a document means updating \
         what points at it, in the same commit.",
        dead.len(),
        dead.into_iter().collect::<Vec<_>>().join("\n  ")
    );
}
