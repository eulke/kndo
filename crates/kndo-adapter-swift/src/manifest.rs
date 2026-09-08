//! `Package.swift` read ONCE, with the grammar, into everything the project
//! model needs from it: a unit per target, and the packages the build resolves
//! from outside. SwiftPM requires every argument labeled, so a label is exact
//! where a scan would guess — a `path:` inside a `.when(...)` condition or a
//! comment never reads as a target's.

use kndo_contract::adapter::DependencyDeclaration;
use kndo_contract::manifest::{ManifestSink, Publication, Unit, UnitDep, UnitKind, UnitRoot};
use kndo_contract::vocab::ProjectPath;
use kndo_toolkit as tk;
use smol_str::SmolStr;
use tree_sitter::Node;

fn labeled_argument<'t>(call: Node<'t>, label: &str, src: &[u8]) -> Option<Node<'t>> {
    let suffix = tk::child_of_kind(call, "call_suffix")?;
    let args = tk::child_of_kind(suffix, "value_arguments")?;
    let mut c = args.walk();
    args.named_children(&mut c)
        .filter(|n| n.kind() == "value_argument")
        .find(|arg| {
            arg.child_by_field_name("name")
                .is_some_and(|n| tk::text(n, src) == label)
        })
        .and_then(|arg| arg.child_by_field_name("value"))
}

fn string_text(value: Node<'_>, src: &[u8]) -> Option<String> {
    let mut found = None;
    tk::walk(value, &mut |n| {
        if n.kind() == "line_str_text" && found.is_none() {
            found = Some(tk::text(n, src).to_string());
        }
    });
    found
}

fn package_name(url_or_path: &str) -> Option<&str> {
    let last = url_or_path.trim_end_matches('/').rsplit('/').next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty()).then_some(name)
}

/// The project structure `Package.swift` STATES: one unit per target, its
/// roots from `path:` or SwiftPM's predefined directory, its `exclude:` and
/// `sources:` narrowing, the sibling targets it compiles against, and — for a
/// test target — those same siblings as FRIENDS, because `@testable import`
/// reaches their `internal`.
///
/// Read with the grammar, not by line: SwiftPM requires every argument
/// labeled, so a label is exact where a scan would guess, and a `path:` inside
/// a `.when(...)` condition or a comment never reads as a target's.
pub fn structure(manifest: &ProjectPath, content: &[u8], out: &mut ManifestSink) {
    let language = tree_sitter_swift::LANGUAGE.into();
    let Some(tree) = tk::parse(&language, content) else {
        return;
    };
    let dir = manifest.as_str().rsplit_once('/').map_or("", |(d, _)| d);
    let join = |rel: &str| -> SmolStr {
        if dir.is_empty() {
            SmolStr::new(rel)
        } else {
            SmolStr::new(format!("{dir}/{rel}"))
        }
    };

    // Only what the `Package(...)` call itself labels: a `.target(name:)`
    // inside another target's `dependencies:` NAMES a target, it does not
    // declare one, and a scan of the whole tree reads vapor's `Vapor` five
    // times. SwiftPM labels both lists, so the labels are the reading.
    let Some(package) = calls_named(tree.root_node(), content, &["Package"])
        .into_iter()
        .next()
        .or_else(|| {
            // `Package(...)` is a plain identifier callee, not a `.member`.
            calls_with_identifier_callee(tree.root_node(), content, "Package")
                .into_iter()
                .next()
        })
    else {
        return;
    };

    // What the package RESOLVES from outside, named as the ecosystem imports
    // it: the last path segment of the url (any `.git` dropped) or of the
    // local path. Read from the `Package(...)` call's own `dependencies:`, so
    // a `.package(url:)` a target's list mentions is not counted twice and a
    // nested package's is not counted here at all.
    if let Some(list) = labeled_argument(package, "dependencies", content) {
        let mut names: Vec<SmolStr> = Vec::new();
        for call in elements_named(list, content, &[".package"]) {
            for label in ["url", "path"] {
                if let Some(value) = labeled_argument(call, label, content)
                    && let Some(text) = string_text(value, content)
                    && let Some(name) = package_name(&text)
                {
                    names.push(SmolStr::new(name));
                }
            }
        }
        names.sort_unstable();
        names.dedup();
        for name in names {
            out.dependency(DependencyDeclaration::name_only(name));
        }
    }

    // A product publishes the targets it names, so those targets' exported
    // API is consumed outside the project and the rest is the package's own.
    let mut published: Vec<String> = Vec::new();
    if let Some(products) = labeled_argument(package, "products", content) {
        for call in elements_named(products, content, &[".library", ".executable"]) {
            if let Some(list) = labeled_argument(call, "targets", content) {
                published.extend(strings_in(list, content));
            }
        }
    }

    let Some(targets) = labeled_argument(package, "targets", content) else {
        return;
    };
    for kind in [
        ".target",
        ".executableTarget",
        ".testTarget",
        ".macro",
        ".systemLibrary",
        ".plugin",
    ] {
        for call in elements_named(targets, content, &[kind]) {
            let Some(name) = labeled_argument(call, "name", content)
                .and_then(|n| string_text(n, content))
                .filter(|n| !n.is_empty())
            else {
                continue;
            };
            let test = kind == ".testTarget";
            // `path:` is the target's own directory; without one SwiftPM looks
            // in its predefined place, which is the convention this reads and
            // not a layout table: `Sources/<name>` for a target, `Tests/<name>`
            // for a test target.
            let root = labeled_argument(call, "path", content)
                .and_then(|n| string_text(n, content))
                .unwrap_or_else(|| {
                    let predefined = if test { "Tests" } else { "Sources" };
                    format!("{predefined}/{name}")
                });
            let mut roots = vec![join(&root)];
            // `sources:` narrows the target to the listed paths INSIDE its
            // directory: the roots become those, and the directory stops
            // being one.
            if let Some(list) = labeled_argument(call, "sources", content) {
                let sources = strings_in(list, content);
                if !sources.is_empty() {
                    roots = sources
                        .iter()
                        .map(|s| join(&format!("{root}/{s}")))
                        .collect();
                }
            }
            let mut excludes: Vec<SmolStr> = labeled_argument(call, "exclude", content)
                .map(|list| {
                    strings_in(list, content)
                        .iter()
                        .map(|s| join(&format!("{root}/{s}")))
                        .collect()
                })
                .unwrap_or_default();
            excludes.sort_unstable();
            excludes.dedup();
            let mut depends_on: Vec<SmolStr> = labeled_argument(call, "dependencies", content)
                .map(|list| target_dependencies(list, content))
                .unwrap_or_default();
            depends_on.sort_unstable();
            depends_on.dedup();
            out.unit(Unit {
                name: SmolStr::new(&name),
                kind: match kind {
                    ".testTarget" => UnitKind::Test,
                    ".executableTarget" => UnitKind::Executable,
                    ".macro" | ".plugin" => UnitKind::Tooling,
                    _ => UnitKind::Library,
                },
                roots: roots.into_iter().map(UnitRoot::from).collect(),
                excludes,
                entries: Vec::new(),
                // A test target's dependencies are its FRIENDS: `@testable
                // import Vapor` reaches Vapor's `internal`, which is exactly
                // what friendship means and what no path convention can know.
                depends_on: depends_on
                    .into_iter()
                    .map(|d| match test {
                        true => UnitDep::friend(d),
                        false => UnitDep::on(d),
                    })
                    .collect(),
                // A library product NAMES the targets it publishes, so a
                // target no product names is the package's own — the one
                // place SwiftPM states this, and stating it is not the same
                // as leaving it unsaid.
                publication: if published.iter().any(|t| t == &name) {
                    Publication::Published
                } else {
                    Publication::Unpublished
                },
                // A Swift module IS the target, and its files declare no
                // namespace clause: the module name is the scope forest's,
                // never a prefix on a path.
                namespace_root: None,
            });
        }
    }
}

