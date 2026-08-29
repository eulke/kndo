//! Rust language adapter. The one Rust-shaped idea the whole adapter
//! is built on: **the module tree IS the file graph** — `mod foo;` is an import (the parent's
//! certain `ImportsFile` edge to the child), and a file no `mod` chain reaches is dead to the
//! compiler, a verdict reachability then reproduces for free.

mod extraction;
mod manifest;
mod parsing;
mod resolution;

use kndo_core::adapter::{
    AdapterDescriptor, CyclePolicy, CycleTolerance, FileClaim, ImportSpec, LanguageAdapter,
    ManifestFacts, ProjectPath, Resolution, ResolveCtx, SourceFile, VisibilityRung,
    VisibilityScope,
};
use smol_str::SmolStr;

pub struct RustAdapter;

/// `build.rs`, `.cargo/`, and the de-facto `xtask/` task-runner convention are tooling.
/// Test-role dirs are deliberately absent here: Cargo's `tests`/`benches`/`examples` are
/// *package-relative* conventions (they bind to the `Cargo.toml` beside them, not to the
/// segment wherever it appears — a workspace-excluded crate living under some ancestor's
/// `examples/` is ordinary production source), so they're declared as
/// `package_test_dirs` on the descriptor and matched by core assembly, which knows which
/// manifest owns which file.
const PATH_PATTERNS: kndo_adapter_toolkit::classify::PathPatterns =
    kndo_adapter_toolkit::classify::PathPatterns {
        test_name_markers: &[],
        test_dirs: &[],
        tooling_name_markers: &["build.rs"],
        tooling_dirs: &[".cargo", "xtask"],
    };

impl LanguageAdapter for RustAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            activation: Vec::new(),
            dependencies: Vec::new(),
            id: SmolStr::new("rust"),
            // Bump whenever the serialized facts shape or the emission semantics change.
            facts_schema_version: 26,
            file_globs: vec![SmolStr::new("**/*.rs")],
            manifest_globs: vec![SmolStr::new("**/Cargo.toml")],
            grammar_version: SmolStr::new("tree-sitter-rust 0.24"),
            // [File "private", Module "pub(super)", Package "pub(crate)", Public "pub"].
            // `pub(super)` names the parent module's SUBTREE — a region strictly between one
            // file and one package, distinct from both `pub(crate)` above it and `private`
            // below it. Collapsing it into `pub(crate)` would widen every `pub(super)` item to
            // crate-wide visibility, leaving `private-type-leak` blind to leaks that stop at the
            // parent module's boundary. `pub(in path)` still widens: this adapter does not
            // resolve its path to a unit key yet, and widening
            // only ever silences an accusation, never fabricates one.
            visibility_ladder: vec![
                VisibilityRung {
                    // Rust's "private" is NOT the file: it is the declaring module AND its
                    // descendants, which is exactly what a Module rung anchored on the file's
                    // own unit says. A `File` rung here is too narrow: it would accuse
                    // ripgrep's `flags::parse::lookup` of leaking `flags::mod`'s private `Flag`,
                    // which every module under `flags` can spell perfectly well.
                    scope: VisibilityScope::Module,
                    label: SmolStr::new("private"),
                    surface_transitive: false,
                },
                VisibilityRung {
                    scope: VisibilityScope::Module,
                    label: SmolStr::new("pub(super)"),
                    surface_transitive: false,
                },
                VisibilityRung {
                    scope: VisibilityScope::Package,
                    label: SmolStr::new("pub(crate)"),
                    surface_transitive: false,
                },
                VisibilityRung {
                    scope: VisibilityScope::Public,
                    label: SmolStr::new("pub"),
                    surface_transitive: true,
                },
            ],
            // Module cycles inside a crate are legal and common
            // (the canonical Idiomatic example). Package cycles are NOT Impossible —
            // dev-dependency cycles are legal cargo, and kndo's package edges include test
            // files — so Impossible would suppress real, visible structure.
            cycle_policy: CyclePolicy {
                file_cycles: CycleTolerance::Idiomatic,
                package_cycles: CycleTolerance::Idiomatic,
            },
            // The crate name IS the `use` specifier's root segment — resolve() structurally
            // identifies the declared dependency every time.
            resolves_dependency_usage: true,
            declares_units_of_testing: true,
            // Cargo's target-dir conventions, anchored at the owning Cargo.toml (see
            // PATH_PATTERNS above). An example consumes the API from outside exactly like a
            // test — a symbol alive only through its own demo is the `test-only` verdict.
            package_test_dirs: vec![
                SmolStr::new("tests"),
                SmolStr::new("benches"),
                SmolStr::new("examples"),
            ],
            builtin_member_types: builtin_member_types(),
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        kndo_adapter_toolkit::classify::claim_by_extension(path, &["rs"], "rust", &PATH_PATTERNS)
    }

    fn claim_manifest(&self, path: &ProjectPath) -> bool {
        matches!(path.0.rsplit('/').next(), Some("Cargo.toml"))
    }

    fn extract(&self, file: &SourceFile<'_>) -> kndo_core::adapter::FileFacts {
        extraction::extract(file.path.0.as_str(), file.content)
    }

    fn extract_manifest(&self, file: &SourceFile<'_>, ctx: &ResolveCtx<'_>) -> ManifestFacts {
        manifest::extract(file.path.0.as_str(), file.content, ctx)
    }

    /// Cargo treats `-` and `_` as interchangeable in a crate name, so a plugin author should
    /// not have to guess which spelling a project used: `serde-json` finds `serde_json`.
    fn declares_dependency(&self, facts: &ManifestFacts, query: &str) -> bool {
        let normalize = |n: &str| n.replace('_', "-");
        let query = normalize(query);
        facts
            .dependencies
            .iter()
            .chain(&facts.workspace_dependencies)
            .any(|d| normalize(&d.name) == query)
    }

    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
        resolution::resolve(spec, ctx)
    }
}

