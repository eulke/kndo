//! `cargo xtask capture` — the transcripts, re-taken from the build tools
//! themselves.
//!
//! Every capture names a fixture, the commands to run in it, and the mapping
//! from those commands' answers to [`ToolClaim`]s. The mapping is the only
//! ecosystem knowledge here and it is deliberately dumb: it renames the tool's
//! own words into the contract's vocabulary and nothing else, because a capture
//! that reasoned about a manifest would be a second reader agreeing with the
//! first — which is the exact circle transcripts exist to break. Choosing which
//! questions to ask is not that circle; inventing the answers would be.
//!
//! A capture runs in a COPY of the fixture, so build residue (`target/`,
//! `.gradle/`, `*.egg-info`) never reaches the repository, and it writes
//! `transcript.json` back into the fixture directory beside `expectations.toml`.
//! What the tools answer here is what the gate replays with no toolchain
//! present, which is what lets it run in CI.

use crate::{Result, workspace_root};
use kndo_contract::adapter::DependencyScope;
use kndo_contract::evidence::RootKind;
use kndo_contract::manifest::UnitKind;
use kndo_contract::vocab::ProjectPath;
use kndo_testkit::transcript::{Producer, Reading, ToolClaim, ToolTranscript};
use serde_json::Value;
use smol_str::SmolStr;
use std::path::Path;
use std::process::Command;

/// What one command answered, and the tree it answered about.
struct Answers<'a> {
    /// One entry per command, in the order they were declared.
    said: &'a [String],
    /// The copy the tool ran in, so an absolute path in an answer can be made
    /// relative to the tree.
    here: &'a Path,
    /// Every file of the tree, project-relative — for the tools that answer
    /// with a DIRECTORY and leave the files it holds to their own documented
    /// rule.
    files: &'a [ProjectPath],
}

