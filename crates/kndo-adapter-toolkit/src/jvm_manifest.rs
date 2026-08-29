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
    AdapterDiagnostic, DiagnosticLevel, ManifestDependency, ManifestFacts, ManifestRoot,
    ProjectPath, ResolveCtx,
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

/// The JVM half of [`kndo_core::adapter::LanguageAdapter::declares_dependency`]: a Maven or
/// Gradle dependency is stored under its full `groupId:artifactId` coordinate, but an
/// activation rule is written by a human who says `spring-boot-starter-thymeleaf`, not
/// `org.springframework.boot:spring-boot-starter-thymeleaf`. So a query matches either the
/// whole coordinate or the artifact id alone.
///
/// Matching the bare artifact id can in principle match two groups publishing the same
/// artifact name. That is the keep-alive direction — a conventions plugin turning on for a
/// project that does not use that exact vendor's artifact contributes roots and edges nobody
/// asked for, which can only suppress findings, never invent one (RFC 0012 §2) — and it is the
/// only spelling an author can reasonably be expected to write.
pub fn declares_dependency(facts: &ManifestFacts, query: &str) -> bool {
    facts
        .dependencies
        .iter()
        .chain(&facts.workspace_dependencies)
        .any(|d| {
            d.name == query
                || d.name
                    .rsplit(':')
                    .next()
                    .is_some_and(|artifact| artifact == query)
        })
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
    // A POM carrying a DOCTYPE is legal and `roxmltree` refuses one by default, which would
    // fail the whole manifest silently. Allowed for the same reason as the coverage
    // ingesters': no external entity is ever resolved.
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = match roxmltree::Document::parse_with_options(text, options) {
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
    let properties = maven_properties(project);
    collect_maven_deps(xml_child(project, "dependencies"), &properties, &mut out);

    // Only packages (non-virtual poms) get source-tree root promotion; a pure-aggregator
    // `<packaging>pom</packaging>` with no source tree contributes topology only.
    if !out.private {
        let dir = crate::paths::dirname(path);
        // A DECLARED source directory wins over the convention. Maven's `<sourceDirectory>` is
        // the module saying where its code is, and a module that says so is not guessing —
        // guava declares `src` (with tests in a sibling `test`), which the hardcoded
        // `src/main/java` convention would never match, leaving the module's entire
        // publishable surface unpromoted and every public class in it reading as `unused`.
        // The convention is the fallback, not the rule.
        let declared = maven_declared_source_root(project, &properties)
            // …and a module that declares nothing may still be told where its code is by an
            // ANCESTOR. Maven inheritance is not a corner: guava declares `<sourceDirectory>`
            // exactly once, in `guava-parent`, and all ten modules inherit it — reading only
            // each pom's own text finds zero production roots in guava, leaving 88% of the
            // repository unreachable.
            .or_else(|| maven_inherited_source_root(project, path, ctx));
        match declared {
            Some(declared) => {
                promote_source_roots(&join(dir, &declared), ctx, &mut out, layout);
            }
            None => {
                for root in layout.source_roots {
                    let source_root = join(dir, root);
                    promote_source_roots(&source_root, ctx, &mut out, layout);
                }
            }
        }
    }
    out
}

/// `<build><sourceDirectory>`, module-relative, or `None` when the pom does not declare one.
///
/// `${basedir}`/`${project.basedir}` is the pom's own directory and is stripped — the result is
/// joined onto that directory anyway. Any other unresolved placeholder yields `None` rather
/// than a guess: it depends on a build kndo never runs, and falling back to the convention is
/// the honest outcome. An absolute path likewise names something outside the project.
///
/// `<testSourceDirectory>` is deliberately not consulted. Promotion is about the *production*
/// surface, and a declared source directory that happens to contain tests is a shape no
/// observed project has — guava puts them in a sibling.
fn maven_declared_source_root(
    project: roxmltree::Node<'_, '_>,
    properties: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let declared = xml_child(project, "build")
        .and_then(|b| xml_child_text(b, "sourceDirectory"))?
        .trim();
    let resolved = interpolate_maven(declared, properties)?;
    let resolved = resolved
        .strip_prefix("${basedir}")
        .or_else(|| resolved.strip_prefix("${project.basedir}"))
        .unwrap_or(&resolved)
        .trim_start_matches('/')
        .trim_end_matches('/')
        .to_string();
    // Absolute, empty, or escaping the module: nothing this can point at inside the project.
    (!resolved.is_empty() && !resolved.starts_with('/') && !resolved.starts_with(".."))
        .then_some(resolved)
}

/// How many `<parent>` hops to follow before giving up.
///
/// Real hierarchies are two or three deep (guava is one). The bound is not a performance
/// measure — it is what keeps a `<relativePath>` cycle, or a pom that names itself, from
/// looping forever on input kndo does not control.
const MAVEN_PARENT_DEPTH: usize = 16;

/// `<build><sourceDirectory>` declared by an ANCESTOR pom, module-relative to *this* pom.
///
/// Maven resolves `<parent>` by `<relativePath>`, defaulting to `../pom.xml`; a `<parent>` with
/// no resolvable file on disk is one that comes from a repository, which kndo never fetches —
/// that chain simply ends. Each hop's value is interpolated against **that** pom's own
/// `<properties>`, which is what Maven does: the declaration and the properties it mentions
/// live in the same document. The result is then joined onto the *child's* directory by the
/// caller, which is also Maven's rule — `<sourceDirectory>` is resolved per-module against each
/// module's own basedir, which is exactly why one declaration in a parent serves ten modules
/// with ten different source trees.
///
/// Returns `None` when no ancestor declares one, when the chain leaves the project, or when the
/// context has no manifest-text channel (import resolution) — all of which mean "I cannot see
/// a declaration", never "there is none", and the caller falls back to the convention either
/// way.
fn maven_inherited_source_root(
    project: roxmltree::Node<'_, '_>,
    path: &str,
    ctx: &ResolveCtx<'_>,
) -> Option<String> {
    let mut current_dir = crate::paths::dirname(path).to_string();
    // What this pom says about its parent, as owned data. Each hop reduces the pom it reads to
    // the same shape and drops the document — a `roxmltree::Document` borrows its text, so
    // carrying nodes up the chain would mean keeping every text alive with it.
    let mut want = maven_parent_link(project)?;
    let mut seen: Vec<String> = vec![path.to_string()];

    for _ in 0..MAVEN_PARENT_DEPTH {
        let parent_path = normalize_relative(&current_dir, &want.relative_path)?;
        // A cycle (`<relativePath>` pointing back down the chain, or a pom naming itself) is
        // malformed input, not a shape to follow.
        if seen.contains(&parent_path) {
            return None;
        }
        let text = ctx.read_manifest(&ProjectPath(SmolStr::new(&parent_path)))?;
        let summary = maven_summarize(&text)?;

        // Maven accepts the pom at `<relativePath>` only when its coordinates are the ones the
        // child declared; otherwise the parent comes from the repository, which kndo never
        // fetches. This is what keeps a `../pom.xml` that happens to be an unrelated module
        // from being read as an ancestor — and it is not hypothetical: guava's `futures/*`
        // modules name `guava-parent` with no `<relativePath>`, no `futures/pom.xml` beside
        // them, and versions (`26.0-android`) the in-repo parent has not carried for years.
        //
        // `<version>` is deliberately not compared: kndo is locating a source directory, not
        // building, and a version-skewed but coordinate-matching parent on disk is still the
        // file the author edits.
        if !summary.identifies_as(&want) {
            return None;
        }
        if let Some(declared) = summary.source_directory {
            return Some(declared);
        }
        seen.push(parent_path.clone());
        current_dir = crate::paths::dirname(&parent_path).to_string();
        want = summary.parent?;
    }
    None
}

/// A pom's `<parent>` reference: which module it names, and where on disk it says to look.
struct MavenParentLink {
    group_id: Option<String>,
    artifact_id: String,
    /// Relative to the declaring pom's own directory. Maven's default is `../pom.xml`.
    relative_path: String,
}

/// Everything the inheritance walk needs from one pom, owned so its document can be dropped.
struct MavenSummary {
    group_id: Option<String>,
    artifact_id: Option<String>,
    /// `<build><sourceDirectory>`, already interpolated against this pom's own properties.
    source_directory: Option<String>,
    parent: Option<MavenParentLink>,
}

impl MavenSummary {
    /// Whether this pom is the module `link` names. `groupId` may legitimately be absent on
    /// either side — a pom inherits its own group from *its* parent — and an absent one cannot
    /// disprove a match, so the artifact id decides.
    fn identifies_as(&self, link: &MavenParentLink) -> bool {
        if self.artifact_id.as_deref() != Some(link.artifact_id.as_str()) {
            return false;
        }
        match (&link.group_id, &self.group_id) {
            (Some(want), Some(found)) => want == found,
            _ => true,
        }
    }
}

fn maven_summarize(text: &str) -> Option<MavenSummary> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    // A parent that does not parse ends the chain quietly: this is a fallback pass, and that
    // pom's own extraction reports the parse error where it belongs.
    let doc = roxmltree::Document::parse_with_options(text, options).ok()?;
    let project = doc.root_element();
    let properties = maven_properties(project);
    Some(MavenSummary {
        group_id: xml_child_text(project, "groupId").map(|s| s.trim().to_string()),
        artifact_id: xml_child_text(project, "artifactId").map(|s| s.trim().to_string()),
        source_directory: maven_declared_source_root(project, &properties),
        parent: maven_parent_link(project),
    })
}

