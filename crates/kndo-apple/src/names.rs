//! Name → declaration, the one step both plugins share: an artifact names a
//! class as a bare string, and only the graph knows where — and whether — it
//! is declared.

use kndo_contract::evidence::SymbolKind;
use kndo_contract::extension::GraphAccess;
use kndo_contract::vocab::ProjectPath;
use std::collections::{BTreeMap, BTreeSet};

/// The graph's declarations as an artifact can name them: top-level types by
/// name, and each file's members by owner. Types only, top-level only, for a
/// class-naming string — an artifact names a class to instantiate, so a
/// same-named function or a nested type is not what it meant.
pub(crate) struct TypeIndex<'a> {
    types: BTreeMap<&'a str, Vec<&'a ProjectPath>>,
    members: BTreeMap<&'a ProjectPath, BTreeMap<&'a str, BTreeSet<&'a str>>>,
}

impl<'a> TypeIndex<'a> {
    pub(crate) fn build(graph: &'a dyn GraphAccess) -> Self {
        let mut types: BTreeMap<&'a str, Vec<&'a ProjectPath>> = BTreeMap::new();
        let mut members: BTreeMap<&'a ProjectPath, BTreeMap<&'a str, BTreeSet<&'a str>>> =
            BTreeMap::new();
        for d in graph.declarations() {
            match d.owner {
                None if *d.kind == SymbolKind::Type => {
                    let files = types.entry(d.name).or_default();
                    if files.last() != Some(&d.path) {
                        files.push(d.path);
                    }
                }
                None => {}
                Some(owner) => {
                    members
                        .entry(d.path)
                        .or_default()
                        .entry(owner)
                        .or_default()
                        .insert(d.name);
                }
            }
        }
        TypeIndex { types, members }
    }

    /// The files declaring a type named `name` CLOSEST to `artifact`: every
    /// match under the deepest ancestor directory of the artifact that holds
    /// one, or every match when none shares an ancestor short of the root.
    ///
    /// A name is all the artifact gives, one repository can declare it in
    /// several targets, and the graph knows no module to discriminate by —
    /// proximity is the one rule both sides of the question can see, because a
    /// target's artifacts and its sources share a directory subtree. The
    /// fallback is the keep-alive direction: with nothing to choose by, rooting
    /// every candidate can only keep something alive, never accuse.
    pub(crate) fn nearest(&self, name: &str, artifact: &ProjectPath) -> Vec<&'a ProjectPath> {
        let Some(candidates) = self.types.get(name) else {
            return Vec::new();
        };
        let mut dir = kndo_toolkit::parent_dir(artifact.as_str());
        loop {
            let near: Vec<&'a ProjectPath> = candidates
                .iter()
                .copied()
                .filter(|p| under(dir, p.as_str()))
                .collect();
            if !near.is_empty() {
                return near;
            }
            if dir.is_empty() {
                return candidates.clone();
            }
            dir = kndo_toolkit::parent_dir(dir);
        }
    }

    /// Whether `file` declares `member` under `owner` — the check that keeps a
    /// connection to a property the class does not declare (inherited, or
    /// stale in the document) from being asserted at all.
    pub(crate) fn declares_member(&self, file: &ProjectPath, owner: &str, member: &str) -> bool {
        self.members
            .get(file)
            .and_then(|owners| owners.get(owner))
            .is_some_and(|names| names.contains(member))
    }
}

fn under(dir: &str, path: &str) -> bool {
    dir.is_empty()
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_contract::extension::DeclaredSymbol;

    struct Graph {
        paths: Vec<ProjectPath>,
        declared: Vec<(ProjectPath, &'static str, SymbolKind, Option<&'static str>)>,
    }

    impl GraphAccess for Graph {
        fn paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_> {
            Box::new(self.paths.iter())
        }
        fn contains(&self, path: &ProjectPath) -> bool {
            self.paths.binary_search(path).is_ok()
        }
        fn declarations(&self) -> Box<dyn Iterator<Item = DeclaredSymbol<'_>> + '_> {
            Box::new(
                self.declared
                    .iter()
                    .map(|(path, name, kind, owner)| DeclaredSymbol {
                        path,
                        name,
                        kind,
                        owner: *owner,
                    }),
            )
        }
    }

    fn graph(declared: &[(&str, &'static str, SymbolKind, Option<&'static str>)]) -> Graph {
        let mut paths: Vec<ProjectPath> = declared
            .iter()
            .map(|(p, ..)| ProjectPath::new(*p))
            .collect();
        paths.sort();
        paths.dedup();
        Graph {
            paths,
            declared: declared
                .iter()
                .map(|(p, n, k, o)| (ProjectPath::new(*p), *n, k.clone(), *o))
                .collect(),
        }
    }

    #[test]
    fn the_nearest_declaration_wins_and_the_root_falls_back_to_all() {
        let g = graph(&[
            ("ios/App/Scene.swift", "Scene", SymbolKind::Type, None),
            ("mac/App/Scene.swift", "Scene", SymbolKind::Type, None),
            ("shared/Model.swift", "Model", SymbolKind::Type, None),
        ]);
        let index = TypeIndex::build(&g);
        let names = |files: Vec<&ProjectPath>| {
            files
                .iter()
                .map(|p| p.as_str().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(index.nearest("Scene", &ProjectPath::new("ios/Base.lproj/Main.storyboard"))),
            ["ios/App/Scene.swift"]
        );
        assert_eq!(
            names(index.nearest("Scene", &ProjectPath::new("elsewhere/Main.storyboard"))),
            ["ios/App/Scene.swift", "mac/App/Scene.swift"],
            "no shared ancestor: every candidate, in path order"
        );
        assert!(
            index
                .nearest("Missing", &ProjectPath::new("ios/Main.storyboard"))
                .is_empty()
        );
    }

    #[test]
    fn only_top_level_types_answer_a_class_name() {
        let g = graph(&[
            ("a.swift", "Outer", SymbolKind::Type, None),
            ("a.swift", "Inner", SymbolKind::Type, Some("Outer")),
            (
                "a.swift",
                "titleImageView",
                SymbolKind::Variable,
                Some("Outer"),
            ),
            ("b.swift", "Outer", SymbolKind::Function, None),
        ]);
        let index = TypeIndex::build(&g);
        let at = ProjectPath::new("Main.storyboard");
        assert_eq!(
            index.nearest("Outer", &at).len(),
            1,
            "the function in b.swift is not a class"
        );
        assert!(
            index.nearest("Inner", &at).is_empty(),
            "nested types are not instantiable by name"
        );
        let a = ProjectPath::new("a.swift");
        assert!(index.declares_member(&a, "Outer", "titleImageView"));
        assert!(!index.declares_member(&a, "Outer", "dataSource"));
        assert!(!index.declares_member(&ProjectPath::new("b.swift"), "Outer", "titleImageView"));
    }
}