/// One tool's capture over one fixture.
struct Capture {
    /// `crates/<crate>/tests/fixtures/<name>` — where the transcript lands.
    fixture: &'static str,
    tool: &'static str,
    /// What tells the tool its own version.
    version: &'static [&'static str],
    /// What asks the question. More than one where a tool answers one
    /// expression at a time; the transcript records them all, so a capture is
    /// reproducible from the file alone.
    commands: &'static [&'static [&'static str]],
    /// Commands the tool needs run before it can answer — a workspace install
    /// that materialises the symlinks a resolver walks. Their output is not
    /// read; they are recorded in the transcript so a capture is reproducible
    /// from the file alone.
    prepare: &'static [&'static [&'static str]],
    /// The (file, specifier) pairs handed to a RESOLVER, which answers what it
    /// is asked and nothing else. Empty for every other shape.
    asks: &'static [(&'static str, &'static str)],
    reading: Reading,
    read: fn(Answers<'_>) -> Result<Vec<ToolClaim>>,
}

const CAPTURES: &[Capture] = &[
    Capture {
        fixture: "crates/kndo-adapter-rust/tests/fixtures/main-in-every-target",
        tool: "cargo",
        version: &["cargo", "--version"],
        commands: &[&[
            "cargo",
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ]],
        prepare: &[],
        asks: &[],
        // `--no-deps` is every target and every dependency table of every
        // package in the workspace, and nothing outside it.
        reading: Reading::Whole,
        read: cargo,
    },
    Capture {
        fixture: "crates/kndo-adapter-go/tests/fixtures/tool-ignored-paths",
        tool: "go",
        version: &["go", "version"],
        commands: &[&["go", "list", "-e", "-json", "./..."]],
        prepare: &[],
        asks: &[],
        // SAMPLED, and measured: against go 1.24.7 an explicit import of
        // `testdata/gen` or of `_scratch/old` builds and runs, while `./...`
        // lists neither — the pattern reaches less than the compiler compiles,
        // so what this command omits is not a claim that go skips it.
        reading: Reading::Sampled,
        read: go_list,
    },
    Capture {
        fixture: "crates/kndo-adapter-java/tests/fixtures/coverage-jacoco",
        tool: "maven",
        version: &["mvn", "--version"],
        commands: MAVEN,
        prepare: &[],
        asks: &[],
        // The effective model's source directories are every directory this pom
        // compiles, inherited defaults included.
        reading: Reading::Whole,
        read: maven,
    },
    Capture {
        fixture: "crates/kndo-adapter-kotlin/tests/fixtures/visibility-ladder-and-internal",
        tool: "maven",
        version: &["mvn", "--version"],
        commands: MAVEN,
        prepare: &[],
        asks: &[],
        reading: Reading::Whole,
        read: maven,
    },
    Capture {
        fixture: "crates/kndo-adapter-ts/tests/fixtures/conditional-subpaths",
        tool: "node",
        version: &["node", "--version"],
        commands: &[&["node", "-e", RESOLVER]],
        // The symlinks a workspace resolver walks are npm's to create; without
        // them node answers about a tree that is not this one.
        prepare: &[&["npm", "install", "--offline", "--no-audit", "--no-fund"]],
        asks: &[
            ("packages/app/main.js", "@demo/lib"),
            ("packages/app/main.js", "@demo/lib/client"),
            ("packages/app/main.js", "@demo/lib/tools/probe"),
            ("packages/app/main.js", "@demo/lib/internal/secret"),
            ("packages/lib/src/index.js", "./client.js"),
        ],
        // A resolver answers the specifiers it is handed and knows nothing of
        // the ones it is not.
        reading: Reading::Sampled,
        read: node_resolve,
    },
    Capture {
        fixture: "crates/kndo-adapter-python/tests/fixtures/pep508-spellings",
        tool: "packaging",
        version: &[
            "python3",
            "-c",
            "import packaging; print('packaging', packaging.__version__)",
        ],
        commands: &[&["python3", "-c", REQUIREMENTS]],
        prepare: &[],
        asks: &[],
        // Every requirement the runtime table declares, and only that table:
        // extras and PEP 735 groups are other tables, under other scopes.
        reading: Reading::Whole,
        read: packaging,
    },
];

/// Maven answers one expression at a time, and its EFFECTIVE model is the pom
/// with the super-pom's and any parent's contribution already applied — the
/// half a pom's own text never states.
const MAVEN: &[&[&str]] = &[
    &[
        "mvn",
        "-o",
        "-q",
        "help:evaluate",
        "-Dexpression=project.build.sourceDirectory",
        "-DforceStdout",
    ],
    &[
        "mvn",
        "-o",
        "-q",
        "help:evaluate",
        "-Dexpression=project.build.testSourceDirectory",
        "-DforceStdout",
    ],
    &[
        "mvn",
        "-o",
        "-q",
        "help:evaluate",
        "-Dexpression=project.modules",
        "-DforceStdout",
    ],
];

/// Node's own resolver, asked one specifier at a time from the file that writes
/// it. `createRequire(file).resolve` runs the real algorithm — `exports`
/// subpaths, conditions, the `null` target — so what comes back is not a
/// reading of `package.json`, it is the runtime's answer. It answers under the
/// `require` condition, which is one of the branches the adapter offers; a
/// claim that names one branch is satisfied by the set that contains it.
const RESOLVER: &str = r#"
const { createRequire } = require('node:module');
const path = require('node:path');
const asks = JSON.parse(process.env.KNDO_ASKS);
const out = [];
for (const [from, specifier] of asks) {
  try {
    out.push({ from, specifier, to: createRequire(path.resolve(from)).resolve(specifier) });
  } catch (e) {
    out.push({ from, specifier, refused: String(e.code || e.message) });
  }
}
console.log(JSON.stringify(out));
"#;

/// `packaging.requirements.Requirement` — the PEP 508 parser pip and setuptools
/// both call — asked which distribution each requirement string names. The
/// requirement STRINGS come from `tomllib`, the interpreter's own TOML reader,
/// because reading TOML is not the question: which name a requirement spells
/// is, and that answer is `packaging`'s alone.
///
/// Not the wheel metadata a build backend writes, deliberately: that file
/// renders a name (`A.B_c-D` becomes `A.B-c-D`), and a rendering is a third
/// spelling neither the manifest nor the reader uses.
const REQUIREMENTS: &str = r#"
import json, tomllib
from packaging.requirements import Requirement
with open("pyproject.toml", "rb") as f:
    doc = tomllib.load(f)
out = [Requirement(spec).name for spec in doc["project"]["dependencies"]]
print(json.dumps(out))
"#;

pub fn run(args: &[String]) -> Result<()> {
    let only = crate::flag(args, "--only");
    let root = workspace_root();
    let mut taken = 0;
    for capture in CAPTURES {
        if only
            .as_deref()
            .is_some_and(|o| !capture.fixture.contains(o))
        {
            continue;
        }
        let fixture = root.join(capture.fixture);
        if !fixture.join("project").is_dir() {
            return Err(format!("no such fixture: {}", capture.fixture));
        }
        let out = take(capture, &fixture)?;
        let text = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())? + "\n";
        std::fs::write(fixture.join("transcript.json"), text).map_err(|e| e.to_string())?;
        println!(
            "{}: {} claims from {}",
            capture.fixture,
            out.says.len(),
            out.producer.version
        );
        taken += 1;
    }
    if taken == 0 {
        return Err("no capture matched".to_string());
    }
    Ok(())
}

fn take(capture: &Capture, fixture: &Path) -> Result<ToolTranscript> {
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let project = scratch.path().join("project");
    copy_tree(&fixture.join("project"), &project)?;
    let asks = serde_json::to_string(capture.asks).map_err(|e| e.to_string())?;
    // A tool that prints a paragraph about itself is asked for its first line:
    // the version, not this machine's java home and locale.
    let version = run_in(&project, capture.version, &asks)?
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    for argv in capture.prepare {
        run_in(&project, argv, &asks)?;
    }
    let said: Vec<String> = capture
        .commands
        .iter()
        .map(|argv| run_in(&project, argv, &asks))
        .collect::<Result<Vec<String>>>()?;
    let here = std::fs::canonicalize(&project).map_err(|e| e.to_string())?;
    let files = tree_files(&here);
    let mut says = (capture.read)(Answers {
        said: &said,
        here: &here,
        files: &files,
    })?;
    says.sort();
    says.dedup();
    if says.is_empty() {
        return Err(format!("{} answered nothing", capture.tool));
    }
    Ok(ToolTranscript {
        producer: Producer {
            tool: capture.tool.to_string(),
            version,
            command: capture
                .prepare
                .iter()
                .chain(capture.commands)
                .map(|argv| argv.join(" "))
                .collect::<Vec<String>>()
                .join(" && "),
        },
        reading: capture.reading,
        says,
    })
}

fn run_in(dir: &Path, argv: &[&str], asks: &str) -> Result<String> {
    let out = Command::new(argv[0])
        .args(&argv[1..])
        .current_dir(dir)
        // A capture must answer about the tree, never about this machine's
        // caches or workspaces: a stray `go.work` or `CARGO_TARGET_DIR` two
        // levels up would silently change what the tool enumerates.
        .env("GOWORK", "off")
        .env("GOFLAGS", "-mod=mod")
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .env("KNDO_ASKS", asks)
        .output()
        .map_err(|e| format!("{}: {e}", argv[0]))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.trim().is_empty() {
        return Err(format!(
            "{} said nothing:\n{}",
            argv.join(" "),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(text)
}

/// A path the tool answered in, relative to the tree it ran on.
fn relative(path: &str, here: &Path) -> Option<ProjectPath> {
    let rel = Path::new(path.trim()).strip_prefix(here).ok()?;
    Some(ProjectPath::new(rel.to_str()?.replace('\\', "/")))
}

fn tree_files(root: &Path) -> Vec<ProjectPath> {
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<ProjectPath>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out);
        } else if let Some(rel) = path.to_str().and_then(|p| relative(p, root)) {
            out.push(rel);
        }
    }
}

/// `cargo metadata`: every target cargo builds, with the file it enters it
/// through, plus every dependency table of every package.
fn cargo(answer: Answers<'_>) -> Result<Vec<ToolClaim>> {
    let doc: Value = serde_json::from_str(&answer.said[0]).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for package in doc["packages"].as_array().into_iter().flatten() {
        let Some(manifest) = package["manifest_path"]
            .as_str()
            .and_then(|p| relative(p, answer.here))
        else {
            continue;
        };
        for target in package["targets"].as_array().into_iter().flatten() {
            let Some(file) = target["src_path"]
                .as_str()
                .and_then(|p| relative(p, answer.here))
            else {
                continue;
            };
            let kinds = target["kind"].as_array().into_iter().flatten();
            for kind in kinds.filter_map(|k| k.as_str()).filter_map(cargo_kind) {
                out.push(ToolClaim::Enters {
                    file: file.clone(),
                    kind,
                });
            }
        }
        for dep in package["dependencies"].as_array().into_iter().flatten() {
            let Some(name) = dep["name"].as_str() else {
                continue;
            };
            let scope = match (dep["optional"].as_bool(), dep["kind"].as_str()) {
                (Some(true), _) => DependencyScope::Optional,
                (_, Some("dev")) => DependencyScope::Dev,
                (_, Some("build")) => DependencyScope::Build,
                _ => DependencyScope::Prod,
            };
            out.push(ToolClaim::Declares {
                manifest: manifest.clone(),
                name: SmolStr::new(name),
                scope: Some(scope),
            });
        }
    }
    Ok(out)
}

/// Cargo's target kinds, in the engine's vocabulary. The compiled shapes of a
/// library are one unit kind — `rlib` and `cdylib` are two ways to emit the
/// same crate, not two targets.
fn cargo_kind(kind: &str) -> Option<UnitKind> {
    Some(match kind {
        "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro" => UnitKind::Library,
        "bin" => UnitKind::Executable,
        "test" => UnitKind::Test,
        "bench" => UnitKind::Bench,
        "example" => UnitKind::Example,
        "custom-build" => UnitKind::Tooling,
        _ => return None,
    })
}

/// `go list -e -json ./...`: every file the go tool compiles into a package,
/// and which of them are its tests.
fn go_list(answer: Answers<'_>) -> Result<Vec<ToolClaim>> {
    let mut out = Vec::new();
    for package in json_stream(&answer.said[0])? {
        let Some(dir) = package["Dir"]
            .as_str()
            .and_then(|d| relative(d, answer.here))
        else {
            continue;
        };
        let under = |file: &str| match dir.as_str().is_empty() {
            true => ProjectPath::new(file),
            false => ProjectPath::new(format!("{}/{file}", dir.as_str())),
        };
        for (field, kind) in [
            ("GoFiles", RootKind::Production),
            ("CgoFiles", RootKind::Production),
            ("TestGoFiles", RootKind::Test),
            ("XTestGoFiles", RootKind::Test),
        ] {
            for file in package[field].as_array().into_iter().flatten() {
                let Some(file) = file.as_str() else { continue };
                out.push(ToolClaim::Compiles {
                    file: under(file),
                    kind,
                });
            }
        }
    }
    Ok(out)
}

/// Maven's two source directories, to the files under them. The directories are
/// maven's answer; that what it compiles under them is `**/*.java` (and, where
/// the kotlin plugin is configured, `**/*.kt` beside them) is the compiler
/// plugin's own documented default and the one inference this capture makes.
fn maven(answer: Answers<'_>) -> Result<Vec<ToolClaim>> {
    let mut out = Vec::new();
    for (said, kind) in [
        (&answer.said[0], RootKind::Production),
        (&answer.said[1], RootKind::Test),
    ] {
        let Some(dir) = relative(said, answer.here) else {
            continue;
        };
        let prefix = format!("{}/", dir.as_str());
        for file in answer.files.iter().filter(|f| {
            f.as_str().starts_with(&prefix)
                && (f.as_str().ends_with(".java") || f.as_str().ends_with(".kt"))
        }) {
            out.push(ToolClaim::Compiles {
                file: file.clone(),
                kind,
            });
        }
    }
    for module in tags(&answer.said[2], "module") {
        out.push(ToolClaim::Aggregates {
            manifest: ProjectPath::new("pom.xml"),
            member: ProjectPath::new(format!("{module}/pom.xml")),
        });
    }
    Ok(out)
}

/// The text of every `<tag>…</tag>` in one of maven's XML dumps.
fn tags<'t>(text: &'t str, tag: &str) -> Vec<&'t str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        rest = &rest[start + open.len()..];
        let Some(end) = rest.find(&close) else { break };
        out.push(rest[..end].trim());
        rest = &rest[end + close.len()..];
    }
    out
}

