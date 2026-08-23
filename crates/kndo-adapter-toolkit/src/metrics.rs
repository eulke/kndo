//! Function-shape metrics (feeding the `duplicate` and `crap` analyses): a single walk
//! over a callable's tree-sitter subtree yields cyclomatic complexity,
//! LOC, and winnowing fingerprints over the *normalized* token stream — identifiers and
//! literals canonicalized, comments skipped — so Type-1 clones (reformatting, comments) and
//! Type-2 clones (renamed identifiers/literals) fingerprint identically, while any structural
//! edit changes the stream.
//!
//! Like [`crate::classify`], the per-language *data* ([`MetricsSyntax`]: which node kinds are
//! branches, identifiers, literals, comments) lives in each adapter; the machinery lives here
//! once. Fingerprint hashing is blake3-derived — stable across builds and platforms, so cached
//! facts never silently change meaning the way a `DefaultHasher` upgrade would.
//!
//! Winnowing (Schleimer/Wilkerson/Aiken): hash every K-token gram, slide a window of W gram
//! hashes, keep each window's minimum (rightmost on ties), dedupe consecutive picks. The
//! guarantee: any shared token run of at least K + W − 1 tokens contributes at least one
//! shared fingerprint — long enough runs can't hide, short accidental overlaps mostly don't
//! fire.

use tree_sitter::Node;

/// K: tokens per gram. Small enough that a meaningful statement sequence forms a gram, large
/// enough that trivial three-token overlaps don't.
const GRAM: usize = 10;
/// W: grams per winnowing window — with K, sets the guarantee threshold at K + W − 1 = 17
/// shared tokens.
const WINDOW: usize = 8;

/// An adapter's metric-relevant node kinds, as data.
#[derive(Debug)]
pub struct MetricsSyntax {
    /// Kinds that add one decision branch each (`if_statement`, `case` clauses, `catch`,
    /// and the short-circuit operator leaves `&&`/`||` — their tree-sitter leaf kind is the
    /// operator text itself).
    pub branch_kinds: &'static [&'static str],
    /// Leaf kinds canonicalized to `ID` — the Type-2 rename immunity.
    pub identifier_kinds: &'static [&'static str],
    /// Kinds canonicalized to `LIT`, *without descending* (a string literal's internal
    /// fragments would otherwise leak content into the stream).
    pub literal_kinds: &'static [&'static str],
    /// Kinds skipped entirely, subtree and all (comments).
    pub skip_kinds: &'static [&'static str],
}

/// One callable's computed shape.
#[derive(Debug, Clone)]
pub struct FunctionShape {
    /// 1 + branch count — the classic McCabe lower bound.
    pub cyclomatic: u32,
    /// Physical lines from the callable's first to last, inclusive.
    pub loc: u32,
    /// Normalized-stream length, before any minimum gate.
    pub token_count: usize,
    /// Winnowing fingerprints — empty when `token_count < min_tokens` (too small to
    /// meaningfully clone-match; the metric fields above are still real).
    pub fingerprints: Vec<u64>,
}

pub fn function_shape(node: Node, syntax: &MetricsSyntax, min_tokens: usize) -> FunctionShape {
    let mut tokens: Vec<&'static str> = Vec::new();
    let mut branches = 0u32;
    collect(node, syntax, &mut tokens, &mut branches);

    let loc = node.end_position().row as u32 - node.start_position().row as u32 + 1;
    let fingerprints = if tokens.len() < min_tokens {
        Vec::new()
    } else {
        winnow(&tokens)
    };
    FunctionShape {
        cyclomatic: 1 + branches,
        loc,
        token_count: tokens.len(),
        fingerprints,
    }
}

/// The stream needs no source text at all: identifiers/literals are canonical labels and
/// keyword/punctuation leaves are their own kind — which is exactly the Type-1/Type-2
/// immunity property, made structural.
fn collect(node: Node, syntax: &MetricsSyntax, tokens: &mut Vec<&'static str>, branches: &mut u32) {
    let kind = node.kind();
    if syntax.skip_kinds.contains(&kind) {
        return;
    }
    if syntax.branch_kinds.contains(&kind) {
        *branches += 1;
    }
    if syntax.literal_kinds.contains(&kind) {
        tokens.push("LIT");
        return; // never descend into a literal — fragments would leak content
    }
    if syntax.identifier_kinds.contains(&kind) {
        tokens.push("ID");
        return;
    }
    if node.child_count() == 0 {
        // Keyword/punctuation leaves: the kind IS the token (`if`, `{`, `&&`, `return`).
        tokens.push(kind);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, syntax, tokens, branches);
    }
}

