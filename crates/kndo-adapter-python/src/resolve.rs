//! Resolution over Python's import forms, against the roots the MANIFESTS
//! declared. An absolute dotted path is a path under a source root —
//! `flask.json` is `<root>/flask/json.py` or `<root>/flask/json/__init__.py`,
//! and which directories are roots is `pyproject.toml`'s answer (`package-dir`,
//! `packages.find.where`, the `src` layout setuptools reads off the tree). A
//! unit that hangs under a name its roots do not contain says so, and the name
//! comes off the specifier before the path is joined.
//!
//! Relative imports keep their leading dots: one dot is the current package,
//! each further dot climbs one; an empty remainder is the package's own
//! `__init__.py`. Third-party modules resolve nowhere — keep-alive, never an
//! accusation.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;
use kndo_toolkit as tk;

pub fn resolve(from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    if let Some(stripped) = specifier.strip_prefix('.') {
        return resolve_relative(from, stripped, cx);
    }
    let Some(project) = cx.project() else {
        return Resolution::Unresolved;
    };
    // The unit compiling this file first — inside a monorepo, a distribution
    // resolves its own dotted names before a sibling's — then every other.
    let own = project.unit_of(from);
    let units = own
        .into_iter()
        .chain(project.units().filter(|u| Some(*u) != own));
    for unit in units {
        // A unit whose roots do not contain its package names it out loud:
        // `package-dir = {"mypkg" = "lib"}` makes `mypkg.helpers` the file
        // `lib/helpers.py`, so the declared name comes off the front.
        let rest = match &unit.namespace_root {
            Some(ns) => match specifier == ns.as_str() {
                true => "",
                false => match specifier.strip_prefix(ns.as_str()) {
                    Some(rest) => rest.strip_prefix('.').unwrap_or(rest),
                    None => continue,
                },
            },
            None => specifier,
        };
        let path = rest.replace('.', "/");
        for root in &unit.roots {
            for candidate in module_and_package(root.path.as_str(), &path) {
                if cx.contains(&candidate) {
                    return Resolution::File(candidate);
                }
            }
        }
    }
    // The project root is a source root too, and always: `sys.path` holds the
    // directory the interpreter starts in, which is why `from src.logic import
    // add` imports under a `package-dir = {"" = "src"}` layout — the installed
    // distribution calls that module `logic`, and a test run from the root
    // calls it `src.logic`. Both are real, and the declared roots answer first
    // because that is the shape the package ships as.
    for candidate in module_and_package("", &specifier.replace('.', "/")) {
        if cx.contains(&candidate) {
            return Resolution::File(candidate);
        }
    }
    Resolution::Unresolved
}

/// The two files one dotted name can be: the module, and the package it opens.
fn module_and_package(root: &str, path: &str) -> [ProjectPath; 2] {
    let under = |shape: String| match (root.is_empty(), shape.is_empty()) {
        (_, true) => ProjectPath::new(root),
        (true, false) => ProjectPath::new(shape),
        (false, false) => ProjectPath::new(format!("{root}/{shape}")),
    };
    [
        under(match path.is_empty() {
            true => String::new(),
            false => format!("{path}.py"),
        }),
        under(match path.is_empty() {
            true => "__init__.py".to_string(),
            false => format!("{path}/__init__.py"),
        }),
    ]
}

fn resolve_relative(
    from: &ProjectPath,
    after_first_dot: &str,
    cx: &ResolveContext<'_>,
) -> Resolution {
    // `.x` → package dir; `..x` → one up; `.` alone → the package itself.
    let extra_dots = after_first_dot.chars().take_while(|c| *c == '.').count();
    let remainder = &after_first_dot[extra_dots..];
    let mut dir = tk::parent_dir(from.as_str()).to_string();
    for _ in 0..extra_dots {
        dir = tk::parent_dir(&dir).to_string();
    }
    for candidate in module_and_package(&dir, &remainder.replace('.', "/")) {
        if cx.contains(&candidate) {
            return Resolution::File(candidate);
        }
    }
    Resolution::Unresolved
}
