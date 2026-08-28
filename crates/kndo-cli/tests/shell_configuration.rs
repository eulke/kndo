//! The "shell" configuration — a `kndo` binary with every first-party grammar off and just
//! enough to load third-party WASM components — is a claim the design contract makes, and CI's
//! `shell-build` job proves it by actually building it. This test guards the seam between the
//! two: **the feature names that workflow passes must exist in this crate and forward.**
//!
//! It replaces `default_features_mirror_the_library`, which watched a list this crate no longer
//! keeps: `default` is now `["kndo/default"]`, one entry that cannot drift from what the
//! library enables, so there is nothing left there to guard. What can still break is the
//! workflow's command going stale against a renamed feature — and that failure surfaces today
//! only as a cargo error deep in a several-minute release build, on a job whose whole point is
//! to be the one that catches shell regressions.
//!
//! Reading the workflow rather than hardcoding the names is the point. A copy of the list here
//! would be exactly the duplication the change this test ships removed.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo-cli has two ancestors up to the workspace root")
        .to_path_buf()
}

/// Every `--features <list>` that accompanies a `--no-default-features` on a `-p kndo-cli`
/// command line in CI, flattened to individual feature names.
fn shell_features_named_by_ci() -> Vec<String> {
    let workflow = std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml"))
        .expect(".github/workflows/ci.yml");
    let mut out = Vec::new();
    for line in workflow.lines() {
        if !(line.contains("-p kndo-cli") && line.contains("--no-default-features")) {
            continue;
        }
        let Some(rest) = line.split("--features ").nth(1) else {
            continue;
        };
        let list = rest.split_whitespace().next().unwrap_or_default();
        out.extend(
            list.split(',')
                .filter(|f| !f.is_empty())
                .map(str::to_string),
        );
    }
    out.sort();
    out.dedup();
    out
}

fn cli_features() -> toml::Table {
    let manifest: toml::Table =
        std::fs::read_to_string(repo_root().join("crates/kndo-cli/Cargo.toml"))
            .expect("crates/kndo-cli/Cargo.toml")
            .parse()
            .expect("a valid manifest");
    manifest["features"].as_table().expect("[features]").clone()
}

#[test]
fn ci_names_shell_features_this_crate_actually_has() {
    let named = shell_features_named_by_ci();
    // Not an assertion about a count — about the parse having found anything at all. A regex
    // that matched nothing would make every assertion below vacuously true, which is the one
    // way a test like this fails silently.
    assert!(
        !named.is_empty(),
        "no `-p kndo-cli --no-default-features --features …` command found in ci.yml — either \
         the shell-build job is gone (a design-contract claim CI no longer proves) or this \
         parse went stale"
    );

    let features = cli_features();
    let missing: Vec<&String> = named
        .iter()
        .filter(|f| !features.contains_key(*f))
        .collect();
    assert!(
        missing.is_empty(),
        "ci.yml builds the shell configuration with features this crate does not declare: \
         {missing:?} — the job fails with a cargo error several minutes into a release build; \
         fix the name in one place or the other"
    );

    for feature in &named {
        let forwards = features[feature].as_array().is_some_and(|deps| {
            deps.iter()
                .filter_map(toml::Value::as_str)
                .any(|d| d.starts_with("kndo/"))
        });
        assert!(
            forwards,
            "`{feature}` forwards nothing to `kndo/…`; kndo-cli has no cfg(feature) of its own, \
             so a feature that forwards nothing turns on nothing"
        );
    }
}

#[test]
fn default_forwards_the_librarys_own_default_set() {
    let features = cli_features();
    let default: Vec<&str> = features["default"]
        .as_array()
        .expect("default = [...]")
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    assert_eq!(
        default,
        ["kndo/default"],
        "the shipped binary's default set must BE the library's, by reference — spelling the \
         list out again is what let kndo:uikit be registered in the library and shipped in no \
         binary at all"
    );
}
