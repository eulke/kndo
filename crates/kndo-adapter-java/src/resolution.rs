//! Import resolution: pure lookups against `ResolveCtx`'s known-
//! files/units index, mirroring javac's own resolution rules — never querying it.
//!
//! Anchors: same-package (no import) is handled entirely by the core's `unit`-based fallback
//! — never reaches this module at all. Every import specifier here is either a
//! plain `pkg` (from `import pkg.Type;`/`import pkg.*;`) or a static-import sentinel
//! `pkg::Cls`.
//!
//! Bare first segment precedence: `java`/`javax` stdlib prefix → known unit (package) → else
//! `Resolution::Unresolved` — deliberately NOT a `Dependency` fallback (no
//! reliable package→Maven/Gradle-coordinate mapping exists, so guessing here would flood
//! `undeclared` with false positives for the overwhelming majority of third-party code).

use kndo_core::adapter::{ImportSpec, Resolution, ResolveCtx};

pub(crate) fn resolve(spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
    let s = spec.specifier.as_str();

    // Static-import sentinel: "pkg::Cls" resolves on the package half only —
    // the member lookup inside the target file is the core's own binding-table job.
    let package = s.split_once("::").map_or(s, |(pkg, _)| pkg);

    if package == "java"
        || package.starts_with("java.")
        || package == "javax"
        || package.starts_with("javax.")
    {
        return Resolution::Stdlib;
    }

    resolve_package(package, ctx)
}

/// A package resolves to the first file (path order) among every claimed `.java` file
/// declaring that `unit` — which one is nominal doesn't affect correctness,
/// since the core's same-unit fallback makes every file in the package individually
/// reachable regardless of which one a wildcard/import edge lands on literally.
fn resolve_package(package: &str, ctx: &ResolveCtx<'_>) -> Resolution {
    let Some(target) = ctx.unit_files(package).first() else {
        return Resolution::Unresolved;
    };
    Resolution::File(target.clone(), kndo_core::vocab::Confidence::Certain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::adapter::{ImportSpec, ProjectPath};
    use kndo_core::vocab::Confidence;
    use rustc_hash::FxHashSet;
    use smol_str::SmolStr;

    fn path(p: &str) -> ProjectPath {
        ProjectPath(SmolStr::new(p))
    }

    fn spec(specifier: &str, from: &str) -> ImportSpec {
        ImportSpec {
            specifier: SmolStr::new(specifier),
            from: path(from),
        }
    }

    #[test]
    fn java_and_javax_prefixes_are_stdlib() {
        let known: FxHashSet<ProjectPath> = FxHashSet::default();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(
            resolve(&spec("java.util", "src/main/java/p/C.java"), &ctx),
            Resolution::Stdlib
        );
        assert_eq!(
            resolve(&spec("javax.swing", "src/main/java/p/C.java"), &ctx),
            Resolution::Stdlib
        );
        assert_eq!(resolve(&spec("java", "p/C.java"), &ctx), Resolution::Stdlib);
    }

    #[test]
    fn unresolved_third_party_never_becomes_a_dependency_edge() {
        // No reliable package->coordinate mapping — must stay Unresolved, never
        // guess a Resolution::Dependency (that would flood `undeclared`).
        let known: FxHashSet<ProjectPath> = FxHashSet::default();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(
            resolve(&spec("com.google.common.collect", "p/C.java"), &ctx),
            Resolution::Unresolved
        );
    }

    #[test]
    fn static_import_sentinel_resolves_on_the_package_half() {
        let known: FxHashSet<ProjectPath> = [path("src/main/java/com/foo/Bar.java")]
            .into_iter()
            .collect();
        let units: rustc_hash::FxHashMap<SmolStr, Vec<ProjectPath>> = [(
            SmolStr::new("com.foo"),
            vec![path("src/main/java/com/foo/Bar.java")],
        )]
        .into_iter()
        .collect();
        let ctx = ResolveCtx::new(&known).with_units(&units);
        assert_eq!(
            resolve(
                &spec("com.foo::Bar", "src/main/java/com/foo/Other.java"),
                &ctx
            ),
            Resolution::File(path("src/main/java/com/foo/Bar.java"), Confidence::Certain)
        );
    }
}
