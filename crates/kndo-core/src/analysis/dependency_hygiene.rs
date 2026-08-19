//! `unused`/`test-only` on declared dependencies (RFC 0005 §5): a `ManifestDependency`
//! classified by who, if anyone, actually imports it.
//!
//! | Importers (this dependency's own package) | Finding |
//! |---|---|
//! | none | `unused` — declared, never imported |
//! | only test-role files (`prod`/`optional` scope) | `test-only` — belongs in `devDependencies` |
//! | anything else | used — no finding |
//!
//! Scope rules from the RFC, applied literally:
//! - **`peer`** is exempt from both verdicts entirely — a peer dependency is a contract with
//!   the consumer, not a usage claim.
//! - **`dev`/`build`** only ever get `unused` (checked against *all* importers, test-role
//!   included) — `test-only` isn't a meaningful classification for a scope that is *supposed*
//!   to be test/tooling-only; that's not a misdeclaration, it's correct.
//! - **`optional`** gets the same two verdicts as `prod` but at `possible` confidence
//!   (runtime-conditional by design). The RFC says this belongs "below the default report
//!   floor" — there is no such floor/`--verbose` filtering built yet (M2+), so today these
//!   findings still show; demoting the confidence is the honest, available half of that rule.
//!
//! "Test-role" here means the importing file's own `FileClass::role == Test` (a purely
//! syntactic, already-available signal).
//!
//! **CLI-only dependencies.** A `scripts`-invoked tool (`"test": "xo && ava"`) never produces
//! an `ImportsDependency` edge — nothing `import`s a binary — so without a second signal every
//! CLI-only devDependency reads as declared-but-never-imported and gets falsely flagged
//! `unused`. `graph.script_invoked_dependencies` (built from each manifest's `scripts`, adapter
//! §7 open-question-2-adjacent) supplies it: a script-invoked name counts as one synthetic
//! `FileRole::Tooling` importer, folded into the same importer-roles classification table
//! everything else already goes through — not a parallel code path. Tooling-role "importers"
//! only ever push a dependency out of `unused`; they can never make it `test-only` (a script
//! invocation is never test-role), matching the "at least one production- or
//! tooling-reachable file" used-verdict RFC 0005 §5's table already states for real imports.

use std::collections::{HashMap, HashSet};

use crate::analysis::{finding_id, package_discriminator, package_label};
use crate::engine::{Finding, Location, Severity};
use crate::graph::{DeclaredDependency, ProjectGraph};
use crate::vocab::{Confidence, DependencyId, DependencyScope, EdgeKind, FileRole, PackageId};

pub fn find_dependency_hygiene(graph: &ProjectGraph) -> Vec<Finding> {
    let dep_id_by_name: HashMap<&str, DependencyId> = graph
        .dependencies
        .iter()
        .enumerate()
        .map(|(i, d)| (d.name.as_str(), DependencyId(i as u32)))
        .collect();

    let mut importer_roles: HashMap<(DependencyId, PackageId), Vec<FileRole>> = HashMap::new();
    for edge in &graph.edges {
        if let EdgeKind::ImportsDependency { from, to } = edge.kind {
            let file = &graph.files[from.0 as usize];
            let role = file.class.map(|c| c.role).unwrap_or(FileRole::Production);
            importer_roles
                .entry((to, file.package))
                .or_default()
                .push(role);
        }
    }

    let mut findings = Vec::new();
    let mut seen: HashSet<(PackageId, &str)> = HashSet::new();
    for dep in &graph.declared_dependencies {
        if dep.scope == DependencyScope::Peer {
            continue; // exempt entirely (RFC 0005 §5)
        }
        // Multiple manifest fields (or manifests) can redeclare the same name for the same
        // package (a real inconsistency `version-skew` already flags) — one hygiene verdict
        // per (package, name), not one per declaration.
        if !seen.insert((dep.package, dep.name.as_str())) {
            continue;
        }

        let mut roles: Vec<FileRole> = dep_id_by_name
            .get(dep.name.as_str())
            .and_then(|&id| importer_roles.get(&(id, dep.package)))
            .cloned()
            .unwrap_or_default();
        if graph
            .script_invoked_dependencies
            .contains(&(dep.package, dep.name.clone()))
        {
            roles.push(FileRole::Tooling);
        }
        let confidence = if dep.scope == DependencyScope::Optional {
            Confidence::Possible
        } else {
            Confidence::Certain
        };

        if roles.is_empty() {
            findings.push(unused_finding(graph, dep, confidence));
            continue;
        }

        let test_only_eligible =
            matches!(dep.scope, DependencyScope::Prod | DependencyScope::Optional);
        if test_only_eligible && roles.iter().all(|&r| r == FileRole::Test) {
            findings.push(test_only_finding(graph, dep, confidence));
        }
    }
    findings
}

