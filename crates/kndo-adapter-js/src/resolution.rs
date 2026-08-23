//! Import resolution.
//!
//! Covers: relative/absolute specifiers against the discovered-file index (extension
//! resolution order + directory `index.*` fallback), `node:`-prefixed and bare Node builtins
//! (→ `Stdlib`), workspace-member names and subpaths (`resolve_workspace`), and
//! bare package specifiers via the subpath→package mapping (→ `Dependency`). Not covered
//! (deliberately, not silently missing): self-reference
//! `imports` (`#internal/*`), `exports`/`tsconfig paths` maps, pnpm symlink layouts.

use kndo_core::adapter::{ImportSpec, ProjectPath, Resolution, ResolveCtx};
use kndo_core::vocab::Confidence;
use smol_str::SmolStr;

/// Order matters: TS extensions before JS.
const TS_EXTS: &[&str] = &["ts", "tsx", "mts", "cts"];
const JS_EXTS: &[&str] = &["js", "jsx", "mjs", "cjs"];

/// The js-ts stdlib dataset — generated data via the shared `kndo-stdlib v1` mechanism
/// (toolkit `stdlib` module). Regenerate with `cargo xtask gen-stdlib js-ts`;
/// never hand-edit. The bare-name set is frozen by Node's own policy (new builtins are
/// `node:`-prefix-only — that prefix is the *structural* signal passed to the classifier).
static STDLIB: std::sync::LazyLock<kndo_adapter_toolkit::stdlib::StdlibIndex<'static>> =
    std::sync::LazyLock::new(|| {
        kndo_adapter_toolkit::stdlib::StdlibIndex::parse(include_str!("stdlib.txt"))
            .expect("shipped stdlib.txt is malformed — regenerate: cargo xtask gen-stdlib js-ts")
    });

pub fn resolve(spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
    let s = spec.specifier.as_str();

    if s.starts_with('#') {
        // Self-reference `imports` map — needs package.json `imports`, which is not parsed.
        return Resolution::Unresolved;
    }

    if s.starts_with('.') || s.starts_with('/') {
        return resolve_relative(spec.from.0.as_str(), s, ctx);
    }

    // Workspace members first (a name matching a sibling manifest resolves to internal
    // files): a bare specifier naming an in-repo package
    // outranks the entire external-specifier ladder below — the code demonstrably lives in
    // this repo, the strongest possible statement of what the name means. Checking before
    // even the structural-stdlib rule is safe by construction: npm package names cannot
    // contain `:`, so `node:fs` can never collide with a workspace name. This check sits in
    // the adapter rather than the toolkit's shared precedence because subpath
    // resolution needs this language's own candidate ladder, which the toolkit deliberately
    // doesn't own.
    let package_name = package_name_from_specifier(s);
    if let Some(member) = ctx.workspace_member(&package_name) {
        if let Some(resolution) = resolve_workspace(s, &package_name, member, ctx) {
            return resolution;
        }
        // No concrete in-repo file matched (the member's entries are build artifacts absent
        // from a source checkout, or the subpath names a built layout) — fall through to the
        // external ladder below: the specifier still names a *consumed package*, and losing
        // the ImportsDependency evidence would silently un-count a genuinely used
        // dependency. Only the file edge is unknowable, and it points at nothing in-tree.
    }

    // Bare specifier: the toolkit owns the precedence (structural stdlib > declared dep >
    // stdlib list > dependency); this adapter supplies only what it alone knows — the
    // structural signal and the subpath→package mapping.
    kndo_adapter_toolkit::stdlib::classify_bare_specifier(
        s,
        package_name,
        s.starts_with("node:"),
        &STDLIB,
        ctx,
    )
}

