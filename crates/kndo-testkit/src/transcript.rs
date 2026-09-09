//! What a BUILD TOOL says a tree means, captured from the tool itself.
//!
//! Every other manifest test is one hand grading another: a manifest string I
//! wrote, and the answer I expected, authored in the same sitting from the same
//! reading of the same spec. A transcript breaks that circle. `cargo metadata`,
//! `go list`, `help:effective-pom`, node's own resolver and setuptools' PEP 517
//! hook each answer questions [`ManifestEvidence`] also answers; the answers are
//! captured to a file with the command and version that produced them, and the
//! adapter is graded against them. Two independent derivations agreeing is
//! evidence; one derivation agreeing with itself is not.
//!
//! A transcript sits in a fixture directory beside `expectations.toml`, and the
//! pair divides the tree cleanly: `expectations.toml` claims what the RUN must
//! report, `transcript.json` claims what the BUILD says the tree is. The tool
//! ran on that fixture's `project/`, so the capture is re-runnable —
//! `cargo xtask capture` re-runs it — and the checked-in answers replay with no
//! toolchain present, which is what lets the gate run in CI.
//!
//! ```json
//! {
//!   "producer": {
//!     "tool": "cargo",
//!     "version": "cargo 1.98.0",
//!     "command": "cargo metadata --no-deps --format-version 1 --offline"
//!   },
//!   "reading": "whole",
//!   "says": [
//!     { "enters": { "file": "src/lib.rs", "kind": "library" } },
//!     { "declares": { "manifest": "Cargo.toml", "name": "serde", "scope": "prod" } }
//!   ]
//! }
//! ```
//!
//! Only the questions BOTH sides key by a path or by a manifest's own table are
//! claims here, because those are the ones the two derivations cannot spell
//! differently. A unit's NAME is not one of them — cargo calls a bench
//! `throughput` where the engine calls it `bench:throughput`, so grading names
//! would grade our renaming rather than our reading. Publication and package
//! identity stay with the hand-written manifest tests for the same reason.

use kndo_contract::adapter::{DependencyScope, ResolveContext, Resolution, SourceFile};
use kndo_contract::evidence::RootKind;
use kndo_contract::manifest::{ManifestEvidence, ManifestSink, UnitKind};
use kndo_contract::plugin::Plugin;
use kndo_contract::vocab::Confidence;
use kndo_contract::vocab::ProjectPath;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One command's worth of answers from one build tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolTranscript {
    pub producer: Producer,
    pub reading: Reading,
    pub says: Vec<ToolClaim>,
}

/// Who answered, and how to ask again. The command is verbatim so a capture is
/// reproducible without reading the code that captured it, and the version is
/// what the tool printed about itself — a transcript whose answers stop
/// matching after a toolchain bump must be re-captured, not edited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Producer {
    pub tool: String,
    pub version: String,
    pub command: String,
}

/// How much of the answer the command enumerated — a property of the command,
/// which is why it is stated once per transcript and a tool answering two ways
/// gets two transcripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reading {
    /// Everything of these kinds the tree contains. A claim the adapter makes
    /// that the tool did not is then an invention, and graded as one.
    Whole,
    /// Some of it — a resolver answers the specifiers it was handed and knows
    /// nothing of the ones it was not. Extra claims from the adapter are not
    /// contradictions, so only agreement is graded.
    Sampled,
}