/// This pom's `<parent>`: the module it names and the path it says to look at, relative to
/// this pom's own directory. Maven's default is `../pom.xml`, and a `<relativePath>` naming a
/// directory means that directory's `pom.xml`.
fn maven_parent_link(project: roxmltree::Node<'_, '_>) -> Option<MavenParentLink> {
    let parent = xml_child(project, "parent")?;
    // A `<parent>` naming no artifact is malformed; nothing on disk can be it.
    let artifact_id = xml_child_text(parent, "artifactId")?.trim().to_string();
    // Presence and text are separate questions here, and `xml_child_text` answers only the
    // second: `<relativePath/>` has no text node, so it and an absent element look identical
    // through it. They mean opposite things — an EMPTY `<relativePath/>` is Maven's explicit
    // "resolve this parent from the repository, not the filesystem", the one spelling that must
    // NOT fall back to `../pom.xml`, while an absent element is exactly what does.
    let relative_path = match xml_child(parent, "relativePath") {
        Some(node) => {
            let rel = node.text().unwrap_or_default().trim().to_string();
            if rel.is_empty() {
                return None;
            }
            if rel.ends_with(".xml") {
                rel
            } else {
                format!("{}/pom.xml", rel.trim_end_matches('/'))
            }
        }
        None => "../pom.xml".to_string(),
    };
    Some(MavenParentLink {
        group_id: xml_child_text(parent, "groupId").map(|s| s.trim().to_string()),
        artifact_id,
        relative_path,
    })
}

