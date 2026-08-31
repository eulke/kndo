//! Resolution by path suffix, the nearest-module preference, and the
//! directory-plus-mirror unit.

use kndo_adapter_java::JavaAdapter;
use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::extension::Extension;
use kndo_contract::vocab::ProjectPath;
use kndo_testkit::resolve_in;
use std::collections::BTreeSet;

fn path(p: &str) -> ProjectPath {
    ProjectPath::new(p)
}

#[test]
fn a_type_import_resolves_to_its_file_by_the_directory_convention() {
    let r = resolve_in(
        &JavaAdapter::new(),
        &[
            "src/main/java/com/util/Helper.java",
            "src/main/java/com/foo/Main.java",
        ],
        "src/main/java/com/foo/Main.java",
        "com.util.Helper",
    );
    assert_eq!(
        r,
        Resolution::File(path("src/main/java/com/util/Helper.java"))
    );
}

#[test]
fn a_nested_type_import_falls_back_to_the_outer_file() {
    let r = resolve_in(
        &JavaAdapter::new(),
        &["src/main/java/com/util/Outer.java"],
        "src/main/java/com/foo/Main.java",
        "com.util.Outer.Inner",
    );
    assert_eq!(
        r,
        Resolution::File(path("src/main/java/com/util/Outer.java"))
    );
}

#[test]
fn a_wildcard_import_resolves_to_the_package_directory() {
    let r = resolve_in(
        &JavaAdapter::new(),
        &[
            "src/main/java/com/util/A.java",
            "src/main/java/com/util/B.java",
            "src/main/java/com/util/sub/C.java",
        ],
        "src/main/java/com/foo/Main.java",
        "com.util",
    );
    assert_eq!(
        r,
        Resolution::Files(vec![
            path("src/main/java/com/util/A.java"),
            path("src/main/java/com/util/B.java"),
        ]),
        "direct children only — subpackages are separate packages"
    );
}

#[test]
fn an_import_prefers_the_importers_own_module_over_a_sibling() {
    // retrofit's shape: two modules both hold `retrofit2/`; the intra-module
    // import must land home, or a phantom cross-module edge appears.
    let r = resolve_in(
        &JavaAdapter::new(),
        &[
            "android-test/src/test/java/retrofit2/Retrofit.java",
            "retrofit/src/main/java/retrofit2/Retrofit.java",
            "retrofit/src/main/java/retrofit2/HttpServiceMethod.java",
        ],
        "retrofit/src/main/java/retrofit2/HttpServiceMethod.java",
        "retrofit2.Retrofit",
    );
    assert_eq!(
        r,
        Resolution::File(path("retrofit/src/main/java/retrofit2/Retrofit.java"))
    );
    // A package living only in the sibling still resolves — nearest falls back.
    let r = resolve_in(
        &JavaAdapter::new(),
        &["android-test/src/test/java/retrofit2/Only.java"],
        "retrofit/src/main/java/retrofit2/HttpServiceMethod.java",
        "retrofit2.Only",
    );
    assert_eq!(
        r,
        Resolution::File(path("android-test/src/test/java/retrofit2/Only.java"))
    );
}

#[test]
fn third_party_packages_stay_unresolved() {
    let r = resolve_in(
        &JavaAdapter::new(),
        &["src/main/java/com/foo/Main.java"],
        "src/main/java/com/foo/Main.java",
        "org.junit.Assert",
    );
    assert_eq!(r, Resolution::Unresolved);
}

#[test]
fn the_unit_is_the_directory_plus_the_test_mirror() {
    let files = [
        "src/main/java/com/foo/Widget.java",
        "src/main/java/com/foo/Helper.java",
        "src/main/java/com/bar/Other.java",
        "src/test/java/com/foo/WidgetTest.java",
    ];
    let known: BTreeSet<ProjectPath> = files.iter().map(|p| ProjectPath::new(*p)).collect();
    let cx = ResolveContext::new(&known);
    let a = JavaAdapter::new();

    let main_mates = a.sees(&path("src/main/java/com/foo/Widget.java"), &cx);
    assert_eq!(
        main_mates,
        vec![path("src/main/java/com/foo/Helper.java")],
        "production sees its siblings, never the test tree"
    );

    let test_mates = a.sees(&path("src/test/java/com/foo/WidgetTest.java"), &cx);
    assert_eq!(
        test_mates,
        vec![
            path("src/main/java/com/foo/Helper.java"),
            path("src/main/java/com/foo/Widget.java"),
        ],
        "a test class shares its package with the mirrored main directory"
    );
}

#[test]
fn manifest_dependencies_carry_both_spellings() {
    let a = JavaAdapter::new();
    let pom = path("pom.xml");
    let deps = a.manifest_dependencies(&SourceFile {
        path: &pom,
        content: b"<project><dependencies>\n  <dependency>\n    <groupId>com.squareup.okhttp3</groupId>\n    <artifactId>okhttp</artifactId>\n  </dependency>\n</dependencies></project>",
    });
    assert!(deps.iter().any(|d| d == "com.squareup.okhttp3:okhttp"));
    assert!(deps.iter().any(|d| d == "okhttp"));

    let gradle = path("build.gradle");
    let deps = a.manifest_dependencies(&SourceFile {
        path: &gradle,
        content: b"dependencies {\n  implementation 'io.vertx:vertx-core:4.5.0'\n}\n",
    });
    assert!(deps.iter().any(|d| d == "io.vertx:vertx-core"));
    assert!(deps.iter().any(|d| d == "vertx-core"));
}
