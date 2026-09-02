//! Resolution over the spellings Sass allows: the path as written, then with a
//! stylesheet suffix, then the partial (`_name.scss`) and index forms. A bare
//! specifier tries the same spellings beside the importing sheet — the
//! importing file's directory is Sass's first load path — and is an external
//! package when none exists. `sass:` modules are the compiler's own.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;

pub(crate) fn resolve(from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    let specifier = specifier.trim_start_matches('~');
    if specifier.starts_with("sass:") || specifier.contains("://") || specifier.starts_with("//") {
        return Resolution::Unresolved;
    }
    let found = if specifier.starts_with('/') {
        kndo_toolkit::nearest_rooted_match(from, specifier, cx)
    } else {
        kndo_toolkit::join_relative(kndo_toolkit::parent_dir(from.as_str()), specifier).and_then(
            |base| {
                candidates(&base)
                    .into_iter()
                    .map(ProjectPath::new)
                    .find(|p| cx.contains(p))
            },
        )
    };
    found.map_or(Resolution::Unresolved, Resolution::File)
}

/// Candidate paths in resolution order: as written, with a suffix, the partial
/// beside it, then the directory-as-module forms.
fn candidates(base: &str) -> Vec<String> {
    let (dir, name) = match base.rsplit_once('/') {
        Some((dir, name)) => (format!("{dir}/"), name),
        None => (String::new(), base),
    };
    vec![
        base.to_string(),
        format!("{base}.scss"),
        format!("{base}.css"),
        format!("{dir}_{name}.scss"),
        format!("{dir}_{name}.css"),
        format!("{base}/_index.scss"),
        format!("{base}/index.scss"),
        format!("{base}/index.css"),
    ]
}
