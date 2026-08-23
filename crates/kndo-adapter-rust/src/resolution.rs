//! Import resolution (docs/adapters/rust.md §3): pure path arithmetic over `ResolveCtx`'s
//! known file set, mirroring the module-tree rules the compiler applies — never querying it.
//!
//! Anchors: `crate::` → the owning package's crate root (`src/lib.rs`, else `src/main.rs` —
//! the nearest ancestor directory holding a `Cargo.toml`); `self::` → the importing file's
//! own module directory; `super::` → one module step up, iterated. A *directory-owner* file
//! (`mod.rs`, `lib.rs`, `main.rs`) owns its directory; a named file `a.rs` owns child
//! directory `a/`.
//!
//! Tail rule (spec §3, two-step): try the full path as a module file; on a miss, drop the
//! last segment and resolve the rest — the tail was an item, and the import's binding
//! resolves it inside the target file. One rule, no name-shape heuristics.
//!
//! Bare first segment precedence: stdlib (the five-crate extern prelude) → workspace member
//! (with `_` ↔ `-` normalization, returning the *declared* spelling) → declared dependency →
//! local module (`self::` retry — expression paths may name sibling modules bare) →
//! `Dependency(name, Probable)` as the undeclared-crate fallback (what feeds the
//! `undeclared` analysis; a real local name resolves in the earlier step and never gets here).

use kndo_core::adapter::{ImportSpec, ProjectPath, Resolution, ResolveCtx, WorkspaceMember};
use kndo_core::vocab::Confidence;
use smol_str::SmolStr;

/// The extern-prelude sysroot crates — a language-stability guarantee, hand-maintained by
/// design (spec §3), not generated toolchain data.
const STDLIB: [&str; 5] = ["std", "core", "alloc", "proc_macro", "test"];

pub(crate) fn resolve(spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
    let s = spec.specifier.as_str();
    let from = spec.from.0.as_str();

    // `file:<relative path>` — #[path] mods and include! literals: relative to the file's dir.
    if let Some(rel) = s.strip_prefix("file:") {
        let joined = normalize(&format!("{}/{rel}", dirname(from)));
        let path = ProjectPath(SmolStr::new(joined));
        return if ctx.contains(&path) {
            Resolution::File(path, Confidence::Certain)
        } else {
            Resolution::Unresolved
        };
    }

    let segments: Vec<&str> = s.split("::").filter(|p| !p.is_empty()).collect();
    let Some((&root, rest)) = segments.split_first() else {
        return Resolution::Unresolved;
    };

    match root {
        "crate" => {
            let Some((src_dir, root_file)) = crate_root(from, ctx) else {
                return Resolution::Unresolved;
            };
            resolve_in_module(&src_dir, Some(root_file), rest, ctx)
        }
        "self" => {
            let base = own_module_dir(from);
            resolve_in_module(&base, Some(from.to_string()), rest, ctx)
        }
        "super" => {
            // Iterated supers: `super::super::x`.
            let mut supers = 1;
            let mut rest = rest;
            while rest.first() == Some(&"super") {
                supers += 1;
                rest = &rest[1..];
            }
            let mut dir = own_module_dir(from);
            for _ in 0..supers {
                dir = parent_module_dir(&dir);
            }
            resolve_in_module(&dir, None, rest, ctx)
        }
        _ => resolve_bare(root, rest, from, ctx),
    }
}

