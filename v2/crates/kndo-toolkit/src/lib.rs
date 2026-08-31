//! The adapters' paved road: helpers whose behavior is identical for a grammar they
//! have never seen. Grammar constants stay in their adapters, next to the grammar;
//! the four lines of parser scaffolding around them live here once (v1 carried five
//! verbatim copies of `parse`, three with comments defending the copy).

use kndo_contract::vocab::Span;
use tree_sitter::{Language, Node, Parser, Tree};

/// Parse `source` with `language`; `None` when the grammar refuses to load or the
/// parse produces no tree (extraction then degrades through a diagnostic).
pub fn parse(language: &Language, source: &[u8]) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(language).ok()?;
    parser.parse(source, None)
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
/// span can exclude them. An adapter whose comment markers carry more meaning
/// than text (Rust's doc markers) keeps its own extraction instead — that
/// difference is grammar knowledge too.
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

/// A generated-file marker scan over the head of a source file: any of the
/// declared needles inside a comment-shaped line marks the whole file. The
/// mechanics; the needles are the adapter's declaration (Go keeps its own
/// stricter convention-anchored scan — that difference is its own knowledge).
pub fn generated_marked(source: &[u8], needles: &[&str]) -> bool {
    let head = &source[..source.len().min(2048)];
    std::str::from_utf8(head).is_ok_and(|s| {
        s.lines().take(24).any(|l| {
            let l = l.trim();
            (l.starts_with("//") || l.starts_with("/*") || l.starts_with('*'))
                && needles.iter().any(|n| l.contains(n))
        })
    })
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

    /// A JVM language's spec differs only in identity: same evidence streams,
    /// same build system's manifests. One spelling of that fact.
    pub fn jvm_spec(
        coordinate: &'static str,
        version: u32,
        extensions: &[&'static str],
    ) -> kndo_contract::extension::ExtensionSpec {
        use kndo_contract::evidence::{EvidenceStream, EvidenceStreams};
        kndo_contract::extension::ExtensionSpec::builder(coordinate, version)
            .extensions(extensions)
            .emits(EvidenceStreams::of(&[
                EvidenceStream::Comments,
                EvidenceStream::Metrics,
            ]))
            .manifests(MANIFEST_GLOBS)
            .build()
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

    /// Dependency NAMES from one manifest, dispatched by file name — the whole
    /// activation read, shared verbatim by the JVM adapters.
    pub fn dependencies(manifest: &SourceFile<'_>) -> Vec<SmolStr> {
        let name = manifest
            .path
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or_default();
        let Ok(text) = std::str::from_utf8(manifest.content) else {
            return Vec::new();
        };
        let mut out = match name {
            "pom.xml" => maven(text),
            "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts" => {
                gradle(text)
            }
            _ => Vec::new(),
        };
        out.sort();
        out.dedup();
        out
    }

    /// `<dependency>` blocks inside `<dependencies>`: pair each `<groupId>` with
    /// its `<artifactId>` in document order.
    pub fn maven(text: &str) -> Vec<SmolStr> {
        let mut out = Vec::new();
        let mut in_dependencies = false;
        let mut group: Option<&str> = None;
        let mut artifact: Option<&str> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.contains("<dependencies>") {
                in_dependencies = true;
            }
            if line.contains("</dependencies>") {
                in_dependencies = false;
            }
            if !in_dependencies {
                continue;
            }
            if line.contains("<dependency>") || line.contains("</dependency>") {
                group = None;
                artifact = None;
            }
            if let Some(v) = tag_value(line, "groupId") {
                group = Some(v);
            }
            if let Some(v) = tag_value(line, "artifactId") {
                artifact = Some(v);
            }
            if let (Some(g), Some(a)) = (group, artifact) {
                out.push(SmolStr::new(format!("{g}:{a}")));
                out.push(SmolStr::new(a));
                group = None;
                artifact = None;
            }
        }
        out
    }

    fn tag_value<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = line.find(&open)? + open.len();
        let end = line.find(&close)?;
        (start <= end).then(|| line[start..end].trim())
    }

    /// Quoted `group:artifact[:version]` coordinates anywhere in the script — the
    /// shape every dependency notation shares (`implementation "g:a:v"`,
    /// `api('g:a')`, version catalogs excluded by their own syntax).
    pub fn gradle(text: &str) -> Vec<SmolStr> {
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("//") {
                continue;
            }
            for quote in ['"', '\''] {
                let mut rest = line;
                while let Some(start) = rest.find(quote) {
                    let after = &rest[start + 1..];
                    let Some(end) = after.find(quote) else {
                        break;
                    };
                    let literal = &after[..end];
                    rest = &after[end + 1..];
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
                            out.push(SmolStr::new(format!("{g}:{a}")));
                            out.push(SmolStr::new(a));
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
