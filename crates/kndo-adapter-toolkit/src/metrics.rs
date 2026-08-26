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

use kndo_core::adapter::{FileFacts, FunctionMetrics, Span};
use smol_str::SmolStr;
use tree_sitter::Node;

/// K: tokens per gram. Small enough that a meaningful statement sequence forms a gram, large
/// enough that trivial three-token overlaps don't.
const GRAM: usize = 10;
/// W: grams per winnowing window — with K, sets the guarantee threshold at K + W − 1 = 17
/// shared tokens.
const WINDOW: usize = 8;

/// Default granularity gate: bodies under 50 normalized tokens don't fingerprint (their
/// metrics are still emitted, for `crap`) — the same default every adapter used to redeclare
/// as its own local constant.
pub const MIN_CLONE_TOKENS: usize = 50;

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
    /// Kinds that introduce a callable BODY of their own — a closure, a lambda, an inline
    /// function expression. Such a node becomes its OWN shape *if it is big enough to carry
    /// clone evidence by itself* (`min_clone_tokens`): its branches and its tokens then leave
    /// the enclosing shape's stream, which keeps a single `FN` placeholder in their place, and
    /// it is measured, fingerprinted and reported in its own right. Below that floor it stays
    /// an expression and folds into its owner, as it did before this field existed.
    ///
    /// Empty is a valid answer, and it means no splitting: closures fold into their enclosing
    /// callable, the way every adapter behaved before this field existed. Declare a kind only
    /// where it genuinely introduces an authored callable — this is the adapter reporting its
    /// grammar, never guessing a policy. What is DONE with the split is uniform across
    /// languages (see [`push_function_metrics`]); which kinds trigger it is per-language.
    ///
    /// Only *named* nodes are considered, so a grammar whose `function` keyword token shares
    /// a name with a node kind can't accidentally split on the keyword.
    pub nested_callable_kinds: &'static [&'static str],
    /// Kinds that CONSTRUCT a value: a struct/object/record literal, a constructor call used
    /// as an expression. Only ever consulted for the whole-body test
    /// ([`FunctionShape::body_is_construction`]) — a construction nested inside real logic is
    /// ordinary code and stays clone-eligible.
    ///
    /// Empty is a valid answer. Kotlin and Swift declare nothing here and that is correct:
    /// constructing a value in both is an ordinary `call_expression`, syntactically
    /// indistinguishable from any other call, so the adapter has nothing true to report.
    /// Guessing (an uppercase callee, say) would be the adapter inventing a verdict, in the
    /// accusation direction.
    pub construction_kinds: &'static [&'static str],
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
    /// This body is a single value-construction expression and nothing else.
    pub body_is_construction: bool,
}

/// Is this body a single construction expression and nothing else?
///
/// Walks the body's NAMED children — punctuation is unnamed and comments are skipped, so no
/// per-language wrapper vocabulary is needed — following the chain while each level has
/// exactly one. A `block` holding one `struct_expression`, or a `statement_block` holding one
/// `return_statement` holding one `object`, both reach the construction; a body that also
/// binds a local, or branches, or calls anything else, has two named children somewhere and
/// stops.
///
/// Deliberately all-or-nothing. A function that constructs AND does work is ordinary code: its
/// structure is authored, and copy-paste of it is exactly what `duplicate` should catch.
///
/// No carve-out is needed for a construction carrying a callback, and that is the previous
/// commit paying for itself: a promoted closure's tokens are not in this stream at all (only
/// an `FN` placeholder is), and the closure is its own shape, which this test never sees.
fn body_is_construction(body: Node, syntax: &MetricsSyntax) -> bool {
    if syntax.construction_kinds.is_empty() {
        return false;
    }
    let mut node = body;
    loop {
        if syntax.construction_kinds.contains(&node.kind()) {
            return true;
        }
        let mut cursor = node.walk();
        let named: Vec<Node> = node
            .named_children(&mut cursor)
            .filter(|c| !syntax.skip_kinds.contains(&c.kind()))
            .take(2)
            .collect();
        match named.as_slice() {
            [only] => node = *only,
            _ => return false,
        }
    }
}