/// Resolve `segments` inside module directory `base`. `self_file` is the file the empty path
/// resolves to (the module's own file), when known. Two-step tail rule per the module docs.
fn resolve_in_module(
    base: &str,
    self_file: Option<String>,
    segments: &[&str],
    ctx: &ResolveCtx<'_>,
) -> Resolution {
    if segments.is_empty() {
        // `use self;` / a path that consumed every segment: the module's own file.
        let file = match self_file {
            Some(f) => f,
            None => match module_file_for_dir(base, ctx) {
                Some(f) => f,
                None => return Resolution::Unresolved,
            },
        };
        let path = ProjectPath(SmolStr::new(file));
        return if ctx.contains(&path) {
            Resolution::File(path, Confidence::Certain)
        } else {
            Resolution::Unresolved
        };
    }
    // Step 1: full path as a module file.
    if let Some(path) = walk_module_path(base, segments, ctx) {
        return Resolution::File(path, Confidence::Certain);
    }
    // Step 2: the tail was an item — resolve the parent module, bindings do the rest.
    let (rest, _tail) = segments.split_at(segments.len() - 1);
    if rest.is_empty() {
        let file = match self_file {
            Some(f) => f,
            None => match module_file_for_dir(base, ctx) {
                Some(f) => f,
                None => return Resolution::Unresolved,
            },
        };
        let path = ProjectPath(SmolStr::new(file));
        return if ctx.contains(&path) {
            Resolution::File(path, Confidence::Certain)
        } else {
            Resolution::Unresolved
        };
    }
    match walk_module_path(base, rest, ctx) {
        Some(path) => Resolution::File(path, Confidence::Certain),
        None => Resolution::Unresolved,
    }
}

/// `base/a/b` for segments `[a, b]`: each segment is `<dir>/<seg>.rs` or `<dir>/<seg>/mod.rs`;
/// only the final segment must land on an existing file (intermediates just extend the dir —
/// `a.rs` and `a/mod.rs` both put children in `a/`).
fn walk_module_path(base: &str, segments: &[&str], ctx: &ResolveCtx<'_>) -> Option<ProjectPath> {
    let mut dir = base.to_string();
    for (i, seg) in segments.iter().enumerate() {
        let last = i == segments.len() - 1;
        if last {
            let as_file = ProjectPath(SmolStr::new(format!("{dir}/{seg}.rs")));
            if ctx.contains(&as_file) {
                return Some(as_file);
            }
            let as_mod = ProjectPath(SmolStr::new(format!("{dir}/{seg}/mod.rs")));
            if ctx.contains(&as_mod) {
                return Some(as_mod);
            }
            return None;
        }
        dir = format!("{dir}/{seg}");
    }
    None
}

fn resolve_bare(root: &str, rest: &[&str], from: &str, ctx: &ResolveCtx<'_>) -> Resolution {
    if STDLIB.contains(&root) {
        return Resolution::Stdlib;
    }

    // Workspace member, `_` ↔ `-` normalized; the DECLARED spelling is returned so
    // dependency hygiene cross-references correctly (spec §3).
    let hyphenated = root.replace('_', "-");
    let member = ctx
        .workspace_member(root)
        .map(|m| (root.to_string(), m))
        .or_else(|| {
            ctx.workspace_member(&hyphenated)
                .map(|m| (hyphenated.clone(), m))
        });
    if let Some((declared, member)) = member {
        // A package's own bins and tests import its lib BY NAME (`use cycles::…` from
        // src/main.rs or tests/) — that is the self-crate, not a dependency: resolve to
        // plain files so no ImportsDependency edge (and no phantom `undeclared`) appears.
        let self_crate = if member.dir.is_empty() {
            !from.contains('/') || !from.starts_with("crates/") || {
                // root-package member: every project file belongs to it unless a nested
                // member owns it — the nested member would have matched by name instead.
                true
            }
        } else {
            from.starts_with(&format!("{}/", member.dir))
        };
        let resolution = resolve_into_member(&declared, member, rest, ctx);
        if self_crate {
            return match resolution {
                Resolution::WorkspaceMember {
                    target, confidence, ..
                } => Resolution::File(target, confidence),
                other => other,
            };
        }
        return resolution;
    }

    let dep = if ctx.is_declared_dependency(&SmolStr::new(root)) {
        Some(root.to_string())
    } else if ctx.is_declared_dependency(&SmolStr::new(&hyphenated)) {
        Some(hyphenated)
    } else {
        None
    };
    if let Some(declared) = dep {
        return Resolution::Dependency(SmolStr::new(declared), Confidence::Certain);
    }

    // Expression paths name sibling modules bare (`helpers::run()` ≡ `self::helpers::run()`)
    // — try the local module before accusing anyone of an undeclared dependency.
    let mut local_segments = vec![root];
    local_segments.extend_from_slice(rest);
    if let Some(path) = walk_module_path(&own_module_dir(from), &local_segments, ctx) {
        return Resolution::File(path, Confidence::Certain);
    }
    if local_segments.len() > 1 {
        let (parent, _) = local_segments.split_at(local_segments.len() - 1);
        if let Some(path) = walk_module_path(&own_module_dir(from), parent, ctx) {
            return Resolution::File(path, Confidence::Certain);
        }
    }

    // A crate-shaped name matching nothing declared: the undeclared-dependency fallback —
    // this is what gives the `undeclared` analysis an edge to judge. Type-shaped roots
    // (`Vec::new`) never reach resolve (extraction routes them through references).
    if root
        .chars()
        .next()
        .is_some_and(|c| c.is_lowercase() || c == '_')
    {
        return Resolution::Dependency(SmolStr::new(root), Confidence::Probable);
    }
    Resolution::Unresolved
}

