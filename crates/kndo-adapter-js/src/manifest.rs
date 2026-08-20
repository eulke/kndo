//! `package.json` manifest extraction — spec docs/adapters/js-ts.md §4.
//!
//! Scope for this increment: identity (`name`, `private`), scoped dependencies, `exports`
//! surface detection, and roots. Root detection covers `bin` (unconditional — an executable
//! entry point is a root regardless of publish status), `main`/`module`/`exports` gated on
//! `!private` (RFC 0011 §5: an unpublished app's exports are not roots on their own; something
//! must actually import them), and `scripts` → tooling roots (path-looking tokens resolving
//! to known files — spec §7 open question 2's drafted rule). `types`/`typings` are resolution
//! inputs only — `.d.ts` carries no runtime edge (spec §1). `pnpm-workspace.yaml` topology
//! stays deferred (needs a YAML parser this crate doesn't otherwise need).

use kndo_core::adapter::{
    Diagnostic, DiagnosticLevel, ManifestDependency, ManifestFacts, ManifestRoot, ProjectPath,
    ResolveCtx,
};
use kndo_core::vocab::{Confidence, DependencyScope, RootKind};
use smol_str::SmolStr;

pub fn extract(path: &str, content: &[u8], ctx: &ResolveCtx<'_>) -> ManifestFacts {
    let text = match std::str::from_utf8(content) {
        Ok(t) => t,
        Err(_) => return invalid("package.json is not valid UTF-8".to_string()),
    };
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return invalid(format!("invalid JSON: {e}")),
    };
    let Some(obj) = value.as_object() else {
        return invalid("package.json root is not an object".to_string());
    };

    let package_name = obj.get("name").and_then(|v| v.as_str()).map(SmolStr::new);
    let private = obj
        .get("private")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let workspace_members = obj
        .get("workspaces")
        .map(workspace_globs)
        .unwrap_or_default();
    let dependencies = collect_dependencies(obj);
    let declares_surface = obj.contains_key("exports");

    let mut entry_points = Vec::new();
    let mut roots = Vec::new();

    // `bin`: always a root — an executable entry point regardless of publish status.
    if let Some(bin) = obj.get("bin") {
        for spec in string_leaves(bin) {
            entry_points.push(SmolStr::new(&spec));
            if let Some((target, confidence)) = resolve_entry(path, &spec, ctx, Confidence::Certain)
            {
                roots.push(ManifestRoot {
                    kind: RootKind::Production,
                    target,
                    confidence,
                });
            }
        }
    }

    // An import entry must be a *source* file this adapter could claim — real manifests
    // (found dogfooding against colinhacks/zod) have `exports` leaves like "./package.json"
    // (self-reference) and type declarations; resolving a sibling's bare-name import to a
    // .json would be a junk edge, so non-claimable targets never become entries. (They can
    // still be roots when the root logic wants them — this filter is entries-only.)
    let is_source_entry = |target: &ProjectPath| {
        target
            .0
            .rsplit('.')
            .next()
            .is_some_and(|ext| crate::EXTENSIONS.contains(&ext))
    };

    // `main`/`module`: unambiguous single-field entry points, roots only in library mode —
    // but always `resolved_entries` (a sibling importing this package by name resolves
    // through its entry regardless of `private`; RFC 0011 §4).
    let mut resolved_entries = Vec::new();
    for key in ["main", "module"] {
        if let Some(v) = obj.get(key).and_then(|v| v.as_str()) {
            entry_points.push(SmolStr::new(v));
            if let Some((target, confidence)) = resolve_entry(path, v, ctx, Confidence::Certain) {
                if is_source_entry(&target) {
                    resolved_entries.push((target.clone(), confidence));
                }
                if !private {
                    roots.push(ManifestRoot {
                        kind: RootKind::Production,
                        target,
                        confidence,
                    });
                }
            }
        }
    }

    // `exports`: every string leaf, condition precedence unresolved (not needed to know
    // *something* is reachable — only to know *which* condition wins, a resolve()-time
    // concern deferred with the rest of exports-map resolution). `probable`: multiple leaves
    // may be alternate builds of the same target, not all simultaneously "the" entry.
    if let Some(exports) = obj.get("exports") {
        for spec in string_leaves(exports) {
            entry_points.push(SmolStr::new(&spec));
            if let Some((target, confidence)) =
                resolve_entry(path, &spec, ctx, Confidence::Probable)
            {
                if is_source_entry(&target) {
                    resolved_entries.push((target.clone(), confidence));
                }
                if !private {
                    roots.push(ManifestRoot {
                        kind: RootKind::Production,
                        target,
                        confidence,
                    });
                }
            }
        }
    }

    // `types`/`typings`: resolution inputs only, never roots.
    for key in ["types", "typings"] {
        if let Some(v) = obj.get(key).and_then(|v| v.as_str()) {
            entry_points.push(SmolStr::new(v));
        }
    }

    // `scripts` feeds two independent facts, from two independent token slices of each
    // command (whitespace tokens for roots, shell-clause leading tokens for invoked names —
    // no shell parsing, just enough tokenizing to tell the two apart):
    //
    // 1. Tooling roots (spec §4; the §7 open-question-2 drafted rule, implemented as
    //    drafted): any path-looking whitespace token that resolves to a known file is a
    //    tooling root. "Path-looking" (contains `/` or `.`) filters out bare tool names: `ava`
    //    must never root a coincidental ./ava.js, because npm runs the node_modules binary,
    //    not that file. `--flag=./x.js` tokens contribute their value side. `probable`, not
    //    certain — a token match is a heuristic, nothing declares the file.
    // 2. `script_invoked_names`: the leading token of each shell clause (`"xo && ava"` splits
    //    on `&&`/`||`/`;`/`|` into clauses `"xo"`, `"ava"` — the invoked binary is each
    //    clause's *first* token only; later tokens are that binary's own arguments, e.g.
    //    `start` and `--single-run` in `"karma start --single-run"` are NOT candidate names).
    //    A CLI-only devDependency never gets an `ImportsDependency` edge, so this is what lets
    //    dependency hygiene (RFC 0005 §5, dependency_hygiene.rs) see it as used at all —
    //    cross-referenced there against real declared dependency names, so an unrelated word
    //    happening to be a script's first token (`node`, `tsc`) simply matches nothing.
    let mut script_invoked_names = Vec::new();
    if let Some(scripts) = obj.get("scripts").and_then(|v| v.as_object()) {
        let mut seen: rustc_hash::FxHashSet<_> = rustc_hash::FxHashSet::default();
        for command in scripts.values().filter_map(|v| v.as_str()) {
            for clause in command.split(['&', '|', ';']) {
                if let Some(first) = clause.split_whitespace().next() {
                    if !(first.starts_with('-') || first.contains('/') || first.contains('.')) {
                        script_invoked_names.push(SmolStr::new(first));
                    }
                }
            }
            for raw_token in command.split_whitespace() {
                let token = match raw_token.split_once('=') {
                    Some((flag, value)) if flag.starts_with('-') => value,
                    _ => raw_token,
                };
                if !(token.contains('/') || token.contains('.')) {
                    continue;
                }
                if let Some((target, confidence)) =
                    resolve_entry(path, token, ctx, Confidence::Probable)
                {
                    if seen.insert(target.clone()) {
                        roots.push(ManifestRoot {
                            kind: RootKind::Tooling,
                            target,
                            confidence,
                        });
                    }
                }
            }
        }
    }

    ManifestFacts {
        package_name,
        private,
        workspace_members,
        dependencies,
        entry_points,
        script_invoked_names,
        roots,
        resolved_entries,
        declares_surface,
        diagnostics: Vec::new(),
    }
}

