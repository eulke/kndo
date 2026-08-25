//! TS/TSX grammar setup — the one piece of tree-sitter-typescript-specific machinery this
//! adapter owns directly rather than through the toolkit: no other language needs this
//! grammar, so it has no business living in code every adapter compiles (`kndo-adapter-toolkit`
//! is the paved road *shared* across languages, not a dumping ground for one language's setup).

use tree_sitter::{Language, Tree};

fn typescript_language() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}

fn tsx_language() -> Language {
    tree_sitter_typescript::LANGUAGE_TSX.into()
}

/// Parse source with the TSX grammar (JSX-aware) when `tsx` is true, else plain TypeScript —
/// which also parses ordinary JS/CJS (one grammar, two module systems).
pub fn parse(source: &[u8], tsx: bool) -> Option<Tree> {
    let mut parser = tree_sitter::Parser::new();
    let lang = if tsx {
        tsx_language()
    } else {
        typescript_language()
    };
    parser.set_language(&lang).ok()?;
    parser.parse(source, None)
}

#[cfg(test)]
mod introspect {
    //! Not adapter tests — a ground-truth probe of the grammar's real field names: the
    //! extraction code in this crate is written against verified facts instead of
    //! assumptions. `cargo test -p kndo-adapter-js introspect -- --nocapture --ignored`.

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
export function foo(x: number): number { return x; }
export default function bar() {}
export class Baz {}
export default class {}
export interface Quux { id: number; }
export type Alias = string;
export const a = 1, b = 2;
export enum Color { Red, Green = "g" }
export { a as c };
export * from "./other";
import { x, y as z } from "./mod";
import type { T } from "./types";
import def, { named } from "./mixed";
"#;
        let tree = parse(src, false).unwrap();
        dump(tree.root_node(), src, 0);
    }

    #[test]
    #[ignore]
    fn dump_reference_shapes() {
        let src = br#"
function outer() {
    foo();
    obj.method();
    const { a, b: renamed } = obj;
    const literal = { key: 1, shorthand, [computed]: 2 };
    return <Foo bar={baz} />;
}
class C extends Base implements IFace {
    field: SomeType = value;
    method(): ReturnType { return new Ctor(); }
}
"#;
        let tree = parse(src, true).unwrap();
        dump(tree.root_node(), src, 0);
    }

    #[test]
    #[ignore]
    fn dump_binding_shapes() {
        let src = br#"
function f(x, y = defaultVal, { z }: Opts): void {
    const arrow = (a, b = other) => a + b + y + z;
    for (const item of items) { use(item); }
    try {} catch (e) { log(e); }
    let arr: Array<Item> = [];
    const template = `${value} and ${other.thing}`;
    counter = counter + 1;
}
"#;
        let tree = parse(src, false).unwrap();
        dump(tree.root_node(), src, 0);
    }

    #[test]
    #[ignore]
    fn dump_reexport_shapes() {
        let src = br#"
export * from "./barrel-all";
export type * from "./barrel-all-type";
export * as ns from "./barrel-ns";
export { a, b as c } from "./barrel-named";
export type { a, b as c } from "./barrel-named-type";
"#;
        let tree = parse(src, false).unwrap();
        dump(tree.root_node(), src, 0);
    }

    #[test]
    #[ignore]
    fn dump_cjs_shapes() {
        let src = br#"
const whole = require("./whole");
const { a, b: renamed } = require("./named");
let lazy = require("lodash");
require("./side-effect");
if (cond) { const nested = require("./nested"); }
const dynamic = require(someVariable);
module.exports = { f, g: localG, computed: 1 };
module.exports = function main() {};
module.exports = someExpression;
exports.foo = function () {};
exports.bar = localBar;
module.exports.baz = 42;
"#;
        let tree = parse(src, false).unwrap();
        dump(tree.root_node(), src, 0);
    }

    #[test]
    #[ignore]
    fn dump_dynamic_shapes() {
        let src = br#"
const a = await import("./literal");
import("./side-effect-dyn");
const b = await import(someVar);
const c = await import(`./locales/${lang}.json`);
const d = require("./prefix/" + name);
const e = require(`no-prefix-${x}`);
eval("code");
const f = new Function("return 1");
const g = require.resolve("./resolved");
window.eval("indirect");
"#;
        let tree = parse(src, false).unwrap();
        dump(tree.root_node(), src, 0);
    }

    #[test]
    #[ignore]
    fn dump_namespace_member_shapes() {
        let src = br#"
import * as ns from "./mod";
ns.used();
const v = ns.value;
ns[dynamicKey]();
callback(ns);
const alias = ns;
exports.storage.setItem("k", "v");
const r = exports.reader;
register(exports);
module.exports.humanize(x);
sink(module.exports);
exports.written = 1;
module.exports = whole;
"#;
        let tree = parse(src, false).unwrap();
        dump(tree.root_node(), src, 0);
    }

    #[test]
    #[ignore]
    fn dump_comment_shapes() {
        let src = br#"
// kndo:allow unused reason text
function a() {}

/* kndo:allow-file version-skew */

/**
 * kndo:allow unused:enum-member
 */
function b() {}

function c() {} // kndo:allow unused trailing comment
"#;
        let tree = parse(src, false).unwrap();
        dump(tree.root_node(), src, 0);
    }

    #[test]
    #[ignore]
    fn dump_import_clause_shapes() {
        let src = br#"
import def from "./a";
import { x, y as z } from "./b";
import def2, { named } from "./c";
import * as ns from "./d";
import type { T } from "./e";
"#;
        let tree = parse(src, false).unwrap();
        dump(tree.root_node(), src, 0);
    }
}
