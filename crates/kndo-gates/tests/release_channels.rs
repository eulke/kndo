//! **Every release channel spells the artifact the producer uploads.**
//!
//! Four things consume kndo's release artifacts and none of them can call Rust:
//! `install.sh`, `action/action.yml`, `packaging/homebrew/kndo.rb.tmpl`, and the
//! published install page (`docs/src/install.md`). Each spells the artifact's name and assumes its
//! layout independently of the others and of the one producer (`xtask package`,
//! naming by `kndo_gates::release`) — and a channel that drifts (a stripped tag
//! prefix, a triple the release never builds, an extraction that skips the
//! archive's staged directory) is invisible until a real tag is pushed and a
//! real download 404s. These tests read the consumers as text and check them
//! against the table, so a channel that drifts fails on the commit that drifts
//! it. Text matching is the point: the consumers ARE text, and a test that
//! re-derived their behavior would be a fifth copy.

use kndo_gates::release::{self, TARGETS};

const TAG: &str = "v1.2.0";

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A triple no release builds must not appear anywhere a user could be sent
/// to download it.
fn assert_no_unreleased_triples(text: &str, what: &str) {
    for foreign in ["-unknown-linux-gnu", "-pc-windows-"] {
        assert!(
            !text.contains(foreign),
            "{what} names a `{foreign}` target, which no release builds"
        );
    }
}

#[test]
fn the_release_workflow_builds_the_table_and_packages_through_xtask() {
    let yml = kndo_gates::render_release();
    let in_yml: Vec<&str> = yml
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- target: "))
        .collect();
    let expected: Vec<&str> = TARGETS.iter().map(|t| t.triple).collect();
    assert_eq!(in_yml, expected, "the matrix is the table, rendered");
    assert!(
        yml.contains("-p xtask") && yml.contains("-- package"),
        "the workflow packages through `cargo run -p xtask -- package`"
    );
    let commands: String = yml
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    for hand_rolled in ["tar -czf", "Compress-Archive"] {
        assert!(
            !commands.contains(hand_rolled),
            "the workflow builds an archive by hand (`{hand_rolled}`) — the layout has one producer"
        );
    }
    for target in TARGETS {
        assert!(
            yml.contains(&release::sha_placeholder(target.triple)),
            "the tap job substitutes no checksum for {}",
            target.triple
        );
    }
}

