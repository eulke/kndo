//! End-to-end proof of **every built-in plugin**, in the baseline-then-plugin shape
//! `docs/src/plugins/authoring.md` requires of anyone writing one:
//!
//! > make a fixture project exhibiting your conventions, run kndo *without* your plugin
//! > (baseline — the findings your plugin should fix must actually fire, **or your test is
//! > vacuous**), then *with* it, and assert the delta.
//!
//! Each scenario runs the same tree twice through `kndo::open` — once without whatever
//! activates the plugin, once with it — and asserts four things, not one:
//!
//! 1. **The baseline fires.** The findings the plugin exists to remove are present without it.
//!    Skipping this is how a proof passes while proving nothing.
//! 2. **Exactly those disappear.** Not "fewer findings" — the named ones.
//! 3. **Unrelated dead code stays reported.** The blanket-suppression failure mode: a plugin
//!    that keeps everything alive would pass (1) and (2) and be worthless.
//! 4. **The contribution record matches.** `PluginContribution`'s roots/edges/annotations, and
//!    `dropped` — the targets that did not resolve. A plugin whose fixture *should* produce
//!    drops (a UIKit outlet on a framework class) must produce exactly those and no others;
//!    that is what turns "contributed 0" into a debuggable fact rather than a dead end.
//!
//! The suite is closed against `kndo::default_plugins()` at the bottom
//! (`every_built_in_plugin_is_proven_here`): a new built-in without a proof fails this test,
//! the same way `Plugin::mutates_graph()` has no default so nobody can forget to decide it.

use std::path::Path;

use kndo_core::plugin::PluginContribution;

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn check(root: &Path) -> kndo_core::engine::RunResult {
    let overrides = kndo_core::engine::ConfigOverrides {
        use_cache: false,
        threads: Some(1),
        ..kndo_core::engine::ConfigOverrides::default()
    };
    let mut engine = kndo::open(root, overrides).expect("kndo::open");
    engine.check(kndo_core::engine::RunMode::Full)
}

/// Every finding of one category, as `path` or `path#symbol` — the spelling `kndo explain`
/// and the human renderer both use, so a failing assertion names something a reader can paste
/// back into the CLI.
fn findings_of(result: &kndo_core::engine::RunResult, category: &str) -> Vec<String> {
    let mut out: Vec<String> = result
        .findings
        .iter()
        .filter(|f| f.category == category)
        .map(|f| {
            let path = f
                .location
                .path
                .as_ref()
                .map(|p| p.0.to_string())
                .unwrap_or_default();
            match &f.location.symbol {
                Some(symbol) => format!("{path}#{symbol}"),
                None => path,
            }
        })
        .collect();
    out.sort();
    out
}

/// Paths of every `unused` finding — the surface most convention plugins exist to clean up,
/// kept as its own helper because that is how the majority of these proofs read.
fn unused_paths(result: &kndo_core::engine::RunResult) -> Vec<String> {
    findings_of(result, "unused")
        .into_iter()
        .map(|s| s.split('#').next().unwrap_or_default().to_string())
        .collect()
}

fn active_ids(root: &Path) -> Vec<String> {
    kndo::plugin_resolution(root)
        .plugins
        .into_iter()
        .filter(|p| p.active.is_some())
        .map(|p| p.id)
        .collect()
}

/// What the plugin actually put into the graph this run. `None` means its graph-mutation
/// hooks never ran — either it did not activate, or it declares `mutates_graph() == false`,
/// and those are different bugs with the same symptom, which is why the assertion helpers
/// below say which one they found.
fn contribution<'a>(
    result: &'a kndo_core::engine::RunResult,
    id: &str,
) -> Option<&'a PluginContribution> {
    result.plugin_contributions.iter().find(|c| c.id == id)
}

