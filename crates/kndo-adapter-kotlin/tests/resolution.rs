//! Resolution by the namespace a Kotlin import names — the same toolkit
//! mechanism Java uses, because the two share one package namespace.

use kndo_adapter_kotlin::KotlinAdapter;
use kndo_contract::adapter::Resolution;
use kndo_contract::vocab::ProjectPath;
use kndo_testkit::resolve_in_namespaces;

/// What Kotlin adds to Java's question, as rows: a mixed module, a name that
/// is not a type, and a file whose directory contradicts its package.
#[test]
fn an_import_answers_with_the_package_whatever_the_file_is_called() {
    let cases: &[kndo_testkit::NamespaceCase] = &[
        (
            // One package namespace, two languages. The old resolver needed a
            // `.java` fallback for this; a clause needs no fallback at all.
            "a java class from kotlin",
            &[
                ("src/main/java/com/util/Legacy.java", "com.util"),
                ("src/main/kotlin/com/foo/Main.kt", "com.foo"),
            ],
            "com.util.Legacy",
            &["src/main/java/com/util/Legacy.java"],
        ),
        (
            // A top-level function may live in ANY file of its package, since
            // Kotlin ties no file name to its contents.
            "a top-level function",
            &[
                ("src/main/kotlin/com/util/Numbers.kt", "com.util"),
                ("src/main/kotlin/com/util/Strings.kt", "com.util"),
            ],
            "com.util.helper",
            &[
                "src/main/kotlin/com/util/Numbers.kt",
                "src/main/kotlin/com/util/Strings.kt",
            ],
        ),
        (
            // Kotlin RECOMMENDS the directory mirror and enforces it nowhere; a
            // source set laid out flat is ordinary Kotlin. The directory
            // fallback could only ever agree with a layout, so it disagreed
            // with the language exactly here.
            "a directory that contradicts the package",
            &[
                ("src/main/kotlin/com/foo/Main.kt", "com.foo"),
                ("src/test/kotlin/HelperTest.kt", "com.util"),
            ],
            "com.util.HelperTest",
            &["src/test/kotlin/HelperTest.kt"],
        ),
    ];
    for (what, tree, specifier, expected) in cases {
        let got = resolve_in_namespaces(&KotlinAdapter::new(), tree, tree[0].0, specifier);
        let want = Resolution::Files(expected.iter().map(|p| ProjectPath::new(*p)).collect());
        assert_eq!(got, want, "{what}: `{specifier}`");
    }
}

#[test]
fn third_party_packages_stay_unresolved() {
    let r = resolve_in_namespaces(
        &KotlinAdapter::new(),
        &[("src/main/kotlin/com/foo/Main.kt", "com.foo")],
        "src/main/kotlin/com/foo/Main.kt",
        "io.ktor.server.Application",
    );
    assert_eq!(r, Resolution::Unresolved);
}
