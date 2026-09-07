//! Extraction against inline documents: the self-root and its kind, which tags
//! and attributes are references, which values leave the project.

use kndo_adapter_html::HtmlAdapter;
use kndo_contract::evidence::{Attachment, 
    FileEvidence, ImportShape, ImportTarget, RegionMode, RootKind, RootTarget,
};
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
            region: None,
        },
        &mut sink,
    );
    let ev = sink.finish();
    assert!(ev.roots.is_empty() && ev.imports.is_empty() && ev.diagnostics.is_empty());
}

#[test]
fn an_inline_script_or_style_is_a_region_of_its_language() {
    let source = r##"<script type="module">
  import def from './default.js'
</script>
<script>
  function classic() {}
</script>
<script type="module" src="./with-src.js">
  import "./body-of-a-src-script-is-ignored.js";
</script>
<script type="importmap">{ "imports": {} }</script>
<script type="text/javascript; charset=utf-8">var legacy = 1;</script>
<style>
  @import "./inline.css";
</style>
<style>   </style>
<!-- <script>function commentedOut() {}</script> -->"##;
    let ev = extract("index.html", source);
    assert_eq!(
        specifiers(&ev),
        ["./with-src.js"],
        "a page's own imports are its attributes'"
    );
    let regions: Vec<(&str, RegionMode, &str)> = ev
        .embedded
        .iter()
        .map(|r| {
            (
                r.language.as_str(),
                r.mode,
                &source[r.span.start as usize..r.span.end as usize],
            )
        })
        .collect();
    assert_eq!(
        regions,
        [
            (
                "js",
                RegionMode::Module,
                "\n  import def from './default.js'\n"
            ),
            ("js", RegionMode::Script, "\n  function classic() {}\n"),
            ("js", RegionMode::Script, "var legacy = 1;"),
            ("css", RegionMode::Module, "\n  @import \"./inline.css\";\n"),
        ],
        "a src script's body, a data script, an empty style and a commented-out \
         script are no region"
    );
    assert!(
        ev.declarations.is_empty(),
        "a region's code is its language's to read, never this adapter's"
    );
}

#[test]
fn a_scripts_body_is_text_and_a_page_may_not_be_ascii() {
    // lodash's test pages write their script tags from JavaScript strings.
    let source = r##"<title>Ünïcödé — tests</title>
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
<script src="./real.js"></script>"##;
    let ev = extract("test/index.html", source);
    assert_eq!(specifiers(&ev), ["./real.js"]);
    let regions: Vec<&str> = ev
        .embedded
        .iter()
        .map(|r| &source[r.span.start as usize..r.span.end as usize])
        .collect();
    assert_eq!(
        regions.len(),
        3,
        "the classic script, the style, the module"
    );
    assert!(
        regions[2].contains("import \"./réel.js\""),
        "a region's span is the body's bytes, after non-ASCII text: {:?}",
        regions[2]
    );
}

#[test]
fn a_page_under_a_test_path_joins_the_project_in_a_test_run_alone() {
    assert_eq!(
        extract("src/__tests__/harness.html", "<p>x</p>\n").attachment,
        Attachment::TestOnly
    );
    assert_eq!(
        extract("src/index.html", "<p>x</p>\n").attachment,
        Attachment::Regular
    );
}
