//! Extraction against inline sources: reach, module edges, every `use` shape,
//! qualified-path imports, attribute roots, impl/trait members, reference
//! exclusions, and comment spans.

use kndo_adapter_rust::RustAdapter;
use kndo_contract::evidence::{
    FileEvidence, ImportShape, ImportTarget, Reach, RootKind, RootTarget, SymbolKind,
};

fn extract(path: &str, source: &str) -> FileEvidence {
    kndo_testkit::extract_evidence(&RustAdapter::new(), path, source)
}

fn decl<'e>(ev: &'e FileEvidence, name: &str) -> &'e kndo_contract::evidence::Declaration {
    kndo_testkit::declaration_named(ev, name)
}

fn import<'e>(ev: &'e FileEvidence, specifier: &str) -> &'e kndo_contract::evidence::Import {
    kndo_testkit::import_named(ev, specifier)
}

#[test]
fn declarations_and_reach_shapes() {
    let ev = extract(
        "src/lib.rs",
        r#"
pub fn visible() {}
fn hidden() {}
pub(crate) fn crate_wide() {}
pub(super) fn super_wide() {}
pub struct Config;
trait Runner { fn run(&self); }
pub const LIMIT: u32 = 8;
static BUFFER: u8 = 0;
pub mod outer { pub fn inner() {} }
"#,
    );
    assert_eq!(decl(&ev, "visible").reach, Reach::Exported);
    assert_eq!(decl(&ev, "hidden").reach, Reach::Private);
    // `pub(crate)` is the compiler's crate boundary — a bounded region;
    // `pub(super)` keeps Exported until module-tree regions are enumerable.
    assert_eq!(
        decl(&ev, "crate_wide").reach,
        Reach::Scoped {
            scope: "crate".into()
        }
    );
    assert_eq!(decl(&ev, "super_wide").reach, Reach::Exported);
    assert_eq!(decl(&ev, "Config").kind, SymbolKind::Type);
    assert_eq!(decl(&ev, "Runner").kind, SymbolKind::Type);
    let run = decl(&ev, "run");
    assert_eq!(run.kind, SymbolKind::Method);
    assert_eq!(
        run.owner.map(|o| o.index()),
        Some(
            ev.declarations
                .iter()
                .position(|d| d.name == "Runner")
                .unwrap()
        )
    );
    assert_eq!(decl(&ev, "LIMIT").kind, SymbolKind::Constant);
    assert_eq!(decl(&ev, "BUFFER").kind, SymbolKind::Variable);
    assert_eq!(decl(&ev, "outer").kind, SymbolKind::Module);
    assert_eq!(decl(&ev, "inner").reach, Reach::Exported);
}

#[test]
fn mod_without_body_is_an_edge_not_a_declaration() {
    // `mod foo;` is module-system plumbing, the same posture as an import
    // statement: it draws the edge and declares nothing accusable. In a lib tree,
    // `pub mod` re-publishes the child's surface; a private mod binds nothing.
    let ev = extract("src/lib.rs", "pub mod net;\nmod util;\n");
    assert!(!ev.declarations.iter().any(|d| d.name == "net"));
    let edge = import(&ev, "self::net");
    assert!(matches!(&edge.shape, ImportShape::ReexportAll));
    assert!(matches!(&edge.target, ImportTarget::Relative(_)));
    assert!(matches!(
        &import(&ev, "self::util").shape,
        ImportShape::Bindings(b) if b.is_empty()
    ));
    // In a bin-style file nothing can import, `pub mod` publishes to no one.
    let bin = extract("src/main.rs", "pub mod net;\n");
    assert!(matches!(
        &import(&bin, "self::net").shape,
        ImportShape::Bindings(b) if b.is_empty()
    ));
    // A `#[path]` attribute redirects where the module file lives.
    let redirected = extract("src/lib.rs", "#[path = \"imp/unix.rs\"]\nmod imp;\n");
    import(&redirected, "self::imp::unix");
    // An inline mod is a container of code and stays declared.
    let inline = extract("src/lib.rs", "mod inline_here { pub fn f() {} }\n");
    assert_eq!(decl(&inline, "inline_here").kind, SymbolKind::Module);
}

