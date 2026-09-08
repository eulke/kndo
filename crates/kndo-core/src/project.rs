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
//! It replaced four hooks, and replacing means they are gone: one door, one
//! evidence type, and no second way to tell the engine what a manifest says.

use crate::discover::DiscoveredFile;
use kndo_contract::adapter::{ResolveContext, SourceFile};
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::manifest::{ManifestEvidence, ManifestSink, UnitKind, UnitRoot};
use kndo_contract::vocab::ProjectPath;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};

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
    /// Source roots, normalized: a unit that declared none compiles its
    /// manifest's own directory, so this is never empty of entries — only of
    /// characters, at the project root.
    pub roots: Vec<UnitRoot>,
    pub excludes: Vec<SmolStr>,
    pub entries: Vec<ProjectPath>,
    /// Every unit of this project this one compiles against, transitively,
    /// ascending — the resolution of what the manifest NAMED. Ascending so a
    /// membership test is a binary search and the order is a function of the
    /// unit list alone.
    pub compiles_against: Vec<u32>,
    /// The units whose unit-reaching names this one may use — the resolution
    /// of the [`kndo_contract::manifest::UnitDep`]s the manifest marked
    /// FRIEND, direct and ascending: friendship is the build system's
    /// statement about one pair of units and never carries over a third.
    pub friend_of: Vec<u32>,
    /// The manifest that aggregates this unit's — Maven's `<modules>`,
    /// Cargo's `workspace.members`, a SwiftPM package's targets — which is
    /// the group a `Reach::Unit { up: 1 }` declaration pools over; `None`
    /// where no manifest lists this one.
    pub group: Option<ProjectPath>,
    /// Whether the outside world consumes this unit's exported API —
    /// [`kndo_contract::manifest::Unit::is_published`], read once.
    pub published: bool,
    /// The name this unit's namespaces hang under when its ROOTS do not
    /// contain it — see [`kndo_contract::manifest::Unit::namespace_root`].
    pub namespace_root: Option<SmolStr>,
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
            .filter(|r| path.is_under(&r.path) && (r.recursive || directly_in(path, &r.path)))
            .map(|r| r.path.len())
            .max()
    }
}

