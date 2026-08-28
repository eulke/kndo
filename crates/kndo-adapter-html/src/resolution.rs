//! Resolving what a document names. HTML has no module algorithm — a browser fetches the path
//! as written, relative to the document — so this is path arithmetic and a lookup, with no
//! candidate list and no extension guessing.

use kndo_core::adapter::{ImportSpec, ProjectPath, Resolution, ResolveCtx};
use kndo_core::vocab::Confidence;
use smol_str::SmolStr;

pub(crate) fn resolve(spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
    let joined = kndo_adapter_toolkit::paths::join(
        kndo_adapter_toolkit::paths::dirname(spec.from.0.as_str()),
        spec.specifier.as_str(),
    );
    let path = ProjectPath(SmolStr::new(joined));
    if ctx.contains(&path) {
        return Resolution::File(path, Confidence::Certain);
    }
    // A path the project does not contain is a build artifact (`./dist/bundle.js`), a
    // server-generated asset, or a typo — and this adapter cannot tell which. `Unresolved`
    // rather than a `Dependency` guess: an HTML reference never names a package.
    Resolution::Unresolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;

    fn resolve_from(from: &str, specifier: &str, known: &[&str]) -> Resolution {
        let set: FxHashSet<ProjectPath> = known
            .iter()
            .map(|p| ProjectPath(SmolStr::new(*p)))
            .collect();
        let ctx = ResolveCtx::new(&set);
        resolve(
            &ImportSpec {
                specifier: SmolStr::new(specifier),
                from: ProjectPath(SmolStr::new(from)),
            },
            &ctx,
        )
    }

    #[test]
    fn a_sibling_module_resolves() {
        assert_eq!(
            resolve_from("app/index.html", "./main.js", &["app/main.js"]),
            Resolution::File(
                ProjectPath(SmolStr::new("app/main.js")),
                Confidence::Certain
            )
        );
    }

    #[test]
    fn a_parent_relative_path_resolves() {
        assert_eq!(
            resolve_from(
                "app/pages/index.html",
                "../shared/a.js",
                &["app/shared/a.js"]
            ),
            Resolution::File(
                ProjectPath(SmolStr::new("app/shared/a.js")),
                Confidence::Certain
            )
        );
    }

    /// No extension guessing: HTML means the path it writes. `./main` is not `./main.js`,
    /// because a browser would not fetch the latter either.
    #[test]
    fn an_extensionless_specifier_is_not_guessed_at() {
        assert_eq!(
            resolve_from("index.html", "./main", &["main.js"]),
            Resolution::Unresolved
        );
    }

    /// A build artifact the checkout does not contain resolves to nothing, and to nothing in
    /// particular — never a package guess, since an HTML reference cannot name one.
    #[test]
    fn a_path_outside_the_project_is_unresolved() {
        assert_eq!(
            resolve_from("index.html", "./dist/bundle.js", &["src/main.js"]),
            Resolution::Unresolved
        );
    }
}