/// Asserts the plugin ran and contributed exactly this much. `dropped` is spelled out rather
/// than counted: a drop is a target that did not resolve, and the difference between "the two
/// UIKit framework outlets we expect to miss" and "everything missed because the walk broke"
/// is invisible in a count.
#[track_caller]
fn assert_contributed(
    result: &kndo_core::engine::RunResult,
    id: &str,
    roots: u32,
    edges: u32,
    annotations: u32,
) {
    let c = contribution(result, id).unwrap_or_else(|| {
        panic!(
            "{id} contributed nothing this run — it did not activate, or it declares \
             mutates_graph() == false. Contributions present: {:?}",
            result
                .plugin_contributions
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        (c.roots, c.edges, c.annotations),
        (roots, edges, annotations),
        "{id} contribution record (dropped: {:?})",
        c.dropped
    );
}

/// Categories no analysis judged this run. An **ingester's** proof lives here rather than in
/// the contribution record: `mutates_graph() == false`, so it puts no facts in the graph — the
/// delta it must produce is an analysis that stops saying "unknown". `crap` abstains with no
/// coverage ingested, and zero `crap` findings then means *unmeasured*, not clean. That is the
/// exact confusion these four plugins exist to resolve, so it is what their proofs assert.
fn abstained(result: &kndo_core::engine::RunResult) -> Vec<String> {
    let mut out: Vec<String> = result
        .abstained
        .iter()
        .map(|a| a.category.to_string())
        .collect();
    out.sort();
    out
}

/// One source file whose function is complex enough for `crap` to have something to say once
/// it can measure — shared by all four ingester proofs so the only variable between them is
/// the report format and its path.
fn write_measurable_source(root: &Path) {
    write(
        root,
        "package.json",
        r#"{"name": "f", "main": "src/index.js"}"#,
    );
    write(
        root,
        "src/index.js",
        "export function classify(n) {\n         \x20 if (n > 10) { return 'big'; }\n         \x20 if (n > 5) { return 'mid'; }\n         \x20 if (n > 0) { return 'small'; }\n         \x20 return 'zero';\n         }\n",
    );
}

/// The shared body of the four ingester proofs: without a report `crap` abstains; with one at
/// the format's own well-known path it judges. Each caller supplies only what differs.
#[track_caller]
fn ingester_proof(id: &str, report_path: &str, report: &str) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_measurable_source(root);

    let without = check(root);
    assert!(
        abstained(&without).contains(&"crap".to_string()),
        "{id}: the baseline must abstain, or this proof is vacuous: {:?}",
        abstained(&without)
    );

    write(root, report_path, report);
    let with = check(root);
    assert!(
        !abstained(&with).contains(&"crap".to_string()),
        "{id}: with a report at {report_path}, crap must judge instead of abstaining: {:?} \
         (diagnostics: {:?})",
        abstained(&with),
        with.diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        contribution(&with, id).is_none(),
        "{id} is an ingester: it must put no facts in the graph"
    );
}

#[test]
fn coverage_lcov_ingests_from_its_well_known_path() {
    ingester_proof(
        "kndo:coverage-lcov",
        "coverage/lcov.info",
        "TN:\nSF:src/index.js\nDA:1,1\nDA:2,1\nDA:3,0\nDA:4,0\nDA:5,0\nDA:6,1\nLF:6\nLH:3\nend_of_record\n",
    );
}

#[test]
fn coverage_cobertura_ingests_from_its_well_known_path() {
    ingester_proof(
        "kndo:coverage-cobertura",
        "coverage.xml",
        r#"<?xml version="1.0"?>
<coverage><packages><package name="src"><classes>
  <class filename="src/index.js"><lines>
    <line number="1" hits="1"/><line number="2" hits="1"/><line number="3" hits="0"/>
    <line number="4" hits="0"/><line number="5" hits="0"/><line number="6" hits="1"/>
  </lines></class>
</classes></package></packages></coverage>
"#,
    );
}

#[test]
fn coverage_jacoco_ingests_from_its_well_known_path() {
    ingester_proof(
        "kndo:coverage-jacoco",
        "jacoco.xml",
        r#"<?xml version="1.0"?>
<report><package name="src">
  <sourcefile name="index.js">
    <line nr="1" ci="1"/><line nr="2" ci="1"/><line nr="3" ci="0"/>
    <line nr="4" ci="0"/><line nr="5" ci="0"/><line nr="6" ci="1"/>
  </sourcefile>
</package></report>
"#,
    );
}

#[test]
fn coverage_go_ingests_from_its_well_known_path() {
    ingester_proof(
        "kndo:coverage-go",
        "coverage.out",
        "mode: set\nsrc/index.js:1.1,2.2 1 1\nsrc/index.js:3.1,5.2 3 0\nsrc/index.js:6.1,6.2 1 1\n",
    );
}

/// The shared body of the three **machinery-dispatch** proofs (`serde`, `rkyv`, `wasmtime`).
/// Each marks the members of a hand-written impl as implicitly invoked, so the finding they
/// remove is `untested` — a framework calls those methods, no test ever does by name, and
/// without the plugin the impl reads as a test blind spot.
///
/// The fixture needs real test roots for `untested` to judge at all: with none it abstains,
/// and an abstaining analysis would make every one of these proofs vacuous.
#[track_caller]
fn machinery_proof(id: &str, crate_dep: &str, lib_rs: &str, marked: &str) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/lib.rs", lib_rs);
    // Exercises `build` and nothing else — so `never_tested` below is the control that must
    // stay reported in both runs, and the impl member is the one the plugin rescues.
    write(
        root,
        "tests/it.rs",
        "#[test]\nfn builds() {\n    let _ = f::build();\n}\n",
    );

    let manifest = "[package]\nname = \"f\"\nversion = \"0.1.0\"\n";
    write(root, "Cargo.toml", manifest);
    let without = check(root);
    let untested_without = findings_of(&without, "untested");
    assert!(
        untested_without.contains(&marked.to_string()),
        "{id}: the baseline must report {marked}, or this proof is vacuous: \
         {untested_without:?} (abstained: {:?})",
        abstained(&without)
    );

    write(
        root,
        "Cargo.toml",
        &format!("{manifest}\n[dependencies]\n{crate_dep} = \"1\"\n"),
    );
    let with = check(root);
    let untested_with = findings_of(&with, "untested");
    assert!(
        !untested_with.contains(&marked.to_string()),
        "{id}: the machinery invokes {marked}; it is not a test blind spot: {untested_with:?}"
    );
    assert!(
        untested_with.contains(&"src/lib.rs#never_tested".to_string()),
        "{id} must not blanket-suppress: a genuinely untested function stays reported: \
         {untested_with:?}"
    );
    assert!(active_ids(root).contains(&id.to_string()));
}

