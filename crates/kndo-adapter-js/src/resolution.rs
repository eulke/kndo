//! Import resolution — spec docs/adapters/js-ts.md §3, first slice.
//!
//! Covers: relative/absolute specifiers against the discovered-file index (extension
//! resolution order + directory `index.*` fallback), `node:`-prefixed and bare Node builtins
//! (→ `Stdlib`), and bare package specifiers via the subpath→package mapping (→
//! `Dependency`). Deferred to later commits (each already named in the spec, not silently
//! missing): self-reference `imports` (`#internal/*`), `exports`/`tsconfig paths` maps,
//! workspace-package resolution (RFC 0011), pnpm symlink layouts.

use kndo_core::adapter::{ImportSpec, ProjectPath, Resolution, ResolveCtx};
use kndo_core::vocab::Confidence;
use smol_str::SmolStr;

/// Order matters: TS extensions before JS, per spec §3.
const TS_EXTS: &[&str] = &["ts", "tsx", "mts", "cts"];
const JS_EXTS: &[&str] = &["js", "jsx", "mjs", "cjs"];

/// Bare-importable Node builtins — **generated data, never hand-maintained code**
/// (`scripts/gen-node-builtins.mjs`, sourced from Node's own `module.builtinModules`).
///
/// Two facts make this maintainable without touching adapter code on new Node releases:
/// 1. Since ~v18, Node's policy is that every NEW builtin is `node:`-prefix-only (`node:test`,
///    `node:sqlite` — the bare names fail); the prefix is handled structurally in `resolve`,
///    so this bare-name set is a frozen legacy list, not a moving target.
/// 2. If it ever does move, updating is regenerating a data file, not editing code.
///
/// Known limitation until manifest facts reach `ResolveCtx`: a *declared* dependency whose
/// name shadows a builtin (the userland `punycode` package is real) should win over this
/// list — manifest-declared beats builtin once the manifest slice lands (spec §3).
static NODE_BUILTINS: std::sync::LazyLock<std::collections::HashSet<&'static str>> =
    std::sync::LazyLock::new(|| {
        include_str!("node_builtins.txt")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect()
    });

pub fn resolve(spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
    let s = spec.specifier.as_str();

    if s.starts_with('#') {
        // Self-reference `imports` map — needs package.json `imports`, not yet parsed.
        return Resolution::Unresolved;
    }

    if s.starts_with('.') || s.starts_with('/') {
        return resolve_relative(spec.from.0.as_str(), s, ctx);
    }

    if let Some(builtin) = s.strip_prefix("node:") {
        let _ = builtin; // `node:`-prefixed is unambiguously stdlib regardless of the name.
        return Resolution::Stdlib;
    }
    if NODE_BUILTINS.contains(s) {
        return Resolution::Stdlib;
    }

    Resolution::Dependency(package_name_from_specifier(s), Confidence::Certain)
}

fn resolve_relative(from: &str, spec: &str, ctx: &ResolveCtx<'_>) -> Resolution {
    let from_dir = dirname(from);
    let base = join(from_dir, spec);
    for candidate in candidates(&base) {
        let path = ProjectPath(SmolStr::new(candidate));
        if ctx.contains(&path) {
            return Resolution::File(path, Confidence::Certain);
        }
    }
    Resolution::Unresolved
}

fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Joins a specifier onto the importing file's directory, normalizing `.`/`..` segments.
/// Pure string manipulation — no filesystem access (adapters stay fs-free by contract).
fn join(from_dir: &str, spec: &str) -> String {
    let mut stack: Vec<&str> = if from_dir.is_empty() {
        vec![]
    } else {
        from_dir.split('/').collect()
    };
    let spec = match spec.strip_prefix('/') {
        Some(rest) => {
            stack.clear();
            rest
        }
        None => spec,
    };
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            s => stack.push(s),
        }
    }
    stack.join("/")
}

/// Candidate paths in resolution order: explicit path as given, then extension appends
/// (TS before JS), then `.d.ts`, then directory `index.*` in the same extension order.
fn candidates(base: &str) -> Vec<String> {
    let mut out = vec![base.to_string()];
    for ext in TS_EXTS.iter().chain(JS_EXTS.iter()) {
        out.push(format!("{base}.{ext}"));
    }
    out.push(format!("{base}.d.ts"));
    for ext in TS_EXTS.iter().chain(JS_EXTS.iter()) {
        out.push(format!("{base}/index.{ext}"));
    }
    out
}

