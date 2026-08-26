//! `unused`/`test-only` on declared dependencies: a `ManifestDependency`
//! classified by who, if anyone, actually imports it.
//!
//! | Importers (this dependency's own package) | Finding |
//! |---|---|
//! | none | `unused` — declared, never imported |
//! | only test-role files (`prod`/`optional` scope) | `test-only` — belongs in `devDependencies` |
//! | anything else | used — no finding |
//!
//! Scope rules, applied literally:
//! - **`peer`** is exempt from both verdicts entirely — a peer dependency is a contract with
//!   the consumer, not a usage claim.
//! - **`dev`/`build`** only ever get `unused` (checked against *all* importers, test-role
//!   included) — `test-only` isn't a meaningful classification for a scope that is *supposed*
//!   to be test/tooling-only; that's not a misdeclaration, it's correct.
//! - **`optional`** gets the same two verdicts as `prod` but at `possible` confidence
//!   (runtime-conditional by design). These verdicts belong "below the default report
//!   floor" — no such floor/`--verbose` filtering is built, so these
//!   findings still show; demoting the confidence is the honest, available half of that rule.
//!
//! "Test-role" here means the importing file's own `FileClass::role == Test` (a purely
//! syntactic, already-available signal) — or, for production files with sub-file test
//! regions (`FileFacts::test_spans`), an import whose *site* sits inside such a region: a
//! `prod`-scoped dependency consumed only under `#[cfg(test)]` gates belongs in
//! dev-dependencies exactly as if the imports lived in test files.
//!
//! **CLI-only dependencies.** A `scripts`-invoked tool (`"test": "xo && ava"`) never produces
//! an `ImportsDependency` edge — nothing `import`s a binary — so without a second signal every
//! CLI-only devDependency reads as declared-but-never-imported and gets falsely flagged
//! `unused`. `graph.script_invoked_dependencies` (built from each manifest's
//! `scripts`) supplies it: a script-invoked name counts as one synthetic
//! `FileRole::Tooling` importer, folded into the same importer-roles classification table
//! everything else already goes through — not a parallel code path. Tooling-role "importers"
//! only ever push a dependency out of `unused`; they can never make it `test-only` (a script
//! invocation is never test-role), matching the "at least one production- or
//! tooling-reachable file" used-verdict the table already states for real imports.
//!
//! **Languages that can't resolve dependency usage** (`PackageNode::resolves_dependency_usage`
//! — Java: an import's package has no reliable mapping to its Maven/Gradle coordinate without
//! resolving the classpath, which kndo structurally never does). Their packages are skipped
//! entirely, with one diagnostic per run rather than a false-positive `unused` flood — a
//! dependency graph.declared_dependencies knows about but has zero recorded usage evidence for
//! is not a meaningful "unused" claim when usage evidence can never exist in the first place.
//! `version-skew` is unaffected: it compares declared versions across manifests directly, no
//! usage edge needed, so it stays fully precise for every language.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::adapter::{Diagnostic, DiagnosticLevel};
use crate::analysis::{finding_id, package_discriminator, package_label, FindingIdParts};
use crate::engine::{Finding, Location, Severity};
use crate::graph::{DeclaredDependency, ProjectGraph};
use crate::vocab::{
    Category, Confidence, DependencyId, DependencyScope, EdgeKind, FileRole, Group, PackageId,
    SubjectKind,
};

