//! Resolution: suffix with the `.java` and package-directory fallbacks, and
//! the directory-plus-mirrors unit.

use kndo_adapter_kotlin::KotlinAdapter;
use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::evidence::Reach;
use kndo_contract::extension::Extension;
use kndo_contract::vocab::ProjectPath;
use kndo_testkit::resolve_in;
use std::collections::BTreeSet;

fn path(p: &str) -> ProjectPath {
    ProjectPath::new(p)
}

#[test]
fn a_class_import_resolves_to_its_conventional_file() {
    let r = resolve_in(
        &KotlinAdapter::new(),
        &[
            "src/main/kotlin/com/util/Helper.kt",
            "src/main/kotlin/com/foo/Main.kt",
        ],
        "src/main/kotlin/com/foo/Main.kt",
        "com.util.Helper",
    );
    assert_eq!(
        r,
        Resolution::File(path("src/main/kotlin/com/util/Helper.kt"))
    );
}

#[test]
fn a_java_class_resolves_from_kotlin_in_a_mixed_project() {
    let r = resolve_in(
        &KotlinAdapter::new(),
        &["src/main/java/com/util/Legacy.java"],
        "src/main/kotlin/com/foo/Main.kt",
        "com.util.Legacy",
    );
    assert_eq!(
        r,
        Resolution::File(path("src/main/java/com/util/Legacy.java"))
    );
}

#[test]
fn a_top_level_function_import_falls_back_to_the_package_directory() {
    // `import com.util.helper` — a top-level function may live in ANY file of
    // the package; Kotlin does not tie file names to contents.
    let r = resolve_in(
        &KotlinAdapter::new(),
        &[
            "src/main/kotlin/com/util/Strings.kt",
            "src/main/kotlin/com/util/Numbers.kt",
        ],
        "src/main/kotlin/com/foo/Main.kt",
        "com.util.helper",
    );
    assert_eq!(
        r,
        Resolution::Files(vec![
            path("src/main/kotlin/com/util/Numbers.kt"),
            path("src/main/kotlin/com/util/Strings.kt"),
        ])
    );
}

#[test]
fn third_party_packages_stay_unresolved() {
    let r = resolve_in(
        &KotlinAdapter::new(),
        &["src/main/kotlin/com/foo/Main.kt"],
        "src/main/kotlin/com/foo/Main.kt",
        "io.ktor.server.Application",
    );
    assert_eq!(r, Resolution::Unresolved);
}

#[test]
fn the_unit_is_the_directory_plus_both_main_mirrors() {
    let files = [
        "src/main/kotlin/com/foo/Widget.kt",
        "src/main/kotlin/com/foo/Helper.kt",
        "src/main/java/com/foo/Legacy.java",
        "src/test/kotlin/com/foo/WidgetTest.kt",
    ];
    let known: BTreeSet<ProjectPath> = files.iter().map(|p| ProjectPath::new(*p)).collect();
    let cx = ResolveContext::new(&known);
    let a = KotlinAdapter::new();

    let main_mates = a.sees(&path("src/main/kotlin/com/foo/Widget.kt"), &cx);
    assert_eq!(
        main_mates,
        vec![
            path("src/main/java/com/foo/Legacy.java"),
            path("src/main/kotlin/com/foo/Helper.kt"),
        ],
        "joint compilation: production sees its package across BOTH main spellings"
    );

    let test_mates = a.sees(&path("src/test/kotlin/com/foo/WidgetTest.kt"), &cx);
    assert_eq!(
        test_mates,
        vec![
            path("src/main/java/com/foo/Legacy.java"),
            path("src/main/kotlin/com/foo/Helper.kt"),
            path("src/main/kotlin/com/foo/Widget.kt"),
        ],
        "a test sees the mirrored main package in BOTH source-set spellings"
    );
}

#[test]
fn the_module_region_is_the_source_set_tree() {
    let files = [
        "core/src/main/kotlin/com/a/A.kt",
        "core/src/main/kotlin/com/b/B.kt",
        "core/src/test/kotlin/com/a/ATest.kt",
        "core/src/main/java/com/a/Legacy.java",
        "dao/src/main/kotlin/com/c/C.kt",
        "core/build.gradle.kts",
    ];
    let known: BTreeSet<ProjectPath> = files.iter().map(|p| ProjectPath::new(*p)).collect();
    let cx = ResolveContext::new(&known);
    let a = KotlinAdapter::new();

    let region = a
        .seen_from(
            &path("core/src/main/kotlin/com/a/A.kt"),
            &Reach::Unit { up: 0 },
            &cx,
        )
        .expect("the unit is bounded from the source-set layout");
    assert_eq!(
        region,
        vec![
            path("core/src/main/java/com/a/Legacy.java"),
            path("core/src/main/kotlin/com/a/A.kt"),
            path("core/src/main/kotlin/com/b/B.kt"),
            path("core/src/test/kotlin/com/a/ATest.kt"),
        ],
        "every source under core's src trees, no dao, no manifests"
    );
    assert!(
        a.seen_from(
            &path("core/src/main/kotlin/com/a/A.kt"),
            &Reach::Scoped {
                scope: "package".into()
            },
            &cx
        )
        .is_none(),
        "an unknown token is unanswerable, never guessed"
    );
}
