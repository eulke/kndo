//! `cargo xtask check-doc-freshness` — a PR that touches a source path with real design-doc
//! coverage also touches the `internal/` document that covers it.
//!
//! This needs the actual base-vs-head diff to know what changed, which is exactly what a plain
//! `cargo test` cannot discover without shelling out to git — hence an `xtask` subcommand
//! rather than a test, following [`crate::package`]'s split between library logic
//! ([`from_args`]/[`check`], reachable from a test with no process spawned) and the thin CLI
//! wrapper in `main.rs`.
//!
//! [`DOC_COVERAGE`] is deliberately small: a path absent from it carries no obligation. Listing
//! every file in the workspace would either force unrelated docs to move in lockstep with
//! unrelated code, or — once that got noisy enough — get silenced wholesale, which is the
//! failure this gate exists to prevent. Each row here is backed by the matching document's own
//! content (see `internal/README.md`'s "Document map"), not a guess at what "should" be
//! documented.

use std::path::{Path, PathBuf};
use std::process::Command;

/// One source path with real, unambiguous coverage in one `internal/` document.
pub struct DocCoverage {
    /// A path prefix, relative to the workspace root (`crates/kndo-core/src/plugin.rs`,
    /// `crates/kndo-adapter-`). Matched by [`str::starts_with`], so a directory prefix ends in
    /// `/` and a single-file prefix names the file exactly.
    pub prefix: &'static str,
    /// Substrings that exempt an otherwise-matching path — conformance fixtures and test code
    /// change on their own cadence and are asserted byte-identical by their own gate, not by
    /// this one.
    pub excludes: &'static [&'static str],
    /// The `internal/` document path, relative to the workspace root, that documents `prefix`.
    pub doc: &'static str,
}

/// The path → document table. Extending it is a PR to this file with a citation to the
/// document section that covers the new path — the same evidence bar every row here was held
/// to.
pub const DOC_COVERAGE: &[DocCoverage] = &[
    // The `Plugin` trait's ecosystem: descriptor, activation, dependencies, sinks, hooks — RFC
    // 0003/0015/0017/0018 in PLUGINS.md narrate this file's behavior end to end.
    DocCoverage {
        prefix: "crates/kndo-core/src/plugin.rs",
        excludes: &[],
        doc: "internal/PLUGINS.md",
    },
    // The WASM host bridge for both ABI worlds (`kndo:adapter`, `kndo:plugin`) — CONTRACTS.md's
    // "WASM ABI" section is normative for this crate by name ("The host bridge
    // (`crates/kndo-plugin-api`)"), distinctly from the ecosystem plugin crates below it in the
    // workspace, whose per-plugin behavior is documented under `docs/src/plugins/`, outside
    // `internal/` and outside this gate's scope.
    DocCoverage {
        prefix: "crates/kndo-plugin-api/",
        excludes: &[],
        doc: "internal/CONTRACTS.md",
    },
    // Every language adapter crate, including the toolkit: ADAPTERS.md's per-language sections
    // cite `kndo_adapter_toolkit::` functions directly (`ContentMarkers`, `jvm_manifest`,
    // `metrics`, the stdlib format) as the shared implementation each language delegates to.
    // Conformance fixtures move on their own byte-identical gate, not this one.
    DocCoverage {
        prefix: "crates/kndo-adapter-",
        excludes: &["/tests/", "/fixtures/"],
        doc: "internal/ADAPTERS.md",
    },
    // `LanguageAdapter` and `Engine` are §2 and §5 of CONTRACTS.md's "Core traits".
    DocCoverage {
        prefix: "crates/kndo-core/src/adapter.rs",
        excludes: &[],
        doc: "internal/CONTRACTS.md",
    },
    DocCoverage {
        prefix: "crates/kndo-core/src/engine.rs",
        excludes: &[],
        doc: "internal/CONTRACTS.md",
    },
    // RFC 0004/0013 (graph & cache, incremental patch) and RFC 0005/0012 (analyses, reference
    // semantics) in GRAPH-CACHE-AND-ANALYSES.md.
    DocCoverage {
        prefix: "crates/kndo-core/src/graph/",
        excludes: &[],
        doc: "internal/GRAPH-CACHE-AND-ANALYSES.md",
    },
    DocCoverage {
        prefix: "crates/kndo-core/src/cache.rs",
        excludes: &[],
        doc: "internal/GRAPH-CACHE-AND-ANALYSES.md",
    },
    DocCoverage {
        prefix: "crates/kndo-core/src/analysis/",
        excludes: &[],
        doc: "internal/GRAPH-CACHE-AND-ANALYSES.md",
    },
    // RFC 0006/0007/0009/0010 (CLI & output, navigation, human interface, CI/Action) in
    // CLI-OUTPUT-AND-INTERFACE.md. `query.rs`/`query_envelope.rs` are the navigation verbs'
    // core-side implementation; `kndo-cli` and `action/` are its two frontends.
    DocCoverage {
        prefix: "crates/kndo-core/src/query.rs",
        excludes: &[],
        doc: "internal/CLI-OUTPUT-AND-INTERFACE.md",
    },
    DocCoverage {
        prefix: "crates/kndo-core/src/query_envelope.rs",
        excludes: &[],
        doc: "internal/CLI-OUTPUT-AND-INTERFACE.md",
    },
    DocCoverage {
        prefix: "crates/kndo-cli/",
        excludes: &[],
        doc: "internal/CLI-OUTPUT-AND-INTERFACE.md",
    },
    DocCoverage {
        prefix: "action/",
        excludes: &[],
        doc: "internal/CLI-OUTPUT-AND-INTERFACE.md",
    },
    // RFC 0008/0014 (performance & parallelism, distribution & release) in
    // PERFORMANCE-WORKSPACES-AND-RELEASE.md. `xtask::package` is the release artifact's one
    // producer; `release.yml` and `install.sh` are two of its four checked consumers
    // (`xtask/tests/release_channels.rs` covers all four staying in agreement with each other —
    // a separate concern from this gate, which asks only whether the design doc moved).
    DocCoverage {
        prefix: "xtask/src/package.rs",
        excludes: &[],
        doc: "internal/PERFORMANCE-WORKSPACES-AND-RELEASE.md",
    },
    DocCoverage {
        prefix: "xtask/src/bench.rs",
        excludes: &[],
        doc: "internal/PERFORMANCE-WORKSPACES-AND-RELEASE.md",
    },
    DocCoverage {
        prefix: ".github/workflows/release.yml",
        excludes: &[],
        doc: "internal/PERFORMANCE-WORKSPACES-AND-RELEASE.md",
    },
    DocCoverage {
        prefix: "install.sh",
        excludes: &[],
        doc: "internal/PERFORMANCE-WORKSPACES-AND-RELEASE.md",
    },
];