/// Node's ESM resolver, one specifier at a time. A refusal is an answer too —
/// `ERR_PACKAGE_PATH_NOT_EXPORTED` is the manifest saying no — but it names no
/// file, so it states no claim of a kind this vocabulary has.
fn node_resolve(answer: Answers<'_>) -> Result<Vec<ToolClaim>> {
    let doc: Value = serde_json::from_str(&answer.said[0]).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in doc.as_array().into_iter().flatten() {
        let (Some(from), Some(specifier)) = (row["from"].as_str(), row["specifier"].as_str())
        else {
            continue;
        };
        let (from, specifier) = (ProjectPath::new(from), SmolStr::new(specifier));
        // `ERR_PACKAGE_PATH_NOT_EXPORTED` is the manifest refusing the
        // specifier, which is a claim; every other failure is node not finding
        // a file, which states nothing about the manifest.
        match (row["to"].as_str(), row["refused"].as_str()) {
            (Some(to), _) => {
                if let Some(to) = relative(to, answer.here) {
                    out.push(ToolClaim::Resolves {
                        from,
                        specifier,
                        to,
                    });
                }
            }
            (None, Some("ERR_PACKAGE_PATH_NOT_EXPORTED")) => {
                out.push(ToolClaim::Refuses { from, specifier })
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The distribution name `packaging` read out of each requirement in the
/// runtime table — the one part of a requirement string a manifest reader must
/// agree on, whatever specifiers, extras brackets and markers surround it.
fn packaging(answer: Answers<'_>) -> Result<Vec<ToolClaim>> {
    let doc: Value = serde_json::from_str(&answer.said[0]).map_err(|e| e.to_string())?;
    Ok(doc
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|name| name.as_str())
        .map(|name| ToolClaim::Declares {
            manifest: ProjectPath::new("pyproject.toml"),
            name: SmolStr::new(name),
            scope: Some(DependencyScope::Prod),
        })
        .collect())
}

/// `go list` writes one JSON object per package with no array around them.
fn json_stream(text: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    for value in stream.by_ref() {
        out.push(value.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