#[test]
fn serde_impls_are_machinery_invoked_and_gated_by_the_manifest() {
    machinery_proof(
        "kndo:serde",
        "serde",
        "pub struct Thing {\n    pub n: u32,\n}\n\n         impl serde::Serialize for Thing {\n         \x20   fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {\n         \x20       s.serialize_u32(self.n)\n         \x20   }\n         }\n\n         pub fn build() -> Thing {\n    Thing { n: 1 }\n}\n\n         pub fn never_tested() -> u32 {\n    7\n}\n",
        "src/lib.rs#Thing.serialize",
    );
}

#[test]
fn rkyv_impls_are_machinery_invoked_and_gated_by_the_manifest() {
    machinery_proof(
        "kndo:rkyv",
        "rkyv",
        "pub struct Thing {\n    pub n: u32,\n}\n\n         impl rkyv::Archive for Thing {\n         \x20   fn resolve(&self, out: u32) -> u32 {\n         \x20       out + self.n\n         \x20   }\n         }\n\n         pub fn build() -> Thing {\n    Thing { n: 1 }\n}\n\n         pub fn never_tested() -> u32 {\n    7\n}\n",
        "src/lib.rs#Thing.resolve",
    );
}

#[test]
fn wasmtime_generated_host_traits_are_machinery_invoked() {
    // Recognized by the SHAPE `bindgen!` gives the trait (`Host…` / `…Imports`), never by a
    // list of names — so the fixture uses a trait no wasmtime version has ever shipped, which
    // is the point: a real host's generated trait is named by its own WIT world.
    machinery_proof(
        "kndo:wasmtime",
        "wasmtime",
        "pub trait HostThings {\n    fn read_thing(&mut self) -> u32;\n}\n\n         pub struct Host {\n    pub n: u32,\n}\n\n         impl HostThings for Host {\n         \x20   fn read_thing(&mut self) -> u32 {\n         \x20       self.n\n         \x20   }\n         }\n\n         pub fn build() -> Host {\n    Host { n: 1 }\n}\n\n         pub fn never_tested() -> u32 {\n    7\n}\n",
        "src/lib.rs#Host.read_thing",
    );
}

#[test]
fn info_plist_principal_class_is_gated_by_the_bundle() {
    // Alamofire's shape: an Apple bundle names a class as a string and the system
    // instantiates it. `main.swift` exists so the two dead files do not roll up into one
    // directory finding — a rollup would still pass the assertions below while hiding which
    // file the plugin actually rescued.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "App/main.swift",
        "let boot = Bootstrap()\nboot.run()\n\nclass Bootstrap {\n    func run() {}\n}\n",
    );
    write(
        root,
        "App/SceneRoot.swift",
        "class SceneRoot {\n    func start() {}\n}\n",
    );
    write(
        root,
        "App/Orphan.swift",
        "class Orphan {\n    func neverCalled() -> Int { return 1 }\n}\n",
    );

    let without = check(root);
    let unused_without = findings_of(&without, "unused");
    assert!(
        unused_without.contains(&"App/SceneRoot.swift".to_string()),
        "the baseline must report the principal class's file: {unused_without:?}"
    );
    assert!(!active_ids(root).contains(&"kndo:info-plist".to_string()));

    write(
        root,
        "App/Info.plist",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n         <plist version=\"1.0\">\n<dict>\n         \x20 <key>NSPrincipalClass</key>\n         \x20 <string>SceneRoot</string>\n         </dict>\n</plist>\n",
    );
    let with = check(root);
    let unused_with = findings_of(&with, "unused");
    assert!(
        !unused_with.contains(&"App/SceneRoot.swift".to_string()),
        "the bundle names it, so the system instantiates it: {unused_with:?}"
    );
    assert!(
        unused_with.contains(&"App/Orphan.swift".to_string()),
        "a class the bundle does not name stays reported: {unused_with:?}"
    );
    assert_contributed(&with, "kndo:info-plist", 1, 0, 0);
}

