//! Extraction: one recursive item pass (descending into `mod` bodies, queueing
//! `impl` blocks until their types are known), then one pruned full-tree walk for
//! references, comments, and qualified-path edges. Everything ambiguous degrades
//! toward keep-alive: a name we cannot classify is a use, a binding we cannot name is
//! never declared (and so never accused). Deliberately undeclared: `macro_rules!`
//! macros (textual scope), struct fields and enum variants (derives dispatch on them
//! invisibly) — never accusing what the grammar alone cannot prove dead.

use kndo_contract::evidence::{
    DeclarationId, EvidenceSink, ImportBinding, ImportShape, ImportTarget, Reach, RefKind,
    RootKind, RootTarget, SymbolKind,
};
use kndo_contract::vocab::Confidence;
use kndo_toolkit as tk;
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node;

pub fn extract(
    path: &kndo_contract::vocab::ProjectPath,
    source: &[u8],
    tree: &tree_sitter::Tree,
    out: &mut EvidenceSink,
) {
    let root = tree.root_node();
    let mut cx = ItemPass {
        source,
        main_root_kind: main_root_kind(path),
        unimportable_root: unimportable_crate_root(path),
        types: BTreeMap::new(),
        impls: Vec::new(),
        redirects: BTreeMap::new(),
        uses: Vec::new(),
        stack: Vec::new(),
        out,
    };
    cx.items(root);
    let impls = std::mem::take(&mut cx.impls);
    for node in impls {
        cx.impl_members(node);
    }
    // `use` trees resolve against the whole file's `mod` statements (a
    // `#[path]`-redirect can be declared after the `use` that rides its alias), so
    // they process once every item has been seen.
    let uses = std::mem::take(&mut cx.uses);
    for (node, stack) in uses {
        cx.use_declaration(node, &stack);
    }
    references_and_comments(root, source, out);
}

/// What a top-level `fn main` anchors, by cargo's own path conventions: a build
/// script and an example are tooling targets; anywhere else it is the binary entry.
fn main_root_kind(path: &kndo_contract::vocab::ProjectPath) -> RootKind {
    let p = path.as_str();
    let name = p.rsplit('/').next().unwrap_or(p);
    if name == "build.rs" || p.starts_with("examples/") || p.contains("/examples/") {
        RootKind::Tooling
    } else {
        RootKind::Production
    }
}

/// Whether this file roots a target nothing can import — a binary, build script,
/// test, bench, or example crate. `pub mod` in such a file publishes to no one, so
/// its edge stays mute instead of re-exporting the child's surface.
fn unimportable_crate_root(path: &kndo_contract::vocab::ProjectPath) -> bool {
    let p = path.as_str();
    let name = p.rsplit('/').next().unwrap_or(p);
    let in_dir = |d: &str| p.starts_with(&format!("{d}/")) || p.contains(&format!("/{d}/"));
    name == "main.rs"
        || name == "build.rs"
        || in_dir("bin")
        || in_dir("examples")
        || in_dir("tests")
        || in_dir("benches")
}

struct ItemPass<'a, 'o> {
    source: &'a [u8],
    main_root_kind: RootKind,
    unimportable_root: bool,
    /// Type name → its declaration, for wiring `impl` members to their owner.
    types: BTreeMap<String, DeclarationId>,
    impls: Vec<Node<'a>>,
    /// `#[path = "…"]`-redirected module names → their real path segments. `use`
    /// paths through the alias substitute these (`use self::imp::*` where `mod imp`
    /// points at `disabled.rs`) — one variant per redirect, since cfg gates can
    /// declare the same alias twice.
    redirects: BTreeMap<String, Vec<Vec<String>>>,
    uses: Vec<(Node<'a>, Vec<String>)>,
    /// Inline-`mod` names enclosing the current item. `self::`/`super::` in a `use`
    /// or a `mod foo;` mean something different inside `mod tests { ... }` than at
    /// the top of the file; rebasing against this stack keeps the emitted specifier
    /// file-relative.
    stack: Vec<String>,
    out: &'o mut EvidenceSink,
}

impl<'a> ItemPass<'a, '_> {
    fn items(&mut self, container: Node<'a>) {
        let mut cursor = container.walk();
        let children: Vec<Node<'a>> = container.named_children(&mut cursor).collect();
        for item in children {
            self.item(item);
        }
    }

