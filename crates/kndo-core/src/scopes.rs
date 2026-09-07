//! The scope forest: where a name may legally be used, as structure rather than
//! as a path convention.
//!
//! Today it holds one layer — namespaces, from the clause each file declares
//! ([`kndo_contract::evidence::FileEvidence::namespace`]) — and that layer
//! already replaces what nine adapters used to compute from directory shapes.
//! guava is the case that proves the difference: its tests sit in
//! `guava-tests/test/...` while its sources sit in `guava/src/...`, so no
//! `src/main` ↔ `src/test` mirror rule can pair them, yet both files declare
//! `package com.google.common.io` — and their manifests say `guava-tests`
//! compiles against `guava`, which is what makes the two one namespace while
//! the identically-named Android mirror stays another.
//!
//! The owners and embedded regions of the full forest land with the consumers
//! that read them; a layer with nobody asking is a field, not a structure.

use crate::analysis::DeclaredCapabilities;
use crate::graph::Graph;
use kndo_contract::extension::NamespaceSpan;
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};

pub struct Scopes {
    /// file → the namespace node it declares itself into. Every file has one:
    /// a file declaring no namespace is a node of its own, so a
    /// namespace-reaching declaration there pools nothing beyond its file.
    of_file: Vec<u32>,
    /// namespace node → the files of THIS node, ascending: one name inside one
    /// compilation.
    files: Vec<Vec<u32>>,
    /// namespace node → those files plus every file spelling the same name in
    /// a unit that compiles against this node's, ascending. Equal to `files`
    /// wherever no manifest said otherwise.
    spanned: Vec<Vec<u32>>,
    /// file → whether the language that claims it says a namespace spans the
    /// compilation. Per file, because the DECLARATION's language decides who
    /// may name it, and one namespace can hold two languages' files.
    spans: Vec<bool>,
    /// unit → the files a unit-reaching declaration in it pools over: the
    /// unit's own files plus every friend's, ascending. The unit layer of the
    /// forest, read where a manifest named the unit; until one does, the
    /// adapter's own enumeration stands in.
    unit_pool: Vec<Vec<u32>>,
    /// unit → the files of every unit its aggregator lists beside it, its own
    /// included, ascending; empty for a unit no manifest aggregates. What a
    /// group-reaching declaration (Swift's `package`) pools over.
    group_pool: Vec<Vec<u32>>,
    /// directory → the files under it, ascending, for every directory of the
    /// tree, the root as the empty path. What a directory-reaching
    /// declaration (Go's `internal`) pools over.
    directories: BTreeMap<SmolStr, Vec<u32>>,
    /// (compilation, namespace segments) → node, so a namespace spelled by
    /// name (`pub(in crate::a)`) is found inside the speller's own
    /// compilation.
    nodes: BTreeMap<(Compilation, Vec<SmolStr>), u32>,
    /// file → its compilation, the key a named namespace is looked up under.
    compilation_of: Vec<Compilation>,
    /// file → its directory, the empty path at the root — where a directory
    /// climb starts.
    dirs: Vec<SmolStr>,
    /// node → the node it hangs under in a mount forest, `None` at a tree's
    /// root and for every node of a language that mounts nothing.
    parent_node: Vec<Option<u32>>,
    /// node → its own files plus every descendant node's, ascending. What a
    /// namespace reaches where namespaces NEST: a private name is readable in
    /// its module and everything mounted under it.
    subtree: Vec<Vec<u32>>,
    /// node → whether it stands in a mount forest, so a pool reads the
    /// subtree instead of the node's own files.
    node_in_forest: Vec<bool>,
    /// file → whether it stands in a mount forest.
    forest: Vec<bool>,
    /// file → the node its tree is rooted at; its own node where it is not
    /// mounted.
    root_node: Vec<u32>,
}

