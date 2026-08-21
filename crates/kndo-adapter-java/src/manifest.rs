//! Manifest extraction (docs/adapters/java.md §4): Maven `pom.xml` fully structured via
//! `roxmltree`, Gradle `build.gradle`/`.kts` + `settings.gradle`/`.kts` best-effort line-
//! scanned — Gradle's actual grammar is a real programming language (Groovy/Kotlin), so only
//! literal-string forms are recognized; anything computed is silently invisible, never
//! misparsed.

use kndo_core::adapter::{
    Diagnostic, DiagnosticLevel, ManifestDependency, ManifestFacts, ManifestRoot, ProjectPath,
    ResolveCtx,
};
use kndo_core::vocab::{Confidence, DependencyScope, RootKind};
use smol_str::SmolStr;

pub(crate) fn extract(path: &str, content: &[u8], ctx: &ResolveCtx<'_>) -> ManifestFacts {
    let Ok(text) = std::str::from_utf8(content) else {
        let mut out = ManifestFacts::default();
        out.diagnostics.push(diag("manifest is not valid UTF-8"));
        return out;
    };
    if path.rsplit('/').next() == Some("pom.xml") {
        extract_maven(path, text, ctx)
    } else if matches!(
        path.rsplit('/').next(),
        Some("settings.gradle" | "settings.gradle.kts")
    ) {
        extract_gradle_settings(text)
    } else {
        extract_gradle_build(path, text, ctx)
    }
}

// ---------------------------------------------------------------- Maven (pom.xml)

fn extract_maven(path: &str, text: &str, ctx: &ResolveCtx<'_>) -> ManifestFacts {
    let mut out = ManifestFacts::default();
    let doc = match roxmltree::Document::parse(text) {
        Ok(d) => d,
        Err(e) => {
            out.diagnostics
                .push(diag(&format!("pom.xml parse error: {e}")));
            return out;
        }
    };
    let project = doc.root_element();

    out.package_name = maven_package_name(project);
    out.private = maven_is_private(project);
    out.workspace_members = maven_workspace_members(project);
    // `<dependencyManagement>` entries are version pins for CHILDREN, not real dependencies of
    // this module — `collect_maven_deps` only ever sees a plain `<dependencies>` block, never
    // collected (spec §4).
    collect_maven_deps(xml_child(project, "dependencies"), &mut out);

    // Only packages (non-virtual poms) get source-tree root promotion; a pure-aggregator
    // `<packaging>pom</packaging>` with no `src/main/java` contributes topology only.
    if !out.private {
        promote_source_roots(&maven_source_root(path), ctx, &mut out);
    }
    out
}

fn maven_source_root(path: &str) -> ProjectPath {
    let dir = kndo_adapter_toolkit::paths::dirname(path);
    if dir.is_empty() {
        ProjectPath(SmolStr::new("src/main/java"))
    } else {
        ProjectPath(SmolStr::new(format!("{dir}/src/main/java")))
    }
}

/// The two-part coordinate IS the cross-module identity (spec §4) — `groupId:artifactId`,
/// falling back to `<parent><groupId>` when the child inherits it (the common pattern). No
/// `artifactId` means no identity at all.
fn maven_package_name(project: roxmltree::Node<'_, '_>) -> Option<SmolStr> {
    let artifact_id = xml_child_text(project, "artifactId")?;
    let group_id = xml_child_text(project, "groupId")
        .or_else(|| xml_child(project, "parent").and_then(|p| xml_child_text(p, "groupId")));
    Some(match group_id {
        Some(g) => SmolStr::new(format!("{g}:{artifact_id}")),
        None => SmolStr::new(artifact_id),
    })
}

/// `<packaging>` ≠ jar (default) → app/aggregator mode (spec §4): Maven has no `private` flag,
/// so this is the packaging-based proxy — `pom` (aggregator, no code) and `war` (deployable
/// app, not an importable dependency) are the two treated as private.
fn maven_is_private(project: roxmltree::Node<'_, '_>) -> bool {
    matches!(xml_child_text(project, "packaging"), Some("pom" | "war"))
}

/// `<modules><module>sub-a</module></modules>` — RFC 0011 §3 workspace topology.
fn maven_workspace_members(project: roxmltree::Node<'_, '_>) -> Vec<SmolStr> {
    let Some(modules) = xml_child(project, "modules") else {
        return Vec::new();
    };
    modules
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "module")
        .filter_map(|n| n.text())
        .map(|t| SmolStr::new(t.trim()))
        .collect()
}

fn xml_child<'a, 'i>(node: roxmltree::Node<'a, 'i>, tag: &str) -> Option<roxmltree::Node<'a, 'i>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == tag)
}

fn xml_child_text<'a>(node: roxmltree::Node<'a, '_>, tag: &str) -> Option<&'a str> {
    xml_child(node, tag).and_then(|n| n.text()).map(str::trim)
}

