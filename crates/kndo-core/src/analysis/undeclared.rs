//! `undeclared` — an import resolves to a dependency the *importing file's own package*
//! doesn't declare (the "phantom internal dependency" pattern
//! generalized to external deps too): a name that only resolves today because hoisting or
//! transitive resolution happens to make it reachable — the kind of thing that breaks on a
//! clean install elsewhere.
//!
//! Scoped per owning package via `ProjectGraph`'s ownership (nearest-manifest-
//! ancestor): a name declared by sibling package B never suppresses a real finding for
//! package A merely because both live in the same repo. In the common single-manifest case
//! every file owns the same package, so this collapses to a simple global
//! check — the per-package scoping only matters for the monorepo case.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::analysis::{finding_id, package_discriminator, package_label, FindingIdParts};
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{
    Category, Confidence, DependencyId, EdgeKind, FileOrigin, Group, PackageId, SubjectKind,
};

/// `strict` is `--strict`, which promotes this analysis from a warning to an error.
/// A phantom dependency is a build that works by accident — it resolves today through
/// hoisting or transitivity and breaks on a clean install elsewhere — so a project that opts
/// into strictness wants its build to say so rather than to warn about it.
///
/// Read here rather than promoted by a central post-pass: severity is part of what a verdict
/// means, and a table mapping categories to strict severities would be a second place to keep
/// in sync with the analysis that decides the ordinary one.
pub fn find_undeclared_dependencies(graph: &ProjectGraph, strict: bool) -> Vec<Finding> {
    let mut declared_by_package: HashMap<PackageId, HashSet<&str>> = HashMap::default();
    for dep in &graph.declared_dependencies {
        declared_by_package
            .entry(dep.package)
            .or_default()
            .insert(dep.name.as_str());
    }

    let mut importers: HashMap<(DependencyId, PackageId), Vec<&str>> = HashMap::default();
    for edge in &graph.edges {
        if let EdgeKind::ImportsDependency { from, to } = edge.kind {
            // Possible-tier dependency claims never accuse: that tier marks derived module
            // imports whose root was already covered by a `use` in scope (`use std::io;`
            // then `io::x::y` — the reconstructed `io::x` import exists for resolution
            // keep-alive, not as evidence anyone imports a crate named `io`). Certain and
            // Probable — real `use`/`import` statements and uncovered path roots — keep
            // accusing.
            if edge.confidence < Confidence::Probable {
                continue;
            }
            let file = &graph.files[from.0 as usize];
            // Origin exemption, the same two-level shape every sibling analysis applies
            // (`crap`, `duplicate`, `internal_only`, `private_type_leak`, `test_only`,
            // `untested`, `unused`, `cyclic`): a generated or vendored file's imports are not
            // its package's authored intent — nobody is going to add a dependency declaration
            // to satisfy a checked-in bundle.
            if file
                .class
                .is_some_and(|c| matches!(c.origin, FileOrigin::Generated | FileOrigin::Vendored))
            {
                continue;
            }
            // Whose declarations does this file answer to? Ownership is nearest-ancestor by
            // DIRECTORY, which is right everywhere except here: a Jazzy-generated `.js` under
            // `docs/` in a Swift repo, or a `web/app.js` beside a `go.mod`, is charged to
            // `Package.swift` / `go.mod` and accused of a dependency that manifest could never
            // have declared. Every Swift repo in the field audit reported a phantom `jquery`
            // this way, and it was the whole of hugo's `undeclared` column.
            if !graph.packages[file.package.0 as usize]
                .governs_dependencies_of(file.language.as_deref())
            {
                continue;
            }
            importers
                .entry((to, file.package))
                .or_default()
                .push(file.path.0.as_str());
        }
    }

    let mut findings = Vec::new();
    for (&(dep_id, package), files) in &importers {
        // The manifest has to be able to answer the question at all. When the claiming
        // adapter says its import specifiers don't structurally identify manifest coordinates
        // (Swift: `Package.swift` names a dependency's repository URL, never the module names
        // it exports — so `import Foo` can never be matched against a declaration), "not
        // declared here" is not evidence of anything. `dependency_hygiene` gates on this too,
        // so the two analyses agree about which packages can be judged by their declarations
        // at all.
        if !graph.packages[package.0 as usize].resolves_dependency_usage {
            continue;
        }
        let name = graph.dependencies[dep_id.0 as usize].name.as_str();
        let declared = declared_by_package
            .get(&package)
            .is_some_and(|names| names.contains(name));
        if declared {
            continue;
        }
        let discriminator = package_discriminator(graph, package);
        findings.push(Finding {
            advisory: false,
            id: finding_id(FindingIdParts {
                category: &Category::UNDECLARED,
                subject_kind: &SubjectKind::DEPENDENCY,
                path: name,
                symbol_path: "",
                discriminator: &discriminator,
            }),
            category: Category::UNDECLARED,
            group: Group::Defect,
            subject_kind: SubjectKind::DEPENDENCY,
            severity: match strict {
                true => Severity::Error,
                false => Severity::Warning,
            },
            confidence: Confidence::Certain,
            message: format!(
                "{name} is imported but not declared in {}'s manifest (phantom dependency — likely resolving via hoisting/transitivity){}",
                package_label(graph, package),
                importer_summary(files)
            ),
            location: Location {
                // The manifest that *should* declare it — the one real single-file anchor this
                // finding has, unlike version-skew/duplicate which genuinely span many files.
                path: graph.packages[package.0 as usize].manifest.clone(),
                range: None,
                symbol: Some(name.to_string()),
                package: graph.package_name(package).map(str::to_string),
            },
            related: Vec::new(),
            rolled_up: None,
            sources: Vec::new(),
            delta: None,
            delta_origin: None,
        });
    }
    findings
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
    use crate::graph::{DeclaredDependency, DependencyNode, FileNode, PackageNode};
    use crate::vocab::{
        Confidence, DependencyScope, Edge, FileClass, FileId, FileOrigin, Provenance,
    };
    use smol_str::SmolStr;

    fn file(path: &str, package: PackageId) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass::default()),
            package,
            unit: None,
            unit_parent: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
            string_attr_args: Vec::new(),
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

    #[test]
    fn undeclared_dependency_is_reported() {
        let files = vec![file("a.ts", PackageId(0))];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("left-pad"),
        }];
        let edges = vec![imports_dep_edge(FileId(0), DependencyId(0))];
        let graph = ProjectGraph::for_test(files, vec![], dependencies, edges);
        let findings = find_undeclared_dependencies(&graph, false);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "undeclared");
        assert_eq!(findings[0].group, crate::vocab::Group::Defect);
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
                version_req: Some(SmolStr::new("^4.0.0")),
                scope: DependencyScope::Prod,
            }]);
        assert!(find_undeclared_dependencies(&graph, false).is_empty());
    }

    #[test]
    fn a_package_whose_imports_cannot_name_declarations_is_never_accused() {
        // Swift's shape: `Package.swift` states a dependency's repository URL, never the module
        // names it exports, so `import Foo` can never be matched against a declaration and
        // "not declared here" is evidence of nothing. `dependency_hygiene` gates on
        // `resolves_dependency_usage` too, keeping the two analyses in agreement — without it,
        // every Swift repo in the field audit would report a phantom `jquery` dependency
        // against its `Package.swift`, because a Jazzy-generated `.js` file under `docs/` is
        // owned by the nearest manifest, which is the Swift one.
        let files = vec![file("docs/js/typeahead.jquery.js", PackageId(0))];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("jquery"),
        }];
        let edges = vec![imports_dep_edge(FileId(0), DependencyId(0))];
        let graph = ProjectGraph::for_test(files, vec![], dependencies, edges).with_packages(vec![
            PackageNode {
                manifest: Some(ProjectPath(SmolStr::new("Package.swift"))),
                name: Some(SmolStr::new("swift-pkg")),
                private: false,
                declares_surface: false,
                surface: Vec::new(),
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                resolves_dependency_usage: false,
                manifest_claim_languages: vec![SmolStr::new("mock")],
            },
        ]);
        assert!(find_undeclared_dependencies(&graph, false).is_empty());
    }

    #[test]
    fn a_generated_or_vendored_file_does_not_accuse_its_package() {
        // The same two-level origin exemption every sibling analysis applies: a checked-in
        // bundle's imports are not its package's authored intent — nobody adds a dependency
        // declaration to satisfy generated code.
        let dependencies = || {
            vec![DependencyNode {
                name: SmolStr::new("jquery"),
            }]
        };
        let edges = || vec![imports_dep_edge(FileId(0), DependencyId(0))];

        for origin in [FileOrigin::Generated, FileOrigin::Vendored] {
            let mut f = file("docs/js/bundle.js", PackageId(0));
            f.class = Some(FileClass {
                role: crate::vocab::FileRole::Production,
                origin,
            });
            let graph = ProjectGraph::for_test(vec![f], vec![], dependencies(), edges());
            assert!(
                find_undeclared_dependencies(&graph, false).is_empty(),
                "{origin:?} files must not accuse their package"
            );
        }

        // The authored control still reports — the exemption is about origin, not the path.
        let graph = ProjectGraph::for_test(
            vec![file("docs/js/bundle.js", PackageId(0))],
            vec![],
            dependencies(),
            edges(),
        );
        assert_eq!(find_undeclared_dependencies(&graph, false).len(), 1);
    }

    #[test]
    fn strict_promotes_the_verdict_to_error_and_changes_nothing_else() {
        // `--strict` promotes this analysis's severity from warning to error. A phantom
        // dependency is a build that works by accident; a project that opts into strictness
        // wants the build to say so. Everything else about the finding — its id above all —
        // must be identical, or `--strict` would silently invalidate baselines and suppressions.
        let graph = ProjectGraph::for_test(
            vec![file("docs/js/bundle.js", PackageId(0))],
            vec![],
            vec![DependencyNode {
                name: SmolStr::new("jquery"),
            }],
            vec![imports_dep_edge(FileId(0), DependencyId(0))],
        );
        let lenient = find_undeclared_dependencies(&graph, false);
        let strict = find_undeclared_dependencies(&graph, true);
        assert_eq!(lenient.len(), 1);
        assert_eq!(strict.len(), 1);
        assert_eq!(lenient[0].severity, Severity::Warning);
        assert_eq!(strict[0].severity, Severity::Error);
        assert_eq!(
            lenient[0].id, strict[0].id,
            "identity is a stability contract — severity is not part of it"
        );
        assert_eq!(lenient[0].message, strict[0].message);
    }

    #[test]
    fn sibling_packages_own_declaration_does_not_shadow_the_other() {
        // Package 1 declares `chalk`; package 2 doesn't but imports it — package ownership
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
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                manifest: None,
                name: None,
                private: false,
                declares_surface: false,
                surface: Vec::new(),
                resolves_dependency_usage: true,
                manifest_claim_languages: Vec::new(),
            },
            PackageNode {
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                manifest: Some(ProjectPath(SmolStr::new("packages/a/package.json"))),
                name: Some(SmolStr::new("@demo/a")),
                private: true,
                declares_surface: false,
                surface: Vec::new(),
                resolves_dependency_usage: true,
                manifest_claim_languages: vec![SmolStr::new("mock")],
            },
            PackageNode {
                workspace_entry: None,
                targets: Vec::new(),
                executables: Vec::new(),
                manifest: Some(ProjectPath(SmolStr::new("packages/b/package.json"))),
                name: Some(SmolStr::new("@demo/b")),
                private: true,
                declares_surface: false,
                surface: Vec::new(),
                resolves_dependency_usage: true,
                manifest_claim_languages: vec![SmolStr::new("mock")],
            },
        ];
        let graph = ProjectGraph::for_test(files, vec![], dependencies, edges)
            .with_packages(packages)
            .with_declared_dependencies(vec![DeclaredDependency {
                package: PackageId(1),
                manifest: ProjectPath(SmolStr::new("packages/a/package.json")),
                name: SmolStr::new("chalk"),
                version_req: Some(SmolStr::new("^5.0.0")),
                scope: DependencyScope::Prod,
            }]);

        let findings = find_undeclared_dependencies(&graph, false);
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
        let a = find_undeclared_dependencies(&graph, false);
        let b = find_undeclared_dependencies(&graph, false);
        assert_eq!(a[0].id, b[0].id);
    }
}
