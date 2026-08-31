//! Extraction facts: reach mapping, nominal members, dispatch roots, imports,
//! and the file-role roots the standard layout dictates.

use kndo_adapter_java::JavaAdapter;
use kndo_contract::evidence::{ImportShape, ImportTarget, Reach, RootKind, RootTarget};
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
    assert_eq!(declaration_named(&ev, "b").reach, Reach::Exported);
    assert_eq!(declaration_named(&ev, "c").reach, Reach::Private);
    assert_eq!(declaration_named(&ev, "d").reach, Reach::Private);
}

#[test]
fn interface_members_are_implicitly_public_and_members_are_owned() {
    let ev = ev(
        "src/main/java/com/foo/Api.java",
        "package com.foo;\ninterface Api {\n  int limit = 3;\n  void call();\n}\n",
    );
    assert_eq!(declaration_named(&ev, "Api").reach, Reach::Private);
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
    for name in ["main", "toString", "readObject"] {
        let ix = ev.declarations.iter().position(|d| d.name == name).unwrap();
        assert!(
            rooted.contains(&ix),
            "{name} must be rooted: {:#?}",
            ev.roots
        );
    }
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
fn generated_files_declare_nothing_but_their_imports_stay() {
    let ev = ev(
        "src/main/java/com/foo/Proto.java",
        "// Code generated by protoc. DO NOT EDIT\n\
         package com.foo;\n\
         import com.util.Helper;\n\
         public class Proto {}\n",
    );
    assert!(ev.declarations.is_empty(), "{:#?}", ev.declarations);
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
