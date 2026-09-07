//! The root story: what package.json declares, what the FILE declares (a
//! shebang, a test runner's membership), and the degradations (built entries
//! that don't exist, unparseable manifests) that must anchor nothing. What a
//! path declares is the spec's `file_roles`, gated in `kndo-gates`.

use kndo_adapter_ts::TypeScriptAdapter;
use kndo_contract::adapter::DependencyScope;
use kndo_contract::adapter::{ResolveContext, SourceFile};
use kndo_contract::evidence::{Attachment, EvidenceSink, RootKind, RootTarget};
use kndo_contract::extension::Extension;
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeSet;

fn cx_files(files: &[&str]) -> BTreeSet<ProjectPath> {
    files.iter().map(|f| ProjectPath::new(*f)).collect()
}

fn manifest_roots(manifest_path: &str, json: &str, files: &[&str]) -> Vec<(String, RootKind)> {
    let known = cx_files(files);
    let cx = ResolveContext::new(&known);
    let path = ProjectPath::new(manifest_path);
    TypeScriptAdapter::new()
        .roots(
            &SourceFile {
                path: &path,
                content: json.as_bytes(),
                region: None,
            },
            &cx,
        )
        .into_iter()
        .map(|r| (r.file.as_str().to_string(), r.kind))
        .collect()
}

#[test]
fn package_json_entry_fields_anchor_roots() {
    let roots = manifest_roots(
        "package.json",
        r#"{
            "main": "index.js",
            "module": "./src/index.mjs",
            "bin": { "tool": "./cli.js" },
            "exports": { ".": { "import": "./src/index.mjs", "require": "./index.js" },
                         "./util": "./src/util.js" }
        }"#,
        &[
            "index.js",
            "src/index.mjs",
            "cli.js",
            "src/util.js",
            "src/other.js",
        ],
    );
    let files: Vec<&str> = roots.iter().map(|(f, _)| f.as_str()).collect();
    assert_eq!(
        files,
        ["cli.js", "index.js", "src/index.mjs", "src/util.js"]
    );
    assert!(roots.iter().all(|(_, k)| *k == RootKind::Production));
}

#[test]
fn entries_resolve_like_imports() {
    // "main" without extension, and a .js entry whose source is the .ts beside it.
    let roots = manifest_roots(
        "pkg/package.json",
        r#"{ "main": "src/index", "module": "./src/entry.js" }"#,
        &["pkg/src/index.ts", "pkg/src/entry.ts"],
    );
    let files: Vec<&str> = roots.iter().map(|(f, _)| f.as_str()).collect();
    assert_eq!(files, ["pkg/src/entry.ts", "pkg/src/index.ts"]);
}

#[test]
fn wildcard_exports_imports_scripts_and_companions_anchor() {
    let roots = manifest_roots(
        "pkg/package.json",
        r##"{
            "exports": { "./types/*": "./types/*" },
            "imports": { "#flag": { "default": "./misc/false.js" } },
            "scripts": { "gen": "tsx scripts/generate.ts --flag" }
        }"##,
        &[
            "pkg/types/a.d.ts",
            "pkg/types/deep/b.d.ts",
            "pkg/misc/false.js",
            "pkg/misc/false.d.ts",
            "pkg/scripts/generate.ts",
            "pkg/src/unrelated.ts",
        ],
    );
    let files: Vec<(&str, RootKind)> = roots.iter().map(|(f, k)| (f.as_str(), *k)).collect();
    assert_eq!(
        files,
        [
            ("pkg/misc/false.d.ts", RootKind::Production),
            ("pkg/misc/false.js", RootKind::Production),
            ("pkg/types/a.d.ts", RootKind::Production),
            ("pkg/types/deep/b.d.ts", RootKind::Production),
            ("pkg/scripts/generate.ts", RootKind::Tooling),
        ]
    );
}

#[test]
fn manifest_declares_its_package() {
    use kndo_contract::adapter::ResolveContext;
    let known = cx_files(&["packages/core/src/index.ts"]);
    let cx = ResolveContext::new(&known);
    let path = ProjectPath::new("packages/core/package.json");
    let pkgs = TypeScriptAdapter::new().packages(
        &SourceFile {
            path: &path,
            content: br#"{ "name": "@demo/core", "main": "src/index.ts" }"#,
            region: None,
        },
        &cx,
    );
    assert_eq!(pkgs.len(), 1);
    assert_eq!(pkgs[0].name, "@demo/core");
    assert_eq!(
        pkgs[0].entry.as_ref().map(|p| p.as_str()),
        Some("packages/core/src/index.ts")
    );
    assert_eq!(pkgs[0].dir, "packages/core");
}

