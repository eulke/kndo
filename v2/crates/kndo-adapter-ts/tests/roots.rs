//! The root story: what package.json declares, what conventions declare, and the
//! degradations (built entries that don't exist, unparseable manifests) that must
//! anchor nothing.

use kndo_adapter_ts::TypeScriptAdapter;
use kndo_contract::adapter::{LanguageAdapter, ResolveContext, SourceFile};
use kndo_contract::evidence::{EvidenceSink, RootKind, RootTarget};
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