/// `use member_name::a::b` — the sibling's entry for the bare name, or a module walk into
/// its `src/` for subpaths (deep imports recorded, RFC 0011 §4).
fn resolve_into_member(
    declared: &str,
    member: &WorkspaceMember,
    rest: &[&str],
    ctx: &ResolveCtx<'_>,
) -> Resolution {
    let entry = member.entry.as_ref();
    if rest.is_empty() {
        return match entry {
            Some((path, confidence)) => Resolution::WorkspaceMember {
                name: SmolStr::new(declared),
                target: path.clone(),
                confidence: *confidence,
            },
            None => Resolution::Dependency(SmolStr::new(declared), Confidence::Certain),
        };
    }
    let src = if member.dir.is_empty() {
        "src".to_string()
    } else {
        format!("{}/src", member.dir)
    };
    if let Some(path) = walk_module_path(&src, rest, ctx) {
        return Resolution::WorkspaceMember {
            name: SmolStr::new(declared),
            target: path,
            confidence: Confidence::Certain,
        };
    }
    if rest.len() > 1 {
        let (parent, _) = rest.split_at(rest.len() - 1);
        if let Some(path) = walk_module_path(&src, parent, ctx) {
            return Resolution::WorkspaceMember {
                name: SmolStr::new(declared),
                target: path,
                confidence: Confidence::Certain,
            };
        }
    }
    // The item lives on the entry (`use member::Item`).
    match entry {
        Some((path, confidence)) => Resolution::WorkspaceMember {
            name: SmolStr::new(declared),
            target: path.clone(),
            confidence: *confidence,
        },
        None => Resolution::Dependency(SmolStr::new(declared), Confidence::Certain),
    }
}

// ---------------------------------------------------------------- path arithmetic

