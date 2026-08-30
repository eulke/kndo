//! Relative-specifier resolution against the project's known files, in Node/TS
//! candidate order: the path as written, extension candidates, the `.js`-names-the-
//! compiled-file swap, then directory `index.*`. The candidate list is fixed, so the
//! first hit is deterministic; anything unplaceable is `Unresolved` — keep-alive,
//! never an accusation.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;

/// A compiled-output extension in a specifier names the SOURCE beside it — the
/// grammar-specific swap table, spelled once.
const COMPILED_TO_SOURCE: [(&str, &[&str]); 2] = [(".js", &[".ts", ".tsx"]), (".jsx", &[".tsx"])];

pub fn resolve(
    from: &ProjectPath,
    specifier: &str,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Resolution {
    if !specifier.starts_with('.') {
        return resolve_bare(specifier, cx, exts);
    }
    let dir = parent_dir(from);
    match resolve_in_dir(dir, specifier, cx, exts) {
        Some(path) => Resolution::File(path),
        None => Resolution::Unresolved,
    }
}

/// A bare specifier links inside the project only when one of its own manifests
/// declares the package (a workspace sibling): the exact name resolves to the
/// declared entry; a subpath resolves against the package directory when the layout
/// matches. Everything else is an external package — `Unresolved`, keep-alive.
fn resolve_bare(specifier: &str, cx: &ResolveContext<'_>, exts: &[String]) -> Resolution {
    let (name, subpath) = split_bare(specifier);
    let Some(pkg) = cx.package(name) else {
        return Resolution::Unresolved;
    };
    match subpath {
        None => Resolution::File(pkg.entry.clone()),
        Some(sub) => match resolve_in_dir(&pkg.dir, sub, cx, exts) {
            Some(path) => Resolution::File(path),
            None => Resolution::Unresolved,
        },
    }
}

/// `@scope/name/sub/path` → (`@scope/name`, `sub/path`); `name/sub` → (`name`, `sub`).
fn split_bare(specifier: &str) -> (&str, Option<&str>) {
    let name_segments = if specifier.starts_with('@') { 2 } else { 1 };
    let mut boundary = 0;
    let mut seen = 0;
    for (i, c) in specifier.char_indices() {
        if c == '/' {
            seen += 1;
            if seen == name_segments {
                boundary = i;
                break;
            }
        }
    }
    if boundary == 0 {
        (specifier, None)
    } else {
        (&specifier[..boundary], Some(&specifier[boundary + 1..]))
    }
}

pub(crate) fn parent_dir(path: &ProjectPath) -> &str {
    match path.as_str().rfind('/') {
        Some(i) => &path.as_str()[..i],
        None => "",
    }
}

/// The shared candidate machinery, without the leading-dot requirement — manifest
/// entries (`"main": "index.js"`) are dir-relative but rarely spelled `./`.
pub(crate) fn resolve_in_dir(
    dir: &str,
    specifier: &str,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Option<ProjectPath> {
    let joined = normalize(dir, specifier)?;
    candidates(&joined, exts)
        .into_iter()
        .map(ProjectPath::new)
        .find(|p| cx.contains(p))
}

/// Joins and collapses `.`/`..` segments. `None` when the specifier escapes the
/// project root — nothing inside the project can be meant.
fn normalize(dir: &str, spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

fn candidates(joined: &str, exts: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if !joined.is_empty() {
        out.push(joined.to_string());
        for e in exts {
            out.push(format!("{joined}{e}"));
        }
        for (compiled, sources) in COMPILED_TO_SOURCE {
            if let Some(stem) = joined.strip_suffix(compiled) {
                out.extend(sources.iter().map(|s| format!("{stem}{s}")));
                break;
            }
        }
    }
    let prefix = if joined.is_empty() {
        String::new()
    } else {
        format!("{joined}/")
    };
    for e in exts {
        out.push(format!("{prefix}index{e}"));
    }
    out
}
