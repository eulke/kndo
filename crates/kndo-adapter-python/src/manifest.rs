//! Dependency NAMES from the Python manifests kndo consults: `pyproject.toml`'s
//! `[project]` dependency arrays and `requirements*.txt` lines. A dependency
//! specifier ("flask-sqlalchemy>=3.0; python_version>'3.9'") is trimmed to the
//! NAME the ecosystem imports the package by — everything up to the first
//! version/marker/extras character.

use smol_str::SmolStr;

pub fn dependencies(path: &str, content: &[u8]) -> Vec<SmolStr> {
    let Ok(text) = std::str::from_utf8(content) else {
        return Vec::new();
    };
    let file = path.rsplit('/').next().unwrap_or(path);
    let mut out: Vec<SmolStr> = if file == "pyproject.toml" {
        pyproject(text)
    } else {
        requirements(text)
    };
    out.sort();
    out.dedup();
    out
}

/// Every quoted string inside a `dependencies = [ … ]` array (the `[project]`
/// table's and each `[project.optional-dependencies]` entry's — both are
/// dependency lists), trimmed to its name.
fn pyproject(text: &str) -> Vec<SmolStr> {
    let mut out = Vec::new();
    let mut in_array = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if !in_array {
            if trimmed.starts_with("dependencies") && trimmed.contains('=') && trimmed.contains('[')
            {
                in_array = !trimmed.contains(']');
                collect_quoted(trimmed, &mut out);
            } else if (trimmed.ends_with("= [") || trimmed.ends_with("=["))
                && in_optional_list_header(trimmed)
            {
                in_array = true;
            }
            continue;
        }
        collect_quoted(trimmed, &mut out);
        if trimmed.contains(']') {
            in_array = false;
        }
    }
    out
}

/// `[project.optional-dependencies]` entries look like `dev = [`; the section
/// header itself was seen on an earlier line, but keying on the `name = [`
/// shape alone would swallow unrelated arrays — so only single-word keys
/// qualify, the shape those tables use.
fn in_optional_list_header(line: &str) -> bool {
    line.split('=').next().is_some_and(|k| {
        !k.trim().is_empty() && !k.trim().contains(' ') && k.trim() != "dependencies"
    })
}

fn collect_quoted(line: &str, out: &mut Vec<SmolStr>) {
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else { break };
        if let Some(name) = dependency_name(&after[..end]) {
            out.push(SmolStr::new(name));
        }
        rest = &after[end + 1..];
    }
}

fn requirements(text: &str) -> Vec<SmolStr> {
    text.lines()
        .filter_map(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with('-') {
                return None;
            }
            dependency_name(t).map(SmolStr::new)
        })
        .collect()
}

fn dependency_name(spec: &str) -> Option<&str> {
    let end = spec
        .find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .unwrap_or(spec.len());
    let name = &spec[..end];
    (!name.is_empty()).then_some(name)
}