impl Scopes {
    pub fn build(graph: &Graph, capabilities: &[(SmolStr, DeclaredCapabilities)]) -> Scopes {
        // A node is one namespace inside one COMPILATION: the unit that
        // compiles the file when a manifest named one, and otherwise the
        // source root its own declaration implies — or, where the language
        // MOUNTS its namespaces, the chain of segments the mounts spell,
        // inside the tree they are rooted at.
        let chains = mount_chains(graph);
        let mut nodes: BTreeMap<(Compilation, Vec<SmolStr>), u32> = BTreeMap::new();
        let mut of_file: Vec<u32> = Vec::with_capacity(graph.files.len());
        let mut compilation_of: Vec<Compilation> = Vec::with_capacity(graph.files.len());
        let mut files: Vec<Vec<u32>> = Vec::new();
        let mut unit_of_node: Vec<Option<u32>> = Vec::new();
        let mut segments_of_node: Vec<Vec<SmolStr>> = Vec::new();
        for (i, f) in graph.files.iter().enumerate() {
            let key = if let Some((root, segments)) = &chains[i] {
                (
                    Compilation::Tree(SmolStr::new(graph.files[*root].path.as_str())),
                    segments.clone(),
                )
            } else if f.evidence.namespace.is_empty() {
                // Its own node, named by nothing another file can spell.
                (
                    Compilation::Alone(SmolStr::new(f.path.as_str())),
                    Vec::new(),
                )
            } else {
                let compilation = match f.unit {
                    Some(u) => Compilation::Unit(u),
                    None => Compilation::Root(source_root(f)),
                };
                (compilation, f.evidence.namespace.clone())
            };
            let node = *nodes.entry(key.clone()).or_insert_with(|| {
                files.push(Vec::new());
                unit_of_node.push(f.unit);
                segments_of_node.push(key.1.clone());
                (files.len() - 1) as u32
            });
            files[node as usize].push(i as u32);
            of_file.push(node);
            compilation_of.push(key.0);
        }
        // The forest, in node terms: who hangs under whom, and every node's
        // subtree. A node nobody mounts and that mounts nobody has neither.
        let node_in_forest: Vec<bool> = {
            let mut out = vec![false; files.len()];
            for (i, chain) in chains.iter().enumerate() {
                if chain.is_some() {
                    out[of_file[i] as usize] = true;
                }
            }
            out
        };
        let mut parent_node: Vec<Option<u32>> = vec![None; files.len()];
        for (i, f) in graph.files.iter().enumerate() {
            if let Some(edge) = &f.mounted_by {
                let node = of_file[i] as usize;
                parent_node[node].get_or_insert(of_file[edge.parent as usize]);
            }
        }
        let subtree = subtrees(&files, &parent_node);
        let root_node: Vec<u32> = (0..graph.files.len())
            .map(|i| match &chains[i] {
                Some((root, _)) => of_file[*root],
                None => of_file[i],
            })
            .collect();
        let forest: Vec<bool> = chains.iter().map(Option::is_some).collect();
        let spanned = span_nodes(graph, &files, &unit_of_node, &segments_of_node);
        let spans = graph
            .files
            .iter()
            .map(|f| {
                capabilities
                    .iter()
                    .find(|(c, _)| *c == f.adapter)
                    .is_some_and(|(_, caps)| caps.namespace_span == NamespaceSpan::Compilation)
            })
            .collect();
        Scopes {
            of_file,
            files,
            spanned,
            spans,
            unit_pool: unit_pools(graph),
            group_pool: group_pools(graph),
            directories: directories(graph),
            nodes,
            compilation_of,
            dirs: graph
                .files
                .iter()
                .map(|f| SmolStr::new(f.path.as_str().rsplit_once('/').map_or("", |(d, _)| d)))
                .collect(),
            parent_node,
            subtree,
            node_in_forest,
            forest,
            root_node,
        }
    }

    /// The files a `Reach::Unit { up: 1 }` declaration in `unit` pools over:
    /// every unit the same manifest aggregates. `None` where no manifest
    /// aggregates the unit — unbounded, keep-alive.
    pub fn group_pool(&self, unit: u32) -> Option<&[u32]> {
        let pool = &self.group_pool[unit as usize];
        (!pool.is_empty()).then_some(pool.as_slice())
    }