fn collect_maven_deps(deps: Option<roxmltree::Node<'_, '_>>, out: &mut ManifestFacts) {
    let Some(deps) = deps else {
        return;
    };
    let dependency_nodes = deps
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "dependency");
    for dep in dependency_nodes {
        if let Some(dependency) = maven_dependency(dep) {
            out.dependencies.push(dependency);
        }
    }
}

fn maven_dependency(dep: roxmltree::Node<'_, '_>) -> Option<ManifestDependency> {
    let artifact_id = xml_child_text(dep, "artifactId")?;
    let group_id = xml_child_text(dep, "groupId").unwrap_or("");
    let name = if group_id.is_empty() {
        artifact_id.to_string()
    } else {
        format!("{group_id}:{artifact_id}")
    };
    Some(ManifestDependency {
        name: SmolStr::new(name),
        version_req: SmolStr::new(xml_child_text(dep, "version").unwrap_or("*")),
        scope: maven_dependency_scope(xml_child_text(dep, "scope")),
    })
}

/// "provided": supplied by the runtime environment, not bundled — same contract-with-the-
/// consumer semantics as npm's peerDependencies (spec §4). "runtime"/"system"/"compile"/absent
/// are a documented approximation to `Prod` — genuinely used, just not compile-visible; kndo's
/// taxonomy has no runtime-only scope (spec §4).
fn maven_dependency_scope(scope: Option<&str>) -> DependencyScope {
    match scope {
        Some("test") => DependencyScope::Dev,
        Some("provided") => DependencyScope::Peer,
        _ => DependencyScope::Prod,
    }
}

// ---------------------------------------------------------------- Gradle

/// `settings.gradle`/`.kts`: `include(':sub-a')` / `include 'sub-a'` line-scanned for
/// workspace topology only (spec §4) — the Gradle analogue of Maven's `<modules>`.
fn extract_gradle_settings(text: &str) -> ManifestFacts {
    let mut out = ManifestFacts::default();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("include") else {
            continue;
        };
        for lit in string_literals(rest) {
            // ":a:b" → "a/b" (Gradle's colon-path convention).
            let path = lit.trim_start_matches(':').replace(':', "/");
            if !path.is_empty() {
                out.workspace_members.push(SmolStr::new(path));
            }
        }
    }
    out
}

/// `build.gradle`/`.kts`: best-effort literal-string scan of the `dependencies { … }` block
/// (spec §4) — computed/interpolated/version-catalog coordinates are invisible, never
/// misparsed. `apply plugin: 'application'` / `plugins { application }` marks app mode.
fn extract_gradle_build(path: &str, text: &str, ctx: &ResolveCtx<'_>) -> ManifestFacts {
    let private = text
        .lines()
        .any(|l| l.contains("'application'") || l.contains("\"application\""));
    let mut out = ManifestFacts {
        private,
        package_name: gradle_package_name(path, text),
        dependencies: gradle_dependencies(text),
        ..ManifestFacts::default()
    };

    if !out.private {
        let dir = kndo_adapter_toolkit::paths::dirname(path);
        let src_root = if dir.is_empty() {
            ProjectPath(SmolStr::new("src/main/java"))
        } else {
            ProjectPath(SmolStr::new(format!("{dir}/src/main/java")))
        };
        promote_source_roots(&src_root, ctx, &mut out);
    }
    out
}

/// `group = 'com.foo'` (or `group 'com.foo'`, top-level, outside any block) combined with the
/// containing directory's name (Gradle's own project-name-is-the-dirname default).
fn gradle_package_name(path: &str, text: &str) -> Option<SmolStr> {
    let group = gradle_group_literal(text);
    let project_name = kndo_adapter_toolkit::paths::dirname(path)
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty());
    match (group.as_deref(), project_name) {
        (Some(g), Some(n)) => Some(SmolStr::new(format!("{g}:{n}"))),
        (None, Some(n)) => Some(SmolStr::new(n)),
        (Some(g), None) => Some(SmolStr::new(g)),
        (None, None) => None,
    }
}

fn gradle_group_literal(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let l = l.trim();
        let rest = l.strip_prefix("group")?;
        let rest = rest.trim_start().strip_prefix('=').unwrap_or(rest).trim();
        string_literals(rest).into_iter().next()
    })
}

/// Every `configuration "group:artifact:version"` literal inside the (brace-depth-tracked)
/// `dependencies { … }` block.
fn gradle_dependencies(text: &str) -> Vec<ManifestDependency> {
    let is_block_open = |l: &str| {
        let t = l.trim();
        t.starts_with("dependencies") && t.contains('{')
    };
    let Some(start) = text.lines().position(is_block_open) else {
        return Vec::new();
    };

    let mut deps = Vec::new();
    let mut depth = 1i32;
    for line in text.lines().skip(start + 1) {
        let trimmed = line.trim();
        depth += trimmed.matches('{').count() as i32;
        depth -= trimmed.matches('}').count() as i32;
        if depth <= 0 {
            break;
        }
        deps.extend(gradle_dependency_line(trimmed));
    }
    deps
}