/// Kingfisher's demo, reduced to the one shape that mattered: a view controller whose
/// `@IBOutlet` is touched only inside its own file, so `internal-only` says "private would
/// suffice" — which would break the storyboard connection at runtime.
const STORYBOARD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB" targetRuntime="iOS.CocoaTouch">
  <scenes>
    <scene sceneID="s-1">
      <objects>
        <viewController id="vc-1" customClass="GIFViewController" customModule="App">
          <view key="view" id="v-1">
            <connections>
              <outlet property="imageView" destination="iv-1" id="o-1"/>
              <outlet property="dataSource" destination="iv-1" id="o-2"/>
            </connections>
          </view>
        </viewController>
      </objects>
    </scene>
  </scenes>
</document>
"#;

#[test]
fn uikit_outlets_are_gated_by_the_storyboard() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "App/GIFViewController.swift",
        "import UIKit\n\n         class GIFViewController: UIViewController {\n         \x20   @IBOutlet var imageView: UIImageView!\n\n         \x20   override func viewDidLoad() {\n         \x20       super.viewDidLoad()\n         \x20       imageView.alpha = 1\n         \x20   }\n         }\n",
    );
    // The control: dead in both runs. A plugin that kept everything alive would pass every
    // other assertion here.
    write(
        root,
        "App/Orphan.swift",
        "class Orphan {\n    func neverCalled() -> Int { return 1 }\n}\n",
    );

    let without = check(root);
    assert_eq!(
        findings_of(&without, "internal-only"),
        vec!["App/GIFViewController.swift#GIFViewController.imageView"],
        "the baseline must fire, or this proof is vacuous"
    );
    assert!(!active_ids(root).contains(&"kndo:uikit".to_string()));

    write(root, "App/Base.lproj/Main.storyboard", STORYBOARD);
    let with = check(root);

    assert!(
        findings_of(&with, "internal-only").is_empty(),
        "the storyboard references the outlet from outside the file: {:?}",
        findings_of(&with, "internal-only")
    );
    assert_eq!(
        unused_paths(&with),
        vec!["App/Orphan.swift"],
        "unrelated dead code stays reported"
    );
    assert!(active_ids(root).contains(&"kndo:uikit".to_string()));

    // One root (the class UIKit instantiates), two edges (the class, plus the one outlet that
    // names a real declaration). `dataSource` is UIKit's own property, not the controller's —
    // it must drop, and the drop is the assertion: "contributed 2 edges" alone would look
    // identical if the walk had silently lost `imageView` and kept something else.
    assert_contributed(&with, "kndo:uikit", 1, 2, 0);
    let dropped = &contribution(&with, "kndo:uikit").unwrap().dropped;
    assert_eq!(
        dropped.len(),
        1,
        "exactly the framework outlet: {dropped:?}"
    );
    assert!(dropped[0].contains("dataSource"), "{dropped:?}");
}

#[test]
fn nextjs_pages_router_roots_are_gated_by_the_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // A page nothing imports (file-system routed), whose data hook nothing calls — plus one
    // genuinely dead module that must stay reported either way.
    write(
        root,
        "pages/index.tsx",
        "export default function Home() { return null; }\n\
         export function getServerSideProps() { return { props: {} }; }\n",
    );
    write(
        root,
        "src/orphan.ts",
        "export function orphan() { return 1; }\n",
    );

    write(root, "package.json", r#"{"dependencies": {"react": "18"}}"#);
    let without = check(root);
    let unused_without = unused_paths(&without);
    assert!(
        unused_without.iter().any(|p| p == "pages/index.tsx"),
        "without `next` in the manifest the plugin must stay inactive and the page is an \
         orphan: {unused_without:?}"
    );
    assert!(!active_ids(root).contains(&"kndo:nextjs".to_string()));

    write(
        root,
        "package.json",
        r#"{"dependencies": {"react": "18", "next": "14"}}"#,
    );
    let with = check(root);
    let unused_with = unused_paths(&with);
    assert!(
        !unused_with.iter().any(|p| p == "pages/index.tsx"),
        "with `next` declared, the routed page and its exports are framework-consumed: \
         {unused_with:?}"
    );
    assert!(
        unused_with.iter().any(|p| p == "src/orphan.ts"),
        "the plugin must not blanket-suppress: genuinely dead code stays reported: \
         {unused_with:?}"
    );
    assert!(active_ids(root).contains(&"kndo:nextjs".to_string()));
}

