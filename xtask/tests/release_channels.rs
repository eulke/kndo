//! **Every release channel spells the artifact the producer actually uploads.**
//!
//! Four things consume kndo's release artifacts and none of them can call Rust: `install.sh`,
//! `action/action.yml`, `packaging/homebrew/kndo.rb.tmpl`, and the published install docs. Each
//! used to spell the artifact name and assume its layout independently, which is how three of
//! the four came to be broken at once with nothing failing:
//!
//! - the Action downloaded `-unknown-linux-gnu` triples the release has never built, from a
//!   URL with the tag's leading `v` stripped — two independent 404s on every platform;
//! - `install.sh` extracted correctly and then copied `kndo` from the extraction root, where
//!   the archive's staged directory means it is not;
//! - the docs' copy-pasteable one-liner had both faults.
//!
//! None of it was discoverable before a tag was pushed. These tests read the four files as
//! text and check them against [`xtask::package`], so a channel that drifts fails on the PR
//! that drifts it. Text matching is the point: these consumers *are* text, and a test that
//! re-derived their behavior instead of reading it would be a fifth copy.

use xtask::package::{self, Archive};

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ has a parent")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A representative tag. Everything here is about *shape*, so one concrete tag exercises it.
const TAG: &str = "v1.2.0";

// ---------------------------------------------------------------------------------------
// The producer
// ---------------------------------------------------------------------------------------

/// `release.yml`'s build matrix is [`package::TARGETS`], not a second list beside it.
#[test]
fn the_release_matrix_is_the_target_table() {
    let yml = read(".github/workflows/release.yml");
    let in_yml: Vec<&str> = yml
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- target: "))
        .collect();
    let expected: Vec<&str> = package::TARGETS.iter().map(|t| t.triple).collect();
    assert_eq!(
        in_yml, expected,
        "release.yml builds a different set of targets than xtask::package::TARGETS declares — \
         they are one list"
    );

    // …and each one on the runner and build mode the table says.
    for target in package::TARGETS {
        let block = yml
            .split("- target: ")
            .find(|b| b.starts_with(target.triple))
            .unwrap_or_else(|| panic!("no matrix entry for {}", target.triple));
        let head: String = block.lines().take(3).collect::<Vec<_>>().join("\n");
        assert!(
            head.contains(&format!("os: {}", target.runner)),
            "{} should build on {}: {head}",
            target.triple,
            target.runner
        );
        assert!(
            head.contains(&format!("cross: {}", target.cross)),
            "{} should have cross: {}: {head}",
            target.triple,
            target.cross
        );
    }
}

/// The workflow packages by calling `xtask package`, never by hand-rolling an archive. This is
/// what keeps the layout single-sourced: a `tar -czf` line in the workflow would be a second
/// producer, and the layout is exactly what two consumers got wrong.
#[test]
fn the_workflow_packages_through_xtask() {
    let yml = read(".github/workflows/release.yml");
    // Checked as two substrings rather than one literal: the invocation carries flags and is
    // line-continued, and pinning its exact spelling would make this a test about formatting.
    assert!(
        yml.contains("-p xtask") && yml.contains("-- package"),
        "release.yml must package through `cargo run -p xtask -- package`"
    );
    // Comment lines are dropped first: the step's own comment explains *why* there is no
    // `tar -czf` here, and a check that cannot tell prose from a command would flag it.
    let commands: String = yml
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    for hand_rolled in ["tar -czf", "Compress-Archive"] {
        assert!(
            !commands.contains(hand_rolled),
            "release.yml still builds an archive by hand (`{hand_rolled}`) — the artifact's \
             layout has one producer, and it is xtask::package"
        );
    }
}

// ---------------------------------------------------------------------------------------
// The consumers
// ---------------------------------------------------------------------------------------

