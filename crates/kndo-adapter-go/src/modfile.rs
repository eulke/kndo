//! The `go.mod`/`go.work` file format, read as the format and not as lines.
//!
//! Both files are one grammar: a sequence of DIRECTIVES, each a verb followed
//! by tokens, written either on one line (`require path v1.0.0`) or as a block
//! (`require (` … `)`) whose every line is a directive of that verb. A token is
//! a bare word or a quoted Go string; `//` starts a comment, but only outside a
//! quoted string; and the comment a line ends with is not decoration — a
//! requirement's says whether the module is indirect.
//!
//! Read as lines, three of the six spellings `go mod edit -json` accepts for
//! that one comment came back wrong, in both directions. The rule the tool
//! actually applies is [`Directive::indirect`], and it is stated here once.

use std::borrow::Cow;

/// One directive: its verb, its tokens unquoted, and the comment its line
/// ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive<'a> {
    pub verb: &'a str,
    pub tokens: Vec<Cow<'a, str>>,
    /// The text after this line's `//`, untrimmed and without the slashes.
    /// `None` where the line carried no comment.
    pub comment: Option<&'a str>,
}

impl<'a> Directive<'a> {
    pub fn token(&self, nth: usize) -> Option<&str> {
        self.tokens.get(nth).map(|t| t.as_ref())
    }

    /// Is this requirement INDIRECT — in the build because something else
    /// needs it, stating no usage claim of its own?
    ///
    /// The go tool's rule, and every clause of it is load-bearing: the line's
    /// comment, trimmed, up to the first `;`, must be exactly `indirect`. So
    /// `//indirect` and `//   indirect; needed by x` are indirect, while
    /// `// indirect dependency`, `// Indirect` and a second `// indirect`
    /// after another comment are not. Measured against `go mod edit -json`.
    pub fn indirect(&self) -> bool {
        let Some(comment) = self.comment else {
            return false;
        };
        let first = comment.split(';').next().unwrap_or_default();
        first.trim() == "indirect"
    }
}

/// Every directive the file states, blocks flattened into the verb that opened
/// them. A block's opening line contributes no directive of its own: `require (`
/// states nothing until a line inside it does.
pub fn directives(text: &str) -> Vec<Directive<'_>> {
    let mut out = Vec::new();
    let mut block: Option<&str> = None;
    for line in text.lines() {
        let (body, comment) = split_comment(line);
        let body = body.trim();
        if let Some(verb) = block {
            if body.starts_with(')') {
                block = None;
                continue;
            }
            if body.is_empty() {
                continue;
            }
            out.push(Directive {
                verb,
                tokens: tokens(body),
                comment,
            });
            continue;
        }
        let Some(verb) = first_word(body) else {
            continue;
        };
        let rest = body[verb.len()..].trim_start();
        // A block opens with `(` and nothing after it: the go tool rejects a
        // value on the opening line, so nothing here needs to read one.
        if rest == "(" {
            block = Some(verb);
            continue;
        }
        if rest.is_empty() {
            continue;
        }
        out.push(Directive {
            verb,
            tokens: tokens(rest),
            comment,
        });
    }
    out
}

/// Every directive of one verb.
pub fn of<'t, 'v: 't>(text: &'t str, verb: &'v str) -> impl Iterator<Item = Directive<'t>> + 't {
    directives(text).into_iter().filter(move |d| d.verb == verb)
}

/// The line's content and its comment, split at the first `//` that is not
/// inside a quoted string — a module path never holds one, and a `replace`
/// target on Windows might.
fn split_comment(line: &str) -> (&str, Option<&str>) {
    let bytes = line.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        match (quote, bytes[i]) {
            (Some(b'"'), b'\\') => i += 1,
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, c @ (b'"' | b'`')) => quote = Some(c),
            (None, b'/') if bytes.get(i + 1) == Some(&b'/') => {
                return (&line[..i], Some(&line[i + 2..]));
            }
            (None, _) => {}
        }
        i += 1;
    }
    (line, None)
}

/// The tokens of one directive body: bare words, and quoted strings unquoted.
fn tokens(body: &str) -> Vec<Cow<'_, str>> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        match bytes[i] {
            q @ (b'"' | b'`') => {
                let start = i + 1;
                let mut j = start;
                let mut escaped = false;
                while j < bytes.len() {
                    if q == b'"' && bytes[j] == b'\\' {
                        escaped = true;
                        j += 2;
                        continue;
                    }
                    if bytes[j] == q {
                        break;
                    }
                    j += 1;
                }
                let raw = &body[start..j.min(body.len())];
                out.push(if escaped {
                    Cow::Owned(unescape(raw))
                } else {
                    Cow::Borrowed(raw)
                });
                i = j + 1;
            }
            _ => {
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                out.push(Cow::Borrowed(&body[start..i]));
            }
        }
    }
    out
}

/// A Go interpreted string literal's escapes, to the extent a manifest uses
/// them: a path with a quote or a backslash in it.
fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn first_word(body: &str) -> Option<&str> {
    let end = body
        .find(|c: char| c.is_whitespace() || c == '(')
        .unwrap_or(body.len());
    (end > 0).then(|| &body[..end])
}