#[test]
fn use_shapes() {
    let ev = extract(
        "src/lib.rs",
        r#"
use crate::a::Widget;
use crate::a::{Colour, draw as paint};
use crate::b::*;
pub use crate::a::Facade;
pub use crate::c::*;
use serde::Serialize;
use crate::d::{self};
"#,
    );
    // A plain `use` leaf emits a pair: the named binding (what keeps a private
    // item a child legally imports) and the namespace record (what keeps the
    // surface alias scopes hide).
    let shapes_of = |spec: &str| -> Vec<&ImportShape> {
        ev.imports
            .iter()
            .filter(|i| match &i.target {
                ImportTarget::Relative(s) | ImportTarget::Package(s) => s == spec,
                _ => false,
            })
            .map(|i| &i.shape)
            .collect()
    };
    let widget = shapes_of("crate::a::Widget");
    assert!(widget.iter().any(
        |s| matches!(s, ImportShape::Bindings(b) if b.len() == 1 && b[0].imported == "Widget")
    ));
    assert!(
        widget
            .iter()
            .any(|s| matches!(s, ImportShape::Namespace { local } if local == "Widget"))
    );
    assert!(
        shapes_of("crate::a::draw")
            .iter()
            .any(|s| matches!(s, ImportShape::Namespace { local } if local == "paint"))
    );
    assert!(matches!(&import(&ev, "crate::b").shape, ImportShape::Glob));
    assert!(matches!(
        &import(&ev, "crate::a::Facade").shape,
        ImportShape::Reexport(b) if b.len() == 1 && b[0].imported == "Facade"
    ));
    assert!(matches!(
        &import(&ev, "crate::c").shape,
        ImportShape::ReexportAll
    ));
    let serde = import(&ev, "serde::Serialize");
    assert!(matches!(&serde.target, ImportTarget::Package(_)));
    assert!(
        shapes_of("crate::d")
            .iter()
            .any(|s| matches!(s, ImportShape::Namespace { local } if local == "d"))
    );
}

#[test]
fn inline_test_mod_rebases_super_to_this_file() {
    let ev = extract(
        "src/lib.rs",
        r#"
pub fn production() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works() { production(); }
}
"#,
    );
    // `use super::*` inside the inline mod names THIS file, not its parent.
    let glob = import(&ev, "self");
    assert!(matches!(&glob.shape, ImportShape::Glob));
    // The cfg(test) mod and the #[test] fn are both test-rooted declarations.
    let tests_ix = ev
        .declarations
        .iter()
        .position(|d| d.name == "tests")
        .unwrap();
    let works_ix = ev
        .declarations
        .iter()
        .position(|d| d.name == "works")
        .unwrap();
    for ix in [tests_ix, works_ix] {
        assert!(
            ev.roots.iter().any(|r| r.kind == RootKind::Test
                && matches!(r.target, RootTarget::Declaration(id) if id.index() == ix)),
            "declaration {ix} carries a Test root: {:#?}",
            ev.roots
        );
    }
}

#[test]
fn qualified_paths_become_deduplicated_imports() {
    let ev = extract(
        "src/main.rs",
        r#"
fn main() {
    crate::config::load();
    crate::config::load();
    util::helper();
    let s: crate::model::State = Default::default();
}
"#,
    );
    let load = import(&ev, "crate::config::load");
    match &load.shape {
        ImportShape::Bindings(b) => {
            let names: Vec<&str> = b.iter().map(|x| x.imported.as_str()).collect();
            assert_eq!(names, ["config", "load"]);
        }
        other => panic!("expected bindings, got {other:?}"),
    }
    assert_eq!(
        ev.imports
            .iter()
            .filter(
                |i| matches!(&i.target, ImportTarget::Relative(s) if s == "crate::config::load")
            )
            .count(),
        1,
        "the same path emits one edge"
    );
    assert!(matches!(
        &import(&ev, "util::helper").target,
        ImportTarget::Package(_)
    ));
    import(&ev, "crate::model::State");
    // The path's own identifiers still land as references.
    assert!(ev.references.iter().any(|r| r.name == "load"));
    assert!(ev.references.iter().any(|r| r.name == "helper"));
}

#[test]
fn attribute_roots() {
    let ev = extract(
        "src/lib.rs",
        r#"
#[test]
fn unit() {}

#[tokio::test]
async fn integration() {}

#[no_mangle]
pub extern "C" fn entry() {}

fn plain() {}
"#,
    );
    let rooted_kind = |name: &str| {
        let ix = ev.declarations.iter().position(|d| d.name == name).unwrap();
        ev.roots
            .iter()
            .find(|r| matches!(r.target, RootTarget::Declaration(id) if id.index() == ix))
            .map(|r| r.kind)
    };
    assert_eq!(rooted_kind("unit"), Some(RootKind::Test));
    assert_eq!(rooted_kind("integration"), Some(RootKind::Test));
    assert_eq!(rooted_kind("entry"), Some(RootKind::Production));
    assert_eq!(rooted_kind("plain"), None);
}

#[test]
fn impl_members_inherent_owned_trait_impls_silent() {
    let ev = extract(
        "src/lib.rs",
        r#"
pub struct Server;

impl Server {
    pub fn start(&self) {}
    fn tick(&self) {}
    const RETRIES: u8 = 3;
}

impl std::fmt::Display for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }
}