/// `dir` joined with a `../`-bearing relative path, as a project path. `None` when it escapes
/// the project root — a parent outside the analyzed tree is one kndo cannot read anyway.
fn normalize_relative(dir: &str, relative: &str) -> Option<String> {
    let mut segments: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}

/// `value` with `${key}` placeholders substituted from the manifest's own `<properties>`.
/// `${basedir}` is left in place for the caller, which knows what it means; anything else
/// still unresolved yields `None`.
fn interpolate_maven(
    value: &str,
    properties: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find("${") {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let close = tail.find('}')?;
        let key = &tail[2..close];
        match properties.get(key) {
            Some(v) => out.push_str(v),
            // `basedir` is the caller's to strip; every other unknown is a build-time value.
            None if key == "basedir" || key == "project.basedir" => out.push_str(&tail[..=close]),
            None => return None,
        }
        rest = &tail[close + 1..];
    }
    out.push_str(rest);
    Some(out)
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

fn collect_maven_deps(
    deps: Option<roxmltree::Node<'_, '_>>,
    properties: &std::collections::HashMap<String, String>,
    out: &mut ManifestFacts,
) {
    let Some(deps) = deps else {
        return;
    };
    let dependency_nodes = deps
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "dependency");
    for dep in dependency_nodes {
        if let Some(dependency) = maven_dependency(dep, properties) {
            out.dependencies.push(dependency);
        }
    }
}