/// One thing a build tool said about the tree it ran on, in the vocabulary
/// [`ManifestEvidence`] answers in. Every claim is keyed by a path or by a
/// manifest's own spelling: the coordinates two independent derivations of the
/// same tree cannot disagree about by accident.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolClaim {
    /// The build compiles this file, and this is the color it compiles it
    /// under — the claim that grades source roots, excludes, inherited source
    /// directories and every ignore rule at once. [`RootKind`] rather than
    /// [`UnitKind`] because a build's target granularity is its own: go links
    /// an executable per `main` PACKAGE where its unit is the module, and
    /// demanding the two agree on a target would grade our model of go rather
    /// than our reading of it. The color is the fact both sides really state,
    /// and the one every analysis downstream reads.
    Compiles { file: ProjectPath, kind: RootKind },
    /// The build ENTERS a target of this kind through this file. [`UnitKind`]
    /// here, because an entry names a target and nothing else does.
    Enters { file: ProjectPath, kind: UnitKind },
    /// This manifest declares a dependency the manifest spells `name`.
    Declares {
        manifest: ProjectPath,
        name: SmolStr,
        scope: Option<DependencyScope>,
    },
    /// This manifest aggregates that one: a workspace member, a reactor module.
    Aggregates {
        manifest: ProjectPath,
        member: ProjectPath,
    },
    /// The build resolves `specifier`, written in `from`, to `to`.
    Resolves {
        from: ProjectPath,
        specifier: SmolStr,
        to: ProjectPath,
    },
    /// The build REFUSES `specifier` written in `from` — npm's `null` subpath
    /// target and every other "the manifest does not hand this out". A refusal
    /// is an answer, and the one an alias table states most deliberately; our
    /// side says it as [`Resolution::Unresolved`], which for a specifier the
    /// project's own manifest maps means exactly this.
    Refuses { from: ProjectPath, specifier: SmolStr },
}

impl ToolClaim {
    /// Which question this claim answers — the variant itself, which is the
    /// answer already: two claims of one kind are comparable, two of different
    /// kinds never contradict, and that is what lets a `Whole` transcript grade
    /// only the kinds it actually carries. A parallel enum of kind names would
    /// be the same fact written twice, free to drift.
    fn question(&self) -> std::mem::Discriminant<ToolClaim> {
        std::mem::discriminant(self)
    }
}

/// Where an adapter and the tool that owns the manifest disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disagreement {
    /// The tool said it; the adapter does not.
    Contradicted(ToolClaim),
    /// The adapter says it; the tool enumerated this kind and did not.
    Invented(ToolClaim),
}

impl std::fmt::Display for Disagreement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Disagreement::Contradicted(c) => write!(f, "the tool says {c:?}, the adapter does not"),
            Disagreement::Invented(c) => write!(f, "the adapter says {c:?}, the tool does not"),
        }
    }
}

impl ToolTranscript {
    /// Read a transcript from a fixture directory's `transcript.json`.
    pub fn read(path: &Path) -> ToolTranscript {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// Grade the project's reading of `tree` against what the tool said about
    /// it. The whole plugin set, not one plugin, because a tree with a pom and
    /// a `build.gradle` in it is read by two and the engine reads it with all
    /// of them. Empty is agreement.
    pub fn check(&self, plugins: &[Box<dyn Plugin>], tree: &Tree) -> Vec<Disagreement> {
        let ours = tree.as_the_project_reads_it(plugins);
        let mut out: Vec<Disagreement> = self
            .says
            .iter()
            .filter(|claim| !ours.contains(claim) && !tree.answers(plugins, claim))
            .cloned()
            .map(Disagreement::Contradicted)
            .collect();
        if self.reading == Reading::Whole {
            let asked: Vec<std::mem::Discriminant<ToolClaim>> =
                self.says.iter().map(ToolClaim::question).collect();
            let said: BTreeSet<&ToolClaim> = self.says.iter().collect();
            out.extend(
                ours.iter()
                    .filter(|claim| asked.contains(&claim.question()) && !said.contains(claim))
                    .cloned()
                    .map(Disagreement::Invented),
            );
        }
        out
    }
}

/// A project tree read from disk, as the engine hands it to an adapter: paths
/// relative to the root, contents for the manifests among them.
pub struct Tree {
    files: BTreeMap<ProjectPath, Vec<u8>>,
}

impl Tree {
    /// Every file under `root`, recursively. The tool ran on this same
    /// directory, which is what makes the two readings comparable.
    pub fn read(root: &Path) -> Tree {
        let mut files = BTreeMap::new();
        walk(root, root, &mut files);
        Tree { files }
    }

    fn paths(&self) -> BTreeSet<ProjectPath> {
        self.files.keys().cloned().collect()
    }

