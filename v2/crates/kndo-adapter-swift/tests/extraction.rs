use kndo_adapter_swift::SwiftAdapter;
use kndo_contract::adapter::SourceFile;
use kndo_contract::evidence::{
    Declaration, EvidenceSink, FileEvidence, ImportShape, ImportTarget, Reach, RefKind, RootKind,
    RootTarget, SymbolKind,
};
use kndo_contract::extension::Extension;
use kndo_contract::vocab::{Confidence, ProjectPath};

fn ev(path: &str, src: &str) -> FileEvidence {
    let a = SwiftAdapter::new();
    let mut sink = EvidenceSink::new(src.len() as u32, a.spec().emits().clone());
    a.extract(
        &SourceFile {
            path: &ProjectPath::new(path),
            content: src.as_bytes(),
        },
        &mut sink,
    );
    sink.finish()
}

fn decl<'e>(e: &'e FileEvidence, name: &str) -> &'e Declaration {
    e.declarations
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("declaration `{name}` missing"))
}

fn scoped(tok: &str) -> Reach {
    Reach::Scoped { scope: tok.into() }
}

#[test]
fn the_default_rung_is_the_module() {
    let e = ev(
        "Sources/App/Widget.swift",
        "open class Widget {\n\
             private func a() {}\n\
             fileprivate func b() {}\n\
             internal func c() {}\n\
             public func d() {}\n\
             open func e() {}\n\
             func plain() {}\n\
         }\n\
         struct Bare {}\n",
    );
    assert_eq!(decl(&e, "Widget").reach, Reach::Exported);
    assert_eq!(decl(&e, "a").reach, Reach::Private);
    assert_eq!(
        decl(&e, "b").reach,
        Reach::Private,
        "fileprivate is a file fact"
    );
    assert_eq!(decl(&e, "c").reach, scoped("module"));
    assert_eq!(decl(&e, "d").reach, Reach::Exported);
    assert_eq!(decl(&e, "e").reach, Reach::Exported);
    assert_eq!(
        decl(&e, "plain").reach,
        scoped("module"),
        "no modifier IS internal"
    );
    assert_eq!(decl(&e, "Bare").reach, scoped("module"));
}

#[test]
fn members_attribute_and_same_file_extensions_attach() {
    let e = ev(
        "Sources/App/A.swift",
        "struct Point {\n    var x = 0\n    let tag = \"p\"\n}\n\
         extension Point {\n    func flip() {}\n}\n\
         extension Elsewhere {\n    func lone() {}\n}\n",
    );
    let point = decl(&e, "Point");
    assert_eq!(point.kind, SymbolKind::Type);
    let x = decl(&e, "x");
    assert_eq!(x.kind, SymbolKind::Variable);
    assert_eq!(decl(&e, "tag").kind, SymbolKind::Constant);
    let point_ix = e
        .declarations_with_ids()
        .find(|(_, d)| d.name == "Point")
        .map(|(id, _)| id);
    assert_eq!(x.owner, point_ix);
    assert_eq!(
        decl(&e, "flip").owner,
        point_ix,
        "same-file extension attaches"
    );
    let lone = decl(&e, "lone");
    assert_eq!(
        lone.owner, None,
        "cross-file extension member stays ownerless"
    );
    assert_eq!(
        lone.kind,
        SymbolKind::Method,
        "…but still pools as a member"
    );
}

#[test]
fn initializers_deinit_and_enum_cases_are_never_declared() {
    let e = ev(
        "Sources/App/E.swift",
        "enum Mode { case fast, slow }\n\
         class Box {\n    init() { helper() }\n    deinit { helper() }\n}\n\
         func helper() {}\n",
    );
    assert!(
        e.declarations
            .iter()
            .all(|d| d.name != "fast" && d.name != "slow")
    );
    assert!(e.declarations.iter().all(|d| !d.name.contains("init")));
    // …their bodies still contribute references.
    assert!(e.references.iter().filter(|r| r.name == "helper").count() >= 2);
    // Case names never leak into the reference pool from their declaration.
    assert!(e.references.iter().all(|r| r.name != "fast"));
}

