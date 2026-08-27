//! End-to-end proof of the built-in convention plugins, in a baseline-then-plugin shape:
//! each scenario runs the
//! same code twice through `kndo::open`, once *without* the activating manifest dependency
//! (plugin inactive — the findings the plugin exists to suppress must be present) and once
//! *with* it (plugin active — those findings, and only those, disappear). That proves the
//! activation gate and the contributed facts in one pair of runs, and guards against the
//! blanket-suppression failure mode: unrelated dead code must stay reported in both runs.

use std::path::Path;

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

/// Paths of every `unused` finding (file- and symbol-kind alike) — the surface both plugins
/// exist to clean up.
fn unused_paths(result: &kndo_core::engine::RunResult) -> Vec<String> {
    result
        .findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.path.as_ref().map(|p| p.0.to_string()))
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