    fn item(&mut self, item: Node<'a>) {
        let attrs = attributes_of(item, self.source);
        let reach = if has_visibility(item) {
            Reach::Exported
        } else {
            Reach::Private
        };
        match item.kind() {
            "function_item" => {
                if let Some(n) = item.child_by_field_name("name") {
                    let name = tk::text(n, self.source);
                    let id =
                        self.out
                            .declaration(name, SymbolKind::Function, tk::span(item), reach);
                    self.out.metrics(id, function_metrics(item));
                    root_for_attrs(&attrs, id, self.out);
                    // A top-level `fn main` is the language's entry convention: in
                    // any target the runtime calls it, and extraction cannot see
                    // the manifest that would say which files are targets. Probable
                    // — convention, not this file's own statement — and colored by
                    // the path's own convention (build scripts and examples are
                    // tooling).
                    if name == "main" && self.stack.is_empty() {
                        self.out.root(
                            RootTarget::Declaration(id),
                            self.main_root_kind,
                            Confidence::Probable,
                        );
                    }
                }
            }
            "struct_item" | "enum_item" | "union_item" | "trait_item" | "type_item" => {
                if let Some(n) = item.child_by_field_name("name") {
                    let name = tk::text(n, self.source);
                    let id = self
                        .out
                        .declaration(name, SymbolKind::Type, tk::span(item), reach);
                    self.types.entry(name.to_string()).or_insert(id);
                    root_for_attrs(&attrs, id, self.out);
                    if item.kind() == "trait_item" {
                        self.trait_members(item, id, reach);
                    }
                }
            }
            "const_item" | "static_item" => {
                if let Some(n) = item.child_by_field_name("name") {
                    let kind = if item.kind() == "const_item" {
                        SymbolKind::Constant
                    } else {
                        SymbolKind::Variable
                    };
                    let id =
                        self.out
                            .declaration(tk::text(n, self.source), kind, tk::span(item), reach);
                    root_for_attrs(&attrs, id, self.out);
                }
            }
            "mod_item" => {
                if let Some(n) = item.child_by_field_name("name") {
                    let name = tk::text(n, self.source);
                    match item.child_by_field_name("body") {
                        Some(body) => {
                            let id = self.out.declaration(
                                name,
                                SymbolKind::Module,
                                tk::span(item),
                                reach,
                            );
                            root_for_attrs(&attrs, id, self.out);
                            self.stack.push(name.to_string());
                            self.items(body);
                            self.stack.pop();
                        }
                        // `mod foo;` is module-system plumbing, not an accusable
                        // declaration — the same posture as an import statement. A
                        // private mod emits the bare edge: the module lives in its
                        // own file, and binding nothing keeps that file reachable
                        // without handing over its whole surface. `pub mod foo;`
                        // RE-PUBLISHES the child's exported surface through this
                        // crate's own — a lib's pub-mod tree is its published API —
                        // so the edge is a reexport-all. `#[path = "other.rs"]`
                        // redirects where the file lives, relative to this module's
                        // own directory.
                        None => {
                            let mut segments = vec!["self".to_string()];
                            segments.extend(self.stack.iter().cloned());
                            match attrs.iter().find_map(|a| path_attribute(a)) {
                                Some(redirect) => {
                                    self.redirects
                                        .entry(name.to_string())
                                        .or_default()
                                        .push(redirect.clone());
                                    segments.extend(redirect);
                                }
                                None => segments.push(name.to_string()),
                            }
                            let shape = if reach == Reach::Exported && !self.unimportable_root {
                                ImportShape::ReexportAll
                            } else {
                                ImportShape::Bindings(Vec::new())
                            };
                            self.out.import(
                                ImportTarget::Relative(SmolStr::new(segments.join("::"))),
                                shape,
                                tk::span(item),
                                Confidence::Certain,
                            );
                        }
                    }
                }
            }
            "impl_item" => self.impls.push(item),
            "use_declaration" => self.uses.push((item, self.stack.clone())),
            "extern_crate_declaration" => {
                if let Some(n) = item.child_by_field_name("name") {
                    let name = tk::text(n, self.source);
                    let local = item
                        .child_by_field_name("alias")
                        .map(|a| tk::text(a, self.source))
                        .unwrap_or(name);
                    self.out.import(
                        ImportTarget::Package(SmolStr::new(name)),
                        ImportShape::Namespace {
                            local: SmolStr::new(local),
                        },
                        tk::span(item),
                        Confidence::Certain,
                    );
                }
            }
            _ => {}
        }
    }

