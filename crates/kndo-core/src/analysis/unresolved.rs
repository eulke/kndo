//! `unresolved` — a relative import specifier that points at no file.
//!
//! RFC 0005 §5 specified this category, `docs/src/rules.md` published it, and the suppression
//! registry accepted it in a pragma — while nothing emitted it and `graph::assemble` dropped
//! `Resolution::Unresolved` on the floor with a comment saying a future analysis would want it.
//! A broken relative path was therefore invisible: no edge, no finding, and whatever the import
//! would have kept alive silently reported `unused` instead.
//!
//! What makes it reportable at all is that the *adapter* already classified the specifier
//! ([`crate::adapter::ImportKind`]). A `Package` specifier resolving to nothing is the
//! declaration contract's business (`undeclared`) and is never reported twice; a `Relative` one
//! is a path the adapter understood and could not follow, which is almost always a rename that
//! missed a call site. Severity `error`: this is a defect, not waste.
//!
//! Confidence is the import's own. An adapter that could only partly read a dynamic specifier
//! reports it below `Certain`, and the finding inherits that — a templated `import(…)` is not
//! evidence of a broken path, and lands under the default report floor.

use crate::analysis::{finding_id, FindingIdParts};
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{Category, FileOrigin, Group, SubjectKind};

pub fn find_unresolved(graph: &ProjectGraph) -> Vec<Finding> {
    graph
        .unresolved_imports
        .iter()
        .filter(|(file, _)| {
            let f = &graph.files[file.0 as usize];
            // An unclaimed file has no adapter and so no resolution to have failed; generated
            // and vendored files are not the project's to fix, the same exemption every other
            // analysis applies.
            f.class
                .is_some_and(|c| !matches!(c.origin, FileOrigin::Generated | FileOrigin::Vendored))
        })
        .map(|(file, u)| {
            let path = &graph.files[file.0 as usize].path;
            Finding {
                advisory: false,
                id: finding_id(FindingIdParts {
                    category: &Category::UNRESOLVED,
                    subject_kind: &SubjectKind::IMPORT,
                    path: path.0.as_str(),
                    symbol_path: u.specifier.as_str(),
                    discriminator: "",
                }),
                category: Category::UNRESOLVED,
                group: Group::Defect,
                subject_kind: SubjectKind::IMPORT,
                severity: Severity::Error,
                confidence: u.confidence,
                message: format!(
                    "{}'s import of '{}' resolves to no file — a broken path or a rename that \
                     missed this call site",
                    path.0, u.specifier
                ),
                location: Location {
                    path: Some(path.clone()),
                    range: Some(u.span),
                    symbol: None,
                    package: graph
                        .package_name(graph.files[file.0 as usize].package)
                        .map(str::to_string),
                },
                related: Vec::new(),
                delta: None,
                delta_origin: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ProjectPath, Span};
    use crate::graph::{FileNode, UnresolvedImport};
    use crate::vocab::FileId;
    use crate::vocab::{Confidence, FileClass, FileRole};
    use smol_str::SmolStr;

    fn file(path: &str, origin: FileOrigin) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass {
                role: FileRole::Production,
                origin,
            }),
            package: crate::vocab::PackageId(0),
            unit: None,
            unit_parent: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
        }
    }

    fn unresolved(specifier: &str, confidence: Confidence) -> UnresolvedImport {
        UnresolvedImport {
            specifier: SmolStr::new(specifier),
            span: Span {
                start: (3, 1),
                end: (3, 20),
            },
            confidence,
        }
    }

    fn graph_with(files: Vec<FileNode>, imports: Vec<(FileId, UnresolvedImport)>) -> ProjectGraph {
        let mut g = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        g.unresolved_imports = imports;
        g
    }

    #[test]
    fn a_relative_import_that_resolves_to_nothing_is_an_error() {
        let g = graph_with(
            vec![file("src/a.mock", FileOrigin::Authored)],
            vec![(FileId(0), unresolved("./gone", Confidence::Certain))],
        );
        let findings = find_unresolved(&g);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "unresolved");
        assert_eq!(findings[0].group, Group::Defect);
        assert_eq!(findings[0].subject_kind, "import");
        assert_eq!(findings[0].severity, Severity::Error);
        assert!(findings[0].message.contains("./gone"));
        assert_eq!(
            findings[0].location.range.map(|r| r.start.0),
            Some(3),
            "located at the import, not the file"
        );
    }

    #[test]
    fn the_import_own_confidence_carries_through() {
        // A specifier the adapter could only partly read (`import(templated)`) is not evidence
        // of a broken path — it arrives below Certain and lands under the default floor.
        let g = graph_with(
            vec![file("src/a.mock", FileOrigin::Authored)],
            vec![(FileId(0), unresolved("./maybe", Confidence::Possible))],
        );
        assert_eq!(find_unresolved(&g)[0].confidence, Confidence::Possible);
    }

    #[test]
    fn generated_and_vendored_files_are_not_the_project_to_fix() {
        let g = graph_with(
            vec![
                file("gen/a.mock", FileOrigin::Generated),
                file("vendor/b.mock", FileOrigin::Vendored),
            ],
            vec![
                (FileId(0), unresolved("./gone", Confidence::Certain)),
                (FileId(1), unresolved("./gone", Confidence::Certain)),
            ],
        );
        assert!(find_unresolved(&g).is_empty());
    }
}
