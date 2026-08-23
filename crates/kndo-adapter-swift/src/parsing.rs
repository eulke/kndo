use tree_sitter::{Language, Parser, Tree};

fn swift_language() -> Language {
    tree_sitter_swift::LANGUAGE.into()
}

pub(crate) fn parse(source: &[u8]) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&swift_language()).ok()?;
    parser.parse(source, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(src: &[u8]) -> String {
        parse(src).unwrap().root_node().to_sexp()
    }

    /// Ground-truth node shapes: declarations (class/struct/enum
    /// share `class_declaration`, distinguished by `declaration_kind`; `protocol_declaration`
    /// is separate), visibility modifiers (`private`/`fileprivate`/`internal`/`public`/`open`
    /// — each a `visibility_modifier` leaf inside `modifiers`, absent `modifiers` = the
    /// internal default), inheritance/conformance (one unified `inheritance_specifier` list),
    /// extensions, `override`, initializers, static/class members. Kept `#[ignore]`d — run
    /// with `--ignored --nocapture` to re-verify against a tree-sitter-swift version bump.
    #[test]
    #[ignore]
    fn probe_declarations_and_modifiers() {
        println!(
            "{}",
            dump(
                b"import Foundation\n\
                  \n\
                  class Widget: Base, Greeter {\n\
                  \x20   private let x: Int = 1\n\
                  \x20   internal func compute() -> Int { return x }\n\
                  \x20   fileprivate func helper() {}\n\
                  \x20   override func go() {}\n\
                  \x20   static let shared = Widget()\n\
                  \x20   init() {}\n\
                  \x20   init(x: Int) { self.x = x }\n\
                  \x20   struct Inner {\n\
                  \x20       func innerFn() {}\n\
                  \x20   }\n\
                  }\n\
                  protocol Greeter {\n\
                  \x20   func go()\n\
                  }\n\
                  open class Base {}\n\
                  extension Widget {\n\
                  \x20   func extra() {}\n\
                  }\n\
                  enum Color {\n\
                  \x20   case red\n\
                  \x20   case green\n\
                  }\n\
                  typealias Alias = String\n\
                  fileprivate func topLevelFn() {}\n\
                  public let topLevelVal = 5\n"
            )
        );
    }

    /// Control-flow/expression shapes feeding cyclomatic complexity:
    /// `if_statement`, `guard_statement`, `switch_entry`, `for_statement`,
    /// `while_statement`, `catch_block` (not `do_statement` itself), `&&`/`||` leaves — nil-
    /// coalescing (`??`) and force-unwrap (`!`) deliberately NOT branches.
    #[test]
    #[ignore]
    fn probe_control_flow_and_metrics_shapes() {
        println!(
            "{}",
            dump(
                b"func f(a: Bool, b: Bool, x: Int) -> Int {\n\
                  \x20   if a && b { return 1 } else if a || b { return 2 }\n\
                  \x20   guard x > 0 else { return 0 }\n\
                  \x20   switch x {\n\
                  \x20   case 1: return 10\n\
                  \x20   default: return 0\n\
                  \x20   }\n\
                  \x20   for i in 0..<10 { print(i) }\n\
                  \x20   while true { break }\n\
                  \x20   do {\n\
                  \x20       try risky()\n\
                  \x20   } catch {\n\
                  \x20       handle()\n\
                  \x20   }\n\
                  \x20   let n: Int? = nil\n\
                  \x20   let m = n ?? 0\n\
                  \x20   let q = n!\n\
                  \x20   return m + q\n\
                  }\n"
            )
        );
    }

    /// Reference/navigation shapes: bare calls, qualified calls
    /// (`Obj.member()`), chained navigation, `self`, closures, `@main`/`main.swift`-shaped
    /// top-level code.
    #[test]
    #[ignore]
    fn probe_reference_and_navigation_shapes() {
        println!(
            "{}",
            dump(
                b"class C {\n\
                  \x20   func caller() {\n\
                  \x20       helper()\n\
                  \x20       Other.staticCall()\n\
                  \x20       self.field = 1\n\
                  \x20       Outer.Inner.deep()\n\
                  \x20       [1, 2].map { $0 * 2 }\n\
                  \x20   }\n\
                  \x20   func helper() {}\n\
                  \x20   var field: Int = 0\n\
                  }\n\
                  @main\n\
                  struct App {\n\
                  \x20   static func main() {\n\
                  \x20       print(\"hi\")\n\
                  \x20   }\n\
                  }\n"
            )
        );
    }
}