/// The ELEMENTS of an array literal that are `.<name>(...)` calls with a
/// callee in `names`. Direct elements only: a target's dependency list holds
/// `.target(name:)` calls too, and those name a target rather than declaring
/// one.
fn elements_named<'t>(list: Node<'t>, src: &[u8], names: &[&str]) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut c = list.walk();
    for item in list.named_children(&mut c) {
        if item.kind() == "call_expression"
            && let Some(callee) = item.child(0)
            && callee.kind() == "prefix_expression"
            && names.contains(&tk::text(callee, src))
        {
            out.push(item);
        }
    }
    out
}

/// `Package(...)` and its kin: a call whose callee is a bare identifier.
fn calls_with_identifier_callee<'t>(node: Node<'t>, src: &[u8], name: &str) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.kind() == "call_expression"
            && let Some(callee) = n.child(0)
            && callee.kind() == "simple_identifier"
            && tk::text(callee, src) == name
        {
            out.push(n);
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            stack.push(child);
        }
    }
    out.sort_by_key(|n| n.start_byte());
    out
}

/// Every `.<name>(...)` implicit-member call under `node` whose callee is one
/// of `names`, in source order.
fn calls_named<'t>(node: Node<'t>, src: &[u8], names: &[&str]) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.kind() == "call_expression"
            && let Some(callee) = n.child(0)
            && callee.kind() == "prefix_expression"
            && names.contains(&tk::text(callee, src))
        {
            out.push(n);
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            stack.push(child);
        }
    }
    out.sort_by_key(|n| n.start_byte());
    out
}

/// The string literals directly inside an array literal, in order. A nested
/// call's own strings are skipped: `[.process("Resources")]` names no path of
/// this target's, and reading one would exclude a directory nobody excluded.
fn strings_in(list: Node<'_>, src: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut c = list.walk();
    for item in list.named_children(&mut c) {
        if item.kind() == "line_string_literal"
            && let Some(text) = string_text(item, src)
        {
            out.push(text);
        }
    }
    out
}

/// What a target's `dependencies:` names of THIS package: a bare string, and
/// `.target(name:)`. A `.product(name:package:)` is another package's, so it
/// names no unit here — the engine resolves a dependency name to a unit of
/// this project or to nothing, and nothing is the honest answer for one.
fn target_dependencies(list: Node<'_>, src: &[u8]) -> Vec<SmolStr> {
    let mut out: Vec<SmolStr> = strings_in(list, src).iter().map(SmolStr::new).collect();
    for call in calls_named(list, src, &[".target", ".byName"]) {
        if let Some(name) = labeled_argument(call, "name", src).and_then(|n| string_text(n, src)) {
            out.push(SmolStr::new(name));
        }
    }
    out
}