#[test]
fn nextjs_app_router_matches_reserved_basenames_per_monorepo_package() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // The `next` dependency lives in a nested package, not the root manifest (a monorepo-
    // wide manifest scan finds it), and the app root is derived from that package.json — so
    // `apps/web/app/page.tsx` classifies while a colocated non-reserved module does not.
    write(root, "package.json", r#"{"dependencies": {}}"#);
    write(
        root,
        "apps/web/package.json",
        r#"{"dependencies": {"next": "15"}}"#,
    );
    write(
        root,
        "apps/web/app/page.tsx",
        "export const revalidate = 60;\n\
         export default function Page() { return null; }\n",
    );
    write(
        root,
        "apps/web/app/unwired.tsx",
        "export function Unwired() { return null; }\n",
    );

    let result = check(root);
    let unused = unused_paths(&result);
    assert!(
        !unused.iter().any(|p| p == "apps/web/app/page.tsx"),
        "the reserved basename under the nested app root is routed: {unused:?}"
    );
    assert!(
        unused.iter().any(|p| p == "apps/web/app/unwired.tsx"),
        "a non-reserved basename under app/ is an ordinary module and must still earn \
         reachability through imports: {unused:?}"
    );
}

#[test]
fn nextjs_custom_page_extensions_narrow_the_pages_tier() {
    // next.config.js customizes pageExtensions to only the `.page.tsx` suffix —
    // read through the content channel, this must stop treating a plain `.tsx` under pages/
    // as routed (real Next.js wouldn't route it either), while the `.page.tsx` sibling still
    // gets rooted.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"dependencies": {"react": "18", "next": "14"}}"#,
    );
    write(
        root,
        "next.config.js",
        "module.exports = { pageExtensions: ['page.tsx'] };\n",
    );
    write(
        root,
        "pages/index.page.tsx",
        "export default function Home() { return null; }\n",
    );
    write(
        root,
        "pages/about.tsx",
        "export default function About() { return null; }\n",
    );

    let result = check(root);
    let unused = unused_paths(&result);
    assert!(
        !unused.iter().any(|p| p == "pages/index.page.tsx"),
        "the custom-suffixed page is still routed: {unused:?}"
    );
    assert!(
        unused.iter().any(|p| p == "pages/about.tsx"),
        "a plain .tsx no longer qualifies once pageExtensions is customized: {unused:?}"
    );
}

#[test]
fn express_conventional_entry_is_gated_by_the_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // The generator-layout shape: `app.js` is launched by an unclaimed script (`bin/www`),
    // so nothing in the graph imports it — but everything it wires must stay live through it.
    write(
        root,
        "app.js",
        "import { router } from './routes.js';\nexport const app = { router };\n",
    );
    write(root, "routes.js", "export const router = {};\n");

    write(root, "package.json", r#"{"dependencies": {}}"#);
    let without = check(root);
    let unused_without = unused_paths(&without);
    assert!(
        unused_without.iter().any(|p| p == "app.js"),
        "without `express` declared the entry convention must not fire: {unused_without:?}"
    );

    write(
        root,
        "package.json",
        r#"{"dependencies": {"express": "4"}}"#,
    );
    let with = check(root);
    let unused_with = unused_paths(&with);
    assert!(
        !unused_with
            .iter()
            .any(|p| p == "app.js" || p == "routes.js"),
        "the rooted entry keeps itself and everything it imports live: {unused_with:?}"
    );
    assert!(active_ids(root).contains(&"kndo:express".to_string()));
}

#[test]
fn express_manifest_derived_entry_rescues_a_non_conventionally_named_file() {
    // `main-entry.js` matches none of the default name conventions (app/server, at the root
    // or under src/) — only reading package.json's `"scripts"."start"` (the content
    // channel's own upgrade) can identify it as the real entry.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "main-entry.js",
        "import { router } from './routes.js';\nexport const app = { router };\n",
    );
    write(root, "routes.js", "export const router = {};\n");
    write(
        root,
        "package.json",
        r#"{"dependencies": {"express": "4"}, "scripts": {"start": "node ./main-entry.js"}}"#,
    );

    let result = check(root);
    let unused = unused_paths(&result);
    assert!(
        !unused
            .iter()
            .any(|p| p == "main-entry.js" || p == "routes.js"),
        "package.json's scripts.start should have rescued the manifest-named entry: {unused:?}"
    );
}

