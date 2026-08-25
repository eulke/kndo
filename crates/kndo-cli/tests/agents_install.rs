//! `kndo agents install` contract: the embedded skill lands in `.agents/skills/kndo/`,
//! `.claude/skills/kndo` becomes a relative symlink to it, re-runs are idempotent, kndo-owned
//! files are restored on drift, and anything else squatting on the `.claude` name is refused
//! rather than clobbered. Plus drift guards keyed to the repo itself: the skill must keep
//! naming every finding category in `docs/src/rules.md` and every navigation verb in the
//! usage text, so adding one without teaching the skill fails here.

use std::path::{Path, PathBuf};
use std::process::Command;

fn kndo() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kndo"))
}

const SKILL_FILES: [&str; 5] = [
    "SKILL.md",
    "references/navigation.md",
    "references/findings-playbook.md",
    "references/output-format.md",
    "references/suppressions-and-baseline.md",
];

fn install(dir: &Path) -> std::process::Output {
    kndo()
        .args(["agents", "install"])
        .current_dir(dir)
        .output()
        .expect("running kndo agents install")
}

#[test]
fn install_writes_the_package_and_the_claude_symlink() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = install(dir.path());
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("skill: installed at .agents/skills/kndo"),
        "{stdout}"
    );

    for rel in SKILL_FILES {
        assert!(
            dir.path().join(".agents/skills/kndo").join(rel).is_file(),
            "missing installed file {rel}"
        );
    }

    #[cfg(unix)]
    {
        let link = dir.path().join(".claude/skills/kndo");
        let target = std::fs::read_link(&link).expect(".claude/skills/kndo must be a symlink");
        assert_eq!(target, PathBuf::from("../../.agents/skills/kndo"));
        // The relative target must actually resolve: the linked SKILL.md is readable.
        assert!(link.join("SKILL.md").is_file());
    }
}

#[test]
fn installed_skill_md_names_the_binary_version_with_no_placeholder_left() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(install(dir.path()).status.success());
    let skill_md = std::fs::read_to_string(dir.path().join(".agents/skills/kndo/SKILL.md"))
        .expect("reading installed SKILL.md");
    assert!(skill_md.contains(&format!("version: {}", env!("CARGO_PKG_VERSION"))));
    assert!(
        !skill_md.contains("{{"),
        "unresolved placeholder in installed SKILL.md"
    );
    assert!(
        skill_md.starts_with("---\nname: kndo\n"),
        "frontmatter must lead with the name"
    );
    assert!(
        skill_md.contains("description:"),
        "frontmatter must carry a description"
    );
}

#[test]
fn rerun_is_idempotent_and_reports_up_to_date() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(install(dir.path()).status.success());
    let before = std::fs::read_to_string(dir.path().join(".agents/skills/kndo/SKILL.md")).unwrap();

    let output = install(dir.path());
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("skill: up to date"), "{stdout}");
    assert!(stdout.contains("link: .claude/skills/kndo ok"), "{stdout}");

    let after = std::fs::read_to_string(dir.path().join(".agents/skills/kndo/SKILL.md")).unwrap();
    assert_eq!(before, after);
}

#[test]
fn drifted_files_are_restored_because_the_package_is_kndo_owned() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(install(dir.path()).status.success());
    let playbook = dir
        .path()
        .join(".agents/skills/kndo/references/findings-playbook.md");
    std::fs::write(&playbook, "user edit\n").expect("corrupting the installed file");

    let output = install(dir.path());
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("skill: updated (1 files)"), "{stdout}");
    let restored = std::fs::read_to_string(&playbook).unwrap();
    assert!(restored.contains("# Findings resolution playbook"));
}

#[test]
fn a_foreign_claude_entry_is_refused_but_the_agents_copy_still_lands() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A real directory (say, a hand-written skill) already owns the name.
    std::fs::create_dir_all(dir.path().join(".claude/skills/kndo")).unwrap();

    let output = install(dir.path());
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("refusing to overwrite"), "{stderr}");
    // Files are written before linking, so the harness-neutral copy is complete even when
    // the link step refuses — a re-run after moving the obstruction only needs to link.
    assert!(dir.path().join(".agents/skills/kndo/SKILL.md").is_file());
}

#[test]
fn agents_without_a_known_action_is_a_usage_error() {
    for args in [
        vec!["agents"],
        vec!["agents", "remove"],
        vec!["agents", "install", "x"],
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let output = kndo()
            .args(&args)
            .current_dir(dir.path())
            .output()
            .expect("running kndo agents");
        assert_eq!(output.status.code(), Some(2), "args: {args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("usage: kndo agents install"),
            "args: {args:?}"
        );
        assert!(!dir.path().join(".agents").exists(), "args: {args:?}");
    }
}

#[test]
fn init_advises_about_the_skill_without_installing_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = kndo()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .expect("running kndo init");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("agent skill: not installed (install with `kndo agents install`)"),
        "{stdout}"
    );
    assert!(!dir.path().join(".agents").exists());

    assert!(install(dir.path()).status.success());
    let output = kndo()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .expect("re-running kndo init");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("agent skill: installed (.agents/skills/kndo)"),
        "{stdout}"
    );
}

// --- Drift guards: the embedded skill tracks the repo it ships from ---------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn embedded(rel: &str) -> String {
    // Read via the crate's own source tree: identical to what include_str! embedded.
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("skill")
            .join(rel),
    )
    .unwrap_or_else(|e| panic!("reading skill/{rel}: {e}"))
}

#[test]
fn playbook_covers_every_category_documented_in_rules_md() {
    let rules =
        std::fs::read_to_string(repo_root().join("docs/src/rules.md")).expect("docs/src/rules.md");
    let playbook = embedded("references/findings-playbook.md");
    for line in rules.lines() {
        let Some(heading) = line.strip_prefix("## ") else {
            continue;
        };
        if heading == "The category registry" {
            continue;
        }
        assert!(
            playbook.contains(&format!("### {heading}")),
            "docs/src/rules.md documents category `{heading}` but the skill playbook has no `### {heading}` recipe"
        );
    }
}

#[test]
fn navigation_reference_covers_every_verb_in_the_usage_text() {
    let output = kndo().arg("--help").output().expect("kndo --help");
    let usage = String::from_utf8_lossy(&output.stdout).to_string();
    let verbs_line = usage
        .lines()
        .find(|l| l.contains("graph navigation verbs"))
        .expect("usage must list the navigation verbs");
    let nav = embedded("references/navigation.md");
    let skill_md = embedded("SKILL.md");
    for verb in verbs_line.split_whitespace().next().unwrap().split('|') {
        assert!(
            nav.contains(&format!("kndo {verb}")),
            "usage lists verb `{verb}` but references/navigation.md never shows it"
        );
        assert!(
            skill_md.contains(&format!("`kndo {verb}")),
            "usage lists verb `{verb}` but SKILL.md's verb table never shows it"
        );
    }
}