fn unused_finding(
    graph: &ProjectGraph,
    dep: &DeclaredDependency,
    confidence: Confidence,
) -> Finding {
    let name = dep.name.as_str();
    Finding {
        id: finding_id(
            "unused",
            "dependency",
            name,
            "",
            &package_discriminator(graph, dep.package),
        ),
        category: "unused".to_string(),
        group: "waste".to_string(),
        subject_kind: "dependency".to_string(),
        severity: Severity::Warning,
        confidence,
        message: format!(
            "{name} is declared in {}'s manifest but never imported",
            package_label(graph, dep.package)
        ),
        location: dependency_location(graph, dep),
    }
}

fn test_only_finding(
    graph: &ProjectGraph,
    dep: &DeclaredDependency,
    confidence: Confidence,
) -> Finding {
    let name = dep.name.as_str();
    Finding {
        id: finding_id(
            "test-only",
            "dependency",
            name,
            "",
            &package_discriminator(graph, dep.package),
        ),
        category: "test-only".to_string(),
        group: "waste".to_string(),
        subject_kind: "dependency".to_string(),
        severity: Severity::Info, // RFC 0005 §3: info by default
        confidence,
        message: format!(
            "{name} is declared in {}'s manifest but only imported by test files — belongs in devDependencies",
            package_label(graph, dep.package)
        ),
        location: dependency_location(graph, dep),
    }
}

