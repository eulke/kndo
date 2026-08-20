//! Import resolution (docs/adapters/go.md §3). Structurally simpler than JS's in one dimension
//! (no relative imports, no dynamic-confidence ladder — every import is a fully-qualified path
//! at `Confidence::Certain`) and genuinely harder in another: a Go import names a *package* (a
//! directory of files), not a file, and `go.mod` module paths have no fixed segment count the
//! way npm's `@scope/name` does — resolving a subpath requires a longest-declared-prefix search,
//! not a fixed split.

use kndo_core::adapter::{ImportSpec, ProjectPath, Resolution, ResolveCtx};
use kndo_core::vocab::Confidence;
use smol_str::SmolStr;

pub fn resolve(spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
    let s = spec.specifier.as_str();

    // 1. Same-module (or, once go.work lands, sibling-module) internal package: every named
    //    manifest in the graph is registered as a workspace member (RFC 0011 §4), including
    //    this project's own single `go.mod` — "the monorepo model with n = 1" — so a plain
    //    longest-prefix search over registered module paths covers both cases with the same
    //    lookup, no special-casing "is this my own module." Segment-by-segment from the full
    //    specifier down to nothing: `go.mod` module paths vary in length, so unlike JS's fixed
    //    `@scope/name` shape there is no fixed prefix length to try first.
    let mut candidate = s;
    loop {
        if let Some(member) = ctx.workspace_member(candidate) {
            if let Some(resolution) = resolve_into_package(spec, s, candidate, member, ctx) {
                return resolution;
            }
            break; // matched a declared module but found no file in the target package
        }
        match candidate.rfind('/') {
            Some(i) => candidate = &candidate[..i],
            None => break,
        }
    }

    // 2. Stdlib: no structural prefix exists in Go the way `node:` does (docs/adapters/go.md
    //    §3), and no subpath→package split applies either (stdlib import paths are referenced
    //    in full, `"encoding/json"`, never shortened) — the whole precedence collapses to "is
    //    this exact path in the generated list." `kndo-adapter-toolkit::stdlib
    //    ::classify_bare_specifier` doesn't fit here: it's built around JS's "package_name is
    //    already the subpath-stripped name" shape and its fallback unconditionally returns
    //    `Dependency`, which would misclassify every one of Go's variable-length module paths
    //    (RFC 0002 §6: "an adapter supplies only what it alone knows" — this adapter checks the
    //    shared `StdlibIndex` directly instead of forcing JS's precedence shape onto it).
    if STDLIB.contains(s) {
        return Resolution::Stdlib;
    }

    // 3. External dependency: longest-declared-`require`-prefix match — the one genuinely new
    //    resolution primitive Go needs that JS's fixed-segment-count convention doesn't
    //    (docs/adapters/go.md §3).
    if let Some(name) = longest_declared_prefix(s, ctx) {
        return Resolution::Dependency(SmolStr::new(&name), Confidence::Certain);
    }

    Resolution::Unresolved
}

/// `candidate` is a workspace member's declared module path that `s` is prefixed by (checked by
/// the caller before this runs); resolves the remaining subpath to a concrete file within that
/// package directory. `""` subpath (importing the module path exactly) still needs *some* file
/// to point a resolution at — Go packages are directories, contracts §2 has no multi-file
/// resolution target (docs/adapters/go.md §3) — so both cases go through `pick_package_file`.
///
/// Which *variant* depends on whose module the importing file lives in (RFC 0012 §10):
///
/// - **Own module** (the matched member's directory is an ancestor of the importing file):
///   plain `Resolution::File`. A module's own subpackages need no `require` entry — a module
///   can't require itself. Using `WorkspaceMember` here was the first cut's bug, caught
///   dogfooding: a module importing its own subpackage read as `undeclared` (a "phantom
///   dependency on itself").
/// - **Sibling module** (a `go.work` workspace member the importer does *not* live in):
///   `Resolution::WorkspaceMember` — assembly derives BOTH edge kinds from it (contracts §2):
///   `ImportsFile` for real cross-module reachability, `ImportsDependency` for the declaration
///   contract, which go.work does **not** waive — each module's `go.mod` must still `require`
///   its siblings for standalone builds (`go mod tidy` adds them), so an undeclared sibling
///   import is a genuine phantom dependency and a declared-but-unimported one is genuinely
///   unused, exactly RFC 0011 §4's model.
///
/// A module nested inside another module's directory tree would blur the ancestry test; Go
/// itself strongly discourages nested modules and the tie simply resolves toward the safer
/// `File` (no dependency-contract accusation).
fn resolve_into_package(
    spec: &ImportSpec,
    s: &str,
    matched_module_path: &str,
    member: &kndo_core::adapter::WorkspaceMember,
    ctx: &ResolveCtx<'_>,
) -> Option<Resolution> {
    let subpath = s
        .strip_prefix(matched_module_path)
        .unwrap_or("")
        .trim_start_matches('/');
    let target_dir = kndo_adapter_toolkit::paths::join(member.dir.as_str(), subpath);
    let target = pick_package_file(&target_dir, ctx)?;

    let importer_dir = kndo_adapter_toolkit::paths::dirname(spec.from.0.as_str());
    let own_module = dir_owns(member.dir.as_str(), importer_dir);
    if own_module {
        Some(Resolution::File(target, Confidence::Certain))
    } else {
        Some(Resolution::WorkspaceMember {
            name: SmolStr::new(matched_module_path),
            target,
            confidence: Confidence::Certain,
        })
    }
}