/// `@org/ui` → the member's resolved entry; `@org/ui/button` → the subpath against the
/// member's own directory via the same candidate ladder relative imports use — a deep import:
/// its edge is always recorded (reachability must stay correct — deep-imported
/// code IS used); the `deep-import` *verdict* is a separate analysis. `None` when no concrete in-repo file
/// matches — the caller then falls through to the external ladder so the dependency-usage
/// evidence survives (see the call site).
fn resolve_workspace(
    spec: &str,
    package_name: &str,
    member: &kndo_core::adapter::WorkspaceMember,
    ctx: &ResolveCtx<'_>,
) -> Option<Resolution> {
    let subpath = spec
        .strip_prefix(package_name)
        .unwrap_or("")
        .trim_start_matches('/');
    if subpath.is_empty() {
        return member
            .entry
            .as_ref()
            .map(|(target, confidence)| Resolution::WorkspaceMember {
                name: SmolStr::new(package_name),
                target: target.clone(),
                confidence: *confidence,
            });
    }
    let base = kndo_adapter_toolkit::paths::join(member.dir.as_str(), subpath);
    for candidate in candidates(&base) {
        let path = ProjectPath(SmolStr::new(candidate));
        if ctx.contains(&path) {
            return Some(Resolution::WorkspaceMember {
                name: SmolStr::new(package_name),
                target: path,
                confidence: Confidence::Certain,
            });
        }
    }
    None
}

fn resolve_relative(from: &str, spec: &str, ctx: &ResolveCtx<'_>) -> Resolution {
    use kndo_adapter_toolkit::paths;
    let base = paths::join(paths::dirname(from), spec);
    for candidate in candidates(&base) {
        let path = ProjectPath(SmolStr::new(candidate));
        if ctx.contains(&path) {
            return Resolution::File(path, Confidence::Certain);
        }
    }
    Resolution::Unresolved
}

