//! Maven/Gradle manifest fidelity shared by every JVM-family adapter (Java, Kotlin —
//! the two share infra). The two ecosystems' manifest shapes have zero language
//! dependency: `pom.xml`/`build.gradle` describe dependency coordinates and module topology
//! the same way whether the module's source is `.java` or `.kt`. What DOES differ per
//! language is the source-root convention (`src/main/java` vs `src/main/kotlin`) and which
//! file extension counts as a source file for root promotion — both threaded through as a
//! [`JvmSourceLayout`] rather than hardcoded.
//!
//! Maven `pom.xml` is fully structured via `roxmltree` (real XML, groupId inheritance, scope
//! mapping, `<dependencyManagement>` excluded). Gradle `build.gradle`/`.kts` +
//! `settings.gradle`/`.kts` are best-effort line-scanned — Gradle's actual grammar is a real
//! programming language (Groovy/Kotlin), so only literal-string forms are recognized; anything
//! computed is silently invisible, never misparsed.

use kndo_core::adapter::{
    Diagnostic, DiagnosticLevel, ManifestDependency, ManifestFacts, ManifestRoot, ProjectPath,
    ResolveCtx,
};
use kndo_core::vocab::{Confidence, DependencyScope, RootKind};
use smol_str::SmolStr;

/// Per-language source-tree conventions a JVM-family adapter's manifest extraction needs
/// (everything else about Maven/Gradle fidelity is language-blind).
pub struct JvmSourceLayout {
    /// Source roots relative to the manifest's directory, in promotion order. More than one
    /// when Gradle registers several for a language: Kotlin's `main` source set includes
    /// both `src/main/kotlin` and `src/main/java`, and real projects
    /// keep `.kt` files under the latter. The `source_ext` filter keeps each language's
    /// promotion to its own files even in a shared directory.
    pub source_roots: &'static [&'static str],
    /// Extension (with leading dot) a file must have to be promoted as a module root.
    pub source_ext: &'static str,
    /// File basenames that declare nothing promotable (Java's `module-info.java`,
    /// `package-info.java`) — skipped during root promotion. Empty when the language has no
    /// such convention (Kotlin).
    pub skip_file_names: &'static [&'static str],
}

pub fn extract(
    path: &str,
    content: &[u8],
    ctx: &ResolveCtx<'_>,
    layout: &JvmSourceLayout,
) -> ManifestFacts {
    let Ok(text) = std::str::from_utf8(content) else {
        let mut out = ManifestFacts::default();
        out.diagnostics.push(diag("manifest is not valid UTF-8"));
        return out;
    };
    if path.rsplit('/').next() == Some("pom.xml") {
        extract_maven(path, text, ctx, layout)
    } else if matches!(
        path.rsplit('/').next(),
        Some("settings.gradle" | "settings.gradle.kts")
    ) {
        extract_gradle_settings(text)
    } else {
        extract_gradle_build(path, text, ctx, layout)
    }
}

// ---------------------------------------------------------------- Maven (pom.xml)

fn extract_maven(
    path: &str,
    text: &str,
    ctx: &ResolveCtx<'_>,
    layout: &JvmSourceLayout,
) -> ManifestFacts {
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
    // this module — `collect_maven_deps` only ever sees a plain `<dependencies>` block.
    collect_maven_deps(xml_child(project, "dependencies"), &mut out);

    // Only packages (non-virtual poms) get source-tree root promotion; a pure-aggregator
    // `<packaging>pom</packaging>` with no source tree contributes topology only.
    if !out.private {
        let dir = crate::paths::dirname(path);
        for root in layout.source_roots {
            let source_root = join(dir, root);
            promote_source_roots(&source_root, ctx, &mut out, layout);
        }
    }
    out
}

fn join(dir: &str, rel: &str) -> ProjectPath {
    if dir.is_empty() {
        ProjectPath(SmolStr::new(rel))
    } else {
        ProjectPath(SmolStr::new(format!("{dir}/{rel}")))
    }
}

/// The two-part coordinate IS the cross-module identity — `groupId:artifactId`, falling back
/// to `<parent><groupId>` when the child inherits it (the common pattern). No `artifactId`
/// means no identity at all.
fn maven_package_name(project: roxmltree::Node<'_, '_>) -> Option<SmolStr> {
    let artifact_id = xml_child_text(project, "artifactId")?;
    let group_id = xml_child_text(project, "groupId")
        .or_else(|| xml_child(project, "parent").and_then(|p| xml_child_text(p, "groupId")));
    Some(match group_id {
        Some(g) => SmolStr::new(format!("{g}:{artifact_id}")),
        None => SmolStr::new(artifact_id),
    })
}