fn invalid(message: String) -> ManifestFacts {
    ManifestFacts {
        diagnostics: vec![Diagnostic {
            level: DiagnosticLevel::Warn,
            path: None, // the core fills this in when merging, same as FileFacts diagnostics
            message,
            span: None,
        }],
        ..ManifestFacts::default()
    }
}

/// Entry-point specifiers are always manifest-relative — never a bare-package lookup, unlike
/// an ordinary import specifier — so this resolves directly against the manifest's directory
/// with the same candidate ladder `resolve()` uses for relative imports, instead of going
/// through the bare-vs-relative dispatch that a `./`-less "index.js" would trip over.
fn resolve_entry(
    manifest_path: &str,
    spec: &str,
    ctx: &ResolveCtx<'_>,
    confidence: Confidence,
) -> Option<(ProjectPath, Confidence)> {
    use kndo_adapter_toolkit::paths;
    let base = paths::join(paths::dirname(manifest_path), spec);
    for candidate in crate::resolution::candidates(&base) {
        let path = ProjectPath(SmolStr::new(candidate));
        if ctx.contains(&path) {
            return Some((path, confidence));
        }
    }
    None
}

fn collect_dependencies(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Vec<ManifestDependency> {
    let mut deps = Vec::new();
    for (key, scope) in [
        ("dependencies", DependencyScope::Prod),
        ("devDependencies", DependencyScope::Dev),
        ("peerDependencies", DependencyScope::Peer),
        ("optionalDependencies", DependencyScope::Optional),
    ] {
        if let Some(map) = obj.get(key).and_then(|v| v.as_object()) {
            for (name, version) in map {
                deps.push(ManifestDependency {
                    name: SmolStr::new(name),
                    version_req: SmolStr::new(version.as_str().unwrap_or("")),
                    scope,
                });
            }
        }
    }
    deps
}

/// npm `workspaces` is either a bare glob array or `{ packages: [...] }`.
fn workspace_globs(value: &serde_json::Value) -> Vec<SmolStr> {
    let array = match value {
        serde_json::Value::Array(arr) => Some(arr),
        serde_json::Value::Object(obj) => obj.get("packages").and_then(|v| v.as_array()),
        _ => None,
    };
    array
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(SmolStr::new)
                .collect()
        })
        .unwrap_or_default()
}

