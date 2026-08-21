use tree_sitter::{Language, Parser, Tree};

fn java_language() -> Language {
    tree_sitter_java::LANGUAGE.into()
}

pub(crate) fn parse(source: &[u8]) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&java_language()).ok()?;
    parser.parse(source, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(src: &[u8]) -> String {
        parse(src).unwrap().root_node().to_sexp()
    }

    /// Ground-truth node shapes (docs/adapters/java.md §2): declarations, imports, members,
    /// generics, references. Kept `#[ignore]`d — run with `--ignored --nocapture` to
    /// re-verify against a tree-sitter-java upgrade.
    #[test]
    #[ignore]
    fn probe_declarations_and_members() {
        println!(
            "{}",
            dump(
                b"package com.foo.bar;\n\
                  import com.other.Thing;\n\
                  import com.other.*;\n\
                  import static com.other.Thing.CONST;\n\
                  public class Widget extends Base implements Runnable, java.io.Closeable {\n\
                  \x20   private int x;\n\
                  \x20   protected static final String NAME = \"w\";\n\
                  \x20   public Widget(int x) { this.x = x; }\n\
                  \x20   @Override\n\
                  \x20   public void run() { helper(); Other.staticCall(); }\n\
                  \x20   private void helper() {}\n\
                  \x20   class Inner { void m() {} }\n\
                  \x20   static class Nested {}\n\
                  \x20   interface Sub { void go(); }\n\
                  \x20   enum Color { RED, GREEN; void tag() {} }\n\
                  \x20   record Point(int x, int y) {}\n\
                  \x20   public static void main(String[] args) {}\n\
                  }\n\
                  interface Foo extends Bar, Baz {}\n\
                  @interface MyAnno { String value(); }\n"
            )
        );
    }

    #[test]
    #[ignore]
    fn probe_generics_lambdas_refs_anon_class() {
        println!(
            "{}",
            dump(
                b"package p;\n\
                  public class Box<T extends Comparable<T>> {\n\
                  \x20   static { init(); }\n\
                  \x20   private java.util.List<String> items;\n\
                  \x20   void run() throws java.io.IOException {\n\
                  \x20       items.forEach(x -> System.out.println(x));\n\
                  \x20       items.forEach(Box::print);\n\
                  \x20       Runnable r = new Runnable() { public void run() {} };\n\
                  \x20   }\n\
                  \x20   static void init() {}\n\
                  \x20   static void print(String s) {}\n\
                  }\n"
            )
        );
    }

    #[test]
    #[ignore]
    fn probe_switch_ternary_loops_try() {
        println!(
            "{}",
            dump(
                b"class C {\n\
                  \x20   void m(int x) {\n\
                  \x20       int y = x > 0 ? 1 : 2;\n\
                  \x20       switch (x) {\n\
                  \x20           case 1: break;\n\
                  \x20           default: break;\n\
                  \x20       }\n\
                  \x20       for (int i = 0; i < 10; i++) {}\n\
                  \x20       for (String s : list) {}\n\
                  \x20       try { m(1); } catch (Exception e) {}\n\
                  \x20   }\n\
                  }\n"
            )
        );
    }

    /// Field-name ground truth: `object_creation_expression`'s anonymous `class_body` is an
    /// UNLABELED trailing child (no `body:` field, unlike `method_declaration`'s) — extraction
    /// finds it by kind, not `child_by_field_name`. Also pins that `modifiers`' keyword
    /// children (`public`, `static`, …) are anonymous tokens invisible in `to_sexp()` output
    /// even though `visibility()`/`has_modifier()` see them fine via `.children()`.
    #[test]
    #[ignore]
    fn probe_anon_class_body_field_and_main_static() {
        println!(
            "{}",
            dump(
                b"class C {\n\
                  \x20   public static void main(String[] a) {}\n\
                  \x20   void go() { Runnable r = new Runnable() { public void run() {} }; }\n\
                  }\n"
            )
        );
    }
}
