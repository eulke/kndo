//! The project's structure, assembled from what its manifests SAY — units with
//! the directories they compile and the files they are entered through.
//!
//! Before this module the same knowledge was re-derived per adapter from path
//! conventions (a source-set layout, a Cargo target's auto-discovery, SwiftPM's
//! `path:`), which is why nine adapters each carried a convention table and why
//! a project laid out unusually was judged as if it were laid out normally. Now
//! one hook, [`kndo_contract::extension::Extension::extract_manifest`], writes
//! [`ManifestEvidence`], and the engine owns the structure: which unit compiles
//! a file, which files a unit is entered through, and — through
//! [`UnitKind::color`] — what color those entries anchor.
//!
//! The four hooks this replaces (`roots`, `packages`, `manifest_dependencies`,
//! `manifest_mentions`) are still read and merged, so an adapter migrates on
//! its own schedule and the bridge retires by deletion, not by a flag.

use crate::discover::DiscoveredFile;
use kndo_contract::adapter::{ResolveContext, SourceFile};
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::manifest::{ManifestEvidence, ManifestSink, UnitKind};
use kndo_contract::vocab::ProjectPath;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::BTreeSet;

/// One manifest as its claiming adapters read it. Path-ordered, one entry per
/// discovered manifest — a manifest that declares nothing still has an entry,
/// because it is still the manifest its directory's files answer to.
pub struct ManifestRead {
    pub manifest: ProjectPath,
    pub evidence: ManifestEvidence,
    /// The coordinates that claimed this manifest, in registration order. The
    /// first is the claimant whose declared capabilities judge its
    /// dependencies.
    pub adapters: Vec<SmolStr>,
}

/// One unit as the project sees it: what a manifest declared, plus the manifest
/// that declared it (identity is the pair — parallel trees legitimately give
/// two units one name).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectUnit {
    pub name: SmolStr,
    pub kind: UnitKind,
    pub manifest: ProjectPath,
    /// Source-root directories, normalized: a unit that declared none compiles
    /// its manifest's own directory, so this is never empty of entries — only
    /// of characters, at the project root.
    pub roots: Vec<SmolStr>,
    pub excludes: Vec<SmolStr>,
    pub entries: Vec<ProjectPath>,
}

impl ProjectUnit {
    /// Does this unit compile `path`? The longest root that contains it, minus
    /// anything an exclude covers. `None` when no root does; `Some(depth)`
    /// carries how deep the matching root was, which is how nested units
    /// (a workspace member inside a workspace) resolve without an ordering rule.
    fn depth_of(&self, path: &ProjectPath) -> Option<usize> {
        if self.excludes.iter().any(|e| path.is_under(e)) {
            return None;
        }
        self.roots
            .iter()
            .filter(|r| path.is_under(r))
            .map(|r| r.len())
            .max()
    }
}

/// What the project's manifests declared, assembled. Serialized with the graph:
/// a pure function of the manifest set and its contents, which the persisted
/// graph's manifest-state hash already covers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Project {
    /// Every declared unit, ordered by (manifest, name, kind) — stable against
    /// a manifest reordering its own targets.
    pub units: Vec<ProjectUnit>,
}

impl Project {
    /// The unit compiling `path`: the one whose source root is the longest
    /// prefix of it, excludes honored. `None` when no unit claims the file —
    /// which is every file until its adapter migrates, and the reason every
    /// consumer degrades toward its pre-unit behavior.
    pub fn unit_of(&self, path: &ProjectPath) -> Option<u32> {
        self.units
            .iter()
            .enumerate()
            .filter_map(|(i, u)| u.depth_of(path).map(|d| (d, i)))
            // Deepest root wins; the earlier unit breaks a tie, so the answer
            // is a function of the sorted unit list alone.
            .max_by_key(|&(d, i)| (d, std::cmp::Reverse(i)))
            .map(|(_, i)| i as u32)
    }