/// Is `sub` the directory `dir` itself, or nested anywhere under it? `""` = the project root,
/// which owns everything.
fn dir_owns(dir: &str, sub: &str) -> bool {
    dir.is_empty() || sub == dir || sub.starts_with(&format!("{dir}/"))
}

/// The lexicographically-first non-test `.go` file directly in `dir` — deterministic (RFC 0008
/// §4; `ResolveCtx::files_in_dir`'s own iteration order isn't), and *which* file doesn't matter
/// for correctness beyond existing: `FileFacts::unit` (contracts §2) makes every file in the
/// directory equally reachable for symbol resolution regardless of which one `Resolution::File`
/// nominally names (docs/adapters/go.md §3).
fn pick_package_file(dir: &str, ctx: &ResolveCtx<'_>) -> Option<ProjectPath> {
    ctx.files_in_dir(dir)
        .filter(|p| p.0.ends_with(".go") && !p.0.ends_with("_test.go"))
        .min_by(|a, b| a.0.as_str().cmp(b.0.as_str()))
        .cloned()
}

/// Longest module path declared in `go.mod`'s `require` set that `s` is a `/`-prefix of. Unlike
/// step 1's workspace-member search (point lookups against a known-name index), there's no
/// analogous index for *external* dependency names to probe — so this walks the declared set
/// directly. `ResolveCtx` doesn't expose the declared-dependency set for iteration (only
/// membership via `is_declared_dependency`), so this needs the same segment-shrinking probe step
/// 1 uses, just checked against declared-dependency membership instead of workspace members.
fn longest_declared_prefix(s: &str, ctx: &ResolveCtx<'_>) -> Option<String> {
    let mut candidate = s;
    loop {
        if ctx.is_declared_dependency(&SmolStr::new(candidate)) {
            return Some(candidate.to_string());
        }
        match candidate.rfind('/') {
            Some(i) => candidate = &candidate[..i],
            None => return None,
        }
    }
}

