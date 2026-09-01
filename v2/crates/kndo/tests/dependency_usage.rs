//! The dependency subjects of `unused` and `test-only` end-to-end: production
//! declarations judged through each adapter's declared spelling, every skip a
//! named abstention, and health counting exactly what was judged.

use kndo::{AbstentionReason, AbstentionScope, Category, Config, RunMode, Subject};
use kndo_testkit::TempProject;

fn dependency_findings(snap: &kndo::Snapshot) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = snap
        .findings
        .iter()
        .filter_map(|f| match &f.subject {
            Subject::Dependency {
                owner_manifest,
                name,
            } => Some((
                f.category.as_str().to_string(),
                owner_manifest.as_str().to_string(),
                name.to_string(),
            )),
            _ => None,
        })
        .filter(|(c, _, _)| c == "unused" || c == "test-only" || c == "undeclared")
        .collect();
    out.sort();
    out
}

#[test]
fn production_dependencies_are_judged_by_who_imports_them() {
    let p = TempProject::new();
    p.file(
        "package.json",
        r#"{
  "name": "app",
  "main": "src/index.js",
  "scripts": { "build": "script-dep src" },
  "browser": { "./src/node.js": "alias-dep" },
  "dependencies": {
    "imported-dep": "^1", "sibling-dep": "^1", "test-dep": "^1",
    "idle-dep": "^1", "script-dep": "^1", "alias-dep": "^1"
  },
  "devDependencies": { "idle-dev": "^1" },
  "peerDependencies": { "idle-peer": "^1" },
  "optionalDependencies": { "idle-optional": "^1" }
}"#,
    );
    // `phantom-dep`: imported, declared nowhere. `fs`: the platform's. `spelled`:
    // an alias the tree names in a literal. `optional-x`: a conditional load.
    p.file(
        "src/index.js",
        "import { a } from \"imported-dep\";\nimport { ph } from \"phantom-dep\";\nimport fs from \"fs\";\nimport { s } from \"spelled\";\nconst alias = \"spelled\";\nfunction lazy() { return require(\"optional-x\"); }\nexport function app() { return a(ph, fs, s, alias, lazy); }\n",
    );
    p.file(
        "src/index.test.js",
        "import { t } from \"test-dep\";\nimport { app } from \"./index.js\";\nt(app());\n",
    );
    // A sibling package imports what only the root declares: hoisting keeps the
    // root's declaration in use — never this package's accusation to make.
    p.file(
        "packages/b/package.json",
        r#"{ "name": "b", "main": "src/index.js" }"#,
    );
    p.file(
        "packages/b/src/index.js",
        "import { s } from \"sibling-dep\";\nexport function b() { return s(); }\n",
    );
    // An unclaimed single-file component could import: the manifest abstains.
    p.file(
        "packages/widgets/package.json",
        r#"{ "name": "widgets", "main": "src/index.js", "dependencies": { "widget-dep": "^1" } }"#,
    );
    p.file(
        "packages/widgets/src/index.js",
        "export function widget() { return 1; }\n",
    );
    p.file(
        "packages/widgets/src/App.vue",
        "<script>\nimport { w } from \"widget-dep\";\nexport default { w };\n</script>\n",
    );
    // Every owned file is a test: a fixture package, not judged.
    p.file(
        "packages/fixture/package.json",
        r#"{ "name": "fixture", "dependencies": { "fixture-dep": "^1" } }"#,
    );
    p.file(
        "packages/fixture/src/x.test.js",
        "import { f } from \"fixture-dep\";\nf();\n",
    );
    // Nothing reaches the package's files: dead, and the file findings say so.
    p.file(
        "packages/dead/package.json",
        r#"{ "name": "dead", "dependencies": { "dead-dep": "^1" } }"#,
    );
    p.file(
        "packages/dead/src/orphan.js",
        "export function orphan() { return 1; }\n",
    );
    // Unscoped ecosystem: `unused` judges, `test-only` has nowhere to move it.
    p.file(
        "gomod/go.mod",
        "module example.com/app\n\ngo 1.22\n\nrequire (\n\tgithub.com/x/direct v1.0.0\n\tgithub.com/x/testlib v1.0.0\n\tgithub.com/x/idle v1.0.0\n\tgithub.com/x/indirect v1.0.0 // indirect\n)\n",
    );
    p.file(
        "gomod/main.go",
        "package main\n\nimport (\n\t\"fmt\"\n\t\"github.com/x/direct\"\n\t\"github.com/x/phantom/sub\"\n)\n\nfunc main() { fmt.Println(direct.Run(), sub.Go()) }\n",
    );
    p.file(
        "gomod/main_test.go",
        "package main\n\nimport (\n\t\"testing\"\n\t\"github.com/x/testlib\"\n)\n\nfunc TestMain(t *testing.T) { testlib.Check(t) }\n",
    );
    // No declared spelling derives a package from a Python import: abstain.
    p.file(
        "py/pyproject.toml",
        "[project]\nname = \"pyapp\"\ndependencies = [\"requests\"]\n",
    );
    p.file("py/app.py", "import requests\n\nprint(requests)\n");
    // Cargo: a crate-rooted `use` accuses; `std`, a type-headed path and a
    // path continuing a `use` never do.
    p.file(
        "rs/Cargo.toml",
        "[package]\nname = \"rsapp\"\nversion = \"0.0.0\"\n\n[dependencies]\ndeclared-crate = \"1\"\n",
    );
    p.file(
        "rs/src/main.rs",
        "use declared_crate::x;\nuse phantom_crate::y;\nuse std::io;\n\nfn main() -> io::Result<()> {\n    let v: Vec<u8> = Vec::new();\n    std::process::exit(x(y(v)))\n}\n",
    );

    let session = kndo::open(p.root(), Config::default()).expect("open");
    let snap = session.analyze(RunMode::Full).expect("analyze");

    assert_eq!(
        dependency_findings(&snap),
        [
            ("test-only".into(), "package.json".into(), "test-dep".into()),
            (
                "undeclared".into(),
                "gomod/go.mod".into(),
                "github.com/x/phantom/sub".into()
            ),
            (
                "undeclared".into(),
                "package.json".into(),
                "phantom-dep".into()
            ),
            (
                "undeclared".into(),
                "rs/Cargo.toml".into(),
                "phantom_crate".into()
            ),
            (
                "unused".into(),
                "gomod/go.mod".into(),
                "github.com/x/idle".into()
            ),
            ("unused".into(), "package.json".into(), "idle-dep".into()),
        ],
        "{:#?}",
        snap.findings
    );

    let mut abstentions: Vec<(String, String, String)> = snap
        .abstained
        .iter()
        .filter(|a| matches!(a.scope, AbstentionScope::Manifests { .. }))
        .map(|a| {
            (
                a.category.as_str().to_string(),
                match &a.reason {
                    AbstentionReason::UnclaimedImporters { suffixes } => {
                        format!("unclaimed-importers:{}", suffixes.join(","))
                    }
                    other => format!("{other:?}"),
                },
                format!("{:?}", a.scope),
            )
        })
        .collect();
    abstentions.sort();
    let one = "Manifests { unjudged: 1 }".to_string();
    let expected: Vec<(String, String, String)> = ["test-only", "undeclared", "unused"]
        .iter()
        .flat_map(|category| {
            [
                "NothingReachesOwnedFiles",
                "OwnedFilesAreTests",
                "SpecifierIdentityUnderivable",
                "unclaimed-importers:vue",
            ]
            .map(|reason| (category.to_string(), reason.to_string(), one.clone()))
        })
        .collect();
    assert_eq!(abstentions, expected);
    assert!(
        [Category::UNUSED, Category::TEST_ONLY, Category::UNDECLARED]
            .iter()
            .all(|c| snap.judged.contains(c)),
        "a manifest-scoped abstention is not a whole-run one"
    );

    // Health divides by what was judged: the root's six production declarations,
    // the Go module's three direct requirements and the Cargo package's one, on
    // top of files and declarations — never a dev, peer, optional, transitive,
    // unjudged or undeclared one.
    let report = snap.report();
    let health = report.health.expect("health is measured");
    let graph_subjects: u32 = snap
        .graph
        .files
        .iter()
        .map(|f| 1 + f.evidence.declarations.len() as u32)
        .sum();
    assert_eq!(health.subjects - graph_subjects, 6 + 3 + 1, "{health:#?}");
    // Per package, the same rule: the files and declarations the package owns
    // (`package_of`), plus the dependencies its manifest had judged. A manifest
    // without an entry declares no package here, so its files fall to the
    // nearest enclosing one — the root — like any other unpackaged file.
    let owned_subjects = |manifest: &str| -> u32 {
        let ix = snap
            .graph
            .packages
            .iter()
            .position(|p| p.manifest.as_str() == manifest)
            .map(|i| i as u32);
        snap.graph
            .files
            .iter()
            .filter(|f| snap.graph.package_of(f.path.as_str()) == ix)
            .map(|f| 1 + f.evidence.declarations.len() as u32)
            .sum()
    };
    let row = |manifest: &str| -> u32 {
        health
            .by_package
            .iter()
            .find(|p| p.manifest.as_str() == manifest)
            .map(|p| p.subjects)
            .unwrap_or_else(|| panic!("no row for {manifest}: {:?}", health.by_package))
    };
    assert_eq!(row("package.json"), owned_subjects("package.json") + 6);
    assert_eq!(row("gomod/go.mod"), owned_subjects("gomod/go.mod") + 3);
    assert_eq!(
        row("packages/widgets/package.json"),
        owned_subjects("packages/widgets/package.json")
    );
    let total: u32 = health.by_package.iter().map(|p| p.subjects).sum();
    assert_eq!(total, health.subjects, "the partition reconciles");
}