fn dirname(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn is_dir_owner(path: &str) -> bool {
    matches!(basename(path), "mod.rs" | "lib.rs" | "main.rs")
}

/// The directory this file's child modules live in (spec §3): owners own their directory;
/// `a.rs` owns `a/`.
fn own_module_dir(path: &str) -> String {
    let dir = dirname(path);
    if is_dir_owner(path) {
        dir.to_string()
    } else {
        let stem = basename(path).trim_end_matches(".rs");
        if dir.is_empty() {
            stem.to_string()
        } else {
            format!("{dir}/{stem}")
        }
    }
}

/// One module step up from a module directory: the parent module's own children-directory —
/// which is simply the filesystem parent (both `a.rs` and `a/mod.rs` parent to `a/`'s
/// container).
fn parent_module_dir(dir: &str) -> String {
    dirname(dir).to_string()
}

/// The file that IS the module owning directory `dir`: `dir/mod.rs`, else the sibling
/// `<dir>.rs` (`src/a.rs` owns `src/a/`) — `None` when neither exists (e.g. `src/` itself,
/// whose owner is the crate root and always passed explicitly).
fn module_file_for_dir(dir: &str, ctx: &ResolveCtx<'_>) -> Option<String> {
    let as_mod = format!("{dir}/mod.rs");
    if ctx.contains(&ProjectPath(SmolStr::new(as_mod.as_str()))) {
        return Some(as_mod);
    }
    let sibling = format!("{dir}.rs");
    if ctx.contains(&ProjectPath(SmolStr::new(sibling.as_str()))) {
        return Some(sibling);
    }
    None
}

/// The owning package's module-tree anchor: its source directory and crate-root file
/// (`lib.rs` preferred over `main.rs`, spec §3). Nearest ancestor with a Cargo.toml; when
/// that package doesn't follow the `src/` convention, the manifest's own declared targets
/// decide — a `[[bin]] path = "crates/core/main.rs"` roots a whole module tree there, and
/// every `crate::` path inside it anchors on the target file's directory.
fn crate_root(from: &str, ctx: &ResolveCtx<'_>) -> Option<(String, String)> {
    let mut dir = dirname(from).to_string();
    loop {
        let manifest = if dir.is_empty() {
            "Cargo.toml".to_string()
        } else {
            format!("{dir}/Cargo.toml")
        };
        if ctx.contains(&ProjectPath(SmolStr::new(manifest))) {
            let src = if dir.is_empty() {
                "src".to_string()
            } else {
                format!("{dir}/src")
            };
            for candidate in ["lib.rs", "main.rs"] {
                let path = format!("{src}/{candidate}");
                if ctx.contains(&ProjectPath(SmolStr::new(path.as_str()))) {
                    return Some((src, path));
                }
            }
            return manifest_target_anchor(from, &dir, ctx);
        }
        if dir.is_empty() {
            return None;
        }
        dir = dirname(&dir).to_string();
    }
}

/// Convention miss (no `src/lib.rs` / `src/main.rs` under the owning manifest): anchor on
/// the manifest's *declared* targets instead. Among the owning member's target files whose
/// directory contains `from`, the deepest wins (the target owns its module subtree);
/// same-directory ties prefer `lib.rs`, mirroring the convention path's preference.
fn manifest_target_anchor(
    from: &str,
    manifest_dir: &str,
    ctx: &ResolveCtx<'_>,
) -> Option<(String, String)> {
    let member = ctx
        .workspace_members_iter()
        .find(|m| m.dir.as_str() == manifest_dir)?;
    let from_dir = dirname(from);
    member
        .targets
        .iter()
        .map(|t| t.0.as_str())
        .filter(|t| dir_contains(dirname(t), from_dir))
        .max_by_key(|t| (dirname(t).len(), basename(t) == "lib.rs"))
        .map(|t| (dirname(t).to_string(), t.to_string()))
}

/// Whether `dir` is `from_dir` itself or a path ancestor of it (`""`, the project root,
/// contains everything).
fn dir_contains(dir: &str, from_dir: &str) -> bool {
    dir.is_empty()
        || from_dir == dir
        || (from_dir.len() > dir.len()
            && from_dir.starts_with(dir)
            && from_dir.as_bytes()[dir.len()] == b'/')
}

/// Normalize `a/b/../c` and `./` segments — include!/#[path] literals use them.
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::{FxHashMap, FxHashSet};

    fn known(files: &[&str]) -> FxHashSet<ProjectPath> {
        files
            .iter()
            .map(|f| ProjectPath(SmolStr::new(*f)))
            .collect()
    }

    fn spec(specifier: &str, from: &str) -> ImportSpec {
        ImportSpec {
            specifier: SmolStr::new(specifier),
            from: ProjectPath(SmolStr::new(from)),
        }
    }

    fn file_of(r: Resolution) -> String {
        match r {
            Resolution::File(p, _) => p.0.to_string(),
            other => panic!("expected File, got {other:?}"),
        }
    }

    #[test]
    fn crate_paths_walk_the_module_tree_from_the_crate_root() {
        let files = known(&[
            "Cargo.toml",
            "src/lib.rs",
            "src/a.rs",
            "src/a/b.rs",
            "src/c/mod.rs",
        ]);
        let ctx = ResolveCtx::new(&files);
        assert_eq!(
            file_of(resolve(&spec("crate::a", "src/lib.rs"), &ctx)),
            "src/a.rs"
        );
        assert_eq!(
            file_of(resolve(&spec("crate::a::b", "src/lib.rs"), &ctx)),
            "src/a/b.rs"
        );
        assert_eq!(
            file_of(resolve(&spec("crate::c", "src/lib.rs"), &ctx)),
            "src/c/mod.rs"
        );
        // Two-step: `Thing` is an item in b.
        assert_eq!(
            file_of(resolve(&spec("crate::a::b::Thing", "src/lib.rs"), &ctx)),
            "src/a/b.rs"
        );
    }

    #[test]
    fn self_and_super_anchor_at_the_module_directories() {
        let files = known(&[
            "Cargo.toml",
            "src/lib.rs",
            "src/a.rs",
            "src/a/child.rs",
            "src/sibling.rs",
        ]);
        let ctx = ResolveCtx::new(&files);
        // a.rs owns a/ — self::child from a.rs.
        assert_eq!(
            file_of(resolve(&spec("self::child", "src/a.rs"), &ctx)),
            "src/a/child.rs"
        );
        // super from a/child.rs is module `a` — an item there lands on a's own file.
        assert_eq!(
            file_of(resolve(&spec("super::item_in_a", "src/a/child.rs"), &ctx)),
            "src/a.rs"
        );
        // …and two supers reach the crate root's children: src/sibling.rs.
        assert_eq!(
            file_of(resolve(
                &spec("super::super::sibling", "src/a/child.rs"),
                &ctx
            )),
            "src/sibling.rs"
        );
        // mod declarations resolve exactly like self:: (they ARE self::).
        assert_eq!(
            file_of(resolve(&spec("self::a", "src/lib.rs"), &ctx)),
            "src/a.rs"
        );
    }

    #[test]
    fn crate_root_prefers_lib_and_respects_nested_packages() {
        let files = known(&[
            "Cargo.toml",
            "src/lib.rs",
            "src/main.rs",
            "src/x.rs",
            "crates/inner/Cargo.toml",
            "crates/inner/src/lib.rs",
            "crates/inner/src/y.rs",
        ]);
        let ctx = ResolveCtx::new(&files);
        assert_eq!(
            file_of(resolve(&spec("crate::x", "src/main.rs"), &ctx)),
            "src/x.rs"
        );
        // A file inside the nested package anchors at ITS crate root.
        assert_eq!(
            file_of(resolve(&spec("crate::y", "crates/inner/src/lib.rs"), &ctx)),
            "crates/inner/src/y.rs"
        );
    }

    #[test]
    fn bare_roots_follow_the_precedence() {
        let files = known(&["Cargo.toml", "src/lib.rs", "src/helpers.rs"]);
        let deps: FxHashSet<SmolStr> = [SmolStr::new("serde-json")].into_iter().collect();
        let mut members: FxHashMap<SmolStr, WorkspaceMember> = FxHashMap::default();
        members.insert(
            SmolStr::new("sibling-crate"),
            WorkspaceMember {
                dir: SmolStr::new("crates/sib"),
                entry: Some((
                    ProjectPath(SmolStr::new("crates/sib/src/lib.rs")),
                    Confidence::Certain,
                )),
                targets: Vec::new(),
            },
        );
        let ctx = ResolveCtx::new(&files)
            .with_declared_dependencies(&deps)
            .with_workspace_members(&members);

        assert!(matches!(
            resolve(&spec("std::collections::HashMap", "src/lib.rs"), &ctx),
            Resolution::Stdlib
        ));
        // Underscore form finds the hyphen-declared dependency, declared spelling returned.
        match resolve(&spec("serde_json::to_string", "src/lib.rs"), &ctx) {
            Resolution::Dependency(name, Confidence::Certain) => {
                assert_eq!(name.as_str(), "serde-json")
            }
            other => panic!("{other:?}"),
        }
        // Workspace member, underscore-normalized.
        match resolve(&spec("sibling_crate::Thing", "src/lib.rs"), &ctx) {
            Resolution::WorkspaceMember { name, target, .. } => {
                assert_eq!(name.as_str(), "sibling-crate");
                assert_eq!(target.0.as_str(), "crates/sib/src/lib.rs");
            }
            other => panic!("{other:?}"),
        }
        // Bare expression path naming a local module resolves locally, never as undeclared.
        assert_eq!(
            file_of(resolve(&spec("helpers::run", "src/lib.rs"), &ctx)),
            "src/helpers.rs"
        );
        // A crate-shaped stranger: the undeclared fallback the analysis feeds on.
        assert!(matches!(
            resolve(&spec("mystery_crate::x", "src/lib.rs"), &ctx),
            Resolution::Dependency(n, Confidence::Probable) if n.as_str() == "mystery_crate"
        ));
    }

    #[test]
    fn declared_targets_anchor_a_module_tree_outside_src() {
        // ripgrep's shape: the root manifest declares `[[bin]] path = "crates/core/main.rs"`
        // and there is no `src/` at all — the bin target's directory owns the module tree,
        // so `crate::` paths from anywhere inside it anchor there (spec §3).
        let files = known(&[
            "Cargo.toml",
            "crates/core/main.rs",
            "crates/core/logger.rs",
            "crates/core/flags/mod.rs",
            "crates/core/flags/parse.rs",
        ]);
        let mut members: FxHashMap<SmolStr, WorkspaceMember> = FxHashMap::default();
        members.insert(
            SmolStr::new("ripgrep"),
            WorkspaceMember {
                dir: SmolStr::new(""),
                entry: None,
                targets: vec![ProjectPath(SmolStr::new("crates/core/main.rs"))],
            },
        );
        let ctx = ResolveCtx::new(&files).with_workspace_members(&members);
        assert_eq!(
            file_of(resolve(&spec("crate::flags", "crates/core/main.rs"), &ctx)),
            "crates/core/flags/mod.rs"
        );
        // …and from a file two module levels deep, back to a crate-root sibling.
        assert_eq!(
            file_of(resolve(
                &spec("crate::logger", "crates/core/flags/parse.rs"),
                &ctx
            )),
            "crates/core/logger.rs"
        );
        assert_eq!(
            file_of(resolve(
                &spec("crate::flags::parse", "crates/core/flags/mod.rs"),
                &ctx
            )),
            "crates/core/flags/parse.rs"
        );
    }

    #[test]
    fn workspace_subpaths_are_deep_imports_into_the_sibling() {
        let files = known(&["crates/sib/src/lib.rs", "crates/sib/src/internal.rs"]);
        let mut members: FxHashMap<SmolStr, WorkspaceMember> = FxHashMap::default();
        members.insert(
            SmolStr::new("sib"),
            WorkspaceMember {
                dir: SmolStr::new("crates/sib"),
                entry: Some((
                    ProjectPath(SmolStr::new("crates/sib/src/lib.rs")),
                    Confidence::Certain,
                )),
                targets: Vec::new(),
            },
        );
        let ctx = ResolveCtx::new(&files).with_workspace_members(&members);
        match resolve(&spec("sib::internal::Secret", "src/lib.rs"), &ctx) {
            Resolution::WorkspaceMember { target, .. } => {
                assert_eq!(target.0.as_str(), "crates/sib/src/internal.rs")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn file_prefix_joins_and_normalizes_relative_to_the_importer() {
        let files = known(&["src/other/loc.rs", "src/gen/output.rs"]);
        let ctx = ResolveCtx::new(&files);
        assert_eq!(
            file_of(resolve(&spec("file:other/loc.rs", "src/a.rs"), &ctx)),
            "src/other/loc.rs"
        );
        assert_eq!(
            file_of(resolve(
                &spec("file:../gen/output.rs", "src/deep/mod.rs"),
                &ctx
            )),
            "src/gen/output.rs"
        );
    }
}