/// Is `path` a file OF `dir` rather than of something nested under it? What a
/// non-recursive [`UnitRoot`] compiles.
fn directly_in(path: &ProjectPath, dir: &str) -> bool {
    let rest = match dir.is_empty() {
        true => path.as_str(),
        false => &path.as_str()[dir.len() + 1..],
    };
    !rest.contains('/')
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

    /// Can a file compiled in `viewer` name a namespace-scoped declaration of
    /// `target`? True for the unit itself, and for every unit it compiles
    /// against: a test module on the classpath of the library it exercises
    /// reads that library's namespace as its own.
    ///
    /// The engine asks this only where a language has said its namespaces
    /// span a compilation ([`kndo_contract::extension::NamespaceSpan`]) — the
    /// relation is about who is BUILT together, and what that implies about
    /// naming is the language's to state.
    pub fn sees_into(&self, viewer: u32, target: u32) -> bool {
        viewer == target
            || self.units[viewer as usize]
                .compiles_against
                .binary_search(&target)
                .is_ok()
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
    // Every manifest's content beside every other's: a reader whose build
    // system composes manifests (a Maven pom and its parent) reads the
    // others through the context, never from disk.
    let mut manifest_paths: BTreeSet<ProjectPath> = BTreeSet::new();
    crate::graph::for_each_manifest(files, adapters, |_, f| {
        manifest_paths.insert(f.path.clone());
    });
    let manifests: BTreeMap<ProjectPath, &[u8]> = files
        .iter()
        .filter(|f| manifest_paths.contains(&f.path))
        .map(|f| (f.path.clone(), f.content.as_slice()))
        .collect();
    let cx = ResolveContext::with_manifests(known, &manifests);
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
        // ONE manifest states a thing once, however many adapters claim it: a
        // Maven pom is read by java and by kotlin, and a project with both
        // does not thereby have two of every unit. What each adapter adds is
        // what the others did not already say.
        fn union<T: PartialEq>(into: &mut Vec<T>, from: Vec<T>) {
            for value in from {
                if !into.contains(&value) {
                    into.push(value);
                }
            }
        }
        union(&mut merged.units, read.units);
        union(&mut merged.members, read.members);
        union(&mut merged.packages, read.packages);
        union(&mut merged.dependencies, read.dependencies);
        union(&mut merged.mentions, read.mentions);
        union(&mut merged.aliases, read.aliases);
        union(&mut merged.ignores, read.ignores);
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

/// The units every manifest declared, normalized, ordered, and with every
/// named dependency resolved to the unit it means.
pub fn assemble(reads: &[ManifestRead]) -> Project {
    let mut units: Vec<ProjectUnit> = Vec::new();
    let mut named: Vec<(Vec<SmolStr>, Vec<SmolStr>)> = Vec::new();
    for read in reads {
        let dir = read
            .manifest
            .as_str()
            .rsplit_once('/')
            .map_or("", |(d, _)| d);
        for unit in &read.evidence.units {
            let mut roots: Vec<UnitRoot> = unit.roots.clone();
            // A unit that names no source root compiles its manifest's own
            // directory: the engine knows where the manifest is, so an adapter
            // never spells a path it did not read.
            if roots.is_empty() {
                roots.push(UnitRoot::from(dir));
            }
            roots.sort_by(|a, b| (&a.path, a.recursive).cmp(&(&b.path, b.recursive)));
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
                compiles_against: Vec::new(),
                friend_of: Vec::new(),
                published: unit.is_published(),
                group: None,
                namespace_root: unit.namespace_root.clone(),
            });
            named.push((
                unit.depends_on.iter().map(|d| d.unit.clone()).collect(),
                unit.depends_on
                    .iter()
                    .filter(|d| d.friend)
                    .map(|d| d.unit.clone())
                    .collect(),
            ));
        }
    }
    let mut order: Vec<usize> = (0..units.len()).collect();
    order.sort_by_key(|&i| {
        let u = &units[i];
        (u.manifest.clone(), u.name.clone(), u.kind)
    });
    let rank: Vec<u32> = {
        let mut r = vec![0u32; units.len()];
        for (position, &i) in order.iter().enumerate() {
            r[i] = position as u32;
        }
        r
    };
    // Both relations a manifest states by NAME resolve the same way: the
    // unit's own manifest first, then the nearest aggregator above it.
    let (direct, friends): (Vec<Vec<u32>>, Vec<Vec<u32>>) = {
        let aggregators = Aggregators::of(reads);
        let by_manifest_and_name: BTreeMap<(&ProjectPath, &SmolStr), u32> = units
            .iter()
            .enumerate()
            .map(|(i, u)| ((&u.manifest, &u.name), rank[i]))
            .collect();
        let resolve_all = |i: usize, names: &[SmolStr]| -> Vec<u32> {
            let mut out: Vec<u32> = names
                .iter()
                .filter_map(|n| aggregators.resolve(&units[i].manifest, n, &by_manifest_and_name))
                .filter(|&d| d != rank[i])
                .collect();
            out.sort_unstable();
            out.dedup();
            out
        };
        (
            (0..units.len())
                .map(|i| resolve_all(i, &named[i].0))
                .collect(),
            (0..units.len())
                .map(|i| resolve_all(i, &named[i].1))
                .collect(),
        )
    };
    let mut sorted: Vec<ProjectUnit> = order.iter().map(|&i| units[i].clone()).collect();
    let by_rank: Vec<Vec<u32>> = order.iter().map(|&i| direct[i].clone()).collect();
    let aggregators = Aggregators::of(reads);
    for (i, unit) in sorted.iter_mut().enumerate() {
        unit.compiles_against = closure(i as u32, &by_rank);
        unit.friend_of = friends[order[i]].clone();
        unit.group = aggregators.parent.get(&unit.manifest).cloned();
    }
    Project { units: sorted }
}

