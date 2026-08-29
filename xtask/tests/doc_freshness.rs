//! **A diff that touches a mapped path also touches the document that covers it.**
//!
//! `check`/`missing` are pure over a path list, so most of this file proves them without
//! spawning git at all — the fast, robust half `xtask/tests/release_channels.rs` favors for
//! text-shaped assertions. `changed_files_reports_the_real_diff_against_a_resolvable_base`
//! is the one place this file does spawn git, against a throwaway two-commit repo under
//! `tempfile::tempdir()`, so the `--base` plumbing itself is proven end to end and not just
//! assumed correct because the pure half passes.

use xtask::doc_freshness;

#[test]
fn a_mapped_path_with_its_doc_touched_passes() {
    let changed = vec![
        "crates/kndo-core/src/plugin.rs".to_string(),
        "internal/PLUGINS.md".to_string(),
    ];
    assert!(doc_freshness::check(&changed).is_ok());
}

#[test]
fn the_same_change_without_the_doc_fails_naming_both() {
    let changed = vec!["crates/kndo-core/src/plugin.rs".to_string()];
    let err = doc_freshness::check(&changed).expect_err("plugin.rs has real coverage");
    assert!(
        err.contains("touched crates/kndo-core/src/plugin.rs but not internal/PLUGINS.md"),
        "message does not name the specific path and doc: {err}"
    );
}

#[test]
fn an_unmapped_path_never_fails_regardless_of_what_else_changed() {
    // Alone: nothing in DOC_COVERAGE claims this path, so there is nothing to check.
    let alone = vec!["crates/kndo-core/src/gitutil.rs".to_string()];
    assert!(doc_freshness::check(&alone).is_ok());

    // Alongside a real violation: the unmapped path contributes no obligation of its own, and
    // the failure names only the path that actually has coverage.
    let mixed = vec![
        "crates/kndo-core/src/gitutil.rs".to_string(),
        "crates/kndo-core/src/plugin.rs".to_string(),
    ];
    let err = doc_freshness::check(&mixed).expect_err("plugin.rs still lacks its doc");
    assert!(err.contains("crates/kndo-core/src/plugin.rs"));
    assert!(err.contains("internal/PLUGINS.md"));
    assert!(
        !err.contains("gitutil.rs"),
        "an unmapped path must never appear in the failure: {err}"
    );
}

#[test]
fn every_row_is_satisfiable_and_names_a_document_this_repository_actually_ships() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ has a parent")
        .to_path_buf();
    for row in doc_freshness::DOC_COVERAGE {
        assert!(
            root.join(row.doc).is_file(),
            "{} names {}, which does not exist",
            row.prefix,
            row.doc
        );
        // Touching the prefix itself plus its own doc must always be clean — a row that failed
        // this would mean its own doc path is wrong, or excludes swallow the prefix whole.
        let changed = vec![
            format!("{}synthetic-marker", row.prefix.trim_end_matches('/')),
            row.doc.to_string(),
        ];
        assert!(
            doc_freshness::check(&changed).is_ok(),
            "{} + {} should be clean together",
            row.prefix,
            row.doc
        );
    }
}

#[test]
fn adapter_conformance_fixtures_are_excluded_from_the_adapters_doc_obligation() {
    let changed = vec!["crates/kndo-adapter-go/tests/conformance.rs".to_string()];
    assert!(
        doc_freshness::check(&changed).is_ok(),
        "test/fixture paths move on their own byte-identical gate, not this one"
    );

    // Non-excluded source under the same crate still owes ADAPTERS.md.
    let changed = vec!["crates/kndo-adapter-go/src/lib.rs".to_string()];
    let err = doc_freshness::check(&changed).expect_err("adapter source has real coverage");
    assert!(err.contains("internal/ADAPTERS.md"));
}

#[test]
fn the_wasm_host_bridge_is_distinct_from_the_ecosystem_plugin_crates() {
    // kndo-plugin-api is the ABI host bridge CONTRACTS.md's "WASM ABI" section names by path.
    let changed = vec!["crates/kndo-plugin-api/src/host.rs".to_string()];
    let err = doc_freshness::check(&changed).expect_err("the host bridge has real coverage");
    assert!(err.contains("internal/CONTRACTS.md"));

    // An ecosystem plugin crate's own detection logic is documented under docs/src/plugins/,
    // outside internal/ and outside this gate's scope — no obligation here.
    let changed = vec!["crates/kndo-plugin-express/src/lib.rs".to_string()];
    assert!(doc_freshness::check(&changed).is_ok());
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git failed to start");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

/// `changed_files` against a real, throwaway repo — not just the pure logic above — so the
/// `base...HEAD` plumbing (and its merge-base semantics) is proven, not merely assumed.
#[test]
fn changed_files_reports_the_real_diff_against_a_resolvable_base() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "test@kndo"]);
    git(root, &["config", "user.name", "test"]);

    std::fs::write(root.join("a.txt"), "one\n").expect("write a.txt");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    git(root, &["branch", "base-branch"]);

    // A commit on the base branch AFTER the fork point must not appear in the diff — proving
    // this is `base...HEAD` (merge-base) semantics, not a plain two-dot `base..HEAD`.
    git(root, &["checkout", "-q", "base-branch"]);
    std::fs::write(root.join("only-on-base.txt"), "x\n").expect("write only-on-base.txt");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base-only"]);
    git(root, &["checkout", "-q", "-"]);

    std::fs::write(root.join("b.txt"), "two\n").expect("write b.txt");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "head"]);

    let changed = doc_freshness::changed_files(root, "base-branch").expect("git diff succeeds");
    assert_eq!(changed, vec!["b.txt".to_string()]);
}

#[test]
fn an_unresolvable_base_is_a_clear_error_not_a_panic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "test@kndo"]);
    git(root, &["config", "user.name", "test"]);
    std::fs::write(root.join("a.txt"), "one\n").expect("write a.txt");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);

    let err = doc_freshness::changed_files(root, "origin/does-not-exist")
        .expect_err("no such ref exists");
    assert!(
        err.contains("origin/does-not-exist") && err.contains("fetch"),
        "error should name the unresolvable ref and suggest fetching it: {err}"
    );
}
