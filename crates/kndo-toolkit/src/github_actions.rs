//! GitHub Actions launchers, read for the files they RUN: the `run:` steps of a
//! workflow (`.github/workflows/*.yml`) and of a composite action (`action.yml`),
//! and a JavaScript action's `main`/`pre`/`post` entries. The walk is identical
//! for every language — which runtimes hand a file to which adapter is the
//! adapter's knowledge (`node scripts/release.ts` is js-ts's to read) — so the
//! steps, and the directory each runs in, are answered here once.
//!
//! Directories are answered as the step sees them. A workflow step runs at the
//! repository root unless `working-directory` narrows it, at the step, the job,
//! or the workflow's `defaults`. A composite action's step runs in the CALLER's
//! workspace, so a file of the action's own is spelled through
//! `$GITHUB_ACTION_PATH` (or `${{ github.action_path }}`); both that and the
//! workspace variable are expanded to spellings relative to the step's
//! directory, so a command reads like any other relative reference.

use yaml_rust2::{Yaml, YamlLoader};

/// One `run:` step: its command, with GitHub's path variables expanded relative
/// to `dir`, and the project directory it runs in (`""` at the project root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStep {
    pub command: String,
    pub dir: String,
}

/// What a file is to GitHub Actions, from its path alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launcher {
    /// A workflow under `<workspace>/.github/workflows/`.
    Workflow { workspace: String },
    /// An `action.yml`/`action.yaml` manifest, composite or JavaScript, in `dir`.
    Action { dir: String },
}

pub fn launcher(path: &str) -> Option<Launcher> {
    let (dir, name) = match path.rfind('/') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    };
    if !(name.ends_with(".yml") || name.ends_with(".yaml")) {
        return None;
    }
    if name == "action.yml" || name == "action.yaml" {
        return Some(Launcher::Action {
            dir: dir.to_string(),
        });
    }
    let workspace = dir.strip_suffix(".github/workflows")?;
    let workspace = if workspace.is_empty() {
        ""
    } else {
        workspace.strip_suffix('/')?
    };
    Some(Launcher::Workflow {
        workspace: workspace.to_string(),
    })
}

/// The `run:` steps the file declares, in document order — nothing for a file
/// that is not a launcher or does not parse, and nothing for a step whose
/// directory climbs out of the project.
pub fn run_steps(path: &str, yaml: &str) -> Vec<RunStep> {
    let Some(kind) = launcher(path) else {
        return Vec::new();
    };
    let Some(doc) = document(yaml) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match kind {
        Launcher::Workflow { workspace } => {
            let vars = Vars {
                workspace: &workspace,
                action: None,
            };
            let workflow_wd = working_directory(&doc["defaults"]);
            if let Some(jobs) = doc["jobs"].as_hash() {
                for (_, job) in jobs.iter() {
                    let job_wd = working_directory(&job["defaults"]).or(workflow_wd);
                    steps(&job["steps"], &workspace, job_wd, &vars, &mut out);
                }
            }
        }
        Launcher::Action { dir } => {
            let vars = Vars {
                workspace: "",
                action: Some(&dir),
            };
            steps(&doc["runs"]["steps"], "", None, &vars, &mut out);
        }
    }
    out
}

/// A JavaScript action's entry files as its manifest spells them — relative to
/// the action's directory, the way `package.json` spells `main`.
pub fn action_entries(path: &str, yaml: &str) -> Vec<String> {
    if !matches!(launcher(path), Some(Launcher::Action { .. })) {
        return Vec::new();
    }
    let Some(doc) = document(yaml) else {
        return Vec::new();
    };
    ["main", "pre", "post"]
        .iter()
        .filter_map(|key| doc["runs"][*key].as_str().map(str::to_string))
        .collect()
}

struct Vars<'a> {
    workspace: &'a str,
    action: Option<&'a str>,
}

fn document(yaml: &str) -> Option<Yaml> {
    YamlLoader::load_from_str(yaml).ok()?.into_iter().next()
}

fn working_directory(defaults: &Yaml) -> Option<&str> {
    defaults["run"]["working-directory"].as_str()
}

fn steps(
    list: &Yaml,
    base: &str,
    inherited: Option<&str>,
    vars: &Vars<'_>,
    out: &mut Vec<RunStep>,
) {
    let Some(list) = list.as_vec() else {
        return;
    };
    for step in list {
        let Some(run) = step["run"].as_str() else {
            continue;
        };
        let dir = match step["working-directory"].as_str().or(inherited) {
            Some(wd) => match crate::join_relative(base, &expand(wd, base, vars)) {
                Some(dir) => dir,
                None => continue,
            },
            None => base.to_string(),
        };
        let command = expand(run, &dir, vars);
        out.push(RunStep { command, dir });
    }
}