    /// Every manifest in the tree, read by the plugin that claims it.
    fn manifests(&self, plugins: &[Box<dyn Plugin>]) -> Vec<(ProjectPath, ManifestEvidence)> {
        let known = self.paths();
        let cx = ResolveContext::new(&known);
        let mut out = Vec::new();
        for plugin in plugins {
            let Some(set) = globs(plugin.spec().manifests()) else {
                continue;
            };
            for (path, content) in self.files.iter().filter(|(p, _)| set.is_match(p.as_str())) {
                let mut sink = ManifestSink::new();
                plugin.extract_manifest(
                    &SourceFile {
                        path,
                        content,
                        region: None,
                    },
                    &cx,
                    &mut sink,
                );
                out.push((path.clone(), sink.finish()));
            }
        }
        out
    }

    /// Everything the plugins say about this tree, in the tool's vocabulary —
    /// the enumerable kinds only: a resolution is a question that must be
    /// asked, so it is answered by [`Tree::answers`] instead.
    fn as_the_project_reads_it(&self, plugins: &[Box<dyn Plugin>]) -> BTreeSet<ToolClaim> {
        let read = self.manifests(plugins);
        let mut out = BTreeSet::new();
        for (manifest, evidence) in &read {
            for dep in &evidence.dependencies {
                out.insert(ToolClaim::Declares {
                    manifest: manifest.clone(),
                    name: dep.name.clone(),
                    scope: dep.scope,
                });
            }
            for member in &evidence.members {
                out.insert(ToolClaim::Aggregates {
                    manifest: manifest.clone(),
                    member: member.clone(),
                });
            }
            for unit in &evidence.units {
                for entry in &unit.entries {
                    out.insert(ToolClaim::Enters {
                        file: entry.clone(),
                        kind: unit.kind,
                    });
                }
            }
        }
        // A file belongs to the unit whose root is the LONGEST prefix of its
        // path — `Unit::roots`' own rule, and the only place outside the engine
        // that has to apply it, because the tool answers per file and the
        // manifest answers per directory. Its color is that unit's, unless the
        // language declares a role for the path — `**/*_test.go` is a test file
        // in a module whose unit is a library, and only the spec says so.
        let units: Vec<(&ProjectPath, &kndo_contract::manifest::Unit)> = read
            .iter()
            .flat_map(|(m, e)| e.units.iter().map(move |u| (m, u)))
            .collect();
        // Only a file some language CLAIMS and none of them IGNORES is a file
        // the build compiles — both are spec data, the same two globs discovery
        // applies, so a `go.mod` (claimed by nobody as source) and a
        // `vendor/` copy (ignored by the tool that owns it) are not units'
        // files however deep under a root they sit.
        let claimed: Vec<globset::GlobSet> =
            plugins.iter().filter_map(|p| globs(p.spec().claims())).collect();
        let ignored: Vec<globset::GlobSet> =
            plugins.iter().filter_map(|p| globs(p.spec().ignores())).collect();
        for file in self.files.keys() {
            let path = file.as_str();
            if !claimed.iter().any(|set| set.is_match(path))
                || ignored.iter().any(|set| set.is_match(path))
            {
                continue;
            }
            let mut best: Option<(usize, RootKind)> = None;
            for (manifest, unit) in &units {
                let Some(len) = compiles(manifest, unit, file) else {
                    continue;
                };
                if best.is_none_or(|(had, _)| len > had) {
                    best = Some((len, unit.kind.color()));
                }
            }
            let Some((_, kind)) = best else { continue };
            let declared = plugins
                .iter()
                .flat_map(|p| p.spec().roles_for(file.as_str()))
                .find(|(_, confidence)| *confidence == Confidence::Certain);
            out.insert(ToolClaim::Compiles {
                file: file.clone(),
                kind: declared.map_or(kind, |(kind, _)| kind),
            });
        }
        out
    }

