//! The adapters' paved road: helpers whose behavior is identical for a grammar they
//! have never seen. Grammar constants stay in their adapters, next to the grammar;
//! the four lines of parser scaffolding around them live here once (v1 carried five
//! verbatim copies of `parse`, three with comments defending the copy).

pub mod github_actions;

use kndo_contract::evidence::EvidenceSink;
use kndo_contract::manifest::Version;
use kndo_contract::vocab::Span;
use tree_sitter::{Language, Node, Parser, Tree};

/// Parse `source` with `language`; `None` when the grammar refuses to load or the
/// parse produces no tree (extraction then degrades through a diagnostic).
pub fn parse(language: &Language, source: &[u8]) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(language).ok()?;
    parser.parse(source, None)
}

/// The whole parse-and-report opening every extraction shares — and the ONE
/// place a reader states what it could not read.
///
/// This used to end in a diagnostic: "syntax errors in file — evidence may be
/// partial". True, and prose, which no analysis can act on. Meanwhile `unused`
/// takes its entire basis from absence, so a file the reader only half read is
/// the one place absence means nothing — and the engine had no way to hear it.
/// It reports [`UnreadName`] now, which is a fact, and the diagnostic is gone.
///
/// **Only two nodes are asked what their children leave uncovered**: an
/// `ERROR`, which is the parser saying it could not place this text, and the
/// root, where recovery can drop a whole top-level item. Inside a node the
/// parser BUILT, uncovered bytes are that node's own text and the reader read
/// them — see [`visit`], which is the whole rule.
///
/// Measured on the corpus: Exposed loses four accusations, and all four are
/// false positives the reader could not see it was making —
/// `allReferencesMatch` is called twice from a `when` guard (Kotlin 2.1, a
/// construct the pinned grammar cannot read), `isPersistedIn` from a body past
/// a break, `RollbackCheckInterceptor` from three lines the recovery mislexed.
/// The engine now abstains over exactly those names instead of accusing them,
/// which is the whole point: a grammar gap becomes an abstention rather than a
/// false verdict, and no patch to the grammar was needed to get there.
///
/// Two narrower rules came first and each cost a measurement. Everything from
/// the first `ERROR` to end of file withheld 49 of vapor's declarations, all of
/// them swift-testing methods the item walk had RECOVERED from under one — so a
/// subtree under an `ERROR` is placed, and so is an `extra`, which the grammar
/// itself declares placeless. Asking every node what its leaves leave uncovered
/// read the inside of tokens as unread: ripgrep 19393 names, of which 18658
/// were the words in its comments (`tree-sitter-rust` gives a `line_comment`'s
/// text no node), 735 the `r` of a raw string; python 197, all docstring prose;
/// css 470, the digits of its colours. Under the rule above those are 0, 0 and
/// 15, the corpus reports are byte-identical, and what is left in every
/// language is code its grammar could not read.
///
/// What this does not reach is text a leaf covers but MISREADS — a
/// `string_content` absorbing twelve lines of statements, measured in Exposed's
/// `StatementInterceptorTests.kt`. Open, and named rather than approximated;
/// in that file the loose tokens beside the mislexed leaf already withhold the
/// two subjects it would have withheld.
pub fn parse_reporting(
    language: &Language,
    source: &[u8],
    out: &mut kndo_contract::evidence::EvidenceSink,
) -> Option<Tree> {
    let Some(tree) = parse(language, source) else {
        // No tree at all: nothing in this file was read.
        names_in(source, 0, source.len(), out);
        return None;
    };
    unread(&tree, source, out);
    Some(tree)
}

/// The names in text this parse did not account for — see
/// [`parse_reporting`] for what that means and why.
fn unread(tree: &Tree, source: &[u8], out: &mut kndo_contract::evidence::EvidenceSink) {
    let mut gaps: Vec<(usize, usize)> = Vec::new();
    let root = tree.root_node();
    visit(root, true, &mut gaps);
    // Text past the tree's own end is text no node covers.
    if root.end_byte() < source.len() {
        gaps.push((root.end_byte(), source.len()));
    }
    // Document order: a reader of the evidence reads spans in the order the
    // file has them.
    gaps.sort_unstable();
    for (from, to) in gaps {
        names_in(source, from, to, out);
    }
}

