use tree_sitter::{Language, Parser, Tree};

fn kotlin_language() -> Language {
    tree_sitter_kotlin_ng::LANGUAGE.into()
}

pub(crate) fn parse(source: &[u8]) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&kotlin_language()).ok()?;
    parser.parse(source, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(src: &[u8]) -> String {
        parse(src).unwrap().root_node().to_sexp()
    }

    /// Ground-truth node shapes (docs/adapters/kotlin.md §2): package/import, visibility
    /// modifiers (private/internal/protected/public — each a `visibility_modifier` leaf inside
    /// `modifiers`, default = no `modifiers` node at all), class/interface/enum/data/sealed/
    /// inner, primary+secondary constructors, companion objects, `override`/`open`/`abstract`.
    /// Kept `#[ignore]`d — run with `--ignored --nocapture` to re-verify against a
    /// tree-sitter-kotlin-ng upgrade.
    #[test]
    #[ignore]
    fn probe_declarations_and_modifiers() {
        println!(
            "{}",
            dump(
                b"package com.foo.bar\n\
                  import com.other.Thing as Alias\n\
                  import com.other.helpers.*\n\
                  class Widget(private val x: Int) : Base(), Interface1 {\n\
                  \x20   val y: Int = 1\n\
                  \x20   internal fun compute(): Int = x + y\n\
                  \x20   protected fun helper() {}\n\
                  \x20   override fun contractFn() {}\n\
                  \x20   constructor(a: Int, b: Int) : this(a) { println(b) }\n\
                  \x20   companion object Named {\n\
                  \x20       fun factory(): Widget = Widget(0)\n\
                  \x20   }\n\
                  \x20   class Inner { fun innerFn() {} }\n\
                  }\n\
                  interface Interface1\n\
                  open class Base\n\
                  sealed class Sealed\n\
                  data class Point(val x: Int, val y: Int)\n\
                  enum class Color { RED, GREEN }\n\
                  object Singleton { fun singletonFn() {} }\n\
                  fun topLevelFn(a: Int, b: Int): Int = a + b\n\
                  private val topLevelPrivate = 5\n\
                  typealias MyAlias = String\n"
            )
        );
    }

    /// Control flow / expression shapes that feed cyclomatic complexity (docs/adapters/
    /// kotlin.md §2): `if_expression`, `when_entry`, `for_statement`, `while_statement`,
    /// `try_expression`, `&&`/`||` leaves, elvis (`?:`) and not-null (`!!`) — deliberately NOT
    /// branch kinds (§2's last metrics bullet).
    #[test]
    #[ignore]
    fn probe_control_flow_and_metrics_shapes() {
        println!(
            "{}",
            dump(
                b"fun f(a: Boolean, b: Boolean, x: Int): Int {\n\
                  \x20   if (a && b) { return 1 } else if (a || b) { return 2 }\n\
                  \x20   when (x) {\n\
                  \x20       1 -> return 10\n\
                  \x20       else -> return 0\n\
                  \x20   }\n\
                  \x20   for (i in 0..10) { print(i) }\n\
                  \x20   while (true) { break }\n\
                  \x20   try { risky() } catch (e: Exception) { handle(e) } finally { cleanup() }\n\
                  \x20   val n: Int? = null\n\
                  \x20   val m = n ?: 0\n\
                  \x20   val q = n!!\n\
                  \x20   return m + q\n\
                  }\n"
            )
        );
    }

    /// Reference/navigation shapes (docs/adapters/kotlin.md §2): bare calls, qualified calls
    /// (`Obj.member()`), chained navigation (`A.B.c()`), lambdas, string-template
    /// interpolation, extension-function declarations, `is`/`as` type checks.
    #[test]
    #[ignore]
    fn probe_reference_and_navigation_shapes() {
        println!(
            "{}",
            dump(
                b"fun String.extFn(): Int = this.length\n\
                  class C {\n\
                  \x20   fun caller() {\n\
                  \x20       helper()\n\
                  \x20       Other.staticCall()\n\
                  \x20       Outer.Inner.deep()\n\
                  \x20       listOf(1, 2).map { it * 2 }\n\
                  \x20       val s = \"value: ${1 + 2}\"\n\
                  \x20       val ok = s is String\n\
                  \x20       val cast = (s as Any) as String\n\
                  \x20   }\n\
                  \x20   fun helper() {}\n\
                  }\n"
            )
        );
    }

    /// A verified upstream tree-sitter-kotlin-ng 1.1.0 grammar bug (docs/adapters/kotlin.md
    /// §0's last bullet): a meta-annotated, parameterless `annotation class` mis-parses as a
    /// bogus `infix_expression` chaining "annotation"/"class"/the name as three identifiers —
    /// the SAME source with a primary constructor, or without the leading annotation, parses
    /// correctly. Kept `#[ignore]`d and re-checked (not asserted against, since asserting on a
    /// known-broken shape would just pin the bug) on every grammar upgrade — if a future
    /// version fixes it, this probe's printed output changes and extraction can stop treating
    /// it as a gap.
    #[test]
    #[ignore]
    fn probe_annotation_class_grammar_edge_case() {
        println!(
            "BROKEN (no params): {}",
            dump(b"@Retention(AnnotationRetention.RUNTIME)\nannotation class Marker\n")
        );
        println!(
            "OK (with params): {}",
            dump(b"@Retention(AnnotationRetention.RUNTIME)\nannotation class Marker(val x: Int)\n")
        );
    }

    /// A second, independently-verified upstream grammar edge case (docs/adapters/kotlin.md
    /// §0): a `class`/`interface`/`object` body written entirely on ONE line with real content
    /// (`class Inner { fun m() {} }`) mis-parses — the same source reformatted across multiple
    /// lines parses cleanly. Real-world Kotlin overwhelmingly uses multi-line bodies for
    /// anything but an empty declaration, so this is a narrow formatting artifact, not a
    /// blocker — but every fixture and hand-written test in this adapter deliberately avoids
    /// single-line bodies with content because of it. Kept `#[ignore]`d for the same
    /// re-verify-on-upgrade reason as the probe above.
    #[test]
    #[ignore]
    fn probe_single_line_body_grammar_edge_case() {
        println!(
            "BROKEN (single line): {}",
            dump(b"class Outer {\n    class Inner { fun m() {} }\n}\n")
        );
        println!(
            "OK (multi line): {}",
            dump(b"class Outer {\n    class Inner {\n        fun m() {}\n    }\n}\n")
        );
    }
}