/// `install.sh` asks for the archive that exists, and takes the binary from where it is.
#[test]
fn the_installer_matches_the_artifact() {
    let sh = read("install.sh");

    // Name: `$VERSION` is the tag verbatim (that is what the releases API's `tag_name` is), so
    // the composed name must be the producer's, `v` and all.
    assert!(
        sh.contains(r#"ARCHIVE="kndo-${VERSION}-${TARGET}.tar.gz""#),
        "install.sh composes a different archive name than xtask::package::artifact produces"
    );
    assert!(
        !sh.contains("${VERSION#v}")
            && !sh.contains(
                "$
{VERSION#v}"
            ),
        "install.sh must not strip the tag's leading `v` — the uploaded asset carries it"
    );

    // Layout: the archive nests everything under one directory, so extraction must strip it.
    assert!(
        sh.contains("--strip-components=1"),
        "install.sh must strip the archive's staged directory — without it, the `kndo` it \
         copies from the extraction root does not exist"
    );

    // Platforms: every uname pair the table declares resolves to that pair's triple, and
    // install.sh names no triple the release does not build.
    for target in package::TARGETS {
        for (os, arch) in target.uname {
            let line = sh
                .lines()
                .find(|l| l.contains(&format!("{os}/{arch}")))
                .unwrap_or_else(|| panic!("install.sh does not detect {os}/{arch}"));
            assert!(
                line.contains(target.triple),
                "install.sh maps {os}/{arch} to something other than {}: {line}",
                target.triple
            );
        }
    }
    assert_no_unreleased_triples(&sh, "install.sh");
}

/// The GitHub Action downloads the same asset the release uploads.
#[test]
fn the_action_matches_the_artifact() {
    let yml = read("action/action.yml");

    assert!(
        !yml.contains("version_no_v"),
        "the Action must not strip the tag's leading `v` — `kndo-1.2.0-…` was never uploaded, \
         `kndo-v1.2.0-…` was"
    );
    assert!(
        yml.contains(r#"asset="kndo-${KNDO_VERSION}-${target}.tar.gz""#),
        "the Action composes a different asset name than xtask::package::artifact produces"
    );
    assert!(
        yml.contains("--strip-components=1"),
        "the Action must strip the archive's staged directory"
    );

    for target in package::TARGETS {
        for (os, arch) in target.uname {
            let line = yml
                .lines()
                .find(|l| l.contains(&format!(r#""{os} {arch}""#)))
                .unwrap_or_else(|| panic!("action.yml does not detect `{os} {arch}`"));
            assert!(
                line.contains(target.triple),
                "action.yml maps `{os} {arch}` to something other than {}: {line}",
                target.triple
            );
        }
    }
    assert_no_unreleased_triples(&yml, "action/action.yml");
}

/// The Homebrew formula's four URLs are four real assets.
#[test]
fn the_homebrew_template_matches_the_artifact() {
    let tmpl = read("packaging/homebrew/kndo.rb.tmpl");
    // The template renders `{{VERSION}}` without the leading `v` and writes the `v` itself, so
    // substituting the bare version reproduces exactly what a release names.
    let rendered = tmpl.replace("{{VERSION}}", TAG.trim_start_matches('v'));

    for target in package::TARGETS {
        // Homebrew serves macOS and Linux only; Windows has no formula.
        if target.triple.contains("windows") {
            continue;
        }
        let art = package::artifact(TAG, target);
        assert!(
            rendered.contains(&format!("/releases/download/{TAG}/{}", art.file_name)),
            "the formula has no URL for {} — expected …/releases/download/{TAG}/{}",
            target.triple,
            art.file_name
        );
    }
    assert_no_unreleased_triples(&tmpl, "packaging/homebrew/kndo.rb.tmpl");

    // `bin.install "kndo"` works because Homebrew enters the archive's single root directory;
    // the binary's name inside is the one the table says.
    assert!(
        rendered.contains(&format!(
            r#"bin.install "{}""#,
            package::TARGETS[0].binary()
        )),
        "the formula installs a binary by a name the archive does not contain"
    );
}

/// The install docs' copy-pasteable commands are commands that work.
#[test]
fn the_install_docs_match_the_artifact() {
    let md = read("docs/src/install.md");
    assert_no_unreleased_triples(&md, "docs/src/install.md");

    // Any concrete asset name in the docs must be one a release actually produces. The docs
    // name a version of their own, so compare on shape: `kndo-v<something>-<triple>.<ext>`.
    for line in md.lines() {
        let Some(at) = line.find("kndo-") else {
            continue;
        };
        let rest = &line[at..];
        let Some(end) = rest.find(".tar.gz").map(|i| i + ".tar.gz".len()) else {
            continue;
        };
        let name = &rest[..end];
        let Some(target) = package::TARGETS
            .iter()
            .find(|t| name.contains(t.triple) && t.archive == Archive::TarGz)
        else {
            continue;
        };
        assert_eq!(
            name,
            package::artifact(
                name.trim_start_matches("kndo-")
                    .split(&format!("-{}", target.triple))
                    .next()
                    .unwrap_or_default(),
                target
            )
            .file_name,
            "docs/src/install.md names an asset shaped differently than releases produce"
        );
        assert!(
            name.starts_with("kndo-v"),
            "docs/src/install.md drops the tag's leading `v` from {name} — that asset does not \
             exist"
        );
    }

    // And the extraction one-liner must account for the staged directory, exactly as the
    // installer does.
    if md.contains("| tar -xz") {
        assert!(
            md.contains("--strip-components=1"),
            "the docs' `tar -xz` one-liner leaves the binary inside the archive's staged \
             directory, so the `./kndo` that follows it does not exist"
        );
    }
}

/// No channel may name a target triple the release does not build. This is the check that
/// would have caught the Action asking for `-unknown-linux-gnu`: the name was well-formed and
/// plausible, and no asset by it has ever existed.
fn assert_no_unreleased_triples(text: &str, what: &str) {
    const PLAUSIBLE_BUT_UNBUILT: &[&str] = &[
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-pc-windows-gnu",
        "aarch64-pc-windows-msvc",
    ];
    for triple in PLAUSIBLE_BUT_UNBUILT {
        assert!(
            !text.contains(triple),
            "{what} names {triple}, which no release builds — see xtask::package::TARGETS"
        );
    }
}

// ---------------------------------------------------------------------------------------
// The producer's own shape
// ---------------------------------------------------------------------------------------

/// The layout every consumer strips exactly one component of: one directory, named for the
/// archive, holding the binary and the two files that accompany it.
#[test]
fn every_artifact_nests_under_one_directory_named_for_itself() {
    for target in package::TARGETS {
        let art = package::artifact(TAG, target);
        assert_eq!(art.stem, format!("kndo-{TAG}-{}", target.triple));
        assert_eq!(
            art.file_name,
            format!("{}.{}", art.stem, target.archive.extension())
        );
        assert_eq!(
            art.binary_in_archive,
            format!("{}/{}", art.stem, target.binary())
        );
        assert_eq!(
            art.binary_in_archive.matches('/').count(),
            1,
            "exactly one component to strip"
        );
    }
}

/// Windows gets `kndo.exe` in a `.zip`; everything else `kndo` in a `.tar.gz`. Stated as a test
/// because two consumers hard-code the Unix answer and must keep being right to.
#[test]
fn the_windows_target_is_the_only_zip_and_the_only_exe() {
    let zips: Vec<&str> = package::TARGETS
        .iter()
        .filter(|t| t.archive == Archive::Zip)
        .map(|t| t.triple)
        .collect();
    assert_eq!(zips, vec!["x86_64-pc-windows-msvc"]);
    for target in package::TARGETS {
        let expected = if target.triple.contains("windows") {
            "kndo.exe"
        } else {
            "kndo"
        };
        assert_eq!(target.binary(), expected, "{}", target.triple);
    }
}

/// A real archive, produced and read back: the name, the nesting and the contents are what the
/// consumers are checked against above. Without this the rest of the file could agree perfectly
/// with a producer that writes something else.
#[test]
fn packaging_produces_the_archive_the_consumers_expect() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = package::target("x86_64-unknown-linux-musl").expect("a released target");
    let bin = dir.path().join("kndo");
    std::fs::write(&bin, b"#!/bin/sh\necho kndo\n").expect("write the stand-in binary");

    let out = package::package(&root(), TAG, target, &bin, dir.path()).expect("package");
    let art = package::artifact(TAG, target);
    assert_eq!(
        out.file_name().and_then(|n| n.to_str()),
        Some(&*art.file_name)
    );

    let file = std::fs::File::open(&out).expect("open the archive");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut names: Vec<String> = archive
        .entries()
        .expect("entries")
        .map(|e| {
            e.expect("entry")
                .path()
                .expect("path")
                .display()
                .to_string()
        })
        .collect();
    names.sort();

    let mut expected = vec![art.binary_in_archive.clone()];
    for extra in package::EXTRA_FILES {
        expected.push(format!("{}/{extra}", art.stem));
    }
    expected.sort();
    assert_eq!(names, expected);
}