/// The stretches of `n` that no child of it accounts for — see
/// [`parse_reporting`] for which nodes are asked and why.
fn visit(n: Node<'_>, is_root: bool, gaps: &mut Vec<(usize, usize)>) {
    if n.child_count() == 0 {
        return;
    }
    let mut cursor = n.walk();
    // Only two nodes are asked what their children leave uncovered: an `ERROR`,
    // which is the parser saying it could not place this text, and the root,
    // where recovery can drop a whole top-level item. Inside any node the
    // parser BUILT, uncovered bytes are the node's own text — the digits of a
    // colour, the `r` of a raw string, the prose in a docstring — and the
    // reader read them.
    if n.is_error() || is_root {
        let mut placed_to = n.start_byte();
        for child in n.children(&mut cursor) {
            // Under an `ERROR`, a bare token is exactly what was NOT placed;
            // a subtree the parser built is read by the item walk, and an
            // `extra` is placeless by the grammar's own declaration. Under the
            // root there is no such distinction: a token at top level is a
            // token the parser put there.
            let placed = child.child_count() != 0
                || !n.is_error()
                || (child.is_extra() && !child.is_error());
            if !placed {
                continue;
            }
            if child.start_byte() > placed_to {
                gaps.push((placed_to, child.start_byte()));
            }
            placed_to = placed_to.max(child.end_byte());
        }
        if placed_to < n.end_byte() {
            gaps.push((placed_to, n.end_byte()));
        }
    }
    let mut cursor = n.walk();
    for child in n.children(&mut cursor) {
        visit(child, false, gaps);
    }
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

/// Every item a container holds, INCLUDING the ones a syntax error swallowed.
///
/// tree-sitter recovers from a construct it cannot parse by wrapping it — and,
/// routinely, everything after it — in an `ERROR` node. A walk over named
/// children then stops at that node and the declarations on the far side of the
/// break are simply not there: one `when` guard or one `get` used as an infix
/// name costs a Kotlin file every declaration it holds, the run finds no root
/// anywhere, and every colour question is abstained on. Measured on Exposed
/// under kotlin-ng 1.1.0: 61 of 802 files parse with an error, 81 declarations
/// sit under one, and 7 files yield nothing at all while holding something.
///
/// So an `ERROR` is descended INTO rather than skipped: its own named children
/// take its place, recursively. What comes out is fragments — a caller's
/// `match` on the kind ignores what it does not recognise, exactly as it
/// ignores every other kind it has no rule for — and what a fragment IS, the
/// grammar still says.
pub fn items_tolerant<'t>(container: Node<'t>, lift: &[&str]) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut cursor = container.walk();
    for child in container.named_children(&mut cursor) {
        match child.is_error() {
            true => out.extend(
                items_tolerant(child, lift)
                    .into_iter()
                    .filter(|n| lift.contains(&n.kind())),
            ),
            false => out.push(child),
        }
    }
    out
}