impl Elsewhere {
    fn orphan_method(&self) {}
}
"#,
    );
    let server_ix = ev
        .declarations
        .iter()
        .position(|d| d.name == "Server")
        .unwrap();
    let start = decl(&ev, "start");
    assert_eq!(start.kind, SymbolKind::Method);
    assert_eq!(start.owner.map(|o| o.index()), Some(server_ix));
    assert_eq!(start.reach, Reach::Exported);
    assert_eq!(decl(&ev, "tick").reach, Reach::Private);
    assert_eq!(
        decl(&ev, "RETRIES").owner.map(|o| o.index()),
        Some(server_ix)
    );
    // Trait impls declare nothing: `fmt` is the trait's shape, not this file's
    // accusable surface.
    assert!(!ev.declarations.iter().any(|d| d.name == "fmt"));
    // An impl for a type declared elsewhere: still a member, just ownerless.
    let orphan = decl(&ev, "orphan_method");
    assert_eq!(orphan.kind, SymbolKind::Method);
    assert!(orphan.owner.is_none());
}

#[test]
fn reference_exclusions_and_comments() {
    let ev = extract(
        "src/lib.rs",
        r#"
/// Documented.
pub fn compute(input: u32) -> u32 {
    let doubled = input * 2;
    helper(doubled)
}
// kndo:allow unused
fn helper(n: u32) -> u32 { n }
"#,
    );
    // Declaration names and parameter bindings are not uses; argument reads are.
    assert!(!ev.references.iter().any(|r| r.name == "compute"));
    assert!(ev.references.iter().any(|r| r.name == "input"));
    assert!(ev.references.iter().any(|r| r.name == "helper"));
    assert_eq!(ev.comments.len(), 2);
    // Doc-slashes strip so pragmas parse the same in every comment form.
    let source = r#"
/// Documented.
pub fn compute(input: u32) -> u32 {
    let doubled = input * 2;
    helper(doubled)
}
// kndo:allow unused
fn helper(n: u32) -> u32 { n }
"#;
    let texts: Vec<&str> = ev
        .comments
        .iter()
        .map(|c| &source[c.text.start as usize..c.text.end as usize])
        .collect();
    assert_eq!(texts, [" Documented.", " kndo:allow unused"]);
}

#[test]
fn metrics_fingerprint_structural_clones() {
    let ev = extract(
        "src/lib.rs",
        r#"
fn alpha(items: &[u32]) -> u32 {
    let mut total = 0;
    for item in items {
        if *item > 10 { total += item; }
    }
    total
}
fn beta(values: &[u32]) -> u32 {
    let mut sum = 0;
    for value in values {
        if *value > 99 { sum += value; }
    }
    sum
}
"#,
    );
    let m = |name: &str| {
        let ix = ev.declarations.iter().position(|d| d.name == name).unwrap();
        ev.metrics
            .iter()
            .find(|(id, _)| id.index() == ix)
            .map(|(_, m)| m)
            .unwrap_or_else(|| panic!("{name} has metrics"))
    };
    let a = m("alpha");
    let b = m("beta");
    assert_eq!(a.cyclomatic, 3, "for + if");
    assert_eq!(
        a.fingerprints, b.fingerprints,
        "renamed identifiers and changed numbers fingerprint identically"
    );
}

#[test]
fn broken_source_degrades_to_diagnostic() {
    let ev = extract("src/lib.rs", "pub fn broken( {{{{");
    assert!(!ev.diagnostics.is_empty());
}

#[test]
fn macro_template_names_root_their_declarations() {
    let ev = extract(
        "src/messages.rs",
        "macro_rules! log_err {\n\
             ($msg:expr) => {\n\
                 crate::messages::set_flag($msg)\n\
             };\n\
         }\n\
         pub(crate) fn set_flag(_m: &str) {}\n\
         pub(crate) fn unrelated() {}\n",
    );
    let ix = |name: &str| {
        ev.declarations_with_ids()
            .find(|(_, d)| d.name == name)
            .map(|(id, _)| id.index())
            .unwrap()
    };
    let rooted: Vec<usize> = ev
        .roots
        .iter()
        .filter_map(|r| match &r.target {
            RootTarget::Declaration(id) => Some(id.index()),
            _ => None,
        })
        .collect();
    assert!(
        rooted.contains(&ix("set_flag")),
        "a name the macro template mentions resolves at every expansion site"
    );
    assert!(
        !rooted.contains(&ix("unrelated")),
        "declarations the template never names stay unrooted"
    );
}

#[test]
fn use_items_inside_function_bodies_are_imports() {
    use kndo_contract::evidence::ImportTarget;
    let ev = kndo_testkit::extract_evidence(
        &kndo_adapter_rust::RustAdapter::new(),
        "src/lib.rs",
        "pub fn describe(b: &[u8]) -> String {\n    use bstr::ByteSlice;\n    b.as_bstr().to_string()\n}\n\
         mod inner {\n    pub fn f() { use winapi_util::file; let _ = file::typ; }\n}\n",
    );
    let packages: Vec<&str> = ev
        .imports
        .iter()
        .filter_map(|i| match &i.target {
            ImportTarget::Package(p) => Some(p.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        packages.iter().any(|p| p.starts_with("bstr")),
        "a function-body `use` is an import: {packages:?}"
    );
    assert!(
        packages.iter().any(|p| p.starts_with("winapi_util")),
        "…inside a nested module too: {packages:?}"
    );
}