/// The local-dev default: this repository's branches are `main`/`develop` (`ci.yml`'s `push`
/// triggers), and `origin/main` is what a normal clone already tracks.
pub const DEFAULT_BASE: &str = "origin/main";

/// Every `internal/` document [`DOC_COVERAGE`] maps `path` to, deduplicated. Empty means `path`
/// carries no documentation obligation under this gate.
fn docs_for(path: &str) -> Vec<&'static str> {
    let mut docs: Vec<&'static str> = DOC_COVERAGE
        .iter()
        .filter(|c| path.starts_with(c.prefix) && !c.excludes.iter().any(|e| path.contains(e)))
        .map(|c| c.doc)
        .collect();
    docs.sort_unstable();
    docs.dedup();
    docs
}

/// Every `(path, doc)` pair this diff still owes: `path` is in `changed` and has real coverage,
/// but `doc` is not itself in `changed`. Sorted and deduplicated so the same pair never appears
/// twice even if two rows happened to name the same document.
pub fn missing(changed: &[String]) -> Vec<(String, &'static str)> {
    let touched: std::collections::BTreeSet<&str> = changed.iter().map(String::as_str).collect();
    let mut out: Vec<(String, &'static str)> = Vec::new();
    for path in changed {
        for doc in docs_for(path) {
            if !touched.contains(doc) {
                out.push((path.clone(), doc));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `Ok` if every path in `changed` that has real doc coverage also has its document touched in
/// the same `changed` set; otherwise `Err` naming every untouched document next to the path
/// that needed it, so a contributor knows exactly what to open.
pub fn check(changed: &[String]) -> Result<(), String> {
    let missing = missing(changed);
    if missing.is_empty() {
        return Ok(());
    }
    let lines: Vec<String> = missing
        .iter()
        .map(|(path, doc)| format!("  touched {path} but not {doc}"))
        .collect();
    Err(format!(
        "this diff changes a path with documented coverage, without changing the document \
         that covers it:\n{}\n\nCLAUDE.md's \"Keep internal/ current\" rule: a PR that changes \
         the behavior a document describes updates that document in the same PR.",
        lines.join("\n")
    ))
}

/// Every path this diff touches, relative to `root` — `git diff --name-only base...HEAD`, the
/// same merge-base semantics `kndo check --diff` and `kndo-action`'s `BASE_REF` use, so a
/// contributor's local run and CI agree on what changed.
pub fn changed_files(root: &Path, base: &str) -> Result<Vec<String>, String> {
    let range = format!("{base}...HEAD");
    let output = Command::new("git")
        .args(["diff", "--name-only", &range])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git failed to start: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let hint = base.trim_start_matches("origin/");
        return Err(format!(
            "git diff --name-only {range} failed: {}\n`{base}` must be locally resolvable — \
             fetch it first, e.g. `git fetch origin {hint}:refs/remotes/origin/{hint}`",
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// The `cargo xtask check-doc-freshness [--base <ref>]` command line, resolved into a
/// [`check`] call over the real diff — kept out of `main.rs` so a test can drive it with a
/// fake `root`, the same split [`crate::package::from_args`] uses.
pub fn from_args(args: &[String], root: Result<PathBuf, String>) -> Result<(), String> {
    let base = args
        .iter()
        .position(|a| a == "--base")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or(DEFAULT_BASE);
    let root = root?;
    let changed = changed_files(&root, base)?;
    check(&changed)
}
