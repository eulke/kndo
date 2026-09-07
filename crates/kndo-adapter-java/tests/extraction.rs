//! Extraction facts: reach mapping, nominal members, dispatch roots, imports,
//! and the file-role roots the standard layout dictates.

use kndo_adapter_java::JavaAdapter;
use kndo_contract::evidence::{
    ImportShape, ImportTarget, MarkerTarget, Reach, RootKind, RootTarget,
};
use kndo_testkit::{declaration_named, extract_evidence, import_named};

fn ev(path: &str, source: &str) -> kndo_contract::evidence::FileEvidence {
    extract_evidence(&JavaAdapter::new(), path, source)
}

#[test]
fn visibility_folds_to_binary_reach() {
    let ev = ev(
        "src/main/java/com/foo/Widget.java",
        "package com.foo;\n\
         public class Widget {\n\
           public void a() {}\n\
           protected void b() {}\n\
           void c() {}\n\
           private void d() {}\n\
         }\n",
    );
    assert_eq!(declaration_named(&ev, "Widget").reach, Reach::Exported);
    assert_eq!(declaration_named(&ev, "a").reach, Reach::Exported);
    assert_eq!(
        declaration_named(&ev, "b").reach,
        Reach::Heirs {
            and_namespace: true
        },
        "protected grants the subtypes and the package"
    );
    assert_eq!(
        declaration_named(&ev, "c").reach,
        Reach::Namespace { up: 0 }
    );
    assert_eq!(declaration_named(&ev, "d").reach, Reach::Owner);
}

#[test]
fn interface_members_are_implicitly_public_and_members_are_owned() {
    let ev = ev(
        "src/main/java/com/foo/Api.java",
        "package com.foo;\ninterface Api {\n  int limit = 3;\n  void call();\n}\n",
    );
    assert_eq!(
        declaration_named(&ev, "Api").reach,
        Reach::Namespace { up: 0 }
    );
    assert_eq!(declaration_named(&ev, "call").reach, Reach::Exported);
    assert_eq!(declaration_named(&ev, "limit").reach, Reach::Exported);
    let api = ev
        .declarations
        .iter()
        .position(|d| d.name == "Api")
        .unwrap();
    assert!(
        ev.declarations
            .iter()
            .filter(|d| d.name == "call" || d.name == "limit")
            .all(|d| d.owner.is_some_and(|o| o.index() == api)),
        "members carry their owner: {:#?}",
        ev.declarations
    );
}

#[test]
fn dispatch_and_entry_points_are_rooted_not_guessed() {
    let ev = ev(
        "src/main/java/com/foo/App.java",
        "package com.foo;\n\
         public class App {\n\
           public static void main(String[] args) {}\n\
           @Override public String toString() { return \"x\"; }\n\
           private void readObject(java.io.ObjectInputStream in) {}\n\
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
    // What the grammar alone proves: the JVM entry, and a hook the runtime
    // calls reflectively.
    for name in ["main", "readObject"] {
        let ix = ev.declarations.iter().position(|d| d.name == name).unwrap();
        assert!(
            rooted.contains(&ix),
            "{name} must be rooted: {:#?}",
            ev.roots
        );
    }
    // `@Override` is a marker; the root is the spec's rule, derived by the
    // engine — extraction states the annotation and stops there.
    assert_eq!(markers_on(&ev, "toString"), [("Override", vec![])]);
    let ix = ev
        .declarations
        .iter()
        .position(|d| d.name == "toString")
        .unwrap();
    assert!(!rooted.contains(&ix));
}

/// `(path, args)` of every marker on the declaration `name`, in source order.
fn markers_on<'e>(
    ev: &'e kndo_contract::evidence::FileEvidence,
    name: &str,
) -> Vec<(&'e str, Vec<&'e str>)> {
    let ix = ev
        .declarations
        .iter()
        .position(|d| d.name == name)
        .unwrap_or_else(|| panic!("declaration {name} missing: {:#?}", ev.declarations));
    ev.markers
        .iter()
        .filter(|m| matches!(m.on, kndo_contract::evidence::MarkerTarget::Declaration(id) if id.index() == ix))
        .map(|m| (m.path.as_str(), m.args.iter().map(|a| a.as_str()).collect()))
        .collect()
}