/// Standard winnowing over the K-gram hashes of `tokens` — see module docs for the guarantee.
fn winnow(tokens: &[&str]) -> Vec<u64> {
    if tokens.len() < GRAM {
        return Vec::new();
    }
    let grams: Vec<u64> = (0..=tokens.len() - GRAM)
        .map(|i| gram_hash(&tokens[i..i + GRAM]))
        .collect();

    let mut out: Vec<u64> = Vec::new();
    let mut last_picked: Option<usize> = None;
    for start in 0..grams.len().saturating_sub(WINDOW - 1) {
        let window = &grams[start..start + WINDOW];
        // Rightmost minimum (the classic tie rule keeps selections stable as the window
        // slides).
        let (offset, &value) = window
            .iter()
            .enumerate()
            .rev()
            .min_by_key(|(_, &v)| v)
            .unwrap();
        let index = start + offset;
        if last_picked != Some(index) {
            out.push(value);
            last_picked = Some(index);
        }
    }
    if grams.len() < WINDOW {
        // Shorter than one window: keep the global minimum so a stream that passed
        // `min_tokens` still yields at least one fingerprint.
        out.push(*grams.iter().min().unwrap());
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// blake3 of the gram's tokens, NUL-joined, truncated to u64 — deterministic across builds,
/// platforms, and releases (a cached fingerprint must never change meaning under it).
fn gram_hash(gram: &[&str]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    for t in gram {
        hasher.update(t.as_bytes());
        hasher.update(b"\0");
    }
    let bytes = hasher.finalize();
    u64::from_le_bytes(bytes.as_bytes()[..8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny JS-shaped syntax table — tests exercise the machinery, adapters own real tables.
    const SYNTAX: MetricsSyntax = MetricsSyntax {
        branch_kinds: &["if_statement", "&&", "||"],
        identifier_kinds: &["identifier", "property_identifier"],
        literal_kinds: &["string", "number"],
        skip_kinds: &["comment"],
    };

    fn parse(src: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .unwrap();
        parser.parse(src, None).unwrap()
    }

    fn shape_of(src: &str) -> FunctionShape {
        let tree = parse(src);
        let func = tree.root_node().child(0).unwrap();
        function_shape(func, &SYNTAX, 10)
    }

    #[test]
    fn renamed_identifiers_and_literals_fingerprint_identically() {
        let a = shape_of("function a(x) { if (x > 1) { return x + 'one'; } return 0; }");
        let b = shape_of("function b(y) { if (y > 9) { return y + 'nine'; } return 7; }");
        assert!(!a.fingerprints.is_empty());
        assert_eq!(a.fingerprints, b.fingerprints, "Type-2 clones must match");
    }

    #[test]
    fn reformatting_and_comments_do_not_change_fingerprints() {
        let a = shape_of("function a(x) { if (x > 1) { return x + 1; } return 0; }");
        let b = shape_of(
            "function a(x) {\n  // a comment\n  if (x > 1) {\n    return x + 1;\n  }\n  return 0;\n}",
        );
        assert_eq!(a.fingerprints, b.fingerprints, "Type-1 clones must match");
    }

    #[test]
    fn structural_edits_change_the_fingerprints() {
        let a = shape_of("function a(x) { if (x > 1) { return x + 1; } return 0; }");
        let b = shape_of("function a(x) { while (x > 1) { x -= 1; } return x * 2 + 1; }");
        assert_ne!(a.fingerprints, b.fingerprints);
    }

    #[test]
    fn cyclomatic_counts_branches_and_short_circuits() {
        let s = shape_of(
            "function a(x, y) { if (x && y) { return 1; } if (x || y) { return 2; } return 3; }",
        );
        // 1 + two ifs + one && + one ||
        assert_eq!(s.cyclomatic, 5);
    }

    #[test]
    fn small_functions_get_metrics_but_no_fingerprints() {
        let s = shape_of("function a() {}");
        assert!(s.fingerprints.is_empty());
        assert!(s.token_count < 10);
        assert_eq!(s.cyclomatic, 1);
        assert_eq!(s.loc, 1);
    }

    #[test]
    fn loc_is_the_physical_line_span() {
        let s = shape_of("function a(x) {\n  if (x > 1) {\n    return 1;\n  }\n  return 0;\n}");
        assert_eq!(s.loc, 6);
    }
}
