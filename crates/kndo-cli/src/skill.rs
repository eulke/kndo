//! `kndo agents install` — install the embedded agent skill package into a project.
//!
//! The skill (a SKILL.md entry point plus on-demand reference files) teaches an LLM coding
//! agent how to drive the CLI: graph navigation before edits, the check loop, and the
//! findings-resolution playbook. Content lives in `crates/kndo-cli/skill/` and is embedded
//! at compile time, so the installed skill always matches the binary that installed it.
//!
//! Layout contract: files are written to `.agents/skills/kndo/` (the harness-neutral home)
//! and `.claude/skills/kndo` becomes a relative symlink to that copy, so every agent harness
//! reads one set of files. The installed files are kndo-owned: re-running `install` after a
//! binary upgrade overwrites them — that is the update flow, mirroring how the files carry
//! no user content. Contrast with `init`'s pre-commit hook, which is user-owned and
//! therefore never overwritten.

use std::io;
use std::path::Path;

const SKILL_MD: &str = include_str!("../skill/SKILL.md");
const NAVIGATION_MD: &str = include_str!("../skill/references/navigation.md");
const FINDINGS_PLAYBOOK_MD: &str = include_str!("../skill/references/findings-playbook.md");
const OUTPUT_FORMAT_MD: &str = include_str!("../skill/references/output-format.md");
const SUPPRESSIONS_MD: &str = include_str!("../skill/references/suppressions-and-baseline.md");

pub(crate) const FILES: &[(&str, &str)] = &[
    ("SKILL.md", SKILL_MD),
    ("references/navigation.md", NAVIGATION_MD),
    ("references/findings-playbook.md", FINDINGS_PLAYBOOK_MD),
    ("references/output-format.md", OUTPUT_FORMAT_MD),
    ("references/suppressions-and-baseline.md", SUPPRESSIONS_MD),
];

pub(crate) const AGENTS_SKILL_DIR: &str = ".agents/skills/kndo";
const CLAUDE_SKILLS_DIR: &str = ".claude/skills";
const CLAUDE_LINK_NAME: &str = "kndo";
/// Relative so the project stays relocatable: `.claude/skills/kndo` → up past
/// `skills/` and `.claude/` into the sibling `.agents` tree.
const LINK_TARGET: &str = "../../.agents/skills/kndo";

/// The `{{version}}` placeholder in the authored SKILL.md frontmatter, resolved at install
/// time so the installed file names the binary version that wrote it.
fn rendered(content: &str) -> String {
    content.replace("{{version}}", env!("CARGO_PKG_VERSION"))
}

/// Install (or refresh) the skill under `root`. Returns the status lines to print, one per
/// concern, in the `topic: outcome` style the rest of the CLI's scaffolding output uses.
pub(crate) fn install_skill(root: &Path) -> Result<Vec<String>, String> {
    let skill_dir = root.join(AGENTS_SKILL_DIR);
    let mut written = 0usize;
    let mut created = false;
    for (rel, content) in FILES {
        let path = skill_dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        let desired = rendered(content);
        match std::fs::read_to_string(&path) {
            Ok(existing) if existing == desired => {}
            Ok(_) => {
                std::fs::write(&path, desired)
                    .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
                written += 1;
            }
            Err(_) => {
                std::fs::write(&path, desired)
                    .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
                written += 1;
                created = true;
            }
        }
    }

    let mut lines = vec![match (created, written) {
        (true, _) => format!("skill: installed at {AGENTS_SKILL_DIR}"),
        (false, 0) => "skill: up to date".to_string(),
        (false, n) => format!("skill: updated ({n} files)"),
    }];
    lines.push(link_skill(root)?);
    lines
        .push("skill: commit .agents/ and .claude/ so every agent session picks it up".to_string());
    Ok(lines)
}

/// Ensure `.claude/skills/kndo` resolves to the `.agents` copy. An existing correct symlink
/// is fine; anything else already occupying the name is refused rather than clobbered — the
/// same doctrine as init's hook handling. Where symlinks aren't available (unprivileged
/// Windows), fall back to a plain copy and say so.
fn link_skill(root: &Path) -> Result<String, String> {
    let claude_dir = root.join(CLAUDE_SKILLS_DIR);
    std::fs::create_dir_all(&claude_dir)
        .map_err(|e| format!("failed to create {}: {e}", claude_dir.display()))?;
    let link = claude_dir.join(CLAUDE_LINK_NAME);

    match std::fs::read_link(&link) {
        Ok(target) if target == Path::new(LINK_TARGET) => {
            return Ok(format!("link: {CLAUDE_SKILLS_DIR}/{CLAUDE_LINK_NAME} ok"));
        }
        Ok(target) => {
            return Err(format!(
                "{} is a symlink to {} — expected {LINK_TARGET}; remove it and re-run `kndo agents install`",
                link.display(),
                target.display()
            ));
        }
        // Not a symlink: either nothing is there (install) or something else is (refuse).
        Err(_) if link.exists() => {
            return Err(format!(
                "{} already exists and is not the expected symlink — refusing to overwrite it; move it aside and re-run `kndo agents install`",
                link.display()
            ));
        }
        Err(_) => {}
    }

    match symlink_dir(LINK_TARGET, &link) {
        Ok(()) => Ok(format!(
            "link: {CLAUDE_SKILLS_DIR}/{CLAUDE_LINK_NAME} -> {LINK_TARGET}"
        )),
        // No symlink support (typically unprivileged Windows): a copy still gives every
        // harness the files, at the cost of the two locations drifting until the next
        // `kndo agents install` refreshes both.
        Err(_) => {
            for (rel, content) in FILES {
                let path = link.join(rel);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
                }
                std::fs::write(&path, rendered(content))
                    .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
            }
            Ok(format!(
                "link: symlinks unavailable — copied the skill into {CLAUDE_SKILLS_DIR}/{CLAUDE_LINK_NAME} (re-run `kndo agents install` after upgrades to refresh both copies)"
            ))
        }
    }
}

#[cfg(unix)]
fn symlink_dir(target: &str, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &str, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_md_version_placeholder_resolves_to_the_crate_version() {
        let out = rendered(SKILL_MD);
        assert!(out.contains(&format!("version: {}", env!("CARGO_PKG_VERSION"))));
        assert!(!out.contains("{{"));
    }

    #[test]
    fn every_reference_file_listed_in_skill_md_is_embedded() {
        for (rel, _) in FILES.iter().skip(1) {
            assert!(
                SKILL_MD.contains(rel),
                "SKILL.md never mentions embedded file {rel}"
            );
        }
    }

    #[test]
    fn link_target_climbs_out_of_claude_skills_into_the_agents_copy() {
        // `.claude/skills/kndo` sits two directories deep, so the relative target must
        // climb exactly two levels before descending into `.agents`.
        assert_eq!(LINK_TARGET, format!("../../{AGENTS_SKILL_DIR}"));
    }
}
