//! `kndo:allow` pragma text parsing — comment *syntax* (`//`,
//! `/* */`) is shared by every C-family-descended language kndo has adapters for (JS/TS,
//! Go), so the pragma grammar itself lives here once rather than once per adapter. What differs
//! per language is only *which tree-sitter node kinds are comments* — adapter-declared data
//! that [`collect_suppressions`] takes as a parameter.

use kndo_core::adapter::{RawSuppression, SuppressionScope};
use smol_str::SmolStr;
use tree_sitter::Node;

/// Recursive comment walk shared by every tree-sitter adapter: visits each node whose kind is
/// in `comment_kinds`, parses it with [`parse_suppression_pragma`], and pushes a
/// [`RawSuppression`] at the comment's own span. Which node kinds are comments stays
/// adapter-declared data; the walk and the pragma grammar live here once. Extraction only —
/// binding a pragma to the declaration it covers is core logic, not an adapter's job.
pub fn collect_suppressions(
    node: Node,
    src: &[u8],
    comment_kinds: &[&str],
    out: &mut Vec<RawSuppression>,
) {
    if comment_kinds.contains(&node.kind()) {
        let text = std::str::from_utf8(&src[node.byte_range()]).unwrap_or("");
        if let Some(pragma) = parse_suppression_pragma(text) {
            out.push(RawSuppression {
                span: crate::parsing::span(node),
                category: pragma.category,
                subject: pragma.subject,
                reason: pragma.reason,
                scope: pragma.scope,
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_suppressions(child, src, comment_kinds, out);
    }
}

pub struct ParsedPragma {
    pub scope: SuppressionScope,
    pub category: SmolStr,
    pub subject: Option<SmolStr>,
    pub reason: Option<String>,
}

/// `kndo:allow <category>[:<subject>] [reason…]` (scope `Declaration`) or
/// `kndo:allow-file <category>[:<subject>] [reason…]` (scope `File`) — the pragma grammar,
/// found inside any comment style. Block comments (including `/** … */`) are checked line by
/// line (stripping a leading `*` per line, the common doc-comment convention) since the pragma
/// need not be the comment's first line; `//` comments are always exactly one line. Returns
/// `None` for anything that isn't a pragma — an ordinary comment is never mistaken for one, and
/// `kndo:allow` must be followed by whitespace (or nothing) so a name that merely starts with
/// that text (`kndo:allowlist`, say) doesn't false-match.
pub fn parse_suppression_pragma(comment_text: &str) -> Option<ParsedPragma> {
    let lines: Vec<&str> = if let Some(inner) = comment_text.strip_prefix("//") {
        vec![inner]
    } else {
        // not a recognized comment delimiter shape — never expected in practice
        let inner = comment_text
            .strip_prefix("/*")
            .and_then(|s| s.strip_suffix("*/"))?;
        inner
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                trimmed.strip_prefix('*').unwrap_or(trimmed)
            })
            .collect()
    };
    lines.into_iter().find_map(parse_pragma_line)
}

fn parse_pragma_line(line: &str) -> Option<ParsedPragma> {
    let line = line.trim();
    let (scope, rest) = if let Some(rest) = line.strip_prefix("kndo:allow-file") {
        (SuppressionScope::File, rest)
    } else {
        let rest = line.strip_prefix("kndo:allow")?;
        (SuppressionScope::Declaration, rest)
    };
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None; // e.g. "kndo:allowlist" — the keyword must stand alone
    }
    let rest = rest.trim_start();
    let mut parts = rest.splitn(2, char::is_whitespace);
    let target = parts.next().unwrap_or("");
    if target.is_empty() {
        return None; // "kndo:allow" naming no category isn't a valid pragma
    }
    let reason = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let (category, subject) = match target.split_once(':') {
        Some((c, s)) => (c, Some(SmolStr::new(s))),
        None => (target, None),
    };
    Some(ParsedPragma {
        scope,
        category: SmolStr::new(category),
        subject,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_comment_pragma_parses_category_and_reason() {
        let p = parse_suppression_pragma("// kndo:allow unused stale helper").unwrap();
        assert_eq!(p.category.as_str(), "unused");
        assert_eq!(p.subject, None);
        assert_eq!(p.reason.as_deref(), Some("stale helper"));
        assert_eq!(p.scope, SuppressionScope::Declaration);
    }

    #[test]
    fn file_scope_pragma_is_recognized() {
        let p = parse_suppression_pragma("/* kndo:allow-file version-skew */").unwrap();
        assert_eq!(p.scope, SuppressionScope::File);
        assert_eq!(p.category.as_str(), "version-skew");
    }

    #[test]
    fn subject_facet_is_split_on_colon() {
        let p = parse_suppression_pragma("// kndo:allow unused:enum-member").unwrap();
        assert_eq!(p.category.as_str(), "unused");
        assert_eq!(p.subject.as_deref(), Some("enum-member"));
    }

    #[test]
    fn a_name_that_merely_starts_with_the_keyword_does_not_match() {
        assert!(parse_suppression_pragma("// kndo:allowlist something").is_none());
    }

    #[test]
    fn a_bare_keyword_with_no_category_is_not_a_pragma() {
        assert!(parse_suppression_pragma("// kndo:allow").is_none());
    }

    #[test]
    fn an_ordinary_comment_is_not_a_pragma() {
        assert!(parse_suppression_pragma("// just a comment").is_none());
    }

    #[test]
    fn block_comment_pragma_on_a_non_first_line_is_found() {
        let p =
            parse_suppression_pragma("/**\n * some doc text\n * kndo:allow unused\n */").unwrap();
        assert_eq!(p.category.as_str(), "unused");
    }
}
