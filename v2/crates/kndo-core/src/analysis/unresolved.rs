//! A relative import specifier that points at no file. Without this category a
//! broken path is invisible: assembly drops the edge, and whatever the import
//! would have kept alive reports `unused` instead — the wrong category, naming
//! the wrong problem. Bare specifiers are never judged here: an unmatched
//! package name is an external dependency, which is normal.
//!
//! One precision rule, derived from the corpus experiment: the missing target's
//! parent directory must itself hold at least one file the graph knows. A
//! specifier pointing into a directory the whole analysis never saw —
//! `../dist/node/cli.js` into a gitignored build output — is the project
//! reaching OUTSIDE the analyzed world, deliberately, and "no file there" is
//! not evidence of a typo. A typo or a rename that missed a call site points
//! beside real files, and that is exactly the case that stays judged. The
//! boundary is stated, not hidden: a rename that deleted a whole directory
//! escapes this analysis.

use super::{Analysis, AnalysisContext};
use kndo_contract::evidence::ImportTarget;
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::Subject;
use kndo_contract::vocab::Category;

pub struct Unresolved;

impl Analysis for Unresolved {
    fn id(&self) -> &'static str {
        "unresolved"
    }

    fn category(&self) -> Category {
        Category::UNRESOLVED
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph();
        // Every directory that directly holds a graph file, `/`-separated.
        let dirs: std::collections::BTreeSet<&str> = g
            .files
            .iter()
            .map(|f| parent_dir(f.path.as_str()))
            .collect();
        let mut out = Vec::new();
        for (i, f) in g.files.iter().enumerate() {
            if !cx.measured[i] {
                continue;
            }
            for (import, targets) in f.evidence.imports.iter().zip(&f.import_targets) {
                if !targets.is_empty() {
                    continue;
                }
                let ImportTarget::Relative(spec) = &import.target else {
                    continue;
                };
                // The contract defines Relative as the `./`-style spelling; an
                // adapter routing other project-internal path grammars through
                // the variant is judged by ITS resolver, never by this one.
                if !spec.starts_with('.') {
                    continue;
                }
                // Only the author's own Certain statement accuses. A derived or
                // partly-read specifier (an adapter's submodule probe, a
                // templated dynamic import) is speculation — silence over
                // accusation, the same tier rule every dependency claim obeys.
                if import.confidence != kndo_contract::vocab::Confidence::Certain {
                    continue;
                }
                let Some(target) = normalize(f.path.as_str(), strip_query(spec)) else {
                    continue; // escapes the project root — not this tree's path
                };
                if !dirs.contains(parent_dir(&target)) {
                    continue; // points into a world the graph never saw
                }
                // A target that EXISTS in the discovered tree is not missing —
                // it lives outside the analyzed world (an asset, a manifest);
                // resolution not claiming it is scope, not breakage.
                if g.discovered
                    .binary_search_by(|p| p.as_str().cmp(target.as_str()))
                    .is_ok()
                {
                    continue;
                }
                out.push(Finding::new(
                    Category::UNRESOLVED,
                    Severity::Error,
                    import.confidence,
                    Subject::Import {
                        path: f.path.clone(),
                        specifier: spec.clone(),
                        span: import.span,
                    },
                    "",
                    format!(
                        "import of '{spec}' resolves to no file — a broken path or a \
                         rename that missed this call site"
                    ),
                ));
            }
        }
        out
    }
}

/// The specifier without its query/fragment suffix (`./worker?worker&url`) —
/// resolvers strip it too; the analysis defends independently so a resolver
/// that has not learned a suffix cannot manufacture a finding.
fn strip_query(spec: &str) -> &str {
    spec.split(['?', '#']).next().unwrap_or(spec)
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// `from`-relative `spec` as a root-relative path, `None` when it escapes the
/// root. Purely lexical — the same arithmetic every resolver applies.
fn normalize(from: &str, spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = parent_dir(from)
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::{normalize, parent_dir, strip_query};

    #[test]
    fn the_path_arithmetic_is_lexical_and_root_bounded() {
        assert_eq!(normalize("src/a.js", "./b.js").as_deref(), Some("src/b.js"));
        assert_eq!(
            normalize("src/x/a.js", "../b.js").as_deref(),
            Some("src/b.js")
        );
        assert_eq!(normalize("a.js", "../escape.js"), None);
        assert_eq!(strip_query("./w?worker&url"), "./w");
        assert_eq!(parent_dir("src/a.js"), "src");
        assert_eq!(parent_dir("a.js"), "");
    }
}
