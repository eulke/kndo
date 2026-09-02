//! The tag scan. Three things reach code: a script's `src`, a `link` whose
//! `rel` loads a stylesheet or preloads a module or a script, and the import
//! statements of an inline `<script type="module">`. Everything else a page
//! names — images, icons, manifests, other pages — is a resource or a
//! document, and reachability cannot enter either.

use kndo_contract::adapter::SourceFile;
use kndo_contract::evidence::{EvidenceSink, ImportShape, ImportTarget, RootKind, RootTarget};
use kndo_contract::vocab::{Confidence, Span};
use smol_str::SmolStr;

pub(crate) fn extract(file: &SourceFile<'_>, out: &mut EvidenceSink) {
    // Not text: nothing to read. A binary file with an `.html` name is the
    // user's business, not a defect to report on every run.
    let Ok(text) = std::str::from_utf8(file.content) else {
        return;
    };
    // The document roots itself. A page under a test directory is a test's
    // entry — convention, Probable, the same tier the js-ts adapter gives the
    // path; any other page is production, and that is the plain fact of it.
    if kndo_toolkit::web_test_path(file.path.as_str()) {
        out.root(RootTarget::WholeFile, RootKind::Test, Confidence::Probable);
    } else {
        out.root(
            RootTarget::WholeFile,
            RootKind::Production,
            Confidence::Certain,
        );
    }
    // A commented-out tag references nothing; blanking keeps every offset.
    let blanked = blank_comments(text);
    for reference in references(&blanked) {
        let span = Span::new(
            reference.start as u32,
            (reference.start + reference.value.len()) as u32,
        );
        match reference.kind {
            // A page binds no names from what an attribute loads: naming the
            // file IS the use. An attribute URL is document-relative by
            // definition, so a bare `main.js` is spelled `./main.js` — the
            // one spelling that says so to every resolver and judgment.
            ReferenceKind::Attribute => {
                let Some(specifier) = local_reference(reference.value) else {
                    continue;
                };
                let specifier = if specifier.starts_with('.') || specifier.starts_with('/') {
                    SmolStr::new(specifier)
                } else {
                    SmolStr::from(format!("./{specifier}"))
                };
                out.import(
                    ImportTarget::Relative(specifier),
                    ImportShape::SideEffect,
                    span,
                    Confidence::Certain,
                );
            }
            // An inline module's import is JavaScript's: a path or a package
            // by its spelling. Nothing in the document names what it took, so
            // the whole imported surface stays alive; a dynamic `import()` is
            // the module's own claim about the future, one tier down.
            ReferenceKind::Import { dynamic } => {
                let value = reference.value.trim();
                if value.is_empty() {
                    continue;
                }
                let target = if value.starts_with('.') || value.starts_with('/') {
                    ImportTarget::Relative(SmolStr::new(value))
                } else {
                    ImportTarget::Package(SmolStr::new(value))
                };
                out.import(
                    target,
                    ImportShape::Glob,
                    span,
                    if dynamic {
                        Confidence::Probable
                    } else {
                        Confidence::Certain
                    },
                );
            }
        }
    }
}

/// One value that names a file — an attribute's, or an inline import's
/// specifier — with the byte offset it starts at.
struct Reference<'a> {
    value: &'a str,
    start: usize,
    kind: ReferenceKind,
}

enum ReferenceKind {
    Attribute,
    Import { dynamic: bool },
}

/// One parsed attribute: its name lowercased, its value as written, and the
/// value's byte offset.
struct Attribute<'a> {
    name: String,
    value: &'a str,
    value_at: usize,
}