#[test]
fn dangling_and_broken_manifests_anchor_nothing() {
    // A built entry (dist/) not present in the tree, and unparseable JSON.
    assert!(
        manifest_roots(
            "package.json",
            r#"{ "main": "./dist/node/index.js" }"#,
            &["src/index.ts"],
        )
        .is_empty()
    );
    assert!(manifest_roots("package.json", "{ not json", &["src/index.ts"]).is_empty());
}

fn extracted_roots(path: &str, source: &str) -> Vec<(RootKind, bool)> {
    let adapter = TypeScriptAdapter::new();
    let p = ProjectPath::new(path);
    let mut sink = EvidenceSink::new(source.len() as u32, adapter.spec().emits().clone());
    adapter.extract(
        &SourceFile {
            path: &p,
            content: source.as_bytes(),
            region: None,
        },
        &mut sink,
    );
    sink.finish()
        .roots
        .iter()
        .map(|r| (r.kind, matches!(r.target, RootTarget::WholeFile)))
        .collect()
}

#[test]
fn a_shebang_is_the_one_root_the_file_itself_states() {
    // The path habits — `*.test.*`, `__tests__/`, `*.config.*`, rc-dotfiles —
    // are the spec's `file_roles`, gated in `kndo-gates`. A `#!` line is not a
    // habit and not a path: it is the file saying the loader runs it, so it is
    // the one root extraction still concludes.
    assert_eq!(
        extracted_roots(
            "scripts/run.mjs",
            "#!/usr/bin/env node\nexport const x = 1;\n"
        ),
        vec![(RootKind::Production, true)]
    );
    for (path, source) in [
        ("src/thing.test.ts", "it('works', () => {});\n"),
        ("src/__tests__/helper.ts", "export const h = 1;\n"),
        ("vite.config.ts", "export default {};\n"),
        (".eslintrc.cjs", "module.exports = {};\n"),
        ("src/plain.ts", "export const x = 1;\n"),
    ] {
        assert_eq!(extracted_roots(path, source), vec![], "{path}");
    }
}

#[test]
fn a_test_runners_path_states_the_files_membership() {
    // What no path convention can say on the file's behalf: a spec file joins
    // the project in a test run alone, whatever colour a root gives it.
    let attachment = |path: &str, source: &str| {
        kndo_testkit::extract_evidence(&TypeScriptAdapter::new(), path, source).attachment
    };
    assert_eq!(
        attachment("src/thing.test.ts", "it('works', () => {});\n"),
        Attachment::TestOnly
    );
    assert_eq!(
        attachment("src/__tests__/helper.ts", "export const h = 1;\n"),
        Attachment::TestOnly
    );
    assert_eq!(
        attachment("vite.config.ts", "export default {};\n"),
        Attachment::Regular
    );
    assert_eq!(
        attachment("src/plain.ts", "export const x = 1;\n"),
        Attachment::Regular
    );
}

#[test]
fn built_entries_map_to_their_source() {
    // The manifest points at build output that is not in the tree; the same path
    // under src/ (with the compiled-extension swap) is the entry's source.
    let roots = manifest_roots(
        "pkg/package.json",
        r#"{ "name": "demo", "main": "dist/node/index.js" }"#,
        &["pkg/src/node/index.ts"],
    );
    assert_eq!(
        roots,
        [("pkg/src/node/index.ts".to_string(), RootKind::Production)]
    );
    // An existing built entry wins untouched — no mapping fires.
    let literal = manifest_roots(
        "pkg/package.json",
        r#"{ "name": "demo", "main": "lib/index.js" }"#,
        &["pkg/lib/index.js", "pkg/src/index.js"],
    );
    assert_eq!(
        literal,
        [("pkg/lib/index.js".to_string(), RootKind::Production)]
    );
}

#[test]
fn manifest_dependencies_report_every_section() {
    let path = ProjectPath::new("package.json");
    let json = r#"{
        "name": "demo",
        "dependencies": { "express": "^4", "lodash": "*" },
        "devDependencies": { "vitest": "^1" },
        "peerDependencies": { "react": ">=18" },
        "optionalDependencies": { "fsevents": "^2" },
        "scripts": { "not-a-dep": "echo" }
    }"#;
    let mut deps = TypeScriptAdapter::new().manifest_dependencies(&SourceFile {
        path: &path,
        content: json.as_bytes(),
        region: None,
    });
    deps.sort_by(|a, b| a.name.cmp(&b.name));
    let brief: Vec<(&str, Option<DependencyScope>, Option<&str>)> = deps
        .iter()
        .map(|d| (d.name.as_str(), d.scope, d.version_req.as_deref()))
        .collect();
    assert_eq!(
        brief,
        [
            ("express", Some(DependencyScope::Prod), Some("^4")),
            ("fsevents", Some(DependencyScope::Optional), Some("^2")),
            // `*` names no comparable requirement — silence over false skew.
            ("lodash", Some(DependencyScope::Prod), None),
            ("react", Some(DependencyScope::Peer), Some(">=18")),
            ("vitest", Some(DependencyScope::Dev), Some("^1")),
        ]
    );
    assert!(
        TypeScriptAdapter::new()
            .manifest_dependencies(&SourceFile {
                path: &path,
                content: b"not json",
                region: None,
            })
            .is_empty()
    );
}