    /// Trait definition methods: members of the trait, reached through it — their
    /// personal reach is the trait's, since trait items have no modifiers of their
    /// own.
    fn trait_members(&mut self, trait_item: Node<'a>, owner: DeclarationId, reach: Reach) {
        let Some(body) = trait_item.child_by_field_name("body") else {
            return;
        };
        let mut c = body.walk();
        let members: Vec<Node<'a>> = body.named_children(&mut c).collect();
        for m in members {
            if m.kind() != "function_item" && m.kind() != "function_signature_item" {
                continue;
            }
            let Some(n) = m.child_by_field_name("name") else {
                continue;
            };
            let id = self.out.declaration(
                tk::text(n, self.source),
                SymbolKind::Method,
                tk::span(m),
                reach,
            );
            self.out.member_of(id, owner);
            if m.child_by_field_name("body").is_some() {
                self.out.metrics(id, function_metrics(m));
            }
        }
    }

    /// Inherent `impl` blocks declare methods and associated items, owned by their
    /// type when it is declared in this file (`impl` for a type declared elsewhere
    /// leaves the owner empty — still a member, judged in the global pool). Trait
    /// impls (`impl T for X`) declare nothing: their bodies are the trait's shape,
    /// and accusing a required method would accuse the trait bound.
    fn impl_members(&mut self, impl_item: Node<'a>) {
        if impl_item.child_by_field_name("trait").is_some() {
            return;
        }
        let owner = impl_item
            .child_by_field_name("type")
            .and_then(|t| type_name(t, self.source))
            .and_then(|name| self.types.get(&name).copied());
        let Some(body) = impl_item.child_by_field_name("body") else {
            return;
        };
        let mut c = body.walk();
        let members: Vec<Node<'a>> = body.named_children(&mut c).collect();
        for m in members {
            let (kind, has_metrics) = match m.kind() {
                "function_item" => (SymbolKind::Method, true),
                "const_item" => (SymbolKind::Constant, false),
                _ => continue,
            };
            let Some(n) = m.child_by_field_name("name") else {
                continue;
            };
            let reach = if has_visibility(m) {
                Reach::Exported
            } else {
                Reach::Private
            };
            let id = self
                .out
                .declaration(tk::text(n, self.source), kind, tk::span(m), reach);
            if let Some(owner) = owner {
                self.out.member_of(id, owner);
            }
            if has_metrics {
                self.out.metrics(id, function_metrics(m));
            }
            let attrs = attributes_of(m, self.source);
            root_for_attrs(&attrs, id, self.out);
        }
    }

