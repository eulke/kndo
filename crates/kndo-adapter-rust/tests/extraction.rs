//! Extraction against inline sources: reach, module edges, every `use` shape,
//! qualified-path imports, attributes as markers, impl/trait members, reference
//! exclusions, and comment spans.

use kndo_adapter_rust::RustAdapter;
use kndo_contract::evidence::{
    FileEvidence, ImportShape, ImportTarget, MarkerTarget, Reach, RootTarget, SymbolKind,
};
use kndo_contract::vocab::Confidence;

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
    // No modifier is the module's own namespace — the file, and everything the
    // file mounts under it; `pub(crate)` is the compiler's crate boundary — the
    // unit's reach; `pub(super)` names the ancestor by distance.
    assert_eq!(decl(&ev, "hidden").reach, Reach::Namespace { up: 0 });
    assert_eq!(decl(&ev, "crate_wide").reach, Reach::Unit { up: 0 });
    assert_eq!(decl(&ev, "super_wide").reach, Reach::Namespace { up: 1 });
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
fn mod_without_body_mounts_the_file_it_names() {
    // `mod foo;` is module-system plumbing, the same posture as an import
    // statement: it declares nothing accusable and MOUNTS — the file becomes
    // this module's child namespace, fenced by the `mod`'s own visibility.
    let ev = extract("src/lib.rs", "pub mod net;\nmod util;\n");
    assert!(!ev.declarations.iter().any(|d| d.name == "net"));
    let edge = import(&ev, "self::net");
    assert!(
        matches!(&edge.shape, ImportShape::Mount { namespace, reach }
            if namespace == "net" && *reach == Reach::Exported),
        "{:?}",
        edge.shape
    );
    assert!(matches!(&edge.target, ImportTarget::Relative(_)));
    assert!(
        matches!(&import(&ev, "self::util").shape, ImportShape::Mount { namespace, reach }
            if namespace == "util" && *reach == Reach::Namespace { up: 0 })
    );
    // What a bin's `pub mod` publishes is its unit's business, not the path's:
    // the shape is the same mount either way.
    let bin = extract("src/main.rs", "pub mod net;\n");
    assert!(
        matches!(&import(&bin, "self::net").shape, ImportShape::Mount { reach, .. }
            if *reach == Reach::Exported)
    );
    // A `#[path]` attribute redirects where the module file lives; the segment
    // it is mounted as stays the module's own name.
    let redirected = extract("src/lib.rs", "#[path = \"imp/unix.rs\"]\nmod imp;\n");
    assert!(
        matches!(&import(&redirected, "self::imp::unix").shape, ImportShape::Mount { namespace, .. }
            if namespace == "imp")
    );
    // An inline mod is a container of code: it stays declared, and what it
    // holds is its own.
    let inline = extract("src/lib.rs", "mod inline_here { pub fn f() {} }\n");
    assert_eq!(decl(&inline, "inline_here").kind, SymbolKind::Module);
    let module_ix = inline
        .declarations
        .iter()
        .position(|d| d.name == "inline_here")
        .unwrap();
    assert_eq!(decl(&inline, "f").owner.map(|o| o.index()), Some(module_ix));
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
    // The cfg(test) mod and the #[test] fn each carry their attribute as a
    // marker; the spec's rules make both Test roots in the engine.
    assert_eq!(markers_on(&ev, "tests"), [("cfg", vec!["test"])]);
    assert_eq!(markers_on(&ev, "works"), [("test", vec![])]);
}

