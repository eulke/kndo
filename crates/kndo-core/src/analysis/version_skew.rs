//! `version-skew` — the same external dependency declared with diverging version requirements
//! across manifests: three packages pinning three `lodash` versions is an
//! inconsistency someone will debug eventually. Manifest-only, zero-config, and unaffected by
//! the monorepo-attribution gap `undeclared` carries: this is a pure manifest-to-manifest
//! comparison over `graph.declared_dependencies`, needs no import edge and no notion of which
//! package owns which file, so it doesn't need the `Package` node/ownership to be
//! correct — only to know *which* declaring manifests exist, which extraction already gives.

use std::collections::{BTreeMap, BTreeSet};

use crate::analysis::{finding_id, FindingIdParts};
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{Category, Confidence, Group, SubjectKind};

pub fn find_version_skew(graph: &ProjectGraph) -> Vec<Finding> {
    let mut by_name: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    for dep in &graph.declared_dependencies {
        // A manifest that states no comparable requirement — a BOM/platform-managed JVM
        // coordinate, a Cargo path dependency, a workspace inheritance no pool resolved — is
        // not evidence of anything here. It used to arrive as `"*"` and diverge from every
        // real version, which is how spring-petclinic, mockito, Exposed, koin and
        // kotlinx.coroutines each drew skew findings over dependencies that agree perfectly.
        // Silence is the only honest reading: a comparison the code knows it could not
        // perform must not produce a `certain` finding.
        let Some(version) = &dep.version_req else {
            continue;
        };
        by_name
            .entry(dep.name.as_str())
            .or_default()
            .push((dep.manifest.0.as_str(), version.as_str()));
    }

    let mut findings = Vec::new();
    for (name, mut declarations) in by_name {
        let distinct_versions: BTreeSet<&str> = declarations.iter().map(|&(_, v)| v).collect();
        if distinct_versions.len() <= 1 {
            continue; // one manifest, or several agreeing on the same requirement — no skew
        }
        declarations.sort_unstable();
        declarations.dedup();
        let evidence = declarations
            .iter()
            .map(|(manifest, version)| format!("{manifest} ({version})"))
            .collect::<Vec<_>>()
            .join(", ");
        findings.push(Finding {
            advisory: false,
            id: finding_id(FindingIdParts {
                category: &Category::VERSION_SKEW,
                subject_kind: &SubjectKind::DEPENDENCY,
                path: name,
                symbol_path: "",
                discriminator: "",
            }),
            category: Category::VERSION_SKEW,
            group: Group::Defect,
            subject_kind: SubjectKind::DEPENDENCY,
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: format!("{name} is declared with diverging version requirements: {evidence}"),
            location: Location {
                // Anchored on the lexicographically-first declaring manifest, with every
                // declaration — that one included — in `related`, each noting the requirement
                // it states. The dependency's own name is the single real fact about the
                // subject, so it stays the `symbol`; the anchor makes the finding addressable
                // without asking a consumer to parse the message for a path.
                path: Some(crate::adapter::ProjectPath(smol_str::SmolStr::new(
                    declarations[0].0,
                ))),
                symbol: Some(name.to_string()),
                ..Location::default()
            },
            related: declarations
                .iter()
                .map(|(manifest, version)| crate::engine::RelatedLocation {
                    role: "declaration".to_string(),
                    path: crate::adapter::ProjectPath(smol_str::SmolStr::new(*manifest)),
                    range: None,
                    note: Some((*version).to_string()),
                })
                .collect(),
            delta: None,
            delta_origin: None,
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::graph::DeclaredDependency;
    use crate::vocab::DependencyScope;
    use smol_str::SmolStr;

    fn declared(manifest: &str, name: &str, version_req: &str) -> DeclaredDependency {
        DeclaredDependency {
            package: crate::vocab::PackageId(0),
            manifest: ProjectPath(SmolStr::new(manifest)),
            name: SmolStr::new(name),
            version_req: Some(SmolStr::new(version_req)),
            scope: DependencyScope::Prod,
        }
    }

    fn unknown(manifest: &str, name: &str) -> DeclaredDependency {
        DeclaredDependency {
            version_req: None,
            ..declared(manifest, name, "")
        }
    }

    #[test]
    fn a_manifest_stating_no_requirement_is_not_evidence_of_skew() {
        // The BOM-managed shape: one module pins the version, the others take it from an
        // imported BOM. They agree perfectly. Encoding "states nothing" as `"*"` made every
        // real version diverge from it — a finding on every JVM repository in the field audit.
        let graph = ProjectGraph::for_test(vec![], vec![], vec![], vec![])
            .with_declared_dependencies(vec![
                declared(
                    "app/build.gradle",
                    "org.springframework.boot:starter",
                    "3.2.0",
                ),
                unknown("web/build.gradle", "org.springframework.boot:starter"),
                unknown("api/build.gradle", "org.springframework.boot:starter"),
            ]);
        assert!(find_version_skew(&graph).is_empty());
    }

    #[test]
    fn two_known_versions_still_skew_with_an_unknown_alongside() {
        // The unknown is dropped, not treated as agreement: a real disagreement between the
        // two manifests that DID state a requirement is still a finding.
        let graph = ProjectGraph::for_test(vec![], vec![], vec![], vec![])
            .with_declared_dependencies(vec![
                declared("a/build.gradle", "com.other:lib", "1.0"),
                declared("b/build.gradle", "com.other:lib", "2.0"),
                unknown("c/build.gradle", "com.other:lib"),
            ]);
        let findings = find_version_skew(&graph);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("1.0"));
        assert!(findings[0].message.contains("2.0"));
        assert!(
            !findings[0].message.contains("c/build.gradle"),
            "the manifest that states nothing is not cited as evidence: {}",
            findings[0].message
        );
    }

    #[test]
    fn single_manifest_never_skews() {
        let graph = ProjectGraph::for_test(vec![], vec![], vec![], vec![])
            .with_declared_dependencies(vec![declared("package.json", "lodash", "^4.0.0")]);
        assert!(find_version_skew(&graph).is_empty());
    }

    #[test]
    fn agreeing_manifests_do_not_skew() {
        let graph = ProjectGraph::for_test(vec![], vec![], vec![], vec![])
            .with_declared_dependencies(vec![
                declared("packages/a/package.json", "lodash", "^4.0.0"),
                declared("packages/b/package.json", "lodash", "^4.0.0"),
            ]);
        assert!(find_version_skew(&graph).is_empty());
    }

    #[test]
    fn diverging_manifests_skew() {
        let graph = ProjectGraph::for_test(vec![], vec![], vec![], vec![])
            .with_declared_dependencies(vec![
                declared("packages/a/package.json", "lodash", "^4.0.0"),
                declared("packages/b/package.json", "lodash", "^3.10.1"),
            ]);
        let findings = find_version_skew(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "version-skew");
        assert_eq!(findings[0].group, crate::vocab::Group::Defect);
        assert_eq!(findings[0].subject_kind, "dependency");
        assert!(findings[0]
            .message
            .contains("packages/a/package.json (^4.0.0)"));
        assert!(findings[0]
            .message
            .contains("packages/b/package.json (^3.10.1)"));

        // Addressable without parsing the message: anchored on the first declaring manifest,
        // with every declaration in `related` carrying the requirement it states.
        assert_eq!(
            findings[0].location.path.as_ref().map(|p| p.0.as_str()),
            Some("packages/a/package.json")
        );
        assert_eq!(findings[0].location.symbol.as_deref(), Some("lodash"));
        let related: Vec<(&str, Option<&str>)> = findings[0]
            .related
            .iter()
            .map(|r| (r.path.0.as_str(), r.note.as_deref()))
            .collect();
        assert_eq!(
            related,
            vec![
                ("packages/a/package.json", Some("^4.0.0")),
                ("packages/b/package.json", Some("^3.10.1")),
            ]
        );
        assert!(findings[0].related.iter().all(|r| r.role == "declaration"));
    }

    #[test]
    fn unrelated_names_do_not_cross_contaminate() {
        let graph = ProjectGraph::for_test(vec![], vec![], vec![], vec![])
            .with_declared_dependencies(vec![
                declared("packages/a/package.json", "lodash", "^4.0.0"),
                declared("packages/b/package.json", "react", "^18.0.0"),
            ]);
        assert!(find_version_skew(&graph).is_empty());
    }

    #[test]
    fn finding_id_is_stable_across_runs() {
        let graph = ProjectGraph::for_test(vec![], vec![], vec![], vec![])
            .with_declared_dependencies(vec![
                declared("packages/a/package.json", "lodash", "^4.0.0"),
                declared("packages/b/package.json", "lodash", "^3.10.1"),
            ]);
        let a = find_version_skew(&graph);
        let b = find_version_skew(&graph);
        assert_eq!(a[0].id, b[0].id);
    }
}
