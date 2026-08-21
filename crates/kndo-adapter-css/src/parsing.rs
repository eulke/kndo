//! Parsing (docs/adapters/css.md §0): `tree-sitter-css` for `.css`, `tree-sitter-scss` for
//! `.scss` — dispatched by extension, never by content sniffing. The two grammars share almost
//! their entire node vocabulary verbatim (`tree-sitter-scss` is a strict superset, not a fork
//! with renamed nodes), so `extraction.rs` walks either resulting tree with one shared function
//! dispatched on node *kind*, never on which grammar produced it.

use tree_sitter::{Language, Parser, Tree};

fn css_language() -> Language {
    tree_sitter_css::LANGUAGE.into()
}

fn scss_language() -> Language {
    tree_sitter_scss::language()
}

pub(crate) fn parse(path: &str, source: &[u8]) -> Option<Tree> {
    let mut parser = Parser::new();
    let language = if path.ends_with(".scss") {
        scss_language()
    } else {
        css_language()
    };
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(path: &str, src: &[u8]) -> String {
        parse(path, src).unwrap().root_node().to_sexp()
    }

    /// Ground-truth node shapes (docs/adapters/css.md §2), verified against tree-sitter-css
    /// 0.25.0 and tree-sitter-scss 1.0.0 directly. Kept `#[ignore]`d — run with `--ignored
    /// --nocapture` to re-verify against a grammar version bump.
    #[test]
    #[ignore]
    fn probe_css_declarations_imports_and_references() {
        println!(
            "{}",
            dump(
                "a.css",
                b"@import \"base.css\";\n\
                  @import url(\"theme.css\");\n\
                  :root {\n\
                  \x20   --brand: #ff0000;\n\
                  }\n\
                  .btn {\n\
                  \x20   color: var(--brand);\n\
                  \x20   background: url(\"bg.png\");\n\
                  }\n\
                  @media (min-width: 600px) {\n\
                  \x20   .btn { --brand: blue; }\n\
                  }\n"
            )
        );
    }

    /// SCSS-only shapes: `$variable` declaration (same `declaration`/`property_name` node as a
    /// custom property, `$`-prefixed) and bare-`variable`-leaf reference (no `var(...)` call
    /// wrapper needed, unlike custom properties); `@mixin`/`@include`; `@function`/ordinary
    /// call-expression invocation; `@use`/`@forward`. docs/adapters/css.md §2/§5 — `@use "x" as
    /// y;` and `@extend %x;` are upstream tree-sitter-scss 1.0.0 parse bugs, both reproduced
    /// here (`has_error()` on the dump distinguishes them from the clean shapes).
    #[test]
    #[ignore]
    fn probe_scss_variables_mixins_functions_and_use() {
        println!(
            "{}",
            dump(
                "a.scss",
                b"@use \"tokens\";\n\
                  @forward \"mixins\";\n\
                  \n\
                  $brand: #ff0000;\n\
                  \n\
                  .btn {\n\
                  \x20   color: $brand;\n\
                  }\n\
                  \n\
                  @mixin flex-center($gap: 0) {\n\
                  \x20   display: flex;\n\
                  \x20   gap: $gap;\n\
                  }\n\
                  \n\
                  .card {\n\
                  \x20   @include flex-center(4px);\n\
                  }\n\
                  \n\
                  @function double($n) {\n\
                  \x20   @return $n * 2;\n\
                  }\n\
                  \n\
                  .box {\n\
                  \x20   width: double(4px);\n\
                  }\n"
            )
        );
        println!(
            "use-with-alias has_error={}",
            parse("b.scss", b"@use \"tokens\" as t;\n")
                .unwrap()
                .root_node()
                .has_error()
        );
        println!(
            "extend has_error={}",
            parse(
                "c.scss",
                b"%btn { padding: 4px; }\n.ext { @extend %btn; }\n"
            )
            .unwrap()
            .root_node()
            .has_error()
        );
    }
}