/// Computes [`function_shape`] and appends the resulting [`FunctionMetrics`] to `out` — the
/// same mapping duplicated identically across four adapters before this moved here.
/// `syntax`/`min_clone_tokens` stay parameters (per-language data, not duplication) — most
/// adapters wrap this in a one-line local `push_function_metrics(out, symbol, decl_span, body)`
/// that partially applies its own [`MetricsSyntax`] and [`MIN_CLONE_TOKENS`].
///
/// `decl_span` must be the span of the [`kndo_core::adapter::Declaration`] these metrics
/// describe — the *declaration's* span, not the body's. Assembly resolves metrics to their
/// symbol by that span, so a mismatch silently drops the metrics (no duplication/CRAP analysis
/// for that callable) rather than misattributing them.
pub fn push_function_metrics(
    out: &mut FileFacts,
    symbol: &str,
    decl_span: Span,
    body: Node,
    syntax: &MetricsSyntax,
    min_clone_tokens: usize,
) {
    let mut next_ordinal = 0u16;
    push_shape(
        out,
        symbol,
        decl_span,
        walk(body, syntax, min_clone_tokens),
        decl_span,
        syntax,
        min_clone_tokens,
        &mut next_ordinal,
    );
}

/// One shape, then every callable promoted out of it — depth-first, so `shape_ordinal` is a
/// pre-order numbering over the whole declaration and stays the same on every run.
#[allow(clippy::too_many_arguments)]
fn push_shape(
    out: &mut FileFacts,
    symbol: &str,
    decl_span: Span,
    walked: Walked<'_>,
    shape_span: Span,
    syntax: &MetricsSyntax,
    min_clone_tokens: usize,
    next_ordinal: &mut u16,
) {
    let shape_ordinal = *next_ordinal;
    *next_ordinal += 1;
    let node = walked.node;
    let fingerprints = if walked.tokens.len() < min_clone_tokens {
        Vec::new()
    } else {
        winnow(&walked.tokens)
    };
    out.functions.push(FunctionMetrics {
        symbol: SmolStr::new(symbol),
        span: decl_span,
        shape_span,
        shape_ordinal,
        cyclomatic: 1 + walked.branches,
        loc: node.end_position().row as u32 - node.start_position().row as u32 + 1,
        token_count: walked.tokens.len() as u32,
        fingerprints,
        body_is_construction: body_is_construction(node, syntax),
    });
    for nested in walked.nested {
        let span = crate::parsing::span(nested.node);
        push_shape(
            out,
            symbol,
            decl_span,
            nested,
            span,
            syntax,
            min_clone_tokens,
            next_ordinal,
        );
    }
}

/// One callable's walked subtree: its own normalized stream and branch count, plus the
/// callables promoted out of it — each already walked, so nothing is walked twice.
struct Walked<'t> {
    node: Node<'t>,
    tokens: Vec<&'static str>,
    branches: u32,
    nested: Vec<Walked<'t>>,
}

/// `node`'s own shape — excluding every callable *promoted* out of it — plus those callables.
///
/// A nested callable's own subtree is walked whole (parameters included): unlike a named
/// declaration, whose signature is a promise and whose body is the part that gets
/// copy-pasted, an anonymous callable's parameter list is part of the thing being pasted, and
/// asking each grammar which child is "the body" would be per-language guessing.
pub fn function_shape<'t>(
    node: Node<'t>,
    syntax: &MetricsSyntax,
    min_tokens: usize,
) -> (FunctionShape, Vec<Node<'t>>) {
    let walked = walk(node, syntax, min_tokens);
    let loc = node.end_position().row as u32 - node.start_position().row as u32 + 1;
    let fingerprints = if walked.tokens.len() < min_tokens {
        Vec::new()
    } else {
        winnow(&walked.tokens)
    };
    let nested = walked.nested.iter().map(|w| w.node).collect();
    (
        FunctionShape {
            cyclomatic: 1 + walked.branches,
            loc,
            token_count: walked.tokens.len(),
            fingerprints,
            body_is_construction: body_is_construction(node, syntax),
        },
        nested,
    )
}

fn walk<'t>(node: Node<'t>, syntax: &MetricsSyntax, min_tokens: usize) -> Walked<'t> {
    let mut walked = Walked {
        node,
        tokens: Vec::new(),
        branches: 0,
        nested: Vec::new(),
    };
    collect(node, syntax, min_tokens, &mut walked, true);
    walked
}

