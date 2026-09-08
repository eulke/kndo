//! Rust, through the tree-sitter-rust grammar. The adapter reports evidence only —
//! declarations with binary reach (`pub` in any form is nameable beyond its file),
//! module edges (`mod foo;`, `use` trees, qualified paths), keep-alive-biased
//! references, attributes as markers, comment spans, function metrics — and
//! resolves module paths against the file tree; judgment stays in the engine, and
//! what an attribute means is the spec's dispatch rules.
//!
//! Precision posture: a `use` of one item keeps its target file's whole exported
//! surface (`Namespace`), because Rust's alias scopes cannot be re-derived from one
//! file. Qualified paths (`crate::x::f()`) name their item exactly, so those bind.
//! Over-keeping is the deliberate direction — refinement arrives with the
//! visibility-ladder work, measured.
//!
//! The adapter id is `rust`, the same id v1 used for this territory, so oracle
//! comparisons line up file-for-file.

mod extract;
mod manifest;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{EvidenceSink, EvidenceStream, EvidenceStreams, RootKind};
use kndo_contract::extension::{
    DispatchRule, Effect, Extension, ExtensionSpec, Rung, Step, Trigger,
};
use kndo_contract::vocab::{Confidence, ProjectPath};

pub struct RustAdapter {
    spec: ExtensionSpec,
}

/// What Rust's attributes mean, as data — the language's own statements about
/// liveness, matched by the engine against the markers extraction reports.
/// Test-runner attributes (`#[test]`, `#[tokio::test]`, `#[bench]`) and a
/// `cfg(test)` gate root Test; an entry attribute (`#[tokio::main]`) and the
/// linkage and runtime attributes (`#[no_mangle]`, `#[global_allocator]`, …)
/// root Production — something outside the graph calls them. The dead-code
/// lint's own escape hatches (`allow`/`expect` of `dead_code`, its `unused`
/// and `warnings` groups) exempt: the source already answered the question.
/// Certain throughout: an attribute is the code's own statement.
fn dispatch_rules() -> Vec<DispatchRule> {
    let rule = |when: Trigger, then: Effect| DispatchRule {
        when,
        then,
        confidence: Confidence::Certain,
    };
    let mut rules: Vec<DispatchRule> = ["test", "*::test", "bench", "*::bench"]
        .into_iter()
        .map(|path| rule(Trigger::marker(path), Effect::Root(RootKind::Test)))
        .collect();
    rules.push(rule(
        Trigger::marker_with("cfg", "test"),
        Effect::Root(RootKind::Test),
    ));
    rules.extend(
        [
            "*::main",
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
        ]
        .into_iter()
        .map(|path| rule(Trigger::marker(path), Effect::Root(RootKind::Production))),
    );
    for lint in ["allow", "expect"] {
        for group in ["dead_code", "unused", "warnings"] {
            rules.push(rule(Trigger::marker_with(lint, group), Effect::Exempt));
        }
    }
    // A top-level `fn main` is the language's entry convention: whatever target
    // links the file, the runtime calls it — and its COLOR is that target's,
    // which cargo already stated. One rule per kind of target a `main` can sit
    // in, because a rule that named none would color a build script's entry
    // like a binary's. `Probable`: the convention, not this file's own
    // statement.
    use kndo_contract::manifest::UnitKind;
    for (unit, kind) in [
        (UnitKind::Executable, RootKind::Production),
        (UnitKind::Library, RootKind::Production),
        (UnitKind::Tooling, RootKind::Tooling),
        (UnitKind::Example, RootKind::Tooling),
        (UnitKind::Test, RootKind::Test),
        (UnitKind::Bench, RootKind::Test),
    ] {
        rules.push(DispatchRule {
            when: Trigger::name("main", kndo_contract::evidence::SymbolKind::Function, unit),
            then: Effect::Root(kind),
            confidence: Confidence::Probable,
        });
    }
    rules
}

impl RustAdapter {
    pub fn new() -> Self {
        RustAdapter {
            // 15: `fn main`'s color is the kind of the cargo target the
            // manifest says holds it.
            spec: kndo_toolkit::source_adapter_builder(
                "kndo:rust",
                15,
                &["rs"],
                &["**/Cargo.toml"],
                // Modules within a crate reference each other freely — legal,
                // routine structure, never an initialization hazard.
                kndo_contract::extension::CycleTolerance::Tolerated,
            )
            // No `pub` is the module's own — the narrowest rung Rust spells,
            // and the file is a module, so a `pub(crate)` item used only in
            // its file falls to it. `pub(super)`/`pub(in …)` wait for the
            // module tree.
            .ladder(&[
                Step::new(Rung::Namespace, "private"),
                Step::new(Rung::Unit, "pub(crate)"),
                Step::new(Rung::Exported, "pub"),
            ])
            .emits(EvidenceStreams::of(&[
                EvidenceStream::Comments,
                EvidenceStream::Metrics,
                EvidenceStream::Markers,
            ]))
            .dispatch(dispatch_rules())
            // `mod x;` MOUNTS one namespace inside another: the forest is read
            // off those edges, and nothing a path spells names a Rust module.
            .nesting(kndo_contract::extension::Nesting::Mounted)
            // `serde_json::Value` names `serde-json`: the crate root segment,
            // hyphens spelled as underscores.
            .dependency_identity(kndo_contract::extension::DependencyIdentity::CrateRoot)
            .dependency_builtins(kndo_contract::extension::DependencyBuiltins::Named(
                ["std", "core", "alloc", "proc_macro", "test"]
                    .iter()
                    .map(|s| smol_str::SmolStr::new_static(s))
                    .collect(),
            ))
            .build(),
        }
    }
}

impl Default for RustAdapter {
    fn default() -> Self {
        RustAdapter::new()
    }
}

impl Extension for RustAdapter {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let language = tree_sitter_rust::LANGUAGE.into();
        if let Some(tree) = kndo_toolkit::parse_reporting(&language, file.content, out) {
            extract::extract(file.path, file.content, &tree, out);
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        resolve::resolve(from, specifier, cx)
    }

    fn extract_manifest(
        &self,
        manifest: &SourceFile<'_>,
        cx: &ResolveContext<'_>,
        out: &mut kndo_contract::manifest::ManifestSink,
    ) {
        manifest::structure(manifest, cx, out);
    }
}
