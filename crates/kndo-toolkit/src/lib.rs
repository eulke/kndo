//! The adapters' paved road: helpers whose behavior is identical for a grammar they
//! have never seen. Grammar constants stay in their adapters, next to the grammar;
//! the four lines of parser scaffolding around them live here once (v1 carried five
//! verbatim copies of `parse`, three with comments defending the copy).

pub mod github_actions;

use kndo_contract::vocab::Span;
use tree_sitter::{Language, Node, Parser, Tree};

/// Parse `source` with `language`; `None` when the grammar refuses to load or the
/// parse produces no tree (extraction then degrades through a diagnostic).
pub fn parse(language: &Language, source: &[u8]) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(language).ok()?;
    parser.parse(source, None)
}

/// The whole parse-and-report opening every extraction shares: `None` came with
/// its diagnostic, a tree with syntax errors came with its warning — the two
/// user-facing strings exist once, so per-language output cannot drift.
pub fn parse_reporting(
    language: &Language,
    source: &[u8],
    out: &mut kndo_contract::evidence::EvidenceSink,
) -> Option<Tree> {
    use kndo_contract::evidence::DiagnosticLevel;
    let Some(tree) = parse(language, source) else {
        out.diagnostic(
            DiagnosticLevel::Warn,
            "parse produced no tree — no evidence extracted from this file",
            None,
        );
        return None;
    };
    if tree.root_node().has_error() {
        out.diagnostic(
            DiagnosticLevel::Info,
            "syntax errors in file — evidence may be partial",
            None,
        );
    }
    Some(tree)
}

/// A node's extent as the contract's byte span — tree-sitter yields bytes natively,
/// which is exactly why the contract stores them.
pub fn span(node: Node<'_>) -> Span {
    Span::new(node.start_byte() as u32, node.end_byte() as u32)
}

pub fn text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

/// First named child of the given kind, if any.
pub fn child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    let mut children = node.named_children(&mut cursor);
    children.find(|c| c.kind() == kind)
}

/// Depth-first walk calling `f` on every node (named and anonymous).
pub fn walk(node: Node<'_>, f: &mut dyn FnMut(Node<'_>)) {
    f(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, f);
    }
}

/// FNV-1a over bytes — the stable, dependency-free token hash the fingerprint
/// pipeline builds on. Not cryptographic; collisions only ever merge clone groups
/// toward under-reporting.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Winnowing (Schleimer et al.) over an already-hashed token stream: k-gram hashes,
/// then the minimum of each sliding window, returned sorted and deduplicated so two
/// token streams are structural clones exactly when their fingerprint sets are equal.
/// Grammar-independent by construction — adapters own tokenization and
/// normalization; this owns the guarantee (any shared run of `window + k - 1` tokens
/// shares a fingerprint).
pub fn winnow(token_hashes: &[u64], k: usize, window: usize) -> Vec<u64> {
    if token_hashes.len() < k {
        return Vec::new();
    }
    let grams: Vec<u64> = token_hashes
        .windows(k)
        .map(|gram| {
            let mut h: u64 = 0xcbf29ce484222325;
            for &t in gram {
                h = h.rotate_left(7) ^ t.wrapping_mul(0x100000001b3);
            }
            h
        })
        .collect();
    let mut out: Vec<u64> = if grams.len() <= window {
        grams.iter().copied().min().into_iter().collect()
    } else {
        grams
            .windows(window)
            .map(|w| *w.iter().min().unwrap())
            .collect()
    };
    out.sort_unstable();
    out.dedup();
    out
}

/// Depth-first walk that never enters the named subtrees — the shared shape of
/// every adapter's reference pass: binding/renaming constructs (imports,
/// package clauses) are pruned because their identifiers already became import
/// evidence, or deliberately none.
pub fn walk_pruned(node: Node<'_>, skip: &[&str], f: &mut dyn FnMut(Node<'_>)) {
    if skip.contains(&node.kind()) {
        return;
    }
    f(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_pruned(child, skip, f);
    }
}

/// The comment markers one grammar declares — the language fact, stated at the
/// call site. WHICH nodes are comments is also the adapter's (it matches its
/// grammar's kinds); this is the BYTES that open and close one, so the text
/// span can exclude them. Doc-comment markers that carry meaning beyond the
/// opener (Rust's `//!`/`/*!`) ride the `line_doc`/`block_doc` fields — the
/// helper strips them like any other marker; what they MEAN stays the
/// adapter's business.
pub struct CommentMarkers<'a> {
    /// Line-comment openers, checked in order.
    pub line: &'a [&'a str],
    /// Block-comment (opener, closer) pairs, checked in order.
    pub block: &'a [(&'a str, &'a str)],
    /// Doc/divider bytes that extend a matched LINE opener: any run of them
    /// (`///`, `//!`, `////`) belongs to the marker, so a doc comment strips to
    /// its text and a pragma parses identically in plain and doc form.
    pub line_doc: &'a [u8],
    /// Doc bytes that extend a matched BLOCK opener (`/**`, `/*!`), never
    /// consuming into the closer (`/**/` stays an empty comment).
    pub block_doc: &'a [u8],
}

