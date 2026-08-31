// kndo:allow-file duplicate -- parallel per-language conformance: the java
// twin asserts the same intents; each language keeps its own literals.
//! Extraction facts: Kotlin's public-by-default reach, promoted constructor
//! properties, dispatch roots, import shapes, and the never-declare postures.

use kndo_adapter_kotlin::KotlinAdapter;
use kndo_contract::evidence::{ImportShape, ImportTarget, Reach, RootKind, RootTarget};
use kndo_testkit::{declaration_named, extract_evidence, import_named};

fn ev(path: &str, source: &str) -> kndo_contract::evidence::FileEvidence {
    extract_evidence(&KotlinAdapter::new(), path, source)
}

#[test]
fn no_modifier_means_public_the_opposite_of_java() {
    let ev = ev(
        "src/main/kotlin/com/foo/Widget.kt",
        "package com.foo\n\
         class Widget {\n\
           fun visible() {}\n\
           internal fun moduleWide() {}\n\
           protected fun forSubclasses() {}\n\
           private fun hidden() {}\n\
         }\n\
         private class FileLocal\n",
    );
    assert_eq!(declaration_named(&ev, "Widget").reach, Reach::Exported);
    assert_eq!(declaration_named(&ev, "visible").reach, Reach::Exported);
    assert_eq!(declaration_named(&ev, "moduleWide").reach, Reach::Exported);
    assert_eq!(
        declaration_named(&ev, "forSubclasses").reach,
        Reach::Exported
    );
    assert_eq!(declaration_named(&ev, "hidden").reach, Reach::Private);
    assert_eq!(declaration_named(&ev, "FileLocal").reach, Reach::Private);
}

#[test]
fn members_and_promoted_constructor_properties_carry_their_owner() {
    let ev = ev(
        "src/main/kotlin/com/foo/Widget.kt",
        "package com.foo\n\
         class Widget(val cost: Int, var mode: String, plain: Boolean) {\n\
           val slug = \"x\"\n\
           fun run() {}\n\
         }\n",
    );
    let widget = ev
        .declarations
        .iter()
        .position(|d| d.name == "Widget")
        .unwrap();
    for name in ["cost", "mode", "slug", "run"] {
        assert!(
            declaration_named(&ev, name)
                .owner
                .is_some_and(|o| o.index() == widget),
            "{name} must belong to Widget: {:#?}",
            ev.declarations
        );
    }
    assert!(
        ev.declarations.iter().all(|d| d.name != "plain"),
        "a bare constructor argument is not a property"
    );
}

#[test]
fn override_operator_and_top_level_main_are_rooted() {
    let ev = ev(
        "src/main/kotlin/com/foo/App.kt",
        "package com.foo\n\
         fun main() {}\n\
         class Handler {\n\
           override fun toString(): String = \"h\"\n\
           operator fun invoke() {}\n\
         }\n",
    );
    let rooted: Vec<usize> = ev
        .roots
        .iter()
        .filter_map(|r| match r.target {
            RootTarget::Declaration(id) => Some(id.index()),
            _ => None,
        })
        .collect();
    for name in ["main", "toString", "invoke"] {
        let ix = ev.declarations.iter().position(|d| d.name == name).unwrap();
        assert!(
            rooted.contains(&ix),
            "{name} must be rooted: {:#?}",
            ev.roots
        );
    }
}

#[test]
fn companion_members_attribute_to_the_enclosing_class() {
    let ev = ev(
        "src/main/kotlin/com/foo/Widget.kt",
        "package com.foo\n\
         class Widget {\n\
           companion object {\n\
             fun create(): Widget = Widget()\n\
           }\n\
         }\n",
    );
    let widget = ev
        .declarations
        .iter()
        .position(|d| d.name == "Widget")
        .unwrap();
    assert!(
        declaration_named(&ev, "create")
            .owner
            .is_some_and(|o| o.index() == widget),
        "{:#?}",
        ev.declarations
    );
}

#[test]
fn enum_entries_and_secondary_constructors_are_never_declared() {
    let ev = ev(
        "src/main/kotlin/com/foo/Color.kt",
        "package com.foo\n\
         enum class Color { RED, GREEN }\n\
         class Widget {\n\
           constructor(x: Int) { helper(x) }\n\
         }\n",
    );
    assert!(
        ev.declarations
            .iter()
            .all(|d| d.name != "RED" && d.name != "<init>"),
        "{:#?}",
        ev.declarations
    );
    // The secondary constructor's body still contributes references.
    assert!(ev.references.iter().any(|r| r.name == "helper"));
}

#[test]
fn imports_take_their_shapes_and_the_platform_produces_none() {
    let ev = ev(
        "src/main/kotlin/com/foo/Main.kt",
        "package com.foo\n\
         import java.util.UUID\n\
         import kotlin.math.abs\n\
         import com.util.Helper\n\
         import com.util.Helper as H\n\
         import com.util.*\n\
         class Main\n",
    );
    assert_eq!(
        ev.imports.len(),
        3,
        "platform imports are not evidence: {:#?}",
        ev.imports
    );
    let single = import_named(&ev, "com.util.Helper");
    assert!(matches!(&single.shape, ImportShape::Bindings(b) if b[0].local == "Helper"));
    assert!(
        ev.imports.iter().any(|i| matches!(
            &i.shape,
            ImportShape::Bindings(b) if b[0].local == "H" && b[0].imported == "Helper"
        )),
        "the alias binds the local name: {:#?}",
        ev.imports
    );
    assert!(
        ev.imports.iter().any(
            |i| matches!(&i.target, ImportTarget::Package(s) if s == "com.util")
                && matches!(i.shape, ImportShape::Glob)
        ),
        "a wildcard is a glob over the package"
    );
}

#[test]
fn layout_roles_root_the_file() {
    let test = ev(
        "src/test/kotlin/com/foo/WidgetTest.kt",
        "class WidgetTest\n",
    );
    assert!(
        test.roots
            .iter()
            .any(|r| r.kind == RootKind::Test && matches!(r.target, RootTarget::WholeFile))
    );
    let prod = ev("src/main/kotlin/com/foo/Widget.kt", "class Widget\n");
    assert!(prod.roots.iter().any(|r| r.kind == RootKind::Production));
}

#[test]
fn top_level_functions_and_properties_are_declared_free() {
    let ev = ev(
        "src/main/kotlin/com/foo/Utils.kt",
        "package com.foo\n\
         val MAX = 255\n\
         private var counter = 0\n\
         fun helper() { counter += 1 }\n\
         fun String.slugify(): String = lowercase()\n",
    );
    assert_eq!(declaration_named(&ev, "MAX").reach, Reach::Exported);
    assert_eq!(declaration_named(&ev, "counter").reach, Reach::Private);
    assert!(declaration_named(&ev, "helper").owner.is_none());
    assert!(
        ev.declarations.iter().any(|d| d.name == "slugify"),
        "an extension function is a declaration: {:#?}",
        ev.declarations
    );
}

#[test]
fn a_kdoc_pragma_strips_to_its_text() {
    let ev = ev(
        "src/main/kotlin/com/foo/W.kt",
        "/** kndo:allow-file duplicate -- vendored */\nclass W\n",
    );
    let c = &ev.comments[0];
    assert_eq!(
        c.text.start, 3,
        "the KDoc star belongs to the marker: {c:#?}"
    );
}