/// `install.sh` asks for the archive that exists, and takes the binary from
/// where it is.
#[test]
fn the_installer_matches_the_artifact() {
    let sh = read("install.sh");
    assert!(
        sh.contains(r#"ARCHIVE="kndo-${VERSION}-${TARGET}.tar.gz""#),
        "install.sh composes a different archive name than the producer's"
    );
    assert!(
        !sh.contains("${VERSION#v}"),
        "install.sh must not strip the tag's leading `v` — the uploaded asset carries it"
    );
    assert!(
        sh.contains("--strip-components=1"),
        "install.sh must strip the archive's staged directory"
    );
    for target in TARGETS {
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

#[test]
fn the_action_matches_the_artifact_and_the_cli() {
    let yml = read("action/action.yml");
    assert!(
        yml.contains(r#"asset="kndo-${KNDO_VERSION}-${target}.tar.gz""#),
        "the Action composes a different asset name than the producer's"
    );
    assert!(
        yml.contains("--strip-components=1"),
        "the Action must strip the archive's staged directory"
    );
    for target in TARGETS {
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
    // The CLI's own vocabulary: `--fail-on never` is the report-only value.
    assert!(
        yml.contains("--fail-on never") && !yml.contains("--fail-on none"),
        "the Action's report-only invocation must spell the CLI's `never`"
    );
    assert!(
        yml.contains("error | warning | info | never"),
        "the Action documents the CLI's fail-on values"
    );
}

#[test]
fn the_homebrew_template_matches_the_artifact() {
    let tmpl = read("packaging/homebrew/kndo.rb.tmpl");
    let rendered = tmpl.replace("{{VERSION}}", TAG.trim_start_matches('v'));
    for target in TARGETS {
        assert!(
            rendered.contains(&format!(
                "/releases/download/{TAG}/{}",
                release::archive_name(TAG, target.triple)
            )),
            "the formula has no URL for {}",
            target.triple
        );
        assert!(
            tmpl.contains(&release::sha_placeholder(target.triple)),
            "the formula has no checksum placeholder for {}",
            target.triple
        );
    }
    assert_no_unreleased_triples(&tmpl, "packaging/homebrew/kndo.rb.tmpl");
    assert!(
        rendered.contains(&format!(r#"bin.install "{}""#, release::BINARY)),
        "the formula installs a binary by a name the archive does not contain"
    );
}

/// The install page tells a user what to download and how, and every sentence
/// of it that names the artifact, the installer or the tap is a copy of the
/// table's answer.
#[test]
fn the_install_page_matches_the_artifact_and_the_channels() {
    let page = read("docs/src/install.md");
    for target in TARGETS {
        assert!(
            page.contains(&release::archive_name("<tag>", target.triple)),
            "the install page does not name the archive for {}",
            target.triple
        );
    }
    assert_no_unreleased_triples(&page, "docs/src/install.md");
    assert!(
        page.contains("https://raw.githubusercontent.com/eulke/kondo/main/install.sh"),
        "the install page does not point at the installer"
    );
    for var in ["KNDO_VERSION", "KNDO_INSTALL_DIR", "KNDO_BASE_URL"] {
        assert!(
            read("install.sh").contains(var) && page.contains(var),
            "`{var}` is documented on the install page and honored by install.sh, or neither"
        );
    }
    assert!(
        page.contains("brew install eulke/tap/kndo"),
        "the install page's Homebrew line names another tap than the template ships to"
    );
}

/// Every published URL names the repository that exists — one owner, one name,
/// in every file a user could be sent to.
#[test]
fn every_published_url_names_the_repository_that_exists() {
    const REPO: &str = "eulke/kondo";
    for rel in [
        "README.md",
        "NOTICE",
        "install.sh",
        "action/action.yml",
        "packaging/homebrew/kndo.rb.tmpl",
        "docs/src/install.md",
        "docs/src/ci.md",
        "docs/src/agents.md",
        "docs/src/plugins.md",
    ] {
        let text = read(rel);
        for (i, line) in text.lines().enumerate() {
            let mut rest = line;
            while let Some(at) = rest.find("github.com/") {
                let after = &rest[at + "github.com/".len()..];
                let slug: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.'))
                    .collect();
                let mut parts = slug.split('/');
                let owner = parts.next().unwrap_or("");
                let name = parts.next().unwrap_or("").trim_end_matches(".git");
                if !owner.is_empty() && !name.is_empty() {
                    assert_eq!(
                        format!("{owner}/{name}"),
                        REPO,
                        "{rel}:{}: a published URL names another repository",
                        i + 1
                    );
                }
                rest = after;
            }
        }
    }
}

/// The toolchain CI installs is the toolchain the workspace pins — the generator
/// copies the one file, and this keeps the copy honest.
#[test]
fn the_pinned_toolchain_is_what_ci_installs() {
    let pinned = kndo_gates::toolchain();
    assert!(
        pinned.starts_with(|c: char| c.is_ascii_digit()),
        "rust-toolchain.toml pins a version, not a channel name: {pinned}"
    );
    assert!(
        kndo_gates::render_ci().contains(&format!("toolchain: {pinned}")),
        "the CI workflow installs a toolchain other than the pinned {pinned}"
    );
    assert!(
        kndo_gates::render_release().contains(&format!("toolchain: {pinned}")),
        "the release workflow installs a toolchain other than the pinned {pinned}"
    );
}
