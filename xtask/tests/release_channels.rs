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

/// **Every published link names the repository that exists.**
///
/// ADR 0007 proposes renaming `eulke/kondo` to `eulke/kndo`, and three places had already
/// adopted the new name: `Cargo.toml`'s `repository` — the one piece of metadata crates.io
/// publishes as the project's home — and the two links `kndo plugin new` writes into every
/// scaffolded plugin's docs. All three 404 for anyone who follows them, and would keep doing so
/// until an administrative action nobody scheduled.
///
/// The direction of the rename is not in question; the spelling to publish is. RFC 0014 §3
/// settles it: write the *current* name, because GitHub's post-rename redirect makes it resolve
/// forever once the rename lands, while the new name resolves only after. So the old name is
/// right before and after, and the new name is right only after — which makes this a one-way
/// check rather than a preference.
///
/// Scanned as text across everything the project ships, `internal/` excluded: that is design
/// record, and it discusses both names by necessity. Container image names are excluded too,
/// for the reason given at the check itself.
#[test]
fn every_published_url_names_the_repository_that_exists() {
    const SHIPPED: &[&str] = &[
        "Cargo.toml",
        "README.md",
        "SECURITY.md",
        "CODE_OF_CONDUCT.md",
        "install.sh",
        "action/action.yml",
        "packaging/homebrew/kndo.rb.tmpl",
        "docs/book.toml",
        "docs/src/install.md",
        "crates/kndo/src/author.rs",
        "crates/kndo-core/src/sarif.rs",
        ".github/workflows/release.yml",
    ];
    let mut checked = 0;
    for rel in SHIPPED {
        let text = read(rel);
        for (n, line) in text.lines().enumerate() {
            // A container image is not a repository. `ghcr.io/eulke/kndo` is the *product* name
            // under the `eulke` registry namespace — an image is named whatever is pushed, no
            // redirect and no rename involved — so it is correct as it stands and stays.
            if line.contains("ghcr.io") {
                continue;
            }
            // `homebrew-tap` and any other `eulke/<something>` repo are their own names; only
            // this project's own two spellings are at issue.
            if line.contains("eulke/kndo") && !line.contains("eulke/kondo") {
                panic!(
                    "{rel}:{} names eulke/kndo, which does not exist until the ADR 0007 rename \
                     happens — publish eulke/kondo, which resolves before it and (through \
                     GitHub's redirect) after it too:\n  {}",
                    n + 1,
                    line.trim()
                );
            }
            if line.contains("eulke/kondo") {
                checked += 1;
            }
        }
    }
    assert!(
        checked > 10,
        "only {checked} references found — this test stopped looking at the real files"
    );
}

