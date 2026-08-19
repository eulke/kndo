//! `version-skew` — the same external dependency declared with diverging version requirements
//! across manifests (RFC 0005 §5): three packages pinning three `lodash` versions is an
//! inconsistency someone will debug eventually. Manifest-only, zero-config, and unaffected by
//! the monorepo-attribution gap `undeclared` carries: this is a pure manifest-to-manifest
//! comparison over `graph.declared_dependencies`, needs no import edge and no notion of which
//! package owns which file, so it doesn't need RFC 0011's `Package` node/ownership to be
//! correct — only to know *which* declaring manifests exist, which extraction already gives.

use std::collections::{BTreeMap, BTreeSet};

use crate::analysis::finding_id;
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::Confidence;

pub fn find_version_skew(graph: &ProjectGraph) -> Vec<Finding> {
    let mut by_name: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    for dep in &graph.declared_dependencies {
        by_name
            .entry(dep.name.as_str())
            .or_default()
            .push((dep.manifest.0.as_str(), dep.version_req.as_str()));
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
            id: finding_id("version-skew", "dependency", name, "", ""),
            category: "version-skew".to_string(),
            group: "defect".to_string(),
            subject_kind: "dependency".to_string(),
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: format!("{name} is declared with diverging version requirements: {evidence}"),
            location: Location {
                // Spans every declaring manifest — no single `path` is *the* location (the
                // message already lists all of them; `related` would express it properly and
                // isn't built yet), but the dependency's own name is a real, single fact.
                symbol: Some(name.to_string()),
                ..Location::default()
            },
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
            version_req: SmolStr::new(version_req),
            scope: DependencyScope::Prod,
        }
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
        assert_eq!(findings[0].group, "defect");
        assert_eq!(findings[0].subject_kind, "dependency");
        assert!(findings[0]
            .message
            .contains("packages/a/package.json (^4.0.0)"));
        assert!(findings[0]
            .message
            .contains("packages/b/package.json (^3.10.1)"));
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