/// Everything `start` compiles against, transitively, ascending. A cycle in
/// the declarations — illegal in every build system that has a reactor, and
/// still possible in a file — terminates on the visited set rather than
/// recursing.
fn closure(start: u32, direct: &[Vec<u32>]) -> Vec<u32> {
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut queue: Vec<u32> = direct[start as usize].clone();
    while let Some(u) = queue.pop() {
        if u == start || !seen.insert(u) {
            continue;
        }
        queue.extend_from_slice(&direct[u as usize]);
    }
    seen.into_iter().collect()
}

/// Which manifest aggregates which — Maven's `<modules>`, Cargo's
/// `workspace.members` — and the resolution that needs it.
struct Aggregators {
    /// manifest → the manifest that lists it as a member.
    parent: BTreeMap<ProjectPath, ProjectPath>,
    /// manifest → the manifests it lists, in declaration order.
    members: BTreeMap<ProjectPath, Vec<ProjectPath>>,
}

impl Aggregators {
    fn of(reads: &[ManifestRead]) -> Aggregators {
        let mut parent = BTreeMap::new();
        let mut members = BTreeMap::new();
        for read in reads {
            if read.evidence.members.is_empty() {
                continue;
            }
            for m in &read.evidence.members {
                // The first aggregator to claim a manifest keeps it: reads are
                // path-ordered, so this is a function of the manifest set.
                parent.entry(m.clone()).or_insert(read.manifest.clone());
            }
            members.insert(read.manifest.clone(), read.evidence.members.clone());
        }
        Aggregators { parent, members }
    }

    /// The unit `name` means, seen FROM the manifest that named it: its own
    /// manifest first — one manifest declaring several units lets them name
    /// each other, which is how a Cargo target names its library or a
    /// SwiftPM test target names what it exercises — and then the nearest
    /// aggregator above it that lists a manifest declaring that name. Walking
    /// up and not merely across is what makes a nested reactor's dependency
    /// land in its own reactor: guava's `android/guava-tests` means
    /// `android/guava`, and the root `guava-tests` means `guava`, from the
    /// same word.
    fn resolve(
        &self,
        from: &ProjectPath,
        name: &SmolStr,
        units: &BTreeMap<(&ProjectPath, &SmolStr), u32>,
    ) -> Option<u32> {
        if let Some(&u) = units.get(&(from, name)) {
            return Some(u);
        }
        let mut cur = from;
        while let Some(aggregator) = self.parent.get(cur) {
            for sibling in self.members.get(aggregator).into_iter().flatten() {
                if let Some(&u) = units.get(&(sibling, name)) {
                    return Some(u);
                }
            }
            cur = aggregator;
        }
        None
    }
}

/// The five answers `resolve` asks the project for, indexed once — see
/// [`kndo_contract::adapter::ProjectView`]. Owned here because the view
/// borrows: an adapter reads it, and the engine is what holds it alive.
pub struct ProjectIndex {
    units: Vec<kndo_contract::adapter::UnitView>,
    unit_of: BTreeMap<ProjectPath, u32>,
    aliases: Vec<(SmolStr, kndo_contract::manifest::PathAlias)>,
    namespaces: BTreeMap<ProjectPath, Vec<SmolStr>>,
    in_namespace: BTreeMap<Vec<SmolStr>, Vec<ProjectPath>>,
}

