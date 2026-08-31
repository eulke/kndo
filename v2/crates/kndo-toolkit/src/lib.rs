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
        .map(|m| Span::new(span.start + m.len() as u32, span.end))
        .or_else(|| {
            markers.block.iter().find_map(|(open, close)| {
                (bytes.starts_with(open.as_bytes())
                    && bytes.ends_with(close.as_bytes())
                    && bytes.len() >= open.len() + close.len())
                .then(|| {
                    Span::new(
                        span.start + open.len() as u32,
                        span.end - close.len() as u32,
                    )
                })
            })
        })
        .unwrap_or(span);
    out.comment(span, text);
}