/// GitHub's path variables, spelled relative to `from`.
fn expand(text: &str, from: &str, vars: &Vars<'_>) -> String {
    let mut text = text.to_string();
    let mut substitutions: Vec<(&str, &str)> = vec![
        ("${{ github.workspace }}", vars.workspace),
        ("${{github.workspace}}", vars.workspace),
        ("${GITHUB_WORKSPACE}", vars.workspace),
        ("$GITHUB_WORKSPACE", vars.workspace),
    ];
    if let Some(action) = vars.action {
        substitutions.extend([
            ("${{ github.action_path }}", action),
            ("${{github.action_path}}", action),
            ("${GITHUB_ACTION_PATH}", action),
            ("$GITHUB_ACTION_PATH", action),
        ]);
    }
    for (spelling, target) in substitutions {
        if text.contains(spelling) {
            text = text.replace(spelling, &relative(from, target));
        }
    }
    text
}

/// The path from directory `from` to directory `to`, both project-relative.
fn relative(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(command: &str, dir: &str) -> RunStep {
        RunStep {
            command: command.to_string(),
            dir: dir.to_string(),
        }
    }

    #[test]
    fn a_launcher_is_told_by_its_path() {
        assert_eq!(
            launcher(".github/workflows/ci.yml"),
            Some(Launcher::Workflow {
                workspace: String::new()
            })
        );
        assert_eq!(
            launcher("v2/.github/workflows/ci.yaml"),
            Some(Launcher::Workflow {
                workspace: "v2".into()
            })
        );
        assert_eq!(
            launcher(".github/actions/report/action.yml"),
            Some(Launcher::Action {
                dir: ".github/actions/report".into()
            })
        );
        assert_eq!(
            launcher("action.yaml"),
            Some(Launcher::Action { dir: String::new() })
        );
        assert_eq!(launcher("x.github/workflows/ci.yml"), None);
        assert_eq!(launcher(".github/workflows/README.md"), None);
        assert_eq!(launcher("docs/actions.yml"), None);
    }

    #[test]
    fn workflow_steps_run_where_their_defaults_say() {
        let yaml = "\
name: ci
on: [push]
defaults:
  run:
    working-directory: app
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: node scripts/build.mjs
      - name: Notes
        working-directory: tools
        run: |
          node render.mjs \"$VERSION\" > notes.md
          echo done
  root:
    defaults:
      run:
        working-directory: .
    steps:
      - run: node ${{ github.workspace }}/scripts/x.js
";
        assert_eq!(
            run_steps(".github/workflows/ci.yml", yaml),
            [
                step("node scripts/build.mjs", "app"),
                step(
                    "node render.mjs \"$VERSION\" > notes.md\necho done\n",
                    "tools"
                ),
                step("node ./scripts/x.js", ""),
            ]
        );
    }

    #[test]
    fn an_actions_own_file_is_spelled_through_its_path_variable() {
        let yaml = "\
name: report
runs:
  using: composite
  steps:
    - shell: bash
      run: node \"$GITHUB_ACTION_PATH/render.mjs\"
    - shell: bash
      working-directory: ${{ github.action_path }}
      run: node render.mjs
";
        assert_eq!(
            run_steps(".github/actions/report/action.yml", yaml),
            [
                step("node \".github/actions/report/render.mjs\"", ""),
                step("node render.mjs", ".github/actions/report"),
            ]
        );
        assert!(action_entries(".github/actions/report/action.yml", yaml).is_empty());
    }

    #[test]
    fn a_javascript_action_names_its_entries() {
        let yaml = "runs:\n  using: node20\n  pre: setup.js\n  main: dist/index.js\n";
        assert_eq!(
            action_entries("action.yml", yaml),
            ["dist/index.js", "setup.js"]
        );
        assert!(run_steps("action.yml", yaml).is_empty());
    }

    #[test]
    fn what_does_not_parse_or_climbs_out_declares_nothing() {
        assert!(run_steps(".github/workflows/ci.yml", "jobs: [\n").is_empty());
        assert!(
            run_steps(
                "README.md",
                "jobs:\n  a:\n    steps:\n      - run: node x.js\n"
            )
            .is_empty()
        );
        let climbing = "jobs:\n  a:\n    steps:\n      - working-directory: ../../elsewhere\n        run: node x.js\n";
        assert!(run_steps(".github/workflows/ci.yml", climbing).is_empty());
    }

    #[test]
    fn relative_paths_between_project_directories() {
        assert_eq!(relative("", ""), ".");
        assert_eq!(relative("", "v2/action"), "v2/action");
        assert_eq!(relative("v2", ""), "..");
        assert_eq!(relative("v2/tools", "v2/action"), "../action");
    }
}
