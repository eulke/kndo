//! The range a semver requirement names, and the one comparison a range is for.
//! One reader for two ecosystems: npm and cargo spell `^`, `~`, `=`, `>=` and
//! the `x`/`*` wildcards identically and differ on the BARE form alone, which
//! is why [`kndo_toolkit::Bare`] is the only parameter.

use kndo_contract::manifest::{Version, VersionReq};
use kndo_toolkit::{Bare, semver_range};

#[test]
fn a_requirement_reads_as_the_range_its_ecosystem_means() {
    let v = Version::new;
    let caret = |r: &str| semver_range(r, Bare::Caret);
    let exact = |r: &str| semver_range(r, Bare::Exact);

    // Caret holds the leftmost NON-ZERO number fixed, and where every stated
    // number is zero, the first unstated one is what may move.
    assert_eq!(caret("1.2.3"), Some((v(1, 2, 3), v(2, 0, 0))));
    assert_eq!(caret("^1.2"), Some((v(1, 2, 0), v(2, 0, 0))));
    assert_eq!(caret("^0.2.3"), Some((v(0, 2, 3), v(0, 3, 0))));
    assert_eq!(caret("^0.0.3"), Some((v(0, 0, 3), v(0, 0, 4))));
    assert_eq!(caret("^0.0"), Some((v(0, 0, 0), v(0, 1, 0))));
    assert_eq!(caret("^0"), Some((v(0, 0, 0), v(1, 0, 0))));

    // The same text, pinned: npm's bare form.
    assert_eq!(exact("1.2.3"), Some((v(1, 2, 3), v(1, 2, 4))));
    assert_eq!(exact("~1.2.3"), Some((v(1, 2, 3), v(1, 3, 0))));
    assert_eq!(exact("1.2.x"), Some((v(1, 2, 0), v(1, 3, 0))));
    assert_eq!(exact("1.x"), Some((v(1, 0, 0), v(2, 0, 0))));
    assert_eq!(exact("=2.0.0"), Some((v(2, 0, 0), v(2, 0, 1))));

    // Pre-release and build metadata are not part of the ordering.
    assert_eq!(caret("1.2.3-rc1+build"), Some((v(1, 2, 3), v(2, 0, 0))));
    // Open above: nothing is disjoint from it.
    assert_eq!(caret(">=1.2.3").map(|r| r.1), Some(v(u64::MAX, 0, 0)));

    // What this reader does not spell stays UNCOMPARED — a range guessed
    // wrong is worse than no range at all.
    for unreadable in [">=1, <2", "1.0.0 || 2.0.0", ">1.0 <2.0", "", "latest", "*"] {
        assert_eq!(caret(unreadable), None, "{unreadable}");
    }
}

#[test]
fn two_texts_are_not_two_requirements_and_unknown_is_never_a_conflict() {
    let ranged = |spelled: &str, bare: Bare| VersionReq {
        spelled: spelled.into(),
        range: semver_range(spelled, bare),
    };
    let one = ranged("^1.2", Bare::Caret);
    let wider = ranged("^1.3", Bare::Caret);
    let apart = ranged("^2", Bare::Caret);

    assert_eq!(
        one.disjoint(&wider),
        Some(false),
        "cargo unifies them: two spellings, one resolvable requirement"
    );
    assert_eq!(one.disjoint(&apart), Some(true));
    assert_eq!(
        one.disjoint(&VersionReq::spelled("workspace:*")),
        None,
        "a requirement this vocabulary cannot compare states no conflict"
    );
    assert_eq!(
        VersionReq::spelled("1.2").range,
        None,
        "the spelled-only constructor derives nothing"
    );
}