/// Comment evidence: the node's span with any trailing newline the grammar
/// swallowed trimmed off, and the text span inside the declared markers. The
/// mechanics; the markers come from the adapter.
pub fn comment_evidence(
    n: Node<'_>,
    source: &[u8],
    markers: &CommentMarkers<'_>,
    out: &mut kndo_contract::evidence::EvidenceSink,
) {
    let mut span = span(n);
    while span.end > span.start
        && matches!(source.get(span.end as usize - 1), Some(b'\n') | Some(b'\r'))
    {
        span = Span::new(span.start, span.end - 1);
    }
    let bytes = &source[span.start as usize..(span.end as usize).min(source.len())];
    let text = markers
        .line
        .iter()
        .find(|m| bytes.starts_with(m.as_bytes()))
        .map(|m| {
            let extra = bytes[m.len()..]
                .iter()
                .take_while(|b| markers.line_doc.contains(b))
                .count();
            Span::new(span.start + (m.len() + extra) as u32, span.end)
        })
        .or_else(|| {
            markers.block.iter().find_map(|(open, close)| {
                (bytes.starts_with(open.as_bytes())
                    && bytes.ends_with(close.as_bytes())
                    && bytes.len() >= open.len() + close.len())
                .then(|| {
                    let extra = bytes[open.len()..]
                        .iter()
                        .take_while(|b| markers.block_doc.contains(b))
                        .count()
                        .min(bytes.len() - open.len() - close.len());
                    Span::new(
                        span.start + (open.len() + extra) as u32,
                        span.end - close.len() as u32,
                    )
                })
            })
        })
        .unwrap_or(span);
    out.comment(span, text);
}

/// The marker convention shared across ecosystems whose tools stamp generated
/// output (`@generated`, protobuf/codegen banners): the one needle list the
/// marker-scanning adapters declare. Go's convention is anchored more strictly
/// than a needle can say, so that adapter reads [`header_lines`] itself.
pub const GENERATED_NEEDLES: &[&str] = &["@generated", "Code generated", "DO NOT EDIT"];

/// A source file's HEADER: its shebang, if any, and every blank or comment line
/// before the first line that is neither. Where a generated-file marker may
/// sit, in every ecosystem that stamps one — `cmd/go` says it in as many words
/// ("before the first non-comment, non-blank text") — and the reason the scan
/// is bounded by the header rather than by a line count: a count finds the
/// marker under a short licence and misses the same marker under a long one.
/// Lines arrive trimmed; the language's comment openers are the adapter's
/// declaration, so a `#`-commented language passes its own rather than
/// inheriting C-family ones that could never match.
pub fn header_lines<'a>(
    source: &'a [u8],
    openers: &'a [&'a str],
) -> impl Iterator<Item = &'a str> + 'a {
    let mut in_block = false;
    source
        .split(|&b| b == b'\n')
        .map_while(|line| std::str::from_utf8(line).ok().map(str::trim))
        .enumerate()
        .take_while(move |(i, line)| {
            if in_block {
                in_block = !line.contains("*/");
                return true;
            }
            if line.is_empty() || (*i == 0 && line.starts_with("#!")) {
                return true;
            }
            if line.starts_with("/*") && !line.contains("*/") {
                in_block = true;
                return true;
            }
            openers.iter().any(|o| line.starts_with(o))
        })
        .map(|(_, line)| line)
}

/// The token every language's generated-file banner is reported under. What an
/// adapter SAW is its own ecosystem's spelling (`@generated`,
/// `// Code generated … DO NOT EDIT.`); what it SAYS is this one word, and
/// what the word means is [`kndo_contract::extension::Effect::Generated`],
/// carried as a rule in the spec — so no adapter decides that a file is
/// beyond judgment.
pub const GENERATED_MARKER: &str = "generated";

/// The default rule every marker-scanning language carries: the banner it
/// reports means the generator owns the file.
pub fn generated_rule() -> kndo_contract::extension::DispatchRule {
    kndo_contract::extension::DispatchRule {
        when: kndo_contract::extension::Trigger::marker(GENERATED_MARKER),
        then: kndo_contract::extension::Effect::Generated,
        confidence: kndo_contract::vocab::Confidence::Certain,
    }
}

/// Report the generated-file banner this file carries, if any, as a file
/// marker: the LINE the adapter matched is the argument, so a reader of
/// `describe` sees what convinced it. Reports nothing where the header carries
/// no banner.
pub fn mark_generated(
    source: &[u8],
    needles: &[&str],
    openers: &[&str],
    out: &mut kndo_contract::evidence::EvidenceSink,
) {
    let Some(line) = header_lines(source, openers)
        .find(|l| needles.iter().any(|n| l.contains(n)))
        .map(str::to_string)
    else {
        return;
    };
    out.marker(
        kndo_contract::evidence::MarkerTarget::File,
        GENERATED_MARKER,
        vec![smol_str::SmolStr::new(line)],
        kndo_contract::vocab::Span::new(0, 0),
    );
}

pub mod jvm_manifest {
    //! Maven/Gradle dependency NAMES for activation — the JVM half both the
    //! java and kotlin adapters share (one build system, two languages). Each
    //! dependency is reported under both spellings a rule's author might write:
    //! the full `groupId:artifactId` coordinate and the artifact id alone —
    //! the keep-alive direction, and the only spelling an author can
    //! reasonably be expected to write. Deliberately shallow line-shaped scans,
    //! not XML/Groovy parsers: dependency names are all activation reads.