/// spring-petclinic's view layer, reduced to the two hops the language graph cannot see: a
/// controller returns the bare string `"welcome"`, the template links its stylesheet through a
/// Thymeleaf link expression, and the stylesheet is itself compiled from Sass by a Maven build
/// plugin. Both `kndo:thymeleaf` and `kndo:libsass-maven-plugin` prove against this one tree
/// because their subjects are the same chain — the CSS is what thymeleaf keeps alive and what
/// libsass links back to its source, and neither is provable without the other's file present.
///
/// `src/main/legacy/old.scss` is the shared control: Sass outside any declared `inputPath`,
/// dead in every run below, in its own directory so it never merges with the fixture's other
/// findings into a rollup.
fn petclinic_view_layer(root: &Path) {
    write(
        root,
        "src/main/java/app/HomeController.java",
        "package app;\n\n         public class HomeController {\n         \x20   public String home() {\n         \x20       return \"welcome\";\n         \x20   }\n         }\n",
    );
    write(
        root,
        "src/main/resources/templates/welcome.html",
        "<!DOCTYPE html>\n         <html xmlns:th=\"http://www.thymeleaf.org\">\n         <head><link rel=\"stylesheet\" th:href=\"@{/resources/css/petclinic.css}\" /></head>\n         <body><h1>hi</h1></body>\n         </html>\n",
    );
    write(
        root,
        "src/main/resources/static/resources/css/petclinic.css",
        ".header { color: #123456; }\n",
    );
    write(
        root,
        "src/main/scss/petclinic.scss",
        "$brand: #123456;\n.header { color: $brand; }\n",
    );
    write(
        root,
        "src/main/legacy/old.scss",
        ".legacy { color: red; }\n",
    );
}

/// The pom, with each of the two declarations independently switchable — because each is the
/// gate of exactly one of the two proofs below, and the file is a single artifact.
fn petclinic_pom(root: &Path, thymeleaf: bool, libsass: bool) {
    let dependency = if thymeleaf {
        "<dependency><groupId>org.springframework.boot</groupId>\
         <artifactId>spring-boot-starter-thymeleaf</artifactId></dependency>"
    } else {
        ""
    };
    // At petclinic's own depth — inside a profile, four levels below where a fixed `<build>`
    // lookup would go — so the fixture exercises the descendant walk the field case needs.
    let build = if libsass {
        "<profiles><profile><id>css</id><build><plugins><plugin>\
           <groupId>com.gitlab.haynes</groupId><artifactId>libsass-maven-plugin</artifactId>\
           <configuration>\
             <inputPath>${basedir}/src/main/scss/</inputPath>\
             <outputPath>${basedir}/src/main/resources/static/resources/css/</outputPath>\
           </configuration>\
         </plugin></plugins></build></profile></profiles>"
    } else {
        ""
    };
    write(
        root,
        "pom.xml",
        &format!(
            "<project><modelVersion>4.0.0</modelVersion><groupId>g</groupId>\
             <artifactId>a</artifactId><version>1</version>\n\
             <dependencies>{dependency}</dependencies>\n{build}\n</project>\n"
        ),
    );
}

#[test]
fn thymeleaf_link_expression_is_gated_by_the_starter() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    petclinic_view_layer(root);

    // No libsass declaration in either run: it also removes the CSS finding, by classifying it
    // build output, and a baseline where two mechanisms can produce the same delta proves
    // neither.
    petclinic_pom(root, false, false);
    let without = check(root);
    assert!(
        unused_paths(&without)
            .contains(&"src/main/resources/static/resources/css/petclinic.css".to_string()),
        "no import reaches a stylesheet a template links: the baseline must fire, or this \
         proof is vacuous: {:?}",
        unused_paths(&without)
    );
    assert!(!active_ids(root).contains(&"kndo:thymeleaf".to_string()));

    petclinic_pom(root, true, false);
    let with = check(root);
    let unused = unused_paths(&with);
    assert!(
        !unused.contains(&"src/main/resources/static/resources/css/petclinic.css".to_string()),
        "the template's link expression resolves to it through Spring Boot's static \
         locations: {unused:?}"
    );
    assert!(
        unused.contains(&"src/main/legacy/old.scss".to_string())
            && unused.contains(&"src/main/scss/petclinic.scss".to_string()),
        "the plugin must not blanket-suppress: what no template links stays reported: \
         {unused:?}"
    );
    assert!(active_ids(root).contains(&"kndo:thymeleaf".to_string()));

    // One root (the template, which a controller names by a bare string no plugin can
    // follow), one edge (the stylesheet its link expression resolves to).
    assert_contributed(&with, "kndo:thymeleaf", 1, 1, 0);
}

