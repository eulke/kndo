//! The scope forest: where a name may legally be used, as structure rather than
//! as a path convention.
//!
//! Today it holds one layer — namespaces, from the clause each file declares
//! ([`kndo_contract::evidence::FileEvidence::namespace`]) — and that layer
//! already replaces what nine adapters used to compute from directory shapes.
//! guava is the case that proves the difference: its tests sit in
//! `guava-tests/test/...` while its sources sit in `guava/src/...`, so no
//! `src/main` ↔ `src/test` mirror rule can pair them, yet both files declare
//! `package com.google.common.io` and therefore share one namespace.
//!
//! The units, friends and owners of the full forest land with the consumers
//! that read them; a layer with nobody asking is a field, not a structure.

use crate::graph::Graph;
use smol_str::SmolStr;
use std::collections::BTreeMap;

pub struct Scopes {
    /// file → the namespace node it declares itself into. Every file has one:
    /// a file declaring no namespace is a node of its own, so a
    /// namespace-reaching declaration there pools nothing beyond its file.
    of_file: Vec<u32>,
    /// namespace node → its files, ascending.
    files: Vec<Vec<u32>>,
}

impl Scopes {
    pub fn build(graph: &Graph) -> Scopes {
        let mut nodes: BTreeMap<(SmolStr, Vec<SmolStr>), u32> = BTreeMap::new();
        let mut of_file: Vec<u32> = Vec::with_capacity(graph.files.len());
        let mut files: Vec<Vec<u32>> = Vec::new();
        for (i, f) in graph.files.iter().enumerate() {
            let key = if f.evidence.namespace.is_empty() {
                // Its own node, named by nothing another file can spell.
                (SmolStr::new(f.path.as_str()), Vec::new())
            } else {
                (source_root(f), f.evidence.namespace.clone())
            };
            let node = *nodes.entry(key).or_insert_with(|| {
                files.push(Vec::new());
                (files.len() - 1) as u32
            });
            files[node as usize].push(i as u32);
            of_file.push(node);
        }
        Scopes { of_file, files }
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
        Some(&self.files[self.of_file[file] as usize])
    }
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
                scoped_regions: Vec::new(),
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
        let scopes = Scopes::build(&graph);
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
        let scopes = Scopes::build(&graph);
        assert_eq!(scopes.namespace_pool(0, 0), Some([0u32, 1].as_slice()));
        assert_eq!(scopes.namespace_pool(2, 0), Some([2u32].as_slice()));
    }

    #[test]
    fn a_file_declaring_no_namespace_pools_only_itself() {
        let graph = graph_of(&[("a.js", &[]), ("b.js", &[])]);
        let scopes = Scopes::build(&graph);
        assert_eq!(scopes.namespace_pool(0, 0), Some([0u32].as_slice()));
        assert_eq!(scopes.namespace_pool(1, 0), Some([1u32].as_slice()));
    }

    #[test]
    fn an_ancestor_namespace_is_unbounded_until_a_language_declares_its_nesting() {
        let graph = graph_of(&[("A.java", &["com", "foo"])]);
        let scopes = Scopes::build(&graph);
        assert_eq!(scopes.namespace_pool(0, 1), None);
    }
}