fn string_leaves(value: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_string_leaves(value, &mut out);
    out
}

fn collect_string_leaves(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Object(map) => {
            for v in map.values() {
                collect_string_leaves(v, out);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                collect_string_leaves(v, out);
            }
        }
        _ => {}
    }
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

    fn extract_at(path: &str, json: &str, known: &HashSet<ProjectPath>) -> ManifestFacts {
        extract(path, json.as_bytes(), &ResolveCtx::new(known))
    }

    #[test]
    fn name_and_private_and_dependencies() {
        let known = ctx_with(&["package.json"]);
        let facts = extract_at(
            "package.json",
            r#"{
                "name": "@org/app",
                "private": true,
                "dependencies": { "lodash": "^4.0.0" },
                "devDependencies": { "vitest": "^1.0.0" },
                "peerDependencies": { "react": "^18.0.0" },
                "optionalDependencies": { "fsevents": "^2.0.0" }
            }"#,
            &known,
        );
        assert_eq!(facts.package_name.as_deref(), Some("@org/app"));
        assert!(facts.private);
        assert_eq!(facts.dependencies.len(), 4);
        let scope_of = |name: &str| {
            facts
                .dependencies
                .iter()
                .find(|d| d.name == name)
                .unwrap()
                .scope
        };
        assert_eq!(scope_of("lodash"), DependencyScope::Prod);
        assert_eq!(scope_of("vitest"), DependencyScope::Dev);
        assert_eq!(scope_of("react"), DependencyScope::Peer);
        assert_eq!(scope_of("fsevents"), DependencyScope::Optional);
    }

    #[test]
    fn bin_string_is_root_even_when_private() {
        let known = ctx_with(&["package.json", "cli.js"]);
        let facts = extract_at(
            "package.json",
            r#"{ "private": true, "bin": "./cli.js" }"#,
            &known,
        );
        assert_eq!(facts.roots.len(), 1);
        assert_eq!(facts.roots[0].kind, RootKind::Production);
        assert_eq!(facts.roots[0].target, ProjectPath(SmolStr::new("cli.js")));
        assert_eq!(facts.roots[0].confidence, Confidence::Certain);
    }

    #[test]
    fn bin_object_produces_one_root_per_entry() {
        let known = ctx_with(&["package.json", "bin/a.js", "bin/b.js"]);
        let facts = extract_at(
            "package.json",
            r#"{ "bin": { "tool-a": "./bin/a.js", "tool-b": "./bin/b.js" } }"#,
            &known,
        );
        assert_eq!(facts.roots.len(), 2);
    }

    #[test]
    fn main_is_root_for_library_mode_only() {
        let known = ctx_with(&["package.json", "index.js"]);

        let lib = extract_at("package.json", r#"{ "main": "./index.js" }"#, &known);
        assert_eq!(lib.roots.len(), 1);

        let app = extract_at(
            "package.json",
            r#"{ "private": true, "main": "./index.js" }"#,
            &known,
        );
        assert!(
            app.roots.is_empty(),
            "RFC 0011 §5: a private app's entry point is not a root on its own"
        );
    }

    #[test]
    fn main_pointing_nowhere_produces_no_root() {
        // "main" targeting a dist/ build output that doesn't exist in the analyzed source
        // tree resolves to nothing — correct, not a bug: there is genuinely no file there.
        let known = ctx_with(&["package.json"]);
        let facts = extract_at("package.json", r#"{ "main": "./dist/index.js" }"#, &known);
        assert!(facts.roots.is_empty());
    }

    #[test]
    fn exports_string_leaves_become_probable_roots() {
        let known = ctx_with(&["package.json", "index.js", "sub.js"]);
        let facts = extract_at(
            "package.json",
            r#"{ "exports": { ".": "./index.js", "./sub": { "import": "./sub.js" } } }"#,
            &known,
        );
        assert_eq!(facts.roots.len(), 2);
        assert!(facts
            .roots
            .iter()
            .all(|r| r.confidence == Confidence::Probable));
        assert!(facts.declares_surface);
    }

    #[test]
    fn types_field_never_becomes_a_root() {
        let known = ctx_with(&["package.json", "index.d.ts"]);
        let facts = extract_at("package.json", r#"{ "types": "./index.d.ts" }"#, &known);
        assert!(facts.roots.is_empty());
        assert!(facts.entry_points.iter().any(|e| e == "./index.d.ts"));
    }

    // ---------------------------------------------------------------- scripts → tooling roots

    #[test]
    fn script_file_token_becomes_a_probable_tooling_root() {
        let known = ctx_with(&["package.json", "benchmark.js"]);
        let facts = extract_at(
            "package.json",
            r#"{ "scripts": { "bench": "node benchmark.js" } }"#,
            &known,
        );
        assert_eq!(facts.roots.len(), 1);
        assert_eq!(facts.roots[0].kind, RootKind::Tooling);
        assert_eq!(
            facts.roots[0].target,
            ProjectPath(SmolStr::new("benchmark.js"))
        );
        assert_eq!(facts.roots[0].confidence, Confidence::Probable);
    }

    #[test]
    fn bare_tool_names_never_root_coincidental_files() {
        // npm runs node_modules/.bin/ava — a root-level ava.js is NOT what "ava" invokes.
        let known = ctx_with(&["package.json", "ava.js", "xo.js"]);
        let facts = extract_at(
            "package.json",
            r#"{ "scripts": { "test": "xo && ava" } }"#,
            &known,
        );
        assert!(facts.roots.is_empty());
    }

    #[test]
    fn flag_value_tokens_contribute_their_path_side() {
        let known = ctx_with(&["package.json", "webpack.config.js"]);
        let facts = extract_at(
            "package.json",
            r#"{ "scripts": { "build": "webpack --config=./webpack.config.js" } }"#,
            &known,
        );
        assert_eq!(facts.roots.len(), 1);
        assert_eq!(
            facts.roots[0].target,
            ProjectPath(SmolStr::new("webpack.config.js"))
        );
    }

    #[test]
    fn unknown_script_tokens_produce_no_roots() {
        let known = ctx_with(&["package.json"]);
        let facts = extract_at(
            "package.json",
            r#"{ "scripts": { "build": "node missing.js && tsc -p tsconfig.build.json" } }"#,
            &known,
        );
        assert!(facts.roots.is_empty());
    }

    #[test]
    fn the_same_script_file_roots_once_across_scripts() {
        let known = ctx_with(&["package.json", "run.js"]);
        let facts = extract_at(
            "package.json",
            r#"{ "scripts": { "a": "node run.js", "b": "node run.js --fast" } }"#,
            &known,
        );
        assert_eq!(facts.roots.len(), 1);
    }

    #[test]
    fn script_roots_apply_even_when_private() {
        // Unlike main/exports (library mode), a script reference is direct evidence of
        // invocation regardless of publish status — same reasoning as bin.
        let known = ctx_with(&["package.json", "scripts/build.mjs"]);
        let facts = extract_at(
            "package.json",
            r#"{ "private": true, "scripts": { "build": "node scripts/build.mjs" } }"#,
            &known,
        );
        assert_eq!(facts.roots.len(), 1);
        assert_eq!(facts.roots[0].kind, RootKind::Tooling);
    }

    // ---------------------------------------------------------------- resolved entries

    #[test]
    fn resolved_entries_are_populated_even_for_private_packages() {
        // Roots are private-gated; resolved entries are not — a sibling importing a private
        // member by name still resolves through its entry (RFC 0011 §4).
        let known = ctx_with(&["package.json", "src/index.ts"]);
        let facts = extract_at(
            "package.json",
            r#"{ "private": true, "main": "src/index.ts" }"#,
            &known,
        );
        assert!(facts.roots.is_empty());
        assert_eq!(facts.resolved_entries.len(), 1);
        assert_eq!(
            facts.resolved_entries[0].0,
            ProjectPath(SmolStr::new("src/index.ts"))
        );
        assert_eq!(facts.resolved_entries[0].1, Confidence::Certain);
    }

    #[test]
    fn main_entry_precedes_exports_leaves() {
        let known = ctx_with(&["package.json", "main.ts", "extra.ts"]);
        let facts = extract_at(
            "package.json",
            r#"{ "main": "main.ts", "exports": { "./extra": "./extra.ts" } }"#,
            &known,
        );
        assert_eq!(
            facts.resolved_entries[0].0,
            ProjectPath(SmolStr::new("main.ts"))
        );
        assert_eq!(facts.resolved_entries[0].1, Confidence::Certain);
        assert_eq!(facts.resolved_entries[1].1, Confidence::Probable);
    }

    // ---------------------------------------------------------------- scripts → invoked names

    #[test]
    fn chained_commands_each_contribute_their_leading_token() {
        let known = ctx_with(&["package.json"]);
        let facts = extract_at(
            "package.json",
            r#"{ "scripts": { "test": "xo && ava && tsd" } }"#,
            &known,
        );
        assert_eq!(
            facts.script_invoked_names,
            vec![SmolStr::new("xo"), SmolStr::new("ava"), SmolStr::new("tsd")]
        );
    }

    #[test]
    fn only_the_leading_token_of_a_clause_is_a_candidate_name() {
        // "start" and "--single-run" are `karma`'s own arguments, not invoked tools.
        let known = ctx_with(&["package.json"]);
        let facts = extract_at(
            "package.json",
            r#"{ "scripts": { "test:browser": "karma start --single-run" } }"#,
            &known,
        );
        assert_eq!(facts.script_invoked_names, vec![SmolStr::new("karma")]);
    }

    #[test]
    fn path_looking_leading_tokens_are_not_candidate_names() {
        // `node` is the invoked binary (not a project dependency, harmless if it matches
        // nothing); `scripts/build.mjs` is a path, already handled as a tooling root.
        let known = ctx_with(&["package.json", "scripts/build.mjs"]);
        let facts = extract_at(
            "package.json",
            r#"{ "scripts": { "build": "node scripts/build.mjs" } }"#,
            &known,
        );
        assert_eq!(facts.script_invoked_names, vec![SmolStr::new("node")]);
    }

    #[test]
    fn pipe_and_semicolon_also_split_clauses() {
        let known = ctx_with(&["package.json"]);
        let facts = extract_at(
            "package.json",
            r#"{ "scripts": { "coverage": "cat ./coverage/lcov.info | coveralls; echo done" } }"#,
            &known,
        );
        assert!(facts
            .script_invoked_names
            .contains(&SmolStr::new("coveralls")));
        assert!(facts.script_invoked_names.contains(&SmolStr::new("echo")));
        // `cat` is path-looking (`./coverage/lcov.info` is its argument, `cat` itself is not
        // path-looking) — included; harmless since nothing project-side is ever named `cat`.
        assert!(facts.script_invoked_names.contains(&SmolStr::new("cat")));
    }

    #[test]
    fn workspaces_array_and_object_forms() {
        let known = ctx_with(&["package.json"]);
        let array = extract_at(
            "package.json",
            r#"{ "workspaces": ["packages/*"] }"#,
            &known,
        );
        assert_eq!(array.workspace_members, vec![SmolStr::new("packages/*")]);

        let object = extract_at(
            "package.json",
            r#"{ "workspaces": { "packages": ["packages/*"] } }"#,
            &known,
        );
        assert_eq!(object.workspace_members, vec![SmolStr::new("packages/*")]);
    }

    #[test]
    fn malformed_json_degrades_to_a_diagnostic_not_a_panic() {
        let known = ctx_with(&["package.json"]);
        let facts = extract_at("package.json", "{ not json", &known);
        assert_eq!(facts.diagnostics.len(), 1);
        assert!(facts.roots.is_empty());
        assert!(facts.dependencies.is_empty());
    }

    #[test]
    fn nested_manifest_resolves_entries_relative_to_its_own_directory() {
        let known = ctx_with(&["packages/ui/package.json", "packages/ui/index.js"]);
        let facts = extract_at(
            "packages/ui/package.json",
            r#"{ "main": "./index.js" }"#,
            &known,
        );
        assert_eq!(
            facts.roots[0].target,
            ProjectPath(SmolStr::new("packages/ui/index.js"))
        );
    }
}