    /// Do the plugins make this claim, for the kinds that must be ASKED rather
    /// than enumerated? The plugin that CLAIMS the file the specifier is
    /// written in is the one that resolves it, exactly as in a run.
    fn answers(&self, plugins: &[Box<dyn Plugin>], claim: &ToolClaim) -> bool {
        let (from, specifier, to) = match claim {
            ToolClaim::Resolves {
                from,
                specifier,
                to,
            } => (from, specifier, Some(to)),
            ToolClaim::Refuses { from, specifier } => (from, specifier, None),
            _ => return false,
        };
        // The packages the manifests declare, exactly as the engine collects
        // them from the same sink — without them a bare specifier names
        // nothing, and the tool's answer would be graded against a project
        // half-read.
        let known = self.paths();
        let packages: BTreeMap<SmolStr, kndo_contract::adapter::PackageEntry> = self
            .manifests(plugins)
            .into_iter()
            .flat_map(|(_, evidence)| evidence.packages)
            .map(|package| (package.name.clone(), package))
            .collect();
        let cx = ResolveContext::with_packages(&known, &packages);
        let mut asked = plugins
            .iter()
            .filter(|p| globs(p.spec().claims()).is_some_and(|g| g.is_match(from.as_str())))
            .map(|p| p.resolve(from, specifier, &cx))
            .peekable();
        match to {
            Some(to) => asked.any(|r| match r {
                Resolution::File(path) => path == *to,
                Resolution::Files(paths) => paths.contains(to),
                _ => false,
            }),
            // A refusal must be unanimous AND asked: nobody claimed the file is
            // silence, not agreement.
            None => {
                asked.peek().is_some() && asked.all(|r| matches!(r, Resolution::Unresolved))
            }
        }
    }
}

/// One glob set, or `None` where nothing was declared — an extension that
/// claims no path and an unparseable glob both mean "matches nothing", and a
/// set that matched everything would put every file through every reader.
fn globs(patterns: &[SmolStr]) -> Option<globset::GlobSet> {
    if patterns.is_empty() {
        return None;
    }
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        // Path-shaped, like every other project-path glob in the contract: `*`
        // stays inside one segment and only `**` crosses one.
        if let Ok(glob) = globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
        {
            builder.add(glob);
        }
    }
    builder.build().ok()
}

/// How long the root under which `unit` compiles `file` is, or `None` where it
/// does not compile it. Length rather than a bool because the longest root
/// wins, and a non-recursive root takes only its own directory's files.
fn compiles(
    manifest: &ProjectPath,
    unit: &kndo_contract::manifest::Unit,
    file: &ProjectPath,
) -> Option<usize> {
    if unit
        .excludes
        .iter()
        .any(|e| under(file.as_str(), &joined(manifest, e)).is_some())
    {
        return None;
    }
    // No roots is the manifest's own directory — `ManifestSink::unit`'s rule,
    // so an adapter never spells a path it did not read.
    if unit.roots.is_empty() {
        let dir = joined(manifest, "");
        return under(file.as_str(), &dir).map(|_| dir.len());
    }
    unit.roots
        .iter()
        .filter_map(|root| {
            let dir = joined(manifest, &root.path);
            let rest = under(file.as_str(), &dir)?;
            (root.recursive || !rest.contains('/')).then_some(dir.len())
        })
        .max()
}

/// A path a manifest states, joined to the directory that manifest sits in —
/// the manifest's own directory where it states nothing.
fn joined(manifest: &ProjectPath, path: &str) -> String {
    let dir = match manifest.as_str().rfind('/') {
        Some(i) => &manifest.as_str()[..i],
        None => "",
    };
    match (dir.is_empty(), path.is_empty()) {
        (_, true) => dir.to_string(),
        (true, false) => path.to_string(),
        (false, false) => format!("{dir}/{path}"),
    }
}

/// What follows `dir` in `path`, or `None` where `path` is not under it. An
/// empty `dir` is the project root and everything is under it.
fn under<'p>(path: &'p str, dir: &str) -> Option<&'p str> {
    if dir.is_empty() {
        return Some(path);
    }
    path.strip_prefix(dir)?.strip_prefix('/')
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<ProjectPath, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root)
            && let Some(rel) = rel.to_str()
            && let Ok(content) = std::fs::read(&path)
        {
            out.insert(ProjectPath::new(rel.replace('\\', "/")), content);
        }
    }
}