    fn use_declaration(&mut self, item: Node<'a>, stack: &[String]) {
        let Some(argument) = item.child_by_field_name("argument") else {
            return;
        };
        let public = has_visibility(item);
        let mut leaves = Vec::new();
        expand_use(argument, self.source, &Vec::new(), &mut leaves);
        let mut expanded = Vec::new();
        for mut leaf in leaves {
            leaf.segments = rebase(leaf.segments, stack);
            // A leaf whose first real segment is a `#[path]`-redirected alias walks
            // the redirect's file instead, one variant per redirect.
            let keyword_run = leaf
                .segments
                .iter()
                .take_while(|s| matches!(s.as_str(), "crate" | "self" | "super"))
                .count();
            match leaf
                .segments
                .get(keyword_run)
                .and_then(|s| self.redirects.get(s))
            {
                Some(variants) => {
                    for v in variants {
                        let mut segments = leaf.segments[..keyword_run].to_vec();
                        segments.extend(v.iter().cloned());
                        segments.extend(leaf.segments[keyword_run + 1..].iter().cloned());
                        expanded.push(UseLeaf {
                            segments,
                            alias: leaf.alias.clone(),
                            glob: leaf.glob,
                        });
                    }
                }
                None => expanded.push(leaf),
            }
        }
        for leaf in expanded {
            if leaf.segments.is_empty() {
                continue;
            }
            let target = target_for(&leaf.segments);
            let last = leaf.segments.last().unwrap().clone();
            let local = leaf.alias.unwrap_or_else(|| last.clone());
            let shape = match (leaf.glob, public) {
                (true, true) => ImportShape::ReexportAll,
                (true, false) => ImportShape::Glob,
                (false, true) => ImportShape::Reexport(vec![ImportBinding {
                    imported: SmolStr::new(last),
                    local: SmolStr::new(local),
                }]),
                // One item imported keeps its file's whole exported surface: alias
                // scopes cannot be re-derived from one file, so the honest edge is
                // the namespace one. The named binding rides along as a second
                // record — it is what keeps a PRIVATE item a child module legally
                // imports from its parent, which the namespace record (exported
                // surface only) cannot.
                (false, false) => {
                    self.out.import(
                        target.clone(),
                        ImportShape::Bindings(vec![ImportBinding {
                            imported: SmolStr::new(last.clone()),
                            local: SmolStr::new(local.clone()),
                        }]),
                        tk::span(item),
                        Confidence::Certain,
                    );
                    ImportShape::Namespace {
                        local: SmolStr::new(local),
                    }
                }
            };
            self.out
                .import(target, shape, tk::span(item), Confidence::Certain);
        }
    }
}

struct UseLeaf {
    segments: Vec<String>,
    alias: Option<String>,
    glob: bool,
}

/// Rewrites a path's leading `self`/`super` run against the inline-`mod` stack, so
/// the emitted specifier is file-relative: `use super::*` inside `mod tests { .. }`
/// names this file's own module, not its parent. `crate::` and package paths pass
/// through untouched.
fn rebase(segments: Vec<String>, stack: &[String]) -> Vec<String> {
    if stack.is_empty() || segments.is_empty() {
        return segments;
    }
    let (supers, rest_from) = match segments[0].as_str() {
        "self" => (0usize, 1usize),
        "super" => {
            let supers = segments
                .iter()
                .take_while(|s| s.as_str() == "super")
                .count();
            (supers, supers)
        }
        _ => return segments,
    };
    let keep = stack.len().saturating_sub(supers);
    let supers_left = supers.saturating_sub(stack.len());
    let mut out = Vec::new();
    if supers_left > 0 {
        out.extend(std::iter::repeat_n("super".to_string(), supers_left));
    } else {
        out.push("self".to_string());
        out.extend(stack[..keep].iter().cloned());
    }
    out.extend(segments[rest_from..].iter().cloned());
    out
}

