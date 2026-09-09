//! What the manifests STATE: the unit `package.json` declares and the roots it
//! enters through, the names `tsconfig.json` maps to files, what the FILE
//! declares (a shebang, a test runner's membership), and the degradations
//! (built entries that don't exist, unparseable manifests) that must state
//! nothing. What a path declares is the spec's `file_roles`, gated in
//! `kndo-gates`.

use kndo_adapter_ts::TypeScriptAdapter;
use kndo_contract::adapter::DependencyScope;
use kndo_contract::adapter::SourceFile;
use kndo_contract::evidence::{Attachment, EvidenceSink, MarkerTarget, RootKind, RootTarget};
use kndo_contract::manifest::{ManifestEvidence, Publication, UnitKind};
use kndo_contract::plugin::Plugin;
use kndo_contract::vocab::ProjectPath;

fn evidence(manifest_path: &str, json: &str, files: &[&str]) -> ManifestEvidence {
    kndo_testkit::manifest_evidence(&TypeScriptAdapter::new(), manifest_path, json, files)
}

/// Every root one manifest puts on the graph, the way the engine assembles
/// them: a unit's entries at the colour its kind gives them, then the
/// manifest's own roots — files it says are RUN without entering any unit.
fn manifest_roots(manifest_path: &str, json: &str, files: &[&str]) -> Vec<(String, RootKind)> {
    let read = evidence(manifest_path, json, files);
    let mut out: Vec<(String, RootKind)> = read
        .units
        .iter()
        .flat_map(|u| {
            u.entries
                .iter()
                .map(move |e| (e.as_str().to_string(), u.kind.color()))
        })
        .collect();
    out.extend(
        read.roots
            .iter()
            .map(|r| (r.file.as_str().to_string(), r.kind)),
    );
    out
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

/// `exports` anchors and `imports` does not: the first is the published
/// surface, whose targets are entries whether or not this project names them,
/// and the second is the package talking to ITSELF — a `#flag` target is
/// reached through the alias table or not at all, and rooting it would anchor
/// every internal file a package happens to alias.
#[test]
fn exports_scripts_and_companions_anchor_but_imports_do_not() {
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
            ("pkg/types/a.d.ts", RootKind::Production),
            ("pkg/types/deep/b.d.ts", RootKind::Production),
            ("pkg/scripts/generate.ts", RootKind::Tooling),
        ]
    );
}

#[test]
fn manifest_declares_its_package() {
    let pkgs = evidence(
        "packages/core/package.json",
        r#"{ "name": "@demo/core", "main": "src/index.ts" }"#,
        &["packages/core/src/index.ts"],
    )
    .packages;
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
fn a_shebang_is_reported_and_never_concluded() {
    // The path habits — `*.test.*`, `__tests__/`, `*.config.*`, rc-dotfiles —
    // are the spec's `file_roles`, gated in `kndo-gates`. A `#!` line is not a
    // habit and not a path: it is the file saying the loader runs it. So the
    // extractor reports the marker and one rule says it is a production root;
    // no verdict is concluded here.
    let markers = |path: &str, source: &str| -> Vec<String> {
        kndo_testkit::extract_evidence(&TypeScriptAdapter::new(), path, source)
            .markers
            .iter()
            .filter(|m| matches!(m.on, MarkerTarget::File))
            .map(|m| m.path.to_string())
            .collect()
    };
    assert_eq!(
        markers(
            "scripts/run.mjs",
            "#!/usr/bin/env node\nexport const x = 1;\n"
        ),
        ["#!"]
    );
    for (path, source) in [
        ("src/thing.test.ts", "it('works', () => {});\n"),
        ("src/__tests__/helper.ts", "export const h = 1;\n"),
        ("vite.config.ts", "export default {};\n"),
        (".eslintrc.cjs", "module.exports = {};\n"),
        ("src/plain.ts", "export const x = 1;\n"),
    ] {
        assert_eq!(markers(path, source), Vec::<String>::new(), "{path}");
        assert_eq!(extracted_roots(path, source), vec![], "{path}");
    }
    assert_eq!(
        extracted_roots("scripts/run.mjs", "#!/usr/bin/env node\n"),
        vec![],
        "the extractor concludes no root of its own"
    );
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
fn declared_dependencies_report_every_section() {
    let json = r#"{
        "name": "demo",
        "dependencies": { "express": "^4", "lodash": "*" },
        "devDependencies": { "vitest": "^1" },
        "peerDependencies": { "react": ">=18" },
        "optionalDependencies": { "fsevents": "^2" },
        "scripts": { "not-a-dep": "echo" }
    }"#;
    let mut deps = evidence("package.json", json, &[]).dependencies;
    deps.sort_by(|a, b| a.name.cmp(&b.name));
    let brief: Vec<(&str, Option<DependencyScope>, Option<&str>)> = deps
        .iter()
        .map(|d| {
            (
                d.name.as_str(),
                d.scope,
                d.version_req.as_ref().map(|v| v.spelled.as_str()),
            )
        })
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
        evidence("package.json", "not json", &[])
            .dependencies
            .is_empty()
    );
}