/// Candidate paths in resolution order: explicit path as given, then extension appends
/// (TS before JS), then `.d.ts`, then directory `index.*` in the same extension order.
pub(crate) fn candidates(base: &str) -> Vec<String> {
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

/// `lodash/fp` → `lodash`; `@scope/pkg/sub` → `@scope/pkg`.
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
    use rustc_hash::FxHashSet as HashSet;

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
    fn self_reference_import_is_unresolved_not_wrong() {
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
        // carries all of these; a hand-maintained list misses entries.
        let known = ctx_with(&[]);
        let r = resolve(&spec("src/a.ts", "fs/promises"), &ResolveCtx::new(&known));
        assert_eq!(r, Resolution::Stdlib);
    }

    #[test]
    fn shipped_stdlib_data_parses_and_carries_essentials() {
        // Format/sortedness/duplicate validation is enforced by the shared parser (toolkit
        // stdlib module); here we only guard that the shipped dataset is real, not a stub.
        assert!(STDLIB.len() >= 60, "suspiciously small: {}", STDLIB.len());
        for essential in ["fs", "path", "url", "util", "events"] {
            assert!(STDLIB.contains(essential), "missing {essential}");
        }
        assert_eq!(STDLIB.provenance().0, "js-ts");
    }

    // ---------------------------------------------------------------- workspace members

    use kndo_core::adapter::WorkspaceMember;
    use rustc_hash::FxHashMap as HashMap;

    fn members(entries: &[(&str, &str, Option<&str>)]) -> HashMap<SmolStr, WorkspaceMember> {
        entries
            .iter()
            .map(|&(name, dir, entry)| {
                (
                    SmolStr::new(name),
                    WorkspaceMember {
                        dir: SmolStr::new(dir),
                        entry: entry.map(|e| (ProjectPath(SmolStr::new(e)), Confidence::Certain)),
                        targets: Vec::new(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn bare_workspace_name_resolves_to_the_members_entry() {
        let known = ctx_with(&["packages/ui/src/index.ts"]);
        let map = members(&[("@org/ui", "packages/ui", Some("packages/ui/src/index.ts"))]);
        let ctx = ResolveCtx::new(&known).with_workspace_members(&map);
        let r = resolve(&spec("packages/app/main.ts", "@org/ui"), &ctx);
        assert_eq!(
            r,
            Resolution::WorkspaceMember {
                name: SmolStr::new("@org/ui"),
                target: ProjectPath(SmolStr::new("packages/ui/src/index.ts")),
                confidence: Confidence::Certain,
            }
        );
    }

    #[test]
    fn workspace_subpath_resolves_against_the_members_directory() {
        // The deep-import shape — the edge is always recorded; the verdict is a separate
        // analysis.
        let known = ctx_with(&["packages/ui/button.ts"]);
        let map = members(&[("@org/ui", "packages/ui", None)]);
        let ctx = ResolveCtx::new(&known).with_workspace_members(&map);
        let r = resolve(&spec("packages/app/main.ts", "@org/ui/button"), &ctx);
        assert_eq!(
            r,
            Resolution::WorkspaceMember {
                name: SmolStr::new("@org/ui"),
                target: ProjectPath(SmolStr::new("packages/ui/button.ts")),
                confidence: Confidence::Certain,
            }
        );
    }

    #[test]
    fn unresolvable_member_falls_through_to_the_external_ladder() {
        // A member whose entries are build artifacts absent from a source checkout:
        // no in-repo file matches, but the specifier still names a
        // consumed package — falling to Unresolved would silently un-count a genuinely used
        // dependency. Only the file edge is unknowable; the dependency evidence survives.
        let known = ctx_with(&[]);
        let map = members(&[("@org/ui", "packages/ui", None)]);
        let ctx = ResolveCtx::new(&known).with_workspace_members(&map);
        assert_eq!(
            resolve(&spec("a.ts", "@org/ui"), &ctx),
            Resolution::Dependency(SmolStr::new("@org/ui"), Confidence::Certain)
        );
        assert_eq!(
            resolve(&spec("a.ts", "@org/ui/missing"), &ctx),
            Resolution::Dependency(SmolStr::new("@org/ui"), Confidence::Certain)
        );
    }

    #[test]
    fn workspace_name_outranks_a_declared_external_dependency() {
        // `"@org/ui": "workspace:*"` is declared AND a member — it must resolve internal.
        let known = ctx_with(&["packages/ui/index.ts"]);
        let map = members(&[("@org/ui", "packages/ui", Some("packages/ui/index.ts"))]);
        let mut deps = HashSet::default();
        deps.insert(SmolStr::new("@org/ui"));
        let ctx = ResolveCtx::new(&known)
            .with_declared_dependencies(&deps)
            .with_workspace_members(&map);
        assert!(matches!(
            resolve(&spec("a.ts", "@org/ui"), &ctx),
            Resolution::WorkspaceMember { .. }
        ));
    }

    #[test]
    fn non_member_bare_specifier_still_resolves_externally() {
        let known = ctx_with(&[]);
        let map = members(&[("@org/ui", "packages/ui", None)]);
        let ctx = ResolveCtx::new(&known).with_workspace_members(&map);
        assert_eq!(
            resolve(&spec("a.ts", "lodash"), &ctx),
            Resolution::Dependency(SmolStr::new("lodash"), Confidence::Certain)
        );
    }

    #[test]
    fn declared_dependency_shadows_stdlib_name() {
        // The userland `punycode` package is real: declared in the manifest it must resolve
        // as a dependency, not the deprecated builtin — toolkit precedence rule 2.
        let known = ctx_with(&[]);
        let mut deps = HashSet::default();
        deps.insert(SmolStr::new("punycode"));
        let ctx = ResolveCtx::new(&known).with_declared_dependencies(&deps);
        let r = resolve(&spec("src/a.ts", "punycode"), &ctx);
        assert_eq!(
            r,
            Resolution::Dependency(SmolStr::new("punycode"), Confidence::Certain)
        );
        // Undeclared, the same name stays stdlib.
        let ctx = ResolveCtx::new(&known);
        let r = resolve(&spec("src/a.ts", "punycode"), &ctx);
        assert_eq!(r, Resolution::Stdlib);
    }
}