/// Recursively expands a `use` tree into leaves: `use a::{b, c::*, d as e, self}`
/// yields one leaf per name the statement brings into scope, each with its full
/// path. A `self` leaf names the module the prefix already names.
fn expand_use(node: Node<'_>, source: &[u8], prefix: &[String], out: &mut Vec<UseLeaf>) {
    match node.kind() {
        "identifier" | "crate" | "super" | "metavariable" => {
            let mut segments = prefix.to_vec();
            segments.push(tk::text(node, source).to_string());
            out.push(UseLeaf {
                segments,
                alias: None,
                glob: false,
            });
        }
        "self" => out.push(UseLeaf {
            segments: prefix.to_vec(),
            alias: None,
            glob: false,
        }),
        "scoped_identifier" => {
            let mut segments = prefix.to_vec();
            flatten_path(node, source, &mut segments);
            out.push(UseLeaf {
                segments,
                alias: None,
                glob: false,
            });
        }
        "use_as_clause" => {
            let alias = node
                .child_by_field_name("alias")
                .map(|a| tk::text(a, source).to_string());
            let mut inner = Vec::new();
            if let Some(path) = node.child_by_field_name("path") {
                expand_use(path, source, prefix, &mut inner);
            }
            for mut leaf in inner {
                leaf.alias = alias.clone();
                out.push(leaf);
            }
        }
        "use_list" => {
            let mut c = node.walk();
            for child in node.named_children(&mut c) {
                expand_use(child, source, prefix, out);
            }
        }
        "scoped_use_list" => {
            let mut segments = prefix.to_vec();
            if let Some(path) = node.child_by_field_name("path") {
                flatten_path(path, source, &mut segments);
            }
            if let Some(list) = node.child_by_field_name("list") {
                expand_use(list, source, &segments, out);
            }
        }
        "use_wildcard" => {
            let mut segments = prefix.to_vec();
            if let Some(path) = node.named_child(0) {
                flatten_path(path, source, &mut segments);
            }
            out.push(UseLeaf {
                segments,
                alias: None,
                glob: true,
            });
        }
        _ => {}
    }
}

/// `a::b::c` (however nested) appended to `into` as its segments.
fn flatten_path(node: Node<'_>, source: &[u8], into: &mut Vec<String>) {
    match node.kind() {
        "scoped_identifier" | "scoped_type_identifier" => {
            if let Some(path) = node.child_by_field_name("path") {
                flatten_path(path, source, into);
            }
            if let Some(name) = node.child_by_field_name("name") {
                into.push(tk::text(name, source).to_string());
            }
        }
        "identifier" | "type_identifier" | "crate" | "self" | "super" | "metavariable" => {
            into.push(tk::text(node, source).to_string());
        }
        "generic_type" => {
            if let Some(inner) = node.child_by_field_name("type") {
                flatten_path(inner, source, into);
            }
        }
        _ => {}
    }
}

/// `crate::`/`self::`/`super::` paths stay project-relative; anything else names a
/// package first — the resolver falls back to module-relative when no package
/// matches, so a sibling module used qualified still links.
fn target_for(segments: &[String]) -> ImportTarget {
    let joined = segments.join("::");
    if matches!(segments[0].as_str(), "crate" | "self" | "super") {
        ImportTarget::Relative(SmolStr::new(joined))
    } else {
        ImportTarget::Package(SmolStr::new(joined))
    }
}

/// The name under `impl NAME { .. }`, unwrapping generics; `None` for scoped or
/// exotic types (their declarations live elsewhere).
fn type_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "type_identifier" => Some(tk::text(node, source).to_string()),
        "generic_type" => node
            .child_by_field_name("type")
            .and_then(|t| type_name(t, source)),
        _ => None,
    }
}

fn has_visibility(item: Node<'_>) -> bool {
    let mut c = item.walk();
    item.named_children(&mut c)
        .any(|ch| ch.kind() == "visibility_modifier")
}

/// `path = "foo/bar.rs"` from a `#[path]` attribute, as module-path segments
/// (`["foo", "bar"]`) relative to the declaring module's directory.
fn path_attribute(attr: &str) -> Option<Vec<String>> {
    let rest = attr.strip_prefix("path")?.trim_start().strip_prefix('=')?;
    let quoted = rest.trim().strip_prefix('"')?.strip_suffix('"')?;
    let stem = quoted.strip_suffix(".rs").unwrap_or(quoted);
    Some(stem.split('/').map(str::to_string).collect())
}

