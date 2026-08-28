//! Manifest extraction: a thin Java-specific layout over
//! `kndo_adapter_toolkit::jvm_manifest`, which owns every Maven/Gradle fidelity detail —
//! shared verbatim with the Kotlin adapter, since a
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
    fn a_declared_source_directory_wins_over_the_convention() {
        // guava's shape: `<sourceDirectory>src</sourceDirectory>`, tests in a sibling `test`.
        // Against the hardcoded `src/main/java` this module promoted nothing, so every public
        // class in a publishable library read as `unused` — 9,576 of them on guava.
        let f = maven_facts(
            "<project><build><sourceDirectory>src</sourceDirectory></build></project>",
            &["src/com/foo/A.java", "test/com/foo/ATest.java"],
        );
        let roots: Vec<&str> = f.roots.iter().map(|r| r.target.0.as_str()).collect();
        assert_eq!(roots, vec!["src/com/foo/A.java"]);
    }

    #[test]
    fn a_declared_source_directory_resolves_basedir_and_properties() {
        let f = maven_facts(
            "<project><properties><lay>sources</lay></properties>\
             <build><sourceDirectory>${basedir}/${lay}</sourceDirectory></build></project>",
            &[
                "sources/com/foo/A.java",
                "src/main/java/com/foo/Ignored.java",
            ],
        );
        let roots: Vec<&str> = f.roots.iter().map(|r| r.target.0.as_str()).collect();
        // The declaration replaces the convention rather than adding to it: a module that
        // says where its code is is not also keeping the default.
        assert_eq!(roots, vec!["sources/com/foo/A.java"]);
    }

    #[test]
    fn an_unresolvable_source_directory_falls_back_to_the_convention() {
        // `${project.build.directory}` depends on a build kndo never runs. Falling back to the
        // convention is the honest outcome; guessing a path is not.
        let f = maven_facts(
            "<project><build><sourceDirectory>${project.build.directory}/gen</sourceDirectory>\
             </build></project>",
            &["src/main/java/com/foo/A.java"],
        );
        let roots: Vec<&str> = f.roots.iter().map(|r| r.target.0.as_str()).collect();
        assert_eq!(roots, vec!["src/main/java/com/foo/A.java"]);
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
    fn a_bom_managed_coordinate_declares_no_version() {
        // Two segments, not three: the version comes from an imported BOM. Splitting on the
        // LAST colon read `spring-boot-starter-actuator` as a version of the group id
        // `org.springframework.boot` — which is how `version-skew` came to report artifact ids
        // as diverging versions on every JVM repository the field audit covered.
        let f = gradle_facts(
            "dependencies {\n\
             \x20   implementation 'org.springframework.boot:spring-boot-starter-actuator'\n\
             \x20   implementation 'org.springframework.boot:spring-boot-docker-compose'\n\
             \x20   implementation 'com.other:pinned:1.0'\n\
             }\n",
            &[],
        );
        let names: Vec<&str> = f.dependencies.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "org.springframework.boot:spring-boot-starter-actuator",
                "org.springframework.boot:spring-boot-docker-compose",
                "com.other:pinned",
            ],
            "the whole two-segment coordinate is the identity"
        );
        assert!(
            f.dependencies[..2].iter().all(|d| d.version_req.is_none()),
            "no version is stated, and that is not the same as `*`"
        );
        assert_eq!(f.dependencies[2].version_req.as_deref(), Some("1.0"));
    }

    #[test]
    fn a_gradle_version_variable_resolves_from_the_same_file() {
        let f = gradle_facts(
            "val jmhVersion = \"1.36\"\n\
             dependencies {\n\
             \x20   implementation \"org.openjdk.jmh:jmh-core:$jmhVersion\"\n\
             \x20   implementation \"org.other:thing:${'$'}{catalogOnly}\"\n\
             }\n",
            &[],
        );
        let dep = |n: &str| f.dependencies.iter().find(|d| d.name == n).unwrap();
        assert_eq!(
            dep("org.openjdk.jmh:jmh-core").version_req.as_deref(),
            Some("1.36"),
            "koin declares this literal two lines above the dependency; comparing \
             `$jmhVersion` against another module's hardcoded `1.36` was pure noise"
        );
        assert_eq!(
            dep("org.other:thing").version_req,
            None,
            "a key that lives outside this manifest (gradle.properties, a version catalog) \
             stays unknown — never the literal, which would compare as a version"
        );
    }

    #[test]
    fn a_maven_property_resolves_and_an_absent_version_is_unknown() {
        let f = maven_facts(
            r#"<project>
                 <properties><spring.version>5.3.0</spring.version></properties>
                 <dependencies>
                   <dependency><groupId>org.springframework</groupId><artifactId>core</artifactId><version>${spring.version}</version></dependency>
                   <dependency><groupId>org.springframework</groupId><artifactId>web</artifactId><version>${parent.only}</version></dependency>
                   <dependency><groupId>org.springframework.boot</groupId><artifactId>starter</artifactId></dependency>
                 </dependencies>
               </project>"#,
            &[],
        );
        let dep = |n: &str| f.dependencies.iter().find(|d| d.name == n).unwrap();
        assert_eq!(
            dep("org.springframework:core").version_req.as_deref(),
            Some("5.3.0")
        );
        assert_eq!(
            dep("org.springframework:web").version_req,
            None,
            "a property a PARENT pom declares is out of reach by construction — kndo never \
             resolves the classpath — so the requirement is unknown, not the placeholder text"
        );
        assert_eq!(
            dep("org.springframework.boot:starter").version_req,
            None,
            "no <version> at all is the BOM-managed shape"
        );
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