#[test]
fn manifest_mentions_are_what_it_spells_outside_declarations_and_prose() {
    let path = ProjectPath::new("package.json");
    let json = r#"{
        "name": "demo",
        "description": "an express server with lodash helpers",
        "keywords": ["lodash"],
        "dependencies": {
            "express": "^4", "lodash": "*", "vite": "^5", "jsdom": "^24", "@scope/shim": "^1"
        },
        "devDependencies": { "eslint": "^9", "@biomejs/biome": "^1", "typescript": "^5" },
        "scripts": {
            "lint": "eslint . && @biomejs/biome check src",
            "dev": "node ./node_modules/vite/bin/vite.js --port 3000",
            "typecheck": "tsc -p .",
            "site": "npx marky-markdown@^9.0.1 && npx @scope/tool@1.2.3 run",
            "express-ish": "echo expressive lodash-es"
        },
        "browser": { "jsdom": false, "./node-only.js": "@scope/shim/browser" }
    }"#;
    let mentions = TypeScriptAdapter::new().manifest_mentions(&SourceFile {
        path: &path,
        content: json.as_bytes(),
        region: None,
    });
    let has = |name: &str| mentions.iter().any(|m| m == name);
    // A scoped name is one word, its own slash included; a `browser` alias names
    // the package it maps to, path and all; a `browser` key disabling a package
    // names it; a path into a package and a versioned invocation name the package.
    for named in [
        "eslint",
        "@biomejs/biome",
        "@scope/shim",
        "jsdom",
        "vite",
        "marky-markdown",
        "@scope/tool",
    ] {
        assert!(has(named), "{named} missing from {mentions:?}");
    }
    // `expressive` and `lodash-es` are other words; prose (`description`,
    // `keywords`) never counts; a binary spelled unlike its package (`tsc`) is
    // not a mention of `typescript`.
    for unnamed in ["express", "lodash", "typescript"] {
        assert!(!has(unnamed), "{unnamed} wrongly in {mentions:?}");
    }
    assert!(
        mentions.windows(2).all(|w| w[0] < w[1]),
        "sorted, deduplicated"
    );
}

#[test]
fn a_script_hands_a_runtime_an_entry_however_it_is_spelled() {
    let roots = manifest_roots(
        "package.json",
        r#"{
            "name": "app",
            "scripts": {
                "dev": "node server",
                "debug": "node --inspect-brk server",
                "worker": "tsx src/worker.ts",
                "lint": "eslint src"
            }
        }"#,
        &["server.js", "src/worker.ts", "src/index.js"],
    );
    let paths: Vec<&str> = roots.iter().map(|(p, _)| p.as_str()).collect();
    assert!(paths.contains(&"server.js"), "{paths:?}");
    assert!(paths.contains(&"src/worker.ts"), "{paths:?}");
    // `eslint src` runs no file: a directory handed to a linter roots nothing.
    assert!(!paths.contains(&"src/index.js"), "{paths:?}");
}

/// GitHub Actions launchers read like npm scripts: a workflow step's file from
/// the directory the step runs in, a composite action's own file through
/// `$GITHUB_ACTION_PATH`, a JavaScript action's built entry through its source.
#[test]
fn a_workflow_or_action_step_hands_a_runtime_an_entry_like_a_script() {
    let roots = manifest_roots(
        ".github/workflows/publish.yml",
        "jobs:\n  publish:\n    steps:\n      - run: node scripts/detect-release.ts \"$(git log -1)\"\n      - working-directory: tools\n        run: node render-notes.mjs\n",
        &[
            "scripts/detect-release.ts",
            "tools/render-notes.mjs",
            "scripts/orphan.ts",
        ],
    );
    assert_eq!(
        roots,
        [
            ("scripts/detect-release.ts".to_string(), RootKind::Tooling),
            ("tools/render-notes.mjs".to_string(), RootKind::Tooling),
        ]
    );
    let roots = manifest_roots(
        "action/action.yml",
        "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: node \"$GITHUB_ACTION_PATH/render.mjs\"\n",
        &["action/render.mjs"],
    );
    assert_eq!(
        roots,
        [("action/render.mjs".to_string(), RootKind::Tooling)]
    );
    let roots = manifest_roots(
        "action/action.yml",
        "runs:\n  using: node20\n  main: dist/index.js\n",
        &["action/src/index.ts"],
    );
    assert_eq!(
        roots,
        [("action/src/index.ts".to_string(), RootKind::Production)]
    );
}