/// Every `<script src>`, every `<link href>` that loads code or style, and
/// every import statement of an inline `<script type="module">`, in document
/// order.
fn references(text: &str) -> Vec<Reference<'_>> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut at = 0;
    while let Some(found) = text[at..].find('<') {
        let start = at + found + 1;
        let name_end = start
            + text[start..]
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
                .count();
        let tag = text[start..name_end].to_ascii_lowercase();
        // The tag name ends at whitespace, `/` or `>`: `<scripting-thing` is
        // not `<script`.
        let terminated = matches!(
            bytes.get(name_end),
            Some(b) if b.is_ascii_whitespace() || *b == b'/' || *b == b'>'
        );
        if !terminated || !matches!(tag.as_str(), "script" | "link" | "style") {
            at = start;
            continue;
        }
        let end = text[name_end..]
            .find('>')
            .map_or(text.len(), |e| name_end + e);
        let attributes = attributes(text, name_end, end);
        let attribute = match tag.as_str() {
            "script" => attributes.iter().find(|a| a.name == "src"),
            _ if loads_code(&attributes) => attributes.iter().find(|a| a.name == "href"),
            _ => None,
        };
        if let Some(a) = attribute {
            out.push(Reference {
                value: a.value,
                start: a.value_at,
                kind: ReferenceKind::Attribute,
            });
        }
        at = end.max(start);
        // A script's or style's body is raw text up to its closing tag, never
        // markup: a `<script src>` spelled inside a `document.write` string is
        // text. An inline MODULE script's body is JavaScript, and its import
        // statements are references like any attribute's.
        if matches!(tag.as_str(), "script" | "style") && end < text.len() {
            let body_start = end + 1;
            let closing = format!("</{tag}");
            let body_end = text[body_start..]
                .to_ascii_lowercase()
                .find(&closing)
                .map_or(text.len(), |e| body_start + e);
            let module = attribute.is_none()
                && attributes
                    .iter()
                    .any(|a| a.name == "type" && a.value.eq_ignore_ascii_case("module"));
            if tag == "script" && module {
                for import in inline_imports(&text[body_start..body_end]) {
                    out.push(Reference {
                        value: import.specifier,
                        start: body_start + import.at,
                        kind: ReferenceKind::Import {
                            dynamic: import.dynamic,
                        },
                    });
                }
            }
            at = body_end;
        }
    }
    out
}

/// One import statement's specifier inside an inline module, at its byte
/// offset within the body.
struct InlineImport<'a> {
    specifier: &'a str,
    at: usize,
    dynamic: bool,
}

/// The import statements of one module body, by their forms alone: `import
/// "x"`, `import … from "x"`, `export … from "x"`, `import("x")`. Comments are
/// blanked first so a commented-out import references nothing; the offsets of
/// the blanked copy are the body's own.
fn inline_imports(body: &str) -> Vec<InlineImport<'_>> {
    let text = blank_js_comments(body);
    let bytes = text.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    let skip_ws = |mut j: usize| {
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        j
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // A string or template literal is skipped whole: the keyword inside
        // one is text, not a statement. A specifier is consumed right after
        // its keyword below, so this never skips one.
        if matches!(bytes[i], b'"' | b'\'' | b'`') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        // Byte-wise: `i` walks bytes and may sit inside a multi-byte
        // character, where a `str` slice would panic.
        let keyword = if bytes[i..].starts_with(b"import") {
            "import"
        } else if bytes[i..].starts_with(b"export") {
            "export"
        } else {
            i += 1;
            continue;
        };
        let bounded = (i == 0 || !is_ident(bytes[i - 1]))
            && bytes.get(i + keyword.len()).is_none_or(|b| !is_ident(*b));
        if !bounded {
            i += keyword.len();
            continue;
        }
        let j = skip_ws(i + keyword.len());
        let found = if keyword == "import" && bytes.get(j) == Some(&b'(') {
            string_at(&text, skip_ws(j + 1)).map(|(at, end)| (at, end, true))
        } else if keyword == "import" && matches!(bytes.get(j), Some(b'"' | b'\'')) {
            string_at(&text, j).map(|(at, end)| (at, end, false))
        } else {
            // `… from "x"` before the statement ends. `from` is contextual: a
            // bare `import.meta` or an `export const` never reaches a string.
            let statement_end = text[j..].find(';').map_or(text.len(), |e| j + e);
            keyword_at(&text[j..statement_end], "from")
                .and_then(|f| string_at(&text, skip_ws(j + f + 4)))
                .map(|(at, end)| (at, end, false))
        };
        match found {
            Some((at, end, dynamic)) => {
                out.push(InlineImport {
                    specifier: &body[at..end],
                    at,
                    dynamic,
                });
                i = end + 1;
            }
            None => i += keyword.len(),
        }
    }
    out
}

/// A quoted string starting at `j`: the byte range of its contents.
fn string_at(text: &str, j: usize) -> Option<(usize, usize)> {
    let quote = match text.as_bytes().get(j) {
        Some(q @ (b'"' | b'\'')) => *q as char,
        _ => return None,
    };
    let end = text[j + 1..].find(quote).map(|e| j + 1 + e)?;
    Some((j + 1, end))
}

/// A word at a token boundary — `from` inside `fromage` is not the keyword.
fn keyword_at(hay: &str, word: &str) -> Option<usize> {
    let bytes = hay.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    let mut at = 0;
    while let Some(found) = hay[at..].find(word) {
        let start = at + found;
        let end = start + word.len();
        if (start == 0 || !is_ident(bytes[start - 1]))
            && bytes.get(end).is_none_or(|b| !is_ident(*b))
        {
            return Some(start);
        }
        at = end;
    }
    None
}

