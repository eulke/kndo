//! What an HTML document says about other files.
//!
//! Deliberately a tag scan, not a parse. Every reference this adapter cares about lives in one
//! attribute of one tag, HTML's error recovery means a "malformed" document is still a document
//! a browser renders, and a tree-sitter grammar would buy structure nothing here reads. The scan
//! is written to under-report rather than guess: an attribute it cannot read plainly is skipped.

use kndo_core::adapter::{FileFacts, ImportKind, RawImport, RawRoot, RawRootTarget};
use kndo_core::vocab::{Confidence, RootKind, Span};
use smol_str::SmolStr;

/// Attributes that name another file, by the tag that carries them.
///
/// `<script src>` is the one that matters — it is how every bundler and every plain page names
/// its entry module, and the reason this adapter exists. The rest are the same fact in other
/// tags, and cost nothing to read once the scanner is here.
const FILE_ATTRS: &[(&str, &str)] = &[
    ("script", "src"),
    ("link", "href"),
    ("img", "src"),
    ("source", "src"),
    ("iframe", "src"),
];

pub(crate) fn extract(content: &[u8]) -> FileFacts {
    let mut facts = FileFacts::default();
    let Ok(text) = std::str::from_utf8(content) else {
        // Not text: nothing to read, and no diagnostic — a binary file with an `.html` name is
        // the user's business, not a defect this adapter should report on every run.
        return facts;
    };

    // **The document roots itself.** Nothing imports a page: a browser loads it, a server
    // renders it, a bundler is handed it. That makes an HTML file an entry point in the same
    // sense as `main` — and it is why the modules it names stop reading as unreachable.
    facts.roots.push(RawRoot {
        kind: RootKind::Production,
        target: RawRootTarget::WholeFile,
        confidence: Confidence::Certain,
    });

    let lines = LineIndex::new(text);
    for (tag, attr) in FILE_ATTRS {
        for (value, offset) in attribute_values(text, tag, attr) {
            let Some(specifier) = local_reference(&value) else {
                continue;
            };
            // The span covers the attribute's value as written, so `kndo describe` points at
            // the reference rather than at the top of the document.
            let start = lines.line_col(offset);
            let end = lines.line_col(offset + value.len());
            facts.imports.push(RawImport {
                specifier: SmolStr::new(specifier),
                kind: ImportKind::Relative,
                span: Span { start, end },
                // A page does not bind names from what it loads; naming the file IS the use.
                side_effect_only: true,
                type_only: false,
                confidence: Confidence::Certain,
                bindings: Vec::new(),
                reexported: false,
                opaque_namespace_use: false,
                module_names_visible: false,
                local_alias: None,
                reconstructed: false,
            });
        }
    }
    facts
}

/// Byte offset to `(line, column)`, both 1-based, over one document.
///
/// Built once per file: the alternative is rescanning the text from the start for every
/// reference, which is quadratic on a page with many of them.
struct LineIndex {
    /// Byte offset of the start of each line.
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> LineIndex {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex { starts }
    }

    fn line_col(&self, offset: usize) -> (u32, u32) {
        let line = self.starts.partition_point(|&s| s <= offset).max(1);
        let col = offset.saturating_sub(self.starts[line - 1]) + 1;
        (line as u32, col as u32)
    }
}

/// A reference this adapter can resolve to a file in the project, or `None`.
///
/// Anything addressed off the project is skipped rather than recorded as unresolved: a CDN URL,
/// a `data:` payload and a bare `#anchor` are all correct HTML, and reporting them would turn
/// every page into a source of noise. A root-relative `/assets/app.js` is skipped too — what it
/// names depends on the server's document root, which kndo cannot know.
fn local_reference(raw: &str) -> Option<&str> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    // A scheme (`https:`, `data:`, `mailto:`), a protocol-relative URL, a fragment, a query, or
    // a template placeholder — none of them name a file in this project.
    if value.starts_with('#')
        || value.starts_with('/')
        || value.starts_with("//")
        || value.contains("://")
        || value.starts_with("data:")
        || value.starts_with("mailto:")
        || value.contains("${")
        || value.contains("{{")
    {
        return None;
    }
    // Strip a query string or fragment: `./main.js?v=1` is `./main.js` on disk.
    let value = value.split(['?', '#']).next().unwrap_or(value);
    (!value.is_empty()).then_some(value)
}

/// Every `attr="…"` on every `<tag …>` in `text`, with the byte offset its value starts at.
///
/// Values may be double-quoted, single-quoted, or unquoted — all three are valid HTML, and a
/// scanner that handled only the first would silently miss real references.
fn attribute_values(text: &str, tag: &str, attr: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let lower = text.to_ascii_lowercase();
    let open = format!("<{tag}");
    let mut at = 0;
    while let Some(found) = lower[at..].find(&open) {
        let start = at + found;
        // `<script` must not match `<scripting-thing`: the tag name ends at whitespace or `>`.
        let after = lower[start + open.len()..].chars().next();
        if !matches!(after, Some(c) if c.is_whitespace() || c == '>' || c == '/') {
            at = start + open.len();
            continue;
        }
        let end = lower[start..]
            .find('>')
            .map(|e| start + e)
            .unwrap_or(lower.len());
        if let Some((value, at_in_tag)) = attribute_in(&text[start..end], &lower[start..end], attr)
        {
            out.push((value, start + at_in_tag));
        }
        at = end.max(start + open.len());
    }
    out
}