/// `<packaging>` ≠ jar (default) → app/aggregator mode: Maven has no `private` flag, so this
/// is the packaging-based proxy — `pom` (aggregator, no code) and `war` (deployable app, not
/// an importable dependency) are the two treated as private.
fn maven_is_private(project: roxmltree::Node<'_, '_>) -> bool {
    matches!(xml_child_text(project, "packaging"), Some("pom" | "war"))
}

/// `<modules><module>sub-a</module></modules>` — workspace topology.
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
/// consumer semantics as npm's peerDependencies. "runtime"/"system"/"compile"/absent are a
/// documented approximation to `Prod` — genuinely used, just not compile-visible; kndo's
/// taxonomy has no runtime-only scope.
fn maven_dependency_scope(scope: Option<&str>) -> DependencyScope {
    match scope {
        Some("test") => DependencyScope::Dev,
        Some("provided") => DependencyScope::Peer,
        _ => DependencyScope::Prod,
    }
}

// ---------------------------------------------------------------- Gradle

/// `settings.gradle`/`.kts`: `include(':sub-a')` / `include 'sub-a'` line-scanned for
/// workspace topology only — the Gradle analogue of Maven's `<modules>`.
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

/// `build.gradle`/`.kts`: best-effort literal-string scan of the `dependencies { … }` block —
/// computed/interpolated/version-catalog coordinates are invisible, never misparsed.
/// `apply plugin: 'application'` / `plugins { application }` marks app mode.
fn extract_gradle_build(
    path: &str,
    text: &str,
    ctx: &ResolveCtx<'_>,
    layout: &JvmSourceLayout,
) -> ManifestFacts {
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
        let dir = crate::paths::dirname(path);
        for root in layout.source_roots {
            let source_root = join(dir, root);
            promote_source_roots(&source_root, ctx, &mut out, layout);
        }
    }
    out
}

/// `group = 'com.foo'` (or `group 'com.foo'`, top-level, outside any block) combined with the
/// containing directory's name (Gradle's own project-name-is-the-dirname default).
fn gradle_package_name(path: &str, text: &str) -> Option<SmolStr> {
    let group = gradle_group_literal(text);
    let project_name = crate::paths::dirname(path)
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
    // Kotlin's own compiler-plugin configuration (kapt annotation processing) — same Build
    // scope as Java's annotationProcessor; harmless as dead data for the Java adapter, which
    // never sees a "kapt" line in its own projects.
    ("kapt", DependencyScope::Build),
    ("kaptTest", DependencyScope::Build),
];

fn gradle_scope(configuration: &str) -> Option<DependencyScope> {
    GRADLE_CONFIG_SCOPES
        .iter()
        .find(|(name, _)| *name == configuration)
        .map(|(_, scope)| *scope)
}

/// Every single-quoted or double-quoted literal on a line — the only shape this best-effort
/// scan trusts: `implementation "com.foo:bar:1.0"`, `implementation('com.foo:bar:1.0')`.
fn string_literals(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some((lit, tail)) = next_quoted(rest) {
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

/// One `ManifestRoot{Production, Certain}` per non-test source file under `source_root` — the
/// JVM family has no single entry file the way Rust's `lib.rs` does, so every file in a
/// publishable module's source tree is a promotion trigger; the existing per-file declaration-
/// promotion path (`library_root_files`) does the rest with zero new core mechanism beyond
/// `ResolveCtx::files_under`.
fn promote_source_roots(
    source_root: &ProjectPath,
    ctx: &ResolveCtx<'_>,
    out: &mut ManifestFacts,
    layout: &JvmSourceLayout,
) {
    let mut files: Vec<ProjectPath> = ctx
        .files_under(&source_root.0)
        .filter(|p| p.0.ends_with(layout.source_ext))
        .filter(|p| {
            let name = p.0.rsplit('/').next().unwrap_or(p.0.as_str());
            !layout.skip_file_names.contains(&name)
        })
        .filter(|p| !is_vendored(p.0.as_str()))
        .cloned()
        .collect();
    files.sort(); // deterministic
    for target in files {
        out.roots.push(ManifestRoot {
            kind: RootKind::Production,
            target,
            confidence: Confidence::Certain,
        });
    }
}

fn is_vendored(path: &str) -> bool {
    crate::classify::UNIVERSAL_VENDORED_DIRS
        .iter()
        .any(|d| path.split('/').any(|seg| seg == *d))
}

fn diag(message: &str) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Warn,
        path: None,
        message: message.to_string(),
        span: None,
    }
}