/// `//` and `/* … */` comments replaced by spaces, string and template
/// literals left intact, offsets preserved.
fn blank_js_comments(body: &str) -> String {
    let bytes = body.as_bytes();
    let mut out = body.as_bytes().to_vec();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    quote = None;
                }
                i += 1;
            }
            None => {
                if matches!(b, b'"' | b'\'' | b'`') {
                    quote = Some(b);
                    i += 1;
                } else if bytes[i..].starts_with(b"//") {
                    let end = body[i..].find('\n').map_or(bytes.len(), |e| i + e);
                    out[i..end].iter_mut().for_each(|c| *c = b' ');
                    i = end;
                } else if bytes[i..].starts_with(b"/*") {
                    let end = body[i + 2..]
                        .find("*/")
                        .map_or(bytes.len(), |e| i + 2 + e + 2);
                    out[i..end].iter_mut().for_each(|c| *c = b' ');
                    i = end;
                } else {
                    i += 1;
                }
            }
        }
    }
    // Blanking writes ASCII spaces over whole bytes of ASCII delimiters and
    // comment text, so the copy stays valid UTF-8 at the same offsets.
    String::from_utf8(out).unwrap_or_else(|_| body.to_string())
}

/// A `<link>` reaches code or style through `rel="stylesheet"`,
/// `rel="modulepreload"`, or a `preload`/`prefetch` of a script or a style.
/// An icon, a manifest, a canonical URL name resources and documents.
fn loads_code(attributes: &[Attribute<'_>]) -> bool {
    let attr = |name: &str| {
        attributes
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.to_ascii_lowercase())
    };
    let Some(rel) = attr("rel") else {
        return false;
    };
    let rels: Vec<&str> = rel.split_ascii_whitespace().collect();
    if rels
        .iter()
        .any(|r| matches!(*r, "stylesheet" | "modulepreload"))
    {
        return true;
    }
    rels.iter().any(|r| matches!(*r, "preload" | "prefetch"))
        && attr("as").is_some_and(|a| matches!(a.as_str(), "script" | "style"))
}

/// The attributes of one tag, between the tag name and its `>`. Values may be
/// double-quoted, single-quoted or unquoted — all three are HTML — and a name
/// is read whole, so `data-src` is never `src`.
fn attributes(text: &str, from: usize, to: usize) -> Vec<Attribute<'_>> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = from;
    while i < to {
        while i < to && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        if i >= to {
            break;
        }
        let name_start = i;
        while i < to && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' && bytes[i] != b'/' {
            i += 1;
        }
        let name = text[name_start..i].to_ascii_lowercase();
        if name.is_empty() {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < to && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < to && bytes[j] == b'=' {
            j += 1;
            while j < to && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let (value, value_at, next) = match bytes.get(j) {
                Some(q @ (b'"' | b'\'')) => {
                    let value_start = j + 1;
                    let value_end = text[value_start..to]
                        .find(*q as char)
                        .map_or(to, |e| value_start + e);
                    (&text[value_start..value_end], value_start, value_end + 1)
                }
                _ => {
                    let value_end = j + text[j..to]
                        .bytes()
                        .take_while(|b| !b.is_ascii_whitespace())
                        .count();
                    (&text[j..value_end], j, value_end)
                }
            };
            out.push(Attribute {
                name,
                value,
                value_at,
            });
            i = next;
        } else {
            out.push(Attribute {
                name,
                value: "",
                value_at: i,
            });
            i = j;
        }
    }
    out
}

/// `<!-- … -->` regions replaced by spaces of the same length, so every offset
/// of the scan still points into the original text.
fn blank_comments(text: &str) -> String {
    let mut out = text.to_string();
    let mut at = 0;
    while let Some(found) = text[at..].find("<!--") {
        let start = at + found;
        let end = text[start + 4..]
            .find("-->")
            .map_or(text.len(), |e| start + 4 + e + 3);
        // SAFETY of the replacement: ASCII spaces only, one per byte, so char
        // boundaries stay where they were.
        out.replace_range(start..end, &" ".repeat(end - start));
        at = end;
    }
    out
}

/// The part of an attribute value that names a file in this project, or `None`
/// for a URL that leaves it: a scheme, a protocol-relative address, a fragment,
/// a template placeholder. A query string or fragment is stripped:
/// `./main.js?v=1` is `./main.js` on disk.
fn local_reference(raw: &str) -> Option<&str> {
    let value = raw.trim();
    if value.is_empty()
        || value.starts_with('#')
        || value.starts_with("//")
        || value.contains("://")
        || value.starts_with("data:")
        || value.starts_with("mailto:")
        || value.starts_with("javascript:")
        || value.starts_with("tel:")
        || value.contains("${")
        || value.contains("{{")
    {
        return None;
    }
    let value = value.split(['?', '#']).next().unwrap_or(value);
    (!value.is_empty()).then_some(value)
}