fn maven_dependency(
    dep: roxmltree::Node<'_, '_>,
    properties: &std::collections::HashMap<String, String>,
) -> Option<ManifestDependency> {
    let artifact_id = xml_child_text(dep, "artifactId")?;
    let group_id = xml_child_text(dep, "groupId").unwrap_or("");
    let name = if group_id.is_empty() {
        artifact_id.to_string()
    } else {
        format!("{group_id}:{artifact_id}")
    };
    // No `<version>` at all is the BOM-managed shape (`<dependencyManagement>` in a parent POM
    // supplies it): the manifest states no comparable requirement, which is `None` — never a
    // stand-in version that every real one would then "diverge" from.
    let version_req = xml_child_text(dep, "version")
        .and_then(|raw| resolve_placeholder(raw, properties))
        .map(SmolStr::new);
    Some(ManifestDependency {
        name: SmolStr::new(name),
        version_req,
        scope: maven_dependency_scope(xml_child_text(dep, "scope")),
        inherited: false,
    })
}

/// A declared version with `${…}` / `$…` placeholders substituted from the manifest's own
/// property pool, or `None` when any placeholder in it is unresolved.
///
/// Taking an unresolved placeholder verbatim would read `${spring.version}` as diverging
/// from `5.3.0`, and `$junit5Version` from `$junit5_version` — two spellings of one
/// `gradle.properties` key. The value is not a version and must not be compared as one; the
/// honest answer to "what does this manifest require" is that we could not read it.
fn resolve_placeholder(
    raw: &str,
    properties: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let raw = raw.trim();
    if !raw.contains('$') {
        return (!raw.is_empty()).then(|| raw.to_string());
    }
    let key = raw
        .strip_prefix("${")
        .and_then(|r| r.strip_suffix('}'))
        .or_else(|| raw.strip_prefix('$'))?;
    properties.get(key.trim()).cloned()
}

/// `<properties>` — Maven's own version pool, resolvable without leaving the file. A parent
/// POM's properties are out of reach by construction (kndo never resolves the classpath), and
/// a dependency whose placeholder lives there stays `None`.
fn maven_properties(project: roxmltree::Node<'_, '_>) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let Some(properties) = xml_child(project, "properties") else {
        return out;
    };
    for property in properties.children().filter(|n| n.is_element()) {
        if let Some(value) = property.text().map(str::trim) {
            out.insert(property.tag_name().name().to_string(), value.to_string());
        }
    }
    out
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

    let properties = gradle_properties(text);
    let mut deps = Vec::new();
    let mut depth = 1i32;
    for line in text.lines().skip(start + 1) {
        let trimmed = line.trim();
        depth += trimmed.matches('{').count() as i32;
        depth -= trimmed.matches('}').count() as i32;
        if depth <= 0 {
            break;
        }
        deps.extend(gradle_dependency_line(trimmed, &properties));
    }
    deps
}

/// The zero-or-more dependency coordinates literal on one line of a `dependencies` block, e.g.
/// `implementation 'com.foo:bar:1.0'` or `testImplementation("com.foo:baz:2.0")`.
fn gradle_dependency_line(
    trimmed: &str,
    properties: &std::collections::HashMap<String, String>,
) -> Vec<ManifestDependency> {
    let Some((config, rest)) = trimmed.split_once(|c: char| c.is_whitespace() || c == '(') else {
        return Vec::new();
    };
    let Some(scope) = gradle_scope(config) else {
        return Vec::new();
    };
    string_literals(rest)
        .into_iter()
        .map(|lit| {
            let (name, version_req) = gradle_coordinate(&lit, properties);
            ManifestDependency {
                name: SmolStr::new(name),
                version_req: version_req.map(SmolStr::new),
                scope,
                inherited: false,
            }
        })
        .collect()
}

/// Split one Gradle coordinate literal into `(group:artifact, version)` **by segment count**,
/// never by "everything after the last colon".
///
/// A BOM/platform-managed coordinate has TWO segments and no version at all
/// (`implementation 'org.springframework.boot:spring-boot-starter-actuator'`, with the version
/// supplied by an imported BOM). Splitting on the last colon instead would read that as
/// `name = "org.springframework.boot"`, `version = "spring-boot-starter-actuator"` —
/// reporting an artifact id as a diverging version of a group id on every JVM repository
/// with a BOM-managed dependency.
fn gradle_coordinate(
    lit: &str,
    properties: &std::collections::HashMap<String, String>,
) -> (String, Option<String>) {
    let segments: Vec<&str> = lit.split(':').collect();
    match segments.as_slice() {
        [group, artifact, version, ..] => (
            format!("{group}:{artifact}"),
            resolve_placeholder(version, properties),
        ),
        // Two segments: a full coordinate whose version comes from a BOM. One: a project
        // accessor or a bare name. Neither states a requirement.
        [group, artifact] => (format!("{group}:{artifact}"), None),
        _ => (lit.to_string(), None),
    }
}

