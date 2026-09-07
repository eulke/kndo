//! The tag scan. Two things a page NAMES reach code: a script's `src`, and a
//! `link` whose `rel` loads a stylesheet or preloads a module or a script.
//! Two things a page HOLDS are code: an inline `<script>` and an inline
//! `<style>`, reported as embedded regions of JavaScript and CSS for those
//! extensions to read. Everything else a page names — images, icons,
//! manifests, other pages — is a resource or a document, and reachability
//! cannot enter either.

use kndo_contract::adapter::SourceFile;
use kndo_contract::evidence::{
    Attachment, EvidenceSink, ImportShape, ImportTarget, RegionMode, RootKind, RootTarget,
};
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
        out.attachment(Attachment::TestOnly);
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
    let scan = scan(&blanked);
    for reference in scan.references {
        // A page binds no names from what an attribute loads: naming the
        // file IS the use. An attribute URL is document-relative by
        // definition, so a bare `main.js` is spelled `./main.js` — the
        // one spelling that says so to every resolver and judgment.
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
            Span::new(
                reference.start as u32,
                (reference.start + reference.value.len()) as u32,
            ),
            Confidence::Certain,
        );
    }
    // The body of an inline script or style is the other language's, read by
    // its extension in the page's coordinates: what it declares and imports
    // is the page's evidence.
    for region in scan.regions {
        out.region(region.span, region.language, region.mode);
    }
}

/// One attribute value that names a file, with the byte offset it starts at.
struct Reference<'a> {
    value: &'a str,
    start: usize,
}

/// One inline body of another language, and how it runs.
struct Region {
    span: Span,
    language: &'static str,
    mode: RegionMode,
}

struct Scan<'a> {
    references: Vec<Reference<'a>>,
    regions: Vec<Region>,
}

/// One parsed attribute: its name lowercased, its value as written, and the
/// value's byte offset.
struct Attribute<'a> {
    name: String,
    value: &'a str,
    value_at: usize,
}

/// Every `<script src>` and every `<link href>` that loads code or style, in
/// document order — and every inline `<script>` or `<style>` body, as a
/// region of its language.
fn scan(text: &str) -> Scan<'_> {
    let mut references = Vec::new();
    let mut regions = Vec::new();
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
            references.push(Reference {
                value: a.value,
                start: a.value_at,
            });
        }
        at = end.max(start);
        // A script's or style's body is raw text up to its closing tag, never
        // markup: a `<script src>` spelled inside a `document.write` string is
        // text. An inline body is the other language's code — unless the
        // script has a `src`, when a browser ignores its body, or its type
        // says data rather than JavaScript (an import map, JSON, a template).
        if matches!(tag.as_str(), "script" | "style") && end < text.len() {
            let body_start = end + 1;
            let closing = format!("</{tag}");
            let body_end = text[body_start..]
                .to_ascii_lowercase()
                .find(&closing)
                .map_or(text.len(), |e| body_start + e);
            let language = if attribute.is_some() || text[body_start..body_end].trim().is_empty() {
                None
            } else if tag == "script" {
                script_mode(&attributes).map(|mode| ("js", mode))
            } else {
                Some(("css", RegionMode::Module))
            };
            if let Some((language, mode)) = language {
                regions.push(Region {
                    span: Span::new(body_start as u32, body_end as u32),
                    language,
                    mode,
                });
            }
            at = body_end;
        }
    }
    Scan {
        references,
        regions,
    }
}

/// How an inline script runs, by its `type`: a module, a classic script (no
/// type, or a JavaScript MIME type), or not JavaScript at all — an import
/// map, JSON data, a template — which is `None`.
fn script_mode(attributes: &[Attribute<'_>]) -> Option<RegionMode> {
    let Some(attribute) = attributes.iter().find(|a| a.name == "type") else {
        return Some(RegionMode::Script);
    };
    let essence = attribute
        .value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match essence.as_str() {
        "module" => Some(RegionMode::Module),
        ""
        | "text/javascript"
        | "application/javascript"
        | "text/ecmascript"
        | "application/ecmascript"
        | "text/jscript" => Some(RegionMode::Script),
        _ => None,
    }
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
