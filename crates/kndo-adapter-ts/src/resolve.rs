//! Relative-specifier resolution against the project's known files, in Node/TS
//! candidate order: the path as written, extension candidates, the `.js`-names-the-
//! compiled-file swap, then directory `index.*`. The candidate list is fixed, so the
//! first hit is deterministic; anything unplaceable is `Unresolved` — keep-alive,
//! never an accusation.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;

/// A compiled-output extension in a specifier names the SOURCE beside it — the
/// grammar-specific swap table, spelled once. `.d.ts` last: a declaration file
/// answers a `.js` specifier only when no implementation source does (the shape
/// TS gives type stubs — `types/hot.d.ts` answering `./hot.js`).
const COMPILED_TO_SOURCE: [(&str, &[&str]); 4] = [
    (".js", &[".ts", ".tsx", ".d.ts"]),
    (".jsx", &[".tsx"]),
    (".mjs", &[".mts", ".d.mts"]),
    (".cjs", &[".cts", ".d.cts"]),
];

pub fn resolve(
    from: &ProjectPath,
    specifier: &str,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Resolution {
    // Bundler query/fragment suffixes (`./worker?worker&url`, `./x.svg#icon`)
    // address the same file with extra instructions — the file is what keeps
    // things alive, so resolution sees the path alone. Measured on the corpus:
    // 42 real edges were dying as unresolved behind their suffixes.
    let specifier = specifier.split(['?', '#']).next().unwrap_or(specifier);
    if !specifier.starts_with('.') {
        return resolve_bare(specifier, cx);
    }
    let dir = kndo_toolkit::parent_dir(from.as_str());
    match resolve_in_dir(dir, specifier, cx, exts) {
        Some(path) => Resolution::File(path),
        None => Resolution::Unresolved,
    }
}

/// A bare specifier links inside the project only when one of its own manifests
/// declares the package (a workspace sibling): the exact name resolves to the
/// declared entry; a subpath resolves against the package directory when the layout
/// matches. Everything else is an external package — `Unresolved`, keep-alive.
fn resolve_bare(specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    // The WHOLE specifier first: an npm package name is a scope and a name and
    // stops there, so a declared name that spells more than that is a tsconfig
    // alias (`"vite/module-runner"`), and the alias is what the compiler picks
    // over the package whose subpath it looks like.
    if let Some(pkg) = cx.package(specifier)
        && let Some(entry) = &pkg.entry
    {
        return Resolution::File(entry.clone());
    }
    // A SUBPATH inside a declared package — `pkg/sub/thing` — is what
    // `package.json`'s `exports` map answers, condition by condition, and no
    // other reading of it is the truth: `./sub/*` may map anywhere, may be
    // absent (the package exports nothing but its root), and may differ by
    // `import` vs `require`. Splitting the name off and hoping the directory
    // mirrors the subpath agreed with that map only when the package had no
    // map at all. Until the manifest half reads `exports`, a subpath is an
    // external specifier: keep-alive, never an accusation.
    Resolution::Unresolved
}

/// The shared candidate machinery, without the leading-dot requirement — manifest
/// entries (`"main": "index.js"`) are dir-relative but rarely spelled `./`.
pub(crate) fn resolve_in_dir(
    dir: &str,
    specifier: &str,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Option<ProjectPath> {
    let joined = kndo_toolkit::join_relative(dir, specifier)?;
    candidates(&joined, exts)
        .into_iter()
        .map(ProjectPath::new)
        .find(|p| cx.contains(p))
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