fn dependency_location(graph: &ProjectGraph, dep: &DeclaredDependency) -> Location {
    Location {
        path: Some(dep.manifest.clone()),
        range: None,
        symbol: Some(dep.name.to_string()),
        package: graph.package_name(dep.package).map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::graph::{DependencyNode, FileNode, PackageNode};
    use crate::vocab::{Edge, FileClass, FileId, Provenance};
    use smol_str::SmolStr;

    fn file(path: &str, role: FileRole) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass {
                role,
                origin: crate::vocab::FileOrigin::Authored,
            }),
            package: PackageId(0),
        }
    }

    fn imports_dep_edge(from: FileId, to: DependencyId) -> Edge {
        Edge {
            kind: EdgeKind::ImportsDependency { from, to },
            confidence: Confidence::Certain,
            source: Provenance::Adapter(SmolStr::new("mock")),
        }
    }

    fn declared(name: &str, scope: DependencyScope) -> DeclaredDependency {
        DeclaredDependency {
            package: PackageId(0),
            manifest: ProjectPath(SmolStr::new("package.json")),
            name: SmolStr::new(name),
            version_req: SmolStr::new("^1.0.0"),
            scope,
        }
    }

    fn graph_with(
        files: Vec<FileNode>,
        dependencies: Vec<DependencyNode>,
        edges: Vec<Edge>,
        declared_deps: Vec<DeclaredDependency>,
    ) -> ProjectGraph {
        ProjectGraph::for_test(files, vec![], dependencies, edges)
            .with_packages(vec![PackageNode {
                manifest: Some(ProjectPath(SmolStr::new("package.json"))),
                name: Some(SmolStr::new("demo")),
                private: true,
            }])
            .with_declared_dependencies(declared_deps)
    }

    #[test]
    fn never_imported_prod_dependency_is_unused() {
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("lodash", DependencyScope::Prod)],
        );
        let findings = find_dependency_hygiene(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "unused");
        assert_eq!(findings[0].group, "waste");
        assert_eq!(findings[0].subject_kind, "dependency");
        assert_eq!(findings[0].confidence, Confidence::Certain);
        assert!(findings[0].message.contains("lodash"));
    }

    #[test]
    fn imported_from_production_file_is_not_reported() {
        let files = vec![file("a.ts", FileRole::Production)];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("lodash"),
        }];
        let edges = vec![imports_dep_edge(FileId(0), DependencyId(0))];
        let graph = graph_with(
            files,
            dependencies,
            edges,
            vec![declared("lodash", DependencyScope::Prod)],
        );
        assert!(find_dependency_hygiene(&graph).is_empty());
    }

    #[test]
    fn prod_dependency_imported_only_by_test_files_is_test_only() {
        let files = vec![file("a.test.ts", FileRole::Test)];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("chai"),
        }];
        let edges = vec![imports_dep_edge(FileId(0), DependencyId(0))];
        let graph = graph_with(
            files,
            dependencies,
            edges,
            vec![declared("chai", DependencyScope::Prod)],
        );
        let findings = find_dependency_hygiene(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "test-only");
        assert_eq!(findings[0].group, "waste");
        assert_eq!(findings[0].severity, Severity::Info);
    }

    #[test]
    fn mixed_production_and_test_importers_is_used() {
        let files = vec![
            file("a.ts", FileRole::Production),
            file("a.test.ts", FileRole::Test),
        ];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("lodash"),
        }];
        let edges = vec![
            imports_dep_edge(FileId(0), DependencyId(0)),
            imports_dep_edge(FileId(1), DependencyId(0)),
        ];
        let graph = graph_with(
            files,
            dependencies,
            edges,
            vec![declared("lodash", DependencyScope::Prod)],
        );
        assert!(find_dependency_hygiene(&graph).is_empty());
    }

    #[test]
    fn dev_dependency_used_only_by_test_files_is_not_flagged() {
        // dev scope is *supposed* to be test/tooling-only — that is not a misdeclaration.
        let files = vec![file("a.test.ts", FileRole::Test)];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("vitest"),
        }];
        let edges = vec![imports_dep_edge(FileId(0), DependencyId(0))];
        let graph = graph_with(
            files,
            dependencies,
            edges,
            vec![declared("vitest", DependencyScope::Dev)],
        );
        assert!(find_dependency_hygiene(&graph).is_empty());
    }

    #[test]
    fn unimported_dev_dependency_is_still_unused() {
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("vitest", DependencyScope::Dev)],
        );
        let findings = find_dependency_hygiene(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "unused");
    }

    #[test]
    fn peer_dependency_is_always_exempt() {
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("react", DependencyScope::Peer)],
        );
        assert!(find_dependency_hygiene(&graph).is_empty());
    }

    #[test]
    fn unimported_optional_dependency_demotes_to_possible_confidence() {
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("fsevents", DependencyScope::Optional)],
        );
        let findings = find_dependency_hygiene(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].confidence, Confidence::Possible);
    }

    #[test]
    fn finding_id_is_stable_across_runs() {
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("lodash", DependencyScope::Prod)],
        );
        let a = find_dependency_hygiene(&graph);
        let b = find_dependency_hygiene(&graph);
        assert_eq!(a[0].id, b[0].id);
    }

    // ---------------------------------------------------------------- CLI-only dependencies

    #[test]
    fn script_invoked_dev_dependency_is_not_unused() {
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("xo", DependencyScope::Dev)],
        )
        .with_script_invoked_dependencies(vec![(PackageId(0), SmolStr::new("xo"))]);
        assert!(find_dependency_hygiene(&graph).is_empty());
    }

    #[test]
    fn script_invoked_prod_dependency_is_used_not_test_only() {
        // A CLI invocation is tooling-role evidence, never test-role — it must push the
        // dependency past `unused` without ever making it eligible for `test-only`.
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("prettier", DependencyScope::Prod)],
        )
        .with_script_invoked_dependencies(vec![(PackageId(0), SmolStr::new("prettier"))]);
        assert!(find_dependency_hygiene(&graph).is_empty());
    }

    #[test]
    fn unrelated_script_invocation_does_not_shadow_a_real_unused_dependency() {
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("lodash", DependencyScope::Prod)],
        )
        .with_script_invoked_dependencies(vec![(PackageId(0), SmolStr::new("tsc"))]);
        let findings = find_dependency_hygiene(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "unused");
    }

    #[test]
    fn script_invocation_in_a_sibling_package_does_not_cross_boundaries() {
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("xo", DependencyScope::Dev)],
        )
        .with_script_invoked_dependencies(vec![(PackageId(1), SmolStr::new("xo"))]);
        let findings = find_dependency_hygiene(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "unused");
    }

    #[test]
    fn test_role_importer_plus_script_invocation_is_used_not_test_only() {
        let files = vec![file("a.test.ts", FileRole::Test)];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("vitest"),
        }];
        let edges = vec![imports_dep_edge(FileId(0), DependencyId(0))];
        let graph = graph_with(
            files,
            dependencies,
            edges,
            vec![declared("vitest", DependencyScope::Prod)],
        )
        .with_script_invoked_dependencies(vec![(PackageId(0), SmolStr::new("vitest"))]);
        assert!(find_dependency_hygiene(&graph).is_empty());
    }
}