#[test]
fn mentions_are_what_it_spells_outside_declarations_and_prose() {
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
    let mentions = evidence("package.json", json, &[]).mentions;
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

#[test]
fn a_package_states_the_unit_npm_compiles() {
    let read = evidence(
        "packages/core/package.json",
        r#"{
            "name": "@demo/core",
            "private": true,
            "main": "src/index.ts",
            "dependencies": { "@demo/util": "workspace:*" }
        }"#,
        &["packages/core/src/index.ts"],
    );
    assert_eq!(read.units.len(), 1);
    let unit = &read.units[0];
    assert_eq!(unit.name, "@demo/core");
    assert_eq!(unit.kind, UnitKind::Library);
    // `"private": true` is npm's own word for it, so the unit is not published
    // however the package is otherwise shaped.
    assert_eq!(unit.publication, Publication::Unpublished);
    assert!(!unit.is_published());
    assert_eq!(
        unit.entries.iter().map(|e| e.as_str()).collect::<Vec<_>>(),
        ["packages/core/src/index.ts"]
    );
    assert_eq!(
        unit.depends_on,
        [kndo_contract::manifest::UnitDep::on("@demo/util")]
    );

    // A manifest with no entry field is run, not imported — and npm's
    // resolution rule holds either way: what a consumer writes is a specifier
    // this manifest maps to a FILE, so the form is stated even where the kind
    // makes it moot.
    let app = evidence("app/package.json", r#"{ "name": "app" }"#, &[]);
    assert_eq!(app.units[0].kind, UnitKind::Executable);
    assert_eq!(app.units[0].publication, Publication::ByEntry);
    assert!(
        !app.units[0].is_published(),
        "an executable hands out no API"
    );

    // An unnamed manifest still states its unit, named for the directory it
    // sits in — identity is (manifest, name), so the derived name is enough.
    let unnamed = evidence(
        "tools/package.json",
        r#"{ "main": "run.js" }"#,
        &["tools/run.js"],
    );
    assert_eq!(unnamed.units[0].name, "tools");
}

#[test]
fn a_tsconfig_alias_is_a_package_that_resolves_to_a_file() {
    let read = evidence(
        "app/tsconfig.json",
        r#"{
            "compilerOptions": {
                "paths": {
                    "~utils": ["./src/util.ts"],
                    "@/*": ["./src/*"],
                    "dangling": ["./node_modules/nowhere/index.js"],
                    "half/*": ["./src/one.ts"]
                }
            }
        }"#,
        &["app/src/util.ts", "app/src/deep/thing.ts", "app/src/one.ts"],
    );
    let named: Vec<(&str, Option<&str>, &str)> = read
        .packages
        .iter()
        .map(|p| {
            (
                p.name.as_str(),
                p.entry.as_ref().map(|e| e.as_str()),
                p.dir.as_str(),
            )
        })
        .collect();
    // An exact alias names a file; a wildcard names the directory its subpath
    // resolves against; a target outside the project and a half-wildcard
    // mapping state nothing.
    assert_eq!(
        named,
        [
            ("@", None, "app/src"),
            ("~utils", Some("app/src/util.ts"), "app"),
        ]
    );

    // `baseUrl` moves what the targets are relative to.
    let based = evidence(
        "tsconfig.json",
        r##"{ "compilerOptions": { "baseUrl": "./src", "paths": { "#lib": ["./lib.ts"] } } }"##,
        &["src/lib.ts"],
    );
    assert_eq!(
        based.packages[0].entry.as_ref().map(|e| e.as_str()),
        Some("src/lib.ts")
    );

    // A tsconfig states no unit: it configures a compiler, it does not package
    // anything.
    assert!(read.units.is_empty() && read.dependencies.is_empty());
}