/// Findings plus, when at least one declared dependency belongs to a package whose language
/// can't produce usage edges, the one diagnostic naming how many were skipped instead of a
/// false `unused` per dependency.
pub fn find_dependency_hygiene(graph: &ProjectGraph) -> (Vec<Finding>, Option<Diagnostic>) {
    let dep_id_by_name: HashMap<&str, DependencyId> = graph
        .dependencies
        .iter()
        .enumerate()
        .map(|(i, d)| (d.name.as_str(), DependencyId(i as u32)))
        .collect();

    let mut importer_roles: HashMap<(DependencyId, PackageId), Vec<FileRole>> = HashMap::default();
    for edge in &graph.edges {
        if let EdgeKind::ImportsDependency { from, to } = edge.kind {
            let file = &graph.files[from.0 as usize];
            let mut role = file.class.map(|c| c.role).unwrap_or(FileRole::Production);
            // Sub-file test regions (FileFacts::test_spans): an import whose site sits
            // inside a `#[cfg(test)]` region is a test-role usage even though its file is
            // production — a prod-scoped dependency consumed only under test gates is a
            // `test-only dependency`, exactly as if the imports lived in test files.
            if role == FileRole::Production
                && edge
                    .span
                    .is_some_and(|s| crate::graph::span_in_test_region(&file.test_spans, s))
            {
                role = FileRole::Test;
            }
            // The same ownership question `undeclared` asks, from the other side: a file whose
            // own adapter would never claim this manifest is not evidence about its
            // declarations — neither that one is missing, nor that one is used. Without the
            // symmetry a `web/app.js` beside a `go.mod` could keep a Go dependency "used".
            if !graph.packages[file.package.0 as usize]
                .governs_dependencies_of(file.language.as_deref())
            {
                continue;
            }
            importer_roles
                .entry((to, file.package))
                .or_default()
                .push(role);
        }
    }

    let mut findings = Vec::new();
    let mut skipped = 0u32;
    let mut seen: HashSet<(PackageId, &str)> = HashSet::default();
    for dep in &graph.declared_dependencies {
        if dep.scope == DependencyScope::Peer {
            continue; // exempt entirely
        }
        // Multiple manifest fields (or manifests) can redeclare the same name for the same
        // package (a real inconsistency `version-skew` already flags) — one hygiene verdict
        // per (package, name), not one per declaration.
        if !seen.insert((dep.package, dep.name.as_str())) {
            continue;
        }
        if !graph.packages[dep.package.0 as usize].resolves_dependency_usage {
            skipped += 1;
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

    let diagnostic = (skipped > 0).then(|| Diagnostic {
        level: DiagnosticLevel::Info,
        path: None,
        message: format!(
            "dependency-hygiene: {skipped} declared dependenc{} skipped (unused/test-only) — \
             this language's imports don't map to manifest coordinates without resolving the \
             classpath, so \"no usage evidence\" isn't a meaningful unused claim",
            if skipped == 1 { "y" } else { "ies" }
        ),
        span: None,
    });
    (findings, diagnostic)
}

fn unused_finding(
    graph: &ProjectGraph,
    dep: &DeclaredDependency,
    confidence: Confidence,
) -> Finding {
    let name = dep.name.as_str();
    Finding {
        advisory: false,
        id: finding_id(FindingIdParts {
            category: &Category::UNUSED,
            subject_kind: &SubjectKind::DEPENDENCY,
            path: name,
            symbol_path: "",
            discriminator: &package_discriminator(graph, dep.package),
        }),
        category: Category::UNUSED,
        group: Group::Waste,
        subject_kind: SubjectKind::DEPENDENCY,
        severity: Severity::Warning,
        confidence,
        message: format!(
            "{name} is declared in {}'s manifest but never imported",
            package_label(graph, dep.package)
        ),
        location: dependency_location(graph, dep),
        related: Vec::new(),
        delta: None,
        delta_origin: None,
    }
}

fn test_only_finding(
    graph: &ProjectGraph,
    dep: &DeclaredDependency,
    confidence: Confidence,
) -> Finding {
    let name = dep.name.as_str();
    Finding {
        advisory: false,
        id: finding_id(FindingIdParts {
            category: &Category::TEST_ONLY,
            subject_kind: &SubjectKind::DEPENDENCY,
            path: name,
            symbol_path: "",
            discriminator: &package_discriminator(graph, dep.package),
        }),
        category: Category::TEST_ONLY,
        group: Group::Waste,
        subject_kind: SubjectKind::DEPENDENCY,
        severity: Severity::Info, // info by default
        confidence,
        message: format!(
            "{name} is declared in {}'s manifest but only imported by test files — belongs in devDependencies",
            package_label(graph, dep.package)
        ),
        location: dependency_location(graph, dep),
        related: Vec::new(),
        delta: None,
        delta_origin: None,
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
            unit: None,
            unit_parent: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
        }
    }

    fn imports_dep_edge(from: FileId, to: DependencyId) -> Edge {
        Edge {
            owner: crate::vocab::FileId(0),
            kind: EdgeKind::ImportsDependency { from, to },
            confidence: Confidence::Certain,
            source: Provenance::Adapter(SmolStr::new("mock")),
            span: None,
        }
    }

    fn declared(name: &str, scope: DependencyScope) -> DeclaredDependency {
        DeclaredDependency {
            package: PackageId(0),
            manifest: ProjectPath(SmolStr::new("package.json")),
            name: SmolStr::new(name),
            version_req: Some(SmolStr::new("^1.0.0")),
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
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                manifest: Some(ProjectPath(SmolStr::new("package.json"))),
                name: Some(SmolStr::new("demo")),
                private: true,
                declares_surface: false,
                surface: Vec::new(),
                resolves_dependency_usage: true,
                // The manifest these files' own adapter claims — without it the
                // cross-language ownership gate would (correctly) refuse to read any of
                // them as evidence about this package's declarations.
                manifest_claim_languages: vec![SmolStr::new("mock")],
            }])
            .with_declared_dependencies(declared_deps)
    }

    #[test]
    fn prod_dependency_imported_only_inside_a_test_region_is_test_only() {
        // The import lives in a production FILE, but its site sits inside a
        // `#[cfg(test)]` region (FileFacts::test_spans) — the usage is test-role.
        let mut importer = file("src/lib.rs", FileRole::Production);
        importer.test_spans = vec![crate::adapter::Span {
            start: (10, 1),
            end: (30, 999),
        }];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("insta"),
        }];
        let mut edge = imports_dep_edge(FileId(0), DependencyId(0));
        edge.span = Some(crate::adapter::Span {
            start: (12, 1),
            end: (12, 20),
        });
        let graph = graph_with(
            vec![importer],
            dependencies,
            vec![edge],
            vec![declared("insta", DependencyScope::Prod)],
        );
        let findings = find_dependency_hygiene(&graph).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "test-only");

        // Same shape with the site OUTSIDE the region: used, no finding.
        let mut importer = file("src/lib.rs", FileRole::Production);
        importer.test_spans = vec![crate::adapter::Span {
            start: (10, 1),
            end: (30, 999),
        }];
        let mut edge = imports_dep_edge(FileId(0), DependencyId(0));
        edge.span = Some(crate::adapter::Span {
            start: (2, 1),
            end: (2, 20),
        });
        let graph = graph_with(
            vec![importer],
            vec![DependencyNode {
                name: SmolStr::new("insta"),
            }],
            vec![edge],
            vec![declared("insta", DependencyScope::Prod)],
        );
        assert!(find_dependency_hygiene(&graph).0.is_empty());
    }

    #[test]
    fn a_language_that_cannot_resolve_dependency_usage_is_skipped_with_a_diagnostic() {
        // Java's shape: PackageNode::resolves_dependency_usage is false, so zero usage
        // evidence is NOT a meaningful `unused` claim — the dependency is skipped, and the
        // run gets one informational diagnostic instead of a false-positive finding.
        let graph = ProjectGraph::for_test(vec![], vec![], vec![], vec![])
            .with_packages(vec![PackageNode {
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                manifest: Some(ProjectPath(SmolStr::new("pom.xml"))),
                name: Some(SmolStr::new("demo")),
                private: true,
                declares_surface: false,
                surface: Vec::new(),
                resolves_dependency_usage: false,
                manifest_claim_languages: Vec::new(),
            }])
            .with_declared_dependencies(vec![declared("guava", DependencyScope::Prod)]);
        let (findings, diagnostic) = find_dependency_hygiene(&graph);
        assert!(findings.is_empty(), "no false unused claim");
        let diagnostic = diagnostic.expect("a skip diagnostic");
        assert!(diagnostic.message.contains('1'));
        assert!(diagnostic.message.contains("skipped"));
    }

    #[test]
    fn never_imported_prod_dependency_is_unused() {
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("lodash", DependencyScope::Prod)],
        );
        let findings = find_dependency_hygiene(&graph).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "unused");
        assert_eq!(findings[0].group, crate::vocab::Group::Waste);
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
        assert!(find_dependency_hygiene(&graph).0.is_empty());
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
        let findings = find_dependency_hygiene(&graph).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "test-only");
        assert_eq!(findings[0].group, crate::vocab::Group::Waste);
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
        assert!(find_dependency_hygiene(&graph).0.is_empty());
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
        assert!(find_dependency_hygiene(&graph).0.is_empty());
    }

    #[test]
    fn unimported_dev_dependency_is_still_unused() {
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("vitest", DependencyScope::Dev)],
        );
        let findings = find_dependency_hygiene(&graph).0;
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
        assert!(find_dependency_hygiene(&graph).0.is_empty());
    }

    #[test]
    fn unimported_optional_dependency_demotes_to_possible_confidence() {
        let graph = graph_with(
            vec![],
            vec![],
            vec![],
            vec![declared("fsevents", DependencyScope::Optional)],
        );
        let findings = find_dependency_hygiene(&graph).0;
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
        let a = find_dependency_hygiene(&graph).0;
        let b = find_dependency_hygiene(&graph).0;
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
        assert!(find_dependency_hygiene(&graph).0.is_empty());
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
        assert!(find_dependency_hygiene(&graph).0.is_empty());
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
        let findings = find_dependency_hygiene(&graph).0;
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
        let findings = find_dependency_hygiene(&graph).0;
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
        assert!(find_dependency_hygiene(&graph).0.is_empty());
    }
}
