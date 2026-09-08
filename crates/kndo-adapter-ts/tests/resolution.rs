//! Relative resolution against a known-files set: candidate order, the `.js`-names-
//! the-compiled-file swap, index files, and the escapes that must stay `Unresolved`.

use kndo_adapter_ts::TypeScriptAdapter;
use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::extension::Extension;
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeSet;

fn project(files: &[&str]) -> BTreeSet<ProjectPath> {
    files.iter().map(|f| ProjectPath::new(*f)).collect()
}

fn resolve(files: &[&str], from: &str, spec: &str) -> Resolution {
    kndo_testkit::resolve_in(&TypeScriptAdapter::new(), files, from, spec)
}

fn file(path: &str) -> Resolution {
    Resolution::File(ProjectPath::new(path))
}

#[test]
fn extension_candidates_in_ts_first_order() {
    let files = ["src/a.ts", "src/util.ts", "src/util.js"];
    assert_eq!(resolve(&files, "src/a.ts", "./util"), file("src/util.ts"));
    assert_eq!(
        resolve(&["src/a.ts", "src/util.js"], "src/a.ts", "./util"),
        file("src/util.js")
    );
}

#[test]
fn exact_path_wins() {
    let files = ["src/a.ts", "src/util.mjs"];
    assert_eq!(
        resolve(&files, "src/a.ts", "./util.mjs"),
        file("src/util.mjs")
    );
}

#[test]
fn js_specifier_finds_the_ts_source() {
    let files = ["src/a.ts", "src/util.ts"];
    assert_eq!(
        resolve(&files, "src/a.ts", "./util.js"),
        file("src/util.ts")
    );
}

#[test]
fn directory_resolves_to_index() {
    let files = ["src/a.ts", "src/lib/index.ts"];
    assert_eq!(
        resolve(&files, "src/a.ts", "./lib"),
        file("src/lib/index.ts")
    );
}

#[test]
fn parent_traversal() {
    let files = ["src/deep/a.ts", "src/shared.ts"];
    assert_eq!(
        resolve(&files, "src/deep/a.ts", "../shared"),
        file("src/shared.ts")
    );
}

#[test]
fn root_level_sibling() {
    let files = ["main.ts", "helper.ts"];
    assert_eq!(resolve(&files, "main.ts", "./helper"), file("helper.ts"));
}

#[test]
fn workspace_bare_specifiers_link_to_their_package() {
    use kndo_contract::adapter::PackageEntry;
    use smol_str::SmolStr;
    use std::collections::BTreeMap;

    let known = project(&[
        "packages/core/src/index.ts",
        "packages/core/src/util.ts",
        "packages/app/main.ts",
    ]);
    let mut packages: BTreeMap<SmolStr, PackageEntry> = BTreeMap::new();
    packages.insert(
        SmolStr::new("@demo/core"),
        PackageEntry {
            name: SmolStr::new("@demo/core"),
            entry: Some(ProjectPath::new("packages/core/src/index.ts")),
            dir: SmolStr::new("packages/core"),
            aliases: Vec::new(),
        },
    );
    let cx = ResolveContext::with_packages(&known, &packages);
    let adapter = TypeScriptAdapter::new();
    let from = ProjectPath::new("packages/app/main.ts");

    assert_eq!(
        adapter.resolve(&from, "@demo/core", &cx),
        file("packages/core/src/index.ts")
    );
    // A SUBPATH inside a declared package is what `package.json`'s `exports`
    // map answers — `./src/*` may map anywhere, may be absent, and may differ
    // by condition. Splitting the name off and mirroring the subpath onto the
    // package directory agreed with that map only when the package had none.
    // Until the manifest half reads `exports`, a subpath is external: unresolved
    // is keep-alive, and keep-alive is the safe direction to be wrong in.
    assert_eq!(
        adapter.resolve(&from, "@demo/core/src/util", &cx),
        Resolution::Unresolved
    );
    assert_eq!(adapter.resolve(&from, "react", &cx), Resolution::Unresolved);
}

#[test]
fn unresolvable_stays_unresolved() {
    let files = ["src/a.ts"];
    assert_eq!(
        resolve(&files, "src/a.ts", "./missing"),
        Resolution::Unresolved
    );
    // Escaping the project root can never name a project file.
    assert_eq!(
        resolve(&files, "src/a.ts", "../../../etc/passwd"),
        Resolution::Unresolved
    );
    // Package specifiers are not this resolver's territory.
    assert_eq!(resolve(&files, "src/a.ts", "react"), Resolution::Unresolved);
}