/// What Rust's own generics do to their arguments — the facts no file in a project declares
/// because the declaration lives in the standard library.
///
/// Without these facts, a chain that crosses one of these stops dead:
/// `ls_tree(..).map_err(..)?` then iterated is four hops through `Result` and `Vec` before it
/// reaches the element type, and the type behind it reads as consumed only where it was
/// declared.
///
/// Two conventions, both this adapter's own choice and invisible to the core: `@element` is
/// the member an iteration hops through (a container that does not declare one simply does
/// not type its loop variable — `HashMap` iterates to a tuple, which this model has no way to
/// name, and silence is the right answer), and `@slice` names the anonymous slice/array type
/// so it can carry an `@element` like any other container.
///
/// Deliberately small and evidence-driven, the same discipline as the machinery-trait list: a
/// fact earns its place by closing a measured case, not by completing an API surface.
fn builtin_member_types() -> Vec<kndo_core::adapter::RawMemberType> {
    use kndo_core::adapter::{RawMemberType, TypeExpr};
    // "yields the same argument the receiver had" — the whole reason `Param` exists.
    let passthrough = |owner: &str, member: &str| RawMemberType {
        owner: Some(SmolStr::new(owner)),
        member: SmolStr::new(member),
        yields: TypeExpr::Param(0),
    };
    let mut facts = Vec::new();
    // The success value survives all of these; only the error side changes, and what it
    // changes INTO is a closure's output this adapter cannot name — hence `Unknown`, stated
    // outright rather than implied by a short argument list.
    for member in ["map_err", "inspect", "inspect_err"] {
        facts.push(RawMemberType {
            owner: Some(SmolStr::new("Result")),
            member: SmolStr::new(member),
            yields: TypeExpr::Named {
                name: SmolStr::new("Result"),
                args: vec![TypeExpr::Param(0), TypeExpr::Unknown],
            },
        });
    }
    facts.push(RawMemberType {
        owner: Some(SmolStr::new("Result")),
        member: SmolStr::new("ok"),
        yields: TypeExpr::Named {
            name: SmolStr::new("Option"),
            args: vec![TypeExpr::Param(0)],
        },
    });
    for owner in ["Result", "Option"] {
        for member in ["unwrap", "expect", "unwrap_or_default", "as_ref", "as_mut"] {
            facts.push(passthrough(owner, member));
        }
    }
    // Iteration, declared per container — only the ones that yield a single element. A map is
    // absent on purpose.
    for owner in ["Vec", "VecDeque", "HashSet", "BTreeSet", "Option", "@slice"] {
        facts.push(passthrough(owner, "@element"));
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::vocab::{FileOrigin, FileRole};

    fn path(p: &str) -> ProjectPath {
        ProjectPath(SmolStr::new(p))
    }

    #[test]
    fn the_builtin_table_declares_iteration_only_where_the_element_is_one_type() {
        // A map iterates to a TUPLE, which this model has no way to name, so `HashMap`
        // deliberately declares no `@element` and its loop variable simply does not type.
        // Silence beats a confident wrong type.
        let d = RustAdapter.descriptor();
        let has = |owner: &str, member: &str| {
            d.builtin_member_types
                .iter()
                .any(|m| m.owner.as_deref() == Some(owner) && m.member == member)
        };
        assert!(has("Vec", "@element"));
        assert!(has("@slice", "@element"));
        assert!(!has("HashMap", "@element"), "a map's element is not a type");
        assert!(
            !has("BTreeMap", "@element"),
            "a map's element is not a type"
        );
    }

    #[test]
    fn a_result_operation_keeps_the_success_type_and_admits_it_lost_the_error() {
        // `map_err` is a hop a chain like `ls_tree(..).map_err(..)?` has to cross. The fact
        // says "still a `Result`
        // over the SAME argument 0" — a relationship, not a concrete type — and states
        // outright that it cannot name what the error became.
        let d = RustAdapter.descriptor();
        let fact = d
            .builtin_member_types
            .iter()
            .find(|m| m.owner.as_deref() == Some("Result") && m.member == "map_err")
            .expect("no map_err fact");
        assert_eq!(
            fact.yields,
            kndo_core::adapter::TypeExpr::Named {
                name: SmolStr::new("Result"),
                args: vec![
                    kndo_core::adapter::TypeExpr::Param(0),
                    kndo_core::adapter::TypeExpr::Unknown,
                ],
            }
        );
    }

    #[test]
    fn claims_rs_files_and_rejects_others() {
        let a = RustAdapter;
        assert!(a.claim(&path("src/lib.rs")).is_some());
        assert!(a.claim(&path("src/main.go")).is_none());
        assert!(a.claim(&path("Cargo.toml")).is_none());
    }

    #[test]
    fn roles_follow_the_cargo_directory_conventions() {
        let a = RustAdapter;
        // Test dirs are NOT path-claimed: `tests`/`benches`/`examples` bind to the owning
        // Cargo.toml, so the claim stays Production and the descriptor's
        // `package_test_dirs` lets assembly promote package-relatively.
        assert_eq!(
            a.claim(&path("tests/integration.rs")).unwrap().class.role,
            FileRole::Production
        );
        let d = a.descriptor();
        for dir in ["tests", "benches", "examples"] {
            assert!(
                d.package_test_dirs.iter().any(|s| s == dir),
                "{dir} must be declared package-relative"
            );
        }
        assert_eq!(
            a.claim(&path("build.rs")).unwrap().class.role,
            FileRole::Tooling
        );
        assert_eq!(
            a.claim(&path("xtask/src/main.rs")).unwrap().class.role,
            FileRole::Tooling
        );
        assert_eq!(
            a.claim(&path("src/lib.rs")).unwrap().class.role,
            FileRole::Production
        );
    }

    #[test]
    fn vendor_is_vendored() {
        let a = RustAdapter;
        assert_eq!(
            a.claim(&path("vendor/foo/src/lib.rs"))
                .unwrap()
                .class
                .origin,
            FileOrigin::Vendored
        );
    }

    #[test]
    fn the_trait_surface_delegates_end_to_end() {
        // The conformance suite reaches these methods only through the engine's dynamic
        // dispatch, which static test-reachability cannot see — exercise the trait impl
        // directly: descriptor identity, extraction, manifest extraction, resolution.
        let a = RustAdapter;
        let d = a.descriptor();
        assert_eq!(d.id, "rust");
        assert_eq!(d.visibility_ladder.len(), 4);

        let src_path = path("src/lib.rs");
        let facts = a.extract(&SourceFile {
            path: &src_path,
            content: b"mod child;\npub fn api() {}\n",
        });
        assert!(facts.imports.iter().any(|i| i.specifier == "self::child"));
        assert!(facts.declarations.iter().any(|d| d.name == "api"));

        let known: rustc_hash::FxHashSet<ProjectPath> =
            [path("src/lib.rs"), path("src/child.rs"), path("Cargo.toml")]
                .into_iter()
                .collect();
        let ctx = ResolveCtx::new(&known);
        let manifest_path = path("Cargo.toml");
        let mf = a.extract_manifest(
            &SourceFile {
                path: &manifest_path,
                content: b"[package]\nname = \"demo\"\n",
            },
            &ctx,
        );
        assert_eq!(mf.package_name.as_deref(), Some("demo"));

        let resolved = a.resolve(
            &ImportSpec {
                specifier: SmolStr::new("self::child"),
                from: path("src/lib.rs"),
            },
            &ctx,
        );
        match resolved {
            Resolution::File(p, _) => assert_eq!(p.0, "src/child.rs"),
            other => panic!("expected file resolution, got {other:?}"),
        }
    }

    #[test]
    fn claim_manifest_matches_cargo_toml_only() {
        let a = RustAdapter;
        assert!(a.claim_manifest(&path("Cargo.toml")));
        assert!(a.claim_manifest(&path("crates/x/Cargo.toml")));
        assert!(!a.claim_manifest(&path("Cargo.lock")));
    }
}