/// One attribute's value inside a single tag's text. `lower` is `tag` lowercased, so the
/// attribute name matches case-insensitively while the value keeps its original case.
fn attribute_in(tag: &str, lower: &str, attr: &str) -> Option<(String, usize)> {
    let mut at = 0;
    while let Some(found) = lower[at..].find(attr) {
        let start = at + found;
        at = start + attr.len();
        // The name must stand alone: `src` must not match the `src` inside `data-src`.
        let before = lower[..start].chars().next_back();
        if matches!(before, Some(c) if c.is_alphanumeric() || c == '-' || c == '_') {
            continue;
        }
        let rest = lower[at..].trim_start();
        if !rest.starts_with('=') {
            continue;
        }
        let eq = lower[at..].find('=')? + at + 1;
        let raw = tag[eq..].trim_start();
        let value_at = eq + (tag[eq..].len() - raw.len());
        let quote = raw.chars().next()?;
        return Some(if quote == '"' || quote == '\'' {
            (raw[1..].split(quote).next()?.to_string(), value_at + 1)
        } else {
            // Unquoted: the value ends at whitespace.
            (raw.split_whitespace().next()?.to_string(), value_at)
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imports(html: &str) -> Vec<String> {
        extract(html.as_bytes())
            .imports
            .into_iter()
            .map(|i| i.specifier.to_string())
            .collect()
    }

    /// The case the whole adapter exists for: 65 of vite's playground entry modules read as
    /// `unused` because this line was invisible.
    #[test]
    fn a_module_script_names_its_entry() {
        assert_eq!(
            imports(r#"<script type="module" src="./main.js"></script>"#),
            vec!["./main.js"]
        );
    }

    /// **Every HTML document is a production root.** Nothing imports a page.
    #[test]
    fn the_document_roots_itself() {
        let facts = extract(b"<html><body>hi</body></html>");
        assert_eq!(facts.roots.len(), 1);
        assert_eq!(facts.roots[0].kind, RootKind::Production);
        assert!(matches!(facts.roots[0].target, RawRootTarget::WholeFile));
        assert_eq!(facts.roots[0].confidence, Confidence::Certain);
    }

    #[test]
    fn quoting_styles_all_parse() {
        assert_eq!(imports(r#"<script src='./a.js'>"#), vec!["./a.js"]);
        assert_eq!(imports(r#"<script src=./b.js >"#), vec!["./b.js"]);
        assert_eq!(imports(r#"<script  SRC = "./c.js" >"#), vec!["./c.js"]);
    }

    /// Skipped rather than recorded as unresolved: all of these are correct HTML that names
    /// nothing in the project, and reporting them would make every page a source of noise.
    #[test]
    fn references_that_leave_the_project_are_not_imports() {
        assert!(imports(r#"<script src="https://cdn.example.com/x.js">"#).is_empty());
        assert!(imports(r#"<script src="//cdn.example.com/x.js">"#).is_empty());
        assert!(imports(r##"<link href="#top">"##).is_empty());
        assert!(imports(r#"<img src="data:image/png;base64,AAA">"#).is_empty());
        // Root-relative depends on the server's document root, which kndo cannot know.
        assert!(imports(r#"<script src="/assets/app.js">"#).is_empty());
        // A template placeholder is a value the framework computes, not a path.
        assert!(imports(r#"<script src="${base}/app.js">"#).is_empty());
        assert!(imports(r#"<link href="{{ url_for('static') }}">"#).is_empty());
    }

    #[test]
    fn a_query_or_fragment_is_stripped_from_a_local_path() {
        assert_eq!(
            imports(r#"<script src="./main.js?v=2">"#),
            vec!["./main.js"]
        );
        assert_eq!(imports(r##"<link href="./a.css#x">"##), vec!["./a.css"]);
    }

    /// `data-src` is not `src`, and `<scripting>` is not `<script>` — a substring scanner that
    /// missed either would invent references out of unrelated markup.
    #[test]
    fn neither_attribute_nor_tag_matches_on_a_substring() {
        assert!(imports(r#"<script data-src="./lazy.js">"#).is_empty());
        assert!(imports(r#"<scripting src="./x.js">"#).is_empty());
    }

    #[test]
    fn several_tags_in_one_document_are_all_read() {
        let html = r#"
            <html><head>
              <link rel="stylesheet" href="./style.css">
              <link rel="icon" href="https://cdn.example.com/f.ico">
            </head><body>
              <img src="./logo.png">
              <script type="module" src="./main.js"></script>
            </body></html>"#;
        let mut got = imports(html);
        got.sort();
        assert_eq!(got, vec!["./logo.png", "./main.js", "./style.css"]);
    }

    /// The span points at the value, not at the document — otherwise every reference in a
    /// page would report the same location.
    #[test]
    fn an_import_span_covers_the_attribute_value() {
        let html = "<html>\n<body>\n<script src=\"./main.js\"></script>\n</body>\n</html>";
        let facts = extract(html.as_bytes());
        let span = facts.imports[0].span;
        assert_eq!(span.start.0, 3, "third line");
        // `<script src="` is 13 characters, so the value starts at column 14.
        assert_eq!(span.start.1, 14);
        assert_eq!(span.end, (3, 14 + "./main.js".len() as u32));
    }

    #[test]
    fn a_document_with_no_references_still_roots_itself() {
        let facts = extract(b"<!doctype html><title>t</title>");
        assert!(facts.imports.is_empty());
        assert_eq!(facts.roots.len(), 1);
    }

    #[test]
    fn a_non_utf8_file_yields_nothing_and_says_nothing() {
        let facts = extract(&[0xff, 0xfe, 0x00]);
        assert!(facts.imports.is_empty());
        assert!(facts.roots.is_empty());
        assert!(facts.diagnostics.is_empty());
    }
}
