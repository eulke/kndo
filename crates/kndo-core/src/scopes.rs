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
use std::collections::BTreeMap;

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
}

impl Scopes {
    pub fn build(graph: &Graph, capabilities: &[(SmolStr, DeclaredCapabilities)]) -> Scopes {
        // A node is one namespace inside one COMPILATION: the unit that
        // compiles the file when a manifest named one, and otherwise the
        // source root its own declaration implies.
        let mut nodes: BTreeMap<(Compilation, Vec<SmolStr>), u32> = BTreeMap::new();
        let mut of_file: Vec<u32> = Vec::with_capacity(graph.files.len());
        let mut files: Vec<Vec<u32>> = Vec::new();
        let mut unit_of_node: Vec<Option<u32>> = Vec::new();
        let mut segments_of_node: Vec<Vec<SmolStr>> = Vec::new();
        for (i, f) in graph.files.iter().enumerate() {
            let key = if f.evidence.namespace.is_empty() {
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
            let node = *nodes.entry(key).or_insert_with(|| {
                files.push(Vec::new());
                unit_of_node.push(f.unit);
                segments_of_node.push(f.evidence.namespace.clone());
                (files.len() - 1) as u32
            });
            files[node as usize].push(i as u32);
            of_file.push(node);
        }
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
        }
    }

    /// The files a `Reach::Unit` declaration in `unit` pools over: the unit's
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
        if up > 0 {
            return None;
        }
        let node = self.of_file[file] as usize;
        Some(if self.spans[file] {
            &self.spanned[node]
        } else {
            &self.files[node]
        })
    }
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

/// Which compilation a namespace node belongs to. Three cases and no fallback
/// chain: a unit when a manifest declared one, the source root the file's own
/// declaration implies when none did, and the file itself when it declares no
/// namespace at all.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Compilation {
    Unit(u32),
    Root(SmolStr),
    Alone(SmolStr),
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
                anchored: Vec::new(),
                dispatched: Vec::new(),
                exempt: Vec::new(),
                dispatch_notes: Vec::new(),
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