    use kndo_contract::adapter::SourceFile;
    use smol_str::SmolStr;

    /// A JVM language's spec differs only in identity — the general fact
    /// ([`crate::source_adapter_spec`]) plus the one build system's manifests.
    pub fn jvm_spec(
        coordinate: &'static str,
        version: u32,
        suffixes: &[&'static str],
    ) -> kndo_contract::extension::ExtensionSpec {
        jvm_builder(coordinate, version, suffixes).build()
    }

    /// The same shared spelling, still open: a JVM adapter that declares a
    /// capability beyond it (markers and what they mean) chains and builds.
    pub fn jvm_builder(
        coordinate: &'static str,
        version: u32,
        suffixes: &[&'static str],
    ) -> kndo_contract::extension::ExtensionSpecBuilder {
        crate::source_adapter_builder(
            coordinate,
            version,
            suffixes,
            MANIFEST_GLOBS,
            // JVM compilers resolve reference cycles in multiple passes —
            // routine structure, never an initialization hazard worth a finding.
            kndo_contract::extension::CycleTolerance::Tolerated,
        )
    }

    /// The rule for a lint suppression naming the unused-code check: the author
    /// already answered the question this analysis asks, so the declaration is
    /// exempt and says so as a keeper. The argument pattern matches the
    /// quoted name wherever it sits, alone (`@SuppressWarnings("unused")`) or
    /// inside a brace initializer (`{"unused", "rawtypes"}`).
    pub fn suppresses_unused(marker: &'static str) -> kndo_contract::extension::DispatchRule {
        kndo_contract::extension::DispatchRule {
            when: kndo_contract::extension::Trigger::marker_with(marker, "*\"unused\"*"),
            then: kndo_contract::extension::Effect::Exempt,
            confidence: kndo_contract::vocab::Confidence::Certain,
        }
    }

    /// The one build system's manifest names — the shared half of every JVM
    /// language's spec declaration.
    pub const MANIFEST_GLOBS: &[&str] = &[
        "**/pom.xml",
        "**/build.gradle",
        "**/build.gradle.kts",
        "**/settings.gradle",
        "**/settings.gradle.kts",
    ];

    /// The project structure one JVM manifest STATES: the unit it compiles,
    /// what that unit compiles against, and the manifests it aggregates.
    ///
    /// Maven only, for now. A `pom.xml` names itself (`<artifactId>`), lists
    /// the modules of its reactor (`<modules>`) and the artifacts its unit is
    /// built against (`<dependencies>`), which is exactly what the engine
    /// needs to tell two identically-named units apart — guava declares
    /// `guava` twice, once per reactor. A `<packaging>pom</packaging>` module
    /// compiles nothing: it contributes its member list and no unit. Gradle
    /// says nothing here yet, and that absence is typed: no unit means the
    /// engine falls back to what each file's own declaration implies.
    ///
    /// A module is two units. Its main set names no source root: it compiles
    /// its manifest's own directory minus the test set's, because what the
    /// build adds to the main set (a plugin's generated sources, a GWT
    /// super-source) is not enumerable from the pom, and over-inclusion in
    /// main is the keep-alive direction. Its test set is what the pom STATES:
    /// its own `<testSourceDirectory>`, else the nearest ancestor's along
    /// `<parent>` (Maven's inheritance — guava's `test` directories live in
    /// the root pom), else `src/test/java` and `src/test/kotlin`; plus every
    /// directory the pom's `build-helper-maven-plugin` adds as a test source.
    /// The test set compiles against the main set and is its friend: Kotlin's
    /// `internal` is visible to it, as package-private is through the
    /// namespace span. A directory nobody checked out is a transcription, and
    /// a unit no file belongs to is harmless.
    pub fn structure(
        manifest: &SourceFile<'_>,
        cx: &kndo_contract::adapter::ResolveContext<'_>,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        let path = manifest.path.as_str();
        let file = path.rsplit('/').next().unwrap_or_default();
        let Ok(text) = std::str::from_utf8(manifest.content) else {
            return;
        };
        if file != "pom.xml" {
            // Gradle states its structure in a language, not a document: the
            // block scanner and its version catalog are their own reading.
            // What it does say plainly is read here.
            gradle_structure(manifest, text, out);
            return;
        }
        // A pom is XML and is read as XML. The shallow scanner this replaces
        // could not tell a `<dependency>` from the `<exclusion>` inside it,
        // nor a declared dependency from a `<dependencyManagement>` entry
        // nobody declared — both graded against `mvn help:effective-pom` in
        // `tests/maven.rs`.
        let Ok(doc) = roxmltree::Document::parse(text) else {
            return;
        };
        let project = doc.root_element();
        if project.tag_name().name() != "project" {
            return;
        }
        let properties = properties_of(project);
        let resolve = |raw: &str| interpolate(raw, &properties);
        maven_dependencies(project, out);
        maven_package(project, path, out);
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
        let join = |rel: &str| -> String {
            if dir.is_empty() {
                rel.to_string()
            } else {
                format!("{dir}/{rel}")
            }
        };
        for module in child(project, "modules")
            .into_iter()
            .flat_map(|m| children_named(m, "module"))
            .map(|m| text_of(m))
        {
            let module = module.trim();
            if module.is_empty() {
                continue;
            }
            let rel = if module.ends_with(".xml") {
                module.to_string()
            } else {
                format!("{module}/pom.xml")
            };
            // A transcription, not a claim that the file is there: a reactor
            // may list a module nobody checked out, and resolution matches
            // against the units that actually exist anyway.
            out.member(kndo_contract::vocab::ProjectPath::new(join(&rel)));
        }
        let packaging = child(project, "packaging").map_or("jar".to_string(), text_of);
        if packaging.trim() == "pom" {
            // An aggregator: it lists modules and compiles none of them.
            return;
        }
        let Some(name) = child(project, "artifactId")
            .map(text_of)
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
        else {
            return;
        };
        let name = name.as_str();
        let mut depends_on: Vec<SmolStr> = declared_dependencies(project)
            .into_iter()
            .filter_map(|d| child(d, "artifactId"))
            .map(|a| SmolStr::new(text_of(a).trim()))
            .filter(|a| !a.is_empty())
            .collect();
        depends_on.sort_unstable();
        depends_on.dedup();
        let mut test_roots: Vec<SmolStr> = match test_source_directory(project, path, cx, &resolve)
        {
            Some(dir) => vec![SmolStr::new(join(&dir))],
            None => vec![
                SmolStr::new(join("src/test/java")),
                SmolStr::new(join("src/test/kotlin")),
            ],
        };
        test_roots.extend(
            helper_added_test_sources(project)
                .into_iter()
                .map(|d| SmolStr::new(join(&d))),
        );
        test_roots.sort_unstable();
        test_roots.dedup();
        let mut test_depends_on = depends_on.clone();
        test_depends_on.push(SmolStr::new(name));
        test_depends_on.sort_unstable();
        test_depends_on.dedup();
        out.unit(kndo_contract::manifest::Unit {
            name: SmolStr::new(name),
            kind: kndo_contract::manifest::UnitKind::Library,
            roots: Vec::new(),
            excludes: Vec::new(),
            entries: Vec::new(),
            depends_on,
            friend_of: Vec::new(),
            publication: kndo_contract::manifest::Publication::Unstated,
        });
        out.unit(kndo_contract::manifest::Unit {
            name: SmolStr::new(format!("{name}:test")),
            kind: kndo_contract::manifest::UnitKind::Test,
            roots: test_roots,
            excludes: Vec::new(),
            entries: Vec::new(),
            depends_on: test_depends_on,
            friend_of: vec![SmolStr::new(name)],
            publication: kndo_contract::manifest::Publication::Unstated,
        });
    }

    /// The inner text of every DIRECT child of `body` named `name`, in
    /// document order. A shallow reader and not an XML parser: comments are
    /// skipped, attributes ignored, and depth is all it tracks — which is
    /// what keeps `<parent>`'s own `<artifactId>` and
    /// `<dependencyManagement>`'s `<dependencies>` out of a project's
    /// direct children, where reading them would be a bug.
    /// What a Gradle build file states plainly, until the block scanner and
    /// the version catalog read the rest (M8.d): its dependency coordinates,
    /// and — for a `settings.gradle(.kts)` — the modules it includes, whose
    /// name is positional and held by the settings file rather than by each
    /// module's own build script.
    fn gradle_structure(
        manifest: &SourceFile<'_>,
        text: &str,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        let mut declarations = gradle(text);
        declarations.sort_by(|a, b| {
            (a.name.as_str(), a.scope.map(|s| s as u8))
                .cmp(&(b.name.as_str(), b.scope.map(|s| s as u8)))
        });
        declarations.dedup();
        for declaration in declarations {
            out.dependency(declaration);
        }
        for package in gradle_packages(manifest, text) {
            out.package(package);
        }
    }

    /// A pom's own `<build><testSourceDirectory>`, else the nearest ancestor's
    /// along `<parent>` — Maven's inheritance, read through the context. A
    /// relative directory, to be joined under the INHERITING pom's own
    /// directory: `<testSourceDirectory>test-src</testSourceDirectory>` in a
    /// parent means each child's `test-src`, which is what
    /// `mvn help:effective-pom` answers for the captured reactor.
    fn test_source_directory(
        project: roxmltree::Node<'_, '_>,
        path: &str,
        cx: &kndo_contract::adapter::ResolveContext<'_>,
        resolve: &dyn Fn(&str) -> String,
    ) -> Option<String> {
        let declared = |node: roxmltree::Node<'_, '_>| -> Option<String> {
            let raw = child(node, "build")
                .and_then(|b| child(b, "testSourceDirectory"))
                .map(text_of)?;
            let raw = resolve(raw.trim());
            let raw = raw
                .strip_prefix("${project.basedir}/")
                .or_else(|| raw.strip_prefix("${basedir}/"))
                .unwrap_or(&raw)
                .trim_end_matches('/')
                .to_string();
            (!raw.is_empty() && !raw.contains("${")).then_some(raw)
        };
        if let Some(dir) = declared(project) {
            return Some(dir);
        }
        let mut at = path.to_string();
        let mut next = parent_pom(project, &at);
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        while let Some(parent) = next {
            if !seen.insert(parent.clone()) {
                return None;
            }
            let content = cx.manifest(&kndo_contract::vocab::ProjectPath::new(&parent))?;
            let text = std::str::from_utf8(content).ok()?;
            let doc = roxmltree::Document::parse(text).ok()?;
            let root = doc.root_element();
            if root.tag_name().name() != "project" {
                return None;
            }
            // The ancestor's own properties resolve the ancestor's own value:
            // a `${…}` a pom writes is answered where it is written.
            let properties = properties_of(root);
            let up = |raw: &str| interpolate(raw, &properties);
            if let Some(dir) = declared(root) {
                return Some(interpolate(&dir, &properties));
            }
            let _ = &up;
            next = parent_pom(root, &parent);
            at = parent;
        }
        let _ = at;
        None
    }

    /// The pom a `<parent>` element points at, as a project path: its
    /// `<relativePath>` when spelled (a directory means its `pom.xml`; an
    /// empty one means "only the repository", so no pom here), else Maven's
    /// default `../pom.xml`.
    fn parent_pom(project: roxmltree::Node<'_, '_>, path: &str) -> Option<String> {
        let parent = child(project, "parent")?;
        let relative = match child(parent, "relativePath").map(text_of) {
            Some(spelled) => {
                let spelled = spelled.trim().to_string();
                if spelled.is_empty() {
                    return None;
                }
                if spelled.ends_with(".xml") {
                    spelled
                } else {
                    format!("{}/pom.xml", spelled.trim_end_matches('/'))
                }
            }
            None => "../pom.xml".to_string(),
        };
        crate::join_relative(crate::parent_dir(path), &relative)
    }

    /// The directories the pom's `build-helper-maven-plugin` adds to the
    /// test set (`add-test-source` executions' `<sources>`), relative to the
    /// pom's directory.
    fn helper_added_test_sources(project: roxmltree::Node<'_, '_>) -> Vec<String> {
        let mut out = Vec::new();
        for plugin in child(project, "build")
            .into_iter()
            .flat_map(|b| child(b, "plugins"))
            .flat_map(|p| children_named(p, "plugin"))
        {
            let is_helper = child(plugin, "artifactId")
                .map(text_of)
                .is_some_and(|a| a.trim() == "build-helper-maven-plugin");
            if !is_helper {
                continue;
            }
            for execution in child(plugin, "executions")
                .into_iter()
                .flat_map(|e| children_named(e, "execution"))
            {
                let adds_tests = child(execution, "goals")
                    .into_iter()
                    .flat_map(|g| children_named(g, "goal"))
                    .any(|g| text_of(g).trim() == "add-test-source");
                if !adds_tests {
                    continue;
                }
                for source in child(execution, "configuration")
                    .into_iter()
                    .flat_map(|c| child(c, "sources"))
                    .flat_map(|s| children_named(s, "source"))
                {
                    let text = text_of(source);
                    let dir = text.trim().trim_end_matches('/');
                    if !dir.is_empty() && !dir.contains("${") {
                        out.push(dir.to_string());
                    }
                }
            }
        }
        out
    }

    /// The `<dependency>` elements a pom DECLARES: its own `<dependencies>`
    /// and never `<dependencyManagement>`'s, which state a version for a
    /// dependency somebody else declares. `mvn help:effective-pom` on the
    /// captured reactor shows the difference — `managed-only` appears because
    /// the child declares it, not because the parent manages it.
    fn declared_dependencies<'a, 'i>(
        project: roxmltree::Node<'a, 'i>,
    ) -> Vec<roxmltree::Node<'a, 'i>> {
        child(project, "dependencies")
            .into_iter()
            .flat_map(|d| children_named(d, "dependency"))
            .collect()
    }

    /// Each declared dependency under the scope its `<scope>` states, both the
    /// full coordinate and the bare artifact spelling. An `<exclusion>` is
    /// NOT one of these: it names an artifact this dependency must NOT drag
    /// in, and reading it as a declaration is the defect a document parser
    /// makes impossible.
    fn maven_dependencies(
        project: roxmltree::Node<'_, '_>,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        use kndo_contract::adapter::{DependencyDeclaration, DependencyScope};
        let mut declared: Vec<(SmolStr, Option<DependencyScope>)> = Vec::new();
        for dependency in declared_dependencies(project) {
            let Some(artifact) = child(dependency, "artifactId").map(text_of) else {
                continue;
            };
            let artifact = artifact.trim().to_string();
            if artifact.is_empty() {
                continue;
            }
            let scope = child(dependency, "scope")
                .map(text_of)
                .filter(|s| s.trim() == "test")
                .map(|_| DependencyScope::Dev);
            if let Some(group) = child(dependency, "groupId").map(text_of) {
                let group = group.trim();
                if !group.is_empty() {
                    declared.push((SmolStr::new(format!("{group}:{artifact}")), scope));
                }
            }
            declared.push((SmolStr::new(&artifact), scope));
        }
        declared.sort_by(|a, b| {
            (a.0.as_str(), a.1.map(|s| s as u8)).cmp(&(b.0.as_str(), b.1.map(|s| s as u8)))
        });
        declared.dedup();
        for (name, scope) in declared {
            out.dependency(DependencyDeclaration {
                name,
                scope,
                version_req: None,
            });
        }
    }

    /// A pom describes ITSELF: its `<artifactId>`, prefixed by `<groupId>`
    /// when the pom states one — an inherited groupId stays bare, matching the
    /// bare spelling a dependency also gets.
    fn maven_package(
        project: roxmltree::Node<'_, '_>,
        path: &str,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        let Some(artifact) = child(project, "artifactId")
            .map(text_of)
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
        else {
            return;
        };
        let name = match child(project, "groupId")
            .map(text_of)
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty())
        {
            Some(group) => format!("{group}:{artifact}"),
            None => artifact,
        };
        out.package(kndo_contract::adapter::PackageEntry {
            name: SmolStr::new(name),
            entry: None,
            dir: SmolStr::new(path.rsplit_once('/').map_or("", |(d, _)| d)),
        });
    }

    /// `<properties>` as a table, for the `${…}` a pom writes into a path.
    fn properties_of(
        project: roxmltree::Node<'_, '_>,
    ) -> std::collections::BTreeMap<String, String> {
        child(project, "properties")
            .into_iter()
            .flat_map(|p| p.children().filter(roxmltree::Node::is_element))
            .map(|e| {
                (
                    e.tag_name().name().to_string(),
                    text_of(e).trim().to_string(),
                )
            })
            .collect()
    }

    /// `${name}` replaced where the pom's own properties answer it, once —
    /// enough for the paths this reader takes, and short of Maven's full
    /// recursive interpolation, which a value it cannot resolve says by
    /// keeping its `${…}` and being refused upstream.
    fn interpolate(raw: &str, properties: &std::collections::BTreeMap<String, String>) -> String {
        let mut out = raw.to_string();
        for (key, value) in properties {
            out = out.replace(&format!("${{{key}}}"), value);
        }
        out
    }

    /// The first direct child element named `name`.
    fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
        children_named(node, name).into_iter().next()
    }

