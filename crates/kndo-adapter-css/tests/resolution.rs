//! Resolution over Sass's spellings, against synthetic file sets.

use kndo_adapter_css::CssAdapter;
use kndo_contract::adapter::Resolution;
use kndo_contract::vocab::ProjectPath;

fn resolve(files: &[&str], from: &str, specifier: &str) -> Resolution {
    kndo_testkit::resolve_in(&CssAdapter::new(), files, from, specifier)
}

fn file(path: &str) -> Resolution {
    Resolution::File(ProjectPath::new(path))
}

#[test]
fn relative_paths_resolve_through_the_sass_spellings() {
    assert_eq!(
        resolve(&["src/reset.css"], "src/main.css", "./reset.css"),
        file("src/reset.css")
    );
    assert_eq!(
        resolve(&["src/reset.css"], "src/main.scss", "./reset"),
        file("src/reset.css")
    );
    assert_eq!(
        resolve(&["src/_tokens.scss"], "src/main.scss", "./tokens"),
        file("src/_tokens.scss")
    );
    assert_eq!(
        resolve(&["src/lib/_index.scss"], "src/main.scss", "./lib"),
        file("src/lib/_index.scss")
    );
    assert_eq!(
        resolve(&["theme.scss"], "src/main.scss", "../theme"),
        file("theme.scss")
    );
}

#[test]
fn a_bare_specifier_is_a_sibling_when_one_exists_and_a_package_otherwise() {
    assert_eq!(
        resolve(&["src/_tokens.scss"], "src/main.scss", "tokens"),
        file("src/_tokens.scss")
    );
    assert_eq!(
        resolve(&["src/base.css"], "src/main.css", "base.css"),
        file("src/base.css")
    );
    assert_eq!(
        resolve(&["src/main.css"], "src/main.css", "tailwindcss"),
        Resolution::Unresolved
    );
    assert_eq!(
        resolve(&["src/normalize.css"], "src/main.css", "~normalize.css"),
        file("src/normalize.css")
    );
    assert_eq!(
        resolve(&["src/main.scss"], "src/main.scss", "sass:math"),
        Resolution::Unresolved
    );
}

#[test]
fn a_root_relative_path_finds_the_nearest_root() {
    assert_eq!(
        resolve(
            &["app/styles/base.css", "styles/base.css"],
            "app/pages/page.css",
            "/styles/base.css"
        ),
        file("app/styles/base.css")
    );
}
