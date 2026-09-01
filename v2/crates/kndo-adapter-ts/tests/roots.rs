//! The root story: what package.json declares, what conventions declare, and the
//! degradations (built entries that don't exist, unparseable manifests) that must
//! anchor nothing.

use kndo_adapter_ts::TypeScriptAdapter;
use kndo_contract::adapter::DependencyScope;
use kndo_contract::adapter::{ResolveContext, SourceFile};
use kndo_contract::evidence::{EvidenceSink, RootKind, RootTarget};
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
fn convention_roots_from_path_and_shebang() {
    assert_eq!(
        extracted_roots(
            "scripts/run.mjs",
            "#!/usr/bin/env node\nexport const x = 1;\n"
        ),
        vec![(RootKind::Production, true)]
    );
    assert_eq!(
        extracted_roots("src/thing.test.ts", "it('works', () => {});\n"),
        vec![(RootKind::Test, true)]
    );
    assert_eq!(
        extracted_roots("src/__tests__/helper.ts", "export const h = 1;\n"),
        vec![(RootKind::Test, true)]
    );
    assert_eq!(
        extracted_roots("vite.config.ts", "export default {};\n"),
        vec![(RootKind::Tooling, true)]
    );
    assert_eq!(
        extracted_roots(".eslintrc.cjs", "module.exports = {};\n"),
        vec![(RootKind::Tooling, true)]
    );
    assert_eq!(
        extracted_roots("src/plain.ts", "export const x = 1;\n"),
        vec![]
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
