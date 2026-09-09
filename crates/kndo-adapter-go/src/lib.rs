//! Go, through the tree-sitter-go grammar. What Go imports is the package — a
//! directory of files sharing one namespace with no imports between siblings —
//! and this adapter states it ONCE, as the namespace extraction declares from
//! the package clause and the directory. Everything else follows from that
//! node: a lower-case name reaches it and nothing wider, and `go build`
//! compiles the whole of it — the engine reads the co-visible set off the node
//! itself, so reaching one file of a package reaches its siblings without this
//! adapter enumerating a directory. Imports resolve to [`Resolution::Files`], every
//! non-test `.go` in the package dir. Capitalization IS the visibility: an
//! upper-case initial is exported, anything else package-private.
//!
//! The unit is the MODULE: `go.mod` is the one manifest, it names what other
//! modules import, and Go has no per-target section to split it.
//!
//! The adapter id is `go`, the same id v1 used for this territory, so oracle
//! comparisons line up file-for-file.

mod extract;
mod manifest;
mod modfile;
mod resolve;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::EvidenceSink;
use kndo_contract::evidence::RootKind;
use kndo_contract::manifest::ManifestSink;
use kndo_contract::plugin::{
    DispatchRule, Effect, FileRole, Nesting, Plugin, PluginSpec, Rung, Step, Trigger,
};
use kndo_contract::vocab::ProjectPath;

pub struct GoAdapter {
    spec: PluginSpec,
}

/// What Go's own toolchain does with a name, as data. `go test` compiles a
/// package's `_test.go` files into a test binary and runs every top-level
/// `TestXxx`, `BenchmarkXxx`, `ExampleXxx` and `FuzzXxx` in it by name; the
/// runtime calls every `init` when the package loads. Both read the
/// COMPILATION the file lands in, which for a `_test.go` file is the test
/// binary — the file said so with its attachment — and for every other file
/// is the module's own library. `Certain` throughout: this is the toolchain's
/// behavior, not a habit its users keep.
fn dispatch_rules() -> Vec<DispatchRule> {
    use kndo_contract::evidence::SymbolKind::Function;
    use kndo_contract::manifest::UnitKind;
    let rule = |when: Trigger, then: Effect| DispatchRule {
        when,
        then,
        confidence: kndo_contract::vocab::Confidence::Certain,
    };
    let mut rules: Vec<DispatchRule> = ["Test*", "Benchmark*", "Example*", "Fuzz*"]
        .into_iter()
        .map(|pattern| {
            rule(
                Trigger::name(pattern, Function, UnitKind::Test),
                Effect::Root(RootKind::Test),
            )
        })
        .collect();
    // The runtime calls every `init` when the package loads — the load of the
    // binary this file is compiled into, which for a `_test.go` file is the
    // test binary and for every other file is the module's own.
    rules.push(rule(
        Trigger::name("init", Function, UnitKind::Test),
        Effect::Root(RootKind::Test),
    ));
    rules.push(rule(
        Trigger::name("init", Function, UnitKind::Library),
        Effect::Root(RootKind::Production),
    ));
    rules
}

impl GoAdapter {
    pub fn new() -> Self {
        GoAdapter {
            // 13: a `_test.go` file states its own attachment, and the
            // rules read the compilation it lands in.
            spec: kndo_toolkit::source_adapter_builder(
                "kndo:go",
                16,
                &["go"],
                &["**/go.mod", "**/go.work"],
                // The compiler forbids import cycles: one could only be a
                // resolution artifact here.
                kndo_contract::plugin::CycleTolerance::Tolerated,
            )
            // The go tool's own rule: a file whose NAME begins with `_` is in
            // no package at all, and `vendor` holds copies of other modules,
            // compiled as the dependencies they are. A `testdata` or
            // `_`-prefixed DIRECTORY is neither: `./...` skips both when it
            // expands a pattern, but an explicit import compiles them —
            // `go build ./app` on a package importing `example.com/m/_scratch`
            // succeeds under go1.24.7 — so they are discovered and judged.
            .ignores(&["**/vendor/**", "**/_*.go"])
            // A Go package IS a directory: `go build` compiles the files of one
            // directory together, and two directories writing `package util`
            // are two packages however short the name they share.
            .nesting(Nesting::ByDirectory)
            // `go test` compiles exactly the `_test.go` files of a package and
            // runs nothing else — the toolchain's own rule, not a habit, and
            // the adapter's whole statement about what such a file IS.
            .file_roles(&[FileRole::certain("**/*_test.go", RootKind::Test)])
            .dispatch(dispatch_rules())
            // Capitalization is the whole ladder: nothing sits below the
            // package, so a package-private name used only in its file has
            // nowhere narrower to go and `internal-only` stays silent for it.
            .ladder(&[
                Step::new(Rung::Namespace, "unexported"),
                // An exported name in an `internal` package is spelled the
                // same way and reaches the fence's subtree: the word is
                // `exported` on both rungs, and the advice below either is
                // `unexported`.
                Step::new(Rung::Directory, "exported"),
                Step::new(Rung::Exported, "exported"),
            ])
            // go.mod has no sections: every direct requirement is a build
            // requirement, and "only tests import it" has nowhere to move.
            .dependency_scoping(kndo_contract::plugin::DependencyScoping::Unscoped)
            // An import path names the module whose path prefixes it; a path
            // whose first segment carries no `.` is the standard library.
            .dependency_identity(kndo_contract::plugin::DependencyIdentity::ModulePath)
            .dependency_builtins(kndo_contract::plugin::DependencyBuiltins::UndottedFirstSegment)
            .build(),
        }
    }
}

impl Default for GoAdapter {
    fn default() -> Self {
        GoAdapter::new()
    }
}

impl Plugin for GoAdapter {
    fn spec(&self) -> &PluginSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let language = tree_sitter_go::LANGUAGE.into();
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
        out: &mut ManifestSink,
    ) {
        manifest::structure(manifest, cx, out);
    }
}