#[test]
fn libsass_compilation_is_gated_by_the_pom_declaration() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    petclinic_view_layer(root);

    // Thymeleaf declared in both runs: without it the CSS is dead, and a dead output can keep
    // nothing alive — the link back to the Sass would be true and invisible.
    petclinic_pom(root, true, false);
    let without = check(root);
    assert!(
        unused_paths(&without).contains(&"src/main/scss/petclinic.scss".to_string()),
        "committed Sass is referenced by nothing in the language graph: the baseline must \
         fire, or this proof is vacuous: {:?}",
        unused_paths(&without)
    );
    // The plugin activates on `**/*.scss` regardless of the pom, so activation is NOT the
    // variable here — the declaration is. Asserting that keeps a later reading of this test
    // from mistaking the two.
    assert!(active_ids(root).contains(&"kndo:libsass-maven-plugin".to_string()));
    assert!(
        contribution(&without, "kndo:libsass-maven-plugin").is_none_or(|c| c.edges == 0),
        "with no compilation declared there is nothing to link"
    );

    petclinic_pom(root, true, true);
    let with = check(root);
    let unused = unused_paths(&with);
    assert!(
        !unused.contains(&"src/main/scss/petclinic.scss".to_string()),
        "the stylesheet ships, so the Sass it was compiled from is in use: {unused:?}"
    );
    assert_eq!(
        unused,
        vec!["src/main/legacy/old.scss"],
        "Sass outside the declared inputPath is not this build's source and stays reported"
    );

    // One edge, output → input. No roots: a build plugin makes nothing an entry point, it
    // only says where a file came from — the liveness still has to arrive from the template.
    assert_contributed(&with, "kndo:libsass-maven-plugin", 0, 1, 0);
}

#[test]
fn serde_attribute_paths_are_gated_by_the_manifest() {
    // kndo's own shape, reduced: a predicate named only by `skip_serializing_if`. Its caller
    // is code serde's derive macro generates, which exists in no source file — so plain
    // reachability sees a struct field carrying a string and a function nobody calls. This is
    // exactly the shape `kndo:serde` exists to recognize, drawn from kndo's own codebase.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "src/lib.rs",
        "#[derive(serde::Serialize)]\n         pub struct Envelope {\n         \x20   #[serde(skip_serializing_if = \"usize_is_zero\")]\n         \x20   pub elided: usize,\n         \x20   // A wire label that collides with a real declaration. The adapter records\n         \x20   // this string exactly like the one above; only the plugin's key table tells\n         \x20   // them apart, and getting it wrong here would keep `unrelated` alive.\n         \x20   #[serde(rename = \"unrelated\")]\n         \x20   pub a: u32,\n         }\n         \n         fn usize_is_zero(n: &u32) -> bool {\n    *n == 0\n}\n         \n         fn unrelated() -> u32 {\n    7\n}\n         \n         pub fn build() -> Envelope {\n    Envelope { elided: 0, a: 1 }\n}\n",
    );

    let manifest = "[package]\nname = \"f\"\nversion = \"0.1.0\"\n";
    write(root, "Cargo.toml", manifest);
    let without = check(root);
    let unused_without = findings_of(&without, "unused");
    assert!(
        unused_without.contains(&"src/lib.rs#usize_is_zero".to_string()),
        "a function named only by an attribute string is invisible to the call graph: the \
         baseline must fire, or this proof is vacuous: {unused_without:?}"
    );
    assert!(!active_ids(root).contains(&"kndo:serde".to_string()));

    write(
        root,
        "Cargo.toml",
        &format!("{manifest}\n[dependencies]\nserde = \"1\"\n"),
    );
    let with = check(root);
    let unused_with = findings_of(&with, "unused");
    assert!(
        !unused_with.contains(&"src/lib.rs#usize_is_zero".to_string()),
        "`skip_serializing_if` names a function serde calls: {unused_with:?}"
    );
    // The whole point of the key table, asserted rather than assumed: `rename`'s value is a
    // wire label, and a plugin that treated every attribute string as a path would keep
    // `unrelated` alive here — 72 such collisions in serde's own repo alone, each one a true
    // finding silenced.
    assert!(
        unused_with.contains(&"src/lib.rs#unrelated".to_string()),
        "`rename` names data, not an item — a declaration that merely shares the name stays \
         reported: {unused_with:?}"
    );

    // Exactly one edge: the `skip_serializing_if` value. No roots, no annotations — the
    // fixture's only impl is derived, and a derived impl declares nothing to mark.
    assert_contributed(&with, "kndo:serde", 0, 1, 0);
}

