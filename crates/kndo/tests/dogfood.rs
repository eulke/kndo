//! kndo, run on kndo, must stay clean.
//!
//! Everything else in this suite proves a mechanism on a fixture. This proves the product on
//! itself, which is the only check that cannot be satisfied by a fixture written to pass: the
//! subject is the whole workspace, and it changes every commit.
//!
//! It exists because "we took the findings to zero" is a photograph, not an invariant. Getting
//! here took a `duplicate` count of 28 down to nothing, and every one of those was code someone
//! wrote without noticing they were writing it twice — which is exactly what will happen again.
//! A new finding fails this test. Accepting one is a deliberate act: add it to `ACCEPTED` with
//! a reason, in the same commit, where a reviewer sees it. Never silently.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Findings this repository knowingly carries. Each entry is `(category, path, symbol,
/// why it is accepted)` — the reason is not decoration: an entry no one can justify in a
/// sentence is a bug being filed under "known".
///
/// Empty, and the intent is that it stays that way. An entry here is a debt with a name.
const ACCEPTED: &[(&str, &str, &str)] = &[];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo has two ancestors up to the workspace root")
        .to_path_buf()
}

#[test]
fn kndo_is_clean_under_kndo() {
    let root = workspace_root();
    let overrides = kndo_core::engine::ConfigOverrides {
        // Never the cache: this must analyze what is on disk right now, and it must not leave
        // a `.kndo/` behind in the working tree of whoever ran the suite.
        use_cache: false,
        threads: Some(1),
        ..kndo_core::engine::ConfigOverrides::default()
    };
    let mut engine = kndo::open(&root, overrides).expect("kndo::open on its own workspace");
    let result = engine.check(kndo_core::engine::RunMode::Full);

    let accepted: BTreeSet<(&str, &str, &str)> = ACCEPTED.iter().copied().collect();
    let unaccepted: Vec<String> = result
        .findings
        .iter()
        .filter(|f| {
            let identity = (
                f.category.as_str(),
                f.location.path.as_ref().map(|p| p.0.as_str()).unwrap_or(""),
                f.location.symbol.as_deref().unwrap_or(""),
            );
            !accepted.contains(&identity)
        })
        .map(|f| {
            format!(
                "  {} {}#{} — {}",
                f.category,
                f.location.path.as_ref().map(|p| p.0.as_str()).unwrap_or(""),
                f.location.symbol.as_deref().unwrap_or(""),
                f.message,
            )
        })
        .collect();

    assert!(
        unaccepted.is_empty(),
        "kndo reports {} finding(s) on kndo that nothing accepts.\n{}\n\nFix them, or — if one \
         is genuinely correct to carry — add it to ACCEPTED in this file with the reason, in \
         the same commit.",
        unaccepted.len(),
        unaccepted.join("\n"),
    );

    // An entry that no longer fires is the mirror failure: `ACCEPTED` decaying into a list of
    // things that used to be true is how a baseline stops meaning anything. This is the same
    // rule the `stale` category applies to inline pragmas, applied to this file.
    let live: BTreeSet<(&str, &str, &str)> = result
        .findings
        .iter()
        .map(|f| {
            (
                f.category.as_str(),
                f.location.path.as_ref().map(|p| p.0.as_str()).unwrap_or(""),
                f.location.symbol.as_deref().unwrap_or(""),
            )
        })
        .collect();
    let stale: Vec<_> = accepted.difference(&live).collect();
    assert!(
        stale.is_empty(),
        "ACCEPTED names {} finding(s) that no longer fire — delete them: {stale:?}",
        stale.len(),
    );
}

#[test]
fn zero_findings_means_measured_and_not_merely_unjudged() {
    // Zero findings in a category NO analysis judged is the absence of a measurement, not a
    // pass (`run.abstained`, output-schema §1). Without this, a change that quietly stopped an
    // analysis from running would make the test above *more* likely to pass — the exact
    // failure direction a dogfood gate must not have.
    //
    // `crap` is the one category allowed to abstain, and whether it does is not this repo's
    // content: kndo ingests coverage rather than measuring it, and `lcov.info` is a gitignored
    // build artifact. A contributor who followed CONTRIBUTING's coverage section has one and
    // `crap` gets judged; CI generates it in the same step that runs this test, so here it does
    // not yet exist and `crap` abstains. Both are correct, which is why this asserts a subset
    // rather than an exact list — pinning the exact list would fail on a developer's machine
    // for having MORE measurement, which is nonsense.
    //
    // Anything else appearing here means an analysis stopped judging, and the clean bill of
    // health above is worth less than it looks.
    let root = workspace_root();
    let overrides = kndo_core::engine::ConfigOverrides {
        use_cache: false,
        threads: Some(1),
        ..kndo_core::engine::ConfigOverrides::default()
    };
    let mut engine = kndo::open(&root, overrides).expect("kndo::open on its own workspace");
    let result = engine.check(kndo_core::engine::RunMode::Full);

    let unexpected: Vec<&kndo_core::analysis::Abstention> = result
        .abstained
        .iter()
        .filter(|a| a.category.as_str() != "crap")
        .collect();
    assert!(
        unexpected.is_empty(),
        "an analysis stopped judging, so the zero above is partly unmeasured: {unexpected:?}",
    );
}