    /// Every unit's entries, as (file, color) — what assembly anchors. A unit's
    /// entry is the manifest's own statement, so it anchors `Certain`; an
    /// adapter that only GUESSES an entry reports it through
    /// [`ManifestEvidence::roots`] with a confidence of its own.
    pub fn entry_roots(
        &self,
    ) -> impl Iterator<Item = (&ProjectPath, kndo_contract::evidence::RootKind)> {
        self.units
            .iter()
            .flat_map(|u| u.entries.iter().map(move |e| (e, u.kind.color())))
    }
}

/// Every discovered manifest read once, merged across the adapters that claim
/// it. Launchers are read too and contribute their roots alone: a launcher
/// declares no unit, no package and no dependency ([`ExtensionSpec::launchers`]).
pub fn read_manifests(
    files: &[DiscoveredFile],
    adapters: &[Box<dyn Extension>],
    known: &BTreeSet<ProjectPath>,
) -> Vec<ManifestRead> {
    let cx = ResolveContext::new(known);
    let mut by_path: std::collections::BTreeMap<ProjectPath, (ManifestEvidence, Vec<SmolStr>)> =
        std::collections::BTreeMap::new();
    let mut read = |adapter: &dyn Extension, file: SourceFile<'_>, whole: bool| {
        let mut sink = ManifestSink::new();
        adapter.extract_manifest(&file, &cx, &mut sink);
        let read = sink.finish();
        let (merged, claimants) = by_path.entry(file.path.clone()).or_default();
        merged.roots.extend(read.roots);
        merged.diagnostics.extend(read.diagnostics);
        if !whole {
            return;
        }
        claimants.push(SmolStr::new(adapter.spec().coordinate()));
        merged.units.extend(read.units);
        merged.packages.extend(read.packages);
        merged.dependencies.extend(read.dependencies);
        merged.mentions.extend(read.mentions);
        // The bridge, until every adapter speaks the one hook: what the four
        // hooks this replaces return joins the same evidence, so every
        // consumer reads ONE value and the bridge retires by deleting these
        // four lines with the hooks. An adapter populates one side or the
        // other, never both, so the union is disjoint.
        merged.packages.extend(adapter.packages(&file, &cx));
        merged
            .dependencies
            .extend(adapter.manifest_dependencies(&file));
        merged.mentions.extend(adapter.manifest_mentions(&file));
    };
    crate::graph::for_each_matching(files, adapters, ExtensionSpec::manifests, |a, f| {
        read(a, f, true)
    });
    crate::graph::for_each_matching(files, adapters, ExtensionSpec::launchers, |a, f| {
        read(a, f, false)
    });
    by_path
        .into_iter()
        .map(|(manifest, (evidence, adapters))| ManifestRead {
            manifest,
            evidence,
            adapters,
        })
        .collect()
}