impl ProjectIndex {
    /// Built from what the manifests declared and what each file's own clause
    /// says — the two halves of the plan's "namespaces por cláusula". The
    /// namespace halves are empty on the paths that have no evidence yet (a
    /// manifest read runs before extraction), and a query over them answers
    /// nothing rather than guessing.
    pub fn build(
        project: &Project,
        reads: &[ManifestRead],
        namespaces: impl Iterator<Item = (ProjectPath, Vec<SmolStr>)>,
    ) -> ProjectIndex {
        let units: Vec<kndo_contract::adapter::UnitView> = project
            .units
            .iter()
            .map(|u| kndo_contract::adapter::UnitView {
                name: u.name.clone(),
                kind: u.kind,
                roots: u.roots.clone(),
                namespace_root: u.namespace_root.clone(),
                published: u.published,
                compiles_against: u.compiles_against.clone(),
            })
            .collect();
        let mut aliases: Vec<(SmolStr, kndo_contract::manifest::PathAlias)> = reads
            .iter()
            .flat_map(|r| {
                let dir = SmolStr::new(r.manifest.as_str().rsplit_once('/').map_or("", |(d, _)| d));
                r.evidence
                    .aliases
                    .iter()
                    .map(move |a| (dir.clone(), a.clone()))
            })
            .collect();
        aliases.sort();
        aliases.dedup();
        let mut of_file: BTreeMap<ProjectPath, u32> = BTreeMap::new();
        let mut by_namespace: BTreeMap<Vec<SmolStr>, Vec<ProjectPath>> = BTreeMap::new();
        let mut of_path: BTreeMap<ProjectPath, Vec<SmolStr>> = BTreeMap::new();
        for (path, segments) in namespaces {
            if let Some(unit) = project.unit_of(&path) {
                of_file.insert(path.clone(), unit);
            }
            if !segments.is_empty() {
                by_namespace
                    .entry(segments.clone())
                    .or_default()
                    .push(path.clone());
                of_path.insert(path, segments);
            }
        }
        for files in by_namespace.values_mut() {
            files.sort();
            files.dedup();
        }
        ProjectIndex {
            units,
            unit_of: of_file,
            aliases,
            namespaces: of_path,
            in_namespace: by_namespace,
        }
    }