    /// Every direct child element named `name`, in document order.
    fn children_named<'a, 'i>(
        node: roxmltree::Node<'a, 'i>,
        name: &str,
    ) -> Vec<roxmltree::Node<'a, 'i>> {
        node.children()
            .filter(|c| c.is_element() && c.tag_name().name() == name)
            .collect()
    }

    /// An element's text, comments and child markup excluded.
    fn text_of(node: roxmltree::Node<'_, '_>) -> String {
        node.children()
            .filter(roxmltree::Node::is_text)
            .filter_map(|t| t.text())
            .collect()
    }

    /// The scope a gradle configuration word states: the `test*` family never
    /// ships (publish-safe), the main compile/runtime families do, and an
    /// unrecognized configuration honestly says nothing.
    fn gradle_scope(line: &str) -> Option<kndo_contract::adapter::DependencyScope> {
        use kndo_contract::adapter::DependencyScope as S;
        let word = line
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .find(|w| !w.is_empty())?;
        match word {
            "testImplementation"
            | "testCompileOnly"
            | "testRuntimeOnly"
            | "testApi"
            | "testFixturesImplementation"
            | "testFixturesApi" => Some(S::Dev),
            "implementation"
            | "api"
            | "compileOnly"
            | "runtimeOnly"
            | "annotationProcessor"
            | "kapt"
            | "ksp" => Some(S::Prod),
            _ => None,
        }
    }