/// The stream needs no source text at all: identifiers/literals are canonical labels and
/// keyword/punctuation leaves are their own kind — which is exactly the Type-1/Type-2
/// immunity property, made structural.
fn collect<'t>(
    node: Node<'t>,
    syntax: &MetricsSyntax,
    min_tokens: usize,
    out: &mut Walked<'t>,
    is_root: bool,
) {
    let kind = node.kind();
    if syntax.skip_kinds.contains(&kind) {
        return;
    }
    // A callable nested inside this one MAY be its own shape — but only if it is big enough to
    // carry clone evidence on its own. Promoting a small one would take its tokens out of the
    // enclosing stream without giving them anywhere to land: both halves end up under the
    // clone floor and a real clone stops being reported. Measured on the corpus, that cost 83
    // clone participants — `Receiver.close`, `deleteWhere`, `Interceptor.adapt`: functions
    // whose whole substance is one small lambda. So the SAME floor that decides whether a body
    // is worth fingerprinting decides whether a nested callable is worth separating; below it,
    // the callable is an expression and folds into its owner exactly as before.
    //
    // Never on the root: adapters hand this walk a callable's body, and one of them (JS's
    // `const f = (x) => …`) hands it something that IS a nested-callable kind. Splitting there
    // would emit an empty outer shape and re-measure the same node forever.
    if !is_root && node.is_named() && syntax.nested_callable_kinds.contains(&kind) {
        let nested = walk(node, syntax, min_tokens);
        if nested.tokens.len() >= min_tokens {
            // One `FN` stands in its place, exactly as `LIT`/`ID` stand in for a literal's or
            // an identifier's content: the enclosing stream must record that a callable was
            // passed here without absorbing what it does.
            out.tokens.push("FN");
            out.nested.push(nested);
        } else {
            out.tokens.extend(nested.tokens);
            out.branches += nested.branches;
            // A small lambda may still WRAP a big one. Folding the wrapper must not swallow
            // what the wrapper itself promoted.
            out.nested.extend(nested.nested);
        }
        return;
    }
    if syntax.branch_kinds.contains(&kind) {
        out.branches += 1;
    }
    if syntax.literal_kinds.contains(&kind) {
        out.tokens.push("LIT");
        return; // never descend into a literal — fragments would leak content
    }
    if syntax.identifier_kinds.contains(&kind) {
        out.tokens.push("ID");
        return;
    }
    if node.child_count() == 0 {
        // Keyword/punctuation leaves: the kind IS the token (`if`, `{`, `&&`, `return`).
        out.tokens.push(kind);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, syntax, min_tokens, out, false);
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
        nested_callable_kinds: &["arrow_function", "function_expression"],
        construction_kinds: &["object", "new_expression"],
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
        function_shape(func, &SYNTAX, 10).0
    }

    /// Every shape one declaration produces, in emission order: `(shape_ordinal, cyclomatic,
    /// token_count, first line of `shape_span`)`. Drives `push_function_metrics` itself
    /// rather than `function_shape`, because the ordinals and the spans are the driver's job.
    fn shapes_of(src: &str) -> Vec<(u16, u32, u32, u32)> {
        let tree = parse(src);
        let func = tree.root_node().child(0).unwrap();
        let body = func.child_by_field_name("body").unwrap_or(func);
        let mut out = FileFacts::default();
        push_function_metrics(&mut out, "f", crate::parsing::span(func), body, &SYNTAX, 10);
        out.functions
            .iter()
            .map(|m| {
                (
                    m.shape_ordinal,
                    m.cyclomatic,
                    m.token_count,
                    m.shape_span.start.0,
                )
            })
            .collect()
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

    // ------------------------------------------------------------ shape splitting

    #[test]
    fn a_nested_callable_becomes_its_own_shape() {
        let shapes =
            shapes_of("function a(xs) {\n  return xs.map((x) => {\n    if (x) { return 1; }\n    return 0;\n  });\n}");
        assert_eq!(shapes.len(), 2, "the declaration and the arrow: {shapes:?}");
        let (outer, inner) = (shapes[0], shapes[1]);
        assert_eq!(outer.0, 0, "the declaration's own shape is ordinal 0");
        assert_eq!(inner.0, 1);
        // The arrow's `if` belongs to the arrow, not to `a`.
        assert_eq!(
            outer.1, 1,
            "the enclosing shape keeps only its own branches"
        );
        assert_eq!(inner.1, 2);
        assert!(
            inner.3 > outer.3,
            "each shape's span is its own: {shapes:?}"
        );
    }

    #[test]
    fn the_enclosing_stream_keeps_a_placeholder_where_the_callable_was() {
        // Not merely "the tokens are gone": the enclosing stream must still record that
        // SOMETHING callable was passed here, or `f(cb)` and `f(x)` would look alike.
        // The arrow has to clear the clone floor to be promoted at all, so this one is real
        // work rather than `x + 1` — a small lambda folds into its owner by design.
        let arrow = "(x) => { if (x > 1) { return x + 1; } if (x > 2) { return x + 2; } \
                     const y = x * 3; return y + 4; }";
        let tree = parse(&format!("function a(xs) {{ return xs.map({arrow}); }}"));
        let func = tree.root_node().child(0).unwrap();
        let body = func.child_by_field_name("body").unwrap();
        let walked = walk(body, &SYNTAX, 10);
        assert!(walked.tokens.contains(&"FN"), "{:?}", walked.tokens);
        assert_eq!(walked.nested.len(), 1);
    }

    /// A body long enough to clear a 10-token floor on its own, parameterized so several can
    /// be nested without colliding.
    fn big(tag: &str) -> String {
        format!("{{ const {tag} = 1; if ({tag}) {{ return {tag} + 1; }} return {tag} - 1; }}")
    }

    #[test]
    fn nested_callables_are_numbered_depth_first() {
        let inner = format!("(y) => {}", big("y"));
        let outer = format!(
            "(x) => {{ g({inner}); {} }}",
            big("x").trim_matches(['{', '}'])
        );
        let last = format!("(z) => {}", big("z"));
        let shapes = shapes_of(&format!("function a() {{\n  f({outer});\n  h({last});\n}}"));
        let ordinals: Vec<u16> = shapes.iter().map(|s| s.0).collect();
        assert_eq!(ordinals, vec![0, 1, 2, 3], "{shapes:?}");
        // Depth-first: the arrow inside the first arrow is numbered before `a`'s second one.
        let lines: Vec<u32> = shapes.iter().map(|s| s.3).collect();
        assert!(lines.windows(2).all(|w| w[0] <= w[1]), "{shapes:?}");
    }

    #[test]
    fn a_callable_too_small_to_carry_clone_evidence_folds_into_its_owner() {
        // The corpus lesson: `xs.map((x) => x.name)` promoted would leave BOTH halves under
        // the clone floor, and a real clone of the enclosing function stops being reported.
        // Below the floor a nested callable is an expression, exactly as before the split.
        let src =
            "function a(xs) { const t = xs.map((x) => x.name); if (t) { return t; } return null; }";
        let shapes = shapes_of(src);
        assert_eq!(shapes.len(), 1, "the small arrow folds in: {shapes:?}");
        assert_eq!(
            shapes[0].1, 2,
            "and its owner keeps the whole body's branches"
        );
    }

    #[test]
    fn folding_a_small_callable_does_not_swallow_a_big_one_inside_it() {
        // A tiny wrapper around a substantial lambda: the wrapper folds, but what IT promoted
        // has to keep travelling up, or the big one silently disappears.
        let inner = format!("(y) => {}", big("y"));
        let shapes = shapes_of(&format!("function a() {{ f((x) => g({inner})); }}"));
        assert_eq!(shapes.len(), 2, "{shapes:?}");
        assert_eq!(shapes[1].0, 1, "the promoted one is the inner lambda");
    }

    #[test]
    fn an_adapter_declaring_no_nested_kinds_emits_exactly_one_shape() {
        // The pre-split behaviour, pinned: a language whose grammar has no "this is a
        // callable body" node declares nothing and keeps folding closures into their owner.
        const NO_SPLIT: MetricsSyntax = MetricsSyntax {
            branch_kinds: SYNTAX.branch_kinds,
            identifier_kinds: SYNTAX.identifier_kinds,
            literal_kinds: SYNTAX.literal_kinds,
            skip_kinds: SYNTAX.skip_kinds,
            nested_callable_kinds: &[],
            construction_kinds: SYNTAX.construction_kinds,
        };
        let tree =
            parse("function a(xs) { return xs.map((x) => { if (x) { return 1; } return 0; }); }");
        let func = tree.root_node().child(0).unwrap();
        let body = func.child_by_field_name("body").unwrap();
        let mut out = FileFacts::default();
        push_function_metrics(
            &mut out,
            "a",
            crate::parsing::span(func),
            body,
            &NO_SPLIT,
            10,
        );
        assert_eq!(out.functions.len(), 1);
        assert_eq!(
            out.functions[0].cyclomatic, 2,
            "unsplit, the arrow's branch is the enclosing function's"
        );
    }

    #[test]
    fn the_walk_root_is_never_split() {
        // JS hands this walk an arrow's own body for `const f = (x) => …`; splitting on the
        // root would emit an empty outer shape and re-measure the same node.
        let tree = parse("const f = (x) => { if (x) { return 1; } return 0; };");
        let arrow = tree
            .root_node()
            .descendant_for_byte_range(10, 11)
            .and_then(|n| {
                let mut n = n;
                while n.kind() != "arrow_function" {
                    n = n.parent()?;
                }
                Some(n)
            })
            .expect("the arrow");
        let mut out = FileFacts::default();
        push_function_metrics(
            &mut out,
            "f",
            crate::parsing::span(arrow),
            arrow,
            &SYNTAX,
            10,
        );
        assert_eq!(out.functions.len(), 1, "one shape, not zero and not two");
        assert_eq!(out.functions[0].cyclomatic, 2);
    }

    // ------------------------------------------------------------ construction bodies

    fn constructs(src: &str) -> bool {
        let tree = parse(src);
        let func = tree.root_node().child(0).unwrap();
        let body = func.child_by_field_name("body").unwrap_or(func);
        function_shape(body, &SYNTAX, 10).0.body_is_construction
    }

    #[test]
    fn a_body_that_only_constructs_a_value_is_recognized() {
        assert!(constructs("function a() { return { x: 1, y: 2, z: 3 }; }"));
        assert!(constructs("function a() { return new Thing(1, 2, 3); }"));
    }

    #[test]
    fn a_body_that_also_does_work_is_not_a_construction() {
        assert!(!constructs(
            "function a() { const t = 1; return { x: t }; }"
        ));
        assert!(!constructs(
            "function a(f) { if (f) { return { x: 1 }; } return { x: 2 }; }"
        ));
        assert!(!constructs("function a() { compute(); }"));
    }

    #[test]
    fn a_construction_carrying_a_callback_is_still_a_construction() {
        // The narrowing predicate an earlier design needed, made unnecessary: a promoted
        // closure's tokens are not in this stream at all — only an `FN` placeholder is — and
        // the closure is its own shape, which this exemption never sees. So the construction
        // stays exempt AND the duplicated callback stays visible, on the callback.
        let cb = format!("(x) => {}", big("x"));
        let src = format!("function a() {{ return new Thing({cb}); }}");
        assert!(constructs(&src));
        let shapes = shapes_of(&src);
        assert_eq!(shapes.len(), 2, "the callback is its own shape: {shapes:?}");
    }

    #[test]
    fn an_adapter_declaring_no_construction_kinds_never_exempts_anything() {
        const NO_KINDS: MetricsSyntax = MetricsSyntax {
            branch_kinds: SYNTAX.branch_kinds,
            identifier_kinds: SYNTAX.identifier_kinds,
            literal_kinds: SYNTAX.literal_kinds,
            skip_kinds: SYNTAX.skip_kinds,
            nested_callable_kinds: SYNTAX.nested_callable_kinds,
            construction_kinds: &[],
        };
        let tree = parse("function a() { return { x: 1 }; }");
        let func = tree.root_node().child(0).unwrap();
        let body = func.child_by_field_name("body").unwrap();
        assert!(!function_shape(body, &NO_KINDS, 10).0.body_is_construction);
    }

    #[test]
    fn the_declarations_own_shape_span_is_the_declaration_span() {
        // What keeps every pre-split finding id and location byte-identical.
        let arrow = format!("(x) => {}", big("x"));
        let tree = parse(&format!("function a(xs) {{ return xs.map({arrow}); }}"));
        let func = tree.root_node().child(0).unwrap();
        let body = func.child_by_field_name("body").unwrap();
        let decl_span = crate::parsing::span(func);
        let mut out = FileFacts::default();
        push_function_metrics(&mut out, "a", decl_span, body, &SYNTAX, 10);
        assert_eq!(out.functions[0].shape_span, decl_span);
        assert_eq!(out.functions[0].span, decl_span);
        // …while the nested one carries the declaration's span as its resolution key and its
        // own span as its extent.
        assert_eq!(out.functions[1].span, decl_span);
        assert_ne!(out.functions[1].shape_span, decl_span);
    }
}
