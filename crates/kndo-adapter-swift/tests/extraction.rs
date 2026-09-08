use kndo_adapter_swift::SwiftAdapter;
use kndo_contract::evidence::{
    Attachment, FileEvidence, ImportShape, ImportTarget, MarkerTarget, Reach, RefKind, RootTarget,
    SymbolKind,
};
use kndo_contract::vocab::Confidence;
use kndo_testkit::{declaration_named, extract_evidence};

fn ev(path: &str, src: &str) -> FileEvidence {
    extract_evidence(&SwiftAdapter::new(), path, src)
}

fn unit_wide() -> Reach {
    Reach::Unit { up: 0 }
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
    assert_eq!(declaration_named(&e, "Widget").reach, Reach::Exported);
    assert_eq!(declaration_named(&e, "a").reach, Reach::Owner);
    assert_eq!(
        declaration_named(&e, "b").reach,
        Reach::File,
        "fileprivate is a file fact"
    );
    assert_eq!(declaration_named(&e, "c").reach, unit_wide());
    assert_eq!(declaration_named(&e, "d").reach, Reach::Exported);
    assert_eq!(declaration_named(&e, "e").reach, Reach::Exported);
    assert_eq!(
        declaration_named(&e, "plain").reach,
        unit_wide(),
        "no modifier IS internal"
    );
    assert_eq!(declaration_named(&e, "Bare").reach, unit_wide());
}

