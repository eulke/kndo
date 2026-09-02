//! Resolution over Python's import forms. Absolute dotted paths resolve by
//! path suffix — `a.b.c` is `a/b/c.py` or the package `a/b/c/__init__.py`,
//! found via the toolkit's nearest-suffix match so src-layouts
//! (`src/flask/json.py`) work without configuration. Relative imports keep
//! their leading dots: one dot is the current package, each further dot climbs
//! one; an empty remainder is the package's own `__init__.py`. Third-party
//! modules resolve nowhere — keep-alive, never an accusation.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;
use kndo_toolkit as tk;

pub fn resolve(from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    if let Some(stripped) = specifier.strip_prefix('.') {
        return resolve_relative(from, stripped, cx);
    }
    let path = specifier.replace('.', "/");
    for suffix in [format!("{path}.py"), format!("{path}/__init__.py")] {
        if let Some(file) = tk::nearest_suffix_match(&suffix, from, cx) {
            return Resolution::File(file);
        }
    }
    Resolution::Unresolved
}

fn resolve_relative(
    from: &ProjectPath,
    after_first_dot: &str,
    cx: &ResolveContext<'_>,
) -> Resolution {
    // `.x` → package dir; `..x` → one up; `.` alone → the package itself.
    let extra_dots = after_first_dot.chars().take_while(|c| *c == '.').count();
    let remainder = &after_first_dot[extra_dots..];
    let mut dir = parent_dir(from.as_str()).to_string();
    for _ in 0..extra_dots {
        dir = parent_dir(&dir).to_string();
    }
    let base = if remainder.is_empty() {
        dir.clone()
    } else if dir.is_empty() {
        remainder.replace('.', "/")
    } else {
        format!("{dir}/{}", remainder.replace('.', "/"))
    };
    let module = ProjectPath::new(format!("{base}.py"));
    if cx.contains(&module) {
        return Resolution::File(module);
    }
    let package = ProjectPath::new(if base.is_empty() {
        "__init__.py".to_string()
    } else {
        format!("{base}/__init__.py")
    });
    if cx.contains(&package) {
        return Resolution::File(package);
    }
    Resolution::Unresolved
}

fn parent_dir(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    }
}