/// The `kndo-stdlib v1` dataset (RFC 0002 §6), sourced from `go list std`. Regenerate with
/// `cargo xtask gen-stdlib go`; never hand-edit.
static STDLIB: std::sync::LazyLock<kndo_adapter_toolkit::stdlib::StdlibIndex<'static>> =
    std::sync::LazyLock::new(|| {
        kndo_adapter_toolkit::stdlib::StdlibIndex::parse(include_str!("stdlib.txt"))
            .expect("shipped stdlib.txt is malformed — regenerate: cargo xtask gen-stdlib go")
    });

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::adapter::WorkspaceMember;
    use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

    fn spec(specifier: &str, from: &str) -> ImportSpec {
        ImportSpec {
            specifier: SmolStr::new(specifier),
            from: ProjectPath(SmolStr::new(from)),
        }
    }

    #[test]
    fn stdlib_import_resolves_as_stdlib() {
        let known = HashSet::default();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(
            resolve(&spec("encoding/json", "a.go"), &ctx),
            Resolution::Stdlib
        );
        assert_eq!(resolve(&spec("fmt", "a.go"), &ctx), Resolution::Stdlib);
    }

    #[test]
    fn own_module_subpackage_resolves_to_a_file_in_the_target_directory() {
        let known: HashSet<ProjectPath> = ["a.go", "sub/x.go", "sub/y.go"]
            .iter()
            .map(|p| ProjectPath(SmolStr::new(*p)))
            .collect();
        let mut members = HashMap::default();
        members.insert(
            SmolStr::new("example.com/mod"),
            WorkspaceMember {
                dir: SmolStr::new(""),
                entry: None,
            },
        );
        let ctx = ResolveCtx::new(&known).with_workspace_members(&members);

        let resolved = resolve(&spec("example.com/mod/sub", "a.go"), &ctx);
        match resolved {
            // Plain File, not WorkspaceMember (resolve_into_package's doc: a module's own
            // subpackage needs no ImportsDependency contract — Go has none to validate).
            Resolution::File(target, _) => {
                // Deterministic pick: lexicographically first non-test .go file in `sub/`.
                assert_eq!(target.0.as_str(), "sub/x.go");
            }
            other => panic!("expected File, got {other:?}"),
        }
    }

    #[test]
    fn sibling_module_import_resolves_as_workspace_member() {
        // RFC 0012 §10: the importer lives in moda/, the target module in modb/ — a genuine
        // cross-module edge. WorkspaceMember gives assembly both edge kinds: reachability AND
        // the dependency contract (go.work does not waive `require`; an undeclared sibling is
        // a phantom dependency).
        let known: HashSet<ProjectPath> = ["moda/main.go", "modb/lib.go"]
            .iter()
            .map(|p| ProjectPath(SmolStr::new(*p)))
            .collect();
        let mut members = HashMap::default();
        members.insert(
            SmolStr::new("example.com/a"),
            WorkspaceMember {
                dir: SmolStr::new("moda"),
                entry: None,
            },
        );
        members.insert(
            SmolStr::new("example.com/b"),
            WorkspaceMember {
                dir: SmolStr::new("modb"),
                entry: None,
            },
        );
        let ctx = ResolveCtx::new(&known).with_workspace_members(&members);

        match resolve(&spec("example.com/b", "moda/main.go"), &ctx) {
            Resolution::WorkspaceMember {
                name,
                target,
                confidence,
            } => {
                assert_eq!(name.as_str(), "example.com/b");
                assert_eq!(target.0.as_str(), "modb/lib.go");
                assert_eq!(confidence, Confidence::Certain);
            }
            other => panic!("expected WorkspaceMember, got {other:?}"),
        }
        // And the mirror: the same module resolving its own subpackage stays plain File.
        match resolve(&spec("example.com/b", "modb/lib.go"), &ctx) {
            Resolution::File(target, _) => assert_eq!(target.0.as_str(), "modb/lib.go"),
            other => panic!("expected File, got {other:?}"),
        }
    }

    #[test]
    fn importing_the_module_root_itself_picks_a_file_at_the_root() {
        let known: HashSet<ProjectPath> = ["main.go", "helper.go"]
            .iter()
            .map(|p| ProjectPath(SmolStr::new(*p)))
            .collect();
        let mut members = HashMap::default();
        members.insert(
            SmolStr::new("example.com/mod"),
            WorkspaceMember {
                dir: SmolStr::new(""),
                entry: None,
            },
        );
        let ctx = ResolveCtx::new(&known).with_workspace_members(&members);

        let resolved = resolve(&spec("example.com/mod", "sub/a.go"), &ctx);
        match resolved {
            Resolution::File(target, _) => {
                assert_eq!(target.0.as_str(), "helper.go");
            }
            other => panic!("expected File, got {other:?}"),
        }
    }

    #[test]
    fn external_dependency_resolves_via_longest_declared_prefix() {
        let known = HashSet::default();
        let mut declared = HashSet::default();
        declared.insert(SmolStr::new("golang.org/x/net"));
        let ctx = ResolveCtx::new(&known).with_declared_dependencies(&declared);

        let resolved = resolve(&spec("golang.org/x/net/html", "a.go"), &ctx);
        assert_eq!(
            resolved,
            Resolution::Dependency(SmolStr::new("golang.org/x/net"), Confidence::Certain)
        );
    }

    #[test]
    fn undeclared_external_import_is_unresolved() {
        let known = HashSet::default();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(
            resolve(&spec("github.com/nowhere/nothing", "a.go"), &ctx),
            Resolution::Unresolved
        );
    }
}
