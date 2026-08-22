//! kndo-plugin-demo — the reference external adapter for kndo-plugin-api v1
//! (docs/contracts/wasm-abi.md). Targets a deliberately tiny, invented language ("kdemo") so
//! this crate can hand-scan it with no dependency beyond `wit-bindgen` — pulling in a real
//! tree-sitter grammar here would mean cross-compiling its C sources to `wasm32-unknown-
//! unknown`, a much bigger yak than this demo exists to shave (ADR 0002's grammar choice is a
//! *native*-adapter concern; nothing about the ABI requires a WASM adapter to use it).
//!
//! kdemo syntax, the whole of it:
//! ```text
//! fn helper() {
//!     other();
//! }
//! pub fn main() {
//!     helper();
//! }
//! ```
//! `pub fn` declarations are exported; a function literally named `main` is a production root
//! (RFC 0002 §3-style: language-defined roots only, the same rule every launch adapter follows).
//! Everything else is unreachable unless called from a root's transitive closure — exactly the
//! shape needed to prove `unused` fires correctly through a real external WASM adapter.

// Only ever invoked through the macro path below, never an ordinary `use` — kept as a real
// import anyway (not a suppression-shaped workaround) so dependency hygiene sees a genuine
// usage edge rather than none at all.
use wit_bindgen as _;

wit_bindgen::generate!({
    path: "../../crates/kndo-plugin-api/wit/adapter.wit",
    world: "adapter",
});

use crate::kndo::adapter::types::*;

// kndo:allow unused reason: constructed only via the export!() macro below, invisible to kndo's un-expanded parse
struct DemoAdapter;

// ---------------------------------------------------------------- lexer

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok<'a> {
    Ident(&'a str),
    LParen,
    RParen,
    LBrace,
    RBrace,
}

struct Lexeme<'a> {
    tok: Tok<'a>,
    line: u32,
    col: u32,
}

fn lex(content: &str) -> Vec<Lexeme<'_>> {
    let mut out = Vec::new();
    for (line_idx, line) in content.lines().enumerate() {
        let line_no = (line_idx + 1) as u32;
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if c.is_whitespace() {
                i += 1;
                continue;
            }
            if c == '/' && i + 1 < bytes.len() && bytes[i + 1] as char == '/' {
                break; // line comment to EOL
            }
            if c.is_alphabetic() || c == '_' {
                let start = i;
                while i < bytes.len() {
                    let cc = bytes[i] as char;
                    if cc.is_alphanumeric() || cc == '_' {
                        i += 1;
                    } else {
                        break;
                    }
                }
                out.push(Lexeme {
                    tok: Tok::Ident(&line[start..i]),
                    line: line_no,
                    col: (start + 1) as u32,
                });
                continue;
            }
            match c {
                '(' => out.push(Lexeme {
                    tok: Tok::LParen,
                    line: line_no,
                    col: (i + 1) as u32,
                }),
                ')' => out.push(Lexeme {
                    tok: Tok::RParen,
                    line: line_no,
                    col: (i + 1) as u32,
                }),
                '{' => out.push(Lexeme {
                    tok: Tok::LBrace,
                    line: line_no,
                    col: (i + 1) as u32,
                }),
                '}' => out.push(Lexeme {
                    tok: Tok::RBrace,
                    line: line_no,
                    col: (i + 1) as u32,
                }),
                _ => {}
            }
            i += 1;
        }
    }
    out
}

// ---------------------------------------------------------------- extraction