/// `val x = "1.2.3"` / `def x = '1.2.3'` / `ext { x = "1.2.3" }` — Gradle's in-file version
/// pool, the analogue of Maven's `<properties>`. A `gradle.properties` key or a version catalog
/// lives outside the manifest and stays unresolved, which is `None`, not a literal.
fn gradle_properties(text: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        let assignment = line
            .strip_prefix("val ")
            .or_else(|| line.strip_prefix("def "))
            .or_else(|| line.strip_prefix("var "))
            .unwrap_or(line);
        let Some((key, value)) = assignment.split_once('=') else {
            continue;
        };
        let key = key.split(':').next().unwrap_or(key).trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            continue;
        }
        let literals = string_literals(value);
        if let [only] = literals.as_slice() {
            out.insert(key.to_string(), only.clone());
        }
    }
    out
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

fn diag(message: &str) -> AdapterDiagnostic {
    AdapterDiagnostic {
        level: DiagnosticLevel::Warn,
        message: message.to_string(),
        span: None,
    }
}

#[cfg(test)]
mod parent_link_tests {
    use super::*;

    fn link(pom: &str) -> Option<MavenParentLink> {
        let doc = roxmltree::Document::parse(pom).expect("test pom parses");
        maven_parent_link(doc.root_element())
    }

    /// **An empty `<relativePath/>` means "from the repository", not "next door".**
    ///
    /// Pinned here rather than through `extract`, because there it is unobservable: every
    /// spelling that reaches `normalize_relative` with an empty path resolves back to the
    /// declaring pom itself, which the cycle guard rejects anyway. Two rules landing on the
    /// same answer today is not one rule — a change to how `normalize_relative` reads a leading
    /// `/` (project-root-relative is a perfectly plausible reading) would separate them, and
    /// this pom would start inheriting from a parent Maven never consults.
    #[test]
    fn an_empty_relative_path_declines_the_filesystem() {
        assert!(link(
            "<project><parent><artifactId>p</artifactId><relativePath/></parent></project>"
        )
        .is_none());
        assert!(link(
            "<project><parent><artifactId>p</artifactId><relativePath>  </relativePath>\
             </parent></project>"
        )
        .is_none());
    }

    /// An ABSENT `<relativePath>` is the opposite: Maven's documented default.
    #[test]
    fn an_absent_relative_path_is_the_pom_one_directory_up() {
        let l = link("<project><parent><artifactId>p</artifactId></parent></project>")
            .expect("a parent with no relativePath still links");
        assert_eq!(l.relative_path, "../pom.xml");
        assert_eq!(l.artifact_id, "p");
        assert_eq!(l.group_id, None);
    }

    /// A `<relativePath>` naming a directory means that directory's `pom.xml`.
    #[test]
    fn a_directory_relative_path_gains_the_file_name() {
        let l = link(
            "<project><parent><artifactId>p</artifactId>\
             <relativePath>../build/parent</relativePath></parent></project>",
        )
        .expect("linked");
        assert_eq!(l.relative_path, "../build/parent/pom.xml");

        let l = link(
            "<project><parent><artifactId>p</artifactId>\
             <relativePath>../parent/custom.xml</relativePath></parent></project>",
        )
        .expect("linked");
        assert_eq!(l.relative_path, "../parent/custom.xml");
    }

    /// A `<parent>` naming no artifact is malformed — nothing on disk can be it.
    #[test]
    fn a_parent_without_an_artifact_id_links_to_nothing() {
        assert!(link("<project><parent><groupId>g</groupId></parent></project>").is_none());
        assert!(link("<project></project>").is_none());
    }

    #[test]
    fn a_relative_path_is_resolved_against_the_declaring_directory() {
        assert_eq!(
            normalize_relative("mod", "../pom.xml").as_deref(),
            Some("pom.xml")
        );
        assert_eq!(
            normalize_relative("a/b", "../../pom.xml").as_deref(),
            Some("pom.xml")
        );
        assert_eq!(
            normalize_relative("a/b", "../c/pom.xml").as_deref(),
            Some("a/c/pom.xml")
        );
        // Escaping the project root: a parent kndo cannot read anyway.
        assert_eq!(normalize_relative("", "../pom.xml"), None);
    }
}
