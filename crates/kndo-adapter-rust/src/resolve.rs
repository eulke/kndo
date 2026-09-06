//! Module-path resolution against the project's known files. A Rust path walks the
//! module tree, and the module tree is the file tree: `crate::a::b::item` lives in
//! `src/a/b.rs`, `src/a/b/mod.rs`, or — when `b` is an inline module or an item —
//! further up the same spine. The longest path prefix that names a known file wins,
//! so the walk is deterministic; anything unplaceable is `Unresolved` — keep-alive,
//! never an accusation.
//!
//! Specifier grammar (produced by this adapter's own extraction): `crate::…`,
//! `self::…`, `super::…` (leading run only) are project-relative; anything else
//! tries the workspace packages first and falls back to module-relative, because a
//! sibling module used qualified (`util::helper()`) arrives package-shaped.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;

pub fn resolve(from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    let segments: Vec<&str> = specifier.split("::").collect();
    let (first, rest) = match segments.split_first() {
        Some(split) => split,
        None => return Resolution::Unresolved,
    };
    // `./path/to.rs` — an `include!`, whose argument is a FILE path relative to
    // the including file's directory, not a module path. The one specifier of
    // this adapter's grammar that names a file rather than a module.
    if let Some(rest) = specifier.strip_prefix("./") {
        let dir = parent_dir_owned(from.as_str());
        let file = ProjectPath::new(join(&dir, rest));
        return if cx.contains(&file) {
            Resolution::File(file)
        } else {
            Resolution::Unresolved
        };
    }
    match *first {
        "crate" => {
            let base = crate_src_dir(from, cx);
            let own = own_module_file(&base, &base, cx);
            resolve_in_module_tree(&base, rest, cx, own)
        }
        "self" => {
            let crate_src = crate_src_dir(from, cx);
            let base = dir_equiv(from, cx);
            let own = own_module_file(&base, &crate_src, cx);
            resolve_in_module_tree(&base, rest, cx, own)
        }
        "super" => {
            let supers = segments.iter().take_while(|s| **s == "super").count();
            let crate_src = crate_src_dir(from, cx);
            let mut base = dir_equiv(from, cx);
            for _ in 0..supers {
                if base == crate_src || base.is_empty() {
                    // Above the crate root lives another crate — not this file tree.
                    return Resolution::Unresolved;
                }
                base = parent_dir_owned(&base);
            }
            let own = own_module_file(&base, &crate_src, cx);
            resolve_in_module_tree(&base, &segments[supers..], cx, own)
        }
        // `use ::ext::…` — explicitly external; only a workspace package can match.
        "" => resolve_package(rest, from, cx, false),
        _ => resolve_package(&segments, from, cx, true),
    }
}

/// A package-shaped path: the first segment as a workspace package (a path
/// dependency's lib), else — for plain identifiers, which are how sibling modules
/// arrive when used qualified — the whole path module-relative to `from`.
fn resolve_package(
    segments: &[&str],
    from: &ProjectPath,
    cx: &ResolveContext<'_>,
    try_relative: bool,
) -> Resolution {
    let Some((name, rest)) = segments.split_first() else {
        return Resolution::Unresolved;
    };
    if let Some(pkg) = cx.package(name) {
        let (base, own) = match &pkg.entry {
            Some(entry) => {
                if rest.is_empty() {
                    return Resolution::File(entry.clone());
                }
                (parent_dir_owned(entry.as_str()), Some(entry.clone()))
            }
            None => {
                let base = join(&pkg.dir, "src");
                let own = own_module_file(&base, &base, cx);
                (base, own)
            }
        };
        return resolve_in_module_tree(&base, rest, cx, own);
    }
    if try_relative {
        // The first segment might be a sibling module (uniform paths) — but only a
        // child FILE proves it. Landing in the base's own module file is forbidden
        // here: an external crate's path must stay Unresolved, not become a
        // surface-keeping self-edge.
        let base = dir_equiv(from, cx);
        return resolve_in_module_tree(&base, segments, cx, None);
    }
    Resolution::Unresolved
}

/// The longest prefix of `segments` that names a module file under `base` wins; a
/// fully-consumed prefix with segments left over means those segments are items or
/// inline modules INSIDE the matched file. No prefix at all means the path names an
/// item in `base`'s own module file.
fn resolve_in_module_tree(
    base: &str,
    segments: &[&str],
    cx: &ResolveContext<'_>,
    own: Option<ProjectPath>,
) -> Resolution {
    for k in (1..=segments.len()).rev() {
        let stem = join(base, &segments[..k].join("/"));
        if let Some(found) = module_file(&stem, cx) {
            return Resolution::File(found);
        }
    }
    // The path names an item in `base`'s own module file — including the self-edge
    // a test mod's `use super::*` produces, which really does consume this file's
    // surface. Only confirmed-local bases hand one in (crate/self/super, or a
    // matched workspace package); the sibling-module fallback never does, so an
    // external crate's path stays Unresolved instead of becoming a self-edge.
    match own {
        Some(found) => Resolution::File(found),
        None => Resolution::Unresolved,
    }
}

