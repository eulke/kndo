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

use kndo_core::adapter::{ImportSpec, ProjectPath, Resolution, ResolveCtx};

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

    resolve_package(package, &spec.from, ctx)
}

/// A package resolves to the first file (path order) among every claimed `.java` file
/// declaring that `unit`, **preferring the importer's own package**. Which one is nominal does
/// not affect reachability — the core's same-unit fallback makes every file in the package
/// individually reachable whichever one an edge lands on — but it does affect the literal edge,
/// which `cyclic` reads as evidence. Two Gradle modules that both declare `package retrofit2;`
/// share one unit key (RFC 0012 §8 keys Java units on the declared package name), so without
/// the preference an import could land in an unrelated sibling module and invent a
/// cross-module cycle that neither module's source supports.
fn resolve_package(package: &str, from: &ProjectPath, ctx: &ResolveCtx<'_>) -> Resolution {
    let Some(target) = ctx.unit_files_from(package, from).first() else {
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
    fn an_import_prefers_a_same_package_target_over_a_sibling_module() {
        // retrofit's shape: `retrofit/` and `android-test/` both declare `package retrofit2;`,
        // so they share one unit key. Path order puts android-test first, so resolving
        // repo-globally handed every intra-module import a cross-module target — a phantom
        // edge `cyclic` then reported as a package cycle neither module's source supports.
        let android = path("android-test/src/test/java/retrofit2/BasicCallTest.java");
        let core = path("retrofit/src/main/java/retrofit2/Retrofit.java");
        let importer = path("retrofit/src/main/java/retrofit2/HttpServiceMethod.java");
        let known: FxHashSet<ProjectPath> = [android.clone(), core.clone(), importer.clone()]
            .into_iter()
            .collect();

        let units: rustc_hash::FxHashMap<SmolStr, Vec<ProjectPath>> = [(
            SmolStr::new("retrofit2"),
            vec![android.clone(), core.clone(), importer.clone()],
        )]
        .into_iter()
        .collect();
        let by_package: rustc_hash::FxHashMap<
            u32,
            rustc_hash::FxHashMap<SmolStr, Vec<ProjectPath>>,
        > = [
            (
                1u32,
                [(SmolStr::new("retrofit2"), vec![android.clone()])]
                    .into_iter()
                    .collect(),
            ),
            (
                2u32,
                [(
                    SmolStr::new("retrofit2"),
                    vec![core.clone(), importer.clone()],
                )]
                .into_iter()
                .collect(),
            ),
        ]
        .into_iter()
        .collect();
        let file_package: rustc_hash::FxHashMap<ProjectPath, u32> = [
            (android.clone(), 1u32),
            (core.clone(), 2u32),
            (importer.clone(), 2u32),
        ]
        .into_iter()
        .collect();

        let ctx = ResolveCtx::new(&known)
            .with_units(&units)
            .with_package_units(&by_package, &file_package);
        assert_eq!(
            resolve(&spec("retrofit2", importer.0.as_str()), &ctx),
            Resolution::File(core, Confidence::Certain),
            "an import must land in the importer's own module, not a same-named sibling"
        );

        // A package that genuinely lives only in a sibling module still resolves: the
        // preference falls back rather than refusing, so real cross-module imports survive.
        let only_elsewhere: rustc_hash::FxHashMap<SmolStr, Vec<ProjectPath>> =
            [(SmolStr::new("retrofit2.other"), vec![android.clone()])]
                .into_iter()
                .collect();
        let ctx = ResolveCtx::new(&known)
            .with_units(&only_elsewhere)
            .with_package_units(&by_package, &file_package);
        assert_eq!(
            resolve(&spec("retrofit2.other", importer.0.as_str()), &ctx),
            Resolution::File(android, Confidence::Certain)
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