#[test]
fn annotations_are_markers_as_written() {
    let ev = ev(
        "src/main/java/com/foo/Ann.java",
        "package com.foo;\n\
         @Deprecated\n\
         public class Ann {\n\
           @SuppressWarnings(\"unused\") private int one, two;\n\
           @SuppressWarnings({\"unused\", \"rawtypes\"})\n\
           @org.junit.Test(timeout = 5)\n\
           void probe() {}\n\
         }\n",
    );
    assert_eq!(markers_on(&ev, "Ann"), [("Deprecated", vec![])]);
    // A declaration's annotations reach every name it declares.
    for field in ["one", "two"] {
        assert_eq!(
            markers_on(&ev, field),
            [("SuppressWarnings", vec!["\"unused\""])]
        );
    }
    // A brace initializer is ONE argument; arguments come as written, with
    // whitespace runs collapsed.
    assert_eq!(
        markers_on(&ev, "probe"),
        [
            ("SuppressWarnings", vec!["{\"unused\", \"rawtypes\"}"]),
            ("org.junit.Test", vec!["timeout = 5"]),
        ]
    );
}

#[test]
fn constructors_and_serial_version_uid_are_never_declared() {
    let ev = ev(
        "src/main/java/com/foo/Widget.java",
        "package com.foo;\n\
         public class Widget implements java.io.Serializable {\n\
           private static final long serialVersionUID = 1L;\n\
           public Widget(Helper h) { h.init(); }\n\
         }\n",
    );
    assert!(
        ev.declarations.iter().all(|d| d.name == "Widget"),
        "only the type: {:#?}",
        ev.declarations
    );
    // The constructor body is still walked: its references stay evidence.
    assert!(ev.references.iter().any(|r| r.name == "init"));
    assert!(ev.references.iter().any(|r| r.name == "Helper"));
}

#[test]
fn imports_take_their_shapes_and_the_platform_produces_none() {
    let ev = ev(
        "src/main/java/com/foo/Main.java",
        "package com.foo;\n\
         import java.util.List;\n\
         import com.util.Helper;\n\
         import com.util.*;\n\
         import static com.util.Constants.LIMIT;\n\
         import static com.util.Constants.*;\n\
         public class Main {}\n",
    );
    assert_eq!(ev.imports.len(), 4, "java.util.List is not evidence");
    let single = import_named(&ev, "com.util.Helper");
    assert!(matches!(&single.shape, ImportShape::Bindings(b) if b[0].local == "Helper"));
    let on_demand = import_named(&ev, "com.util");
    assert!(matches!(on_demand.shape, ImportShape::Glob));
    let static_single = ev
        .imports
        .iter()
        .find(|i| matches!(&i.shape, ImportShape::Bindings(b) if b[0].local == "LIMIT"))
        .expect("static single import binds the member");
    assert!(matches!(&static_single.target, ImportTarget::Package(s) if s == "com.util.Constants"));
    assert!(
        ev.imports.iter().any(
            |i| matches!(&i.target, ImportTarget::Package(s) if s == "com.util.Constants")
                && matches!(i.shape, ImportShape::Glob)
        ),
        "static wildcard is a glob over the class file"
    );
}

#[test]
fn layout_roles_root_the_file() {
    let test = ev(
        "src/test/java/com/foo/WidgetTest.java",
        "class WidgetTest {}\n",
    );
    assert!(
        test.roots
            .iter()
            .any(|r| r.kind == RootKind::Test && matches!(r.target, RootTarget::WholeFile))
    );
    let tooling = ev(
        "src/main/java/com/foo/package-info.java",
        "package com.foo;\n",
    );
    assert!(tooling.roots.iter().any(|r| r.kind == RootKind::Tooling));
    let prod = ev("src/main/java/com/foo/Widget.java", "class Widget {}\n");
    assert!(prod.roots.iter().any(|r| r.kind == RootKind::Production));
}

#[test]
fn a_generated_file_reports_its_banner_and_says_everything_else_in_full() {
    let ev = ev(
        "src/main/java/com/foo/Proto.java",
        "// Code generated by protoc. DO NOT EDIT\n\
         package com.foo;\n\
         import com.util.Helper;\n\
         public class Proto {}\n",
    );
    assert_eq!(ev.markers.len(), 1);
    assert_eq!(ev.markers[0].on, MarkerTarget::File);
    assert_eq!(ev.markers[0].path, "generated");
    assert_eq!(ev.declarations.len(), 1, "{:#?}", ev.declarations);
    assert_eq!(ev.imports.len(), 1);
}