/// `<stem>.rs`, then `<stem>/mod.rs`.
fn module_file(stem: &str, cx: &ResolveContext<'_>) -> Option<ProjectPath> {
    let file = ProjectPath::new(format!("{stem}.rs"));
    if cx.contains(&file) {
        return Some(file);
    }
    let modrs = ProjectPath::new(format!("{stem}/mod.rs"));
    cx.contains(&modrs).then_some(modrs)
}

/// The file that IS the module whose children live in `dir`: the crate entry when
/// `dir` is that crate's own source root, else `dir.rs`/`dir/mod.rs`.
fn own_module_file(dir: &str, crate_src: &str, cx: &ResolveContext<'_>) -> Option<ProjectPath> {
    if dir == crate_src {
        for entry in ["lib.rs", "main.rs"] {
            let candidate = ProjectPath::new(join(dir, entry));
            if cx.contains(&candidate) {
                return Some(candidate);
            }
        }
        return None;
    }
    module_file(dir, cx)
}

/// The directory this crate's module tree grows from. The file's own position
/// answers it: the nearest ancestor directory holding a crate entry file — which
/// covers `[[bin]]` targets rooted anywhere (`path = "crates/core/main.rs"` in a
/// lib-less package), nested workspaces, and plain `src/` alike. Only when no
/// entry exists above does the declared package geometry decide.
fn crate_src_dir(from: &ProjectPath, cx: &ResolveContext<'_>) -> String {
    let mut dir = parent_dir_owned(from.as_str());
    loop {
        for entry in ["lib.rs", "main.rs"] {
            if cx.contains(&ProjectPath::new(join(&dir, entry))) {
                return dir;
            }
        }
        if dir.is_empty() {
            break;
        }
        dir = parent_dir_owned(&dir);
    }
    if let Some(pkg) = cx.package_of(from) {
        return match &pkg.entry {
            Some(entry) => parent_dir_owned(entry.as_str()),
            None => join(&pkg.dir, "src"),
        };
    }
    if from.as_str().starts_with("src/") {
        "src".to_string()
    } else {
        String::new()
    }
}

/// The directory a file's child modules live in — `src/a/b.rs` parents `src/a/b/`.
/// Crate-root files (`lib.rs`, `main.rs`, `build.rs`, `mod.rs`, and the single-file
/// crates cargo auto-discovers under `src/bin`, `tests`, `benches`, `examples`)
/// parent their own directory instead.
fn dir_equiv(from: &ProjectPath, cx: &ResolveContext<'_>) -> String {
    let path = from.as_str();
    let name = path.rsplit('/').next().unwrap_or(path);
    if matches!(name, "lib.rs" | "main.rs" | "mod.rs") {
        return parent_dir_owned(path);
    }
    let pkg_dir = cx
        .package_of(from)
        .map(|p| p.dir.to_string())
        .unwrap_or_default();
    let rel = match path.strip_prefix(&format!("{pkg_dir}/")) {
        Some(rel) if !pkg_dir.is_empty() => rel,
        _ => path,
    };
    if rel == "build.rs" || is_single_file_crate(rel) {
        return parent_dir_owned(path);
    }
    path.strip_suffix(".rs").unwrap_or(path).to_string()
}

/// `tests/foo.rs`, `benches/foo.rs`, `examples/foo.rs`, `src/bin/foo.rs` — each its
/// own crate root, so its modules live beside it.
/// Whether this file is a "mod-rs" source file in the Reference's sense — a
/// crate root (`lib.rs`, `main.rs`, a build script, a single-file target) or a
/// `mod.rs` — whose child modules live in its OWN directory. Every other file's
/// children live in a directory named after it, one level below the file. Read
/// without a context, because extraction has none: the package-relative strip
/// [`dir_equiv`] makes only matters for a nested package, and a directory named
/// `tests` inside one is a single-file target's home either way.
pub(crate) fn is_mod_rs(path: &kndo_contract::vocab::ProjectPath) -> bool {
    let path = path.as_str();
    let name = path.rsplit('/').next().unwrap_or(path);
    if matches!(name, "lib.rs" | "main.rs" | "mod.rs" | "build.rs") {
        return true;
    }
    ["tests/", "benches/", "examples/", "src/bin/"]
        .iter()
        .any(|prefix| {
            let at = path
                .strip_prefix(prefix)
                .or_else(|| path.split_once(&format!("/{prefix}")).map(|(_, rest)| rest));
            at.is_some_and(|rest| !rest.contains('/'))
        })
}

fn is_single_file_crate(rel: &str) -> bool {
    for prefix in ["tests/", "benches/", "examples/", "src/bin/"] {
        if let Some(rest) = rel.strip_prefix(prefix)
            && !rest.contains('/')
        {
            return true;
        }
    }
    false
}

fn parent_dir_owned(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

fn join(dir: &str, rest: &str) -> String {
    if dir.is_empty() {
        rest.to_string()
    } else {
        format!("{dir}/{rest}")
    }
}
