//! HTML — a document is an entry point, not a module. Nothing imports a page:
//! a browser loads it, a server renders it, a bundler is handed it. So a
//! document roots itself, and the modules and stylesheets it names become
//! reachable through it — `<script type="module" src="/src/main.ts">` in an
//! `index.html` is how a bundled app names its real entry, and nothing else in
//! the project imports that file. Measured on vite before this adapter
//! existed: 79 of the 676 files reported unused were entries an `index.html`
//! names, and 29 of its 155 pages carry their imports in inline
//! `<script type="module">` bodies — 243 of them, bare package names included.
//!
//! No grammar: every attribute reference lives in one attribute of one tag,
//! HTML's error recovery means a malformed document is still one a browser
//! renders, and a tree would buy structure nothing reads. What a page HOLDS
//! rather than names — an inline `<script>`, an inline `<style>` — is code of
//! another language, and it is reported as an embedded region: the engine
//! hands each to the JavaScript or CSS extension, which reads it as it reads
//! a file, in the page's coordinates. A document itself declares nothing a
//! caller can name, so what this adapter contributes is a root, its
//! attribute edges and its regions. Resolution of an attribute is the URL's
//! own: the path as written, or the root-relative shape only the document's
//! place in the tree can answer; what an inline script imports is
//! JavaScript's to resolve, and the engine routes it there.

mod extract;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::evidence::RootKind;
use kndo_contract::plugin::{FileRole, Plugin, PluginSpec};
use kndo_contract::vocab::ProjectPath;

pub struct HtmlAdapter {
    spec: PluginSpec,
}

impl HtmlAdapter {
    pub fn new() -> Self {
        HtmlAdapter {
            // No manifest of its own: a document declares no package. No
            // markers and no metrics: nothing here carries a pragma or a
            // metric. It DOES declare `UnreadText`, and reports none: a
            // document is read by a scanner that covers all of it, so "no
            // unread text" is a statement this reader can make — which is what
            // the stream is for, and the only way `unused` may judge a page at
            // all.
            spec: PluginSpec::builder("kndo:html", 5)
                .emits(kndo_contract::evidence::EvidenceStreams::of(&[
                    kndo_contract::evidence::EvidenceStream::UnreadText,
                ]))
                .suffixes(&["html", "htm"])
                // A page inside npm's installed dependencies is a dependency's.
                .ignores(&["**/node_modules/**"])
                // A bare specifier in a stylesheet or a page names an npm
                // package: there is no registry of its own to judge it
                // against, and js-ts is the extension that claims the
                // manifests declaring it.
                .ecosystem("kndo:js-ts")
                // A document is an entry point — that is what a page IS, true
                // of every one of them, so it is `Certain` and it is a path
                // fact, not a claim over a manifest that named the file. The
                // test globs are the js-ts adapter's, at the same tier: a
                // test page is BOTH, and taking both is right, because a
                // file the test run alone compiles seeds no production
                // flood whatever colour its root claims.
                .file_roles(&[
                    FileRole::certain("**/*.html", RootKind::Production),
                    FileRole::certain("**/*.htm", RootKind::Production),
                    FileRole::probable("__tests__/**", RootKind::Test),
                    FileRole::probable("**/__tests__/**", RootKind::Test),
                    FileRole::probable("test/**", RootKind::Test),
                    FileRole::probable("**/test/**", RootKind::Test),
                    FileRole::probable("tests/**", RootKind::Test),
                    FileRole::probable("**/tests/**", RootKind::Test),
                    FileRole::probable("**/*.test.*", RootKind::Test),
                    FileRole::probable("**/*.spec.*", RootKind::Test),
                ])
                .build(),
        }
    }
}

impl Default for HtmlAdapter {
    fn default() -> Self {
        HtmlAdapter::new()
    }
}

impl Plugin for HtmlAdapter {
    fn spec(&self) -> &PluginSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        extract::extract(file, out);
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx)
    }
}