    /// The files a `Reach::Directory { up }` declaration in `file` pools over:
    /// everything under the directory `up` levels above the file's own, the
    /// root included when the climb reaches it. `None` when the climb would
    /// leave the tree — unbounded, keep-alive.
    pub fn directory_pool(&self, file: usize, up: u32) -> Option<&[u32]> {
        let mut dir = self.file_dir(file);
        for _ in 0..up {
            if dir.is_empty() {
                return None;
            }
            dir = dir.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
        self.directories.get(dir).map(Vec::as_slice)
    }

    /// The files this one COMPILES WITH, where the language says its namespace
    /// compiles as one: its namespace node's files. Reachability's input, not a
    /// pool — a pool asks who may NAME a declaration, this asks what the
    /// compiler builds together, and only the second makes a file with no
    /// exported name alive because its package is.
    pub fn covisible(&self, file: usize) -> &[u32] {
        &self.files[self.of_file[file] as usize]
    }

    /// The files a `Reach::Named { namespace }` declaration in `file` pools
    /// over: the node of that name inside the file's own compilation. `None`
    /// when no file of that compilation declares the name — unbounded.
    pub fn named_pool(&self, file: usize, namespace: &[SmolStr]) -> Option<&[u32]> {
        let key = (self.compilation_of[file].clone(), namespace.to_vec());
        let node = *self.nodes.get(&key)? as usize;
        Some(self.pool_at(file, node))
    }

    /// The files a `Reach::Unit { up: 0 }` declaration pools over where no
    /// manifest named the unit but the language mounts its namespaces: the
    /// whole tree this file hangs in — a Rust crate, reached from any module
    /// of it. `None` outside a forest, where the adapter's own region still
    /// answers.
    pub fn tree_pool(&self, file: usize) -> Option<&[u32]> {
        self.forest[file].then(|| self.subtree[self.root_node[file] as usize].as_slice())
    }

    /// One node's pool as `file` reads it: the subtree where namespaces nest,
    /// the node's own files where they are flat, and every file of a
    /// same-named node in a unit this one compiles against where the language
    /// says a namespace spans the compilation.
    fn pool_at(&self, file: usize, node: usize) -> &[u32] {
        if self.node_in_forest[node] {
            &self.subtree[node]
        } else if self.spans[file] {
            &self.spanned[node]
        } else {
            &self.files[node]
        }
    }

    fn file_dir(&self, file: usize) -> &str {
        &self.dirs[file]
    }

    /// The files a `Reach::Unit { up: 0 }` declaration in `unit` pools over: the unit's
    /// own and its friends'. Never empty for a unit some file belongs to.
    pub fn unit_pool(&self, unit: u32) -> &[u32] {
        &self.unit_pool[unit as usize]
    }

    /// The files a `Reach::Namespace { up }` declaration in `file` pools over.
    ///
    /// `None` means unbounded — the engine cannot name that node, so the
    /// declaration is judged as published surface, keep-alive. Which is where
    /// `up > 0` sits until a language declares how its namespaces nest: the
    /// ancestor of `com.foo.bar` is `com.foo` in Rust's module tree and
    /// nothing at all in Java's flat packages, and core does not guess.
    pub fn namespace_pool(&self, file: usize, up: u32) -> Option<&[u32]> {
        let mut node = self.of_file[file] as usize;
        if self.forest[file] {
            // Nested namespaces: climb the mounts, and a climb that leaves
            // the tree names nothing this project can enumerate.
            for _ in 0..up {
                node = self.parent_node[node]? as usize;
            }
            return Some(&self.subtree[node]);
        }
        if up > 0 {
            return None;
        }
        Some(self.pool_at(file, node))
    }
}

/// Every file's address in the mount forest: the file its tree is rooted at
/// and the segments the mounts spell down to it. `None` for a file no mount
/// touches — which is every file of a language that mounts nothing, and a
/// standalone file of one that does.
///
/// A chain that closes on itself cannot be a module tree; it stops where it
/// repeats and the file reads as its own root, which keeps a pool bounded by
/// what the evidence actually spelled.
fn mount_chains(graph: &Graph) -> Vec<Option<(usize, Vec<SmolStr>)>> {
    let mut mounts_something = vec![false; graph.files.len()];
    for f in &graph.files {
        if let Some(edge) = &f.mounted_by {
            mounts_something[edge.parent as usize] = true;
        }
    }
    (0..graph.files.len())
        .map(|i| {
            if graph.files[i].mounted_by.is_none() && !mounts_something[i] {
                return None;
            }
            let mut segments: Vec<SmolStr> = Vec::new();
            let mut cursor = i;
            let mut seen: BTreeSet<usize> = BTreeSet::new();
            while let Some(edge) = &graph.files[cursor].mounted_by {
                if !seen.insert(cursor) {
                    break;
                }
                segments.push(edge.segment.clone());
                cursor = edge.parent as usize;
            }
            segments.reverse();
            Some((cursor, segments))
        })
        .collect()
}

/// Each node's files plus every descendant's, ascending. Nodes outside a
/// forest have no descendants, so their subtree is their own files — which
/// nothing reads, and which keeps the vector index-parallel to the nodes.
fn subtrees(files: &[Vec<u32>], parent_node: &[Option<u32>]) -> Vec<Vec<u32>> {
    let mut children: Vec<Vec<u32>> = vec![Vec::new(); files.len()];
    for (node, parent) in parent_node.iter().enumerate() {
        if let Some(p) = parent {
            children[*p as usize].push(node as u32);
        }
    }
    (0..files.len())
        .map(|root| {
            let mut out: Vec<u32> = Vec::new();
            let mut pending = vec![root as u32];
            let mut seen: BTreeSet<u32> = BTreeSet::new();
            while let Some(node) = pending.pop() {
                if !seen.insert(node) {
                    continue;
                }
                out.extend_from_slice(&files[node as usize]);
                pending.extend_from_slice(&children[node as usize]);
            }
            out.sort_unstable();
            out.dedup();
            out
        })
        .collect()
}

/// Each unit's files plus the files of every unit that is its friend — what
/// a unit-reaching name may be used from, per the manifests' own statements.
fn unit_pools(graph: &Graph) -> Vec<Vec<u32>> {
    let units = graph.project.units.len();
    let mut own: Vec<Vec<u32>> = vec![Vec::new(); units];
    for (i, f) in graph.files.iter().enumerate() {
        if let Some(u) = f.unit {
            own[u as usize].push(i as u32);
        }
    }
    let mut pools = own.clone();
    for (viewer, unit) in graph.project.units.iter().enumerate() {
        for &target in &unit.friend_of {
            pools[target as usize].extend_from_slice(&own[viewer]);
        }
    }
    for pool in &mut pools {
        pool.sort_unstable();
        pool.dedup();
    }
    pools
}

/// Each unit's files together with those of every unit its aggregator lists
/// — the group a `package`-reaching name may be used from. Empty where no
/// manifest aggregates the unit.
fn group_pools(graph: &Graph) -> Vec<Vec<u32>> {
    let units = &graph.project.units;
    let mut own: Vec<Vec<u32>> = vec![Vec::new(); units.len()];
    for (i, f) in graph.files.iter().enumerate() {
        if let Some(u) = f.unit {
            own[u as usize].push(i as u32);
        }
    }
    units
        .iter()
        .map(|unit| {
            let Some(group) = &unit.group else {
                return Vec::new();
            };
            let mut pool: Vec<u32> = units
                .iter()
                .enumerate()
                .filter(|(_, other)| other.group.as_ref() == Some(group))
                .flat_map(|(i, _)| own[i].iter().copied())
                .collect();
            pool.sort_unstable();
            pool.dedup();
            pool
        })
        .collect()
}

/// Every directory of the tree — the root as the empty path — with the files
/// under it, ascending: each file is listed under each of its ancestors.
fn directories(graph: &Graph) -> BTreeMap<SmolStr, Vec<u32>> {
    let mut out: BTreeMap<SmolStr, Vec<u32>> = BTreeMap::new();
    for (i, f) in graph.files.iter().enumerate() {
        let mut dir = f.path.as_str().rsplit_once('/').map_or("", |(d, _)| d);
        loop {
            out.entry(SmolStr::new(dir)).or_default().push(i as u32);
            if dir.is_empty() {
                break;
            }
            dir = dir.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }
    out
}

/// Which compilation a namespace node belongs to. Three cases and no fallback
/// chain: a unit when a manifest declared one, the source root the file's own
/// declaration implies when none did, and the file itself when it declares no
/// namespace at all.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Compilation {
    Unit(u32),
    Root(SmolStr),
    Alone(SmolStr),
    /// One mount forest, named by the file its tree is rooted at — a Rust
    /// crate, whose namespaces are its `mod` chain and nothing a path spells.
    Tree(SmolStr),
}

/// Each node's files, plus the files of every same-named node whose unit
/// compiles against this one's — what a language spanning the compilation
/// pools over. Nodes with no unit span nothing: without a manifest there is no
/// statement that two compilations meet.
fn span_nodes(
    graph: &Graph,
    files: &[Vec<u32>],
    unit_of_node: &[Option<u32>],
    segments_of_node: &[Vec<SmolStr>],
) -> Vec<Vec<u32>> {
    let mut by_segments: BTreeMap<&[SmolStr], Vec<usize>> = BTreeMap::new();
    for (n, segments) in segments_of_node.iter().enumerate() {
        if unit_of_node[n].is_some() {
            by_segments.entry(segments).or_default().push(n);
        }
    }
    (0..files.len())
        .map(|n| {
            let Some(mine) = unit_of_node[n] else {
                return files[n].clone();
            };
            let mut out = files[n].clone();
            for &other in by_segments
                .get(segments_of_node[n].as_slice())
                .into_iter()
                .flatten()
            {
                let theirs = unit_of_node[other].expect("grouped only units");
                if other != n && graph.project.sees_into(theirs, mine) {
                    out.extend_from_slice(&files[other]);
                }
            }
            out.sort_unstable();
            out
        })
        .collect()
}

/// Where this file's namespace hangs off the tree: its directory with the
/// namespace's own path removed. `guava/src/com/google/io/X.java` declaring
/// `com.google.io` roots at `guava/src`, and its Android mirror at
/// `android/guava/src` — one name, two compilations, and a package-private
/// name in one is not nameable from the other.
///
/// Derived from the file's OWN declaration minus its path, never from a table
/// of layouts: a directory that does not end in its namespace's path roots at
/// itself, which is the honest answer for a file the convention does not fit.
/// It is the shape a build UNIT will replace, and the reason a test set does
/// not yet pool with the main set it exercises: friendship is a manifest's
/// statement, and none has been read.
fn source_root(f: &crate::graph::GraphFile) -> SmolStr {
    let dir = f.path.as_str().rsplit_once('/').map_or("", |(d, _)| d);
    let mut suffix = String::new();
    for segment in &f.evidence.namespace {
        suffix.push('/');
        suffix.push_str(segment);
    }
    SmolStr::new(dir.strip_suffix(&suffix).unwrap_or(dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_contract::evidence::{EvidenceSink, EvidenceStreams};
    use kndo_contract::vocab::ProjectPath;

    /// A graph whose files each carry a unit, under a project assembled from
    /// the manifests the test spells: `(manifest, unit name, roots, needs)`.
    fn graph_with_units(
        manifests: &[(&str, &str, &[&str], &[&str])],
        aggregator: (&str, &[&str]),
        files: &[(&str, &[&str])],
    ) -> Graph {
        use kndo_contract::manifest::{ManifestEvidence, Unit, UnitKind};
        let mut reads = vec![crate::project::ManifestRead {
            manifest: ProjectPath::new(aggregator.0),
            evidence: ManifestEvidence {
                members: aggregator.1.iter().map(|m| ProjectPath::new(*m)).collect(),
                ..ManifestEvidence::default()
            },
            adapters: Vec::new(),
        }];
        for (manifest, name, roots, needs) in manifests {
            reads.push(crate::project::ManifestRead {
                manifest: ProjectPath::new(*manifest),
                evidence: ManifestEvidence {
                    units: vec![Unit {
                        name: SmolStr::new(*name),
                        kind: UnitKind::Library,
                        roots: roots.iter().map(|r| SmolStr::new(*r)).collect(),
                        excludes: Vec::new(),
                        entries: Vec::new(),
                        depends_on: needs.iter().map(|n| SmolStr::new(*n)).collect(),
                        friend_of: Vec::new(),
                        publication: Default::default(),
                    }],
                    ..ManifestEvidence::default()
                },
                adapters: Vec::new(),
            });
        }
        let project = crate::project::assemble(&reads);
        let mut graph = graph_of(files);
        for f in &mut graph.files {
            f.unit = project.unit_of(&f.path);
        }
        graph.project = project;
        graph
    }

    /// The one adapter of these tests, saying a namespace spans a compilation.
    fn spanning() -> Vec<(SmolStr, DeclaredCapabilities)> {
        vec![(
            SmolStr::new_static("test"),
            DeclaredCapabilities {
                namespace_span: NamespaceSpan::Compilation,
                ..Default::default()
            },
        )]
    }

    fn graph_of(files: &[(&str, &[&str])]) -> Graph {
        let mut out: Vec<crate::graph::GraphFile> = Vec::new();
        for (path, namespace) in files {
            let mut sink = EvidenceSink::new(0, EvidenceStreams::none());
            sink.namespace(namespace.iter().map(|s| SmolStr::new(*s)));
            out.push(crate::graph::GraphFile {
                path: ProjectPath::new(*path),
                adapter: SmolStr::new_static("test"),
                hash_hex: String::new(),
                evidence: sink.finish(),
                sees: Vec::new(),
                regions: Vec::new(),
                published: false,
                mounted_by: None,
                mount_cap: None,
                anchored: Vec::new(),
                dispatched: Vec::new(),
                exempt: Vec::new(),
                dispatch_notes: Vec::new(),
                generated: false,
                witnesses: Vec::new(),
                unit: None,
                imports: Vec::new(),
                import_targets: Vec::new(),
                unresolved_imports: 0,
            });
        }
        Graph {
            files: out,
            manifests: Vec::new(),
            manifest_declarations: Vec::new(),
            discovered: Vec::new(),
            packages: Vec::new(),
            project: Default::default(),
        }
    }

    #[test]
    fn a_namespace_is_its_name_under_its_source_root() {
        let graph = graph_of(&[
            (
                "guava/src/com/google/io/BaseEncoding.java",
                &["com", "google", "io"],
            ),
            (
                "guava/src/com/google/io/ByteStreams.java",
                &["com", "google", "io"],
            ),
            (
                "guava/src/com/google/base/Ascii.java",
                &["com", "google", "base"],
            ),
            // The Android flavor: one package name, another compilation.
            (
                "android/guava/src/com/google/io/BaseEncoding.java",
                &["com", "google", "io"],
            ),
        ]);
        let scopes = Scopes::build(&graph, &[]);
        assert_eq!(
            scopes.namespace_pool(0, 0),
            Some([0u32, 1].as_slice()),
            "the package under this root, and only it"
        );
        assert_eq!(
            scopes.namespace_pool(2, 0),
            Some([2u32].as_slice()),
            "another package is another node, however near its directory"
        );
        assert_eq!(
            scopes.namespace_pool(3, 0),
            Some([3u32].as_slice()),
            "a mirrored tree is a second compilation, not more of the first"
        );
    }

    #[test]
    fn a_layout_the_convention_does_not_fit_roots_at_its_own_directory() {
        // The path does not end in the package it declares — legal, and no
        // table of layouts is consulted: the file roots where it sits.
        let graph = graph_of(&[
            ("sources/Odd.java", &["com", "google", "io"]),
            ("sources/Even.java", &["com", "google", "io"]),
            ("elsewhere/Third.java", &["com", "google", "io"]),
        ]);
        let scopes = Scopes::build(&graph, &[]);
        assert_eq!(scopes.namespace_pool(0, 0), Some([0u32, 1].as_slice()));
        assert_eq!(scopes.namespace_pool(2, 0), Some([2u32].as_slice()));
    }

    #[test]
    fn a_file_declaring_no_namespace_pools_only_itself() {
        let graph = graph_of(&[("a.js", &[]), ("b.js", &[])]);
        let scopes = Scopes::build(&graph, &[]);
        assert_eq!(scopes.namespace_pool(0, 0), Some([0u32].as_slice()));
        assert_eq!(scopes.namespace_pool(1, 0), Some([1u32].as_slice()));
    }

    #[test]
    fn a_namespace_spans_the_units_compiled_against_it_when_the_language_says_so() {
        // guava's shape: the tests module is a SEPARATE artifact whose classes
        // declare the same package, and the two are compiled together. Its
        // Android twin declares the same names and is never on that classpath.
        let graph = graph_with_units(
            &[
                ("guava/pom.xml", "guava", &["guava"], &[]),
                (
                    "guava-tests/pom.xml",
                    "guava-tests",
                    &["guava-tests"],
                    &["guava"],
                ),
                ("android/guava/pom.xml", "guava", &["android/guava"], &[]),
            ],
            (
                "pom.xml",
                &[
                    "guava/pom.xml",
                    "guava-tests/pom.xml",
                    "android/guava/pom.xml",
                ],
            ),
            &[
                (
                    "guava/src/com/google/io/Files.java",
                    &["com", "google", "io"],
                ),
                (
                    "guava-tests/test/com/google/io/FilesTest.java",
                    &["com", "google", "io"],
                ),
                (
                    "android/guava/src/com/google/io/Files.java",
                    &["com", "google", "io"],
                ),
            ],
        );
        let scopes = Scopes::build(&graph, &spanning());
        assert_eq!(
            scopes.namespace_pool(0, 0),
            Some([0u32, 1].as_slice()),
            "the test module compiles against this one, so it may name it — \
             and no directory convention could have paired `src/` with `test/`"
        );
        assert_eq!(
            scopes.namespace_pool(2, 0),
            Some([2u32].as_slice()),
            "the mirror's compilation is entered by nothing here"
        );
        assert_eq!(
            scopes.namespace_pool(1, 0),
            Some([1u32].as_slice()),
            "seeing is directional: the library does not name its tests"
        );
    }

    #[test]
    fn a_namespace_stops_at_its_unit_where_a_language_has_not_spoken() {
        let graph = graph_with_units(
            &[
                ("guava/pom.xml", "guava", &["guava"], &[]),
                (
                    "guava-tests/pom.xml",
                    "guava-tests",
                    &["guava-tests"],
                    &["guava"],
                ),
            ],
            ("pom.xml", &["guava/pom.xml", "guava-tests/pom.xml"]),
            &[
                (
                    "guava/src/com/google/io/Files.java",
                    &["com", "google", "io"],
                ),
                (
                    "guava-tests/test/com/google/io/FilesTest.java",
                    &["com", "google", "io"],
                ),
            ],
        );
        // No capability declared: the default keeps the namespace inside its
        // unit, so the advisory the wider span would silence survives.
        let scopes = Scopes::build(&graph, &[]);
        assert_eq!(scopes.namespace_pool(0, 0), Some([0u32].as_slice()));
        assert_eq!(scopes.namespace_pool(1, 0), Some([1u32].as_slice()));
    }

    #[test]
    fn an_ancestor_namespace_is_unbounded_until_a_language_declares_its_nesting() {
        let graph = graph_of(&[("A.java", &["com", "foo"])]);
        let scopes = Scopes::build(&graph, &[]);
        assert_eq!(scopes.namespace_pool(0, 1), None);
    }
}
