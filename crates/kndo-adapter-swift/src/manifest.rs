//! `Package.swift` extraction: parsed as real Swift source (the
//! same grammar `extraction` uses), not a data format — the `Package(…)` initializer call's
//! labeled arguments are read as data, the SwiftPM analogue of Rust's structured `Cargo.toml`
//! parse rather than Gradle's line-scan (every argument SwiftPM itself requires is labeled, so
//! matching by label is exact, not a heuristic).

use kndo_adapter_toolkit::parsing::{find_child, text};
use kndo_core::adapter::{
    AdapterDiagnostic, DiagnosticLevel, ManifestDependency, ManifestFacts, ManifestRoot,
    ProjectPath, ResolveCtx,
};
use kndo_core::vocab::{Confidence, DependencyScope, RootKind};
use smol_str::SmolStr;
use tree_sitter::Node;

/// The path segment immediately following `Sources/` or `Tests/` — SwiftPM's Standard
/// Directory Layout target name. `None` for a file outside both
/// conventions.
pub(crate) fn unit_for_path(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split('/').collect();
    let idx = segments
        .iter()
        .position(|s| *s == "Sources" || *s == "Tests")?;
    segments.get(idx + 1).map(|s| s.to_string())
}

pub(crate) fn extract(path: &str, content: &[u8], ctx: &ResolveCtx<'_>) -> ManifestFacts {
    let mut out = ManifestFacts::default();
    let Some(tree) = crate::parsing::parse(content) else {
        out.diagnostics.push(diag("failed to parse Package.swift"));
        return out;
    };
    let Some(pkg_call) = find_package_call(tree.root_node(), content) else {
        out.diagnostics
            .push(diag("no Package(...) initializer found"));
        return out;
    };

    out.package_name = labeled_arg(pkg_call, "name", content)
        .and_then(|v| string_literal_text(v, content))
        .map(SmolStr::new);
    let products = labeled_arg(pkg_call, "products", content);
    out.private = !has_library_product(products, content);
    out.dependencies =
        collect_package_dependencies(labeled_arg(pkg_call, "dependencies", content), content);
    let targets = labeled_arg(pkg_call, "targets", content);
    let target_names = collect_target_names(targets, content);
    out.workspace_members = target_names.iter().map(SmolStr::new).collect();

    let dir = kndo_adapter_toolkit::paths::dirname(path);
    let custom_paths = collect_target_paths(targets, content);
    for name in exported_target_names(products, content) {
        // A target's sources live under its `path:` argument when declared (e.g.
        // `.target(name: "MyLib", path: "Source")`), under the SwiftPM
        // Standard Directory Layout `Sources/<name>` otherwise.
        let source_root = custom_paths
            .get(&name)
            .cloned()
            .unwrap_or_else(|| format!("Sources/{name}"));
        promote_target_roots(dir, &source_root, ctx, &mut out);
    }
    // Custom `path:` targets sit outside the `Sources/<name>` convention that
    // `unit_for_path` reads at extraction time, so their files carry no unit — and with it
    // no same-unit resolution at all (the whole module goes dark to cross-file references).
    // The manifest is the only place the mapping exists; assembly applies it to files whose
    // extraction left `unit` unset (ManifestFacts::unit_overrides).
    for (name, custom) in &custom_paths {
        let prefix = if dir.is_empty() {
            custom.clone()
        } else {
            format!("{dir}/{custom}")
        };
        out.unit_overrides
            .push((ProjectPath(SmolStr::new(prefix)), SmolStr::new(name)));
    }
    out
}

// ---------------------------------------------------------------- the Package(...) call

fn find_package_call<'a>(node: Node<'a>, src: &[u8]) -> Option<Node<'a>> {
    if node.kind() == "call_expression" && call_callee_name(node, src) == Some("Package") {
        return Some(node);
    }
    for child in node.children(&mut node.walk()) {
        if let Some(found) = find_package_call(child, src) {
            return Some(found);
        }
    }
    None
}

/// A call's callee name — a plain identifier (`Package(...)`) or the target of a dot-prefixed
/// implicit-member call (`.library(...)`, `.package(...)`), SwiftPM's own factory-method style
/// for every `products`/`dependencies`/`targets` array entry.
fn call_callee_name<'a>(call: Node, src: &'a [u8]) -> Option<&'a str> {
    let callee = call.child(0)?;
    if callee.kind() == "prefix_expression" {
        let target = callee.child_by_field_name("target")?;
        return Some(text(target, src));
    }
    Some(text(callee, src))
}

fn find_value_arguments(call: Node) -> Option<Node> {
    let suffix = find_child(call, "call_suffix")?;
    find_child(suffix, "value_arguments")
}

