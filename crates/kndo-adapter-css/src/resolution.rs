//! Import resolution: one candidate-list algorithm handles plain
//! CSS's `@import` (a literal relative path, extension always present in valid CSS) and SCSS's
//! `@use`/`@forward` (a module specifier, no required leading `./`, Sass's own "partial file"
//! `_name.scss` convention) uniformly — extraction never tags which at-rule produced a given
//! specifier (see extraction.rs's `handle_import_like`), so resolution doesn't need to either.

use kndo_core::adapter::{ImportSpec, Resolution, ResolveCtx};
use kndo_core::vocab::Confidence;
use smol_str::SmolStr;

pub(crate) fn resolve(spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
    let specifier = spec.specifier.as_str();
    // `@use "sass:math"` etc. — a built-in Sass module, never a file.
    if specifier.starts_with("sass:") {
        return Resolution::Unresolved;
    }
    // `@import url(…)` / `@import "https://fonts.googleapis.com/…"` — a stylesheet the browser
    // fetches, not a path into this project. Nothing here can be missing.
    if specifier.starts_with("http://")
        || specifier.starts_with("https://")
        || specifier.starts_with("//")
    {
        return Resolution::Unresolved;
    }
    let base = kndo_adapter_toolkit::paths::join(
        kndo_adapter_toolkit::paths::dirname(spec.from.0.as_str()),
        specifier,
    );
    for candidate in candidates(&base) {
        let path = kndo_core::adapter::ProjectPath(SmolStr::new(candidate));
        if ctx.contains(&path) {
            return Resolution::File(path, Confidence::Certain);
        }
    }
    // Every candidate spelling `@use`/`@import` allows — partials, extensions, index files —
    // was tried, so the miss is real. Whether it is a *defect* depends on what the specifier
    // is: an explicitly relative one (`./x`, `../x`) can only ever have been a path next to
    // this file, so a miss is a broken path. A bare one (`@import "bootstrap"`) is ambiguous —
    // Sass also resolves those through load paths and `node_modules`, which this adapter does
    // not read — so a miss there is no answer, not an accusation.
    if specifier.starts_with("./") || specifier.starts_with("../") {
        Resolution::Missing
    } else {
        Resolution::Unresolved
    }
}

/// Candidate paths in resolution order: the literal path as given (covers plain
/// CSS's always-explicit `@import "./x.css"`), then with `.css`/`.scss` appended, then Sass's
/// partial-file form (`_name.scss` in the same directory), then a directory-as-module form
/// (`{path}/_index.scss`) — a best-effort, syntactic approximation of Sass's real module
/// resolution, not a byte-exact reimplementation of the compiler's probing order.
fn candidates(base: &str) -> Vec<String> {
    let mut out = vec![
        base.to_string(),
        format!("{base}.css"),
        format!("{base}.scss"),
    ];
    match base.rsplit_once('/') {
        Some((dir, name)) => out.push(format!("{dir}/_{name}.scss")),
        None => out.push(format!("_{base}.scss")),
    }
    out.push(format!("{base}/_index.scss"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::adapter::ProjectPath;
    use rustc_hash::FxHashSet;

    fn path(p: &str) -> ProjectPath {
        ProjectPath(SmolStr::new(p))
    }

    fn spec(specifier: &str) -> ImportSpec {
        ImportSpec {
            specifier: SmolStr::new(specifier),
            from: path("src/main.css"),
        }
    }

    #[test]
    fn a_plain_css_import_resolves_the_literal_relative_path() {
        let known: FxHashSet<ProjectPath> = [path("src/base.css")].into_iter().collect();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(
            resolve(&spec("./base.css"), &ctx),
            Resolution::File(path("src/base.css"), Confidence::Certain)
        );
    }

    #[test]
    fn an_scss_use_resolves_to_its_partial_file() {
        let known: FxHashSet<ProjectPath> = [path("src/_tokens.scss")].into_iter().collect();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(
            resolve(&spec("tokens"), &ctx),
            Resolution::File(path("src/_tokens.scss"), Confidence::Certain)
        );
    }

    #[test]
    fn a_bare_sass_builtin_module_is_never_resolved() {
        let known: FxHashSet<ProjectPath> = FxHashSet::default();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(resolve(&spec("sass:math"), &ctx), Resolution::Unresolved);
    }

    #[test]
    fn a_specifier_matching_nothing_is_missing_not_merely_unresolved() {
        // Every spelling `@use`/`@import` allows was tried, so the answer is complete: the
        // path names no file. `Unresolved` would mean "I have no answer", and the `unresolved`
        // analysis (correctly) reports nothing for that.
        let known: FxHashSet<ProjectPath> = FxHashSet::default();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(resolve(&spec("./missing.css"), &ctx), Resolution::Missing);
        // A built-in Sass module is not a path at all — no answer, and no accusation.
        assert_eq!(resolve(&spec("sass:math"), &ctx), Resolution::Unresolved);
        // A stylesheet the browser fetches. Not this project's file, so never missing from it.
        assert_eq!(
            resolve(&spec("https://fonts.googleapis.com/css?family=X"), &ctx),
            Resolution::Unresolved
        );
        // `@import "bootstrap"` — Sass also resolves bare names through load paths and
        // `node_modules`, which this adapter does not read. Ambiguous, so no accusation.
        assert_eq!(resolve(&spec("bootstrap"), &ctx), Resolution::Unresolved);
    }
}