/// The units every manifest declared, normalized and ordered.
pub fn assemble(reads: &[ManifestRead]) -> Project {
    let mut units: Vec<ProjectUnit> = Vec::new();
    for read in reads {
        let dir = read
            .manifest
            .as_str()
            .rsplit_once('/')
            .map_or("", |(d, _)| d);
        for unit in &read.evidence.units {
            let mut roots: Vec<SmolStr> = unit.roots.clone();
            // A unit that names no source root compiles its manifest's own
            // directory: the engine knows where the manifest is, so an adapter
            // never spells a path it did not read.
            if roots.is_empty() {
                roots.push(SmolStr::new(dir));
            }
            roots.sort();
            roots.dedup();
            let mut excludes = unit.excludes.clone();
            excludes.sort();
            excludes.dedup();
            let mut entries = unit.entries.clone();
            entries.sort();
            entries.dedup();
            units.push(ProjectUnit {
                name: unit.name.clone(),
                kind: unit.kind,
                manifest: read.manifest.clone(),
                roots,
                excludes,
                entries,
            });
        }
    }
    units.sort_by(|a, b| (&a.manifest, &a.name, a.kind).cmp(&(&b.manifest, &b.name, b.kind)));
    Project { units }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_contract::manifest::Unit;

    fn read(manifest: &str, units: Vec<Unit>) -> ManifestRead {
        ManifestRead {
            manifest: ProjectPath::new(manifest),
            evidence: ManifestEvidence {
                units,
                ..ManifestEvidence::default()
            },
            adapters: Vec::new(),
        }
    }

    fn unit(name: &str, kind: UnitKind, roots: &[&str], excludes: &[&str]) -> Unit {
        Unit {
            name: name.into(),
            kind,
            roots: roots.iter().map(|r| SmolStr::new(*r)).collect(),
            excludes: excludes.iter().map(|e| SmolStr::new(*e)).collect(),
            entries: Vec::new(),
        }
    }

    #[test]
    fn the_deepest_source_root_owns_the_file() {
        let project = assemble(&[
            read(
                "Cargo.toml",
                vec![unit("workspace", UnitKind::Library, &["crates"], &[])],
            ),
            read(
                "crates/core/Cargo.toml",
                vec![unit("core", UnitKind::Library, &["crates/core/src"], &[])],
            ),
        ]);
        let named = |p: &str| {
            project
                .unit_of(&ProjectPath::new(p))
                .map(|i| project.units[i as usize].name.as_str().to_string())
        };
        assert_eq!(named("crates/core/src/lib.rs").as_deref(), Some("core"));
        assert_eq!(
            named("crates/other/src/lib.rs").as_deref(),
            Some("workspace")
        );
        assert_eq!(named("README.md"), None);
    }

    #[test]
    fn an_exclude_removes_a_file_from_its_unit() {
        let project = assemble(&[read(
            "Cargo.toml",
            vec![unit("app", UnitKind::Executable, &["src"], &["src/vendor"])],
        )]);
        assert!(project.unit_of(&ProjectPath::new("src/main.rs")).is_some());
        assert!(
            project
                .unit_of(&ProjectPath::new("src/vendor/blob.rs"))
                .is_none(),
            "an excluded path belongs to no unit, not to the next one up"
        );
    }

    #[test]
    fn a_unit_with_no_root_compiles_its_manifests_directory() {
        let project = assemble(&[read(
            "services/api/package.json",
            vec![unit("api", UnitKind::Library, &[], &[])],
        )]);
        assert_eq!(project.units[0].roots, ["services/api"]);
        assert!(
            project
                .unit_of(&ProjectPath::new("services/api/src/index.js"))
                .is_some()
        );
        assert!(project.unit_of(&ProjectPath::new("src/index.js")).is_none());
    }

    #[test]
    fn units_are_ordered_by_manifest_then_name_whatever_the_manifest_said() {
        let forward = assemble(&[read(
            "Cargo.toml",
            vec![
                unit("zeta", UnitKind::Test, &["tests"], &[]),
                unit("alpha", UnitKind::Library, &["src"], &[]),
            ],
        )]);
        let reversed = assemble(&[read(
            "Cargo.toml",
            vec![
                unit("alpha", UnitKind::Library, &["src"], &[]),
                unit("zeta", UnitKind::Test, &["tests"], &[]),
            ],
        )]);
        let names =
            |p: &Project| -> Vec<String> { p.units.iter().map(|u| u.name.to_string()).collect() };
        assert_eq!(names(&forward), ["alpha", "zeta"]);
        assert_eq!(names(&forward), names(&reversed));
    }

    #[test]
    fn entries_anchor_the_color_of_their_units_kind() {
        let project = assemble(&[read(
            "Cargo.toml",
            vec![
                Unit {
                    entries: vec![ProjectPath::new("src/lib.rs")],
                    ..unit("lib", UnitKind::Library, &["src"], &[])
                },
                Unit {
                    entries: vec![ProjectPath::new("tests/api.rs")],
                    ..unit("api", UnitKind::Test, &["tests"], &[])
                },
            ],
        )]);
        use kndo_contract::evidence::RootKind;
        let roots: Vec<(String, RootKind)> = project
            .entry_roots()
            .map(|(p, k)| (p.as_str().to_string(), k))
            .collect();
        assert_eq!(
            roots,
            [
                ("tests/api.rs".to_string(), RootKind::Test),
                ("src/lib.rs".to_string(), RootKind::Production),
            ]
        );
    }
}