/// One labeled argument's `value` field — every `Package(...)`/factory-call argument SwiftPM
/// accepts is labeled, so this is an exact lookup, never a positional guess.
fn labeled_arg<'a>(call: Node<'a>, label: &str, src: &[u8]) -> Option<Node<'a>> {
    let value_arguments = find_value_arguments(call)?;
    let arg = value_arguments
        .children(&mut value_arguments.walk())
        .filter(|n| n.kind() == "value_argument")
        .find(|arg| arg_label_matches(*arg, label, src))?;
    arg.child_by_field_name("value")
}

fn arg_label_matches(arg: Node, label: &str, src: &[u8]) -> bool {
    let Some(name_node) = arg.child_by_field_name("name") else {
        return false;
    };
    text(name_node, src) == label
}

fn string_literal_text(node: Node, src: &[u8]) -> Option<String> {
    let text_node = find_child(node, "line_str_text")?;
    Some(text(text_node, src).to_string())
}

// ---------------------------------------------------------------- products / private

fn has_library_product(products: Option<Node>, src: &[u8]) -> bool {
    let Some(products) = products else {
        return false;
    };
    products
        .children(&mut products.walk())
        .filter(|n| n.kind() == "call_expression")
        .any(|el| call_callee_name(el, src) == Some("library"))
}

fn exported_target_names(products: Option<Node>, src: &[u8]) -> Vec<String> {
    let Some(products) = products else {
        return Vec::new();
    };
    products
        .children(&mut products.walk())
        .filter(|n| n.kind() == "call_expression")
        .filter(|call| call_callee_name(*call, src) == Some("library"))
        .filter_map(|call| labeled_arg(call, "targets", src))
        .flat_map(|targets_arr| string_array(targets_arr, src))
        .collect()
}

fn string_array(node: Node, src: &[u8]) -> Vec<String> {
    node.children(&mut node.walk())
        .filter_map(|c| string_literal_text(c, src))
        .collect()
}

// ---------------------------------------------------------------- dependencies

const VERSION_ARG_LABELS: &[&str] = &["from", "exact", "branch", "revision"];

fn collect_package_dependencies(deps: Option<Node>, src: &[u8]) -> Vec<ManifestDependency> {
    let Some(deps) = deps else {
        return Vec::new();
    };
    deps.children(&mut deps.walk())
        .filter(|n| n.kind() == "call_expression")
        .filter_map(|call| package_dependency(call, src))
        .collect()
}

/// `.package(url: "...", from/exact/branch/revision: "...")` — `name` is a best-effort identity
/// derived from the URL's last path segment (the repository name and the module(s)
/// it exports aren't guaranteed identical, a documented approximation).
fn package_dependency(call: Node, src: &[u8]) -> Option<ManifestDependency> {
    if call_callee_name(call, src) != Some("package") {
        return None;
    }
    let url_node = labeled_arg(call, "url", src)?;
    let url = string_literal_text(url_node, src)?;
    Some(ManifestDependency {
        name: SmolStr::new(dependency_name_from_url(&url)),
        version_req: SmolStr::new(dependency_version_req(call, src)),
        scope: DependencyScope::Prod,
        inherited: false,
    })
}

