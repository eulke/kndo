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
//! renders, and a tree would buy structure nothing reads; an inline module's
//! import statements are the one JavaScript shape read here, by a scan of the
//! statement forms alone. No declarations, no metrics, no comments: a document
//! declares nothing a caller can name, so no symbol-level analysis ever judges
//! one. What it contributes is a root and its edges. Resolution of what it
//! names is JavaScript's — a bundler serves the page — so it is delegated to
//! the js-ts adapter, except for the root-relative shape only the document's
//! place in the tree can answer.

mod extract;
mod resolve;

use kndo_adapter_ts::TypeScriptAdapter;
use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::vocab::ProjectPath;

pub struct HtmlAdapter {
    spec: ExtensionSpec,
    js: TypeScriptAdapter,
}

impl HtmlAdapter {
    pub fn new() -> Self {
        HtmlAdapter {
            // No manifest of its own: a document declares no package. No
            // evidence streams: nothing here carries a pragma or a metric.
            spec: ExtensionSpec::builder("kndo:html", 1)
                .suffixes(&["html", "htm"])
                .build(),
            js: TypeScriptAdapter::new(),
        }
    }
}

impl Default for HtmlAdapter {
    fn default() -> Self {
        HtmlAdapter::new()
    }
}

impl Extension for HtmlAdapter {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        extract::extract(file, out);
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx, &self.js)
    }
}
