//! CSS and SCSS — a non-source territory, deliberately narrow: file claiming
//! and the `@import`/`@use`/`@forward` graph, nothing else. Measured on vite
//! against v1's oracle: every one of v1's 137 findings on stylesheets was
//! file-level (an unreferenced sheet, a sheet only tests reach, a broken
//! import); its custom-property, variable and mixin declarations produced not
//! one, so no symbol is extracted here — a selector or a token is nothing the
//! graph can show a consumer of. A stylesheet nothing references is dead
//! exactly like a module nothing imports, and that verdict is what this
//! adapter makes possible.
//!
//! A bare specifier (`@use "tokens"`, `@import "tailwindcss"`) is a package by
//! declaration and a sibling by resolution: extraction spells it as a package
//! (under a bundler that is what it names — vite's stylesheets say so sixteen
//! times over), and resolution tries the sibling spellings Sass allows before
//! leaving it external.

mod extract;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{EvidenceSink, EvidenceStream, EvidenceStreams};
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_contract::vocab::ProjectPath;

pub struct CssAdapter {
    spec: ExtensionSpec,
}

impl CssAdapter {
    pub fn new() -> Self {
        CssAdapter {
            // Comments for suppression pragmas; no metrics, since nothing
            // declared carries a body. No manifest of its own. 2: the
            // generated banner is reported, never concluded.
            spec: ExtensionSpec::builder("kndo:css", 2)
                .suffixes(&["css", "scss"])
                .emits(EvidenceStreams::of(&[
                    EvidenceStream::Comments,
                    EvidenceStream::Markers,
                ]))
                .dispatch(vec![kndo_toolkit::generated_rule()])
                // A sheet inside npm's installed dependencies is a dependency's.
                .ignores(&["**/node_modules/**"])
                // A bare specifier in a stylesheet or a page names an npm
                // package: there is no registry of its own to judge it
                // against, and js-ts is the extension that claims the
                // manifests declaring it.
                .ecosystem("kndo:js-ts")
                .build(),
        }
    }
}

impl Default for CssAdapter {
    fn default() -> Self {
        CssAdapter::new()
    }
}

impl Extension for CssAdapter {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        extract::extract(file, out);
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx)
    }
}