/// Identifier-shaped runs of `source[from..to]`, reported as names the reader
/// could not account for. A byte above ASCII counts as part of a name: every
/// language here admits some Unicode in identifiers, and over-reading a name
/// only ever widens a doubt, never an accusation.
fn names_in(source: &[u8], from: usize, to: usize, out: &mut EvidenceSink) {
    let starts = |b: u8| b.is_ascii_alphabetic() || b == b'_' || b >= 0x80;
    let continues = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80;
    let mut i = from;
    while i < to {
        // A run whose first byte continues the token before it is that token's
        // tail, not a name: `1abc` names nothing.
        if !starts(source[i]) || (i > 0 && continues(source[i - 1])) {
            i += 1;
            continue;
        }
        let mut end = i;
        while end < to && continues(source[end]) {
            end += 1;
        }
        if let Ok(name) = std::str::from_utf8(&source[i..end]) {
            out.unread(name, Span::new(i as u32, end as u32));
        }
        i = end;
    }
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
pub const GENERATED_NEEDLES: &[&str] = &[
    "@generated",
    "Code generated",
    "DO NOT EDIT",
    "Generated by",
];

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
/// what the word means is [`kndo_contract::plugin::Effect::Generated`],
/// carried as a rule in the spec — so no adapter decides that a file is
/// beyond judgment.
pub const GENERATED_MARKER: &str = "generated";

/// The default rule every marker-scanning language carries: the banner it
/// reports means the generator owns the file.
pub fn generated_rule() -> kndo_contract::plugin::DispatchRule {
    kndo_contract::plugin::DispatchRule {
        when: kndo_contract::plugin::Trigger::marker(GENERATED_MARKER),
        then: kndo_contract::plugin::Effect::Generated,
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

    /// One JVM dependency, at the rung a classpath grants. Both readers use
    /// it, because both build systems put the whole compile path on ONE
    /// classpath, where a package is a name every unit on it contributes to —
    /// so the grant is a fact about the EDGE, and the same edge covers
    /// Kotlin's `internal` (`Rung::Unit` is wider, so `Namespace` carries it).
    pub fn on_the_classpath(
        unit: impl Into<smol_str::SmolStr>,
    ) -> kndo_contract::manifest::UnitDep {
        kndo_contract::manifest::UnitDep::granting(unit, kndo_contract::manifest::Grant::Namespace)
    }

    use kndo_contract::adapter::SourceFile;
    use smol_str::SmolStr;

    /// A JVM language's spec differs only in identity — the general fact
    /// ([`crate::source_adapter_spec`]) plus the one build system's manifests.
    pub fn jvm_spec(
        coordinate: &'static str,
        version: u32,
        suffixes: &[&'static str],
    ) -> kndo_contract::plugin::PluginSpec {
        jvm_builder(coordinate, version, suffixes).build()
    }

    /// The same shared spelling, still open: a JVM adapter that declares a
    /// capability beyond it (markers and what they mean) chains and builds.
    pub fn jvm_builder(
        coordinate: &'static str,
        version: u32,
        suffixes: &[&'static str],
    ) -> kndo_contract::plugin::PluginSpecBuilder {
        crate::source_adapter_builder(
            coordinate,
            version,
            suffixes,
            MANIFEST_GLOBS,
            // JVM compilers resolve reference cycles in multiple passes —
            // routine structure, never an initialization hazard worth a finding.
            kndo_contract::plugin::CycleTolerance::Tolerated,
        )
    }

    /// The rule for a lint suppression naming the unused-code check: the author
    /// already answered the question this analysis asks, so the declaration is
    /// exempt and says so as a keeper. The argument pattern matches the
    /// quoted name wherever it sits, alone (`@SuppressWarnings("unused")`) or
    /// inside a brace initializer (`{"unused", "rawtypes"}`).
    pub fn suppresses_unused(marker: &'static str) -> kndo_contract::plugin::DispatchRule {
        kndo_contract::plugin::DispatchRule {
            when: kndo_contract::plugin::Trigger::marker_with(marker, "*\"unused\"*"),
            then: kndo_contract::plugin::Effect::Exempt,
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
        // Gradle's version catalog: a build script names a dependency through
        // an alias (`libs.junit.core`) and this file is where the alias has a
        // coordinate. Read as data for the scripts beside it, never a unit of
        // its own.
        "**/gradle/libs.versions.toml",
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
            gradle_structure(manifest, text, cx, out);
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
        let mut test_roots: Vec<SmolStr> =
            match source_directory(project, path, cx, &resolve, "testSourceDirectory") {
                Some(dir) => vec![SmolStr::new(join(&dir))],
                None => vec![
                    SmolStr::new(join("src/test/java")),
                    SmolStr::new(join("src/test/kotlin")),
                ],
            };
        test_roots.extend(
            helper_added_sources(project, "add-test-source")
                .into_iter()
                .map(|d| SmolStr::new(join(&d))),
        );
        test_roots.sort_unstable();
        test_roots.dedup();
        let mut test_depends_on = depends_on.clone();
        test_depends_on.push(SmolStr::new(name));
        test_depends_on.sort_unstable();
        test_depends_on.dedup();
        // What the pom SAYS it compiles, where it says it — the same inheritance
        // the test side already reads. Unstated, the unit compiles the
        // manifest's own directory: Maven's default is `src/main/java`, and a
        // JVM tree routinely holds a second language's source set beside it
        // that its own plugin compiles, so the narrower reading would drop
        // files the build really builds. Where it IS stated, the tree outside
        // it is a different build's — guava's `guava-gwt/src-super` replaces
        // library files at GWT compile time and is never compiled beside them,
        // so keeping it in the unit resolves names ACROSS two variants of one
        // library.
        // Narrowing is safe only when every statement the pom makes about its
        // main sources is read: `<sourceDirectory>` AND what the build helper
        // adds with `add-source`, the twin of the `add-test-source` the test
        // side already reads. A pom that states neither compiles its own
        // directory, whole.
        let mut main_roots: Vec<SmolStr> = Vec::new();
        if let Some(dir) = source_directory(project, path, cx, &resolve, "sourceDirectory") {
            main_roots.push(SmolStr::new(join(&dir)));
            main_roots.extend(
                helper_added_sources(project, "add-source")
                    .into_iter()
                    .map(|d| SmolStr::new(join(&d))),
            );
            main_roots.sort_unstable();
            main_roots.dedup();
        }
        out.unit(kndo_contract::manifest::Unit {
            name: SmolStr::new(name),
            kind: kndo_contract::manifest::UnitKind::Library,
            roots: main_roots
                .into_iter()
                .map(kndo_contract::manifest::UnitRoot::from)
                .collect(),
            excludes: Vec::new(),
            entries: Vec::new(),
            depends_on: depends_on.into_iter().map(on_the_classpath).collect(),
            // A jar hands out every `public` class of every package it holds:
            // a consumer writes `com.google.common.io.Files`, never a file
            // this manifest mapped.
            publication: kndo_contract::manifest::Publication::ByName,
            namespace_root: None,
        });
        out.unit(kndo_contract::manifest::Unit {
            name: SmolStr::new(format!("{name}:test")),
            kind: kndo_contract::manifest::UnitKind::Test,
            roots: test_roots
                .into_iter()
                .map(kndo_contract::manifest::UnitRoot::from)
                .collect(),
            excludes: Vec::new(),
            entries: Vec::new(),
            // ONE classpath: javac puts every dependency on it, and a package
            // is a name every unit on it contributes to — so a dependent
            // declaring `com.google.common.io` names that package's
            // package-private members, whether it is the module's own test set
            // (Surefire) or a separate artifact (guava-tests). The module's
            // OWN main set is more than that: Surefire compiles the test
            // sources as an ASSOCIATED compilation, which reaches the unit
            // rung too. Being on a classpath is not being inside the module.
            depends_on: test_depends_on
                .into_iter()
                .map(|d| match d == name {
                    true => kndo_contract::manifest::UnitDep::friend(d),
                    false => on_the_classpath(d),
                })
                .collect(),
            publication: kndo_contract::manifest::Publication::Unstated,
            namespace_root: None,
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
        cx: &kndo_contract::adapter::ResolveContext<'_>,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        let path = manifest.path.as_str();
        let file = path.rsplit('/').next().unwrap_or_default();
        // A comment is not code and a string is not syntax: everything below
        // reads a copy with the comments blanked, so `// include("x")` names
        // nothing and a `//` inside a URL is not a comment.
        let code = blank_gradle_comments(text);
        match file {
            "settings.gradle" | "settings.gradle.kts" => settings_structure(path, &code, cx, out),
            "build.gradle" | "build.gradle.kts" => {
                build_script_structure(path, &code, cx, out);
            }
            // The catalog is data for the scripts beside it, never a unit.
            _ => {}
        }
    }

    /// What a `settings.gradle(.kts)` states: every module the build includes,
    /// as a package and as a member manifest. `include` takes any number of
    /// arguments across any number of lines, which is why the call's
    /// PARENTHESES bound the reading rather than the line — `gradle` resolves
    /// the captured build's `include(\n    "app",\n)` to a project, and a
    /// line scanner does not.
    fn settings_structure(
        path: &str,
        code: &str,
        cx: &kndo_contract::adapter::ResolveContext<'_>,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
        let join = |rel: &str| -> String {
            if dir.is_empty() {
                rel.to_string()
            } else {
                format!("{dir}/{rel}")
            }
        };
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for arguments in calls_named(code, "include") {
            for literal in string_literals(arguments) {
                let module = literal.trim().trim_start_matches(':');
                if module.is_empty()
                    || !module
                        .chars()
                        .all(|c| c.is_alphanumeric() || ".-_:".contains(c))
                {
                    continue;
                }
                if !seen.insert(module.to_string()) {
                    continue;
                }
                let rel = module.replace(':', "/");
                let name = module.rsplit(':').next().unwrap_or(module);
                out.package(kndo_contract::adapter::PackageEntry {
                    name: SmolStr::new(name),
                    entry: None,
                    dir: SmolStr::new(join(&rel)),
                    aliases: Vec::new(),
                    subpaths: Vec::new(),
                });
                // A module's own build script states its units; naming it here
                // is how two same-named modules in different builds stay apart.
                // The one spelling the tree HAS: naming both put a script that
                // does not exist in the aggregation, and the context can say
                // which of the two is there.
                for script in ["build.gradle.kts", "build.gradle"] {
                    let member =
                        kndo_contract::vocab::ProjectPath::new(join(&format!("{rel}/{script}")));
                    if cx.manifest(&member).is_some() {
                        out.member(member);
                    }
                }
            }
        }
    }

    /// What a module's `build.gradle(.kts)` states: the two units Gradle's
    /// java plugin gives it, and what each compiles against.
    ///
    /// The main set is the module's directory minus its test tree, because a
    /// plugin may add sources this file never names and over-inclusion in main
    /// is the keep-alive direction; the test set is the layout Gradle
    /// answers, plus any `srcDirs` the script declares for it. The test set is
    /// the main set's FRIEND: Kotlin's `internal` is visible from a module's
    /// own tests, which is the whole reason this reader exists.
    fn build_script_structure(
        path: &str,
        code: &str,
        cx: &kndo_contract::adapter::ResolveContext<'_>,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        use kndo_contract::adapter::{DependencyDeclaration, DependencyScope};
        use kndo_contract::manifest::{Publication, Unit, UnitDep, UnitKind, UnitRoot};
        let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
        let join = |rel: &str| -> SmolStr {
            if dir.is_empty() {
                SmolStr::new(rel)
            } else {
                SmolStr::new(format!("{dir}/{rel}"))
            }
        };
        let name = if dir.is_empty() {
            // The root build script of a single-module build: the directory
            // has no name to take, so the unit is the project itself.
            "root".to_string()
        } else {
            dir.rsplit('/').next().unwrap_or(dir).to_string()
        };
        let catalog = version_catalog(path, cx);

        let mut depends_on: Vec<SmolStr> = Vec::new();
        let mut test_depends_on: Vec<SmolStr> = Vec::new();
        let mut declared: Vec<(SmolStr, Option<DependencyScope>)> = Vec::new();
        for (configuration, argument) in dependency_declarations(code) {
            let scope = gradle_scope(&configuration);
            let test = scope == Some(DependencyScope::Dev);
            // `project(":core")` names a module of this same build, which is a
            // UNIT rather than an artifact: the engine resolves the name among
            // the members the settings file listed.
            if let Some(inner) = call_arguments(&argument, "project") {
                for literal in string_literals(&inner) {
                    let module = literal.trim().trim_start_matches(':');
                    let named = SmolStr::new(module.rsplit(':').next().unwrap_or(module));
                    if test {
                        test_depends_on.push(named);
                    } else {
                        depends_on.push(named);
                    }
                }
                continue;
            }
            for coordinate in gradle_coordinates(&argument, &catalog) {
                if coordinate.contains(':') {
                    declared.push((SmolStr::new(coordinate), scope));
                }
            }
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

        let mut test_roots: Vec<SmolStr> = declared_src_dirs(code, "test")
            .into_iter()
            .map(|d| join(&d))
            .collect();
        if test_roots.is_empty() {
            test_roots = vec![join("src/test/java"), join("src/test/kotlin")];
        }
        test_roots.sort_unstable();
        test_roots.dedup();
        let main_roots: Vec<SmolStr> = declared_src_dirs(code, "main")
            .into_iter()
            .map(|d| join(&d))
            .collect();
        depends_on.sort_unstable();
        depends_on.dedup();
        test_depends_on.extend(depends_on.iter().cloned());
        test_depends_on.push(SmolStr::new(&name));
        test_depends_on.sort_unstable();
        test_depends_on.dedup();
        out.unit(Unit {
            name: SmolStr::new(&name),
            kind: UnitKind::Library,
            roots: main_roots.into_iter().map(UnitRoot::from).collect(),
            excludes: test_roots.clone(),
            entries: Vec::new(),
            depends_on: depends_on.into_iter().map(on_the_classpath).collect(),
            publication: Publication::ByName,
            namespace_root: None,
        });
        out.unit(Unit {
            name: SmolStr::new(format!("{name}:test")),
            kind: UnitKind::Test,
            roots: test_roots.into_iter().map(UnitRoot::from).collect(),
            excludes: Vec::new(),
            entries: Vec::new(),
            // The same classpath rule as Maven's, plus Kotlin's `internal`:
            // Gradle compiles the test set against the MAIN one as an
            // associated compilation, which reaches the unit rung; every other
            // dependency is on the classpath and no more, because a module's
            // `internal` is not a dependent's to name.
            depends_on: test_depends_on
                .into_iter()
                .map(|d| match d == name {
                    true => UnitDep::friend(d),
                    false => on_the_classpath(d),
                })
                .collect(),
            publication: Publication::Unpublished,
            namespace_root: None,
        });
    }

    /// `gradle/libs.versions.toml` beside the build, as alias → coordinate.
    /// Gradle's default catalog is `libs`, found at the SETTINGS file's
    /// directory, so a module's script and the root's read the same one.
    fn version_catalog(
        path: &str,
        cx: &kndo_contract::adapter::ResolveContext<'_>,
    ) -> std::collections::BTreeMap<String, String> {
        let mut at = crate::parent_dir(path).to_string();
        loop {
            let candidate = if at.is_empty() {
                "gradle/libs.versions.toml".to_string()
            } else {
                format!("{at}/gradle/libs.versions.toml")
            };
            if let Some(content) = cx.manifest(&kndo_contract::vocab::ProjectPath::new(&candidate))
                && let Ok(text) = std::str::from_utf8(content)
            {
                return catalog_libraries(text);
            }
            if at.is_empty() {
                return std::collections::BTreeMap::new();
            }
            at = crate::parent_dir(&at).to_string();
        }
    }

    /// `[libraries]` as alias → `group:artifact`. Both spellings a catalog
    /// allows are read: `module = "g:a"`, and `group`/`name` as separate keys.
    /// The alias is normalized the way Gradle's accessors spell it — `-` and
    /// `_` become the `.` of `libs.junit.core`.
    fn catalog_libraries(text: &str) -> std::collections::BTreeMap<String, String> {
        let Ok(root) = text.parse::<toml::Value>() else {
            return std::collections::BTreeMap::new();
        };
        let Some(libraries) = root.get("libraries").and_then(toml::Value::as_table) else {
            return std::collections::BTreeMap::new();
        };
        libraries
            .iter()
            .filter_map(|(alias, value)| {
                let coordinate = match value {
                    toml::Value::String(s) => s.split(':').take(2).collect::<Vec<_>>().join(":"),
                    _ => {
                        if let Some(module) = value.get("module").and_then(toml::Value::as_str) {
                            module.split(':').take(2).collect::<Vec<_>>().join(":")
                        } else {
                            let group = value.get("group").and_then(toml::Value::as_str)?;
                            let name = value.get("name").and_then(toml::Value::as_str)?;
                            format!("{group}:{name}")
                        }
                    }
                };
                Some((alias.replace(['-', '_'], "."), coordinate))
            })
            .collect()
    }

    // ------------------------------------------------- the Gradle block scanner

    /// A copy of `text` with every comment replaced by spaces, so offsets are
    /// preserved and `// include("x")` names nothing. String literals are
    /// opaque: `//` inside one is not a comment, which is what a URL in a
    /// repository declaration needs. Kotlin and Groovy agree on all three
    /// forms this reads — `//`, `/* */`, and `"""…"""`.
    fn blank_gradle_comments(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out: Vec<u8> = bytes.to_vec();
        let mut i = 0usize;
        let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
            for byte in out.iter_mut().take(to).skip(from) {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
        };
        while i < bytes.len() {
            match bytes[i] {
                b'/' if bytes.get(i + 1) == Some(&b'/') => {
                    let end = text[i..].find('\n').map_or(bytes.len(), |e| i + e);
                    blank(&mut out, i, end);
                    i = end;
                }
                b'/' if bytes.get(i + 1) == Some(&b'*') => {
                    let end = text[i + 2..]
                        .find("*/")
                        .map_or(bytes.len(), |e| i + 2 + e + 2);
                    blank(&mut out, i, end);
                    i = end;
                }
                b'"' if text[i..].starts_with("\"\"\"") => {
                    let end = text[i + 3..]
                        .find("\"\"\"")
                        .map_or(bytes.len(), |e| i + 3 + e + 3);
                    i = end;
                }
                quote @ (b'"' | b'\'') => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != quote {
                        i += if bytes[i] == b'\\' { 2 } else { 1 };
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        }
        String::from_utf8(out).unwrap_or_else(|_| text.to_string())
    }

    /// The ARGUMENT TEXT of every `name(...)` call, parentheses balanced and
    /// newlines crossed — `include(\n    "app",\n)` is one call with one
    /// argument, which is what Gradle resolves and a line reader misses.
    fn calls_named<'a>(code: &'a str, name: &str) -> Vec<&'a str> {
        let mut out = Vec::new();
        let bytes = code.as_bytes();
        let mut from = 0usize;
        while let Some(at) = code[from..].find(name) {
            let at = from + at;
            from = at + name.len();
            let before_is_word = at > 0
                && (bytes[at - 1].is_ascii_alphanumeric()
                    || bytes[at - 1] == b'_'
                    || bytes[at - 1] == b'.');
            if before_is_word {
                continue;
            }
            let mut cursor = at + name.len();
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if bytes.get(cursor) != Some(&b'(') {
                continue;
            }
            if let Some(end) = balanced(code, cursor, b'(', b')') {
                out.push(&code[cursor + 1..end]);
                from = end;
            }
        }
        out
    }

    /// The argument text of a `name(...)` call anywhere inside `code`, or
    /// `None` where the call is not there.
    fn call_arguments(code: &str, name: &str) -> Option<String> {
        calls_named(code, name).first().map(|s| (*s).to_string())
    }

    /// The index of the delimiter closing the one at `open`, string literals
    /// skipped so a bracket inside a name never closes a call.
    fn balanced(code: &str, open: usize, opening: u8, closing: u8) -> Option<usize> {
        let bytes = code.as_bytes();
        let mut depth = 0usize;
        let mut i = open;
        while i < bytes.len() {
            match bytes[i] {
                quote @ (b'"' | b'\'') => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != quote {
                        i += if bytes[i] == b'\\' { 2 } else { 1 };
                    }
                }
                b if b == opening => depth += 1,
                b if b == closing => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// Every string literal in `code`, single or double quoted.
    fn string_literals(code: &str) -> Vec<&str> {
        let bytes = code.as_bytes();
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < bytes.len() {
            let quote = bytes[i];
            if quote != b'"' && quote != b'\'' {
                i += 1;
                continue;
            }
            let start = i + 1;
            i = start;
            while i < bytes.len() && bytes[i] != quote {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            if i <= bytes.len() {
                out.push(&code[start..i.min(code.len())]);
            }
            i += 1;
        }
        out
    }

    /// `configuration(argument)` pairs inside the script's `dependencies { … }`
    /// blocks. The BLOCK bounds it — a coordinate-shaped string in a
    /// `repositories` or `publishing` block is not a dependency — and the
    /// configuration word carries the scope.
    fn dependency_declarations(code: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut from = 0usize;
        while let Some(at) = code[from..].find("dependencies") {
            let at = from + at;
            from = at + "dependencies".len();
            let mut cursor = from;
            let bytes = code.as_bytes();
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if bytes.get(cursor) != Some(&b'{') {
                continue;
            }
            let Some(end) = balanced(code, cursor, b'{', b'}') else {
                continue;
            };
            let block = &code[cursor + 1..end];
            from = end;
            for line in block.lines() {
                let line = line.trim();
                let Some(paren) = line.find('(') else {
                    // Groovy's parenless form: `implementation "g:a:v"`.
                    let Some((word, rest)) = line.split_once(char::is_whitespace) else {
                        continue;
                    };
                    if is_configuration_word(word) {
                        out.push((word.to_string(), rest.trim().to_string()));
                    }
                    continue;
                };
                let word = line[..paren].trim();
                if !is_configuration_word(word) {
                    continue;
                }
                let Some(close) = balanced(line, paren, b'(', b')') else {
                    continue;
                };
                out.push((word.to_string(), line[paren + 1..close].to_string()));
            }
        }
        out
    }

    /// A word shaped like a Gradle configuration: letters only, and not one of
    /// the block names a dependency line never starts with.
    fn is_configuration_word(word: &str) -> bool {
        !word.is_empty()
            && word.chars().all(|c| c.is_ascii_alphanumeric())
            && !matches!(word, "if" | "else" | "for" | "return" | "val" | "var")
    }

    /// The coordinates one dependency argument names: a quoted
    /// `group:artifact[:version]`, or a `libs.a.b` alias the catalog answers.
    /// An alias Gradle would resolve and this cannot yields nothing rather
    /// than a guess.
    fn gradle_coordinates(
        argument: &str,
        catalog: &std::collections::BTreeMap<String, String>,
    ) -> Vec<String> {
        let mut out: Vec<String> = string_literals(argument)
            .into_iter()
            .filter(|s| s.contains(':') && !s.starts_with(':'))
            .map(|s| s.split(':').take(2).collect::<Vec<_>>().join(":"))
            .collect();
        for accessor in catalog_accessors(argument) {
            if let Some(coordinate) = catalog.get(&accessor) {
                out.push(coordinate.clone());
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// `libs.junit.core` → `junit.core`: the alias half of a catalog accessor,
    /// wherever it sits in the argument.
    fn catalog_accessors(argument: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (at, _) in argument.match_indices("libs.") {
            let before_is_word = at > 0
                && (argument.as_bytes()[at - 1].is_ascii_alphanumeric()
                    || argument.as_bytes()[at - 1] == b'_'
                    || argument.as_bytes()[at - 1] == b'.');
            if before_is_word {
                continue;
            }
            let rest = &argument[at + "libs.".len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_'))
                .unwrap_or(rest.len());
            let alias = rest[..end].trim_end_matches('.');
            if !alias.is_empty() {
                out.push(alias.to_string());
            }
        }
        out
    }

    /// The `srcDirs(…)` a script declares for one source set, relative to the
    /// module's own directory: `sourceSets { main { java { srcDirs(…) } } }`
    /// and the Kotlin spelling beside it. Nothing declared is not an empty
    /// list — the caller falls back to Gradle's own layout.
    fn declared_src_dirs(code: &str, set: &str) -> Vec<String> {
        let mut out = Vec::new();
        for source_sets in block_bodies(code, "sourceSets") {
            for body in block_bodies(&source_sets, set) {
                for language in ["java", "kotlin", "resources"] {
                    for language_body in block_bodies(&body, language) {
                        for arguments in calls_named(&language_body, "srcDirs") {
                            for literal in string_literals(arguments) {
                                let dir = literal.trim().trim_end_matches('/');
                                if !dir.is_empty() && !dir.contains("${") {
                                    out.push(dir.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// The BODY of every `name { … }` block, braces balanced.
    fn block_bodies(code: &str, name: &str) -> Vec<String> {
        let mut out = Vec::new();
        let bytes = code.as_bytes();
        let mut from = 0usize;
        while let Some(at) = code[from..].find(name) {
            let at = from + at;
            from = at + name.len();
            let before_is_word = at > 0
                && (bytes[at - 1].is_ascii_alphanumeric()
                    || bytes[at - 1] == b'_'
                    || bytes[at - 1] == b'.');
            if before_is_word {
                continue;
            }
            let mut cursor = from;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if bytes.get(cursor) != Some(&b'{') {
                continue;
            }
            if let Some(end) = balanced(code, cursor, b'{', b'}') {
                out.push(code[cursor + 1..end].to_string());
                from = end;
            }
        }
        out
    }

    /// A pom's own `<build><`*tag*`>`, else the nearest ancestor's along
    /// `<parent>` — Maven's inheritance, read through the context. A relative
    /// directory, to be joined under the INHERITING pom's own directory:
    /// `<testSourceDirectory>test-src</testSourceDirectory>` in a parent means
    /// each child's `test-src`, which is what `mvn help:effective-pom` answers
    /// for the captured reactor. `tag` is `sourceDirectory` or
    /// `testSourceDirectory` — one walk, because Maven inherits both the same
    /// way and two copies of it could disagree.
    fn source_directory(
        project: roxmltree::Node<'_, '_>,
        path: &str,
        cx: &kndo_contract::adapter::ResolveContext<'_>,
        resolve: &dyn Fn(&str) -> String,
        tag: &str,
    ) -> Option<String> {
        let declared = |node: roxmltree::Node<'_, '_>| -> Option<String> {
            let raw = child(node, "build")
                .and_then(|b| child(b, tag))
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
    fn helper_added_sources(project: roxmltree::Node<'_, '_>, goal: &str) -> Vec<String> {
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
                let adds = child(execution, "goals")
                    .into_iter()
                    .flat_map(|g| children_named(g, "goal"))
                    .any(|g| text_of(g).trim() == goal);
                if !adds {
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
            let group = child(dependency, "groupId").map(text_of);
            let group = group.as_deref().map(str::trim).unwrap_or_default();
            declared.push(match group.is_empty() {
                true => (SmolStr::new(&artifact), scope),
                false => (SmolStr::new(format!("{group}:{artifact}")), scope),
            });
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
            aliases: Vec::new(),
            subpaths: Vec::new(),
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
}

/// Resolution by the NAMESPACE a dotted specifier names, for the languages
/// whose imports name one rather than a file — `import com.foo.Bar;`,
/// `import a.b.Foo`. The package is a clause its files declare
/// ([`kndo_contract::plugin::Nesting::ByUnit`]), so the files that answer are
/// the files that wrote it, and the import's binding picks the name among
/// them.
///
/// Which prefix of the dotted name IS the package cannot be read off the
/// spelling — `com.foo.Bar` names a type in `com.foo`, `com.foo.*` names the
/// package, `com.foo.Bar.Baz` is a nested type in the same package as its
/// outer — so the longest DECLARED prefix wins: the package that exists is the
/// package that was meant. A name no file in the project declares stays
/// unresolved, which for the JVM is every third-party import: no reliable
/// package-to-coordinate mapping exists without resolving a classpath.
///
/// Shared because it holds for a grammar it has never seen: nothing here reads
/// a path, a suffix or a directory layout.
pub fn resolve_in_namespace(
    from: &kndo_contract::vocab::ProjectPath,
    specifier: &str,
    cx: &kndo_contract::adapter::ResolveContext<'_>,
) -> kndo_contract::adapter::Resolution {
    use kndo_contract::adapter::Resolution;
    let Some(project) = cx.project() else {
        return Resolution::Unresolved;
    };
    let segments: Vec<smol_str::SmolStr> =
        specifier.split('.').map(smol_str::SmolStr::new).collect();
    for take in (1..=segments.len()).rev() {
        let files = project.files_in_namespace(from, &segments[..take]);
        if !files.is_empty() {
            return Resolution::Files(files);
        }
    }
    Resolution::Unresolved
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
    import_cycles: kndo_contract::plugin::CycleTolerance,
) -> kndo_contract::plugin::PluginSpec {
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
    import_cycles: kndo_contract::plugin::CycleTolerance,
) -> kndo_contract::plugin::PluginSpecBuilder {
    use kndo_contract::evidence::{EvidenceStream, EvidenceStreams};
    kndo_contract::plugin::PluginSpec::builder(coordinate, version)
        .suffixes(suffixes)
        // `Markers` and the generated rule ride together with
        // [`mark_generated`]: every source adapter reports the banner its
        // ecosystem writes, so the pairing is the builder's, not each
        // adapter's to remember.
        .emits(EvidenceStreams::of(&[
            EvidenceStream::Comments,
            EvidenceStream::Metrics,
            EvidenceStream::Markers,
            // `parse_reporting` reports it, so every adapter built here
            // declares it: a reader that has not promised to look for text it
            // could not account for cannot report having found none.
            EvidenceStream::UnreadText,
        ]))
        .dispatch(vec![generated_rule()])
        .manifests(manifests)
        .import_cycles(import_cycles)
}

/// The (kind, field) pairs whose subtree BINDS a name rather than uses one.
///
/// Naming a parent KIND is the coarse form, and it is wrong wherever the node
/// also holds a value: `a: int = DEFAULT` binds `a` and READS `DEFAULT`, both
/// under one `typed_default_parameter`. A seat names the field instead, so
/// everything the grammar did not put in the name position stays a reference.
pub struct Seats(pub &'static [(&'static str, &'static str)]);

impl Seats {
    /// Is `n` inside one of these seats — the field child of an ancestor of
    /// that kind, or anywhere beneath it (a name seat is often a pattern node
    /// with the identifier under it)?
    pub fn binds(&self, n: Node<'_>) -> bool {
        let mut child = n;
        while let Some(parent) = child.parent() {
            for (kind, field) in self.0 {
                if parent.kind() != *kind {
                    continue;
                }
                let mut c = parent.walk();
                if parent
                    .children_by_field_name(field, &mut c)
                    .any(|f| f.id() == child.id())
                {
                    return true;
                }
            }
            child = parent;
        }
        false
    }
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

/// How an ecosystem reads a requirement that names a version and no operator:
/// npm pins it, cargo widens it to a caret. The one place the two grammars
/// differ, so one reader answers both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bare {
    Caret,
    Exact,
}

/// The half-open range `[lo, hi)` a semver requirement names, for the SINGLE
/// comparator forms npm and cargo spell the same way (`^`, `~`, `=`, `>=`, a
/// bare version, and a trailing `x`/`*`). A conjunction, a hyphen range, or
/// anything else this does not spell returns `None`: a range guessed wrong is
/// worse than no range at all, because a comparison silently made against the
/// wrong bounds is a finding nobody can check.
pub fn semver_range(req: &str, bare: Bare) -> Option<(Version, Version)> {
    let req = req.trim();
    if req.is_empty() || req.contains([',', '|', ' ']) {
        return None;
    }
    let (op, rest) = match req.strip_prefix(">=") {
        Some(rest) => (">=", rest),
        None => match req.split_at_checked(1) {
            Some(("^", rest)) => ("^", rest),
            Some(("~", rest)) => ("~", rest),
            Some(("=", rest)) | Some(("v", rest)) if !rest.is_empty() => ("=", rest),
            _ => match bare {
                Bare::Caret => ("^", req),
                Bare::Exact => ("=", req),
            },
        },
    };
    let rest = rest.trim().trim_start_matches('v');
    // `1.x`, `1.*` and `1` name the same range: the numbers stated, widened at
    // the first one that is not.
    let stated: Vec<&str> = rest
        .split(['-', '+'])
        .next()?
        .split('.')
        .take_while(|p| !matches!(*p, "x" | "X" | "*"))
        .collect();
    if stated.is_empty() {
        return None;
    }
    let number = |i: usize| -> Option<u64> { stated.get(i).map_or(Some(0), |p| p.parse().ok()) };
    let lo = Version::new(number(0)?, number(1)?, number(2)?);
    let width = stated.len();
    let hi = match op {
        // Open above: nothing bounds it, so nothing is disjoint from it.
        ">=" => Version::new(u64::MAX, 0, 0),
        // A pin moves at the first number it did NOT state; a tilde moves at
        // the patch wherever the minor is stated at all.
        "=" if width >= 3 => Version::new(lo.major, lo.minor, lo.patch + 1),
        "=" if width == 2 => Version::new(lo.major, lo.minor + 1, 0),
        "=" => Version::new(lo.major + 1, 0, 0),
        "~" if width >= 2 => Version::new(lo.major, lo.minor + 1, 0),
        "~" => Version::new(lo.major + 1, 0, 0),
        // Caret holds the leftmost NON-ZERO number fixed — and where every
        // stated number is zero, the first UNSTATED one is what may move.
        _ if lo.major > 0 => Version::new(lo.major + 1, 0, 0),
        _ if lo.minor > 0 => Version::new(0, lo.minor + 1, 0),
        _ if width >= 3 => Version::new(0, 0, lo.patch + 1),
        _ if width == 2 => Version::new(0, 1, 0),
        _ => Version::new(1, 0, 0),
    };
    Some((lo, hi))
}
