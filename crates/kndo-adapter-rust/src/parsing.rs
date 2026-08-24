//! tree-sitter setup for Rust (same shape as the Go adapter's `parsing` module — each
//! grammar's plumbing stays with its adapter rather than behind a false shared abstraction).

use tree_sitter::{Language, Parser, Tree};

fn rust_language() -> Language {
    tree_sitter_rust::LANGUAGE.into()
}

pub(crate) fn parse(source: &[u8]) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&rust_language()).ok()?;
    parser.parse(source, None)
}

#[cfg(test)]
mod introspect {
    //! Ground-truth probe of tree-sitter-rust's real node/field names (same methodology as
    //! the Go and TS adapters): extraction.rs is written against verified facts, never
    //! assumptions. `cargo test -p kndo-adapter-rust introspect -- --nocapture --ignored`.

    use super::*;

    fn dump(node: tree_sitter::Node, src: &[u8], depth: usize) {
        let field = node
            .parent()
            .and_then(|p| {
                (0..p.child_count()).find_map(|i| {
                    if p.child(i)? == node {
                        p.field_name_for_child(i as u32)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or("-");
        let text = if node.child_count() == 0 {
            std::str::from_utf8(&src[node.byte_range()]).unwrap_or("?")
        } else {
            ""
        };
        println!(
            "{}{} field={} '{}'",
            "  ".repeat(depth),
            node.kind(),
            field,
            text
        );
        for i in 0..node.child_count() {
            if let Some(c) = node.child(i) {
                dump(c, src, depth + 1);
            }
        }
    }

    #[test]
    #[ignore]
    fn dump_declaration_shapes() {
        let src = br#"
pub fn free(x: u32) -> u32 { x + 1 }
pub(crate) struct S { field: u32 }
pub(super) enum E { A, B(u32) }
union U { a: u32 }
pub trait Tr { fn m(&self); fn with_default(&self) {} }
impl S { pub fn method(&self) -> u32 { self.field } }
impl Tr for S { fn m(&self) {} }
const C: u32 = 1;
static ST: u32 = 2;
type Alias = u32;
macro_rules! mymacro { () => {} }
mod inline_mod { pub fn inner() {} }
"#;
        let tree = parse(src).unwrap();
        dump(tree.root_node(), src, 0);
    }

    #[test]
    #[ignore]
    fn dump_use_and_mod_shapes() {
        let src = br#"
mod plain;
#[path = "other/loc.rs"]
mod pathed;
use crate::a::b::Thing;
use crate::a::{X, Y as Z};
use self::helpers::run;
use super::sibling;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
pub use crate::a::Reexported;
use crate::deep::*;
use crate::x as alias;
extern crate legacy;
"#;
        let tree = parse(src).unwrap();
        dump(tree.root_node(), src, 0);
    }

    #[test]
    #[ignore]
    fn dump_reference_and_body_shapes() {
        let src = br#"
fn caller() {
    helper();
    helpers::run();
    let s = S { field: 1 };
    s.method();
    s.field;
    let x: Alias = C;
    format!("{}", s);
    include!("gen.rs");
    if x > 1 && x < 10 { helper(); }
    match x { 1 => {}, _ => {} }
    let _ = maybe()?;
}
#[test]
fn a_test() { caller(); }
#[derive(Debug, Clone)]
struct D;
#[no_mangle]
pub extern "C" fn exported_ffi() {}
"#;
        let tree = parse(src).unwrap();
        dump(tree.root_node(), src, 0);
    }
}