/// `lodash/fp` → `lodash`; `@scope/pkg/sub` → `@scope/pkg` (spec §3).
fn package_name_from_specifier(spec: &str) -> SmolStr {
    let mut segs = spec.split('/');
    let first = segs.next().unwrap_or("");
    if first.starts_with('@') {
        if let Some(second) = segs.next() {
            return SmolStr::new(format!("{first}/{second}"));
        }
    }
    SmolStr::new(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn ctx_with(paths: &[&str]) -> HashSet<ProjectPath> {
        paths
            .iter()
            .map(|p| ProjectPath(SmolStr::new(*p)))
            .collect()
    }

    fn spec(from: &str, specifier: &str) -> ImportSpec {
        ImportSpec {
            specifier: SmolStr::new(specifier),
            from: ProjectPath(SmolStr::new(from)),
        }
    }

    #[test]
    fn resolves_relative_with_explicit_extension() {
        let known = ctx_with(&["src/a.ts", "src/b.ts"]);
        let r = resolve(&spec("src/a.ts", "./b.ts"), &ResolveCtx::new(&known));
        assert_eq!(
            r,
            Resolution::File(ProjectPath(SmolStr::new("src/b.ts")), Confidence::Certain)
        );
    }

    #[test]
    fn resolves_bare_relative_preferring_ts_over_js() {
        let known = ctx_with(&["src/a.ts", "src/b.ts", "src/b.js"]);
        let r = resolve(&spec("src/a.ts", "./b"), &ResolveCtx::new(&known));
        assert_eq!(
            r,
            Resolution::File(ProjectPath(SmolStr::new("src/b.ts")), Confidence::Certain)
        );
    }

    #[test]
    fn resolves_directory_index_fallback() {
        let known = ctx_with(&["src/a.ts", "src/util/index.ts"]);
        let r = resolve(&spec("src/a.ts", "./util"), &ResolveCtx::new(&known));
        assert_eq!(
            r,
            Resolution::File(
                ProjectPath(SmolStr::new("src/util/index.ts")),
                Confidence::Certain
            )
        );
    }

    #[test]
    fn parent_traversal_normalizes_correctly() {
        let known = ctx_with(&["src/mod/a.ts", "src/shared.ts"]);
        let r = resolve(&spec("src/mod/a.ts", "../shared"), &ResolveCtx::new(&known));
        assert_eq!(
            r,
            Resolution::File(
                ProjectPath(SmolStr::new("src/shared.ts")),
                Confidence::Certain
            )
        );
    }

    #[test]
    fn project_absolute_specifier_resolves_from_root() {
        let known = ctx_with(&["src/a.ts", "shared.ts"]);
        let r = resolve(&spec("src/a.ts", "/shared"), &ResolveCtx::new(&known));
        assert_eq!(
            r,
            Resolution::File(ProjectPath(SmolStr::new("shared.ts")), Confidence::Certain)
        );
    }

    #[test]
    fn unresolved_when_no_candidate_exists() {
        let known = ctx_with(&["src/a.ts"]);
        let r = resolve(&spec("src/a.ts", "./missing"), &ResolveCtx::new(&known));
        assert_eq!(r, Resolution::Unresolved);
    }

    #[test]
    fn bare_specifier_becomes_dependency() {
        let known = ctx_with(&[]);
        let r = resolve(&spec("src/a.ts", "lodash/fp"), &ResolveCtx::new(&known));
        assert_eq!(
            r,
            Resolution::Dependency(SmolStr::new("lodash"), Confidence::Certain)
        );
    }

    #[test]
    fn scoped_package_subpath_maps_to_scoped_package() {
        let known = ctx_with(&[]);
        let r = resolve(
            &spec("src/a.ts", "@org/ui/button"),
            &ResolveCtx::new(&known),
        );
        assert_eq!(
            r,
            Resolution::Dependency(SmolStr::new("@org/ui"), Confidence::Certain)
        );
    }

    #[test]
    fn node_prefixed_is_stdlib() {
        let known = ctx_with(&[]);
        let r = resolve(&spec("src/a.ts", "node:fs"), &ResolveCtx::new(&known));
        assert_eq!(r, Resolution::Stdlib);
    }

    #[test]
    fn bare_builtin_name_is_stdlib() {
        let known = ctx_with(&[]);
        let r = resolve(&spec("src/a.ts", "path"), &ResolveCtx::new(&known));
        assert_eq!(r, Resolution::Stdlib);
    }

    #[test]
    fn bare_builtin_lookalike_package_is_not_stdlib() {
        // "fsx" isn't a builtin — must not false-positive off a prefix match.
        let known = ctx_with(&[]);
        let r = resolve(&spec("src/a.ts", "fsx"), &ResolveCtx::new(&known));
        assert_eq!(
            r,
            Resolution::Dependency(SmolStr::new("fsx"), Confidence::Certain)
        );
    }

    #[test]
    fn self_reference_import_is_deferred_not_wrong() {
        let known = ctx_with(&[]);
        let r = resolve(
            &spec("src/a.ts", "#internal/util"),
            &ResolveCtx::new(&known),
        );
        assert_eq!(r, Resolution::Unresolved);
    }

    #[test]
    fn builtin_subpath_is_stdlib() {
        // fs/promises is its own entry in module.builtinModules — the generated data
        // carries all of these; a hand list historically missed several.
        let known = ctx_with(&[]);
        let r = resolve(&spec("src/a.ts", "fs/promises"), &ResolveCtx::new(&known));
        assert_eq!(r, Resolution::Stdlib);
    }

    #[test]
    fn generated_builtins_data_is_well_formed() {
        // Guards the generated file itself: non-trivial, essentials present, no duplicates.
        assert!(
            NODE_BUILTINS.len() >= 60,
            "suspiciously small: {}",
            NODE_BUILTINS.len()
        );
        for essential in ["fs", "path", "url", "util", "events"] {
            assert!(NODE_BUILTINS.contains(essential), "missing {essential}");
        }
        let raw_lines = include_str!("node_builtins.txt")
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .count();
        assert_eq!(
            raw_lines,
            NODE_BUILTINS.len(),
            "duplicates in generated data"
        );
    }
}