    pub fn view(&self) -> kndo_contract::adapter::ProjectView<'_> {
        kndo_contract::adapter::ProjectView::new(
            &self.units,
            &self.unit_of,
            &self.aliases,
            &self.namespaces,
            &self.in_namespace,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_contract::manifest::{Unit, UnitDep};

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

    /// An aggregator: a manifest that declares no unit of its own, only the
    /// manifests it lists — Maven's `<packaging>pom</packaging>`.
    fn aggregator(manifest: &str, members: &[&str]) -> ManifestRead {
        ManifestRead {
            manifest: ProjectPath::new(manifest),
            evidence: ManifestEvidence {
                members: members.iter().map(|m| ProjectPath::new(*m)).collect(),
                ..ManifestEvidence::default()
            },
            adapters: Vec::new(),
        }
    }

    fn needing(mut unit: Unit, names: &[&str]) -> Unit {
        unit.depends_on = names.iter().map(|n| UnitDep::on(*n)).collect();
        unit
    }

    /// The same, with every named dependency marked a friendship.
    fn befriending(mut unit: Unit, names: &[&str]) -> Unit {
        unit.depends_on = names.iter().map(|n| UnitDep::friend(*n)).collect();
        unit
    }

    fn index_of(project: &Project, manifest: &str) -> u32 {
        project
            .units
            .iter()
            .position(|u| u.manifest.as_str() == manifest)
            .expect("unit declared by that manifest") as u32
    }

    #[test]
    fn a_dependency_name_resolves_inside_its_own_reactor() {
        // guava's shape: two reactors, each with a `guava` and a `guava-tests`.
        // The word `guava` in each tests module means ITS sibling, and the two
        // compilations never meet.
        let project = assemble(&[
            aggregator("pom.xml", &["guava/pom.xml", "guava-tests/pom.xml"]),
            aggregator(
                "android/pom.xml",
                &["android/guava/pom.xml", "android/guava-tests/pom.xml"],
            ),
            read(
                "guava/pom.xml",
                vec![unit("guava", UnitKind::Library, &["guava"], &[])],
            ),
            read(
                "guava-tests/pom.xml",
                vec![needing(
                    unit("guava-tests", UnitKind::Test, &["guava-tests"], &[]),
                    &["guava"],
                )],
            ),
            read(
                "android/guava/pom.xml",
                vec![unit("guava", UnitKind::Library, &["android/guava"], &[])],
            ),
            read(
                "android/guava-tests/pom.xml",
                vec![needing(
                    unit("guava-tests", UnitKind::Test, &["android/guava-tests"], &[]),
                    &["guava"],
                )],
            ),
        ]);
        let main = index_of(&project, "guava/pom.xml");
        let tests = index_of(&project, "guava-tests/pom.xml");
        let android_main = index_of(&project, "android/guava/pom.xml");
        let android_tests = index_of(&project, "android/guava-tests/pom.xml");

        assert!(project.sees_into(tests, main));
        assert!(project.sees_into(android_tests, android_main));
        assert!(
            !project.sees_into(tests, android_main),
            "the mirror is another reactor, however identical its names"
        );
        assert!(!project.sees_into(android_tests, main));
        assert!(
            !project.sees_into(main, tests),
            "seeing is directional: the library never reads its tests"
        );
        assert!(project.sees_into(main, main), "a unit sees itself");
    }

    #[test]
    fn compiling_against_is_transitive_and_survives_a_cycle() {
        let project = assemble(&[
            aggregator("pom.xml", &["a/pom.xml", "b/pom.xml", "c/pom.xml"]),
            read(
                "a/pom.xml",
                vec![needing(unit("a", UnitKind::Test, &["a"], &[]), &["b"])],
            ),
            read(
                "b/pom.xml",
                vec![needing(unit("b", UnitKind::Library, &["b"], &[]), &["c"])],
            ),
            read(
                "c/pom.xml",
                // A declaration cycle no build system would accept; the
                // closure still terminates and stays a set.
                vec![needing(unit("c", UnitKind::Library, &["c"], &[]), &["a"])],
            ),
        ]);
        let (a, b, c) = (
            index_of(&project, "a/pom.xml"),
            index_of(&project, "b/pom.xml"),
            index_of(&project, "c/pom.xml"),
        );
        assert!(project.sees_into(a, c), "through b");
        assert_eq!(project.units[a as usize].compiles_against, vec![b, c]);
    }

    #[test]
    fn a_dependency_on_something_no_aggregator_lists_resolves_to_nothing() {
        let project = assemble(&[
            aggregator("pom.xml", &["app/pom.xml"]),
            read(
                "app/pom.xml",
                vec![needing(
                    unit("app", UnitKind::Executable, &["app"], &[]),
                    &["junit", "app"],
                )],
            ),
        ]);
        let app = index_of(&project, "app/pom.xml");
        assert!(
            project.units[app as usize].compiles_against.is_empty(),
            "an external artifact is not a unit, and a unit is not its own dependency"
        );
    }

    fn unit(name: &str, kind: UnitKind, roots: &[&str], excludes: &[&str]) -> Unit {
        Unit {
            name: name.into(),
            kind,
            roots: roots.iter().map(|r| UnitRoot::from(*r)).collect(),
            excludes: excludes.iter().map(|e| SmolStr::new(*e)).collect(),
            entries: Vec::new(),
            depends_on: Vec::new(),
            publication: Default::default(),
            namespace_root: None,
        }
    }

    #[test]
    fn a_friend_is_named_the_way_a_dependency_is_and_never_carries_over() {
        let project = assemble(&[
            aggregator("pom.xml", &["core/pom.xml", "other/pom.xml"]),
            read(
                "core/pom.xml",
                vec![
                    unit("core", UnitKind::Library, &["core/src/main/java"], &[]),
                    befriending(
                        unit("core:test", UnitKind::Test, &["core/src/test/java"], &[]),
                        &["core"],
                    ),
                ],
            ),
            read(
                "other/pom.xml",
                vec![befriending(
                    unit("other", UnitKind::Library, &["other"], &[]),
                    &["core:test"],
                )],
            ),
        ]);
        let by_name = |n: &str| -> &ProjectUnit {
            project
                .units
                .iter()
                .find(|u| u.name.as_str() == n)
                .expect("declared")
        };
        let core = project.units.iter().position(|u| u.name == "core").unwrap() as u32;
        let core_test = project
            .units
            .iter()
            .position(|u| u.name == "core:test")
            .unwrap() as u32;
        assert_eq!(by_name("core:test").friend_of, vec![core]);
        assert_eq!(
            by_name("other").friend_of,
            vec![core_test],
            "resolved through the aggregator like a dependency"
        );
        assert!(
            by_name("core").friend_of.is_empty(),
            "friendship is directional"
        );
        assert!(by_name("core").published && !by_name("core:test").published);
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
        assert_eq!(project.units[0].roots, [UnitRoot::from("services/api")]);
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