/// Every id proven above, one line per `#[test]`. The list is written out rather than derived
/// so that adding a plugin and adding its proof are two deliberate edits: a derived list would
/// close the gate against itself and prove nothing.
const PROVEN: &[&str] = &[
    "kndo:coverage-lcov",
    "kndo:coverage-cobertura",
    "kndo:coverage-jacoco",
    "kndo:coverage-go",
    "kndo:express",
    "kndo:info-plist",
    "kndo:libsass-maven-plugin",
    "kndo:nextjs",
    "kndo:rkyv",
    "kndo:serde",
    "kndo:thymeleaf",
    "kndo:uikit",
    "kndo:wasmtime",
];

/// The gate this file exists to make invariant: **a built-in plugin without a proof fails the
/// suite**, the same way `Plugin::mutates_graph()` has no default so nobody can forget to
/// decide it.
///
/// Closed in both directions. A missing proof is the failure everyone expects; a stale entry
/// matters just as much, because a `PROVEN` list naming a plugin this build does not ship stops
/// being a statement about this build. Under a partial feature set the built-in set shrinks
/// and only the first direction is meaningful, so the second checks against the ids this
/// build actually contains.
#[test]
fn every_built_in_plugin_is_proven_here() {
    let built_in: Vec<String> = kndo::default_plugins()
        .iter()
        .map(|p| p.descriptor().id.to_string())
        .collect();

    let unproven: Vec<&String> = built_in
        .iter()
        .filter(|id| !PROVEN.contains(&id.as_str()))
        .collect();
    assert!(
        unproven.is_empty(),
        "built-in plugins with no baseline-then-plugin proof in this file: {unproven:?} — \
         docs/src/plugins/authoring.md requires one, and a plugin nothing asserts is a plugin \
         nothing notices breaking. Add the proof, then the id to PROVEN."
    );

    let stale: Vec<&&str> = PROVEN
        .iter()
        .filter(|id| !built_in.iter().any(|b| b == *id))
        .collect();
    assert!(
        stale.is_empty(),
        "PROVEN names plugins this build does not ship: {stale:?} — either the feature is off \
         (run with --all-features) or the entry outlived its plugin"
    );
}

/// Every built-in has a **published spec** and a row linking to it.
///
/// The proof above says the plugin works; this says a user can find out what it does and where
/// its edges stop. `docs/src/plugins/` is the one place those specs live — the pointers across
/// the RFCs and contracts all resolve there, and `docs/src/plugins.md`'s built-ins table is the
/// index. A plugin that ships with neither is one nobody can evaluate before installing it, and
/// the only reliable way to keep that from happening quietly is to fail here.
///
/// The four coverage ingesters share one document: they are one design decided once, and
/// reading them apart is how their shared rules get re-litigated per format.
#[test]
fn every_built_in_plugin_has_a_published_spec() {
    let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/kndo has two ancestors up to the workspace root")
        .join("docs/src");
    let table = std::fs::read_to_string(docs.join("plugins.md")).expect("docs/src/plugins.md");

    let mut undocumented: Vec<String> = Vec::new();
    let mut unlinked: Vec<String> = Vec::new();
    for plugin in kndo::default_plugins() {
        let id = plugin.descriptor().id.to_string();
        let name = id.strip_prefix("kndo:").unwrap_or(&id);
        let spec = if name.starts_with("coverage-") {
            "coverage".to_string()
        } else {
            name.to_string()
        };
        if !docs.join(format!("plugins/{spec}.md")).is_file() {
            undocumented.push(format!("{id} → docs/src/plugins/{spec}.md"));
        }
        // The row must carry the link, not merely mention the id: an unlinked row leaves the
        // spec written and unreachable, which reads to a user exactly like no spec at all.
        if !table.contains(&format!("[`{id}`](plugins/{spec}.md)")) {
            unlinked.push(id);
        }
    }
    assert!(
        undocumented.is_empty(),
        "built-in plugins with no spec under docs/src/plugins/: {undocumented:?}"
    );
    assert!(
        unlinked.is_empty(),
        "built-in plugins whose row in docs/src/plugins.md does not link to their spec: \
         {unlinked:?}"
    );
}
