//! Import resolution: pure lookups against `ResolveCtx`'s known-
//! files/units index — same algorithm as the Java adapter's (crates/kndo-adapter-java/src/
//! resolution.rs), extended with a `kotlin.`/`kotlin/` stdlib-prefix check alongside `java.`/
//! `javax.` (every Kotlin/JVM project transitively depends on the full Java standard library
//! too, so both prefixes resolve the same way).

use kndo_core::adapter::{ImportSpec, Resolution, ResolveCtx};

pub(crate) fn resolve(spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
    let package = spec.specifier.as_str();

    if is_stdlib_prefix(package) {
        return Resolution::Stdlib;
    }

    let Some(target) = ctx.unit_files_from(package, &spec.from).first() else {
        return Resolution::Unresolved;
    };
    Resolution::File(target.clone(), kndo_core::vocab::Confidence::Certain)
}

fn is_stdlib_prefix(package: &str) -> bool {
    const PREFIXES: &[&str] = &["kotlin", "java", "javax"];
    PREFIXES
        .iter()
        .any(|p| package == *p || package.starts_with(&format!("{p}.")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::adapter::ProjectPath;
    use rustc_hash::FxHashSet;
    use smol_str::SmolStr;

    fn path(p: &str) -> ProjectPath {
        ProjectPath(SmolStr::new(p))
    }

    fn spec(specifier: &str) -> ImportSpec {
        ImportSpec {
            specifier: SmolStr::new(specifier),
            from: path("src/main/kotlin/p/C.kt"),
        }
    }

    #[test]
    fn kotlin_java_and_javax_prefixes_are_stdlib() {
        let known: FxHashSet<ProjectPath> = FxHashSet::default();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(
            resolve(&spec("kotlin.collections"), &ctx),
            Resolution::Stdlib
        );
        assert_eq!(resolve(&spec("java.util"), &ctx), Resolution::Stdlib);
        assert_eq!(resolve(&spec("javax.swing"), &ctx), Resolution::Stdlib);
        assert_eq!(resolve(&spec("kotlin"), &ctx), Resolution::Stdlib);
    }

    #[test]
    fn unresolved_third_party_never_becomes_a_dependency_edge() {
        let known: FxHashSet<ProjectPath> = FxHashSet::default();
        let ctx = ResolveCtx::new(&known);
        assert_eq!(
            resolve(&spec("com.google.common.collect"), &ctx),
            Resolution::Unresolved
        );
    }

    #[test]
    fn known_package_resolves_to_its_first_file() {
        let known: FxHashSet<ProjectPath> = [path("src/main/kotlin/com/foo/Bar.kt")]
            .into_iter()
            .collect();
        let units: rustc_hash::FxHashMap<SmolStr, Vec<ProjectPath>> = [(
            SmolStr::new("com.foo"),
            vec![path("src/main/kotlin/com/foo/Bar.kt")],
        )]
        .into_iter()
        .collect();
        let ctx = ResolveCtx::new(&known).with_units(&units);
        assert_eq!(
            resolve(&spec("com.foo"), &ctx),
            Resolution::File(
                path("src/main/kotlin/com/foo/Bar.kt"),
                kndo_core::vocab::Confidence::Certain
            )
        );
    }
}
