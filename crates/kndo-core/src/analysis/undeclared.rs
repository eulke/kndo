//! `undeclared` — an import resolves to a dependency the *importing file's own package*
//! doesn't declare (RFC 0005 §5, RFC 0011 §4's "phantom internal dependency" pattern
//! generalized to external deps too): a name that only resolves today because hoisting or
//! transitive resolution happens to make it reachable — the kind of thing that breaks on a
//! clean install elsewhere.
//!
//! Scoped per owning package via `ProjectGraph`'s ownership (RFC 0011 §3, nearest-manifest-
//! ancestor): a name declared by sibling package B no longer suppresses a real finding for
//! package A merely because both live in the same repo. In the common single-manifest case
//! every file owns the same package, so this collapses to the simpler global check it used to
//! be — no regression there, just correctness added for the monorepo case.

use std::collections::{HashMap, HashSet};

use crate::analysis::finding_id;
use crate::engine::Finding;
use crate::graph::{PackageNode, ProjectGraph};
use crate::vocab::{DependencyId, EdgeKind, PackageId};

pub fn find_undeclared_dependencies(graph: &ProjectGraph) -> Vec<Finding> {
    let mut declared_by_package: HashMap<PackageId, HashSet<&str>> = HashMap::new();
    for dep in &graph.declared_dependencies {
        declared_by_package
            .entry(dep.package)
            .or_default()
            .insert(dep.name.as_str());
    }

    let mut importers: HashMap<(DependencyId, PackageId), Vec<&str>> = HashMap::new();
    for edge in &graph.edges {
        if let EdgeKind::ImportsDependency { from, to } = edge.kind {
            let file = &graph.files[from.0 as usize];
            importers
                .entry((to, file.package))
                .or_default()
                .push(file.path.0.as_str());
        }
    }

    let mut findings = Vec::new();
    for (&(dep_id, package), files) in &importers {
        let name = graph.dependencies[dep_id.0 as usize].name.as_str();
        let declared = declared_by_package
            .get(&package)
            .is_some_and(|names| names.contains(name));
        if declared {
            continue;
        }
        let discriminator = package_discriminator(graph, package);
        findings.push(Finding {
            id: finding_id("undeclared", "dependency", name, "", &discriminator),
            category: "undeclared".to_string(),
            group: "defect".to_string(),
            subject_kind: "dependency".to_string(),
            message: format!(
                "{name} is imported but not declared in {}'s manifest (phantom dependency — likely resolving via hoisting/transitivity){}",
                package_label(graph, package),
                importer_summary(files)
            ),
        });
    }
    findings
}

/// The stable, empty-for-the-implicit-package identity used in the finding id — deliberately
/// *not* the human-readable label (which can be absent or a display name), so ids stay stable
/// across packages that share a name but not a manifest path.
fn package_discriminator(graph: &ProjectGraph, package: PackageId) -> String {
    match graph.packages[package.0 as usize].manifest.as_ref() {
        Some(path) => path.0.to_string(),
        None => String::new(),
    }
}

fn package_label(graph: &ProjectGraph, package: PackageId) -> String {
    match &graph.packages[package.0 as usize] {
        PackageNode {
            name: Some(name), ..
        } => name.to_string(),
        PackageNode {
            manifest: Some(path),
            ..
        } => path.0.to_string(),
        PackageNode { .. } => "the project (no manifest)".to_string(),
    }
}

