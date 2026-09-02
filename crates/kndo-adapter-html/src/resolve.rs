//! A root-relative reference (`/src/main.ts`) is the one shape only the
//! document's place in the tree can answer — the server's root is unknown, and
//! the toolkit's nearest-ancestor rule stands in for it. Everything else is
//! JavaScript's to resolve, and the js-ts adapter resolves it: a relative path
//! with the spellings a bundler allows (an extensionless `./main` is
//! `main.ts`), a bare name as a workspace package or an external one.

use kndo_adapter_ts::TypeScriptAdapter;
use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::extension::Extension;
use kndo_contract::vocab::ProjectPath;

pub(crate) fn resolve(
    from: &ProjectPath,
    specifier: &str,
    cx: &ResolveContext<'_>,
    js: &TypeScriptAdapter,
) -> Resolution {
    let path = specifier.split(['?', '#']).next().unwrap_or(specifier);
    if path.starts_with('/') {
        return kndo_toolkit::nearest_rooted_match(from, path, cx)
            .map_or(Resolution::Unresolved, Resolution::File);
    }
    js.resolve(from, specifier, cx)
}
