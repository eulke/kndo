//! Manifest extraction (docs/adapters/kotlin.md §4): a thin Kotlin-specific layout over
//! `kndo_adapter_toolkit::jvm_manifest`, shared verbatim with the Java adapter — see
//! docs/adapters/java.md §4 for the full Maven/Gradle fidelity description; nothing here
//! diverges except the source-root convention.

use kndo_adapter_toolkit::jvm_manifest::{self, JvmSourceLayout};
use kndo_core::adapter::{ManifestFacts, ResolveCtx};

const LAYOUT: JvmSourceLayout = JvmSourceLayout {
    source_root: "src/main/kotlin",
    source_ext: ".kt",
    skip_file_names: &[],
};

pub(crate) fn extract(path: &str, content: &[u8], ctx: &ResolveCtx<'_>) -> ManifestFacts {
    jvm_manifest::extract(path, content, ctx, &LAYOUT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::adapter::ProjectPath;
    use kndo_core::vocab::DependencyScope;
    use rustc_hash::FxHashSet;
    use smol_str::SmolStr;

    fn ctx_with(files: &[&str]) -> FxHashSet<ProjectPath> {
        files
            .iter()
            .map(|f| ProjectPath(SmolStr::new(*f)))
            .collect()
    }

    #[test]
    fn publishable_module_roots_every_kotlin_source_file() {
        let known = ctx_with(&[
            "src/main/kotlin/com/foo/A.kt",
            "src/main/kotlin/com/foo/bar/B.kt",
            "src/test/kotlin/com/foo/ATest.kt",
        ]);
        let ctx = ResolveCtx::new(&known);
        let f = extract(
            "pom.xml",
            b"<project><artifactId>demo</artifactId></project>",
            &ctx,
        );
        let roots: Vec<&str> = f.roots.iter().map(|r| r.target.0.as_str()).collect();
        assert!(roots.contains(&"src/main/kotlin/com/foo/A.kt"));
        assert!(roots.contains(&"src/main/kotlin/com/foo/bar/B.kt"));
        assert!(!roots.contains(&"src/test/kotlin/com/foo/ATest.kt"));
    }

    #[test]
    fn kapt_configuration_is_a_build_scope_dependency() {
        let known = ctx_with(&[]);
        let ctx = ResolveCtx::new(&known);
        let f = extract(
            "build.gradle",
            b"dependencies {\n    kapt 'com.other:processor:1.0'\n}\n",
            &ctx,
        );
        let dep = f
            .dependencies
            .iter()
            .find(|d| d.name == "com.other:processor")
            .unwrap();
        assert_eq!(dep.scope, DependencyScope::Build);
    }
}
