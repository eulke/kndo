//! Resolution over the SwiftPM layout. The TARGET is the unit and it is flat:
//! `Sources|Tests/<Target>/**` shares one namespace whatever its
//! subdirectories, tests are their own module reaching main code only through
//! an explicit import, and a file outside the layout takes its first path
//! segment as its target (the content-free spelling of a `path:` override —
//! Alamofire's `Source/**` is one module). An `import` names a module: a local
//! target resolves to every file of that target; SDK and external-package
//! modules resolve nowhere — keep-alive, never an accusation.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;

/// The SwiftPM target a path belongs to: the segment after `Sources/` or
/// `Tests/`, or the first path segment outside that layout; `None` for a file
/// at the repository root (no unit, names scoped to the file). Path-only by
/// construction — extraction emits it as the file's namespace clause, and a
/// namespace an adapter can only state by walking the tree is not one.
pub(crate) fn target_of(path: &str) -> Option<(&str, bool)> {
    let segments: Vec<&str> = path.split('/').collect();
    if let Some(ix) = segments
        .iter()
        .position(|s| *s == "Sources" || *s == "Tests")
    {
        // The target segment must be a DIRECTORY — something follows it. A file
        // sitting DIRECTLY under `Sources/`/`Tests/` belongs to a flat
        // path-override layout (Alamofire's `testTarget(path: "Tests")`): the
        // directory itself is the one target.
        if ix + 2 < segments.len() {
            return Some((segments[ix + 1], segments[ix] == "Tests"));
        }
        return Some((segments[ix], segments[ix] == "Tests"));
    }
    (segments.len() > 1).then(|| (segments[0], false))
}

/// Every `.swift` file of the named target, in path order.
fn target_files(target: &str, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
    let mut out: Vec<ProjectPath> = cx
        .known_files()
        .filter(|p| {
            p.as_str().ends_with(".swift")
                && target_of(p.as_str()).is_some_and(|(t, _)| t == target)
        })
        .cloned()
        .collect();
    out.sort();
    out
}

pub fn resolve(_from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    // `import Foo` — a module name. A local target with that name is the whole
    // directory-unit; anything else (SDK frameworks, external packages) stays
    // Unresolved, keep-alive.
    let files = target_files(specifier, cx);
    if files.is_empty() {
        Resolution::Unresolved
    } else {
        Resolution::Files(files)
    }
}