/// **The declared MSRV covers every package, and CI builds on exactly it.**
///
/// `rust-version` in `[workspace.package]` applies to nothing on its own: members inherit
/// package fields one by one (`version.workspace = true`, `edition.workspace = true`, …), so a
/// crate that does not opt in carries no MSRV at all and a new crate silently opts out by
/// default. And a declared MSRV nothing builds against is a number that drifts the first time
/// someone uses a newer feature — so the `msrv` CI job pins the same version, and this test ties
/// the two together.
///
/// The number itself was measured, not chosen: `cargo check --workspace --all-features` fails
/// on 1.85 and 1.82, and cargo names the binding constraint out of the locked graph
/// (`smol_str@0.3.6 requires rustc 1.89`).
#[test]
fn the_declared_msrv_is_inherited_everywhere_and_built_in_ci() {
    let manifest = read("Cargo.toml");
    let msrv = manifest
        .lines()
        .find_map(|l| l.trim().strip_prefix("rust-version = "))
        .map(|v| v.trim().trim_matches('"').to_string())
        .expect("[workspace.package] declares a rust-version");

    let mut members = 0;
    for entry in std::fs::read_dir(root().join("crates")).expect("crates/") {
        let dir = entry.expect("dir entry").path();
        let cargo = dir.join("Cargo.toml");
        if !cargo.is_file() {
            continue;
        }
        members += 1;
        let text = std::fs::read_to_string(&cargo).expect("member manifest");
        assert!(
            text.contains("rust-version.workspace = true"),
            "{} does not inherit rust-version, so the workspace MSRV does not cover it",
            cargo.display()
        );
    }
    assert!(members > 15, "only {members} member crates found");

    let ci = read(".github/workflows/ci.yml");
    assert!(
        ci.contains(&format!("toolchain: {msrv}")),
        "no CI job builds on the declared MSRV {msrv} — an unbuilt MSRV is a number, not a claim"
    );
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
/// **Every released target is a `.tar.gz` holding a binary called `kndo`.**
///
/// This used to assert that `x86_64-pc-windows-msvc` was the one `.zip` with the one `.exe`.
/// Windows is no longer a release target (RFC 0014 §3.3) — `tree-sitter-scss`'s build script
/// hands `cl.exe` a flag it refuses, so the binary could never be built — and with it went the
/// zip writer, which nothing then produced. What is asserted here is what the four consumers
/// actually resolve today; if a second archive format ever returns, this is the test that has
/// to change first.
fn every_released_target_is_a_tar_gz_named_kndo() {
    for target in package::TARGETS {
        assert_eq!(
            target.archive,
            Archive::TarGz,
            "{} is packed as something else",
            target.triple
        );
        assert_eq!(target.binary(), "kndo", "{}", target.triple);
        let art = package::artifact(TAG, target);
        assert!(
            art.file_name.ends_with(".tar.gz"),
            "{} -> {}",
            target.triple,
            art.file_name
        );
        assert!(
            !target.triple.contains("windows"),
            "Windows is not a release target; adding one back means restoring an archive \
             format and an installer path, not just a row in this table"
        );
    }
}

/// A real archive, produced and read back: the name, the nesting and the contents are what the
/// consumers are checked against above. Without this the rest of the file could agree perfectly
/// with a producer that writes something else.
/// **The packaging command line resolves the way a release depends on.**
///
/// `from_args` used to be `package_inner` in `main.rs`, where no test could reach it — kndo's
/// own `crap` analysis reported it at 0% coverage on this repository. Argument handling is
/// where a release command goes wrong quietly, so each rule it applies is asserted here.
#[test]
fn the_package_command_line_resolves_target_tag_and_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = root();
    let args = |v: &[&str]| -> Vec<String> { v.iter().map(|s| s.to_string()).collect() };

    // A target that is not in the table is refused, and the error names what IS built rather
    // than leaving the caller to guess.
    let err = package::from_args(
        &args(&["--target", "x86_64-unknown-linux-gnu"]),
        Ok(root.clone()),
    )
    .expect_err("gnu is not a released target");
    assert!(err.contains("unknown target"), "{err}");
    assert!(
        err.contains("x86_64-unknown-linux-musl"),
        "the message lists the real table: {err}"
    );

    // `--target` is required: there is no sensible default for what to build.
    let err = package::from_args(&args(&[]), Ok(root.clone())).expect_err("no target");
    assert!(err.contains("--target"), "{err}");

    // With an explicit binary, tag and out-dir, the artifact lands where it was asked to and
    // carries the name every consumer resolves.
    let bin = dir.path().join("kndo");
    std::fs::write(&bin, b"stand-in").expect("write");
    let out = package::from_args(
        &args(&[
            "--target",
            "x86_64-unknown-linux-musl",
            "--tag",
            TAG,
            "--bin",
            bin.to_str().expect("utf-8"),
            "--out-dir",
            dir.path().to_str().expect("utf-8"),
        ]),
        Ok(root.clone()),
    )
    .expect("package");
    let target = package::target("x86_64-unknown-linux-musl").expect("target");
    assert_eq!(
        out.file_name().and_then(|n| n.to_str()),
        Some(&*package::artifact(TAG, target).file_name)
    );

    // A failure finding the workspace root is reported, not papered over with a default.
    let err = package::from_args(
        &args(&["--target", "x86_64-unknown-linux-musl"]),
        Err("no workspace here".to_string()),
    )
    .expect_err("the root error propagates");
    assert_eq!(err, "no workspace here");
}

/// Omitting `--tag` defaults to the workspace version with the `v` a git tag carries, so a
/// local `cargo xtask package` produces exactly the name a release would — the property the
/// four consumers are checked against everywhere else in this file.
#[test]
fn an_omitted_tag_defaults_to_the_workspace_version_with_its_v() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("kndo");
    std::fs::write(&bin, b"stand-in").expect("write");
    let args: Vec<String> = [
        "--target",
        "x86_64-unknown-linux-musl",
        "--bin",
        bin.to_str().expect("utf-8"),
        "--out-dir",
        dir.path().to_str().expect("utf-8"),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let out = package::from_args(&args, Ok(root())).expect("package");
    let name = out
        .file_name()
        .and_then(|n| n.to_str())
        .expect("a file name");
    assert!(
        name.starts_with("kndo-v"),
        "the default tag carries the leading v: {name}"
    );
}

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