/// Outer `#[...]` attribute paths of an item (`test`, `tokio::test`, `cfg`, ...),
/// argument lists included as written.
fn attributes_of(item: Node<'_>, source: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut prev = item.prev_named_sibling();
    while let Some(p) = prev {
        match p.kind() {
            "attribute_item" => {
                if let Some(attr) = p.named_child(0) {
                    out.push(tk::text(attr, source).to_string());
                }
            }
            // Doc comments may sit between an item and its attributes.
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        prev = p.prev_named_sibling();
    }
    out
}

/// Attribute-declared liveness. Test-runner attributes (`#[test]`, `#[tokio::test]`,
/// `#[bench]`) and `#[cfg(test)]` gates root a declaration as Test; linkage and
/// runtime attributes (`#[no_mangle]`, `#[global_allocator]`, ...) mean something
/// outside the graph calls it — a Production root. Certain either way: the attribute
/// is the code's own statement.
fn root_for_attrs(attrs: &[String], id: DeclarationId, out: &mut EvidenceSink) {
    const PRODUCTION: [&str; 10] = [
        "no_mangle",
        "export_name",
        "global_allocator",
        "panic_handler",
        "alloc_error_handler",
        "used",
        "proc_macro",
        "proc_macro_derive",
        "proc_macro_attribute",
        "start",
    ];
    for attr in attrs {
        let path = attr.split('(').next().unwrap_or(attr).trim();
        let last = path.rsplit("::").next().unwrap_or(path);
        if last == "test" || last == "bench" {
            out.root(
                RootTarget::Declaration(id),
                RootKind::Test,
                Confidence::Certain,
            );
            return;
        }
        // `#[tokio::main]`-style attributes mark an entry point.
        if last == "main" || PRODUCTION.contains(&path) {
            out.root(
                RootTarget::Declaration(id),
                RootKind::Production,
                Confidence::Certain,
            );
            return;
        }
        if path == "cfg" && attr.contains("test") && !attr.contains("not(test)") {
            out.root(
                RootTarget::Declaration(id),
                RootKind::Test,
                Confidence::Certain,
            );
            return;
        }
    }
}

/// Metrics over one function-shaped node. Leaves are normalized by class —
/// identifiers, strings and numbers collapse to their kind — so Type-2 clones
/// (renamed, re-valued) fingerprint identically; everything else keeps its literal
/// kind. Comments never count.
fn function_metrics(node: Node<'_>) -> kndo_contract::evidence::FunctionMetrics {
    let mut token_hashes: Vec<u64> = Vec::new();
    let mut cyclomatic = 1u32;
    tk::walk(node, &mut |n| {
        match n.kind() {
            "if_expression" | "while_expression" | "for_expression" | "loop_expression"
            | "match_arm" | "try_expression" => {
                cyclomatic += 1;
            }
            "binary_expression" => {
                let mut c = n.walk();
                if n.children(&mut c)
                    .any(|ch| matches!(ch.kind(), "&&" | "||"))
                {
                    cyclomatic += 1;
                }
            }
            _ => {}
        }
        if n.child_count() == 0 {
            let class = match n.kind() {
                "identifier"
                | "field_identifier"
                | "type_identifier"
                | "shorthand_field_identifier" => "id",
                "string_content" => "str",
                "integer_literal" | "float_literal" => "num",
                "line_comment" | "block_comment" => return,
                other => other,
            };
            token_hashes.push(tk::fnv1a(class.as_bytes()));
        }
    });
    let loc = (node.end_position().row - node.start_position().row + 1) as u32;
    kndo_contract::evidence::FunctionMetrics {
        cyclomatic,
        loc,
        token_count: token_hashes.len() as u32,
        fingerprints: tk::winnow(&token_hashes, 5, 4),
    }
}

fn references_and_comments(root: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    let mut seen_paths: BTreeSet<String> = BTreeSet::new();
    tk::walk_pruned(
        root,
        &["use_declaration", "extern_crate_declaration"],
        &mut |n| {
            match n.kind() {
                "line_comment" | "block_comment" => {
                    comment(n, source, out);
                    return;
                }
                "scoped_identifier" | "scoped_type_identifier" => {
                    path_import(n, source, &mut seen_paths, out);
                    // Fall through is deliberate in spirit: the identifiers inside the
                    // path still land as references via their own visits.
                    return;
                }
                // Inline format arguments (`"{VERSION}"`) are real uses the string hides.
                "string_content" => {
                    format_arg_references(n, source, out);
                    return;
                }
                // Attribute strings name items by convention (`schemars(schema_with =
                // "f")`, `serde(with = "m")`): every identifier-shaped word is a use.
                "attribute_item" => {
                    attribute_string_references(n, source, out);
                    return;
                }
                _ => {}
            }
            if !matches!(
                n.kind(),
                "identifier" | "type_identifier" | "field_identifier"
            ) {
                return;
            }
            let Some(parent) = n.parent() else {
                return;
            };
            if !is_use(n, parent) {
                return;
            }
            out.reference(tk::text(n, source), classify(n, parent), tk::span(n));
        },
    );
}

/// `{name}` and `{name:spec}` inside a string are inline format arguments — Rust's
/// own capture syntax — and the dominant way modern code uses a binding inside
/// `format!`/`println!`. `{{` escapes stay literal. Over-matching in a non-format
/// string only keeps something alive.
fn format_arg_references(n: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    let text = tk::text(n, source).as_bytes();
    let mut i = 0;
    while i < text.len() {
        if text[i] != b'{' {
            i += 1;
            continue;
        }
        if text.get(i + 1) == Some(&b'{') {
            i += 2;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while end < text.len() && (text[end].is_ascii_alphanumeric() || text[end] == b'_') {
            end += 1;
        }
        let ident_ok = end > start && !text[start].is_ascii_digit();
        let closed = matches!(text.get(end), Some(&b'}') | Some(&b':'));
        if ident_ok && closed {
            let name = std::str::from_utf8(&text[start..end]).unwrap_or("");
            out.reference(name, RefKind::Read, tk::span(n));
        }
        i = end.max(start);
        i += 1;
    }
}

/// Every identifier-shaped word inside an attribute's string arguments, path
/// segments included — `with = "crate::x::f"` names `x` and `f`.
fn attribute_string_references(attr: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    tk::walk(attr, &mut |n| {
        if n.kind() != "string_content" {
            return;
        }
        let span = tk::span(n);
        let text = tk::text(n, source);
        for word in text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
            if !word.is_empty() && !word.starts_with(|c: char| c.is_ascii_digit()) {
                out.reference(word, RefKind::Read, span);
            }
        }
    });
}

/// A qualified path in expression or type position (`crate::x::f(...)`,
/// `x::Type::CONST`) is an inline import: Rust's module system needs no `use` to
/// reach an item. One deduplicated edge per distinct path, binding every segment
/// after the leading keywords — the module chain and the item alike, all declared in
/// the file the path lands in.
///
/// Unlike `use` and `mod` items, these paths are NOT rebased against the
/// inline-`mod` stack (this walk is flat): `super::f()` inside `mod tests { .. }`
/// resolves as if written at the top of the file, usually to a miss. That is
/// deliberate — the path's own identifiers land as same-file references, which is
/// the keep that matters there, and a missed edge degrades to `Unresolved`,
/// keep-alive, never an accusation.
fn path_import(node: Node<'_>, source: &[u8], seen: &mut BTreeSet<String>, out: &mut EvidenceSink) {
    // Only the outermost scoped node carries the whole path.
    if node
        .parent()
        .is_some_and(|p| matches!(p.kind(), "scoped_identifier" | "scoped_type_identifier"))
    {
        return;
    }
    let mut segments = Vec::new();
    flatten_path(node, source, &mut segments);
    if segments.len() < 2 {
        return;
    }
    let bound: Vec<&String> = segments
        .iter()
        .skip_while(|s| matches!(s.as_str(), "crate" | "self" | "super"))
        .collect();
    if bound.is_empty() {
        return;
    }
    let joined = segments.join("::");
    if !seen.insert(joined.clone()) {
        return;
    }
    let bindings = bound
        .iter()
        .map(|s| ImportBinding {
            imported: SmolStr::new(s.as_str()),
            local: SmolStr::new(s.as_str()),
        })
        .collect();
    out.import(
        target_for(&segments),
        ImportShape::Bindings(bindings),
        tk::span(node),
        Confidence::Certain,
    );
}

/// Binding and naming positions are not uses. The bias is deliberate: excluding too
/// little keeps something alive; excluding too much accuses it — so only positions
/// that are unambiguously bindings or declarations are excluded.
fn is_use(n: Node<'_>, parent: Node<'_>) -> bool {
    match parent.kind() {
        // A declaration's own name declares.
        "function_item"
        | "function_signature_item"
        | "struct_item"
        | "enum_item"
        | "union_item"
        | "trait_item"
        | "type_item"
        | "const_item"
        | "static_item"
        | "mod_item"
        | "macro_definition"
        | "enum_variant"
        | "field_declaration"
        | "extern_crate_declaration"
        | "const_parameter" => parent.child_by_field_name("name") != Some(n),
        // Lifetimes name lifetimes, never symbols.
        "lifetime" => false,
        // Parameters and simple binding patterns bind.
        "parameter" | "let_declaration" | "for_expression" | "let_condition" => {
            parent.child_by_field_name("pattern") != Some(n)
        }
        "closure_parameters" | "tuple_pattern" => false,
        // Struct-literal keys name a field being set; shorthand also reads.
        "field_initializer" => parent.child_by_field_name("field") != Some(n),
        _ => true,
    }
}

fn classify(n: Node<'_>, parent: Node<'_>) -> RefKind {
    if parent.kind() == "call_expression" && parent.child_by_field_name("function") == Some(n) {
        return RefKind::Call;
    }
    if matches!(parent.kind(), "scoped_identifier")
        && parent.child_by_field_name("name") == Some(n)
        && parent.parent().is_some_and(|gp| {
            gp.kind() == "call_expression" && gp.child_by_field_name("function") == Some(parent)
        })
    {
        return RefKind::Call;
    }
    if parent.kind() == "field_expression" && parent.child_by_field_name("field") == Some(n) {
        let called = parent.parent().is_some_and(|gp| {
            gp.kind() == "call_expression" && gp.child_by_field_name("function") == Some(parent)
        });
        return if called { RefKind::Call } else { RefKind::Read };
    }
    if parent.kind() == "macro_invocation" && parent.child_by_field_name("macro") == Some(n) {
        return RefKind::Call;
    }
    if n.kind() == "type_identifier" {
        return RefKind::TypeUse;
    }
    RefKind::Read
}

fn comment(n: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    let mut span = tk::span(n);
    // This grammar's line_comment swallows the trailing newline; the comment ends
    // before it.
    while span.end > span.start
        && matches!(source.get(span.end as usize - 1), Some(b'\n') | Some(b'\r'))
    {
        span = kndo_contract::vocab::Span::new(span.start, span.end - 1);
    }
    let bytes = &source[span.start as usize..(span.end as usize).min(source.len())];
    // `//`, `///`, `//!` and `/*`, `/**`, `/*!` all strip to their text, so pragmas
    // parse the same in plain and doc comments.
    let text = if bytes.starts_with(b"//") {
        let extra = bytes[2..]
            .iter()
            .take_while(|&&b| b == b'/' || b == b'!')
            .count() as u32;
        kndo_contract::vocab::Span::new(span.start + 2 + extra, span.end)
    } else if bytes.starts_with(b"/*") && bytes.ends_with(b"*/") && bytes.len() >= 4 {
        let extra = bytes[2..]
            .iter()
            .take_while(|&&b| b == b'*' || b == b'!')
            .count()
            .min(bytes.len().saturating_sub(4)) as u32;
        kndo_contract::vocab::Span::new(span.start + 2 + extra, span.end - 2)
    } else {
        span
    };
    out.comment(span, text);
}
