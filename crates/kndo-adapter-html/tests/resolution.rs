//! Resolution against synthetic file sets: the path as written, the nearest
//! root for a root-relative one, never a guessed extension.

use kndo_adapter_html::HtmlAdapter;
use kndo_contract::adapter::Resolution;
use kndo_contract::vocab::ProjectPath;

fn resolve(files: &[&str], from: &str, specifier: &str) -> Resolution {
    kndo_testkit::resolve_in(&HtmlAdapter::new(), files, from, specifier)
}

fn file(path: &str) -> Resolution {
    Resolution::File(ProjectPath::new(path))
}

#[test]
fn a_document_relative_path_resolves_as_written() {
    // (known files, the document, the reference, what it names)
    let cases: &[(&[&str], &str, &str, &str)] = &[
        (
            &["app/main.js"],
            "app/index.html",
            "./main.js",
            "app/main.js",
        ),
        (
            &["app/main.js"],
            "app/index.html",
            "./main.js?v=3",
            "app/main.js",
        ),
        (
            &["shared/lib.js"],
            "app/pages/index.html",
            "../../shared/lib.js",
            "shared/lib.js",
        ),
    ];
    for (files, from, specifier, expected) in cases {
        assert_eq!(
            resolve(files, from, specifier),
            file(expected),
            "{specifier} from {from}"
        );
    }
}

#[test]
fn a_root_relative_path_resolves_at_the_nearest_ancestor_that_holds_it() {
    let files = &[
        "playground/a/index.html",
        "playground/a/src/main.ts",
        "playground/b/src/main.ts",
        "src/main.ts",
    ];
    assert_eq!(
        resolve(files, "playground/a/index.html", "/src/main.ts"),
        file("playground/a/src/main.ts")
    );
    assert_eq!(
        resolve(files, "playground/c/index.html", "/src/main.ts"),
        file("src/main.ts"),
        "no nearer root holds it: the project root does"
    );
    assert_eq!(
        resolve(files, "playground/a/index.html", "/src/other.ts"),
        Resolution::Unresolved
    );
}

#[test]
fn the_project_is_never_left() {
    assert_eq!(
        resolve(&["main.js"], "index.html", "../main.js"),
        Resolution::Unresolved
    );
}

#[test]
fn javascript_resolves_what_the_page_imports() {
    assert_eq!(
        resolve(&["src/main.ts"], "index.html", "./src/main"),
        file("src/main.ts"),
        "an inline import is a bundler's to resolve: the js-ts spellings apply"
    );
    assert_eq!(
        resolve(&["src/main.ts"], "index.html", "vue"),
        Resolution::Unresolved,
        "a bare name with no workspace package behind it is external"
    );
}
