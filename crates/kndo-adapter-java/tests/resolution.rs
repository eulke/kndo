//! Resolution by the namespace a Java import names.

use kndo_adapter_java::JavaAdapter;
use kndo_contract::adapter::Resolution;
use kndo_contract::vocab::ProjectPath;
use kndo_testkit::resolve_in_namespaces;

/// One project, asked five ways. Each case is a tree of (path, package clause),
/// a specifier, and the files the clause makes answer — the whole of what this
/// resolver decides, so a case is a row and not a function.
#[test]
fn an_import_answers_with_the_package_that_declared_the_clause() {
    let cases: &[kndo_testkit::NamespaceCase] = &[
        (
            // A type import names a type IN a package; the package's files are              the files that wrote its clause, and the import's binding picks              the type among them. Resolution's job ends at the package.
            "a type import",
            &[
                ("src/main/java/com/foo/Main.java", "com.foo"),
                ("src/main/java/com/util/Helper.java", "com.util"),
                ("src/main/java/com/util/Other.java", "com.util"),
            ],
            "com.util.Helper",
            &[
                "src/main/java/com/util/Helper.java",
                "src/main/java/com/util/Other.java",
            ],
        ),
        (
            // javac's layout convention is how a compiler FINDS sources; the
            // clause is what a name MEANS. A file under `legacy/` declaring
            // `com.util` is in `com.util`, and path-suffix matching could not
            // see it at all.
            "a package no directory mirrors",
            &[
                ("legacy/Helper.java", "com.util"),
                ("src/main/java/com/foo/Main.java", "com.foo"),
            ],
            "com.util.Helper",
            &["legacy/Helper.java"],
        ),
        (
            // `com.util.Outer.Inner` — the longest DECLARED prefix is
            // `com.util`, so nothing has to know how deep the nesting goes.
            "a nested type",
            &[("src/main/java/com/util/Outer.java", "com.util")],
            "com.util.Outer.Inner",
            &["src/main/java/com/util/Outer.java"],
        ),
        (
            // A type-import-on-demand covers ONE package: a subpackage writes a
            // different clause and is a different namespace.
            "a wildcard, and not its subpackages",
            &[
                ("src/main/java/com/util/A.java", "com.util"),
                ("src/main/java/com/util/B.java", "com.util"),
                ("src/main/java/com/util/sub/C.java", "com.util.sub"),
            ],
            "com.util",
            &[
                "src/main/java/com/util/A.java",
                "src/main/java/com/util/B.java",
            ],
        ),
        (
            // retrofit's shape: two source trees both declaring `retrofit2`. On
            // the JVM that IS one package — the classpath does not know modules
            // — so both answer. Preferring the importer's own tree was a guess
            // about a layout, and a layout is not what the language says.
            "one package split across modules",
            &[
                ("android/src/test/java/retrofit2/Retrofit.java", "retrofit2"),
                ("retrofit/src/main/java/retrofit2/Call.java", "retrofit2"),
            ],
            "retrofit2.Retrofit",
            &[
                "android/src/test/java/retrofit2/Retrofit.java",
                "retrofit/src/main/java/retrofit2/Call.java",
            ],
        ),
    ];
    for (what, tree, specifier, expected) in cases {
        let got = resolve_in_namespaces(&JavaAdapter::new(), tree, tree[0].0, specifier);
        let want = Resolution::Files(expected.iter().map(|p| ProjectPath::new(*p)).collect());
        assert_eq!(got, want, "{what}: `{specifier}`");
    }
}

#[test]
fn third_party_packages_stay_unresolved() {
    // No file in the project declares `org.junit`, so nothing answers. That is
    // the deliberate posture: no reliable package-to-coordinate mapping exists
    // without resolving a classpath, and guessing would flood false positives.
    let r = resolve_in_namespaces(
        &JavaAdapter::new(),
        &[("src/main/java/com/foo/Main.java", "com.foo")],
        "src/main/java/com/foo/Main.java",
        "org.junit.Assert",
    );
    assert_eq!(r, Resolution::Unresolved);
}

#[test]
fn declared_dependencies_carry_both_spellings() {
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