#[test]
fn enum_constants_are_not_declared_but_methods_carry_metrics() {
    let ev = ev(
        "src/main/java/com/foo/Color.java",
        "package com.foo;\n\
         public enum Color {\n\
           RED, GREEN;\n\
           public boolean warm(int x) { if (x > 0 && x < 10) { return true; } return false; }\n\
         }\n",
    );
    // `values()`/`valueOf` reach every constant namelessly — never declared,
    // never accusable.
    assert!(ev.declarations.iter().all(|d| d.name != "RED"));
    let warm = ev
        .declarations
        .iter()
        .position(|d| d.name == "warm")
        .unwrap();
    let (_, m) = ev
        .metrics
        .iter()
        .find(|(id, _)| id.index() == warm)
        .expect("methods carry metrics");
    assert_eq!(m.cyclomatic, 3, "if + && past the base");
}

#[test]
fn a_javadoc_pragma_strips_to_its_text() {
    let ev = ev(
        "src/main/java/com/foo/Widget.java",
        "/** kndo:allow-file duplicate -- vendored */\npublic class Widget {}\n",
    );
    let c = &ev.comments[0];
    // The `/**` opener strips whole: the text starts at the pragma, which is
    // what lets `kndo:allow` START its comment inside a doc block.
    assert_eq!(c.text.start, 3, "doc star belongs to the marker: {c:#?}");
}

#[test]
fn qualified_type_segments_are_spelling_not_uses() {
    let ev = ev(
        "src/main/java/com/foo/Uses.java",
        "package com.foo;\n\
         public class Uses {\n\
           public java.util.function.Function<String, Integer> f() { return null; }\n\
           java.util.Map.Entry<String, Integer> e;\n\
         }\n",
    );
    let named = |name: &str| ev.references.iter().filter(|r| r.name == name).count();
    assert_eq!(
        named("java") + named("util") + named("function"),
        0,
        "lowercase qualifier segments are package spelling, never uses: {:#?}",
        ev.references
    );
    assert!(named("Function") >= 1, "the named type stays a reference");
    assert!(
        named("Map") >= 1,
        "an uppercase outer-class qualifier stays"
    );
    assert!(named("Entry") >= 1);
}

#[test]
fn generic_and_qualified_supertypes_classify_as_extend() {
    use kndo_contract::evidence::RefKind;
    let ev = ev(
        "src/main/java/com/foo/Sub.java",
        "package com.foo;\n\
         public class Sub extends Base<String> implements I<Long>, J {\n}\n\
         class Base<T> {}\n\
         interface I<T> {}\n\
         interface J {}\n",
    );
    let kind_of = |name: &str| {
        ev.references
            .iter()
            .find(|r| r.name == name)
            .map(|r| r.kind)
    };
    assert_eq!(kind_of("Base"), Some(RefKind::Extend));
    assert_eq!(kind_of("I"), Some(RefKind::Extend));
    assert_eq!(kind_of("J"), Some(RefKind::Extend));
    // A supertype's type ARGUMENTS are ordinary type uses, not Extend.
    assert_eq!(kind_of("String"), Some(RefKind::TypeUse));
    assert_eq!(kind_of("Long"), Some(RefKind::TypeUse));
}

#[test]
fn supertypes_are_relations_by_bare_name() {
    let ev = ev(
        "src/main/java/com/foo/Impl.java",
        "package com.foo;\n\
         class Impl<E> extends com.foo.Abstract<E> implements Runnable, Iface<E> {\n\
           void run() {}\n\
         }\n\
         interface Iface<E> extends Base {}\n",
    );
    let relations: Vec<(String, String, String)> = ev
        .relations
        .iter()
        .map(|r| {
            (
                ev.declarations[r.from.index()].name.to_string(),
                format!("{:?}", r.kind),
                r.to.to_string(),
            )
        })
        .collect();
    assert_eq!(
        relations,
        [
            // Qualification and generic arguments are stripped: the promise is
            // the type's own name, spelled as a reference to it would be.
            (
                "Impl".to_string(),
                "Extends".to_string(),
                "Abstract".to_string()
            ),
            (
                "Impl".to_string(),
                "Implements".to_string(),
                "Runnable".to_string()
            ),
            (
                "Impl".to_string(),
                "Implements".to_string(),
                "Iface".to_string()
            ),
            (
                "Iface".to_string(),
                "Implements".to_string(),
                "Base".to_string()
            ),
        ]
    );
}
