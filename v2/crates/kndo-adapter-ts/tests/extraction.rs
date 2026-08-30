//! Extraction against inline sources: every import shape, reach and aliasing,
//! members, reference exclusions, comments, and degradation on broken input.

use kndo_adapter_ts::TypeScriptAdapter;
use kndo_contract::adapter::{LanguageAdapter, SourceFile};
use kndo_contract::evidence::{
    EvidenceSink, FileEvidence, ImportShape, ImportTarget, Reach, RefKind, SymbolKind,
};
use kndo_contract::vocab::ProjectPath;

fn extract(path: &str, source: &str) -> FileEvidence {
    let adapter = TypeScriptAdapter::new();
    let path = ProjectPath::new(path);
    let mut sink = EvidenceSink::new(source.len() as u32, adapter.spec().emits().clone());
    adapter.extract(
        &SourceFile {
            path: &path,
            content: source.as_bytes(),
        },
        &mut sink,
    );
    sink.finish()
}

fn decl<'e>(ev: &'e FileEvidence, name: &str) -> &'e kndo_contract::evidence::Declaration {
    ev.declarations
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("declaration {name} missing: {:#?}", ev.declarations))
}

#[test]
fn declarations_reach_and_aliases() {
    let ev = extract(
        "src/a.ts",
        r#"
export function visible() {}
function hidden() {}
export default function entry() {}
const secret = 1;
export const setting = 2;
let counter = 0;
function renamed() {}
export { renamed as publicName };
interface Shape {}
export type Alias = string;
enum Color { Red, Green }
"#,
    );
    assert_eq!(decl(&ev, "visible").reach, Reach::Exported);
    assert_eq!(decl(&ev, "hidden").reach, Reach::Private);
    let entry = decl(&ev, "entry");
    assert_eq!(entry.reach, Reach::Exported);
    assert_eq!(entry.exported_as.as_deref(), Some("default"));
    assert_eq!(decl(&ev, "secret").kind, SymbolKind::Constant);
    assert_eq!(decl(&ev, "secret").reach, Reach::Private);
    assert_eq!(decl(&ev, "setting").reach, Reach::Exported);
    assert_eq!(decl(&ev, "counter").kind, SymbolKind::Variable);
    let renamed = decl(&ev, "renamed");
    assert_eq!(renamed.reach, Reach::Exported);
    assert_eq!(renamed.exported_as.as_deref(), Some("publicName"));
    assert_eq!(decl(&ev, "Shape").kind, SymbolKind::Type);
    assert_eq!(decl(&ev, "Alias").reach, Reach::Exported);
    assert_eq!(decl(&ev, "Color").kind, SymbolKind::Type);
}