fn importer_summary(importers: &[&str]) -> String {
    if importers.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<&str> = importers.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.len() <= 3 {
        format!(": imported by {}", sorted.join(", "))
    } else {
        format!(
            ": imported by {}, {}, {} and {} more",
            sorted[0],
            sorted[1],
            sorted[2],
            sorted.len() - 3
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::graph::{DeclaredDependency, DependencyNode, FileNode};
    use crate::vocab::{Confidence, DependencyScope, Edge, FileClass, FileId, Provenance};
    use smol_str::SmolStr;

    fn file(path: &str, package: PackageId) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass::default()),
            package,
        }
    }

    fn imports_dep_edge(from: FileId, to: DependencyId) -> Edge {
        Edge {
            kind: EdgeKind::ImportsDependency { from, to },
            confidence: Confidence::Certain,
            source: Provenance::Adapter(SmolStr::new("mock")),
        }
    }

    #[test]
    fn undeclared_dependency_is_reported() {
        let files = vec![file("a.ts", PackageId(0))];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("left-pad"),
        }];
        let edges = vec![imports_dep_edge(FileId(0), DependencyId(0))];
        let graph = ProjectGraph::for_test(files, vec![], dependencies, edges);
        let findings = find_undeclared_dependencies(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "undeclared");
        assert_eq!(findings[0].group, "defect");
        assert_eq!(findings[0].subject_kind, "dependency");
        assert!(findings[0].message.contains("left-pad"));
        assert!(findings[0].message.contains("a.ts"));
    }

    #[test]
    fn declared_dependency_is_not_reported() {
        let files = vec![file("a.ts", PackageId(0))];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("lodash"),
        }];
        let edges = vec![imports_dep_edge(FileId(0), DependencyId(0))];
        let graph = ProjectGraph::for_test(files, vec![], dependencies, edges)
            .with_declared_dependencies(vec![DeclaredDependency {
                package: PackageId(0),
                manifest: ProjectPath(SmolStr::new("package.json")),
                name: SmolStr::new("lodash"),
                version_req: SmolStr::new("^4.0.0"),
                scope: DependencyScope::Prod,
            }]);
        assert!(find_undeclared_dependencies(&graph).is_empty());
    }

    #[test]
    fn sibling_packages_own_declaration_does_not_shadow_the_other() {
        // Package 1 declares `chalk`; package 2 doesn't but imports it — RFC 0011 §3 ownership
        // must not let package 1's declaration paper over package 2's real phantom dependency.
        let files = vec![
            file("packages/a/index.ts", PackageId(1)),
            file("packages/b/index.ts", PackageId(2)),
        ];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("chalk"),
        }];
        let edges = vec![
            imports_dep_edge(FileId(0), DependencyId(0)),
            imports_dep_edge(FileId(1), DependencyId(0)),
        ];
        let packages = vec![
            PackageNode {
                manifest: None,
                name: None,
                private: false,
            },
            PackageNode {
                manifest: Some(ProjectPath(SmolStr::new("packages/a/package.json"))),
                name: Some(SmolStr::new("@demo/a")),
                private: true,
            },
            PackageNode {
                manifest: Some(ProjectPath(SmolStr::new("packages/b/package.json"))),
                name: Some(SmolStr::new("@demo/b")),
                private: true,
            },
        ];
        let graph = ProjectGraph::for_test(files, vec![], dependencies, edges)
            .with_packages(packages)
            .with_declared_dependencies(vec![DeclaredDependency {
                package: PackageId(1),
                manifest: ProjectPath(SmolStr::new("packages/a/package.json")),
                name: SmolStr::new("chalk"),
                version_req: SmolStr::new("^5.0.0"),
                scope: DependencyScope::Prod,
            }]);

        let findings = find_undeclared_dependencies(&graph);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("@demo/b"));
        assert!(findings[0].message.contains("packages/b/index.ts"));
    }

    #[test]
    fn finding_id_is_stable_across_runs() {
        let files = vec![file("a.ts", PackageId(0))];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("left-pad"),
        }];
        let edges = vec![imports_dep_edge(FileId(0), DependencyId(0))];
        let graph = ProjectGraph::for_test(files, vec![], dependencies, edges);
        let a = find_undeclared_dependencies(&graph);
        let b = find_undeclared_dependencies(&graph);
        assert_eq!(a[0].id, b[0].id);
    }
}
