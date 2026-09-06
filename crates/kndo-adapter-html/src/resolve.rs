//! A page's references are URLs, and a URL names a file exactly. A
//! document-relative one (`./main.js`, the spelling extraction gives every
//! attribute value) joins the document's directory; a root-relative one
//! (`/src/main.ts`) is the one shape only the document's place in the tree
//! can answer — the server's root is unknown, and the toolkit's
//! nearest-ancestor rule stands in for it. No guessed extension and no
//! package: a browser requests what the attribute says.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;

pub(crate) fn resolve(from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    let path = specifier.split(['?', '#']).next().unwrap_or(specifier);
    if path.starts_with('/') {
        return kndo_toolkit::nearest_rooted_match(from, path, cx)
            .map_or(Resolution::Unresolved, Resolution::File);
    }
    match kndo_toolkit::join_relative(kndo_toolkit::parent_dir(from.as_str()), path)
        .map(ProjectPath::new)
    {
        Some(target) if cx.contains(&target) => Resolution::File(target),
        _ => Resolution::Unresolved,
    }
}