    /// The modules a `settings.gradle(.kts)` includes: `include(":a", ":b")`
    /// names each one, and the directory is the module path with `:` as `/`,
    /// relative to the settings file. A module's own `build.gradle` declares
    /// no package — a Gradle module's name is positional and the settings
    /// file holds it.
    fn gradle_packages(
        manifest: &SourceFile<'_>,
        text: &str,
    ) -> Vec<kndo_contract::adapter::PackageEntry> {
        let file = manifest
            .path
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or_default();
        if !matches!(file, "settings.gradle" | "settings.gradle.kts") {
            return Vec::new();
        }
        let dir_of_manifest = manifest
            .path
            .as_str()
            .rsplit_once('/')
            .map(|(d, _)| d)
            .unwrap_or("");
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("//") || !line.contains("include") {
                continue;
            }
            for quote in ['"', '\''] {
                let mut rest = line;
                while let Some(start) = rest.find(quote) {
                    let after = &rest[start + 1..];
                    let Some(end) = after.find(quote) else { break };
                    let literal = &after[..end];
                    rest = &after[end + 1..];
                    // `include(":a")` and `include("a")` are the same
                    // declaration; the colon is optional.
                    let module = literal.strip_prefix(':').unwrap_or(literal);
                    let ok = !module.is_empty()
                        && module
                            .chars()
                            .all(|c| c.is_alphanumeric() || ".-_:".contains(c));
                    if ok {
                        let name = module.rsplit(':').next().unwrap_or(module);
                        let rel = module.replace(':', "/");
                        let dir = if dir_of_manifest.is_empty() {
                            rel
                        } else {
                            format!("{dir_of_manifest}/{rel}")
                        };
                        out.push(kndo_contract::adapter::PackageEntry {
                            name: SmolStr::new(name),
                            entry: None,
                            dir: SmolStr::new(dir),
                        });
                    }
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out.dedup_by(|a, b| a.name == b.name);
        out
    }

    /// Quoted `group:artifact[:version]` coordinates anywhere in the script — the
    /// shape every dependency notation shares (`implementation "g:a:v"`,
    /// `api('g:a')`, version catalogs excluded by their own syntax).
    pub fn gradle(text: &str) -> Vec<kndo_contract::adapter::DependencyDeclaration> {
        use kndo_contract::adapter::DependencyDeclaration;
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("//") {
                continue;
            }
            let scope = gradle_scope(line);
            for quote in ['"', '\''] {
                let mut rest = line;
                while let Some(start) = rest.find(quote) {
                    let after = &rest[start + 1..];
                    let Some(end) = after.find(quote) else {
                        break;
                    };
                    let literal = &after[..end];
                    rest = &after[end + 1..];
                    // `project(":name")` — a dependency on a workspace sibling,
                    // spelled by module path; the name is the last segment.
                    if line.contains("project(")
                        && let Some(module) = literal.strip_prefix(':')
                    {
                        let name = module.rsplit(':').next().unwrap_or(module);
                        if !name.is_empty()
                            && name
                                .chars()
                                .all(|c| c.is_alphanumeric() || ".-_".contains(c))
                        {
                            out.push(DependencyDeclaration {
                                name: SmolStr::new(name),
                                scope,
                                version_req: None,
                            });
                        }
                        continue;
                    }
                    let mut parts = literal.split(':');
                    if let (Some(g), Some(a)) = (parts.next(), parts.next()) {
                        let extra = parts.next();
                        let well_formed = !g.is_empty()
                            && !a.is_empty()
                            && parts.next().is_none()
                            && g.chars().all(|c| c.is_alphanumeric() || ".-_".contains(c))
                            && a.chars().all(|c| c.is_alphanumeric() || ".-_".contains(c))
                            && extra.is_none_or(|v| !v.is_empty());
                        if well_formed {
                            out.push(DependencyDeclaration {
                                name: SmolStr::new(format!("{g}:{a}")),
                                scope,
                                version_req: None,
                            });
                            out.push(DependencyDeclaration {
                                name: SmolStr::new(a),
                                scope,
                                version_req: None,
                            });
                        }
                    }
                }
            }
        }
        out
    }
}

/// The suffix-matching file closest to the importer: longest shared path
/// prefix, then path order — the shared mechanics of convention-directory
/// resolution (sibling modules holding the same package resolve toward the
/// importer's own tree). The suffix is the adapter's: it encodes the
/// language's file-layout convention.
pub fn nearest_suffix_match(
    suffix: &str,
    from: &kndo_contract::vocab::ProjectPath,
    cx: &kndo_contract::adapter::ResolveContext<'_>,
) -> Option<kndo_contract::vocab::ProjectPath> {
    let mut best: Option<(usize, &kndo_contract::vocab::ProjectPath)> = None;
    for candidate in cx.known_files() {
        let c = candidate.as_str();
        if !(c.ends_with(suffix)
            && (c.len() == suffix.len() || c.as_bytes()[c.len() - suffix.len() - 1] == b'/'))
        {
            continue;
        }
        let score = c
            .bytes()
            .zip(from.as_str().bytes())
            .take_while(|(x, y)| x == y)
            .count();
        let better = match &best {
            None => true,
            Some((s, b)) => score > *s || (score == *s && c < b.as_str()),
        };
        if better {
            best = Some((score, candidate));
        }
    }
    best.map(|(_, p)| p.clone())
}

/// Winnowing parameters — one concept: core pools fingerprint sets across the
/// whole graph, so every adapter must hash and window identically or clones stop
/// matching across languages.
pub const WINNOW_K: usize = 5;
pub const WINNOW_WINDOW: usize = 4;

/// The grammar knowledge a metrics walk needs, as two small functions the
/// adapter declares; everything else — the walk, leaf hashing, winnowing, the
/// line count — is mechanics and lives here once.
pub struct MetricsSpec {
    /// True when this node adds a decision path. The shared rule every adapter
    /// follows: an arm that requires a NEW predicate counts; the arm that
    /// catches the rest (default/else/`_`) does not, and value-producing
    /// null-coalescing operators (`??`, `?:`) are not control forks.
    pub is_branch: fn(&Node<'_>, &[u8]) -> bool,
    /// The token class of one LEAF node: `None` skips it (comments), `Some`
    /// hashes the class — `"id"`/`"str"`/`"num"` for the normalized families,
    /// the node's own kind for everything else.
    pub token_class: fn(&Node<'_>) -> Option<&'static str>,
}

/// Metrics over one declaration — the WHOLE node, signature included: what the
/// reader sees is what fingerprints, identically in every language.
pub fn function_metrics(
    item: Node<'_>,
    spec: &MetricsSpec,
    source: &[u8],
) -> kndo_contract::evidence::FunctionMetrics {
    let mut token_hashes: Vec<u64> = Vec::new();
    let mut cyclomatic = 1u32;
    walk(item, &mut |n| {
        if (spec.is_branch)(&n, source) {
            cyclomatic += 1;
        }
        if n.child_count() == 0
            && let Some(class) = (spec.token_class)(&n)
        {
            token_hashes.push(fnv1a(class.as_bytes()));
        }
    });
    let loc = (item.end_position().row - item.start_position().row + 1) as u32;
    kndo_contract::evidence::FunctionMetrics {
        cyclomatic,
        loc,
        token_count: token_hashes.len() as u32,
        fingerprints: winnow(&token_hashes, WINNOW_K, WINNOW_WINDOW),
    }
}

/// A source-language adapter's spec differs only in identity: suffixes, the
/// ecosystem's manifests, and the same two optional streams every built-in
/// emits (comments for suppression, metrics for duplication). One spelling —
/// an adapter that emits differently writes its own builder chain instead.
pub fn source_adapter_spec(
    coordinate: &'static str,
    version: u32,
    suffixes: &[&'static str],
    manifests: &[&'static str],
    import_cycles: kndo_contract::extension::CycleTolerance,
) -> kndo_contract::extension::ExtensionSpec {
    source_adapter_builder(coordinate, version, suffixes, manifests, import_cycles).build()
}

/// The same shared spelling, still open: an adapter that declares a capability
/// beyond it chains the declaration and builds — one place for the common
/// part, no positional parameter per capability.
pub fn source_adapter_builder(
    coordinate: &'static str,
    version: u32,
    suffixes: &[&'static str],
    manifests: &[&'static str],
    import_cycles: kndo_contract::extension::CycleTolerance,
) -> kndo_contract::extension::ExtensionSpecBuilder {
    use kndo_contract::evidence::{EvidenceStream, EvidenceStreams};
    kndo_contract::extension::ExtensionSpec::builder(coordinate, version)
        .suffixes(suffixes)
        // `Markers` and the generated rule ride together with
        // [`mark_generated`]: every source adapter reports the banner its
        // ecosystem writes, so the pairing is the builder's, not each
        // adapter's to remember.
        .emits(EvidenceStreams::of(&[
            EvidenceStream::Comments,
            EvidenceStream::Metrics,
            EvidenceStream::Markers,
        ]))
        .dispatch(vec![generated_rule()])
        .manifests(manifests)
        .import_cycles(import_cycles)
}

/// The directory holding `path` (`""` at the project root).
pub fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Joins a `/`-separated directory and a relative specifier, collapsing `.`
/// and `..`; `None` when the specifier climbs out of the project root —
/// nothing inside the project can be meant.
pub fn join_relative(dir: &str, spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// A root-relative reference (`/src/main.ts`) resolved against the nearest
/// ancestor directory of `from` under which it names a known file. The server's
/// document root is unknown to the analysis, but it contains the document, so
/// the closest ancestor holding the path is the best-evidenced root — vite
/// serves each playground app from its own directory, and that is exactly what
/// `/src/main.ts` in its `index.html` means.
pub fn nearest_rooted_match(
    from: &kndo_contract::vocab::ProjectPath,
    rooted: &str,
    cx: &kndo_contract::adapter::ResolveContext<'_>,
) -> Option<kndo_contract::vocab::ProjectPath> {
    let rel = rooted.trim_start_matches('/');
    if rel.is_empty() {
        return None;
    }
    let mut dir = parent_dir(from.as_str());
    loop {
        let candidate = if dir.is_empty() {
            rel.to_string()
        } else {
            format!("{dir}/{rel}")
        };
        let path = kndo_contract::vocab::ProjectPath::new(candidate);
        if cx.contains(&path) {
            return Some(path);
        }
        if dir.is_empty() {
            return None;
        }
        dir = parent_dir(dir);
    }
}

/// The web ecosystem's test-file convention, the one list every adapter of the
/// web tree (js-ts, html) roots by: a `__tests__`, `test` or `tests` directory
/// anywhere on the path, or a `.test.`/`.spec.` name.
pub fn web_test_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let in_dir = |d: &str| path.contains(&format!("/{d}/")) || path.starts_with(&format!("{d}/"));
    in_dir("__tests__")
        || in_dir("test")
        || in_dir("tests")
        || name.contains(".test.")
        || name.contains(".spec.")
}

/// An argument list split at DEPTH-ZERO commas, each argument trimmed with its
/// whitespace runs collapsed — the one spelling shared by every grammar whose
/// annotations, attributes and decorators carry a parenthesized list
/// (`#[serde(rename_all = "camelCase")]`, `@SuppressWarnings({"a", "b"})`).
/// Nesting and string literals are opaque, so a comma inside either does not
/// split.
pub fn split_arguments(text: &str) -> Vec<String> {
    let mut pieces: Vec<&str> = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    let mut start = 0;
    for (i, ch) in text.char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                pieces.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    pieces.push(&text[start..]);
    pieces
        .into_iter()
        .map(normalize_whitespace)
        .filter(|a| !a.is_empty())
        .collect()
}

/// Trimmed, whitespace runs outside string literals collapsed to one space.
pub fn normalize_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_str = false;
    let mut escaped = false;
    let mut pending_space = false;
    for ch in text.trim().chars() {
        if in_str {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        if ch == '"' {
            in_str = true;
        }
        out.push(ch);
    }
    out
}
