//! Manifest extraction (docs/adapters/java.md §4): a thin Java-specific layout over
//! `kndo_adapter_toolkit::jvm_manifest`, which owns every Maven/Gradle fidelity detail —
//! shared verbatim with the Kotlin adapter (ROADMAP "Java → Kotlin share infra"), since a
//! `pom.xml`/`build.gradle`'s shape has zero dependency on which JVM language its module
//! compiles. Java contributes only its source-root convention and the two marker files
//! (`module-info.java`, `package-info.java`) that declare nothing promotable.

use kndo_adapter_toolkit::jvm_manifest::{self, JvmSourceLayout};
use kndo_core::adapter::{ManifestFacts, ResolveCtx};

const LAYOUT: JvmSourceLayout = JvmSourceLayout {
    source_roots: &["src/main/java"],
    source_ext: ".java",
    skip_file_names: &["module-info.java", "package-info.java"],
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

    fn maven_facts(content: &str, files: &[&str]) -> ManifestFacts {
        let known = ctx_with(files);
        let ctx = ResolveCtx::new(&known);
        extract("pom.xml", content.as_bytes(), &ctx)
    }

    #[test]
    fn identity_scopes_and_parent_group_fallback() {
        let f = maven_facts(
            r#"<project>
                 <parent><groupId>com.foo</groupId></parent>
                 <artifactId>bar</artifactId>
                 <dependencies>
                   <dependency><groupId>com.other</groupId><artifactId>lib</artifactId><version>1.0</version></dependency>
                   <dependency><groupId>com.other</groupId><artifactId>testlib</artifactId><version>2.0</version><scope>test</scope></dependency>
                   <dependency><groupId>com.other</groupId><artifactId>servlet</artifactId><version>3.0</version><scope>provided</scope></dependency>
                 </dependencies>
               </project>"#,
            &[],
        );
        assert_eq!(f.package_name.as_deref(), Some("com.foo:bar"));
        let dep = |n: &str| f.dependencies.iter().find(|d| d.name == n).unwrap();
        assert_eq!(dep("com.other:lib").scope, DependencyScope::Prod);
        assert_eq!(dep("com.other:testlib").scope, DependencyScope::Dev);
        assert_eq!(dep("com.other:servlet").scope, DependencyScope::Peer);
    }

    #[test]
    fn packaging_pom_and_war_are_private_jar_is_not() {
        let f = maven_facts("<project><packaging>pom</packaging></project>", &[]);
        assert!(f.private);
        let f = maven_facts("<project><packaging>war</packaging></project>", &[]);
        assert!(f.private);
        let f = maven_facts("<project><packaging>jar</packaging></project>", &[]);
        assert!(!f.private);
        let f = maven_facts("<project></project>", &[]);
        assert!(!f.private, "jar is the default packaging");
    }

    #[test]
    fn modules_are_workspace_members() {
        let f = maven_facts(
            "<project><packaging>pom</packaging><modules><module>sub-a</module><module>sub-b</module></modules></project>",
            &[],
        );
        assert_eq!(f.workspace_members, vec!["sub-a", "sub-b"]);
    }

    #[test]
    fn dependency_management_entries_are_not_collected() {
        let f = maven_facts(
            r#"<project><dependencyManagement><dependencies>
                 <dependency><groupId>com.other</groupId><artifactId>bom</artifactId><version>1.0</version></dependency>
               </dependencies></dependencyManagement></project>"#,
            &[],
        );
        assert!(f.dependencies.is_empty());
    }

    #[test]
    fn publishable_module_roots_every_source_file() {
        let f = maven_facts(
            "<project><artifactId>demo</artifactId></project>",
            &[
                "src/main/java/com/foo/A.java",
                "src/main/java/com/foo/bar/B.java",
                "src/main/java/module-info.java",
                "src/test/java/com/foo/ATest.java",
            ],
        );
        let roots: Vec<&str> = f.roots.iter().map(|r| r.target.0.as_str()).collect();
        assert!(roots.contains(&"src/main/java/com/foo/A.java"));
        assert!(roots.contains(&"src/main/java/com/foo/bar/B.java"));
        assert!(!roots.contains(&"src/main/java/module-info.java"));
        assert!(!roots.contains(&"src/test/java/com/foo/ATest.java"));
    }

    #[test]
    fn private_module_gets_no_source_roots() {
        let f = maven_facts(
            "<project><packaging>war</packaging></project>",
            &["src/main/java/com/foo/A.java"],
        );
        assert!(f.roots.is_empty());
    }

    fn gradle_facts(content: &str, files: &[&str]) -> ManifestFacts {
        let known = ctx_with(files);
        let ctx = ResolveCtx::new(&known);
        extract("build.gradle", content.as_bytes(), &ctx)
    }

    #[test]
    fn gradle_dependencies_line_scan() {
        let f = gradle_facts(
            "group = 'com.foo'\n\
             dependencies {\n\
             \x20   implementation 'com.other:lib:1.0'\n\
             \x20   testImplementation(\"com.other:testlib:2.0\")\n\
             \x20   compileOnly 'com.other:servlet:3.0'\n\
             \x20   annotationProcessor 'com.other:proc:1.0'\n\
             }\n",
            &[],
        );
        let dep = |n: &str| f.dependencies.iter().find(|d| d.name == n).unwrap();
        assert_eq!(dep("com.other:lib").scope, DependencyScope::Prod);
        assert_eq!(dep("com.other:testlib").scope, DependencyScope::Dev);
        assert_eq!(dep("com.other:servlet").scope, DependencyScope::Peer);
        assert_eq!(dep("com.other:proc").scope, DependencyScope::Build);
    }

    #[test]
    fn gradle_application_plugin_is_private() {
        let f = gradle_facts("plugins {\n    id 'application'\n}\n", &[]);
        assert!(f.private);
        let f = gradle_facts("plugins {\n    id 'java-library'\n}\n", &[]);
        assert!(!f.private);
    }

    #[test]
    fn gradle_computed_dependency_is_invisible_not_misparsed() {
        let f = gradle_facts("dependencies {\n    implementation libs.foo\n}\n", &[]);
        assert!(f.dependencies.is_empty());
    }

    #[test]
    fn settings_gradle_include_is_workspace_topology() {
        let known = ctx_with(&[]);
        let ctx = ResolveCtx::new(&known);
        let f = extract(
            "settings.gradle",
            b"include ':sub-a'\ninclude(':sub-b:deep')\n",
            &ctx,
        );
        assert_eq!(f.workspace_members, vec!["sub-a", "sub-b/deep"]);
    }
}
