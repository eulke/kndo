use super::*;
// The original `graph.rs` test module relied on a single flat top-of-file scope (every
// `crate::adapter`/`crate::vocab` item used anywhere in `graph.rs`, whether or not the
// split-off submodule the test now sits beside still imports it). Glob-importing both
// vocabularies reproduces that scope without hand-tracking which symbol each test needs.
#[allow(unused_imports)]
use crate::adapter::*;
use crate::discovery;
use crate::testkit::MockAdapter;
#[allow(unused_imports)]
use crate::vocab::*;
use std::fs;

#[test]
fn surface_closure_promotes_transitive_members_of_surface_types_only() {
    use crate::adapter::{ProjectPath, Span, VisibilityLevel};
    let file = FileNode {
        path: ProjectPath(SmolStr::new("src/lib.mock")),
        content_hash: [0; 32],
        language: Some(SmolStr::new("mock")),
        class: Some(crate::vocab::FileClass {
            role: FileRole::Production,
            origin: FileOrigin::Authored,
        }),
        package: crate::vocab::PackageId(0),
        unit: None,
        unit_parent: None,
        test_spans: Vec::new(),
        string_call_sites: Vec::new(),
        string_attr_args: Vec::new(),
    };
    let sym = |name: &str, vis: u8, member_of: Option<&str>, start: u32, end: u32| SymbolNode {
        file: FileId(0),
        name: SmolStr::new(name),
        kind: crate::vocab::SymbolKind::Struct,
        span: Span {
            start: (start, 1),
            end: (end, 1),
        },
        exported: vis > 0,
        visibility: VisibilityLevel(vis),
        member_of: member_of.map(SmolStr::new),
        signature_span: None,
        implicitly_invoked: false,
        nested_scope: false,
        visibility_inherited: false,
        visible_in_unit: None,
        implements: None,
        markers: Vec::new(),
    };
    let symbols = vec![
        sym("Widget", 1, None, 1, 20),            // surface seed (rooted below)
        sym("run", 1, Some("Widget"), 3, 5),      // exported member → surface
        sym("hidden", 0, Some("Widget"), 7, 9),   // private member → never surface
        sym("Orphan", 1, None, 30, 40),           // exported but unrooted → not surface
        sym("gadget", 1, Some("Orphan"), 32, 34), // member of non-surface type → no
    ];
    let edges = vec![Edge {
        kind: EdgeKind::Root {
            kind: RootKind::Production,
            target: NodeRef::Symbol(SymbolId(0)),
        },
        confidence: Confidence::Certain,
        source: Provenance::Adapter(SmolStr::new("mock")),
        span: None,
        owner: FileId(0),
    }];
    // for_test's mock ladder: [private (capped), exported (surface-transitive)].
    let mut graph = ProjectGraph::for_test(vec![file], symbols, vec![], edges);

    recompute_surface_closure(&mut graph);
    let surface_roots: Vec<SymbolId> = graph
        .edges
        .iter()
        .filter_map(|e| match (&e.source, &e.kind) {
            (
                Provenance::Surface,
                EdgeKind::Root {
                    target: NodeRef::Symbol(s),
                    ..
                },
            ) => Some(*s),
            _ => None,
        })
        .collect();
    assert_eq!(
        surface_roots,
        vec![SymbolId(1)],
        "only Widget.run joins the surface"
    );

    // Idempotent: a second run (the warm/patch path) reproduces, never duplicates.
    let before = graph.edges.len();
    recompute_surface_closure(&mut graph);
    assert_eq!(graph.edges.len(), before);
}

/// The canonical fixture helper — see [`crate::testkit::fixture::project`] for why every
/// temporary directory in this workspace comes from `tempfile`. The `name` parameter this
/// helper used to take existed only to disambiguate hand-built directory names; the last
/// thing still feeding it was the sibling `cache_dir`, which is a `TempDir` now too.
fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    crate::testkit::fixture::project(files)
}

/// One assertion shape for both cap tests: every Root edge that targets `path`'s file
/// or its symbols carries `expected`, and the file colors `expected_reach`.
fn assert_roots_capped(
    tree: &[(&str, &str)],
    path: &str,
    expected: crate::vocab::RootKind,
    expected_reach: crate::analysis::reachability::Reachability,
) {
    let dir = project(tree);
    let adapters: Vec<Box<dyn LanguageAdapter>> = vec![Box::new(MockAdapter)];
    let (graph, _) = assemble(dir.path(), &adapters, &[]).unwrap();
    let file_id = graph
        .files
        .iter()
        .position(|f| f.path.0 == path)
        .map(|i| FileId(i as u32))
        .expect("target file in graph");
    let symbol_ids: Vec<SymbolId> = graph
        .symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| s.file == file_id)
        .map(|(i, _)| SymbolId(i as u32))
        .collect();
    let mut root_kinds = Vec::new();
    for edge in &graph.edges {
        if let EdgeKind::Root { kind, target } = edge.kind {
            let hits = match target {
                NodeRef::File(f) => f == file_id,
                NodeRef::Symbol(sym) => symbol_ids.contains(&sym),
            };
            if hits {
                root_kinds.push(kind);
            }
        }
    }
    assert!(!root_kinds.is_empty(), "the entry-point roots must exist");
    assert!(
        root_kinds.iter().all(|k| *k == expected),
        "every root on {path} must be capped to {expected:?}, got {root_kinds:?}"
    );
    let reach = crate::analysis::reachability::compute(&graph);
    assert_eq!(
        reach.get(crate::vocab::NodeRef::File(file_id)).0,
        expected_reach,
        "the capped kind must decide the file's color"
    );
}

#[test]
fn production_roots_are_capped_to_tooling_by_file_role() {
    // A manifest bin root (Certain) AND an in-source root both point at a tooling-role
    // file — both cap sites must fire, or the stronger of the two would keep the file
    // production-reachable (the xtask case: "production-reachable but untested" was a
    // false statement).
    assert_roots_capped(
        &[
            ("manifest.mock", "name p\nroot tool.config.mock\n"),
            ("tool.config.mock", "decl main\nroot-decl main\n"),
        ],
        "tool.config.mock",
        crate::vocab::RootKind::Tooling,
        crate::analysis::reachability::Reachability::ToolingOnly,
    );
}

#[test]
fn production_roots_are_capped_to_test_by_file_role() {
    assert_roots_capped(
        &[
            ("manifest.mock", "name p\nroot helper.test.mock\n"),
            ("helper.test.mock", "decl main\nroot-decl main\n"),
        ],
        "helper.test.mock",
        crate::vocab::RootKind::Test,
        crate::analysis::reachability::Reachability::TestOnly,
    );
}

fn mock_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    vec![Box::new(MockAdapter)]
}

/// [`MockAdapter`] declaring `package_test_dirs: ["tests"]` — everything else delegates,
/// so phase 2b's promotion is the only behavioral difference under test.
struct PackageTestDirsAdapter;

impl LanguageAdapter for PackageTestDirsAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            package_test_dirs: vec![SmolStr::new("tests")],
            ..MockAdapter.descriptor()
        }
    }
    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        MockAdapter.claim(path)
    }
    fn claim_manifest(&self, path: &ProjectPath) -> bool {
        MockAdapter.claim_manifest(path)
    }
    fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
        MockAdapter.extract(file)
    }
    fn extract_manifest(&self, file: &SourceFile<'_>, ctx: &ResolveCtx<'_>) -> ManifestFacts {
        MockAdapter.extract_manifest(file, ctx)
    }
    fn resolve(&self, spec: &crate::adapter::ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
        MockAdapter.resolve(spec, ctx)
    }
}

#[test]
fn package_test_dirs_promote_relative_to_the_owning_manifest() {
    let dir = project(&[
        ("manifest.json", ""),
        ("src/a.mock", "decl prod"),
        ("tests/t.mock", "decl t"),
        // A nested package whose sources live under the root package's `tests/`
        // tree: ownership moves to the nested manifest, so nothing in it is
        // test-role by the ROOT's convention — the false positive this exists for.
        ("tests/guest/manifest.json", ""),
        ("tests/guest/src/l.mock", "decl lib"),
        // Deeper files under a test dir still promote (relative path
        // `tests/deep/d.mock` starts with the declared segment).
        ("tests/deep/d.mock", "decl deep"),
    ]);
    let adapters: Vec<Box<dyn LanguageAdapter>> = vec![Box::new(PackageTestDirsAdapter)];
    let (graph, _) = assemble(dir.path(), &adapters, &[]).unwrap();
    let role = |p: &str| {
        graph.files[graph.file_id(&ProjectPath(SmolStr::new(p))).unwrap().0 as usize]
            .class
            .unwrap()
            .role
    };
    assert_eq!(role("src/a.mock"), FileRole::Production);
    assert_eq!(role("tests/t.mock"), FileRole::Test);
    assert_eq!(role("tests/deep/d.mock"), FileRole::Test);
    assert_eq!(
        role("tests/guest/src/l.mock"),
        FileRole::Production,
        "the nested package's own manifest is the anchor, not the ancestor's tests/"
    );
}

#[test]
fn package_test_dirs_anchor_at_the_root_for_the_implicit_package() {
    // No manifest anywhere: the implicit package's anchor is the project root, so only
    // a top-level `tests/` matches — a deeper `tests/` belongs to no known package
    // convention and stays production.
    let dir = project(&[("tests/t.mock", "decl t"), ("deep/tests/d.mock", "decl d")]);
    let adapters: Vec<Box<dyn LanguageAdapter>> = vec![Box::new(PackageTestDirsAdapter)];
    let (graph, _) = assemble(dir.path(), &adapters, &[]).unwrap();
    let role = |p: &str| {
        graph.files[graph.file_id(&ProjectPath(SmolStr::new(p))).unwrap().0 as usize]
            .class
            .unwrap()
            .role
    };
    assert_eq!(role("tests/t.mock"), FileRole::Test);
    assert_eq!(role("deep/tests/d.mock"), FileRole::Production);
}

#[test]
fn unclaimed_files_still_become_file_nodes() {
    let dir = project(&[("README.md", "hello")]);
    let (graph, diags) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(diags.is_empty());
    assert_eq!(graph.files.len(), 1);
    assert!(graph.files[0].language.is_none());
}

