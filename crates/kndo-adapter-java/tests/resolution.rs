//! Resolution by path suffix and the nearest-module preference.

use kndo_adapter_java::JavaAdapter;
use kndo_contract::adapter::Resolution;
use kndo_contract::vocab::ProjectPath;
use kndo_testkit::resolve_in;

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
fn manifest_dependencies_carry_both_spellings() {
    // A JVM coordinate is `group:artifact`, and code imports neither — so both
    // spellings are declared and a usage judgment abstains on both rather than
    // picking one to be wrong about.
    let deps = kndo_testkit::manifest_evidence(
        &JavaAdapter::new(),
        "pom.xml",
        "<project><dependencies>\n  <dependency>\n    <groupId>com.squareup.okhttp3</groupId>\n    <artifactId>okhttp</artifactId>\n  </dependency>\n</dependencies></project>",
        &[],
    )
    .dependencies;
    assert!(deps.iter().any(|d| d.name == "com.squareup.okhttp3:okhttp"));
    assert!(deps.iter().any(|d| d.name == "okhttp"));

    let deps = kndo_testkit::manifest_evidence(
        &JavaAdapter::new(),
        "build.gradle",
        "dependencies {\n  implementation 'io.vertx:vertx-core:4.5.0'\n}\n",
        &[],
    )
    .dependencies;
    assert!(deps.iter().any(|d| d.name == "io.vertx:vertx-core"));
    assert!(deps.iter().any(|d| d.name == "vertx-core"));
}