/// The zero-or-more dependency coordinates literal on one line of a `dependencies` block, e.g.
/// `implementation 'com.foo:bar:1.0'` or `testImplementation("com.foo:baz:2.0")`.
fn gradle_dependency_line(trimmed: &str) -> Vec<ManifestDependency> {
    let Some((config, rest)) = trimmed.split_once(|c: char| c.is_whitespace() || c == '(') else {
        return Vec::new();
    };
    let Some(scope) = gradle_scope(config) else {
        return Vec::new();
    };
    string_literals(rest)
        .into_iter()
        .map(|lit| {
            // "group:artifact:version" — keep group:artifact as the identity, matching
            // Maven's coordinate shape; a bare "artifact" (no colon) is kept as-is.
            let name = lit.rsplit_once(':').map_or(lit.clone(), |(head, _ver)| {
                let mut parts = head.splitn(2, ':');
                match (parts.next(), parts.next()) {
                    (Some(a), Some(b)) => format!("{a}:{b}"),
                    _ => head.to_string(),
                }
            });
            ManifestDependency {
                name: SmolStr::new(name),
                version_req: SmolStr::new(lit.rsplit_once(':').map(|(_, v)| v).unwrap_or("*")),
                scope,
            }
        })
        .collect()
}

/// Table-driven rather than matched: a `match` over this many string alternatives is itself a
/// CRAP-flagged decision count under kndo's own zero-coverage gate — a lookup keeps this at
/// effectively zero branches while staying just as exhaustive.
const GRADLE_CONFIG_SCOPES: &[(&str, DependencyScope)] = &[
    ("implementation", DependencyScope::Prod),
    ("api", DependencyScope::Prod),
    ("compile", DependencyScope::Prod),
    ("runtimeOnly", DependencyScope::Prod),
    ("runtime", DependencyScope::Prod),
    ("testImplementation", DependencyScope::Dev),
    ("testCompile", DependencyScope::Dev),
    ("testRuntimeOnly", DependencyScope::Dev),
    ("compileOnly", DependencyScope::Peer),
    ("annotationProcessor", DependencyScope::Build),
    ("testAnnotationProcessor", DependencyScope::Build),
];

fn gradle_scope(configuration: &str) -> Option<DependencyScope> {
    GRADLE_CONFIG_SCOPES
        .iter()
        .find(|(name, _)| *name == configuration)
        .map(|(_, scope)| *scope)
}

/// Every single-quoted or double-quoted literal on a line — the only shape this best-effort
/// scan trusts (spec §4): `implementation "com.foo:bar:1.0"`, `implementation('com.foo:bar:1.0')`.
fn string_literals(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        let Some((lit, tail)) = next_quoted(rest) else {
            break;
        };
        out.push(lit);
        rest = tail;
    }
    out
}

/// The first quoted literal in `s` (single- or double-quoted, unterminated is simply not a
/// match), and the remainder of `s` after it.
fn next_quoted(s: &str) -> Option<(String, &str)> {
    let start = s.find(['\'', '"'])?;
    let quote = s.as_bytes()[start] as char;
    let content_start = start + quote.len_utf8();
    let end = s[content_start..].find(quote)?;
    let content_end = content_start + end;
    Some((
        s[content_start..content_end].to_string(),
        &s[content_end + quote.len_utf8()..],
    ))
}

// ---------------------------------------------------------------- shared root promotion

/// One `ManifestRoot{Production, Certain}` per non-test `.java` file under `source_root`
/// (spec §4) — Java has no single entry file the way Rust's `lib.rs` does, so every file in a
/// publishable module's source tree is a promotion trigger; the existing per-file declaration-
/// promotion path (`library_root_files`) does the rest with zero new core mechanism. `module-
/// info.java`/`package-info.java` are skipped (they declare nothing to promote).
fn promote_source_roots(source_root: &ProjectPath, ctx: &ResolveCtx<'_>, out: &mut ManifestFacts) {
    let mut files: Vec<ProjectPath> = ctx
        .files_under(&source_root.0)
        .filter(|p| p.0.ends_with(".java"))
        .filter(|p| {
            let name = p.0.rsplit('/').next().unwrap_or(p.0.as_str());
            name != "module-info.java" && name != "package-info.java"
        })
        .filter(|p| {
            !kndo_adapter_toolkit::classify::UNIVERSAL_VENDORED_DIRS
                .iter()
                .any(|d| p.0.split('/').any(|seg| seg == *d))
        })
        .cloned()
        .collect();
    files.sort(); // deterministic (RFC 0008 §4)
    for target in files {
        out.roots.push(ManifestRoot {
            kind: RootKind::Production,
            target,
            confidence: Confidence::Certain,
        });
    }
}

fn diag(message: &str) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warn,
        path: None,
        message: message.to_string(),
        span: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;

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