/// `(path, args)` of every marker on the declaration `name`, in source order.
fn markers_on<'e>(ev: &'e FileEvidence, name: &str) -> Vec<(&'e str, Vec<&'e str>)> {
    let ix = ev
        .declarations
        .iter()
        .position(|d| d.name == name)
        .unwrap_or_else(|| panic!("declaration {name} missing: {:#?}", ev.declarations));
    ev.markers
        .iter()
        .filter(|m| matches!(m.on, MarkerTarget::Declaration(id) if id.index() == ix))
        .map(|m| (m.path.as_str(), m.args.iter().map(|a| a.as_str()).collect()))
        .collect()
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
fn attributes_are_markers_as_written() {
    let ev = extract(
        "src/lib.rs",
        r#"
#![allow(dead_code)]

#[test]
fn unit() {}

/// Doc comments may sit between an attribute and its item.
#[tokio::test(flavor = "multi_thread",  worker_threads = 2)]
async fn integration() {}

#[unsafe(no_mangle)]
pub extern "C" fn entry() {}

#[unsafe(export_name = "renamed")]
pub extern "C" fn aliased() {}

#[cfg(all(test, not(feature = "slow")))]
#[allow(dead_code, unused_variables)]
fn gated() {}

#[cfg(not(any(unix, windows)))]
fn exotic() {}

#[derive(Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[doc = "hi"]
struct Config;

#[cfg(test)]
impl Config {
    #[inline]
    fn stub() {}
}

trait Runner {
    #[must_use]
    fn run(&self);
}

fn plain() {}
"#,
    );
    // Roots are the engine's to derive; extraction states no attribute root.
    assert!(
        !ev.roots
            .iter()
            .any(|r| matches!(r.target, RootTarget::Declaration(_))),
        "{:?}",
        ev.roots
    );
    // An inner attribute at the top of the file marks the file.
    let file_markers: Vec<(&str, Vec<&str>)> = ev
        .markers
        .iter()
        .filter(|m| m.on == MarkerTarget::File)
        .map(|m| (m.path.as_str(), m.args.iter().map(|a| a.as_str()).collect()))
        .collect();
    assert_eq!(file_markers, [("allow", vec!["dead_code"])]);
    assert_eq!(markers_on(&ev, "unit"), [("test", vec![])]);
    // Arguments come as written, whitespace runs collapsed, split at the
    // top-level commas only.
    assert_eq!(
        markers_on(&ev, "integration"),
        [(
            "tokio::test",
            vec!["flavor = \"multi_thread\"", "worker_threads = 2"]
        )]
    );
    // `unsafe(…)` unwraps to the attribute inside, arguments and all.
    assert_eq!(markers_on(&ev, "entry"), [("no_mangle", vec![])]);
    assert_eq!(
        markers_on(&ev, "aliased"),
        [("export_name", vec!["\"renamed\""])]
    );
    // A cfg predicate flattens to its atoms: `all`/`any` transparent, `not`
    // a `!` prefix.
    assert_eq!(
        markers_on(&ev, "gated"),
        [
            ("cfg", vec!["test", "!feature = \"slow\""]),
            ("allow", vec!["dead_code", "unused_variables"]),
        ]
    );
    assert_eq!(
        markers_on(&ev, "exotic"),
        [("cfg", vec!["!unix", "!windows"])]
    );
    assert_eq!(
        markers_on(&ev, "Config"),
        [
            ("derive", vec!["Debug", "Clone"]),
            ("serde", vec!["rename_all = \"camelCase\""]),
            ("doc", vec!["\"hi\""]),
        ]
    );
    // An attribute on an `impl` block rides every member, before its own.
    assert_eq!(
        markers_on(&ev, "stub"),
        [("cfg", vec!["test"]), ("inline", vec![])]
    );
    assert_eq!(markers_on(&ev, "run"), [("must_use", vec![])]);
    assert!(markers_on(&ev, "plain").is_empty());
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
    assert_eq!(decl(&ev, "tick").reach, Reach::Namespace { up: 0 });
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
fn macro_template_names_are_references_to_their_declarations() {
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
    assert!(
        ev.roots.is_empty(),
        "a template body is a USE, not an entry: {:#?}",
        ev.roots
    );
    let named = |name: &str| ev.references.iter().any(|r| r.name == name);
    assert!(
        named("set_flag"),
        "the template mentions it, and the mention resolves at every expansion \
         site — what this file can say is that the name appears"
    );
    assert!(
        !named("unrelated"),
        "declarations the template never names are not referenced by it"
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

#[test]
fn crate_paths_inside_attributes_are_package_imports() {
    let ev = extract(
        "src/lib.rs",
        r#"
#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Fail {
    #[error("x")]
    X,
}

#[tokio::main]
async fn main() {}
"#,
    );
    let specs: Vec<&str> = ev
        .imports
        .iter()
        .filter_map(|i| match &i.target {
            ImportTarget::Package(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    for expected in ["thiserror::Error", "schemars::JsonSchema", "tokio::main"] {
        assert!(
            specs.contains(&expected),
            "{expected} missing from {specs:?}"
        );
    }
    // A bare derive and a helper attribute name no crate.
    assert!(
        !specs.iter().any(|s| *s == "Debug" || *s == "error"),
        "{specs:?}"
    );
}

#[test]
fn type_headed_and_use_bound_paths_are_not_imports() {
    let ev = extract(
        "src/lib.rs",
        r#"
use std::io;
mod jsont;

fn f() -> io::Result<()> {
    let v: Vec<u8> = Vec::new();
    let d = jsont::Data::new(v);
    regex::Regex::new("x").unwrap();
    Ok(())
}
"#,
    );
    let specs: Vec<&str> = ev
        .imports
        .iter()
        .filter_map(|i| match &i.target {
            ImportTarget::Package(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert!(specs.contains(&"std::io"), "{specs:?}");
    assert!(
        specs.contains(&"regex::Regex::new"),
        "a crate-rooted path: {specs:?}"
    );
    assert!(
        specs.contains(&"jsont::Data::new"),
        "a sibling module path: {specs:?}"
    );
    assert!(
        !specs
            .iter()
            .any(|s| s.starts_with("io::") || s.starts_with("Vec::")),
        "a use-bound or type-headed path names no crate: {specs:?}"
    );
}

#[test]
fn primitive_and_tool_attribute_heads_name_no_crate() {
    let ev = extract(
        "src/lib.rs",
        r#"
#[rustfmt::skip]
#[clippy::cognitive_complexity = "10"]
pub fn f() -> u64 {
    let c = char::from_u32(65);
    let m = u64::MAX;
    usize::try_from(m).map(|_| m).unwrap_or(f64::MAX as u64)
}
"#,
    );
    let specs: Vec<&str> = ev
        .imports
        .iter()
        .filter_map(|i| match &i.target {
            ImportTarget::Package(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert!(specs.is_empty(), "{specs:?}");
}

#[test]
fn a_use_headed_by_another_uses_local_is_that_path() {
    let ev = extract(
        "src/lib.rs",
        r#"
use wire::SymbolKind as WireSymbolKind;
use crate::bindings::kndo::vocab::types as wire;
use std::fmt;
use fmt::Display;
use serde;
use serde::Serialize;

fn f(_: WireSymbolKind, _: &dyn Display, _: &dyn Serialize) {}
"#,
    );
    let specs: Vec<String> = ev
        .imports
        .iter()
        .filter_map(|i| match &i.target {
            ImportTarget::Relative(s) | ImportTarget::Package(s) => Some(s.to_string()),
            _ => None,
        })
        .collect();
    assert!(
        specs.contains(&"crate::bindings::kndo::vocab::types::SymbolKind".to_string()),
        "the alias declared AFTER its use still resolves: {specs:?}"
    );
    assert!(
        specs.contains(&"std::fmt::Display".to_string()),
        "a module bound by `use` heads the path it binds: {specs:?}"
    );
    assert!(
        specs.contains(&"serde::Serialize".to_string()),
        "a crate bound under its own name stays itself: {specs:?}"
    );
    assert!(
        !specs
            .iter()
            .any(|s| s.starts_with("wire::") || s.starts_with("fmt::")),
        "no crate is named after a local: {specs:?}"
    );
}

#[test]
fn a_path_inside_a_macro_invocation_is_a_use_like_any_other() {
    // The grammar hands a macro's arguments over as raw tokens, so a path in
    // them has no node to read: only the token run finds it.
    let ev = extract(
        "src/main.rs",
        r#"
fn main() {
    println!("{}", util::helper());
    assert!(other::deep::flag());
    let _ = vec![Vec::with_capacity(1)];
}
"#,
    );
    let bound = |specifier: &str| -> Vec<String> {
        match &import(&ev, specifier).shape {
            ImportShape::Bindings(bs) => bs.iter().map(|b| b.imported.to_string()).collect(),
            other => panic!("{specifier}: {other:?}"),
        }
    };
    assert_eq!(bound("util::helper"), ["util", "helper"]);
    assert_eq!(bound("other::deep::flag"), ["other", "deep", "flag"]);
    // Inferred from tokens, not parsed: the edge keeps things alive and never
    // accuses a manifest.
    assert_eq!(import(&ev, "util::helper").confidence, Confidence::Possible);
    // A path headed by a type continues something already in scope, and the
    // `use` that brought it carries the import.
    assert!(
        !ev.imports.iter().any(|i| matches!(
            &i.target,
            ImportTarget::Package(s) | ImportTarget::Relative(s) if s.contains("Vec")
        )),
        "a type-headed path names no module"
    );
}

#[test]
fn a_token_run_is_a_path_only_where_the_tokens_touch() {
    // Two macro shapes the grammar hands over as flat tokens: a template
    // interpolation beside an absolute path (`#name ::krate::Trait`), and a
    // path behind a keyword (`Box::<dyn std::error::Error>`). Reading either
    // as one run invents a crate — `name::krate` and `error::Error` — and
    // accuses a manifest of not declaring it.
    let ev = extract(
        "src/lib.rs",
        r#"
fn f() {
    emit!(impl #name ::krate::Trait for T {});
    let _ = err!(Box::<dyn std::error::Error + Send>::from("x"));
}
"#,
    );
    let specifiers: Vec<String> = ev
        .imports
        .iter()
        .filter_map(|i| match &i.target {
            ImportTarget::Package(s) | ImportTarget::Relative(s) => Some(s.to_string()),
            _ => None,
        })
        .collect();
    assert!(
        specifiers.contains(&"krate::Trait".to_string()),
        "{specifiers:?}"
    );
    assert!(
        specifiers.contains(&"std::error::Error".to_string()),
        "{specifiers:?}"
    );
    assert!(
        !specifiers.iter().any(|s| s.starts_with("name::")),
        "a gap between the tokens ends the path: {specifiers:?}"
    );
    assert!(
        !specifiers.iter().any(|s| s.starts_with("error::")),
        "the run resumes at the identifier that broke it: {specifiers:?}"
    );
}

#[test]
fn a_path_attribute_is_anchored_where_the_reference_anchors_it() {
    // The Reference: a top-level `#[path]` is relative to the DIRECTORY THE
    // SOURCE FILE LIVES IN. For a mod-rs file that is where its children live;
    // for any other file it is one module above them.
    let non_mod_rs = extract(
        "src/a.rs",
        "#[path = \"odd.rs\"]\nmod odd;\n\nfn f() { odd::run(); }\n",
    );
    assert!(
        matches!(&import(&non_mod_rs, "super::odd").shape, ImportShape::Mount { namespace, .. }
            if namespace == "odd")
    );
    // And the alias names that same file wherever it is written, not only in a
    // `use`: an expression path substitutes the redirect too.
    assert!(
        matches!(
            &import(&non_mod_rs, "super::odd::run").shape,
            ImportShape::Bindings(_)
        ),
        "{:?}",
        non_mod_rs
            .imports
            .iter()
            .map(|i| format!("{:?}", i.target))
            .collect::<Vec<_>>()
    );

    let mod_rs = extract("src/a/mod.rs", "#[path = \"odd.rs\"]\nmod odd;\n");
    assert!(
        matches!(
            &import(&mod_rs, "self::odd").shape,
            ImportShape::Mount { .. }
        ),
        "a mod-rs file's children live in its own directory"
    );
}

#[test]
fn an_include_pastes_a_file_in() {
    let ev = extract("src/main.rs", "include!(\"gen/tables.rs\");\n");
    let edge = import(&ev, "./gen/tables.rs");
    assert!(
        matches!(edge.shape, ImportShape::Include),
        "{:?}",
        edge.shape
    );
    assert!(matches!(&edge.target, ImportTarget::Relative(_)));
    // A computed path names a file outside the tree, and `include_str!` names
    // data no adapter claims: neither draws an edge.
    let computed = extract(
        "src/main.rs",
        "include!(concat!(env!(\"OUT_DIR\"), \"/x.rs\"));\ninclude_str!(\"notes.txt\");\n",
    );
    assert!(
        !computed
            .imports
            .iter()
            .any(|i| matches!(i.shape, ImportShape::Include)),
        "{:?}",
        computed.imports.len()
    );
}
