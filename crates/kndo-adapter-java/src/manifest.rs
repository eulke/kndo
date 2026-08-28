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

    /// Extract `at` in a project where `manifests` maps each pom's path to its text — the
    /// shape inheritance needs, since resolving a `<parent>` means reading a *different* file.
    /// Mirrors what `graph::assemble` wires: every manifest is a known file, and the text
    /// channel answers for it.
    fn maven_facts_in_project(
        at: &str,
        manifests: &[(&str, &str)],
        source_files: &[&str],
    ) -> ManifestFacts {
        let mut paths: Vec<&str> = manifests.iter().map(|(p, _)| *p).collect();
        paths.extend_from_slice(source_files);
        let known = ctx_with(&paths);
        let owned: Vec<(String, String)> = manifests
            .iter()
            .map(|(p, t)| ((*p).to_string(), (*t).to_string()))
            .collect();
        let read = move |path: &ProjectPath| -> Option<String> {
            owned
                .iter()
                .find(|(p, _)| p == path.0.as_str())
                .map(|(_, t)| t.clone())
        };
        let ctx = ResolveCtx::new(&known).with_manifest_text(&read);
        let text = manifests
            .iter()
            .find(|(p, _)| *p == at)
            .expect("the pom under test is in the project")
            .1;
        extract(at, text.as_bytes(), &ctx)
    }

    /// **A module inherits its source directory from its parent pom.**
    ///
    /// guava's shape, reduced: `<sourceDirectory>` is declared once in the parent and every
    /// module inherits it. Reading only each pom's own text, kndo promoted nothing here and
    /// read the whole publishable surface as unreachable — 88% of guava's findings rested on
    /// this single miss.
    #[test]
    fn a_module_inherits_its_source_directory_from_its_parent() {
        let facts = maven_facts_in_project(
            "mod/pom.xml",
            &[
                (
                    "pom.xml",
                    "<project><groupId>g</groupId><artifactId>parent</artifactId>\
                     <packaging>pom</packaging><build><sourceDirectory>java-src</sourceDirectory>\
                     </build></project>",
                ),
                (
                    "mod/pom.xml",
                    "<project><artifactId>mod</artifactId>\
                     <parent><groupId>g</groupId><artifactId>parent</artifactId></parent>\
                     </project>",
                ),
            ],
            &[
                "mod/java-src/com/foo/A.java",
                "mod/src/main/java/com/foo/B.java",
            ],
        );
        let roots: Vec<&str> = facts.roots.iter().map(|r| r.target.0.as_str()).collect();
        assert!(
            roots.contains(&"mod/java-src/com/foo/A.java"),
            "the inherited directory is the module's source root: {roots:?}"
        );
        assert!(
            !roots.contains(&"mod/src/main/java/com/foo/B.java"),
            "the convention is the fallback, and this module is not falling back: {roots:?}"
        );
    }

    /// The inherited value is resolved per-module against the module's own directory — Maven's
    /// rule, and the reason ONE declaration in a parent serves ten modules with ten different
    /// source trees.
    #[test]
    fn an_inherited_source_directory_is_relative_to_each_module() {
        let parent = (
            "pom.xml",
            "<project><groupId>g</groupId><artifactId>parent</artifactId>\
             <packaging>pom</packaging><build><sourceDirectory>src</sourceDirectory></build>\
             </project>",
        );
        let child = |name: &str| {
            format!(
                "<project><artifactId>{name}</artifactId>\
                 <parent><groupId>g</groupId><artifactId>parent</artifactId></parent></project>"
            )
        };
        let a = child("a");
        let b = child("b");
        for (dir, text) in [("a", &a), ("b", &b)] {
            let facts = maven_facts_in_project(
                &format!("{dir}/pom.xml"),
                &[parent, (&format!("{dir}/pom.xml"), text)],
                &["a/src/A.java", "b/src/B.java"],
            );
            let roots: Vec<&str> = facts.roots.iter().map(|r| r.target.0.as_str()).collect();
            assert_eq!(
                roots.len(),
                1,
                "{dir} promotes only its own tree: {roots:?}"
            );
            assert!(roots[0].starts_with(&format!("{dir}/src/")), "{roots:?}");
        }
    }

    /// **A pom at `../pom.xml` whose coordinates are not the declared parent's is not the
    /// parent.** Maven resolves that one from the repository, which kndo never fetches — this
    /// is guava's `futures/*` modules, which name `guava-parent` at a version the in-repo pom
    /// has not carried for years and have no `futures/pom.xml` beside them.
    #[test]
    fn a_pom_with_different_coordinates_is_not_the_parent() {
        let facts = maven_facts_in_project(
            "mod/pom.xml",
            &[
                (
                    "pom.xml",
                    "<project><groupId>g</groupId><artifactId>someone-else</artifactId>\
                     <packaging>pom</packaging><build><sourceDirectory>java-src</sourceDirectory>\
                     </build></project>",
                ),
                (
                    "mod/pom.xml",
                    "<project><artifactId>mod</artifactId>\
                     <parent><groupId>g</groupId><artifactId>parent</artifactId></parent>\
                     </project>",
                ),
            ],
            &[
                "mod/java-src/com/foo/A.java",
                "mod/src/main/java/com/foo/B.java",
            ],
        );
        let roots: Vec<&str> = facts.roots.iter().map(|r| r.target.0.as_str()).collect();
        assert!(
            roots.contains(&"mod/src/main/java/com/foo/B.java"),
            "an unmatched parent means falling back to the convention: {roots:?}"
        );
        assert!(!roots.contains(&"mod/java-src/com/foo/A.java"), "{roots:?}");
    }

    /// An empty `<relativePath/>` is Maven's explicit "this parent comes from the repository" —
    /// the one spelling that must not fall back to `../pom.xml`.
    #[test]
    fn an_empty_relative_path_does_not_read_the_pom_next_door() {
        let facts = maven_facts_in_project(
            "mod/pom.xml",
            &[
                (
                    "pom.xml",
                    "<project><groupId>g</groupId><artifactId>parent</artifactId>\
                     <packaging>pom</packaging><build><sourceDirectory>java-src</sourceDirectory>\
                     </build></project>",
                ),
                (
                    "mod/pom.xml",
                    "<project><artifactId>mod</artifactId><parent><groupId>g</groupId>\
                     <artifactId>parent</artifactId><relativePath/></parent></project>",
                ),
            ],
            &[
                "mod/java-src/com/foo/A.java",
                "mod/src/main/java/com/foo/B.java",
            ],
        );
        let roots: Vec<&str> = facts.roots.iter().map(|r| r.target.0.as_str()).collect();
        assert!(
            roots.contains(&"mod/src/main/java/com/foo/B.java"),
            "{roots:?}"
        );
        assert!(!roots.contains(&"mod/java-src/com/foo/A.java"), "{roots:?}");
    }

    /// The declaration may be further up than one hop, and a pom that declares nothing simply
    /// passes the question along.
    #[test]
    fn the_declaration_may_live_two_levels_up() {
        let facts = maven_facts_in_project(
            "a/b/pom.xml",
            &[
                (
                    "pom.xml",
                    "<project><groupId>g</groupId><artifactId>root</artifactId>\
                     <packaging>pom</packaging><build><sourceDirectory>src</sourceDirectory>\
                     </build></project>",
                ),
                (
                    "a/pom.xml",
                    "<project><artifactId>mid</artifactId><packaging>pom</packaging>\
                     <parent><groupId>g</groupId><artifactId>root</artifactId></parent></project>",
                ),
                (
                    "a/b/pom.xml",
                    "<project><artifactId>leaf</artifactId>\
                     <parent><groupId>g</groupId><artifactId>mid</artifactId></parent></project>",
                ),
            ],
            &["a/b/src/com/foo/A.java"],
        );
        let roots: Vec<&str> = facts.roots.iter().map(|r| r.target.0.as_str()).collect();
        assert_eq!(roots, vec!["a/b/src/com/foo/A.java"]);
    }

    /// A `<relativePath>` cycle is malformed input, not a shape to follow — and must not hang.
    #[test]
    fn a_parent_cycle_terminates() {
        let facts = maven_facts_in_project(
            "a/pom.xml",
            &[
                (
                    "a/pom.xml",
                    "<project><artifactId>a</artifactId><parent><artifactId>b</artifactId>\
                     <relativePath>../b/pom.xml</relativePath></parent></project>",
                ),
                (
                    "b/pom.xml",
                    "<project><artifactId>b</artifactId><parent><artifactId>a</artifactId>\
                     <relativePath>../a/pom.xml</relativePath></parent></project>",
                ),
            ],
            &["a/src/main/java/com/foo/A.java"],
        );
        // It terminates, and falls back to the convention like any pom that declares nothing.
        let roots: Vec<&str> = facts.roots.iter().map(|r| r.target.0.as_str()).collect();
        assert_eq!(roots, vec!["a/src/main/java/com/foo/A.java"]);
    }

    /// Import resolution's context has no manifest-text channel at all. An adapter must read
    /// that as "I cannot see it" and fall back, never as "there is no parent".
    #[test]
    fn without_the_text_channel_inheritance_degrades_to_the_convention() {
        let facts = maven_facts(
            "<project><artifactId>mod</artifactId>\
             <parent><groupId>g</groupId><artifactId>parent</artifactId></parent></project>",
            &["src/main/java/com/foo/A.java"],
        );
        let roots: Vec<&str> = facts.roots.iter().map(|r| r.target.0.as_str()).collect();
        assert_eq!(roots, vec!["src/main/java/com/foo/A.java"]);
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
