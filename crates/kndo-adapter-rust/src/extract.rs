//! Extraction: one recursive item pass (descending into `mod` bodies, queueing
//! `impl` blocks until their types are known), then one pruned full-tree walk for
//! references, comments, and qualified-path edges. Everything ambiguous degrades
//! toward keep-alive: a name we cannot classify is a use, a binding we cannot name is
//! never declared (and so never accused). Deliberately undeclared: `macro_rules!`
//! macros (textual scope), struct fields and enum variants (derives dispatch on them
//! invisibly) — never accusing what the grammar alone cannot prove dead.

use kndo_contract::evidence::{
    DeclarationId, EvidenceSink, ImportBinding, ImportShape, ImportTarget, MarkerTarget, Reach,
    RefKind, RootKind, RootTarget, SymbolKind,
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
    // The `@generated`/`DO NOT EDIT` convention (prost, bindgen): generated code
    // declares nothing accusable and the FILE is the generator's output, rooted
    // Tooling so it is never accused of being unimported; its imports and
    // references still keep the rest of the project alive. Same needles as the
    // JVM adapters, whose library-mode roots already cover the file half.
    let generated = tk::generated_marked(source, tk::GENERATED_NEEDLES, &["//", "/*", "*"]);
    if generated {
        out.root(
            RootTarget::WholeFile,
            RootKind::Tooling,
            Confidence::Probable,
        );
    }
    let mut cx = ItemPass {
        source,
        generated,
        main_root_kind: main_root_kind(path),
        unimportable_root: unimportable_crate_root(path),
        types: BTreeMap::new(),
        free_declarations: BTreeMap::new(),
        impls: Vec::new(),
        redirects: BTreeMap::new(),
        uses: Vec::new(),
        use_locals: BTreeSet::new(),
        stack: Vec::new(),
        out,
    };
    cx.items(root, MarkerTarget::File);
    cx.nested_uses(root);
    let impls = std::mem::take(&mut cx.impls);
    for node in impls {
        cx.impl_members(node);
    }
    // `use` trees resolve against the whole file's `mod` statements (a
    // `#[path]`-redirect can be declared after the `use` that rides its alias), so
    // they process once every item has been seen.
    let uses = std::mem::take(&mut cx.uses);
    let mut expanded: Vec<(Node<'_>, Vec<UseLeaf>)> = uses
        .iter()
        .map(|(node, stack)| (*node, cx.use_leaves(*node, stack)))
        .collect();
    // A `use` headed by the local another `use` in this file binds
    // (`use a::b as c; use c::d;`) names that path, not a crate called `c`.
    // Locals collect over every leaf first because `use` order carries no
    // meaning; then each head rewrites through them.
    let bound: BTreeMap<String, Vec<String>> = expanded
        .iter()
        .flat_map(|(_, leaves)| leaves.iter())
        .filter_map(|leaf| Some((leaf.local()?.to_string(), leaf.segments.clone())))
        .collect();
    for (_, leaves) in &mut expanded {
        for leaf in leaves {
            leaf.rewrite_head(&bound);
        }
    }
    for (node, leaves) in expanded {
        let public = has_visibility(node);
        for leaf in leaves {
            cx.emit_use(node, public, leaf);
        }
    }
    let free_declarations = std::mem::take(&mut cx.free_declarations);
    let use_locals = std::mem::take(&mut cx.use_locals);
    macro_template_roots(root, source, &free_declarations, out);
    references_and_comments(root, source, &use_locals, out);
}

/// Names a `macro_rules!` template references are resolved at every EXPANSION
/// site, not here: the macro travels (textual scope, `#[macro_use]`,
/// `#[macro_export]`), and narrowing a name its body mentions breaks call
/// sites no reference in this file records. Free declarations of this file
/// named inside a macro body therefore root `Possible` — the same tier as the
/// rest of the dispatch-the-source-never-names family.
fn macro_template_roots(
    root: Node<'_>,
    source: &[u8],
    free_declarations: &BTreeMap<String, DeclarationId>,
    out: &mut EvidenceSink,
) {
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    tk::walk(root, &mut |n| {
        if n.kind() != "macro_definition" {
            return;
        }
        let name_node = n.child_by_field_name("name");
        tk::walk(n, &mut |t| {
            if t.kind() == "identifier"
                && Some(t) != name_node
                && let Some(&id) = free_declarations.get(tk::text(t, source))
                && seen.insert(id.index())
            {
                out.root(
                    RootTarget::Declaration(id),
                    RootKind::Production,
                    Confidence::Possible,
                );
            }
        });
    });
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
    generated: bool,
    main_root_kind: RootKind,
    unimportable_root: bool,
    /// Type name → its declaration, for wiring `impl` members to their owner.
    types: BTreeMap<String, DeclarationId>,
    /// Every free declaration by name, for the macro-template pass: names a
    /// `macro_rules!` body references must stay resolvable at every expansion
    /// site, so they root rather than count as file-local uses.
    free_declarations: BTreeMap<String, DeclarationId>,
    impls: Vec<Node<'a>>,
    /// `#[path = "…"]`-redirected module names → their real path segments. `use`
    /// paths through the alias substitute these (`use self::imp::*` where `mod imp`
    /// points at `disabled.rs`) — one variant per redirect, since cfg gates can
    /// declare the same alias twice.
    redirects: BTreeMap<String, Vec<Vec<String>>>,
    uses: Vec<(Node<'a>, Vec<String>)>,
    /// Every local name a `use` in this file binds — the roots a qualified
    /// path may continue from instead of naming a crate.
    use_locals: BTreeSet<String>,
    /// Inline-`mod` names enclosing the current item. `self::`/`super::` in a `use`
    /// or a `mod foo;` mean something different inside `mod tests { ... }` than at
    /// the top of the file; rebasing against this stack keeps the emitted specifier
    /// file-relative.
    stack: Vec<String>,
    out: &'o mut EvidenceSink,
}

impl<'a> ItemPass<'a, '_> {
    /// The items of a module body — the file's, or an inline `mod`'s, which is
    /// what `on` names: an inner attribute (`#![…]`) speaks for its container.
    fn items(&mut self, container: Node<'a>, on: MarkerTarget) {
        let mut cursor = container.walk();
        let children: Vec<Node<'a>> = container.named_children(&mut cursor).collect();
        for item in children {
            if item.kind() == "inner_attribute_item" {
                self.marker(item, on.clone());
                continue;
            }
            self.item(item);
        }
    }

    /// One `#[…]`/`#![…]` as marker evidence — see [`attribute_parts`].
    fn marker(&mut self, attr_item: Node<'a>, on: MarkerTarget) {
        let Some(attr) = attr_item.named_child(0) else {
            return;
        };
        let Some((path, args)) = attribute_parts(attr, self.source) else {
            return;
        };
        self.out.marker(
            on,
            path,
            args.into_iter().map(SmolStr::from).collect(),
            tk::span(attr_item),
        );
    }

    fn markers(&mut self, attrs: &[Node<'a>], id: DeclarationId) {
        for attr in attrs {
            self.marker(*attr, MarkerTarget::Declaration(id));
        }
    }

    fn item(&mut self, item: Node<'a>) {
        if self.generated
            && !matches!(
                item.kind(),
                "use_declaration" | "mod_item" | "attribute_item"
            )
        {
            return;
        }
        let attrs = attribute_items(item);
        let reach = reach_of(item, self.source);
        match item.kind() {
            "function_item" => {
                if let Some(n) = item.child_by_field_name("name") {
                    let name = tk::text(n, self.source);
                    let id =
                        self.out
                            .declaration(name, SymbolKind::Function, tk::span(item), reach);
                    self.free_declarations.entry(name.to_string()).or_insert(id);
                    self.out.metrics(id, function_metrics(item, self.source));
                    self.markers(&attrs, id);
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
                    let id =
                        self.out
                            .declaration(name, SymbolKind::Type, tk::span(item), reach.clone());
                    self.types.entry(name.to_string()).or_insert(id);
                    self.free_declarations.entry(name.to_string()).or_insert(id);
                    self.markers(&attrs, id);
                    if item.kind() == "trait_item" {
                        self.trait_members(item, id);
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
                    let name = tk::text(n, self.source);
                    let id = self.out.declaration(name, kind, tk::span(item), reach);
                    self.free_declarations.entry(name.to_string()).or_insert(id);
                    self.markers(&attrs, id);
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
                            self.free_declarations.entry(name.to_string()).or_insert(id);
                            self.markers(&attrs, id);
                            self.stack.push(name.to_string());
                            self.items(body, MarkerTarget::Declaration(id));
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
                            match attrs.iter().find_map(|a| path_attribute(*a, self.source)) {
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
    fn trait_members(&mut self, trait_item: Node<'a>, owner: DeclarationId) {
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
            // A trait item has no visibility of its own: it reaches as far as
            // its trait does, which the engine resolves through the owner.
            let id = self.out.declaration(
                tk::text(n, self.source),
                SymbolKind::Method,
                tk::span(m),
                Reach::Inherited,
            );
            self.out.member_of(id, owner);
            if m.child_by_field_name("body").is_some() {
                self.out.metrics(id, function_metrics(m, self.source));
            }
            self.markers(&attribute_items(m), id);
        }
    }

    /// Inherent `impl` blocks declare methods and associated items, owned by their
    /// type when it is declared in this file (`impl` for a type declared elsewhere
    /// leaves the owner empty — still a member, judged in the global pool). Trait
    /// impls (`impl T for X`) declare nothing: their bodies are the trait's shape,
    /// and accusing a required method would accuse the trait bound.
    fn impl_members(&mut self, impl_item: Node<'a>) {
        if self.generated {
            return;
        }
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
        // Attributes on the `impl` block gate or exempt every member lexically
        // (`#[cfg(test)] impl X { … }`); the block declares nothing itself, so
        // each member carries them beside its own.
        let impl_attrs = attribute_items(impl_item);
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
            let reach = reach_of(m, self.source);
            let id = self
                .out
                .declaration(tk::text(n, self.source), kind, tk::span(m), reach);
            if let Some(owner) = owner {
                self.out.member_of(id, owner);
            }
            if has_metrics {
                self.out.metrics(id, function_metrics(m, self.source));
            }
            let mut attrs = impl_attrs.clone();
            attrs.extend(attribute_items(m));
            self.markers(&attrs, id);
        }
    }

    /// `use` items inside function and block bodies (`fn f() { use bstr::ByteSlice;
    /// … }`) are imports like any other — the item walk only sees module-level
    /// items, so this pass collects the rest, each with the `mod` stack of its
    /// enclosing modules.
    fn nested_uses(&mut self, root: Node<'a>) {
        let mut pending: Vec<Node<'a>> = vec![root];
        while let Some(node) = pending.pop() {
            let mut cursor = node.walk();
            let children: Vec<Node<'a>> = node.named_children(&mut cursor).collect();
            for child in children.into_iter().rev() {
                if child.kind() == "use_declaration" {
                    if child.parent().is_some_and(|p| p.kind() == "block") {
                        let mut stack = Vec::new();
                        let mut up = child.parent();
                        while let Some(a) = up {
                            if a.kind() == "mod_item"
                                && let Some(n) = a.child_by_field_name("name")
                            {
                                stack.push(tk::text(n, self.source).to_string());
                            }
                            up = a.parent();
                        }
                        stack.reverse();
                        self.uses.push((child, stack));
                    }
                    continue;
                }
                pending.push(child);
            }
        }
    }

    /// Every leaf of one `use` tree, rebased against the inline-`mod` stack and
    /// walked through `#[path]` redirects.
    fn use_leaves(&self, item: Node<'a>, stack: &[String]) -> Vec<UseLeaf> {
        let Some(argument) = item.child_by_field_name("argument") else {
            return Vec::new();
        };
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
        expanded
    }

    fn emit_use(&mut self, item: Node<'a>, public: bool, leaf: UseLeaf) {
        {
            if leaf.segments.is_empty() {
                return;
            }
            let target = target_for(&leaf.segments);
            let last = leaf.segments.last().unwrap().clone();
            let local = leaf.alias.unwrap_or_else(|| last.clone());
            self.use_locals.insert(local.clone());
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

impl UseLeaf {
    /// The name this leaf binds in its file: the alias, else the last segment;
    /// a glob binds no name of its own.
    fn local(&self) -> Option<&str> {
        if self.glob {
            return None;
        }
        self.alias
            .as_deref()
            .or_else(|| self.segments.last().map(String::as_str))
    }

    /// Replaces a head that is a `use`-bound local of the same file with the
    /// path it binds, repeatedly for a chain of aliases. Bounded: a cycle
    /// (`use a::b as c; use c::d as a;`) is the compiler's to reject, and this
    /// walk must not follow it forever.
    fn rewrite_head(&mut self, bound: &BTreeMap<String, Vec<String>>) {
        for _ in 0..8 {
            let Some(head) = self.segments.first() else {
                return;
            };
            if matches!(head.as_str(), "crate" | "self" | "super") {
                return;
            }
            let Some(path) = bound.get(head) else {
                return;
            };
            if path.first() == Some(head) {
                return;
            }
            let mut segments = path.clone();
            segments.extend(self.segments.drain(1..));
            self.segments = segments;
        }
    }
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
    visibility_node(item).is_some()
}

fn visibility_node(item: Node<'_>) -> Option<Node<'_>> {
    let mut c = item.walk();
    item.named_children(&mut c)
        .find(|ch| ch.kind() == "visibility_modifier")
}

/// No modifier is module-private, and a file is its module until the module
/// tree is declared — File. `pub(crate)` is the compiler's crate boundary,
/// the unit's reach, which [`crate::resolve`] bounds from the package map
/// until the manifest names the crate. `pub(super)` and `pub(in super…)` name
/// an ancestor by distance; `pub(in crate::a)` names one by path, which the
/// engine resolves against the forest; `pub(self)` is the module's own.
/// `pub` → Exported.
fn reach_of(item: Node<'_>, source: &[u8]) -> Reach {
    let Some(v) = visibility_node(item) else {
        return Reach::File;
    };
    let text = tk::text(v, source).trim();
    let Some(scope) = text
        .strip_prefix("pub(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return Reach::Exported;
    };
    let scope = scope.trim();
    let path = scope.strip_prefix("in ").map_or(scope, str::trim);
    match path {
        "crate" => Reach::Unit { up: 0 },
        "self" => Reach::File,
        _ => {
            let segments: Vec<&str> = path.split("::").map(str::trim).collect();
            if segments.iter().all(|s| *s == "super") {
                Reach::Namespace {
                    up: segments.len() as u32,
                }
            } else {
                Reach::Named {
                    namespace: segments.into_iter().map(SmolStr::new).collect(),
                }
            }
        }
    }
}

/// `path = "foo/bar.rs"` from a `#[path]` attribute item, as module-path
/// segments (`["foo", "bar"]`) relative to the declaring module's directory.
fn path_attribute(attr_item: Node<'_>, source: &[u8]) -> Option<Vec<String>> {
    let (path, args) = attribute_parts(attr_item.named_child(0)?, source)?;
    if path != "path" || args.len() != 1 {
        return None;
    }
    let quoted = args[0].strip_prefix('"')?.strip_suffix('"')?;
    let stem = quoted.strip_suffix(".rs").unwrap_or(quoted);
    Some(stem.split('/').map(str::to_string).collect())
}

/// The outer `#[…]` attribute items of an item, in source order. Doc comments
/// may sit between an item and its attributes.
fn attribute_items(item: Node<'_>) -> Vec<Node<'_>> {
    let mut out = Vec::new();
    let mut prev = item.prev_named_sibling();
    while let Some(p) = prev {
        match p.kind() {
            "attribute_item" => out.push(p),
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        prev = p.prev_named_sibling();
    }
    out.reverse();
    out
}

/// The marker an `attribute` node spells: its path as written (whitespace
/// dropped) and its top-level arguments — the parenthesized list split at
/// depth-zero commas, or the one `= value` — each trimmed, inner whitespace
/// runs collapsed to one space. Two of the language's own normalizations, so
/// a rule reads one spelling: `#[unsafe(no_mangle)]` (the 2024 edition's
/// unsafe attributes) unwraps to the attribute inside, and a `cfg` predicate
/// flattens to its atoms — `all`/`any` transparently, `not` as a `!` prefix —
/// so `cfg(test)` and `cfg(all(test, unix))` both carry `test` while
/// `cfg(not(test))` carries `!test`. Meaning stays with the spec's dispatch
/// rules.
fn attribute_parts(attr: Node<'_>, source: &[u8]) -> Option<(String, Vec<String>)> {
    let path_node = attr.named_child(0)?;
    let path: String = tk::text(path_node, source)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let args = match (
        attr.child_by_field_name("arguments"),
        attr.child_by_field_name("value"),
    ) {
        (Some(tt), _) => {
            let text = tk::text(tt, source);
            let inner = text
                .strip_prefix('(')
                .and_then(|t| t.strip_suffix(')'))
                .unwrap_or(text);
            tk::split_arguments(inner)
        }
        (None, Some(value)) => vec![tk::normalize_whitespace(tk::text(value, source))],
        (None, None) => Vec::new(),
    };
    Some(normalize_attribute(path, args))
}

fn normalize_attribute(path: String, args: Vec<String>) -> (String, Vec<String>) {
    if path == "unsafe" && args.len() == 1 {
        let (inner_path, inner_args) = split_attribute_text(&args[0]);
        return normalize_attribute(inner_path, inner_args);
    }
    if path == "cfg" {
        let mut atoms = Vec::new();
        for arg in &args {
            flatten_cfg(arg, false, &mut atoms);
        }
        return (path, atoms);
    }
    (path, args)
}

/// `no_mangle`, `tokio::test(flavor = "x")`, `path = "x"` as text — the shape
/// inside `unsafe(…)` — split into a path and its arguments like a node is.
fn split_attribute_text(text: &str) -> (String, Vec<String>) {
    let text = text.trim();
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':' || c == '$'))
        .unwrap_or(text.len());
    let path = text[..end].to_string();
    let rest = text[end..].trim();
    let args = if let Some(inner) = rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        tk::split_arguments(inner)
    } else if let Some(value) = rest.strip_prefix('=') {
        vec![tk::normalize_whitespace(value)]
    } else {
        Vec::new()
    };
    (path, args)
}

/// A `cfg` predicate to its atoms: `all`/`any` are transparent, `not` flips
/// the `!` prefix — presence is what a rule reads, never the truth table.
fn flatten_cfg(predicate: &str, negated: bool, out: &mut Vec<String>) {
    let p = predicate.trim();
    let call = |name: &str| {
        p.strip_prefix(name)
            .and_then(|r| r.trim_start().strip_prefix('('))
            .and_then(|r| r.strip_suffix(')'))
    };
    if let Some(inner) = call("all").or_else(|| call("any")) {
        for x in tk::split_arguments(inner) {
            flatten_cfg(&x, negated, out);
        }
    } else if let Some(inner) = call("not") {
        for x in tk::split_arguments(inner) {
            flatten_cfg(&x, !negated, out);
        }
    } else if !p.is_empty() {
        out.push(if negated {
            format!("!{p}")
        } else {
            p.to_string()
        });
    }
}

/// Metrics over one function-shaped node. Leaves are normalized by class —
/// identifiers, strings and numbers collapse to their kind — so Type-2 clones
/// (renamed, re-valued) fingerprint identically; everything else keeps its literal
/// kind. Comments never count.
const METRICS: tk::MetricsSpec = tk::MetricsSpec {
    is_branch: |n, _| match n.kind() {
        "if_expression" | "while_expression" | "for_expression" | "loop_expression"
        | "try_expression" => true,
        // The `_` arm is the catch-the-rest, not a new predicate — the shared
        // rule in the spec's contract.
        "match_arm" => n
            .child_by_field_name("pattern")
            .is_none_or(|pat| pat.kind() != "_"),
        "binary_expression" => {
            let mut c = n.walk();
            n.children(&mut c)
                .any(|ch| matches!(ch.kind(), "&&" | "||"))
        }
        _ => false,
    },
    token_class: |n| match n.kind() {
        "identifier" | "field_identifier" | "type_identifier" | "shorthand_field_identifier" => {
            Some("id")
        }
        "string_content" => Some("str"),
        "integer_literal" | "float_literal" => Some("num"),
        "line_comment" | "block_comment" => None,
        other => Some(other),
    },
};

fn function_metrics(node: Node<'_>, source: &[u8]) -> kndo_contract::evidence::FunctionMetrics {
    tk::function_metrics(node, &METRICS, source)
}

fn references_and_comments(
    root: Node<'_>,
    source: &[u8],
    use_locals: &BTreeSet<String>,
    out: &mut EvidenceSink,
) {
    let mut seen_paths: BTreeSet<String> = BTreeSet::new();
    tk::walk_pruned(
        root,
        &["use_declaration", "extern_crate_declaration"],
        &mut |n| {
            match n.kind() {
                "line_comment" | "block_comment" => {
                    tk::comment_evidence(n, source, &COMMENT_MARKERS, out);
                    return;
                }
                "scoped_identifier" | "scoped_type_identifier" => {
                    path_import(n, source, use_locals, &mut seen_paths, out);
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
                "attribute_item" | "inner_attribute_item" => {
                    attribute_string_references(n, source, out);
                    attribute_path_imports(n, source, use_locals, &mut seen_paths, out);
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
fn path_import(
    node: Node<'_>,
    source: &[u8],
    use_locals: &BTreeSet<String>,
    seen: &mut BTreeSet<String>,
    out: &mut EvidenceSink,
) {
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
    // A path headed by a type (`Vec::new`, `Self::x`, `u64::MAX`) or by a name a
    // `use` in this file already binds (`io::Result` after `use std::io`)
    // continues something in scope: it names no module or crate, and the `use`
    // that bound it carries the import. Its identifiers still land as
    // references. A tool attribute (`#[rustfmt::skip]`) names no crate either.
    let head = segments[0].as_str();
    if head.starts_with(|c: char| c.is_ascii_uppercase())
        || PRIMITIVE_TYPES.contains(&head)
        || TOOL_ATTRIBUTES.contains(&head)
        || use_locals.contains(head)
    {
        return;
    }
    emit_path_import(&segments, tk::span(node), seen, out);
}

/// The language's own scalar and string types, which paths may head (`u64::MAX`).
const PRIMITIVE_TYPES: &[&str] = &[
    "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64",
    "i128", "isize", "f32", "f64",
];

/// Registered tool namespaces an attribute path may head: not crates.
const TOOL_ATTRIBUTES: &[&str] = &["rustfmt", "clippy", "rustdoc", "miri", "diagnostic"];

/// One deduplicated import per distinct path, binding every segment after the
/// leading keywords.
fn emit_path_import(
    segments: &[String],
    span: kndo_contract::vocab::Span,
    seen: &mut BTreeSet<String>,
    out: &mut EvidenceSink,
) {
    let bound: Vec<&String> = segments
        .iter()
        .skip_while(|s| matches!(s.as_str(), "crate" | "self" | "super"))
        .collect();
    if bound.is_empty() {
        return;
    }
    let joined = segments.join("::");
    if !seen.insert(joined) {
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
        target_for(segments),
        ImportShape::Bindings(bindings),
        span,
        Confidence::Certain,
    );
}

/// Crate paths spelled inside an attribute — `#[derive(thiserror::Error)]`,
/// `#[tokio::main]`, `#[cfg_attr(…, derive(schemars::JsonSchema))]` — are the
/// only place a derive-macro crate is ever named: an import like any qualified
/// path, or `unused` would accuse the crate in every manifest declaring it. The
/// attribute's own path is a parsed node; inside its token tree, `a::b` is a run
/// of identifier tokens joined by `::`.
fn attribute_path_imports(
    attr: Node<'_>,
    source: &[u8],
    use_locals: &BTreeSet<String>,
    seen: &mut BTreeSet<String>,
    out: &mut EvidenceSink,
) {
    tk::walk(attr, &mut |n| match n.kind() {
        "scoped_identifier" => path_import(n, source, use_locals, seen, out),
        "token_tree" => {
            let mut run: Vec<String> = Vec::new();
            let mut start = 0u32;
            let mut end = 0u32;
            let mut after_separator = false;
            let mut flush = |run: &mut Vec<String>, start: u32, end: u32| {
                let head = run.first().map(String::as_str).unwrap_or("");
                if run.len() >= 2
                    && !head.starts_with(|c: char| c.is_ascii_uppercase())
                    && !TOOL_ATTRIBUTES.contains(&head)
                    && !use_locals.contains(head)
                {
                    emit_path_import(run, kndo_contract::vocab::Span::new(start, end), seen, out);
                }
                run.clear();
            };
            let mut cursor = n.walk();
            for token in n.children(&mut cursor) {
                match token.kind() {
                    "identifier" if run.is_empty() || after_separator => {
                        if run.is_empty() {
                            start = token.start_byte() as u32;
                        }
                        end = token.end_byte() as u32;
                        run.push(tk::text(token, source).to_string());
                        after_separator = false;
                    }
                    "::" if !run.is_empty() && !after_separator => after_separator = true,
                    _ => {
                        flush(&mut run, start, end);
                        after_separator = false;
                    }
                }
            }
            flush(&mut run, start, end);
        }
        _ => {}
    });
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

/// Rust's comment markers: `///`, `//!`, `////` and `/**`, `/*!` all strip to
/// their text — pragmas parse the same in plain and doc comments.
const COMMENT_MARKERS: tk::CommentMarkers<'static> = tk::CommentMarkers {
    line: &["//"],
    block: &[("/*", "*/")],
    line_doc: b"/!",
    block_doc: b"*!",
};