#[test]
fn declarations_become_symbols_with_declares_edges() {
    let dir = project(&[("a.mock", "decl foo\ndecl bar")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(graph.symbols.len(), 2);
    assert_eq!(graph.symbols[0].name.as_str(), "foo");
    let file_id = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
    let declares: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| matches!(e.kind, EdgeKind::Declares { file, .. } if file == file_id))
        .collect();
    assert_eq!(declares.len(), 2);
}

#[test]
fn suppressions_are_collected_per_file_during_assembly() {
    let dir = project(&[
        ("a.mock", "decl foo\nsuppress unused"),
        ("b.mock", "decl bar"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
    assert_eq!(graph.suppressions.len(), 1);
    assert_eq!(graph.suppressions[0].0, a);
    assert_eq!(graph.suppressions[0].1.category.as_str(), "unused");
}

#[test]
fn relative_import_produces_imports_file_edge() {
    let dir = project(&[("a.mock", "import ./b.mock"), ("b.mock", "decl target")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
    let b = graph.file_id(&ProjectPath(SmolStr::new("b.mock"))).unwrap();
    assert!(graph
        .edges
        .iter()
        .any(|e| e.kind == EdgeKind::ImportsFile { from: a, to: b }));
}

#[test]
fn same_unit_files_resolve_each_other_s_symbols_without_any_import() {
    // Go's ordinary case (FileFacts::unit): two files sharing a package
    // directory call each other's declarations with no import statement at all.
    let dir = project(&[
        ("pkg/a.mock", "unit pkg\nref target"),
        ("pkg/b.mock", "unit pkg\nprivate-decl target"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a = graph
        .file_id(&ProjectPath(SmolStr::new("pkg/a.mock")))
        .unwrap();
    let target = graph
        .symbols
        .iter()
        .position(|s| s.name.as_str() == "target")
        .map(|i| crate::vocab::SymbolId(i as u32))
        .unwrap();
    assert!(graph.edges.iter().any(|e| matches!(
        e.kind,
        EdgeKind::References { from, to, .. } if from == NodeRef::File(a) && to == target
    )));
}

#[test]
fn different_unit_files_do_not_resolve_each_other_s_symbols() {
    let dir = project(&[
        ("pkg1/a.mock", "unit pkg1\nref target"),
        ("pkg2/b.mock", "unit pkg2\nprivate-decl target"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a = graph
        .file_id(&ProjectPath(SmolStr::new("pkg1/a.mock")))
        .unwrap();
    assert!(!graph
        .edges
        .iter()
        .any(|e| matches!(e.kind, EdgeKind::References { from, .. } if from == NodeRef::File(a))));
}

// -------------------------------------------------- member-call fallback

fn reference_edges_to<'g>(graph: &'g ProjectGraph, name: &str) -> Vec<&'g Edge> {
    let target = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == name)
            .unwrap() as u32,
    );
    graph
        .edges
        .iter()
        .filter(|e| matches!(e.kind, EdgeKind::References { to, .. } if to == target))
        .collect()
}

#[test]
fn member_call_resolves_via_fallback_at_probable_with_one_candidate() {
    // The Go bug this exists for: `t.helper()` is a bare `helper` reference; the
    // declaration is a member of T. Exact resolution must miss (members never enter the
    // bare-name table), the duck-typed fallback must hit at Probable.
    let dir = project(&[(
        "a.mock",
        "member-decl T helper\ndecl caller\nref helper\nroot-decl caller",
    )]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "helper");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Probable);
}

#[test]
fn member_call_with_several_candidates_keeps_all_alive_at_possible() {
    let dir = project(&[(
        "a.mock",
        "member-decl T get\nmember-decl U get\nref get\nroot-file",
    )]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let t_get = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.member_of.as_deref() == Some("T"))
            .unwrap() as u32,
    );
    let u_get = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.member_of.as_deref() == Some("U"))
            .unwrap() as u32,
    );
    for target in [t_get, u_get] {
        let edge = graph
            .edges
            .iter()
            .find(|e| matches!(e.kind, EdgeKind::References { to, .. } if to == target))
            .expect("every same-named member candidate gets a keep-alive edge");
        assert_eq!(edge.confidence, Confidence::Possible);
    }
}

#[test]
fn member_fallback_reaches_same_unit_siblings() {
    // The cross-file half of the Go bug: the method lives in a sibling file of the same
    // package; the caller has no import and no same-file candidate.
    let dir = project(&[
        (
            "pkg/a.mock",
            "unit pkg\ndecl caller\nref helper\nroot-decl caller",
        ),
        ("pkg/b.mock", "unit pkg\nmember-decl T helper"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "helper");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Probable);
}

#[test]
fn member_never_certain_resolves_and_exact_names_still_win() {
    // A free declaration with the same name as a member: the exact (Certain) resolution
    // wins and the fallback never fires — members must not pollute exact-name lookup.
    let dir = project(&[(
        "a.mock",
        "decl helper\nmember-decl T helper\nref helper\nroot-file",
    )]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let free = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == "helper" && s.member_of.is_none())
            .unwrap() as u32,
    );
    let member = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == "helper" && s.member_of.is_some())
            .unwrap() as u32,
    );
    assert!(graph.edges.iter().any(|e| matches!(
        e.kind, EdgeKind::References { to, .. } if to == free
    ) && e.confidence == Confidence::Certain));
    assert!(!graph
        .edges
        .iter()
        .any(|e| matches!(e.kind, EdgeKind::References { to, .. } if to == member)));
}

#[test]
fn unexported_member_is_not_a_candidate_outside_its_unit() {
    // the visibility-scoped candidacy: a Unit-scoped member (mock ladder level
    // 0) in another unit can't plausibly be the callee — Go's own rule (an unexported
    // method is only legally callable in-package).
    let dir = project(&[
        (
            "pkg1/a.mock",
            "unit pkg1\ndecl caller\nref helper\nroot-decl caller",
        ),
        ("pkg2/b.mock", "unit pkg2\nmember-decl T helper"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(reference_edges_to(&graph, "helper").is_empty());
}

#[test]
fn exported_member_is_a_candidate_project_wide() {
    // The other half: a Public-scoped member (mock ladder level 1) is a candidate for
    // any same-language site, unit boundaries notwithstanding.
    let dir = project(&[
        (
            "pkg1/a.mock",
            "unit pkg1\ndecl caller\nref helper\nroot-decl caller",
        ),
        ("pkg2/b.mock", "unit pkg2\nmember-decl-exported T helper"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "helper");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Probable);
}

// -------------------------------------------------- qualified references

#[test]
fn aliased_import_qualifier_resolves_inside_the_target() {
    let dir = project(&[
        ("a.mock", "import-as j ./b.mock\nqref j Marshal\nroot-file"),
        ("b.mock", "decl Marshal"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "Marshal");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn unaliased_import_qualifier_comes_from_the_targets_unit_name() {
    // The dir≠package fix: the import specifier's last segment is
    // "b.mock", but the target declares itself `yaml` — the qualifier the importer
    // actually writes. Resolution must use the target's declared name, not a specifier
    // guess.
    let dir = project(&[
        ("a.mock", "import ./b.mock\nqref yaml Parse\nroot-file"),
        ("b.mock", "unit-name yaml\ndecl Parse"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "Parse");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn qualified_resolution_reaches_the_targets_unit_siblings() {
    // A Go import names a *package*; the resolved target is one representative file, but
    // the accessed symbol may live in any same-unit sibling.
    let dir = project(&[
        ("app/a.mock", "import-as p ./b.mock\nqref p X\nroot-file"),
        ("app/b.mock", "unit app#p"),
        ("app/c.mock", "unit app#p\ndecl X"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "X");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn a_brace_member_naming_a_submodule_hops_through_the_module_file() {
    // `use crate::internals::{attr, check, Ctxt};` followed by `check::check(cx, ..)`.
    // The import registers ONE qualifier (the target's own name), and `check` stays a mere
    // binding that resolves to no symbol — the module file declares nothing called `check`,
    // it only re-links the file with its own `mod check;`. The answer is in the module
    // file's OWN import table, one hop away (`internal/detection-gaps.md` §8): without it
    // the whole family of helpers behind such a submodule reads as dead.
    let dir = project(&[
        (
            "src/main.mock",
            "import ./internals/mod.mock check,Ctxt\nqref check verify\nroot-file",
        ),
        ("src/internals/mod.mock", "import-as check ./check.mock"),
        ("src/internals/check.mock", "decl verify"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "verify");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn a_direct_qualifier_outranks_the_module_hop() {
    // Both shapes bind the name `check` here: an explicit alias onto one file, and a brace
    // member of a module file that itself links a DIFFERENT `check`. The hop is derived
    // evidence and never overwrites an import that states the binding outright — otherwise
    // a same-named submodule anywhere in the import list could steal a direct alias.
    let dir = project(
        &[
            (
                "src/main.mock",
                "import-as check ./direct.mock\nimport ./internals/mod.mock check\nqref check verify\nroot-file",
            ),
            ("src/direct.mock", "decl verify"),
            ("src/internals/mod.mock", "import-as check ./check.mock"),
            ("src/internals/check.mock", "decl verify"),
        ],
    );
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "verify");
    assert_eq!(edges.len(), 1);
    let target = match edges[0].kind {
        EdgeKind::References { to, .. } => to,
        _ => unreachable!(),
    };
    assert_eq!(
        graph.files[graph.symbols[target.0 as usize].file.0 as usize]
            .path
            .0
            .as_str(),
        "src/direct.mock"
    );
}

#[test]
fn a_type_naming_qualifier_resolves_the_targets_member() {
    // The alias names a TYPE in the target, and the reference is that type's member —
    // Rust's `Thing::from_low_args()` through `use crate::thing::Thing`. The bare table
    // misses (members aren't in it); the target's member table under the qualifier
    // itself is the hit, at Certain.
    let dir = project(&[
        (
            "a.mock",
            "import-as Thing ./b.mock\nqref Thing from_low\nroot-file",
        ),
        ("b.mock", "decl Thing\nmember-decl-exported Thing from_low"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "from_low");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn an_imported_name_as_qualifier_reaches_the_bound_symbols_members() {
    // `use …::{…, SearchMode, …}` then `SearchMode::Standard`: the grouped use-list
    // registers no alias, but the BINDING names the type — the member resolves in the
    // bound symbol's home file at Certain. A member the type doesn't have must NOT
    // settle: it falls through to the duck fallback like any receiver access.
    let dir = project(&[
        (
            "a.mock",
            "import ./b.mock Mode\nqref Mode Standard\nqref Mode stray\nroot-file",
        ),
        (
            "b.mock",
            "decl Mode\nmember-decl-exported Mode Standard\n\
                 decl Other\nmember-decl-exported Other stray",
        ),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let standard = reference_edges_to(&graph, "Standard");
    assert_eq!(standard.len(), 1);
    assert_eq!(standard[0].confidence, Confidence::Certain);
    // `Mode::stray` doesn't exist on Mode — the duck fallback still finds Other.stray
    // as a plausible candidate instead of settling to silence.
    let stray = reference_edges_to(&graph, "stray");
    assert_eq!(stray.len(), 1);
    assert_eq!(stray[0].confidence, Confidence::Probable);
}

#[test]
fn a_dotted_pointer_chains_through_member_type_facts() {
    // the cross-file tier: `low.context_separator.into_bytes()` in a.mock
    // where `low: LowArgs` — the adapter emitted the pointer `LowArgs.context_separator`;
    // LowArgs and its field's type live in b.mock. Every hop is a declared fact:
    // LowArgs in scope (binding) → its home's member-type fact yields ContextSeparator
    // → resolved in that same home → `into_bytes` in its member table, at Certain.
    let dir = project(&[
        (
            "a.mock",
            "import ./b.mock LowArgs\nqref LowArgs.context_separator into_bytes\nroot-file",
        ),
        (
            "b.mock",
            "decl LowArgs\ndecl ContextSeparator\n\
                 member-type LowArgs context_separator ContextSeparator\n\
                 member-decl-exported ContextSeparator into_bytes",
        ),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "into_bytes");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn an_unwrap_marked_hop_resolves_through_the_payload_parameter() {
    // `let chir = config.build()?` then `chir.line_terminator()`: the pointer
    // `Config.build?` takes the member's yields_param (the Result payload), not the
    // wrapper — and the payload type itself gets the Read credit.
    let dir = project(&[
        (
            "a.mock",
            "import ./b.mock Config\nqref Config.build? line_terminator\nroot-file",
        ),
        (
            "b.mock",
            "decl Config\ndecl ConfiguredHIR\n\
                 member-type Config build Result<ConfiguredHIR>\n\
                 member-decl-exported ConfiguredHIR line_terminator",
        ),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "line_terminator");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Certain);
    let hir = graph
        .symbols
        .iter()
        .position(|s| s.name == "ConfiguredHIR")
        .unwrap() as u32;
    assert!(
        graph.edges.iter().any(|e| matches!(
            e.kind,
            EdgeKind::References { to, kind: RefKind::Read, .. } if to == SymbolId(hir)
        )),
        "the payload type is credited with a Read from the site"
    );
}

#[test]
fn an_indexed_projection_selects_that_type_parameter() {
    // `Registry.get?1` projects the SECOND type argument of the member's annotation
    // (`Map<Key, Value>` → `Value`): the `?N` marker is structural — which index an
    // operation extracts is the adapter's knowledge, the core just follows it.
    let dir = project(&[
        (
            "a.mock",
            "import ./b.mock Registry\nqref Registry.get?1 into_bytes\nroot-file",
        ),
        (
            "b.mock",
            "decl Registry\ndecl Key\ndecl Value\n\
                 member-type Registry get Map<Key,Value>\n\
                 member-decl-exported Value into_bytes",
        ),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "into_bytes");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Certain);
    let value = graph
        .symbols
        .iter()
        .position(|s| s.name == "Value")
        .unwrap() as u32;
    assert!(
        graph.edges.iter().any(|e| matches!(
            e.kind,
            EdgeKind::References { to, kind: RefKind::Read, .. } if to == SymbolId(value)
        )),
        "the projected parameter type is credited with a Read from the site"
    );
}

#[test]
fn a_declared_executable_invocation_emits_an_invokes_file_edge() {
    // `invokes-executable app` resolves through the manifest's named executable
    // targets to the bin's entry file (the invoked-program rule); a name no
    // manifest declares emits nothing — silence, never a guess.
    let dir = project(&[
        ("manifest.json", "name app\nexecutable app src/main.mock"),
        ("src/main.mock", "decl main\nroot-decl main"),
        (
            "tests/e2e.test.mock",
            "invokes-executable app\ninvokes-executable ghost",
        ),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let file_id = |suffix: &str| {
        FileId(
            graph
                .files
                .iter()
                .position(|f| f.path.0.ends_with(suffix))
                .unwrap() as u32,
        )
    };
    let invokes: Vec<(NodeRef, FileId)> = graph
        .edges
        .iter()
        .filter_map(|e| match e.kind {
            EdgeKind::InvokesFile { from, to } => Some((from, to)),
            _ => None,
        })
        .collect();
    assert_eq!(
        invokes,
        vec![(
            NodeRef::File(file_id("e2e.test.mock")),
            file_id("src/main.mock")
        )],
        "the declared name resolves to the bin entry; the unknown one stays silent"
    );
}

#[test]
fn a_dotted_pointer_with_no_fact_falls_to_the_duck_fallback() {
    // The chain misses (no member-type fact): the member name still reaches the
    // duck fallback — a pointer never settles.
    let dir = project(&[
        (
            "a.mock",
            "import ./b.mock LowArgs\nqref LowArgs.mystery into_bytes\nroot-file",
        ),
        (
            "b.mock",
            "decl LowArgs\ndecl ContextSeparator\n\
                 member-decl-exported ContextSeparator into_bytes",
        ),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "into_bytes");
    assert_eq!(edges.len(), 1, "duck fallback still reaches the member");
    assert_eq!(edges[0].confidence, Confidence::Probable);
}

#[test]
fn twins_declared_in_one_file_both_receive_the_reference() {
    // The `#[cfg]` alternate pair, which for Rust sits in ONE file rather than across two:
    // `#[cfg(target_os = "macos")] fn socket_dir` beside `#[cfg(not(…))] fn socket_dir`. The
    // single-slot bare table keeps one and displaces the other, so the displaced one had no
    // incoming edge and read as `unused` — a false "delete this" on code every non-mac build
    // compiles. Twins are tracked per UNIT, so this only works once the language keys one;
    // a per-file unit is what a file-scoped module tree gives it (RFC 0012 §8).
    let dir = project(&[
        ("app/a.mock", "import ./b.mock\nroot-file"),
        (
            "app/b.mock",
            "unit app/b\ndecl socket_dir\ndecl socket_dir\ndecl caller\nref-in caller socket_dir",
        ),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    // Both declarations share a name, so count the DISTINCT targets rather than the edges to
    // whichever symbol a name lookup happens to find first.
    let alternates: Vec<SymbolId> = graph
        .symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| s.name == "socket_dir")
        .map(|(i, _)| SymbolId(i as u32))
        .collect();
    assert_eq!(alternates.len(), 2, "both alternates are declared");
    for alternate in alternates {
        assert!(
            graph.edges.iter().any(|e| matches!(
                e.kind,
                EdgeKind::References { to, .. } if to == alternate
            )),
            "each alternate is live under its own configuration; {alternate:?} had none"
        );
    }
}

#[test]
fn a_qualified_member_hit_lands_on_every_twin_declaration() {
    // cfg-alternated impls declare `Data.from_path` twice; the qualified reference
    // targets whichever is compiled, so BOTH must receive the edge — the single-slot
    // table's winner alone would leave the displaced twin reading as dead.
    let dir = project(&[(
        "a.mock",
        "decl Data\nmember-decl-exported Data from_path\n\
             member-decl-exported Data from_path\nqref Data from_path\nroot-file",
    )]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let hit: std::collections::HashSet<SymbolId> = graph
        .edges
        .iter()
        .filter_map(|e| match e.kind {
            EdgeKind::References { to, .. }
                if graph.symbols[to.0 as usize].name == "from_path"
                    && e.confidence == Confidence::Certain =>
            {
                Some(to)
            }
            _ => None,
        })
        .collect();
    assert_eq!(hit.len(), 2, "a Certain edge lands on EACH twin: {hit:?}");
}

#[test]
fn a_same_file_declared_type_as_qualifier_reaches_its_members() {
    // An adapter that types a receiver rewrites `args.matcher()` to qualifier `Mode`,
    // and `Mode` may be declared in the referencing file itself — no import involved.
    let dir = project(&[(
        "a.mock",
        "decl Mode\nmember-decl-exported Mode Standard\nqref Mode Standard\nroot-file",
    )]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "Standard");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn a_barrel_routed_type_qualifier_reaches_the_originals_members() {
    // The importer binds the type through a BARREL (`use crate::opts::{SearchMode}` →
    // the barrel re-exports it from a leaf file): the alias lands on the barrel file,
    // whose bare table holds the fixpoint's alias to the original symbol — the member
    // lookup must follow that symbol home, not stop at the barrel's own member table.
    let dir = project(&[
        (
            "a.mock",
            "import-as SearchMode ./barrel.mock\nqref SearchMode Standard\nroot-file",
        ),
        ("barrel.mock", "reexport ./leaf.mock SearchMode"),
        (
            "leaf.mock",
            "decl SearchMode\nmember-decl-exported SearchMode Standard",
        ),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "Standard");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn a_reexported_glob_aliases_the_targets_exported_surface() {
    // `export * from './leaf'` / `pub use x::*`: the barrel has no binding names up
    // front, yet a consumer reaching through it (`qref b read` with `b` aliased to the
    // barrel) must land on the leaf's export — and the leaf's private names must NOT
    // travel. Chained through a second barrel to exercise the fixpoint rounds.
    let dir = project(&[
        (
            "a.mock",
            "import-as b ./barrel.mock\nqref b read\nroot-file",
        ),
        ("barrel.mock", "reexport-opaque ./mid.mock"),
        ("mid.mock", "reexport-opaque ./leaf.mock"),
        ("leaf.mock", "decl read\nprivate-decl hidden"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "read");
    assert_eq!(edges.len(), 1, "glob re-export chain must resolve `read`");
    assert_eq!(edges[0].confidence, Confidence::Certain);
    assert!(
        reference_edges_to(&graph, "hidden").is_empty(),
        "a private name never travels through a glob re-export"
    );
}

#[test]
fn a_matched_qualifier_settles_resolution_even_on_a_miss() {
    // `j.Marshal` where the target has no `Marshal`: the name lives in that target or
    // nowhere — a same-file free `Marshal` must NOT capture the qualified reference.
    let dir = project(&[
        (
            "a.mock",
            "import-as j ./b.mock\ndecl Marshal\nqref j Marshal\nroot-file",
        ),
        ("b.mock", "decl Other"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(
        reference_edges_to(&graph, "Marshal").is_empty(),
        "the local free decl must not capture a qualified reference"
    );
}

#[test]
fn a_reconstructed_imports_qualifier_does_not_settle_on_a_miss() {
    // The mirror of the test above, and the rule that let the core stop splitting
    // specifiers on `::`. An import the adapter reconstructed from a use site names its
    // qualifier like any other, but it is the adapter's reading of a path, not a statement
    // the file makes — so it does not close the namespace: `j.Marshal` missing in the
    // target falls through to the duck-typed member fallback exactly as an unregistered
    // qualifier would. Settling on it would let one misread path kill a live method.
    //
    // The fixture states `reconstructed` outright. It used to lean on the import's
    // CONFIDENCE as a proxy, which was wrong for a whole class: Rust's `crate`/`self`/`super`
    // rooted synthetic imports are `Certain` about where they resolve while being no
    // statement at all.
    let dir = project(&[
        (
            "a.mock",
            "import-reconstructed-as j ./b.mock\nmember-decl T Marshal\nqref j Marshal\nroot-file",
        ),
        ("b.mock", "decl Other"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(
        !reference_edges_to(&graph, "Marshal").is_empty(),
        "a reconstructed import must not settle a miss away from the member fallback"
    );
}

#[test]
fn a_free_functions_return_type_carries_its_callers_member_access() {
    // `let entry = parse_entry(..); entry.path` — the receiver's type is the callee's
    // declared return, which lives in the CALLEE's file. Without the fact travelling,
    // `TreeEntry.path` has no cross-file use and `internal-only` advises narrowing a type
    // its own consumers read every day (`internal/detection-gaps.md` §3).
    let dir = project(
        &[
            (
                "a.mock",
                "import ./b.mock parse_entry\nqref parse_entry path\nroot-file",
            ),
            (
                "b.mock",
                "decl parse_entry\ncall-type parse_entry TreeEntry\ndecl TreeEntry\nmember-decl-exported TreeEntry path",
            ),
        ],
    );
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "path");
    assert_eq!(edges.len(), 1, "the member must resolve through the yield");
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn a_call_yield_projects_through_the_unwrap_marker() {
    // `let cfg = build(..)?` — the projection marker composes with the base hop exactly as
    // it does with a member one: parameter 0 of `Result<Config, Error>`.
    let dir = project(
        &[
            (
                "a.mock",
                "import ./b.mock build\nqref build? separator\nroot-file",
            ),
            (
                "b.mock",
                "decl build\ncall-type build Result<Config,Error>\ndecl Config\nmember-decl-exported Config separator",
            ),
        ],
    );
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "separator");
    assert_eq!(edges.len(), 1);
    // Certain is the discriminator: without the projected hop the duck-typed member
    // fallback still finds a `separator` by name, at a weaker tier.
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn a_pointer_base_may_be_a_qualifier_rather_than_a_symbol() {
    // `use …::rollup;` + `let rolled = rollup::directory_rollups(..); rolled.dirs` — the
    // function is never bound by name here, only its module is. Without resolving the base
    // through the qualifier table the pointer died at its first segment and every field the
    // caller reads looked file-local (`internal/detection-gaps.md` §3).
    let dir = project(
        &[
            (
                "a.mock",
                "import-as rollup ./b.mock\nqref rollup.directory_rollups dirs\nroot-file",
            ),
            (
                "b.mock",
                "decl directory_rollups\ncall-type directory_rollups DirRollup\ndecl DirRollup\nmember-decl-exported DirRollup dirs",
            ),
        ],
    );
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "dirs");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn a_hop_through_a_language_provided_type_reaches_the_element() {
    // `Box<Item>` is a type no file declares — it has no home to hold a fact — so the chain
    // used to stop dead on it and everything behind it read as unused. The adapter's builtin
    // table says what iterating one yields, in terms of its own argument, and the argument
    // comes from the receiver: the two halves of `internal/detection-gaps.md` §3's last case.
    let dir = project(&[
        (
            "a.mock",
            "import ./b.mock make\nqref make.@element field\nroot-file",
        ),
        (
            "b.mock",
            "decl make\ncall-type make Box<Item>\ndecl Item\nmember-decl-exported Item field",
        ),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edges = reference_edges_to(&graph, "field");
    assert_eq!(edges.len(), 1, "the element hop must reach the member");
    assert_eq!(edges[0].confidence, Confidence::Certain);
}

#[test]
fn a_qualifier_bound_to_alternates_reaches_every_one() {
    // tokio's platform modules: `#[cfg(windows)] #[path="sys.rs"] mod imp;` beside
    // `#[cfg(not(windows))] #[path="stub.rs"] mod imp;`. One name, two real files, and
    // `imp::ctrl_break()` names a live function in each — kndo analyzes the union of build
    // configurations. Keeping only the first left the other with no incoming edge and a false
    // `unused`: the same shape `symbol_twins_per_unit` fixes for declarations, one level up at
    // the module binding.
    let dir = project(&[
        (
            "a.mock",
            "import-as imp ./sys.mock\nimport-as imp ./stub.mock\nqref imp ctrl_break\nroot-file",
        ),
        ("sys.mock", "decl ctrl_break"),
        ("stub.mock", "decl ctrl_break"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    for file in [FileId(1), FileId(2)] {
        let alternate = graph
            .symbols
            .iter()
            .position(|s| s.name == "ctrl_break" && s.file == file)
            .map(|i| SymbolId(i as u32))
            .expect("each alternate is declared");
        assert!(
            graph.edges.iter().any(|e| matches!(
                e.kind,
                EdgeKind::References { to, .. } if to == alternate
            )),
            "the qualifier names both files; {alternate:?} had no edge"
        );
    }
}

#[test]
fn a_synthesized_import_never_shadows_the_files_own_declaration() {
    // tokio's `dump.rs`: it declares `pub struct Trace` AND mentions `super::task::trace::Trace`
    // — a different type — in a field. The adapter synthesizes an import for that inline path so
    // the mention resolves, and its binding used to outrank the file's own declaration, so the
    // file's `-> &Trace` bound to the type it merely names in passing and `private-type-leak`
    // reported a leak that is not there.
    //
    // No language kndo supports lets a WRITTEN import shadow a same-named local declaration
    // (Rust E0255), so a collision here can only ever come from a synthetic import.
    let dir = project(
        &[
            (
                "a.mock",
                "import-reconstructed ./b.mock Trace\ndecl Trace\ndecl caller\nref-in caller Trace\nroot-file",
            ),
            ("b.mock", "decl Trace"),
        ],
    );
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let local = graph
        .symbols
        .iter()
        .position(|s| s.name == "Trace" && s.file == FileId(0))
        .map(|i| SymbolId(i as u32))
        .expect("a.mock declares Trace");
    let edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| matches!(e.kind, EdgeKind::References { to, .. } if to == local))
        .collect();
    assert_eq!(
        edges.len(),
        1,
        "the bare reference belongs to the declaration in this very file"
    );
}

#[test]
fn receiver_qualifier_skips_free_names_and_duck_types_to_members() {
    // `t.helper()`: `t` matches no import, so the name is a member access by
    // construction — the same-file free `helper` is not a candidate; the member is,
    // via the duck fallback.
    let dir = project(&[(
        "a.mock",
        "decl helper\nmember-decl T helper\nqref t helper\nroot-file",
    )]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let free = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == "helper" && s.member_of.is_none())
            .unwrap() as u32,
    );
    let member = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == "helper" && s.member_of.is_some())
            .unwrap() as u32,
    );
    assert!(!graph
        .edges
        .iter()
        .any(|e| matches!(e.kind, EdgeKind::References { to, .. } if to == free)));
    let member_edge = graph
        .edges
        .iter()
        .find(|e| matches!(e.kind, EdgeKind::References { to, .. } if to == member))
        .expect("member fallback edge");
    assert_eq!(member_edge.confidence, Confidence::Probable);
}

// -------------------------------------------------- detected_origin

#[test]
fn detected_origin_overrides_the_claim_time_origin_on_the_file_node() {
    // Claim classifies by path (Authored here); extraction saw a generated banner — the
    // FileNode must carry the corrected origin so every analysis exemption sees it.
    let dir = project(&[("a.mock", "detected-generated\ndecl dead")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let class = graph.files[0].class.expect("claimed");
    assert_eq!(class.origin, FileOrigin::Generated);
    assert_eq!(class.role, FileRole::Production, "role stays claim-time");
}

#[test]
fn member_with_no_matching_call_anywhere_stays_certain_dead() {
    // Dead-is-certain survives the fallback: zero same-named call sites ⇒ zero edges.
    let dir = project(&[("a.mock", "member-decl T orphan\ndecl live\nroot-decl live")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(reference_edges_to(&graph, "orphan").is_empty());
}

// -------------------------------------------------- test regions (FileFacts::test_spans)

#[test]
fn test_gated_module_link_demotes_the_target_file_to_test_role() {
    // `#[cfg(test)] mod tests;` → the child file is a whole-file test the path claim
    // cannot see: phase 2.55 demotes it, and phase 2.6 then gives it the Test root.
    let dir = project(&[
        (
            "a.mock",
            "decl keep\nroot-decl keep\ntest-region 5 9\nmod-link ./child.mock 6",
        ),
        ("child.mock", "decl helper"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let child = graph
        .files
        .iter()
        .position(|f| f.path.0 == "child.mock")
        .unwrap();
    assert_eq!(
        graph.files[child].class.unwrap().role,
        FileRole::Test,
        "a file linked only from inside a test region is test infrastructure"
    );
    assert!(
        graph.edges.iter().any(|e| matches!(
            e.kind,
            EdgeKind::Root {
                kind: RootKind::Test,
                target: NodeRef::File(f),
            } if f.0 as usize == child
        )),
        "the demoted file gets its role-derived Test root"
    );
}

#[test]
fn a_production_module_link_vetoes_the_demotion() {
    // The same child linked from a second file's production code stays production: any
    // ungated module link means the file is compiled outside test builds.
    let dir = project(&[
        ("a.mock", "test-region 5 9\nmod-link ./child.mock 6"),
        ("b.mock", "mod-link ./child.mock 2"),
        ("child.mock", "decl helper"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let child = graph
        .files
        .iter()
        .position(|f| f.path.0 == "child.mock")
        .unwrap();
    assert_eq!(graph.files[child].class.unwrap().role, FileRole::Production);
}

#[test]
fn declarations_inside_test_regions_get_derived_test_roots() {
    // The single-producer contract: adapters declare only the spans;
    // assembly derives the in-source Test roots by containment. A declaration outside
    // every region gets none.
    let dir = project(&[(
        "a.mock",
        "decl-at 2 prod_fn\ntest-region 5 9\ndecl-at 6 test_helper",
    )]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let sym = |name: &str| {
        SymbolId(
            graph
                .symbols
                .iter()
                .position(|s| s.name.as_str() == name)
                .unwrap() as u32,
        )
    };
    let test_rooted = |s: SymbolId| {
        graph.edges.iter().any(|e| {
            matches!(
                e.kind,
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::Symbol(t),
                } if t == s
            )
        })
    };
    assert!(
        test_rooted(sym("test_helper")),
        "span-contained declaration derives a Certain Test root"
    );
    assert!(!test_rooted(sym("prod_fn")));
}

#[test]
fn file_node_carries_sorted_test_spans() {
    let dir = project(&[("a.mock", "test-region 20 30\ntest-region 5 9\ndecl x")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(
        graph.files[0].test_spans,
        vec![
            Span {
                start: (5, 1),
                end: (9, 999)
            },
            Span {
                start: (20, 1),
                end: (30, 999)
            },
        ],
        "canonical order: sorted regardless of emission order"
    );
}

#[test]
fn import_gating_participates_in_the_surface_signature() {
    // Moving an import across a test-region boundary changes phase 2.55's inputs and
    // hygiene's site role — the patch must decline, so the signature must move.
    let claim = MockAdapter
        .claim(&ProjectPath(SmolStr::new("a.mock")))
        .unwrap();
    let sig_of = |content: &str| {
        let path = ProjectPath(SmolStr::new("a.mock"));
        let facts = MockAdapter.extract(&SourceFile {
            path: &path,
            content: content.as_bytes(),
        });
        surface_signature("mock", 1, &claim, &facts)
    };
    let gated = sig_of("test-region 5 9\nmod-link ./child.mock 6");
    let ungated = sig_of("test-region 5 9\nmod-link ./child.mock 2");
    assert_ne!(gated, ungated, "gating flip must move the signature");
    // …but a pure region move that keeps the import on the same side does not.
    let same_side = sig_of("test-region 4 9\nmod-link ./child.mock 6");
    assert_eq!(
        gated, same_side,
        "reformat-shaped shifts keep the signature"
    );
}

// -------------------------------------------------- within attribution

#[test]
fn within_attributes_the_reference_edge_to_the_enclosing_symbol() {
    let dir = project(&[("a.mock", "decl caller\ndecl callee\nref-in caller callee")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let caller = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == "caller")
            .unwrap() as u32,
    );
    let callee = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == "callee")
            .unwrap() as u32,
    );
    assert!(graph.edges.iter().any(|e| matches!(
        e.kind,
        EdgeKind::References { from, to, .. }
            if from == NodeRef::Symbol(caller) && to == callee
    )));
}

#[test]
fn unresolvable_within_falls_back_to_file_attribution() {
    // The design's load-bearing safety property: a `within` naming nothing
    // this file declares degrades to today's file attribution — keep-alive, never a new
    // way to lose an edge.
    let dir = project(&[("a.mock", "decl callee\nref-in ghost callee\nroot-file")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
    let callee = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == "callee")
            .unwrap() as u32,
    );
    assert!(graph.edges.iter().any(|e| matches!(
        e.kind,
        EdgeKind::References { from, to, .. }
            if from == NodeRef::File(a) && to == callee
    )));
}

#[test]
fn transitively_dead_code_is_visible() {
    // The precision symbol attribution exists for: `main → a` (both alive); dead `z → b` — b must
    // die with z instead of surviving through the live file's blanket attribution.
    let dir = project(&[(
        "a.mock",
        "decl main\ndecl a\ndecl z\ndecl b\nref-in main a\nref-in z b\nroot-decl main",
    )]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    let unused: Vec<&str> = findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(unused.contains(&"z"), "{unused:?}");
    assert!(
        unused.contains(&"b"),
        "b is only called by dead z and must die with it: {unused:?}"
    );
    assert!(!unused.contains(&"a"), "{unused:?}");
    assert!(!unused.contains(&"main"), "{unused:?}");
}

#[test]
fn module_level_references_still_fire_when_the_file_loads() {
    // `within: None` = load-time code: importing the file keeps its module-level
    // references alive (the module-load rule).
    let dir = project(&[
        ("entry.mock", "import ./lib.mock\nroot-file"),
        ("lib.mock", "decl used\nref used"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    // Alive is the claim — an `internal-only` info finding (exported, used same-file
    // only) is separate, correct, and out of scope here.
    assert!(!findings
        .iter()
        .any(|f| f.category == "unused" && f.location.symbol.as_deref() == Some("used")));
}

#[test]
fn files_with_no_unit_are_unaffected_same_name_in_another_unit_does_not_leak_in() {
    // A file that never sets `unit` (a file-scoped language) must be unaffected
    // — no accidental cross-file resolution just because some *other*, unrelated file
    // happens to declare a `unit`.
    let dir = project(&[
        ("a.mock", "ref target"),
        ("pkg/b.mock", "unit pkg\nprivate-decl target"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
    assert!(!graph
        .edges
        .iter()
        .any(|e| matches!(e.kind, EdgeKind::References { from, .. } if from == NodeRef::File(a))));
}

#[test]
fn bare_import_produces_dependency_node_and_edge() {
    let dir = project(&[("a.mock", "import lodash")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(graph.dependencies.len(), 1);
    assert_eq!(graph.dependencies[0].name.as_str(), "lodash");
    let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::ImportsDependency {
            from: a,
            to: DependencyId(0)
        }));
}

#[test]
fn same_dependency_imported_twice_shares_one_node() {
    let dir = project(&[("a.mock", "import lodash"), ("b.mock", "import lodash")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(graph.dependencies.len(), 1);
}

#[test]
fn unresolved_import_produces_no_edge_and_no_diagnostic() {
    let dir = project(&[("a.mock", "import ./missing.mock")]);
    let (graph, diags) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(graph
        .edges
        .iter()
        .all(|e| !matches!(e.kind, EdgeKind::ImportsFile { .. })));
    assert!(
        diags.is_empty(),
        "unresolved imports are intentionally silent — no `unresolved` analysis exists"
    );
}

#[test]
fn manifest_is_not_itself_claimed_as_source() {
    let dir = project(&[("manifest.json", "dep lodash")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(graph.files.len(), 1);
    assert!(
        graph.files[0].language.is_none(),
        "manifests are not claimed as source"
    );
}

#[test]
fn no_manifest_means_everyone_owns_the_implicit_package() {
    let dir = project(&[("a.mock", "decl f")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(graph.packages.len(), 1);
    assert!(graph.packages[0].manifest.is_none());
    assert_eq!(graph.files[0].package, PackageId(0));
}

#[test]
fn root_manifest_owns_every_file_under_it() {
    let dir = project(&[("manifest.json", "dep lodash"), ("src/a.mock", "decl f")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(graph.packages.len(), 2);
    let manifest_id = graph
        .file_id(&ProjectPath(SmolStr::new("manifest.json")))
        .unwrap();
    let src_id = graph
        .file_id(&ProjectPath(SmolStr::new("src/a.mock")))
        .unwrap();
    assert_eq!(graph.files[manifest_id.0 as usize].package, PackageId(1));
    assert_eq!(graph.files[src_id.0 as usize].package, PackageId(1));
}

#[test]
fn nested_manifest_shadows_the_root_package_for_its_own_subtree() {
    let dir = project(&[
        ("manifest.json", "dep lodash"),
        ("root.mock", "decl f"),
        ("packages/ui/manifest.json", "dep react"),
        ("packages/ui/button.mock", "decl g"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    // implicit(0) is never used (a root manifest exists); root manifest is 1, nested is 2 —
    // discovery order is alphabetical, so `manifest.json` (root) claims package 1 before
    // `packages/ui/manifest.json` claims package 2.
    assert_eq!(graph.packages.len(), 3);

    let root_file = graph
        .file_id(&ProjectPath(SmolStr::new("root.mock")))
        .unwrap();
    let ui_manifest = graph
        .file_id(&ProjectPath(SmolStr::new("packages/ui/manifest.json")))
        .unwrap();
    let ui_file = graph
        .file_id(&ProjectPath(SmolStr::new("packages/ui/button.mock")))
        .unwrap();

    let root_package = graph.files[root_file.0 as usize].package;
    let ui_package = graph.files[ui_manifest.0 as usize].package;
    assert_ne!(
        root_package, ui_package,
        "the nested manifest must shadow the root one for its own subtree"
    );
    assert_eq!(graph.files[ui_file.0 as usize].package, ui_package);
}

#[test]
fn manifest_root_becomes_root_edge_to_target_file() {
    let dir = project(&[
        ("manifest.json", "root entry.mock"),
        ("entry.mock", "decl f"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let entry = graph
        .file_id(&ProjectPath(SmolStr::new("entry.mock")))
        .unwrap();
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::Root {
            kind: RootKind::Production,
            target: NodeRef::File(entry),
        }));
}

#[test]
fn library_root_files_promote_their_exported_symbols_to_production_roots() {
    // "Published/library: its public API is a production root — external
    // consumers exist by definition." A library's second named export, never called
    // by the package's own code, must not read as `unused`.
    let dir = project(&[
        ("manifest.json", "root entry.mock"),
        ("entry.mock", "decl publicApi\nprivate-decl helper"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let symbol_id = |name: &str| {
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == name)
            .map(|i| SymbolId(i as u32))
            .unwrap()
    };
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::Root {
            kind: RootKind::Production,
            target: NodeRef::Symbol(symbol_id("publicApi")),
        }));
    assert!(!graph.edges.iter().any(|e| matches!(e.kind,
        EdgeKind::Root { target: NodeRef::Symbol(s), .. } if s == symbol_id("helper"))));
}

#[test]
fn barrel_reexport_resolves_transparently_to_the_original_symbol() {
    // "Barrel files… resolved through, transparently." consumer.mock imports
    // `a` from barrel.mock, which never declares `a` itself — only re-exports it from
    // source.mock. A pure barrel entry point re-exporting hundreds of individual types
    // is a very common real-world shape.
    let dir = project(&[
        ("source.mock", "decl a"),
        ("barrel.mock", "reexport ./source.mock a"),
        ("consumer.mock", "import ./barrel.mock a\nref a"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a_symbol = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == "a")
            .unwrap() as u32,
    );
    // Exactly one `a` symbol exists — the barrel didn't fabricate a second declaration.
    assert_eq!(
        graph
            .symbols
            .iter()
            .filter(|s| s.name.as_str() == "a")
            .count(),
        1
    );
    let consumer = graph
        .file_id(&ProjectPath(SmolStr::new("consumer.mock")))
        .unwrap();
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::References {
            from: NodeRef::File(consumer),
            to: a_symbol,
            kind: crate::vocab::RefKind::Read,
        }));
}

/// the equivalence obligation, at the unit level: assemble cold with a cache,
/// mutate, assemble again (the patch path), and compare against a scratch full rebuild
/// of the same tree — the graphs must be EQUAL, not merely finding-equivalent. The
/// project needs ≥ 4 files so one changed file stays under the 30% dirty threshold.
fn patch_equivalence_case(
    name: &str,
    files: &[(&str, &str)],
    mutate: (&str, &str),
    expect_patch: bool,
) {
    // Pad with filler files so one changed file sits under the measured 5% work
    // threshold — the scenarios stay about the guard logic, not the
    // threshold arithmetic.
    let filler: Vec<(String, String)> = (0..20)
        .map(|i| (format!("filler{i}.mock"), format!("decl filler{i}")))
        .collect();
    let mut all: Vec<(&str, &str)> = files.to_vec();
    all.extend(filler.iter().map(|(n, c)| (n.as_str(), c.as_str())));
    let dir = project(&all);
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = crate::cache::ProjectCache::open(cache_dir.path());
    assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();

    fs::write(dir.path().join(mutate.0), mutate.1).unwrap();
    let (patched, patched_diags) =
        assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();
    assert_eq!(
        cache.graph_hits() > 0,
        expect_patch,
        "patch application expectation for {name}"
    );

    let (scratch, scratch_diags) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(
        patched, scratch,
        "patched graph must be identical to the full rebuild ({name})"
    );
    assert_eq!(patched_diags, scratch_diags, "diagnostics too ({name})");
}

#[test]
fn patch_applies_on_a_body_only_edit_and_is_byte_identical() {
    patch_equivalence_case(
        "patch-body-edit",
        &[
            ("a.mock", "decl x\nref helper"),
            ("b.mock", "decl helper\nimport ./a.mock x\nref x"),
            ("c.mock", "decl c1"),
            ("d.mock", "decl d1\nimport ./c.mock c1\nref c1"),
        ],
        // Same declarations, different references and spans — the body-only case.
        ("a.mock", "\n\ndecl x\nref c1\nref helper"),
        true,
    );
}

#[test]
fn patch_falls_back_on_a_surface_change_and_stays_identical() {
    patch_equivalence_case(
        "patch-surface-change",
        &[
            ("a.mock", "decl x"),
            ("b.mock", "decl b1\nimport ./a.mock x\nref x"),
            ("c.mock", "decl c1"),
            ("d.mock", "decl d1"),
        ],
        // A new exported declaration — other files' resolution could change.
        ("a.mock", "decl x\ndecl brand_new"),
        false,
    );
}

#[test]
fn patch_falls_back_when_imports_change() {
    patch_equivalence_case(
        "patch-import-change",
        &[
            ("a.mock", "decl a1\nimport ./c.mock c1"),
            ("b.mock", "decl b1"),
            ("c.mock", "decl c1"),
            ("d.mock", "decl d1"),
        ],
        // The import list is surface (dependency identity + workspace resolution).
        ("a.mock", "decl a1\nimport ./d.mock d1"),
        false,
    );
}

#[test]
fn patch_handles_reference_retargeting_within_the_body() {
    // The regenerated references must resolve against the *other* files' unchanged
    // tables — a.mock stops referencing helper and starts referencing other.
    let filler: Vec<(String, String)> = (0..20)
        .map(|i| (format!("filler{i}.mock"), format!("decl filler{i}")))
        .collect();
    let mut all: Vec<(&str, &str)> = vec![
        ("a.mock", "decl a1\nimport ./b.mock helper\nref helper"),
        ("b.mock", "decl helper\ndecl other"),
        ("c.mock", "decl c1"),
        ("d.mock", "decl d1"),
    ];
    all.extend(filler.iter().map(|(n, c)| (n.as_str(), c.as_str())));
    let dir = project(&all);
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = crate::cache::ProjectCache::open(cache_dir.path());
    assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();
    fs::write(
        dir.path().join("a.mock"),
        "decl a1\nimport ./b.mock other\nref other",
    )
    .unwrap();
    // Rebinding an import binding is an import change → surface change → full rebuild.
    let (patched, _) =
        assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();
    assert_eq!(cache.graph_hits(), 0, "import change must fall back");
    let (scratch, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(patched, scratch);
}

#[test]
fn patched_snapshot_serves_the_next_run_verbatim() {
    // The patched graph is persisted under the new key; a third run with no further
    // changes must hit that snapshot and reproduce the patched graph exactly.
    let filler: Vec<(String, String)> = (0..20)
        .map(|i| (format!("filler{i}.mock"), format!("decl filler{i}")))
        .collect();
    let mut all: Vec<(&str, &str)> = vec![
        ("a.mock", "decl x\nref y"),
        ("b.mock", "decl y"),
        ("c.mock", "decl c1"),
        ("d.mock", "decl d1"),
    ];
    all.extend(filler.iter().map(|(n, c)| (n.as_str(), c.as_str())));
    let dir = project(&all);
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = crate::cache::ProjectCache::open(cache_dir.path());
    assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();
    fs::write(dir.path().join("a.mock"), "\ndecl x\nref y").unwrap();
    let (patched, _) =
        assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();
    assert!(cache.graph_hits() > 0, "the edit should patch");
    let hits_after_patch = cache.graph_hits();
    let (warm, _) = assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();
    assert!(
        cache.graph_hits() > hits_after_patch,
        "third run hits the key"
    );
    assert_eq!(patched, warm);
}

#[test]
fn a_coverage_only_plugin_keeps_the_snapshot_fast_path() {
    // Regression guard for a real shipped bug: the cache/patch bypass was keyed on
    // `plugins.is_empty()`, and the lcov ingester is registered unconditionally by
    // `default_plugins()` — so the graph-snapshot cache and the incremental patch were
    // silently dead on every real `kndo` run from the day the graph hooks were wired.
    // A plugin with `mutates_graph() == false` must be invisible to both fast paths.
    // (A local double stands in for the coverage ingesters, which live in their own
    // plugin crate now — core depends on none of them.)
    struct CoverageOnlyPlugin;
    impl crate::plugin::Plugin for CoverageOnlyPlugin {
        fn descriptor(&self) -> crate::plugin::PluginDescriptor {
            crate::plugin::PluginDescriptor {
                id: smol_str::SmolStr::new("kndo:coverage-test"),
                version: smol_str::SmolStr::new("1"),
                detection: vec![],
                requested_file_access: vec![],
                activation: vec![],
                dependencies: vec![],
            }
        }
        fn mutates_graph(&self) -> bool {
            false
        }
    }
    let dir = project(&[("a.mock", "decl x\nref y"), ("b.mock", "decl y")]);
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = crate::cache::ProjectCache::open(cache_dir.path());
    let plugins: Vec<Box<dyn crate::plugin::Plugin>> = vec![Box::new(CoverageOnlyPlugin)];
    assemble_with_cache(dir.path(), &mock_adapters(), &plugins, Some(&cache)).unwrap();
    let (warm, _) =
        assemble_with_cache(dir.path(), &mock_adapters(), &plugins, Some(&cache)).unwrap();
    assert!(
        cache.graph_hits() > 0,
        "a coverage-only plugin must not bypass the snapshot cache"
    );
    let (cold, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(
        warm, cold,
        "the served snapshot must equal a plugin-less cold build"
    );
}

/// A test plugin exercising every contribution kind the patch must
/// re-derive: a root gated on a content-channel file's *content* (so a stale round is
/// observable), plus an unconditional `annotate_symbols` mark (so the snapshot's
/// `externally_consumed` round-trip is observable too).
struct MarkerGatedPlugin {
    version: &'static str,
}
impl crate::plugin::Plugin for MarkerGatedPlugin {
    fn descriptor(&self) -> crate::plugin::PluginDescriptor {
        crate::plugin::PluginDescriptor {
            id: SmolStr::new("marker-gated"),
            version: SmolStr::new(self.version),
            detection: vec![],
            requested_file_access: vec![SmolStr::new("marker.txt")],
            activation: vec![],
            dependencies: vec![],
        }
    }

    fn mutates_graph(&self) -> bool {
        true
    }

    fn contribute_roots(
        &self,
        graph: &crate::plugin::GraphView<'_>,
        content: &crate::plugin::ContentView<'_>,
        out: &mut crate::plugin::RootSink,
    ) {
        // Unconditional miss: no file declares `ghost_symbol`, so this target never
        // resolves — the contribution-record test asserts it lands in `dropped` (the
        // author kit's debugging record) instead of vanishing without trace.
        out.add(
            crate::plugin::PluginTarget::symbol(
                ProjectPath(SmolStr::new("a.mock")),
                "ghost_symbol",
            ),
            crate::vocab::RootKind::Production,
            Confidence::Probable,
        );
        let marker_on = content
            .read(&ProjectPath(SmolStr::new("marker.txt")))
            .is_some_and(|bytes| bytes == b"on");
        if !marker_on {
            return;
        }
        for file in graph.files() {
            for symbol in graph.symbols_in(&file.path) {
                if symbol.name.as_str() == "root_me" {
                    out.add(
                        crate::plugin::PluginTarget::symbol(file.path.clone(), "root_me"),
                        crate::vocab::RootKind::Production,
                        Confidence::Probable,
                    );
                }
            }
        }
    }

    fn annotate_symbols(
        &self,
        graph: &crate::plugin::GraphView<'_>,
        _content: &crate::plugin::ContentView<'_>,
        out: &mut crate::plugin::AnnotationSink,
    ) {
        for file in graph.files() {
            for symbol in graph.symbols_in(&file.path) {
                if symbol.name.as_str() == "root_me" {
                    out.mark_externally_consumed(file.path.clone(), "root_me");
                }
            }
        }
    }
}

#[test]
fn a_plugin_marked_member_inherits_its_owners_colors() {
    // The kndo:serde shape end-to-end at the core level: a plugin's
    // `mark_implicitly_invoked` (qualified `Owner.name` selector, plus one bogus
    // selector that must drop silently) lands in the graph's plugin partition, and the
    // machinery-dispatch rule then lets the member inherit its owner's colors.
    struct MarkSerialize;
    impl crate::plugin::Plugin for MarkSerialize {
        fn descriptor(&self) -> crate::plugin::PluginDescriptor {
            crate::plugin::PluginDescriptor {
                id: SmolStr::new("kndo:mark-serialize"),
                version: SmolStr::new("1"),
                detection: vec![],
                requested_file_access: vec![],
                activation: vec![],
                dependencies: vec![],
            }
        }
        fn mutates_graph(&self) -> bool {
            true
        }
        fn annotate_symbols(
            &self,
            _graph: &crate::plugin::GraphView<'_>,
            _content: &crate::plugin::ContentView<'_>,
            out: &mut crate::plugin::AnnotationSink,
        ) {
            let path = crate::adapter::ProjectPath(SmolStr::new("a.mock"));
            out.mark_implicitly_invoked(path.clone(), "Glob.serialize");
            out.mark_implicitly_invoked(path, "Ghost.nothing");
        }
    }
    let dir = project(&[(
        "a.mock",
        "decl Glob\nmember-decl Glob serialize\nroot-decl Glob",
    )]);
    let plugins: Vec<Box<dyn crate::plugin::Plugin>> = vec![Box::new(MarkSerialize)];
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &plugins).unwrap();
    let serialize = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name == "serialize")
            .unwrap() as u32,
    );
    assert!(graph.is_plugin_implicitly_invoked(serialize));
    let reach = crate::analysis::reachability::compute(&graph);
    assert_eq!(
        reach.get(NodeRef::Symbol(serialize)),
        (
            crate::analysis::reachability::Reachability::Production,
            Confidence::Probable
        ),
        "the member inherits the rooted owner's color through the machinery rule"
    );
}

#[test]
fn a_plugin_reads_the_trait_a_member_was_declared_under() {
    // What makes a convention plugin a TABLE and nothing else: the adapter recorded which
    // trait's impl declares each member, so the plugin selects by that fact instead of
    // re-parsing the file. The two members share an owner AND a language — only the impl
    // block tells them apart, which is exactly the case that forced source reading before.
    struct TableDriven;
    impl crate::plugin::Plugin for TableDriven {
        fn descriptor(&self) -> crate::plugin::PluginDescriptor {
            crate::plugin::PluginDescriptor {
                id: SmolStr::new("kndo:table-driven"),
                version: SmolStr::new("1"),
                detection: vec![],
                // Nothing to read — the point of the fact.
                requested_file_access: vec![],
                activation: vec![],
                dependencies: vec![],
            }
        }
        fn mutates_graph(&self) -> bool {
            true
        }
        fn annotate_symbols(
            &self,
            graph: &crate::plugin::GraphView<'_>,
            _content: &crate::plugin::ContentView<'_>,
            out: &mut crate::plugin::AnnotationSink,
        ) {
            for file in graph.files() {
                for s in graph.symbols_in(&file.path) {
                    let (Some(owner), Some(t)) = (&s.member_of, &s.implements) else {
                        continue;
                    };
                    if t == "Serialize" && s.name == "serialize" {
                        out.mark_implicitly_invoked(
                            file.path.clone(),
                            format!("{owner}.{name}", name = s.name),
                        );
                    }
                }
            }
        }
    }
    let dir = project(&[(
        "a.mock",
        "decl Glob\n\
             member-impl Serialize Glob serialize\n\
             member-impl Display Glob fmt\n\
             root-decl Glob",
    )]);
    let plugins: Vec<Box<dyn crate::plugin::Plugin>> = vec![Box::new(TableDriven)];
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &plugins).unwrap();
    let id =
        |name: &str| SymbolId(graph.symbols.iter().position(|s| s.name == name).unwrap() as u32);
    assert_eq!(
        graph.symbols[id("serialize").0 as usize]
            .implements
            .as_deref(),
        Some("Serialize"),
        "the fact survives assembly onto the symbol"
    );
    assert!(graph.is_plugin_implicitly_invoked(id("serialize")));
    assert!(
        !graph.is_plugin_implicitly_invoked(id("fmt")),
        "same owner, same file, different impl block — the table must not reach it"
    );
}

fn has_root_me_root(graph: &ProjectGraph) -> bool {
    graph.edges.iter().any(|e| {
        matches!(&e.kind, EdgeKind::Root { target: NodeRef::Symbol(s), .. }
            if graph.symbols[s.0 as usize].name.as_str() == "root_me")
    })
}

/// The fixture behind the three patch/plugin-round tests below: `root_me` exists from the start
/// (so the marker flip is the ONLY change), 20 filler files keep one changed file under
/// the patch's 5% work threshold, and `marker.txt` is unclaimed — its content change is
/// invisible to every adapter guard and only a re-run plugin round can react to it.
fn marker_project() -> tempfile::TempDir {
    let filler: Vec<(String, String)> = (0..20)
        .map(|i| (format!("filler{i}.mock"), format!("decl filler{i}")))
        .collect();
    let mut all: Vec<(&str, &str)> = vec![
        ("a.mock", "decl x\ndecl root_me\nref y"),
        ("b.mock", "decl y"),
        ("marker.txt", "off"),
    ];
    all.extend(filler.iter().map(|(n, c)| (n.as_str(), c.as_str())));
    project(&all)
}

#[test]
fn the_patch_re_derives_plugin_contributions_instead_of_bypassing() {
    // The patch strips every plugin contribution and re-runs the round
    // against the patched graph — proven by flipping a content-channel file the plugin's
    // own gate reads. The flip is invisible to every adapter-side guard (the file is
    // unclaimed), so ONLY a genuinely re-run round can produce the new root.
    let dir = marker_project();
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = crate::cache::ProjectCache::open(cache_dir.path());
    let plugins: Vec<Box<dyn crate::plugin::Plugin>> =
        vec![Box::new(MarkerGatedPlugin { version: "1" })];

    let (cold, _) =
        assemble_with_cache(dir.path(), &mock_adapters(), &plugins, Some(&cache)).unwrap();
    assert!(
        !has_root_me_root(&cold),
        "marker is off — no gated root yet"
    );
    assert!(
        !cold.externally_consumed.is_empty(),
        "the unconditional annotation is present from the cold build"
    );

    fs::write(dir.path().join("marker.txt"), "on").unwrap();
    let (patched, patched_diags) =
        assemble_with_cache(dir.path(), &mock_adapters(), &plugins, Some(&cache)).unwrap();
    assert!(
        cache.graph_hits() > 0,
        "a content flip on an unclaimed file must go through the patch, plugins included"
    );
    assert!(
        has_root_me_root(&patched),
        "the re-run round must see the new marker content — a stale ride-along would not"
    );

    // The equivalence obligation covers plugin-bearing runs: the
    // patched graph — plugin round included — is byte-identical to a scratch rebuild.
    let (scratch, scratch_diags) = assemble(dir.path(), &mock_adapters(), &plugins).unwrap();
    assert_eq!(
        patched, scratch,
        "patched ≡ full rebuild, plugin round included"
    );
    assert_eq!(patched_diags, scratch_diags, "diagnostics too");
}

#[test]
fn a_changed_plugin_set_refuses_the_patch_and_rebuilds() {
    // The plugin-set guard: `classify_file` overrides are baked into
    // `FileNode.class` untagged, so the snapshot's stored plugin-set digest must match —
    // a version bump alone (same id, same hooks) is a different set and full-rebuilds.
    let dir = marker_project();
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = crate::cache::ProjectCache::open(cache_dir.path());

    let v1: Vec<Box<dyn crate::plugin::Plugin>> =
        vec![Box::new(MarkerGatedPlugin { version: "1" })];
    assemble_with_cache(dir.path(), &mock_adapters(), &v1, Some(&cache)).unwrap();

    fs::write(dir.path().join("marker.txt"), "on").unwrap();
    let v2: Vec<Box<dyn crate::plugin::Plugin>> =
        vec![Box::new(MarkerGatedPlugin { version: "2" })];
    let (rebuilt, _) =
        assemble_with_cache(dir.path(), &mock_adapters(), &v2, Some(&cache)).unwrap();
    assert_eq!(
        cache.graph_hits(),
        0,
        "digest mismatch must refuse the patch (and the key already missed)"
    );
    assert!(
        has_root_me_root(&rebuilt),
        "the full rebuild still runs the new set's round"
    );
}

#[test]
fn the_contribution_record_tracks_what_the_round_actually_resolved() {
    // The audit record: the cache's last-run sidecar reflects what each plugin
    // resolved into the graph, and a patch (which re-runs the round) refreshes it — the
    // marker flip changes the recorded root count from 0 to 1.
    let dir = marker_project();
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = crate::cache::ProjectCache::open(cache_dir.path());
    let plugins: Vec<Box<dyn crate::plugin::Plugin>> =
        vec![Box::new(MarkerGatedPlugin { version: "1" })];

    assemble_with_cache(dir.path(), &mock_adapters(), &plugins, Some(&cache)).unwrap();
    let cold_record = cache
        .plugin_contributions()
        .expect("the cold build must leave a contribution record");
    assert_eq!(
        cold_record,
        vec![crate::plugin::PluginContribution {
            id: "marker-gated".to_string(),
            roots: 0,
            edges: 0,
            annotations: 1,
            dropped: vec!["root target `a.mock#ghost_symbol` did not resolve".to_string()],
        }],
        "marker off: only the unconditional annotation resolved, and the unresolvable \
         ghost target is recorded as dropped, not silently lost"
    );

    fs::write(dir.path().join("marker.txt"), "on").unwrap();
    assemble_with_cache(dir.path(), &mock_adapters(), &plugins, Some(&cache)).unwrap();
    assert!(cache.graph_hits() > 0, "the flip goes through the patch");
    let patched_record = cache
        .plugin_contributions()
        .expect("the patch re-runs the round and must refresh the record");
    assert_eq!(
        patched_record[0].roots, 1,
        "marker on: the re-run round's gated root is now in the record: {patched_record:?}"
    );
}

#[test]
fn externally_consumed_round_trips_through_the_snapshot() {
    // Snapshot writes are unconditional even with plugins registered, so the snapshot
    // must persist `externally_consumed` — otherwise every warm hit would silently drop
    // `annotate_symbols` output, losing the exemptions. `ProjectGraph`'s derived
    // `PartialEq` covers the field, so plain equality is the whole assertion.
    let dir = marker_project();
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = crate::cache::ProjectCache::open(cache_dir.path());
    let plugins: Vec<Box<dyn crate::plugin::Plugin>> =
        vec![Box::new(MarkerGatedPlugin { version: "1" })];

    let (cold, _) =
        assemble_with_cache(dir.path(), &mock_adapters(), &plugins, Some(&cache)).unwrap();
    assert!(!cold.externally_consumed.is_empty());
    let (warm, _) =
        assemble_with_cache(dir.path(), &mock_adapters(), &plugins, Some(&cache)).unwrap();
    assert!(
        cache.graph_hits() > 0,
        "second run must be the snapshot hit"
    );
    assert_eq!(
        cold, warm,
        "the warm graph must carry the annotations, not silently drop them"
    );
}

#[test]
fn the_graph_key_folds_the_shared_facts_contract_shape() {
    // A change to a type EVERY adapter emits — `FunctionMetrics` growing a field — used to
    // need a bump in every adapter's `facts_schema_version` for this key to move: the same
    // fact spelled six-plus times, and silently under-invalidating the moment someone bumps
    // five of six. `cache::ENTRY_FORMAT_VERSION` is the one knob for that shape, and folding
    // it here is what makes ONE bump reach the graph snapshot too.
    //
    // Over an EMPTY adapter set the contract term is the only thing `fold_adapter_versions`
    // writes, which is what makes this an exact assertion rather than a "something changed"
    // one: delete the fold and the digest becomes the empty hash.
    let mut folded = blake3::Hasher::new();
    crate::graph::assemble::fold_adapter_versions(&mut folded, &[]);

    let mut expected = blake3::Hasher::new();
    crate::graph::assemble::fold_facts_format(&mut expected, crate::cache::ENTRY_FORMAT_VERSION);

    assert_eq!(
        folded.finalize().as_bytes(),
        expected.finalize().as_bytes(),
        "the adapter-version fold must carry the shared facts-contract shape, so bumping it \
         once reaches the graph snapshot and not only the facts entries"
    );
    // …and the term is a real function of the version, not a constant byte string.
    let mut other = blake3::Hasher::new();
    crate::graph::assemble::fold_facts_format(&mut other, crate::cache::ENTRY_FORMAT_VERSION + 1);
    assert_ne!(expected.finalize().as_bytes(), other.finalize().as_bytes());
}

#[test]
fn compute_graph_key_distinguishes_wasm_plugin_content_from_its_own_id_and_version() {
    // A WASM plugin's declared id+version alone isn't enough — a swapped
    // `.wasm` file with no version bump must still produce a different key. Same
    // descriptor, different `content_hash()`, must fold to different keys.
    struct FakeWasmPlugin(u8);
    impl crate::plugin::Plugin for FakeWasmPlugin {
        fn descriptor(&self) -> crate::plugin::PluginDescriptor {
            crate::plugin::PluginDescriptor {
                id: SmolStr::new("some-wasm-plugin"),
                version: SmolStr::new("1.0.0"),
                detection: vec![],
                requested_file_access: vec![],
                activation: vec![],
                dependencies: vec![],
            }
        }
        fn mutates_graph(&self) -> bool {
            true
        }
        fn content_hash(&self) -> Option<[u8; 32]> {
            Some([self.0; 32])
        }
    }

    let files: [discovery::DiscoveredFile; 0] = [];
    let adapters: Vec<Box<dyn LanguageAdapter>> = vec![];
    let a = FakeWasmPlugin(1);
    let b = FakeWasmPlugin(2);
    let key_a = compute_graph_key(&files, &adapters, &[&a as &dyn crate::plugin::Plugin]);
    let key_b = compute_graph_key(&files, &adapters, &[&b as &dyn crate::plugin::Plugin]);
    assert_ne!(
        key_a, key_b,
        "same id+version, different component bytes, must still be different keys"
    );

    // A compiled-in plugin (content_hash: None) is unaffected — same id+version folds to
    // the same key regardless of anything the trait can't see.
    struct FakeBuiltinPlugin;
    impl crate::plugin::Plugin for FakeBuiltinPlugin {
        fn descriptor(&self) -> crate::plugin::PluginDescriptor {
            crate::plugin::PluginDescriptor {
                id: SmolStr::new("some-builtin-plugin"),
                version: SmolStr::new("1"),
                detection: vec![],
                requested_file_access: vec![],
                activation: vec![],
                dependencies: vec![],
            }
        }
        fn mutates_graph(&self) -> bool {
            true
        }
    }
    let c = FakeBuiltinPlugin;
    let d = FakeBuiltinPlugin;
    assert_eq!(
        compute_graph_key(&files, &adapters, &[&c as &dyn crate::plugin::Plugin]),
        compute_graph_key(&files, &adapters, &[&d as &dyn crate::plugin::Plugin]),
        "two compiled-in plugin instances with identical descriptors must fold identically"
    );
}

#[test]
fn surface_signature_ignores_spans_but_sees_surface_changes() {
    // Bodies and positions move freely under the patch guard; any change to
    // what other files can resolve against must move the signature.
    let base = project(&[("a.mock", "decl x\nref y")]);
    let moved = project(&[("a.mock", "\n\ndecl x\nref y")]);
    let grown = project(&[("a.mock", "decl x\ndecl z\nref y")]);
    let sig = |dir: &std::path::Path| {
        let (g, _) = assemble(dir, &mock_adapters(), &[]).unwrap();
        g.patch_meta[g.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap().0 as usize]
            .surface_sig
            .expect("claimed files carry a signature")
    };
    assert_eq!(
        sig(base.path()),
        sig(moved.path()),
        "span-only movement must not move the signature"
    );
    assert_ne!(
        sig(base.path()),
        sig(grown.path()),
        "a new declaration must move the signature"
    );
}

#[test]
fn patch_meta_records_unit_names_and_reexport_aliases() {
    let dir = project(&[
        ("source.mock", "decl a"),
        ("barrel.mock", "reexport ./source.mock a"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let barrel = graph
        .file_id(&ProjectPath(SmolStr::new("barrel.mock")))
        .unwrap();
    let aliases = &graph.patch_meta[barrel.0 as usize].reexport_aliases;
    assert_eq!(aliases.len(), 1);
    assert_eq!(aliases[0].name.as_str(), "a");
    assert_eq!(
        graph.symbols[aliases[0].symbol.0 as usize].name.as_str(),
        "a"
    );
}

#[test]
fn barrel_chains_resolve_regardless_of_discovery_order() {
    // The fixpoint: `outer` re-exports from `zeta`, which re-exports from the
    // real source — and `outer.mock` sorts BEFORE `zeta.mock`, exactly the discovery
    // order a single-pass one-hop resolution could not handle (outer's lookup would run
    // before zeta's alias exists). The consumer must still reach the one real symbol.
    let dir = project(&[
        ("aaa_source.mock", "decl deep"),
        ("outer.mock", "reexport ./zeta.mock deep"),
        ("zeta.mock", "reexport ./aaa_source.mock deep"),
        ("consumer.mock", "import ./outer.mock deep\nref deep"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(
        graph
            .symbols
            .iter()
            .filter(|s| s.name.as_str() == "deep")
            .count(),
        1
    );
    let deep = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == "deep")
            .unwrap() as u32,
    );
    let consumer = graph
        .file_id(&ProjectPath(SmolStr::new("consumer.mock")))
        .unwrap();
    assert!(
        graph.edges.iter().any(|e| e.kind
            == EdgeKind::References {
                from: NodeRef::File(consumer),
                to: deep,
                kind: crate::vocab::RefKind::Read,
            }),
        "the consumer's reference must resolve through the two-hop chain"
    );
}

#[test]
fn reexport_cycles_terminate_and_resolve_to_nothing() {
    // A cycle of re-exports makes no progress and must simply terminate —
    // no alias ever materializes, nothing hangs, nothing panics.
    let dir = project(&[
        ("ping.mock", "reexport ./pong.mock ghost"),
        ("pong.mock", "reexport ./ping.mock ghost"),
        ("consumer.mock", "import ./ping.mock ghost\nref ghost"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(graph.symbols.iter().all(|s| s.name.as_str() != "ghost"));
}

#[test]
fn assembled_edge_and_diagnostic_order_is_canonical() {
    // Order is data. Assembling the same tree twice — or any two
    // construction paths over identical inputs — must yield identical vectors, which is
    // what the sort guarantees; spot-check that the vector is actually sorted.
    let dir = project(&[
        ("a.mock", "decl x\nref y"),
        ("b.mock", "decl y\nimport ./a.mock x\nref x"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(graph.edges.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn barrel_reexport_from_a_library_root_promotes_the_original_symbol_to_a_production_root() {
    let dir = project(&[
        ("manifest.json", "root barrel.mock"),
        ("source.mock", "decl a"),
        ("barrel.mock", "reexport ./source.mock a"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a_symbol = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name.as_str() == "a")
            .unwrap() as u32,
    );
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::Root {
            kind: RootKind::Production,
            target: NodeRef::Symbol(a_symbol),
        }));
}

#[test]
fn manifest_root_naming_an_unknown_file_is_dropped_not_fabricated() {
    let dir = project(&[("manifest.json", "root nope.mock")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(graph
        .edges
        .iter()
        .all(|e| !matches!(e.kind, EdgeKind::Root { .. })));
}

#[test]
fn manifest_dependencies_reach_the_resolver_as_declared() {
    // With no manifest, `lodash` resolves undeclared (the mock demotes to `probable`).
    let dir = project(&[("a.mock", "import lodash")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edge = graph
        .edges
        .iter()
        .find(|e| matches!(e.kind, EdgeKind::ImportsDependency { .. }))
        .unwrap();
    assert_eq!(edge.confidence, Confidence::Probable);

    // Declared in a manifest, the same import resolves `certain` — proof
    // `declared_dependencies` actually threads from manifest facts into the resolver ctx.
    let dir = project(&[("manifest.json", "dep lodash"), ("a.mock", "import lodash")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let edge = graph
        .edges
        .iter()
        .find(|e| matches!(e.kind, EdgeKind::ImportsDependency { .. }))
        .unwrap();
    assert_eq!(edge.confidence, Confidence::Certain);
}

#[test]
fn inherited_dependencies_resolve_against_the_shared_pool_before_reaching_declared_dependencies() {
    // Root declares the shared pool; a member manifest's `foo` is `inherited` (an
    // unresolved placeholder at extraction time); an unrelated manifest pins the SAME
    // version directly — the two only agree once the placeholder actually resolves.
    let dir = project(&[
        ("manifest.json", "workspace-dep foo 1.2"),
        ("member/manifest.json", "dep-inherited foo"),
        ("external/manifest.json", "dep-version foo 1.2"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let versions: std::collections::BTreeSet<&str> = graph
        .declared_dependencies
        .iter()
        .filter(|d| d.name.as_str() == "foo")
        .filter_map(|d| d.version_req.as_deref())
        .collect();
    assert_eq!(
        versions,
        std::collections::BTreeSet::from(["1.2"]),
        "the inherited dep must resolve to the pool's real version, not the \"workspace\" placeholder"
    );

    // A genuinely differing pin must still show up as a real divergence — resolution
    // only removes the false positive, it never masks a real one.
    let dir = project(&[
        ("manifest.json", "workspace-dep foo 1.2"),
        ("member/manifest.json", "dep-inherited foo"),
        ("external/manifest.json", "dep-version foo 1.3"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let versions: std::collections::BTreeSet<&str> = graph
        .declared_dependencies
        .iter()
        .filter(|d| d.name.as_str() == "foo")
        .filter_map(|d| d.version_req.as_deref())
        .collect();
    assert_eq!(
        versions.len(),
        2,
        "a real version divergence must still be visible after resolution"
    );
}

// ---------------------------------------------------------------- workspace members

#[test]
fn workspace_name_import_produces_both_file_and_dependency_edges() {
    // A workspace-member import: resolution yields the concrete internal file (real reachability) AND
    // the declaration contract stays checkable (an ImportsDependency edge by name).
    let dir = project(&[
        ("packages/a/manifest.json", "name pkg-a\ndep pkg-b"),
        ("packages/a/src.mock", "import pkg-b\nroot-file"),
        (
            "packages/b/manifest.json",
            "name pkg-b\nentry packages/b/lib.mock",
        ),
        ("packages/b/lib.mock", "decl util"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a_src = graph
        .file_id(&ProjectPath(SmolStr::new("packages/a/src.mock")))
        .unwrap();
    let b_lib = graph
        .file_id(&ProjectPath(SmolStr::new("packages/b/lib.mock")))
        .unwrap();
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::ImportsFile {
            from: a_src,
            to: b_lib,
        }));
    assert!(graph
        .dependencies
        .iter()
        .any(|d| d.name.as_str() == "pkg-b"));
    assert!(graph
        .edges
        .iter()
        .any(|e| matches!(e.kind, EdgeKind::ImportsDependency { from, .. } if from == a_src)));
    // And the reachability consequence: b's FILE is alive through the cross-package
    // import even though b is not a root of anything. (Its `util` symbol is still
    // correctly flagged — this fixture's import carries no bindings, nothing references
    // the symbol by name; symbol-level discrimination survives the file being alive.)
    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    assert!(!findings.iter().any(|f| f.subject_kind == "file"
        && f.location.path.as_ref().map(|p| p.0.as_str()) == Some("packages/b/lib.mock")));
}

#[test]
fn same_package_import_gets_the_file_edge_but_no_dependency_contract() {
    // A package's own file naming the package (a test or binary importing its
    // library by package name): `same_package` keeps reachability — the ImportsFile
    // edge — while deriving no ImportsDependency, so neither a phantom `undeclared`
    // ("the package doesn't declare itself") nor dependency-usage credit can appear.
    let dir = project(&[
        (
            "packages/a/manifest.json",
            "name pkg-a\nentry packages/a/lib.mock",
        ),
        ("packages/a/lib.mock", "decl util"),
        ("packages/a/consumer.mock", "import pkg-a\nroot-file"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let consumer = graph
        .file_id(&ProjectPath(SmolStr::new("packages/a/consumer.mock")))
        .unwrap();
    let lib = graph
        .file_id(&ProjectPath(SmolStr::new("packages/a/lib.mock")))
        .unwrap();
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::ImportsFile {
            from: consumer,
            to: lib,
        }));
    assert!(
        !graph.edges.iter().any(
            |e| matches!(e.kind, EdgeKind::ImportsDependency { from, .. } if from == consumer)
        ),
        "no declaration contract exists for a package depending on itself"
    );
    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    assert!(
        !findings.iter().any(|f| f.category == "undeclared"),
        "{findings:?}"
    );
}

#[test]
fn phantom_internal_dependency_is_undeclared() {
    // packages/a imports pkg-b by name WITHOUT declaring it — the table:
    // "import resolves into a sibling package not declared in the importer's manifest →
    // undeclared (phantom internal dependency)".
    let dir = project(&[
        ("packages/a/manifest.json", "name pkg-a"),
        ("packages/a/src.mock", "import pkg-b\nroot-file"),
        (
            "packages/b/manifest.json",
            "name pkg-b\nentry packages/b/lib.mock",
        ),
        ("packages/b/lib.mock", "decl util"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    assert!(findings
        .iter()
        .any(|f| f.category == "undeclared" && f.location.symbol.as_deref() == Some("pkg-b")));
}

#[test]
fn declared_but_unimported_workspace_dep_is_unused() {
    // The other direction of the table: "internal dep declared, no import
    // resolves into that package → unused (subject dependency)".
    let dir = project(&[
        ("packages/a/manifest.json", "name pkg-a\ndep pkg-b"),
        ("packages/a/src.mock", "root-file"),
        (
            "packages/b/manifest.json",
            "name pkg-b\nentry packages/b/lib.mock",
        ),
        ("packages/b/lib.mock", "root-file"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    assert!(findings.iter().any(|f| f.category == "unused"
        && f.subject_kind == "dependency"
        && f.location.symbol.as_deref() == Some("pkg-b")));
}

#[test]
fn workspace_bindings_resolve_to_the_siblings_symbols() {
    // `import { util } from 'pkg-b'` — the binding resolves through b's entry file's
    // symbol table, so `util` is kept alive by a's reference while b's other export
    // is still caught.
    let dir = project(&[
        ("packages/a/manifest.json", "name pkg-a\ndep pkg-b"),
        (
            "packages/a/src.mock",
            "import pkg-b util\nref util\nroot-file",
        ),
        (
            "packages/b/manifest.json",
            "name pkg-b\nentry packages/b/lib.mock",
        ),
        ("packages/b/lib.mock", "decl util\ndecl dead"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    let flagged: Vec<Option<&str>> = findings
        .iter()
        .map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(flagged.contains(&Some("dead")));
    assert!(!flagged.contains(&Some("util")));
}

#[test]
fn import_binding_resolves_through_the_target_s_unit_not_just_its_own_file() {
    // Go's shape: `import "pkg"` resolves to *one* representative file in the target
    // directory (resolution has no multi-file target), but the actually-used
    // symbol may be declared in a *different* file that merely shares the same package
    // (`unit`) — e.g. `resolve()` picks `pkg/x.mock` as the nominal target, but `target` is
    // declared in its sibling `pkg/y.mock`.
    let dir = project(&[
        (
            "a.mock",
            "import ./pkg/x.mock target\nref target\nroot-file",
        ),
        ("pkg/x.mock", "unit pkg"),
        ("pkg/y.mock", "unit pkg\nprivate-decl target"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    let flagged: Vec<Option<&str>> = findings
        .iter()
        .map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(
        !flagged.contains(&Some("target")),
        "the binding should have resolved through pkg's unit table: {findings:?}"
    );
}

#[test]
fn cli_invoke_directive_reaches_script_invoked_dependencies() {
    let dir = project(&[("manifest.json", "dep xo\ncli-invoke xo")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(graph
        .script_invoked_dependencies
        .contains(&(PackageId(1), SmolStr::new("xo"))));
}

#[test]
fn raw_root_whole_file_becomes_root_edge_to_the_file() {
    let dir = project(&[("a.mock", "root-file")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::Root {
            kind: RootKind::Production,
            target: NodeRef::File(a),
        }));
}

#[test]
fn raw_root_declaration_becomes_root_edge_to_the_symbol() {
    let dir = project(&[("a.mock", "decl f\nroot-decl f")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let f = graph.symbols.iter().position(|s| s.name == "f").unwrap() as u32;
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::Root {
            kind: RootKind::Production,
            target: NodeRef::Symbol(SymbolId(f)),
        }));
}

#[test]
fn raw_root_naming_an_unknown_declaration_is_dropped_not_fabricated() {
    let dir = project(&[("a.mock", "root-decl ghost")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(graph
        .edges
        .iter()
        .all(|e| !matches!(e.kind, EdgeKind::Root { .. })));
}

#[test]
fn test_role_files_are_test_roots_not_unused() {
    // "Test roots — test functions/files (language role detection…)". A test
    // file nothing imports is TestOnly, not Unreachable — while a production orphan next
    // to it is still caught.
    let dir = project(&[("a.test.mock", "decl helper"), ("orphan.mock", "decl gone")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let test_file = graph
        .file_id(&ProjectPath(SmolStr::new("a.test.mock")))
        .unwrap();
    let root = graph
        .edges
        .iter()
        .find(|e| {
            e.kind
                == EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(test_file),
                }
        })
        .expect("role-derived test root");
    assert_eq!(root.confidence, Confidence::Probable);

    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    let flagged_paths: Vec<&str> = findings
        .iter()
        .filter_map(|f| f.location.path.as_ref().map(|p| p.0.as_str()))
        .collect();
    assert!(!flagged_paths.contains(&"a.test.mock"));
    assert!(flagged_paths.contains(&"orphan.mock"));
}

#[test]
fn tooling_role_files_root_their_exported_symbols_too() {
    // A config file's exports ARE its interface to the tool that loads it — neither the
    // file nor its exported symbol may be flagged; an unexported dead helper inside the
    // same config still is (symbol-level precision survives the promotion).
    let dir = project(&[(
        "build.config.mock",
        "decl configObject\nprivate-decl helper",
    )]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    assert!(!findings
        .iter()
        .any(|f| f.location.symbol.as_deref() == Some("configObject")));
    assert!(findings
        .iter()
        .any(|f| f.location.symbol.as_deref() == Some("helper")));
}

#[test]
fn opaque_namespace_import_wildcards_over_the_target() {
    // `import * as ns; f(ns)` / `ns[key]` — the namespace escaped static tracking, so
    // every symbol in the target is plausibly used. End-to-end: the
    // target's never-referenced-by-name symbol must stay out of `unused`.
    let dir = project(&[
        ("a.mock", "root-file\nimport-opaque ./b.mock"),
        ("b.mock", "decl viaKey"),
        ("dead.mock", "decl gone"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let b = graph.file_id(&ProjectPath(SmolStr::new("b.mock"))).unwrap();
    let wildcard = graph
        .edges
        .iter()
        .find(|e| e.kind == EdgeKind::Wildcard { from: b })
        .expect("wildcard from the opaquely-consumed target");
    assert_eq!(wildcard.confidence, Confidence::Possible);

    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    assert!(!findings
        .iter()
        .any(|f| f.location.symbol.as_deref() == Some("viaKey")));
    assert!(findings
        .iter()
        .any(|f| f.location.path.as_ref().map(|p| p.0.as_str()) == Some("dead.mock")));
}

#[test]
fn unnarrowed_dynamic_becomes_a_wildcard_edge_from_the_file() {
    let dir = project(&[("a.mock", "dynamic")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
    let wildcard = graph
        .edges
        .iter()
        .find(|e| e.kind == EdgeKind::Wildcard { from: a })
        .expect("wildcard edge");
    assert_eq!(wildcard.confidence, Confidence::Possible);
}

#[test]
fn narrowed_dynamic_imports_the_directorys_files_at_possible() {
    let dir = project(&[
        ("a.mock", "dynamic-narrowed handlers"),
        ("handlers/one.mock", "decl run"),
        ("handlers/sub/two.mock", ""),
        ("handlers/data.json", "{}"), // unclaimed — still a plausible target
        ("elsewhere/other.mock", ""),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
    let id = |p: &str| graph.file_id(&ProjectPath(SmolStr::new(p))).unwrap();
    let imports_from_a: Vec<FileId> = graph
        .edges
        .iter()
        .filter_map(|e| match e.kind {
            EdgeKind::ImportsFile { from, to } if from == a => {
                assert_eq!(e.confidence, Confidence::Possible);
                Some(to)
            }
            _ => None,
        })
        .collect();
    assert!(imports_from_a.contains(&id("handlers/one.mock")));
    assert!(imports_from_a.contains(&id("handlers/sub/two.mock")));
    assert!(imports_from_a.contains(&id("handlers/data.json")));
    assert!(!imports_from_a.contains(&id("elsewhere/other.mock")));
    assert!(!imports_from_a.contains(&a));
    // Each narrowed target also wildcards over its own symbols: a dynamically-imported
    // module is consumed opaquely, so its exports must not stay certain-dead.
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::Wildcard {
            from: id("handlers/one.mock")
        }));
    assert!(!graph
        .edges
        .iter()
        .any(|e| e.kind == EdgeKind::Wildcard { from: a }));
}

#[test]
fn narrowed_dynamic_keeps_target_symbols_possible_alive_end_to_end() {
    // The narrowing promise ("wildcard narrows, nothing false-positive"), at the graph +
    // analysis level: a root file dynamically loading `handlers/` keeps the handler's
    // exported symbol out of `unused`, while a file outside the narrowed scope is still
    // caught.
    let dir = project(&[
        ("a.mock", "root-file\ndynamic-narrowed handlers"),
        ("handlers/one.mock", "decl run"),
        ("dead.mock", "decl gone"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let findings = crate::analysis::run_all(
        &graph,
        &crate::coverage::CoverageMap::default(),
        &crate::analysis::AnalysisTuning::default(),
    )
    .findings;
    let subjects: Vec<(&str, Option<&str>)> = findings
        .iter()
        .map(|f| {
            (
                f.subject_kind.as_str(),
                f.location.path.as_ref().map(|p| p.0.as_str()),
            )
        })
        .collect();
    assert!(subjects.contains(&("file", Some("dead.mock"))));
    assert!(!subjects
        .iter()
        .any(|(_, p)| *p == Some("handlers/one.mock")));
}

#[test]
fn cross_file_reference_resolves_via_import_binding() {
    // a.mock (index 0) references `used`, imported (bound) from b.mock (index 1) — a
    // forward reference in file-discovery order, the case phase 3a/3b split exists for.
    let dir = project(&[
        ("a.mock", "import ./b.mock used\nref used"),
        ("b.mock", "decl used"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
    let used = SymbolId(graph.symbols.iter().position(|s| s.name == "used").unwrap() as u32);
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::References {
            from: NodeRef::File(a),
            to: used,
            kind: crate::vocab::RefKind::Read,
        }));
}

#[test]
fn renamed_binding_resolves_to_the_original_exported_name() {
    // `import { used as alias }` — alias.local != alias.imported.
    let dir = project(&[
        ("a.mock", "import ./b.mock alias=used\nref alias"),
        ("b.mock", "decl used"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let used_symbol = graph.symbols.iter().find(|s| s.name == "used").unwrap();
    assert!(graph
        .edges
        .iter()
        .any(|e| matches!(e.kind, EdgeKind::References { to, .. } if graph.symbols[to.0 as usize].name == used_symbol.name)));
}

#[test]
fn default_binding_resolves_to_the_synthetic_default_export() {
    let dir = project(&[
        ("a.mock", "import ./b.mock main=\nref main"),
        ("b.mock", "decl default"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(graph
        .edges
        .iter()
        .any(|e| matches!(e.kind, EdgeKind::References { .. })));
}

#[test]
fn same_file_reference_resolves_without_an_import() {
    let dir = project(&[("a.mock", "decl helper\nref helper")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let a = graph.file_id(&ProjectPath(SmolStr::new("a.mock"))).unwrap();
    let helper = SymbolId(
        graph
            .symbols
            .iter()
            .position(|s| s.name == "helper")
            .unwrap() as u32,
    );
    assert!(graph.edges.iter().any(|e| e.kind
        == EdgeKind::References {
            from: NodeRef::File(a),
            to: helper,
            kind: crate::vocab::RefKind::Read,
        }));
}

#[test]
fn reference_to_an_unresolvable_name_produces_no_edge() {
    let dir = project(&[("a.mock", "ref ghost")]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert!(graph
        .edges
        .iter()
        .all(|e| !matches!(e.kind, EdgeKind::References { .. })));
}

#[test]
fn assembly_is_deterministic_across_runs() {
    let dir = project(&[
        ("a.mock", "decl x\nimport ./b.mock\nimport lodash"),
        ("b.mock", "decl y"),
        ("c.mock", "decl z\nimport ./a.mock"),
    ]);
    let (g1, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let (g2, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    assert_eq!(g1.edges, g2.edges);
    assert_eq!(
        g1.symbols
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>(),
        g2.symbols
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>()
    );
}

// The correctness gate: `--no-cache` must produce byte-identical findings to a
// cached run. Checked at the graph level — a warm assemble must yield the exact same
// edges and symbols as a cold one on identical input.
#[test]
fn warm_assemble_matches_a_cold_assemble_byte_for_byte() {
    let dir = project(&[
        ("a.mock", "decl x\nimport ./b.mock\nimport lodash"),
        ("b.mock", "decl y"),
        ("c.mock", "decl z\nimport ./a.mock"),
    ]);
    let cold = assemble(dir.path(), &mock_adapters(), &[]).unwrap().0;

    let cache_dir = tempfile::tempdir().unwrap();
    let cache = crate::cache::ProjectCache::open(cache_dir.path());
    // First cached run populates every entry (all misses); second is fully warm.
    assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();
    let warm = assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache))
        .unwrap()
        .0;

    assert_eq!(cold.edges, warm.edges);
    assert_eq!(
        cold.symbols
            .iter()
            .map(|s| (s.name.clone(), s.kind.clone(), s.file))
            .collect::<Vec<_>>(),
        warm.symbols
            .iter()
            .map(|s| (s.name.clone(), s.kind.clone(), s.file))
            .collect::<Vec<_>>()
    );
    assert_eq!(cold.files.len(), warm.files.len());
    assert_eq!(cold.dependencies.len(), warm.dependencies.len());
}

#[test]
fn unchanged_files_are_served_from_the_facts_cache_on_the_second_assemble() {
    let dir = project(&[("a.mock", "decl x"), ("b.mock", "decl y\nimport ./a.mock")]);
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = crate::cache::ProjectCache::open(cache_dir.path());

    assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();
    assert_eq!(cache.hits(), 0); // first run: every file is a miss, then gets stored
    assert_eq!(cache.graph_hits(), 0);

    // Second run: nothing changed, so the *graph* snapshot itself hits (the stronger,
    // whole-assembly skip) before per-file facts are ever consulted — the facts layer
    // stays exactly where the first run left it.
    assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();
    assert_eq!(cache.hits(), 0);
    assert_eq!(cache.graph_hits(), 1);
}

#[test]
fn a_changed_file_misses_the_graph_snapshot_but_still_warms_its_sibling_from_facts() {
    let dir = project(&[("a.mock", "decl x"), ("b.mock", "decl y\nimport ./a.mock")]);
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = crate::cache::ProjectCache::open(cache_dir.path());

    assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();

    // Edit one file — the graph key changes (it folds in the whole file set), so the
    // snapshot must miss; but the *other*, untouched file's facts entry is still valid.
    fs::write(dir.path().join("a.mock"), "decl x2").unwrap();
    assemble_with_cache(dir.path(), &mock_adapters(), &[], Some(&cache)).unwrap();
    assert_eq!(cache.graph_hits(), 0); // never hit — the key never matched after the edit
    assert_eq!(cache.hits(), 1); // b.mock's facts, unchanged, still served from disk
}

/// Regression: a file that declares the same name twice — cfg-alternated `impl` blocks, or
/// platform-gated overloads — must map each declaration's metrics to its OWN symbol.
///
/// The per-file name tables assembly hands the emitter are single-slot/last-wins, so when
/// metrics resolved by NAME both entries landed on whichever declaration was inserted last.
/// `duplicate` then saw two Instances sharing one SymbolId and reported the declaration as a
/// structural clone of itself (both `related` entries pointing at the identical file+range),
/// while health counted the symbol's tokens twice in its duplication numerator. Metrics now
/// resolve by the declaration's own span, which is exact.
#[test]
fn same_named_declarations_keep_their_own_metrics() {
    use crate::adapter::{Declaration, FileFacts, FunctionMetrics, Span};

    let at = |line: u32| Span {
        start: (line, 1),
        end: (line + 2, 1),
    };
    let decl = |span: Span| Declaration {
        name: SmolStr::new("from_path"),
        kind: crate::vocab::SymbolKind::Function,
        span,
        exported: true,
        visibility: VisibilityLevel(1),
        member_of: None,
        signature_span: None,
        implicitly_invoked: false,
        nested_scope: false,
        visibility_inherited: false,
        visible_in_unit: None,
        implements: None,
        markers: Vec::new(),
    };
    let metrics = |span: Span, tokens: u32| FunctionMetrics {
        symbol: SmolStr::new("from_path"),
        span,
        shape_span: span,
        shape_ordinal: 0,
        cyclomatic: 1,
        loc: 3,
        token_count: tokens,
        fingerprints: vec![tokens as u64],
        body_is_construction: false,
    };

    let (first, second) = (at(10), at(40));
    let facts = FileFacts {
        declarations: vec![decl(first), decl(second)],
        functions: vec![metrics(first, 11), metrics(second, 22)],
        ..FileFacts::default()
    };

    let symbols: Vec<SymbolNode> = facts
        .declarations
        .iter()
        .map(|d| SymbolNode {
            file: FileId(0),
            name: d.name.clone(),
            kind: d.kind.clone(),
            span: d.span,
            exported: d.exported,
            visibility: d.visibility,
            member_of: None,
            signature_span: None,
            implicitly_invoked: false,
            nested_scope: false,
            visibility_inherited: false,
            visible_in_unit: None,
            implements: None,
            markers: Vec::new(),
        })
        .collect();

    // Exactly the shape assembly builds: one slot per name, so the twin displaced the first.
    let mut bare_table: HashMap<SmolStr, SymbolId> = HashMap::default();
    bare_table.insert(SmolStr::new("from_path"), SymbolId(1));

    let out = super::assemble::emit_file_declarations(
        0,
        &facts,
        "mock",
        0,
        &symbols,
        &bare_table,
        &HashMap::default(),
        &HashMap::default(),
        None,
    );

    let mut got: Vec<(u32, u32)> = out
        .metrics
        .iter()
        .map(|(id, m)| (id.0, m.token_count))
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![(0, 11), (1, 22)],
        "each declaration keeps its own metrics; \
         resolving by name collapsed both onto SymbolId(1)"
    );
}

/// Regression: a NESTED type's constructor must inherit its container's liveness.
///
/// A constructor is engaged by naming its type, so assembly emits a container → `<init>` edge
/// instead of expecting a reference to bind to `<init>` itself. That lookup went through the
/// file's bare-name table — which holds only declarations with `member_of: None`. A nested type
/// IS a member of its enclosing type (Java's `Utils.ParameterizedTypeImpl`, and the same shape
/// in Kotlin and Swift), so it lives in the qualified table instead and the lookup always
/// missed: the constructor got no incoming edge, and everything only its body reached died
/// with it. In retrofit that killed `Utils.checkNotPrimitive`, called from two nested-class
/// constructors, while the identical top-level shape was fine — the asymmetry this test pins.
#[test]
fn a_nested_types_constructor_inherits_its_containers_liveness() {
    use crate::adapter::{Declaration, FileFacts, Span};

    let at = |line: u32| Span {
        start: (line, 1),
        end: (line + 1, 1),
    };
    let decl =
        |name: &str, kind: crate::vocab::SymbolKind, owner: Option<&str>, span: Span| Declaration {
            name: SmolStr::new(name),
            kind,
            span,
            exported: true,
            visibility: VisibilityLevel(1),
            member_of: owner.map(SmolStr::new),
            signature_span: None,
            implicitly_invoked: false,
            nested_scope: false,
            visibility_inherited: false,
            visible_in_unit: None,
            implements: None,
            markers: Vec::new(),
        };
    use crate::vocab::SymbolKind;
    let facts = FileFacts {
        declarations: vec![
            decl("Outer", SymbolKind::Class, None, at(1)),
            // The nested type: a member of `Outer`, so it never enters the bare table.
            decl("Inner", SymbolKind::Class, Some("Outer"), at(10)),
            decl("<init>", SymbolKind::Constructor, Some("Inner"), at(12)),
            // Control: a top-level type's constructor always worked.
            decl("Free", SymbolKind::Class, None, at(30)),
            decl("<init>", SymbolKind::Constructor, Some("Free"), at(32)),
        ],
        ..FileFacts::default()
    };

    let symbols: Vec<SymbolNode> = facts
        .declarations
        .iter()
        .map(|d| SymbolNode {
            file: FileId(0),
            name: d.name.clone(),
            kind: d.kind.clone(),
            span: d.span,
            exported: d.exported,
            visibility: d.visibility,
            member_of: d.member_of.clone(),
            signature_span: None,
            implicitly_invoked: false,
            nested_scope: false,
            visibility_inherited: false,
            visible_in_unit: None,
            implements: None,
            markers: Vec::new(),
        })
        .collect();

    // Exactly what assembly builds: only `member_of: None` declarations.
    let mut bare_table: HashMap<SmolStr, SymbolId> = HashMap::default();
    bare_table.insert(SmolStr::new("Outer"), SymbolId(0));
    bare_table.insert(SmolStr::new("Free"), SymbolId(3));

    let out = super::assemble::emit_file_declarations(
        0,
        &facts,
        "mock",
        0,
        &symbols,
        &bare_table,
        &HashMap::default(),
        &HashMap::default(),
        None,
    );

    let container_of = |ctor: u32| {
        out.edges.iter().find_map(|e| match e.kind {
            EdgeKind::References { from, to, .. } if to == SymbolId(ctor) => match from {
                NodeRef::Symbol(s) => Some(s.0),
                NodeRef::File(_) => None,
            },
            _ => None,
        })
    };
    assert_eq!(
        container_of(2),
        Some(1),
        "the nested type `Inner` must keep its own constructor alive"
    );
    assert_eq!(container_of(4), Some(3), "top-level control");
}

/// Regression: same-name overloads are twins, and each owns the references in its OWN body.
///
/// `within` is a name, and a name does not distinguish `get(at:)` from `get(path:)` — both
/// declare the selector `Server.get`. The qualified table is single-slot, so every reference
/// in either body attributed to whichever twin landed last; the other had no outgoing
/// references at all and read as dead unless something else named it. vapor's private `get`
/// overload, called from its public sibling one line above, is the shape. The reference's span
/// settles it exactly: it lies inside exactly one of the two bodies.
#[test]
fn same_name_overloads_each_own_the_references_in_their_body() {
    use crate::adapter::{Declaration, FileFacts, RawReference, Span};
    use crate::vocab::{RefKind, SymbolKind};

    let body = |start: u32, end: u32| Span {
        start: (start, 1),
        end: (end, 1),
    };
    let member = |span: Span| Declaration {
        name: SmolStr::new("get"),
        kind: SymbolKind::Method,
        span,
        exported: true,
        visibility: VisibilityLevel(1),
        member_of: Some(SmolStr::new("Server")),
        signature_span: None,
        implicitly_invoked: false,
        nested_scope: false,
        visibility_inherited: false,
        visible_in_unit: None,
        implements: None,
        markers: Vec::new(),
    };

    // Two overloads; the reference sits inside the FIRST one's body.
    let (first, second) = (body(10, 20), body(30, 40));
    let facts = FileFacts {
        declarations: vec![member(first), member(second)],
        references: vec![RawReference {
            name: SmolStr::new("helper"),
            scope_context: None,
            span: Span {
                start: (12, 5),
                end: (12, 20),
            },
            within: Some(SmolStr::new("Server.get")),
            kind: RefKind::Call,
        }],
        ..FileFacts::default()
    };

    let symbols: Vec<SymbolNode> = facts
        .declarations
        .iter()
        .map(|d| SymbolNode {
            file: FileId(0),
            name: d.name.clone(),
            kind: d.kind.clone(),
            span: d.span,
            exported: d.exported,
            visibility: d.visibility,
            member_of: d.member_of.clone(),
            signature_span: None,
            implicitly_invoked: false,
            nested_scope: false,
            visibility_inherited: false,
            visible_in_unit: None,
            implements: None,
            markers: Vec::new(),
        })
        .collect();

    // The single-slot table keeps the LAST twin; the first is displaced into the side map —
    // exactly what `insert_qualified` builds.
    let mut qualified: HashMap<String, SymbolId> = HashMap::default();
    let mut twins: HashMap<String, Vec<SymbolId>> = HashMap::default();
    super::assemble::insert_qualified(
        &mut qualified,
        &mut twins,
        "Server.get".to_string(),
        SymbolId(0),
    );
    super::assemble::insert_qualified(
        &mut qualified,
        &mut twins,
        "Server.get".to_string(),
        SymbolId(1),
    );
    assert_eq!(
        qualified.get("Server.get"),
        Some(&SymbolId(1)),
        "precondition: the naive lookup answers with the second twin"
    );

    let owner = super::assemble::within_owner(
        "Server.get",
        facts.references[0].span,
        &HashMap::default(),
        &qualified,
        &twins,
        &symbols,
    );
    assert_eq!(
        owner,
        Some(SymbolId(0)),
        "the twin whose body contains the reference owns it, not the last-inserted one"
    );
}

/// Regression: two files of one unit declaring the same name — both stay alive.
///
/// Go's `binding.go` (`//go:build !nomsgpack`) and `binding_nomsgpack.go` (`//go:build
/// nomsgpack`) are mutually exclusive and both declare `validate` in package `binding`. The
/// same-unit name table is single-slot, so every reference landed on whichever file was
/// inserted last and its twin read `unused` — in gin one `validate` took all 16 references and
/// the other took none. kndo analyzes the UNION of build configurations by documented policy
/// (`internal/adapters/go.md`), under which both are live.
#[test]
fn same_unit_twins_both_receive_the_reference() {
    let dir = project(&[
        (
            "pkg/a.mock",
            "unit pkg\nprivate-decl validate\nprivate-decl helper",
        ),
        ("pkg/b.mock", "unit pkg\nprivate-decl validate"),
        ("pkg/caller.mock", "unit pkg\nref validate"),
        // Declares `helper` itself, and so does a.mock — the shape where the file that
        // declares one alternate is also where the other's calls live (kotlinx.coroutines
        // writes `expect inline fun yieldThread()` in the very file that calls it, with
        // the `actual`s one file over). Its own declaration first, the unit's twin too.
        (
            "pkg/local.mock",
            "unit pkg\nprivate-decl helper\nref helper",
        ),
        // A name with no twin anywhere: exactly one edge, the containment check that
        // stops the twin machinery from spraying onto ordinary references.
        ("pkg/solo.mock", "unit pkg\nprivate-decl only\nref only"),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();

    let symbol_of = |name: &str, file: &str| -> crate::vocab::SymbolId {
        let (i, _) = graph
            .symbols
            .iter()
            .enumerate()
            .find(|(_, s)| {
                s.name.as_str() == name && graph.files[s.file.0 as usize].path.0.ends_with(file)
            })
            .expect("declaration exists");
        crate::vocab::SymbolId(i as u32)
    };
    let (a, b) = (
        symbol_of("validate", "a.mock"),
        symbol_of("validate", "b.mock"),
    );
    let local = symbol_of("helper", "local.mock");
    let helper_twin = symbol_of("helper", "a.mock");
    let solo = symbol_of("only", "solo.mock");

    let refs_from = |file: &str| -> Vec<crate::vocab::SymbolId> {
        graph
            .edges
            .iter()
            .filter(|e| graph.files[e.owner.0 as usize].path.0.ends_with(file))
            .filter_map(|e| match e.kind {
                EdgeKind::References { to, .. } => Some(to),
                _ => None,
            })
            .collect()
    };

    let mut from_caller = refs_from("caller.mock");
    from_caller.sort_by_key(|s| s.0);
    assert_eq!(
        from_caller,
        {
            let mut want = vec![a, b];
            want.sort_by_key(|s| s.0);
            want
        },
        "every twin receives the reference, not just the single-slot table's winner"
    );

    let mut from_local = refs_from("local.mock");
    from_local.sort_by_key(|s| s.0);
    assert_eq!(
        from_local,
        {
            let mut want = vec![local, helper_twin];
            want.sort_by_key(|s| s.0);
            want
        },
        "its own declaration AND the unit's twin — under the union of configurations only one \
         of the two exists at a time, so each is the target under its own configuration"
    );

    assert_eq!(
        refs_from("solo.mock"),
        vec![solo],
        "a name with no twin resolves to exactly one symbol — the twin set is consulted, \
         never invented"
    );
}

/// A name a wildcard import merely makes VISIBLE sits below members in scope.
///
/// Kotlin resolves an unqualified call against local names, then implicit receivers, and only
/// then imported top-level names — so `updateState(…)` inside a method reaches its own type's
/// member, not a same-named top-level function in a wildcard-imported package. Consulted above
/// the member fallback instead, an unrelated `updateState` in `kotlinx.coroutines.internal`
/// stole both call sites of `StateFlowImpl.updateState` and left it reading `unused`.
#[test]
fn a_member_in_scope_outranks_a_wildcard_visible_name() {
    let dir = project(&[
        // Same directory, different units — the mock resolver only walks `./` siblings,
        // and `unit` is declared, not derived from the path.
        ("src/free.mock", "unit other\nprivate-decl updateState"),
        (
            "src/holder.mock",
            "unit app\nimport-visible ./free.mock\nmember-decl Holder updateState\n\
                 member-decl Holder setValue\nref updateState",
        ),
    ]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();

    let symbol = |name: &str, file: &str| -> crate::vocab::SymbolId {
        let (i, _) = graph
            .symbols
            .iter()
            .enumerate()
            .find(|(_, s)| {
                s.name.as_str() == name && graph.files[s.file.0 as usize].path.0.ends_with(file)
            })
            .expect("declaration exists");
        crate::vocab::SymbolId(i as u32)
    };
    let member = symbol("updateState", "holder.mock");
    let free = symbol("updateState", "free.mock");

    let targets: Vec<crate::vocab::SymbolId> = graph
        .edges
        .iter()
        .filter(|e| {
            graph.files[e.owner.0 as usize]
                .path
                .0
                .ends_with("holder.mock")
        })
        .filter_map(|e| match e.kind {
            EdgeKind::References { to, .. } => Some(to),
            _ => None,
        })
        .collect();

    assert!(
        targets.contains(&member),
        "the enclosing type's own member is the target of an unqualified call"
    );
    assert!(
        !targets.contains(&free),
        "the wildcard-visible top-level name of the same name does not take it"
    );
}

/// `kndo.toml`'s `[[externally-invoked]]`: the project answers the one question static
/// analysis cannot.
///
/// A Spring `@Controller` is instantiated by classpath scanning and called by a servlet
/// dispatcher; a JUnit `@AfterEach` by the runner. The graph is right that nothing references
/// them and the verdict is still a false accusation. The core matches marker STRINGS the
/// project supplied against the ones adapters report, and learns nothing about any framework.
#[test]
fn a_project_declared_marker_makes_a_symbol_an_entry_point() {
    use crate::analysis::reachability;

    let dir = project(&[(
        "src/app.mock",
        "marked-decl Controller show\nmarked-decl Helper hidden\ndecl plain",
    )]);
    let (graph, _) = assemble(dir.path(), &mock_adapters(), &[]).unwrap();
    let id = |name: &str| -> crate::vocab::SymbolId {
        let (i, _) = graph
            .symbols
            .iter()
            .enumerate()
            .find(|(_, s)| s.name.as_str() == name)
            .expect("declaration exists");
        crate::vocab::SymbolId(i as u32)
    };

    let rule = |markers: &[&str], paths: &[&str]| crate::config::ExternallyInvokedRule {
        markers: markers.iter().map(|m| SmolStr::new(*m)).collect(),
        paths: paths
            .iter()
            .map(|p| glob::Pattern::new(p).unwrap())
            .collect(),
    };

    assert_eq!(
        reachability::externally_invoked_symbols(&graph, &[]),
        Vec::new(),
        "no rules, no entry points — the mechanism is inert until the project uses it"
    );
    assert_eq!(
        reachability::externally_invoked_symbols(&graph, &[rule(&["Controller"], &[])]),
        vec![id("show")],
        "only the declaration carrying the configured marker"
    );
    assert_eq!(
        reachability::externally_invoked_symbols(&graph, &[rule(&["Controller"], &["other/**"])]),
        Vec::new(),
        "`paths` scopes the rule; a file outside it is untouched"
    );
    assert_eq!(
        reachability::externally_invoked_symbols(&graph, &[rule(&["Controller"], &["src/**"])]),
        vec![id("show")],
        "and matches when the file is inside it"
    );

    // The point of the whole mechanism: the color changes.
    let before = reachability::compute(&graph);
    assert_eq!(
        before.get(crate::vocab::NodeRef::Symbol(id("show"))).0,
        reachability::Reachability::Unreachable
    );
    let declared = reachability::externally_invoked_symbols(&graph, &[rule(&["Controller"], &[])]);
    let after = reachability::compute_with_roots(&graph, &declared);
    assert_eq!(
        after.get(crate::vocab::NodeRef::Symbol(id("show"))),
        (reachability::Reachability::Production, Confidence::Certain),
        "a project-declared entry point has the standing of a manifest-declared one"
    );
    assert_eq!(
        after.get(crate::vocab::NodeRef::Symbol(id("hidden"))).0,
        reachability::Reachability::Unreachable,
        "an unconfigured marker is inert — this is not a blanket exemption for annotated code"
    );
}