fn dependency_name_from_url(url: &str) -> String {
    let last = url.rsplit('/').next().unwrap_or(url);
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

fn dependency_version_req(call: Node, src: &[u8]) -> String {
    VERSION_ARG_LABELS
        .iter()
        .find_map(|label| labeled_arg(call, label, src).and_then(|v| string_literal_text(v, src)))
        .unwrap_or_else(|| "*".to_string())
}

// ---------------------------------------------------------------- targets & root promotion

fn collect_target_names(targets: Option<Node>, src: &[u8]) -> Vec<String> {
    let Some(targets) = targets else {
        return Vec::new();
    };
    targets
        .children(&mut targets.walk())
        .filter(|n| n.kind() == "call_expression")
        .filter_map(|call| target_name(call, src))
        .collect()
}

fn target_name(call: Node, src: &[u8]) -> Option<String> {
    labeled_arg(call, "name", src).and_then(|v| string_literal_text(v, src))
}

/// Each target's explicit `path:` argument, when declared — the override for the Standard
/// Directory Layout's `Sources/<name>`.
fn collect_target_paths(
    targets: Option<Node>,
    src: &[u8],
) -> std::collections::HashMap<String, String> {
    let Some(targets) = targets else {
        return Default::default();
    };
    targets
        .children(&mut targets.walk())
        .filter(|n| n.kind() == "call_expression")
        .filter_map(|call| {
            let name = target_name(call, src)?;
            let path = labeled_arg(call, "path", src).and_then(|v| string_literal_text(v, src))?;
            Some((name, path))
        })
        .collect()
}

/// One `ManifestRoot{Production, Certain}` per non-test `.swift` file under a publicly-
/// exported target's source tree (its `path:` override or `Sources/<target>`) — the same
/// per-file promotion mechanism Java/Kotlin use, parameterized
/// per-target instead of per-manifest.
fn promote_target_roots(
    dir: &str,
    source_rel: &str,
    ctx: &ResolveCtx<'_>,
    out: &mut ManifestFacts,
) {
    let source_root = join(dir, source_rel);
    let mut files: Vec<ProjectPath> = ctx
        .files_under(&source_root.0)
        .filter(|p| p.0.ends_with(".swift"))
        .filter(|p| !is_vendored(p.0.as_str()))
        .cloned()
        .collect();
    files.sort(); // deterministic output ordering
    for target_file in files {
        out.roots.push(ManifestRoot {
            kind: RootKind::Production,
            target: target_file,
            confidence: Confidence::Certain,
        });
    }
}

fn join(dir: &str, rel: &str) -> ProjectPath {
    if dir.is_empty() {
        ProjectPath(SmolStr::new(rel))
    } else {
        ProjectPath(SmolStr::new(format!("{dir}/{rel}")))
    }
}

fn is_vendored(path: &str) -> bool {
    kndo_adapter_toolkit::classify::UNIVERSAL_VENDORED_DIRS
        .iter()
        .any(|d| path.split('/').any(|seg| seg == *d))
}

// ---------------------------------------------------------------- small tree helpers

fn diag(message: &str) -> AdapterDiagnostic {
    AdapterDiagnostic {
        level: DiagnosticLevel::Warn,
        message: message.to_string(),
        span: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;

    fn ctx_with(files: &[&str]) -> FxHashSet<ProjectPath> {
        files
            .iter()
            .map(|f| ProjectPath(SmolStr::new(*f)))
            .collect()
    }

    fn facts(content: &str, files: &[&str]) -> ManifestFacts {
        let known = ctx_with(files);
        let ctx = ResolveCtx::new(&known);
        extract("Package.swift", content.as_bytes(), &ctx)
    }

    #[test]
    fn unit_for_path_reads_the_sources_and_tests_segment() {
        assert_eq!(
            unit_for_path("Sources/MyLib/Deep/File.swift").as_deref(),
            Some("MyLib")
        );
        assert_eq!(
            unit_for_path("Tests/MyLibTests/FileTests.swift").as_deref(),
            Some("MyLibTests")
        );
        assert_eq!(unit_for_path("scripts/loose.swift"), None);
    }

    const PACKAGE_SRC: &str = r#"
// swift-tools-version:5.7
import PackageDescription

let package = Package(
    name: "MyLib",
    products: [
        .library(name: "MyLib", targets: ["MyLib"])
    ],
    dependencies: [
        .package(url: "https://github.com/foo/bar.git", from: "1.0.0")
    ],
    targets: [
        .target(name: "MyLib", dependencies: ["Bar"]),
        .testTarget(name: "MyLibTests", dependencies: ["MyLib"])
    ]
)
"#;

    #[test]
    fn package_name_products_dependencies_and_targets_are_extracted() {
        let f = facts(PACKAGE_SRC, &[]);
        assert_eq!(f.package_name.as_deref(), Some("MyLib"));
        assert!(!f.private, "a library product makes the package public");
        assert_eq!(f.workspace_members, vec!["MyLib", "MyLibTests"]);
        let dep = f.dependencies.iter().find(|d| d.name == "bar").unwrap();
        assert_eq!(dep.version_req, "1.0.0");
        assert_eq!(dep.scope, DependencyScope::Prod);
    }

    #[test]
    fn a_package_with_no_library_product_is_private() {
        let src = r#"
let package = Package(
    name: "App",
    targets: [
        .executableTarget(name: "App", dependencies: [])
    ]
)
"#;
        let f = facts(src, &[]);
        assert!(f.private);
    }

    #[test]
    fn only_exported_targets_get_root_promotion() {
        let f = facts(
            PACKAGE_SRC,
            &[
                "Sources/MyLib/A.swift",
                "Sources/MyLibTests/Ignored.swift", // not a real convention, just proves scoping
                "Tests/MyLibTests/ATests.swift",
            ],
        );
        let roots: Vec<&str> = f.roots.iter().map(|r| r.target.0.as_str()).collect();
        assert!(roots.contains(&"Sources/MyLib/A.swift"));
        assert!(!roots.contains(&"Tests/MyLibTests/ATests.swift"));
    }

    #[test]
    fn dependency_version_req_falls_back_across_labels() {
        let src = r#"
let package = Package(
    name: "P",
    dependencies: [
        .package(url: "https://example.com/x.git", branch: "main")
    ]
)
"#;
        let f = facts(src, &[]);
        let dep = f.dependencies.iter().find(|d| d.name == "x").unwrap();
        assert_eq!(dep.version_req, "main");
    }
}