fn extract_facts(content: &str) -> FileFacts {
    let toks = lex(content);
    let mut declarations = Vec::new();
    let mut references = Vec::new();
    let mut roots = Vec::new();

    let mut i = 0usize;
    while i < toks.len() {
        let Tok::Ident("fn") = toks[i].tok else {
            i += 1;
            continue;
        };
        let is_pub = i > 0 && matches!(toks[i - 1].tok, Tok::Ident("pub"));
        let decl_start_line = if is_pub {
            toks[i - 1].line
        } else {
            toks[i].line
        };
        let decl_start_col = if is_pub { toks[i - 1].col } else { toks[i].col };

        i += 1;
        let Some(&Lexeme {
            tok: Tok::Ident(name),
            ..
        }) = toks.get(i)
        else {
            continue;
        };
        let name = name.to_string();
        i += 1;

        // skip the parameter list
        if toks.get(i).map(|t| t.tok) != Some(Tok::LParen) {
            continue;
        }
        let mut depth = 0i32;
        while let Some(t) = toks.get(i) {
            match t.tok {
                Tok::LParen => depth += 1,
                Tok::RParen => {
                    depth -= 1;
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                _ => {}
            }
            i += 1;
        }

        if toks.get(i).map(|t| t.tok) != Some(Tok::LBrace) {
            continue;
        }
        let body_start = i + 1;
        let mut j = body_start;
        let mut brace_depth = 1i32;
        while j < toks.len() && brace_depth > 0 {
            match toks[j].tok {
                Tok::LBrace => brace_depth += 1,
                Tok::RBrace => brace_depth -= 1,
                _ => {}
            }
            j += 1;
        }
        let body_end_excl = if brace_depth == 0 { j - 1 } else { j }; // exclude the closing brace
        let end_line = toks
            .get(body_end_excl.saturating_sub(1))
            .map(|t| t.line)
            .unwrap_or(decl_start_line);

        let mut k = body_start;
        while k + 1 < body_end_excl {
            if let Tok::Ident(callee) = toks[k].tok {
                if toks[k + 1].tok == Tok::LParen {
                    references.push(RawReference {
                        name: callee.to_string(),
                        scope_context: None,
                        span: Span {
                            start_line: toks[k].line,
                            start_col: toks[k].col,
                            end_line: toks[k].line,
                            end_col: toks[k].col + callee.len() as u32,
                        },
                        within: Some(name.clone()),
                        kind: RefKind::Call,
                    });
                }
            }
            k += 1;
        }

        if name == "main" {
            roots.push(RawRoot {
                kind: RootKind::Production,
                target: RawRootTarget::Declaration(name.clone()),
            });
        }

        declarations.push(Declaration {
            name: name.clone(),
            kind: SymbolKind::Function,
            span: Span {
                start_line: decl_start_line,
                start_col: decl_start_col,
                end_line,
                end_col: 1,
            },
            exported: is_pub,
            member_of: None,
        });

        i = j;
    }

    FileFacts {
        declarations,
        references,
        roots,
        diagnostics: Vec::new(),
    }
}

// ---------------------------------------------------------------- Guest impl

impl Guest for DemoAdapter {
    fn descriptor() -> AdapterDescriptor {
        AdapterDescriptor {
            id: "kdemo".to_string(),
            facts_schema_version: 1,
            file_globs: vec!["**/*.kdemo".to_string()],
            grammar_version: "hand-scanned-v1".to_string(),
            // RFC 0016 §4: exercises the global-install activation path — irrelevant when this
            // component is dropped project-local (unconditional either way, `kndo/tests/
            // external_adapter.rs`'s own proof), but `kndo/tests/global_adapter_activation.rs`
            // places it in the global tier specifically to prove this rule gates it there.
            activation: vec![ActivationRule::FileExists("*.kdemo-enable".to_string())],
            dependencies: Vec::new(),
        }
    }

    fn claim(path: String) -> Option<FileClaim> {
        if path.ends_with(".kdemo") {
            Some(FileClaim {
                class: FileClass {
                    role: FileRole::Production,
                    origin: FileOrigin::Authored,
                },
            })
        } else {
            None
        }
    }

    fn extract(_path: String, content: String) -> FileFacts {
        extract_facts(&content)
    }
}

export!(DemoAdapter);