#[test]
fn dispatch_the_source_never_names_roots_probable_and_possible() {
    let e = ev(
        "Sources/App/D.swift",
        "class Impl: Base {\n\
             override func refresh() {}\n\
             func maybeWitness() {}\n\
             private func neverWitness() {}\n\
         }\n\
         class Plain {\n    func ordinary() {}\n}\n",
    );
    let rooted: Vec<(usize, Confidence)> = e
        .roots
        .iter()
        .filter_map(|r| match &r.target {
            RootTarget::Declaration(id) => Some((id.index(), r.confidence)),
            _ => None,
        })
        .collect();
    let ix = |name: &str| {
        e.declarations_with_ids()
            .find(|(_, d)| d.name == name)
            .map(|(id, _)| id.index())
            .unwrap()
    };
    assert!(
        rooted.contains(&(ix("refresh"), Confidence::Probable)),
        "override"
    );
    assert!(
        rooted.contains(&(ix("maybeWitness"), Confidence::Possible)),
        "a conforming type's non-private methods may witness external protocols"
    );
    assert!(rooted.iter().all(|(i, _)| *i != ix("neverWitness")));
    assert!(
        rooted.iter().all(|(i, _)| *i != ix("ordinary")),
        "no conformances, no witness keep"
    );
}

#[test]
fn roots_follow_the_layout_and_main_swift() {
    let test = ev("Tests/AppTests/XTests.swift", "final class XTests {}\n");
    assert!(test.roots.iter().any(|r| {
        matches!(r.target, RootTarget::WholeFile)
            && r.kind == RootKind::Test
            && r.confidence == Confidence::Certain
    }));

    let main = ev("Sources/App/main.swift", "run()\n");
    assert!(main.roots.iter().any(|r| {
        matches!(r.target, RootTarget::WholeFile)
            && r.kind == RootKind::Production
            && r.confidence == Confidence::Certain
    }));
    assert!(
        main.references.iter().any(|r| r.name == "run"),
        "top-level code walks"
    );

    let named = ev("Sources/App/LoadTest.swift", "func f() {}\n");
    let kinds: Vec<(RootKind, Confidence)> =
        named.roots.iter().map(|r| (r.kind, r.confidence)).collect();
    assert!(kinds.contains(&(RootKind::Test, Confidence::Probable)));
    assert!(
        kinds.contains(&(RootKind::Production, Confidence::Probable)),
        "a test-shaped NAME keeps the library root"
    );

    let manifest = ev("Package.swift", "let package = Package(name: \"x\")\n");
    assert!(manifest.declarations.is_empty());
    assert!(manifest.roots.iter().any(|r| r.kind == RootKind::Tooling));
}

#[test]
fn references_classify_by_seat() {
    let e = ev(
        "Sources/App/R.swift",
        "class Consumer: Codable {\n\
             func go(w: Widget) {\n\
                 help()\n\
                 w.refresh()\n\
                 let n = w.count\n\
                 let m: Mode = .fast\n\
             }\n\
         }\n",
    );
    let kind_of = |name: &str| {
        e.references
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("ref `{name}` missing"))
            .kind
    };
    assert_eq!(kind_of("help"), RefKind::Call);
    assert_eq!(kind_of("refresh"), RefKind::Call, "member call");
    assert_eq!(kind_of("count"), RefKind::Read, "member read");
    assert_eq!(kind_of("Widget"), RefKind::TypeUse);
    assert_eq!(kind_of("Codable"), RefKind::Extend);
    assert_eq!(
        kind_of("fast"),
        RefKind::Read,
        "dot-shorthand keeps enum cases alive"
    );
}

#[test]
fn imports_are_namespace_shaped_modules() {
    let e = ev(
        "Tests/AppTests/T.swift",
        "import Foundation\n@testable import App\nimport Foo.Bar\n",
    );
    let targets: Vec<(&str, bool)> = e
        .imports
        .iter()
        .map(|i| {
            let name = match &i.target {
                ImportTarget::Package(n) => n.as_str(),
                other => panic!("unexpected target {other:?}"),
            };
            (name, matches!(i.shape, ImportShape::Namespace { .. }))
        })
        .collect();
    assert_eq!(
        targets,
        vec![("Foundation", true), ("App", true), ("Foo", true)]
    );
}

#[test]
fn generated_files_declare_nothing_and_root_tooling() {
    let e = ev(
        "Sources/App/Gen.swift",
        "// Generated by Sourcery — DO NOT EDIT\nimport Foundation\nfunc ghost() {}\n",
    );
    assert!(e.declarations.is_empty());
    assert_eq!(e.imports.len(), 1, "imports still keep the rest alive");
}