#[test]
fn class_members_are_owned_declarations() {
    let ev = extract(
        "src/w.ts",
        r#"
export class Widget {
  constructor() {}
  render() { this.#refresh(); }
  #refresh() {}
  onClick = () => {};
  label = "data";
}
"#,
    );
    let widget = decl(&ev, "Widget");
    assert_eq!(widget.kind, SymbolKind::Type);
    let widget_id = ev
        .declarations
        .iter()
        .position(|d| d.name == "Widget")
        .unwrap();
    for name in ["render", "#refresh", "onClick"] {
        let m = decl(&ev, name);
        assert_eq!(m.kind, SymbolKind::Method, "{name}");
        assert_eq!(m.owner.map(|o| o.index()), Some(widget_id), "{name}");
    }
    // Constructors and data fields are not member declarations.
    assert!(!ev.declarations.iter().any(|d| d.name == "constructor"));
    assert!(!ev.declarations.iter().any(|d| d.name == "label"));
    // `this.#refresh()` is a call reference.
    assert!(
        ev.references
            .iter()
            .any(|r| r.name == "#refresh" && r.kind == RefKind::Call)
    );
}

#[test]
fn imports_in_every_shape() {
    let ev = extract(
        "src/i.ts",
        r#"
import def from "./local";
import { a, b as c } from "../up";
import * as ns from "./ns";
import "./side-effect";
import type { T } from "./types";
import react from "react";
export { x, y as z } from "./re";
export * from "./all";
"#,
    );
    let shapes: Vec<(&ImportTarget, &ImportShape)> =
        ev.imports.iter().map(|i| (&i.target, &i.shape)).collect();
    assert_eq!(ev.imports.len(), 8, "{shapes:#?}");

    let relative = |s: &str| {
        ev.imports
            .iter()
            .find(|i| matches!(&i.target, ImportTarget::Relative(t) if t == s))
            .unwrap_or_else(|| panic!("no relative import {s}"))
    };
    match &relative("./local").shape {
        ImportShape::Bindings(bs) => {
            assert_eq!(bs.len(), 1);
            assert_eq!(bs[0].imported, "default");
            assert_eq!(bs[0].local, "def");
        }
        other => panic!("default import shape: {other:?}"),
    }
    match &relative("../up").shape {
        ImportShape::Bindings(bs) => {
            assert_eq!(bs.len(), 2);
            assert_eq!((bs[0].imported.as_str(), bs[0].local.as_str()), ("a", "a"));
            assert_eq!((bs[1].imported.as_str(), bs[1].local.as_str()), ("b", "c"));
        }
        other => panic!("named import shape: {other:?}"),
    }
    assert!(matches!(
        &relative("./ns").shape,
        ImportShape::Namespace { local } if local == "ns"
    ));
    assert!(matches!(
        &relative("./side-effect").shape,
        ImportShape::SideEffect
    ));
    assert!(matches!(
        &relative("./types").shape,
        ImportShape::TypeOnly(bs) if bs.len() == 1 && bs[0].imported == "T"
    ));
    assert!(
        ev.imports
            .iter()
            .any(|i| matches!(&i.target, ImportTarget::Package(p) if p == "react"))
    );
    match &relative("./re").shape {
        ImportShape::Reexport(bs) => {
            assert_eq!(bs.len(), 2);
            assert_eq!((bs[1].imported.as_str(), bs[1].local.as_str()), ("y", "z"));
        }
        other => panic!("reexport shape: {other:?}"),
    }
    assert!(matches!(&relative("./all").shape, ImportShape::ReexportAll));
}

#[test]
fn references_count_uses_not_bindings() {
    let ev = extract(
        "src/r.ts",
        r#"
import { helper } from "./h";
export function used() {}
function caller() { used(); helper(); }
class Base {}
class Sub extends Base {}
const w = new Sub();
let shape: Shape = w.compute();
export { caller };
"#,
    );
    let refs = |name: &str| -> Vec<RefKind> {
        ev.references
            .iter()
            .filter(|r| r.name == name)
            .map(|r| r.kind)
            .collect()
    };
    assert_eq!(refs("used"), vec![RefKind::Call]);
    assert_eq!(refs("helper"), vec![RefKind::Call], "{:#?}", ev.references);
    assert_eq!(refs("Base"), vec![RefKind::Extend]);
    assert_eq!(refs("Sub"), vec![RefKind::Call]);
    assert_eq!(refs("Shape"), vec![RefKind::TypeUse]);
    assert!(refs("compute").contains(&RefKind::Call));
    // Declaration names and export-clause names are not uses: `caller` appears only
    // in its declaration and in `export { caller }`.
    assert_eq!(refs("caller"), Vec::<RefKind>::new());
    // The import binding position is not a use — `helper` was counted once (its call).
    assert_eq!(refs("helper").len(), 1);
}

#[test]
fn comments_carry_stripped_text_spans() {
    let source = "// line note\nconst x = 1; /* block */\n";
    let ev = extract("src/c.ts", source);
    assert_eq!(ev.comments.len(), 2);
    let text = |i: usize| {
        let t = ev.comments[i].text;
        &source[t.start as usize..t.end as usize]
    };
    assert_eq!(text(0), " line note");
    assert_eq!(text(1), " block ");
}

#[test]
fn broken_source_degrades_with_diagnostics() {
    let ev = extract("src/b.ts", "export function ok() {}\nconst = = = ;;;\n");
    // The parsable half still yields evidence; the damage is reported, not fatal.
    assert!(ev.declarations.iter().any(|d| d.name == "ok"));
    assert!(!ev.diagnostics.is_empty());
}

#[test]
fn plain_js_and_jsx_extract_through_the_tsx_grammar() {
    let ev = extract(
        "src/App.jsx",
        r#"
import { Panel } from "./panel.js";
export function App() { return <Panel title="x" />; }
"#,
    );
    assert_eq!(decl(&ev, "App").reach, Reach::Exported);
    assert!(
        ev.references.iter().any(|r| r.name == "Panel"),
        "a JSX element name is a use: {:#?}",
        ev.references
    );
}
