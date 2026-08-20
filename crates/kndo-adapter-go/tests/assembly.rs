//! End-to-end graph-assembly regressions for the two real bugs multi-file Go dogfooding found
//! (docs/adapters/go.md, RFC 0011 §4, RFC 0005 §1) — both fixed at the core level, exercised
//! here through the real adapter rather than a synthetic mock, since the bugs only manifest
//! through the specific edge shapes Go's resolver and root promotion actually produce.

use kndo_adapter_go::GoAdapter;
use kndo_core::{analysis, graph};
use std::fs;

/// Each caller passes a distinct name: tests run in parallel, and a shared directory races
/// one test's `remove_dir_all` against another's assemble (observed as a real flake).
fn multi_file_module(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kndo-go-assembly-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(
        dir.join("go.mod"),
        "module example.com/demo\n\ngo 1.22\n\nrequire golang.org/x/text v0.14.0\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.go"),
        "package main\n\nimport (\n\t\"fmt\"\n\n\t\"example.com/demo/sub\"\n)\n\nfunc main() {\n\tfmt.Println(sub.Greeting())\n}\n",
    )
    .unwrap();
    // Split across two files on purpose: `Greeting` (greet.go) calls `format` (format.go) with
    // no import — ordinary same-package, cross-file Go, the `FileFacts::unit` mechanism's own
    // reason for existing.
    fs::write(
        dir.join("sub/greet.go"),
        "package sub\n\nfunc Greeting() string {\n\treturn format(\"hello\")\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("sub/format.go"),
        "package sub\n\nfunc format(s string) string {\n\treturn s + \"!\"\n}\n\nfunc unusedHelper() string {\n\treturn \"dead\"\n}\n",
    )
    .unwrap();
    dir
}

#[test]
fn a_root_that_is_only_a_symbol_does_not_strand_its_file_or_its_callees() {
    // Regression: `func main()` in `main.go` is a root (a *symbol*-targeted one — Go has no
    // manifest-level "entry file" the way JS does, docs/adapters/go.md §2). Before the
    // reachability.rs fix, reaching only the `main` symbol never visited `main.go` as a file
    // node, so `main.go`'s own (file-attributed) reference to `sub.Greeting` never propagated —
    // and by the same mechanism, `Greeting`'s file-attributed call to the same-package sibling
    // `format` never propagated either. Both `main.go` and `sub/format.go` read as fully
    // unreachable despite genuinely being used.
    let dir = multi_file_module("symbol-root");
    let (g, diagnostics) = graph::assemble(&dir, &[Box::new(GoAdapter)]).unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let (findings, _) = analysis::run_all(&g);

    let unused_paths: Vec<&str> = findings
        .iter()
        .filter(|f| f.category == "unused" && f.subject_kind == "file")
        .filter_map(|f| f.location.path.as_ref().map(|p| p.0.as_str()))
        .collect();
    assert!(
        !unused_paths.contains(&"main.go"),
        "main.go owns the `main` root and must not read as unused: {unused_paths:?}"
    );
    assert!(
        !unused_paths.contains(&"sub/format.go"),
        "sub/format.go declares `format`, called (same-package, no import) from sub/greet.go: {unused_paths:?}"
    );
    assert!(
        !unused_paths.contains(&"sub/greet.go"),
        "sub/greet.go declares the promoted-root `Greeting`: {unused_paths:?}"
    );
    // `unusedHelper` (also in format.go) is genuinely dead — the fix must not paper over that.
    assert!(findings
        .iter()
        .any(|f| f.category == "unused" && f.location.symbol.as_deref() == Some("unusedHelper")));
}

#[test]
fn importing_the_module_s_own_subpackage_is_never_a_phantom_dependency() {
    // Regression: resolving a same-module subpackage import as `Resolution::WorkspaceMember`
    // (JS's shape for a workspace sibling) also creates an `ImportsDependency` edge toward a
    // dependency named after the *module itself* — which `go.mod` never declares (a module
    // cannot `require` itself), so it read as `undeclared` ("phantom dependency"). Go has no
    // per-sibling declaration concept at all for its own subpackages; fixed by resolving to a
    // plain `Resolution::File` instead (docs/adapters/go.md, resolution.rs's `resolve_into_package`).
    let dir = multi_file_module("own-subpackage");
    let (g, diagnostics) = graph::assemble(&dir, &[Box::new(GoAdapter)]).unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let (findings, _) = analysis::run_all(&g);

    assert!(
        !findings.iter().any(|f| f.category == "undeclared"),
        "a module importing its own subpackage must never read as a phantom dependency: {findings:?}"
    );
    // The declared-but-genuinely-unimported dependency must still fire — the fix must not
    // silence `undeclared`'s sibling verdict along with it.
    assert!(findings.iter().any(|f| f.category == "unused"
        && f.subject_kind == "dependency"
        && f.location.symbol.as_deref() == Some("golang.org/x/text")));
}

#[test]
fn unexported_method_called_through_a_variable_is_not_falsely_unused() {
    // The RFC 0012 §3 bug: methods declare as members (`helper` member_of `T`), but a call
    // `t.helper()` is a bare `helper` reference — name-exact resolution can never connect
    // them (no receiver types in extraction). The duck-typed fallback must keep the method
    // alive at Probable; before it, this exact shape false-positived as unused:method — the
    // one failure mode kndo promises not to have.
    let dir = std::env::temp_dir().join("kndo-go-assembly-method-call");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("go.mod"), "module example.com/m\n\ngo 1.22\n").unwrap();
    fs::write(
        dir.join("main.go"),
        "package main\n\ntype T struct{}\n\nfunc (t T) helper() int { return 1 }\n\nfunc (t T) orphan() int { return 2 }\n\nfunc main() {\n\tt := T{}\n\t_ = t.helper()\n}\n",
    )
    .unwrap();

    let (g, diagnostics) = graph::assemble(&dir, &[Box::new(GoAdapter)]).unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let (findings, _) = analysis::run_all(&g);

    let unused_symbols: Vec<&str> = findings
        .iter()
        .filter(|f| f.category == "unused")
        .filter_map(|f| f.location.symbol.as_deref())
        .collect();
    assert!(
        !unused_symbols.contains(&"T.helper"),
        "called method must stay alive via the member fallback: {unused_symbols:?}"
    );
    // ...while a genuinely uncalled unexported method still dies, qualified in the finding.
    assert!(
        unused_symbols.contains(&"T.orphan"),
        "uncalled method must still be found: {unused_symbols:?}"
    );
}

#[test]
fn a_dead_function_s_callees_die_with_it() {
    // RFC 0012 §4's whole point, end to end in real Go: references are attributed to the
    // symbol they execute inside, so dead `z`'s call no longer keeps `b` alive through the
    // (live) file's blanket attribution — transitive death is visible.
    let dir = std::env::temp_dir().join("kndo-go-assembly-transitive-dead");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("go.mod"), "module example.com/m\n\ngo 1.22\n").unwrap();
    fs::write(
        dir.join("main.go"),
        "package main\n\nfunc a() int { return 1 }\n\nfunc b() int { return 2 }\n\nfunc z() int { return b() }\n\nfunc main() {\n\t_ = a()\n}\n",
    )
    .unwrap();

    let (g, diagnostics) = graph::assemble(&dir, &[Box::new(GoAdapter)]).unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let (findings, _) = analysis::run_all(&g);
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
}
