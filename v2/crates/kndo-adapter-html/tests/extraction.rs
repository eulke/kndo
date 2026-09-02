//! Extraction against inline documents: the self-root and its kind, which tags
//! and attributes are references, which values leave the project.

use kndo_adapter_html::HtmlAdapter;
use kndo_contract::evidence::{FileEvidence, ImportShape, ImportTarget, RootKind, RootTarget};
use kndo_contract::vocab::Confidence;

fn extract(path: &str, source: &str) -> FileEvidence {
    kndo_testkit::extract_evidence(&HtmlAdapter::new(), path, source)
}

fn specifiers(ev: &FileEvidence) -> Vec<String> {
    ev.imports
        .iter()
        .filter_map(|i| match &i.target {
            ImportTarget::Relative(s) | ImportTarget::Package(s) => Some(s.to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_document_roots_itself_by_where_it_lives() {
    let page = extract("app/index.html", "<!doctype html><html></html>");
    assert_eq!(page.roots.len(), 1);
    assert!(matches!(page.roots[0].target, RootTarget::WholeFile));
    assert_eq!(page.roots[0].kind, RootKind::Production);
    assert_eq!(page.roots[0].confidence, Confidence::Certain);
    assert!(page.declarations.is_empty() && page.imports.is_empty());

    let test = extract("test/index.html", "<html></html>");
    assert_eq!(test.roots[0].kind, RootKind::Test);
    assert_eq!(test.roots[0].confidence, Confidence::Probable);
}

#[test]
fn scripts_and_code_loading_links_are_the_references() {
    let ev = extract(
        "index.html",
        r##"<!doctype html>
<html>
  <head>
    <LINK rel="icon" href="/favicon.svg">
    <link rel="stylesheet" href='./style.css'>
    <link rel="modulepreload" href="/src/util.ts">
    <link rel="preload" as="script" href="/src/worker.js">
    <link rel="preload" as="font" href="/fonts/a.woff2">
    <link rel="manifest" href="./manifest.json">
    <!-- <script src="./retired.js"></script> -->
  </head>
  <body>
    <img src="./logo.png">
    <script type="module" src="/src/main.ts?v=2"></script>
    <script src=vendor/lib.js></script>
    <script data-src="./lazy.js"></script>
    <scripting-thing src="./not-a-script.js"></scripting-thing>
    <script>inline()</script>
  </body>
</html>"##,
    );
    assert_eq!(
        specifiers(&ev),
        [
            "./style.css",
            "/src/util.ts",
            "/src/worker.js",
            "/src/main.ts",
            "./vendor/lib.js",
        ],
        "an attribute URL is document-relative: a bare one is spelled so"
    );
    assert!(
        ev.imports
            .iter()
            .all(|i| matches!(i.shape, ImportShape::SideEffect)
                && i.confidence == Confidence::Certain)
    );
}

#[test]
fn the_span_covers_the_value_as_written() {
    let source = r##"<script src="./main.js?v=1"></script>"##;
    let ev = extract("index.html", source);
    let span = ev.imports[0].span;
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        "./main.js?v=1"
    );
}

#[test]
fn references_that_leave_the_project_are_not_imports() {
    let ev = extract(
        "index.html",
        r##"<script src="https://cdn.example.com/x.js"></script>
<script src="//cdn.example.com/y.js"></script>
<script src="data:text/javascript,1"></script>
<script src="${entry}"></script>
<script src="{{ entry }}"></script>
<script src="#top"></script>
<script src=""></script>"##,
    );
    assert!(ev.imports.is_empty(), "{:?}", specifiers(&ev));
}

#[test]
fn a_binary_file_yields_nothing_and_says_nothing() {
    let ev = kndo_testkit::extract_evidence(&HtmlAdapter::new(), "x.html", "\u{fffd}");
    assert_eq!(ev.roots.len(), 1, "utf-8 text of any content still roots");
    let mut sink =
        kndo_contract::evidence::EvidenceSink::new(3, HtmlAdapter::new().spec().emits().clone());
    use kndo_contract::extension::Extension;
    HtmlAdapter::new().extract(
        &kndo_contract::adapter::SourceFile {
            path: &kndo_contract::vocab::ProjectPath::new("x.html"),
            content: &[0xff, 0xfe, 0x00],
        },
        &mut sink,
    );
    let ev = sink.finish();
    assert!(ev.roots.is_empty() && ev.imports.is_empty() && ev.diagnostics.is_empty());
}

#[test]
fn an_inline_module_scripts_imports_are_references() {
    let ev = extract(
        "index.html",
        r##"<script type="module">
  import "./side-effect.css";
  import def from './default.js'
  import { a, b as c } from "/src/named.ts";
  import * as ns from "vue";
  export { x } from './reexport.js';
  export const local = import.meta.url;
  // import "./commented-out.js";
  /* import "./also-commented.js"; */
  const s = "import './inside-a-string.js'";
  const lazy = () => import("./lazy.js");
  fromage("./not-an-import.js");
</script>
<script>
  import "./classic-scripts-cannot-import.js";
</script>
<script type="module" src="./with-src.js">
  import "./body-of-a-src-script-is-ignored.js";
</script>"##,
    );
    let imports: Vec<(String, String, Confidence)> = ev
        .imports
        .iter()
        .map(|i| {
            let (kind, spec) = match &i.target {
                ImportTarget::Relative(s) => ("rel", s.to_string()),
                ImportTarget::Package(s) => ("pkg", s.to_string()),
                _ => ("?", String::new()),
            };
            (kind.to_string(), spec, i.confidence)
        })
        .collect();
    let expected = [
        ("rel", "./side-effect.css", Confidence::Certain),
        ("rel", "./default.js", Confidence::Certain),
        ("rel", "/src/named.ts", Confidence::Certain),
        ("pkg", "vue", Confidence::Certain),
        ("rel", "./reexport.js", Confidence::Certain),
        ("rel", "./lazy.js", Confidence::Probable),
        ("rel", "./with-src.js", Confidence::Certain),
    ];
    assert_eq!(
        imports,
        expected
            .iter()
            .map(|(k, s, c)| (k.to_string(), s.to_string(), *c))
            .collect::<Vec<_>>()
    );
    assert!(
        ev.imports[..6]
            .iter()
            .all(|i| matches!(i.shape, ImportShape::Glob)),
        "nothing in the document names what an inline import took"
    );
    let source = "<script type=\"module\">import x from './a.js'</script>";
    let ev = extract("i.html", source);
    let span = ev.imports[0].span;
    assert_eq!(&source[span.start as usize..span.end as usize], "./a.js");
}

#[test]
fn a_scripts_body_is_text_and_a_page_may_not_be_ascii() {
    // lodash's test pages write their script tags from JavaScript strings.
    let ev = extract(
        "test/index.html",
        r##"<title>Ünïcödé — tests</title>
<script>
  document.write('<script src="./' + ui.buildPath + '"><\/script>');
  var s = '<link rel="stylesheet" href="./' + theme + '.css">';
</script>
<style>
  /* <script src="./in-a-stylesheet.js"></script> */
</style>
<script type="module">
  import "./réel.js"; // après un caractère non ASCII
</script>
<script src="./real.js"></script>"##,
    );
    assert_eq!(specifiers(&ev), ["./réel.js", "./real.js"]);
}