#[test]
fn members_attribute_and_same_file_extensions_attach() {
    let e = ev(
        "Sources/App/A.swift",
        "struct Point {\n    var x = 0\n    let tag = \"p\"\n}\n\
         extension Point {\n    func flip() {}\n}\n\
         extension Elsewhere {\n    func lone() {}\n}\n",
    );
    let point = declaration_named(&e, "Point");
    assert_eq!(point.kind, SymbolKind::Type);
    let x = declaration_named(&e, "x");
    assert_eq!(x.kind, SymbolKind::Variable);
    assert_eq!(declaration_named(&e, "tag").kind, SymbolKind::Constant);
    let point_ix = e
        .declarations_with_ids()
        .find(|(_, d)| d.name == "Point")
        .map(|(id, _)| id);
    assert_eq!(x.owner, point_ix);
    assert_eq!(
        declaration_named(&e, "flip").owner,
        point_ix,
        "same-file extension attaches"
    );
    let lone = declaration_named(&e, "lone");
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
fn the_layout_names_the_module_and_states_the_test_membership() {
    // What SwiftPM's layout means for a file's ROLE is the spec's `file_roles`
    // (gated in `kndo-gates`). What extraction states from the layout is the
    // module: Swift spells nothing between a module and a name, so the target
    // IS the namespace — and a test target is its own, joined in a test build
    // alone.
    let test = ev("Tests/AppTests/XTests.swift", "final class XTests {}\n");
    assert_eq!(test.namespace, ["AppTests"]);
    assert_eq!(test.attachment, Attachment::TestOnly);
    assert!(test.roots.is_empty(), "{:?}", test.roots);

    let main = ev("Sources/App/main.swift", "run()\n");
    assert_eq!(main.namespace, ["App"]);
    assert_eq!(main.attachment, Attachment::Regular);
    assert!(main.roots.is_empty(), "{:?}", main.roots);
    assert!(
        main.references.iter().any(|r| r.name == "run"),
        "top-level code walks"
    );

    // A `path:` override outside the layout: the first segment is the module,
    // the content-free spelling of what the manifest said (Alamofire's
    // `Source/**`).
    let flat = ev("Source/Core/Request.swift", "struct Request {}\n");
    assert_eq!(flat.namespace, ["Source"]);

    // A file at the repository root belongs to no target: its own scope, and
    // the engine is told so by the absence.
    let loose = ev("Scratch.swift", "struct Scratch {}\n");
    assert!(loose.namespace.is_empty());

    let manifest = ev("Package.swift", "let package = Package(name: \"x\")\n");
    assert!(manifest.declarations.is_empty());
    assert!(manifest.roots.is_empty());
    assert!(manifest.namespace.is_empty());
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
fn a_generated_file_reports_its_banner_and_says_everything_else_in_full() {
    let e = ev(
        "Sources/App/Gen.swift",
        "// Generated by Sourcery — DO NOT EDIT\nimport Foundation\nfunc ghost() {}\n",
    );
    assert_eq!(e.markers.len(), 1);
    assert_eq!(e.markers[0].on, MarkerTarget::File);
    assert_eq!(e.markers[0].path, "generated");
    assert_eq!(e.declarations.len(), 1);
    assert_eq!(e.imports.len(), 1, "imports still keep the rest alive");
}

#[test]
fn a_test_target_belongs_to_the_package_in_test_builds_alone() {
    let e = ev("Tests/AppTests/WidgetTests.swift", "class WidgetTests {}\n");
    assert_eq!(e.attachment, Attachment::TestOnly);
    // A test-shaped NAME inside a library target is compiled into it.
    let e = ev("Sources/App/LoadTests.swift", "class LoadTests {}\n");
    assert_eq!(e.attachment, Attachment::Regular);
}

#[test]
fn attributes_and_the_override_modifier_are_markers() {
    let e = ev(
        "Sources/App/Views.swift",
        r#"
import SwiftUI

@main
struct App {
    static func main() {}
}

struct Row: View {
    @State private var count = 0
    @ViewBuilder func body() -> some View {}
}

class Base {
    func draw() {}
}

class Derived: Base {
    override func draw() {}
    @objc @IBAction func tapped(_ sender: Any) {}
}
"#,
    );
    let markers: Vec<(&str, &str)> = e
        .markers
        .iter()
        .filter_map(|m| match m.on {
            MarkerTarget::Declaration(id) => {
                Some((e.declarations[id.index()].name.as_str(), m.path.as_str()))
            }
            _ => None,
        })
        .collect();
    // Attributes on a type, a property and a function, and the one modifier a
    // rule reads. `@State`'s wrapper and `@ViewBuilder`'s builder ride the
    // same structural path: nothing here is a table of known names.
    assert!(markers.contains(&("App", "main")), "{markers:?}");
    assert!(markers.contains(&("count", "State")), "{markers:?}");
    assert!(markers.contains(&("body", "ViewBuilder")), "{markers:?}");
    assert!(markers.contains(&("draw", "override")), "{markers:?}");
    assert!(markers.contains(&("tapped", "objc")), "{markers:?}");
    assert!(markers.contains(&("tapped", "IBAction")), "{markers:?}");
    // The base class's own `draw` carries no modifier and no marker.
    assert_eq!(
        markers.iter().filter(|(n, _)| *n == "draw").count(),
        1,
        "{markers:?}"
    );
}

#[test]
fn the_inheritance_list_is_one_promise_per_name() {
    let e = ev(
        "Sources/App/Model.swift",
        r#"
import Foundation

protocol Drawable: Equatable {}

struct Point: Drawable, Codable {}

class Controller: NSObject, UITableViewDelegate {}

extension Point: CustomStringConvertible {
    var description: String { "" }
}
"#,
    );
    let relations: Vec<(&str, &str)> = e
        .relations
        .iter()
        .map(|r| {
            (
                e.declarations[r.from.index()].name.as_str(),
                r.to.name.as_str(),
            )
        })
        .collect();
    // Swift writes superclass and protocols in ONE list its grammar does not
    // separate, so every name is the same promise; nothing in the engine reads
    // the kind, and a rule that wants `NSObject` compares the name.
    assert_eq!(
        relations,
        [
            ("Drawable", "Equatable"),
            ("Point", "Drawable"),
            ("Point", "Codable"),
            ("Controller", "NSObject"),
            ("Controller", "UITableViewDelegate"),
            // A retroactive conformance is the EXTENDED type's promise, not
            // the extension block's — the block declares nothing at all.
            ("Point", "CustomStringConvertible"),
        ],
        "{relations:?}"
    );
}

#[test]
fn a_bound_value_is_a_reference_and_a_backtick_is_spelling() {
    let e = ev(
        "Sources/App/Bind.swift",
        r#"
struct Holder {
    func `default`() -> Int { 0 }
}

let alpha = beta
let held = Holder()
let picked = held.`default`
"#,
    );
    // S5: `property_declaration`'s `name` field IS the pattern, so naming the
    // parent kind a binder seat threw the VALUE away. `beta` is a use.
    let names: Vec<&str> = e.references.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"beta"), "{names:?}");
    // The quotes are spelling: the declaration and the reference agree on the
    // bare name, which is the only way the pool can join them.
    assert_eq!(declaration_named(&e, "default").name, "default");
    assert!(names.contains(&"default"), "{names:?}");
    assert!(!names.iter().any(|n| n.contains('`')), "{names:?}");
    // And it was read FROM `held` — the receiver the member pool needs.
    let on: Vec<(&str, Option<&str>)> = e
        .references
        .iter()
        .map(|r| (r.name.as_str(), r.on.as_deref()))
        .collect();
    assert!(on.contains(&("default", Some("held"))), "{on:?}");
    // A bare name is read from nothing, and says so.
    assert!(on.contains(&("beta", None)), "{on:?}");
}

#[test]
fn an_operator_is_a_name_and_a_requirement_inherits_its_protocol() {
    let e = ev(
        "Sources/App/Ops.swift",
        r#"
protocol Shape {
    func area() -> Int
    var sides: Int { get }
}

public extension Shape {
    func described() -> String { "" }
}

struct Point {
    static func == (l: Point, r: Point) -> Bool { true }
}

let same = Point() == Point()
"#,
    );
    // The grammar leaves an operator anonymous on BOTH sides, so declaration
    // and use join by the same spelling — and without the declaration the
    // whole `func` was dropped.
    assert_eq!(declaration_named(&e, "==").kind, SymbolKind::Method);
    let calls: Vec<&str> = e.references.iter().map(|r| r.name.as_str()).collect();
    assert!(calls.contains(&"=="), "{calls:?}");
    // A protocol requirement is exactly as visible as its protocol: there is
    // nothing narrower for it to be, and `Inherited` says so instead of
    // guessing the module default.
    assert_eq!(declaration_named(&e, "area").reach, Reach::Inherited);
    // Its PROPERTY requirement is its own node kind — one this adapter used to
    // walk past entirely.
    assert_eq!(declaration_named(&e, "sides").reach, Reach::Inherited);
    // A `public extension` hands its own modifier down.
    assert_eq!(declaration_named(&e, "described").reach, Reach::Exported);
}
