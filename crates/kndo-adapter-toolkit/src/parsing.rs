//! Shared tree-sitter setup (ADR 0002 — the paved road for first-party adapters).

use tree_sitter::{Language, Parser, Tree};

pub fn typescript_language() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}

pub fn tsx_language() -> Language {
    tree_sitter_typescript::LANGUAGE_TSX.into()
}

/// Parse source with the TSX grammar (JSX-aware) when `tsx` is true, else plain TypeScript —
/// which also parses ordinary JS/CJS (one grammar, two module systems; spec §1).
pub fn parse(source: &[u8], tsx: bool) -> Option<Tree> {
    let mut parser = Parser::new();
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
    //! Not adapter tests — a ground-truth probe of the grammar's real field names, run once
    //! to write the extraction code in kndo-adapter-js against verified facts instead of
    //! assumptions. `cargo test -p kndo-adapter-toolkit introspect -- --nocapture --ignored`.

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
