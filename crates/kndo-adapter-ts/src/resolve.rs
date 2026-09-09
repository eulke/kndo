//! Relative-specifier resolution against the project's known files, in Node/TS
//! candidate order: the path as written, extension candidates, the `.js`-names-the-
//! compiled-file swap, then directory `index.*`. The candidate list is fixed, so the
//! first hit is deterministic; anything unplaceable is `Unresolved` — keep-alive,
//! never an accusation.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;
use smol_str::SmolStr;

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
    // ...except a specifier that IS a `#` import: the whole of it is the name
    // the manifest's own table answers, and cutting at the `#` would leave
    // nothing to look up.
    let specifier = match specifier.starts_with('#') {
        true => specifier.split('?').next().unwrap_or(specifier),
        false => specifier.split(['?', '#']).next().unwrap_or(specifier),
    };
    if !specifier.starts_with('.') {
        return resolve_bare(from, specifier, cx, exts);
    }
    let dir = kndo_toolkit::parent_dir(from.as_str());
    match resolve_in_dir(dir, specifier, cx, exts) {
        Some(path) => Resolution::File(path),
        None => Resolution::Unresolved,
    }
}

/// A bare specifier links inside the project only when one of its own
/// manifests declares it: the manifest's own alias table answers a `#` import,
/// a declared package name answers the exact spelling, and that package's
/// `exports` map answers a subpath. Everything else is an external package —
/// `Unresolved`, keep-alive.
fn resolve_bare(
    from: &ProjectPath,
    specifier: &str,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Resolution {
    // A `#` import is the package's own, and only its own files may spell it:
    // the alias table is scoped to the declaring manifest's directory, which
    // is that rule exactly.
    if specifier.starts_with('#') {
        return match cx.project() {
            Some(project) => landing(project.alias(from, specifier), cx, exts),
            None => Resolution::Unresolved,
        };
    }
    // The WHOLE specifier first: an npm package name is a scope and a name and
    // stops there, so a declared name that spells more than that is a tsconfig
    // alias (`"vite/module-runner"`), and the alias is what the compiler picks
    // over the package whose subpath it looks like.
    if let Some(pkg) = cx.package(specifier) {
        // A package that declares `exports` answers its own name through the
        // map — `"."` may be conditional, and the entry field is npm's
        // fallback for a package that declares no map at all.
        let by_map = subpath(pkg, specifier, cx, exts);
        if !matches!(by_map, Resolution::Unresolved) {
            return by_map;
        }
        if let Some(entry) = &pkg.entry {
            return Resolution::File(entry.clone());
        }
    }
    // A SUBPATH inside a declared package — `pkg/sub/thing` — is what
    // `package.json`'s `exports` map answers, condition by condition, and no
    // other reading of it is the truth: `./sub/*` may map anywhere, may be
    // absent (the package exports nothing but its root), and may be `null`,
    // which is the manifest REFUSING the subpath rather than staying silent.
    // Splitting the name off and hoping the directory mirrors the subpath
    // agreed with that map only when the package had no map at all.
    let scoped = specifier.starts_with('@');
    let cut = match scoped {
        true => specifier.match_indices('/').nth(1),
        false => specifier.match_indices('/').next(),
    };
    if let Some((at, _)) = cut
        && let Some(pkg) = cx.package(&specifier[..at])
    {
        return subpath(pkg, specifier, cx, exts);
    }
    Resolution::Unresolved
}

/// What a package's `exports` map answers for one specifier. A map that names
/// the specifier and REFUSES it (npm's `null`) is `Unresolved` all the same —
/// but deliberately, and the caller has already stopped looking elsewhere.
fn subpath(
    pkg: &kndo_contract::adapter::PackageEntry,
    specifier: &str,
    cx: &ResolveContext<'_>,
    exts: &[String],
) -> Resolution {
    let landings: Vec<SmolStr> = pkg
        .subpaths
        .iter()
        .flat_map(|alias| alias.rewrite(specifier))
        .filter(|(target, _)| !target.refuses())
        .map(|(_, path)| path)
        .collect();
    landing(landings, cx, exts)
}

/// EVERY candidate of a rewriting that names a file of this project — not the
/// first. A table offers several because a runtime picks one by condition and
/// the engine picks none, so every branch is a possible resolution and all of
/// them are edges: `"#flag": { "module-sync": "./true.js", "default":
/// "./false.js" }` keeps both files, because kndo cannot know which condition
/// the consumer's runtime sets. Taking the first would keep whichever the
/// manifest's key order happened to put there, which is not a fact about the
/// program.
fn landing(candidates: Vec<SmolStr>, cx: &ResolveContext<'_>, exts: &[String]) -> Resolution {
    let mut found: Vec<ProjectPath> = candidates
        .iter()
        .filter_map(|c| resolve_in_dir("", c, cx, exts))
        .collect();
    found.sort();
    found.dedup();
    match found.len() {
        0 => Resolution::Unresolved,
        1 => Resolution::File(found.remove(0)),
        _ => Resolution::Files(found),
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
