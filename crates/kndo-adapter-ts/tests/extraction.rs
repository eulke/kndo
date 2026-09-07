//! Extraction against inline sources: every import shape, reach and aliasing,
//! members, reference exclusions, comments, and degradation on broken input.

use kndo_adapter_ts::TypeScriptAdapter;
use kndo_contract::evidence::{
    Attachment, FileEvidence, ImportShape, ImportTarget, Reach, RefKind, SymbolKind, Timing,
};
use kndo_contract::vocab::Confidence;

fn extract(path: &str, source: &str) -> FileEvidence {
    kndo_testkit::extract_evidence(&TypeScriptAdapter::new(), path, source)
}

fn decl<'e>(ev: &'e FileEvidence, name: &str) -> &'e kndo_contract::evidence::Declaration {
    kndo_testkit::declaration_named(ev, name)
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
    assert_eq!(decl(&ev, "hidden").reach, Reach::File);
    let entry = decl(&ev, "entry");
    assert_eq!(entry.reach, Reach::Exported);
    assert_eq!(entry.exported_as.as_deref(), Some("default"));
    assert_eq!(decl(&ev, "secret").kind, SymbolKind::Constant);
    assert_eq!(decl(&ev, "secret").reach, Reach::File);
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
    // `import type` binds like any import and never runs: its bindings at `Erased`.
    let types = relative("./types");
    assert!(matches!(
        &types.shape,
        ImportShape::Bindings(bs) if bs.len() == 1 && bs[0].imported == "T"
    ));
    assert_eq!(types.timing, Timing::Erased);
    assert!(
        ev.imports
            .iter()
            .filter(|i| !matches!(&i.target, ImportTarget::Relative(t) if t == "./types"))
            .all(|i| i.timing == Timing::Load),
        "every static import runs at load"
    );
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
fn dynamic_loads_carry_their_moment() {
    let ev = extract(
        "src/d.ts",
        r#"
const eager = require("./eager");
export async function later() {
  const m = await import("./later");
  if (m) { const c = require("./conditional"); }
  return m;
}
import("./top-level-dynamic");
"#,
    );
    let timing = |s: &str| {
        ev.imports
            .iter()
            .find(|i| matches!(&i.target, ImportTarget::Relative(t) if t == s))
            .unwrap_or_else(|| panic!("no import {s}"))
            .timing
    };
    assert_eq!(timing("./eager"), Timing::Load, "a top-level require loads");
    assert_eq!(
        timing("./later"),
        Timing::Lazy,
        "import() runs when evaluated"
    );
    assert_eq!(
        timing("./conditional"),
        Timing::Lazy,
        "a guarded require runs later"
    );
    assert_eq!(
        timing("./top-level-dynamic"),
        Timing::Lazy,
        "even at the top level, import() runs after the static graph linked"
    );
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
fn require_and_dynamic_import_are_imports() {
    let ev = extract(
        "src/cjs.cjs",
        r#"
const util = require("./util.js");
require("./side-effect");
const pkg = require("lodash");
function later() { return import("./lazy.js"); }
const dynamic = require(someVariable);
"#,
    );
    let relative = |s: &str| {
        ev.imports
            .iter()
            .find(|i| matches!(&i.target, ImportTarget::Relative(t) if t == s))
            .unwrap_or_else(|| panic!("no relative import {s}: {:#?}", ev.imports))
    };
    assert!(matches!(
        &relative("./util.js").shape,
        ImportShape::Namespace { local } if local == "util"
    ));
    assert!(matches!(
        &relative("./side-effect").shape,
        ImportShape::SideEffect
    ));
    assert!(matches!(
        &relative("./lazy.js").shape,
        ImportShape::SideEffect
    ));
    assert!(
        ev.imports
            .iter()
            .any(|i| matches!(&i.target, ImportTarget::Package(p) if p == "lodash"))
    );
    // The non-literal require records nothing.
    assert_eq!(ev.imports.len(), 4, "{:#?}", ev.imports);
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
fn metrics_fingerprint_structural_clones() {
    let ev = extract(
        "src/m.ts",
        r#"
export function alpha(list: number[]) {
  let total = 0;
  for (const item of list) {
    if (item > 10) { total += item * 2; } else { total += item; }
  }
  return total;
}
export function beta(values: number[]) {
  let sum = 0;
  for (const v of values) {
    if (v > 99) { sum += v * 7; } else { sum += v; }
  }
  return sum;
}
export function gamma(values: number[]) {
  return values.filter((v) => v > 0).map((v) => v * 2);
}
"#,
    );
    let m = |name: &str| {
        let ix = ev
            .declarations
            .iter()
            .position(|d| d.name == name)
            .unwrap_or_else(|| panic!("decl {name}"));
        ev.metrics
            .iter()
            .find(|(id, _)| id.index() == ix)
            .map(|(_, m)| m)
            .unwrap_or_else(|| panic!("metrics for {name}: {:#?}", ev.metrics))
    };
    let (alpha, beta, gamma) = (m("alpha"), m("beta"), m("gamma"));
    // Renamed identifiers and different literals: same structure, same fingerprints.
    assert_eq!(alpha.fingerprints, beta.fingerprints);
    assert_ne!(alpha.fingerprints, gamma.fingerprints);
    // for + if + else-arm-free counting: 1 + for + if = 3.
    assert_eq!(alpha.cyclomatic, 3);
    assert!(alpha.token_count > 20);
    assert_eq!(alpha.loc, 7);
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

#[test]
fn package_specifiers_inside_string_literals_are_possible_imports() {
    use kndo_contract::evidence::{ImportShape, ImportTarget};
    let ev = kndo_testkit::extract_evidence(
        &kndo_adapter_ts::TypeScriptAdapter::new(),
        "src/plugin.ts",
        "import x from 'real-dep';\n\
         const v = _require('core-js/package.json').version;\n\
         polyfills.add(`regenerator-runtime/runtime.js`);\n\
         const code = `import \"systemjs/dist/s.min.js\";`;\n\
         const prose = 'the quick brown fox';\n\
         const alone = 'lonely-dep';\n\
         export { x, v, code, prose, alone };\n",
    );
    let possible: Vec<&str> = ev
        .imports
        .iter()
        .filter(|i| i.confidence == Confidence::Possible)
        .map(|i| match &i.target {
            ImportTarget::Package(p) => p.as_str(),
            _ => "?",
        })
        .collect();
    assert_eq!(
        possible,
        vec![
            "core-js/package.json",
            "regenerator-runtime/runtime.js",
            "systemjs/dist/s.min.js",
            "lonely-dep",
        ],
        "specifier-shaped literals with a path, or a literal that is one whole \
         specifier — never prose words: {possible:?}"
    );
    assert!(
        ev.imports
            .iter()
            .filter(|i| i.confidence == Confidence::Possible)
            .all(|i| matches!(i.shape, ImportShape::Mention)),
        "a spelled specifier binds nothing"
    );
    // The real import is still exactly one, at its own confidence.
    assert_eq!(
        ev.imports
            .iter()
            .filter(|i| matches!(&i.target, ImportTarget::Package(p) if p == "real-dep"))
            .count(),
        1
    );
}

#[test]
fn conditional_requires_are_probable() {
    let ev = extract(
        "src/index.js",
        r#"
const a = require("a");
function f() {
  return require("b");
}
const c = process.env.X ? require("c") : null;
const d = maybe || require("d");
try {
  require("e");
} catch {}
"#,
    );
    let confidence = |name: &str| {
        ev.imports
            .iter()
            .find(|i| matches!(&i.target, ImportTarget::Package(s) if s == name))
            .map(|i| i.confidence)
            .unwrap_or_else(|| panic!("{name} missing"))
    };
    assert_eq!(confidence("a"), Confidence::Certain);
    for conditional in ["b", "c", "d", "e"] {
        assert_eq!(
            confidence(conditional),
            Confidence::Probable,
            "{conditional}"
        );
    }
}

#[test]
fn a_spec_file_joins_the_project_in_a_test_run_alone() {
    // Whether the runner treats one as an ENTRY is convention; that the
    // published package does not carry it is not.
    let src = "export const a = 1;\n";
    let attachment = |path: &str| extract(path, src).attachment;
    assert_eq!(attachment("src/widget.test.ts"), Attachment::TestOnly);
    assert_eq!(attachment("src/widget.spec.js"), Attachment::TestOnly);
    assert_eq!(attachment("src/__tests__/widget.ts"), Attachment::TestOnly);
    assert_eq!(attachment("src/widget.ts"), Attachment::Regular);
}
