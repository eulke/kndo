//! Extraction against inline sources: the namespace the package clause names,
//! capitalization reach, entry and test roots, the never-declared method class,
//! every import spelling, the internal fence and the generated banner,
//! reference exclusions, and comment spans.

use kndo_adapter_go::GoAdapter;
use kndo_contract::evidence::{
    FileEvidence, ImportShape, ImportTarget, MarkerTarget, Reach, RootKind, RootTarget, SymbolKind,
};
use kndo_contract::extension::Extension;

fn extract(path: &str, source: &str) -> FileEvidence {
    kndo_testkit::extract_evidence(&GoAdapter::new(), path, source)
}

fn decl<'e>(ev: &'e FileEvidence, name: &str) -> &'e kndo_contract::evidence::Declaration {
    kndo_testkit::declaration_named(ev, name)
}

fn import<'e>(ev: &'e FileEvidence, specifier: &str) -> &'e kndo_contract::evidence::Import {
    kndo_testkit::import_named(ev, specifier)
}

#[test]
fn capitalization_is_reach() {
    let ev = extract(
        "pkg/server.go",
        r#"
package pkg

func Public() {}
func private() {}

type Config struct{}
type secret struct{}

const MaxRetries = 3
var counter = 0
"#,
    );
    assert_eq!(decl(&ev, "Public").reach, Reach::Exported);
    assert_eq!(decl(&ev, "private").reach, Reach::Namespace { up: 0 });
    assert_eq!(decl(&ev, "Config").kind, SymbolKind::Type);
    assert_eq!(decl(&ev, "Config").reach, Reach::Exported);
    assert_eq!(decl(&ev, "secret").reach, Reach::Namespace { up: 0 });
    assert_eq!(decl(&ev, "MaxRetries").kind, SymbolKind::Constant);
    assert_eq!(decl(&ev, "counter").kind, SymbolKind::Variable);
}

#[test]
fn entry_and_test_roots() {
    let main = extract(
        "cmd/app/main.go",
        "package main\n\nfunc main() {}\nfunc init() {}\nfunc helper() {}\n",
    );
    let rooted = |ev: &FileEvidence, name: &str| {
        let ix = ev.declarations.iter().position(|d| d.name == name).unwrap();
        ev.roots
            .iter()
            .any(|r| matches!(r.target, RootTarget::Declaration(id) if id.index() == ix))
    };
    assert!(rooted(&main, "main"));
    assert!(rooted(&main, "init"));
    assert!(!rooted(&main, "helper"));

    // `func main` outside `package main` is just a function.
    let lib = extract("pkg/a.go", "package pkg\n\nfunc main() {}\n");
    assert!(!rooted(&lib, "main"));

    let test = extract(
        "pkg/a_test.go",
        "package pkg\n\nfunc TestA(t *testing.T) {}\nfunc init() {}\n",
    );
    // WHAT a `_test.go` file is, the spec declares as a file role and the
    // engine anchors where no unit said otherwise — extraction no longer
    // concludes it from the path.
    assert!(
        !test
            .roots
            .iter()
            .any(|r| matches!(r.target, RootTarget::WholeFile)),
        "the path is not this pass's to read: {:?}",
        test.roots
    );
    let declared = GoAdapter::new().spec().file_roles().to_vec();
    assert_eq!(declared.len(), 1);
    assert_eq!(declared[0].glob, "**/*_test.go");
    assert_eq!(declared[0].kind, RootKind::Test);
    assert_eq!(
        declared[0].confidence,
        kndo_contract::vocab::Confidence::Certain
    );
    // An `init` runs when the binary it is compiled into loads, and a
    // `_test.go` file is compiled into the test binary alone: rooting it
    // Production would flood the package's production color from its tests.
    let init_ix = test
        .declarations
        .iter()
        .position(|d| d.name == "init")
        .unwrap();
    let init_roots: Vec<RootKind> = test
        .roots
        .iter()
        .filter(|r| matches!(r.target, RootTarget::Declaration(id) if id.index() == init_ix))
        .map(|r| r.kind)
        .collect();
    assert_eq!(init_roots, [RootKind::Test]);
}

#[test]
fn methods_are_not_declared() {
    // Go's interfaces are structural: any method may satisfy one and run without
    // its name appearing anywhere — so no method is ever an accusable declaration.
    let ev = extract(
        "pkg/server.go",
        "package pkg\n\ntype Server struct{}\n\nfunc (s *Server) Start() {}\nfunc (s Server) stop() {}\n",
    );
    assert!(!ev.declarations.iter().any(|d| d.name == "Start"));
    assert!(!ev.declarations.iter().any(|d| d.name == "stop"));
    assert_eq!(decl(&ev, "Server").kind, SymbolKind::Type);
}

#[test]
fn imports_in_every_spelling() {
    let ev = extract(
        "pkg/a.go",
        r#"
package pkg

import (
	"fmt"
	renamed "example.com/mod/deep/path"
	_ "example.com/mod/effects"
	. "example.com/mod/dsl"
)
"#,
    );
    // Evidence is faithful to the source: exactly the written imports, nothing
    // synthetic — which files co-compile lives in `sees`.
    assert_eq!(ev.imports.len(), 4);
    assert!(matches!(
        &import(&ev, "fmt").shape,
        ImportShape::Namespace { local } if local == "fmt"
    ));
    assert!(matches!(
        &import(&ev, "example.com/mod/deep/path").shape,
        ImportShape::Namespace { local } if local == "renamed"
    ));
    assert!(matches!(
        &import(&ev, "example.com/mod/effects").shape,
        ImportShape::SideEffect
    ));
    assert!(matches!(
        &import(&ev, "example.com/mod/dsl").shape,
        ImportShape::Glob
    ));
}

#[test]
fn references_and_comments() {
    let source = r#"
package pkg

// kndo:allow unused
func compute(input int) int {
	doubled := input * 2
	return helper(doubled)
}

func helper(n int) int { return n }

func caller() { s.Refresh() }
"#;
    let ev = extract("pkg/a.go", source);
    // Declaration and parameter names are not uses; reads and calls are.
    assert!(!ev.references.iter().any(|r| r.name == "compute"));
    assert!(ev.references.iter().any(|r| r.name == "input"));
    assert!(ev.references.iter().any(|r| r.name == "helper"));
    assert!(ev.references.iter().any(|r| r.name == "Refresh"));
    let texts: Vec<&str> = ev
        .comments
        .iter()
        .map(|c| &source[c.text.start as usize..c.text.end as usize])
        .collect();
    assert_eq!(texts, [" kndo:allow unused"]);
}

#[test]
fn metrics_fingerprint_structural_clones() {
    let ev = extract(
        "pkg/m.go",
        r#"
package pkg

func alpha(items []int) int {
	total := 0
	for _, item := range items {
		if item > 10 {
			total += item
		}
	}
	return total
}

func beta(values []int) int {
	sum := 0
	for _, value := range values {
		if value > 99 {
			sum += value
		}
	}
	return sum
}
"#,
    );
    let m = |name: &str| {
        let ix = ev.declarations.iter().position(|d| d.name == name).unwrap();
        ev.metrics
            .iter()
            .find(|(id, _)| id.index() == ix)
            .map(|(_, m)| m)
            .unwrap_or_else(|| panic!("{name} has metrics"))
    };
    assert_eq!(m("alpha").cyclomatic, 3, "for + if");
    assert_eq!(
        m("alpha").fingerprints,
        m("beta").fingerprints,
        "renamed identifiers and changed numbers fingerprint identically"
    );
}

#[test]
fn the_package_clause_and_the_directory_name_the_namespace() {
    // The directory addresses the package, the clause names it: the pair is
    // unique by construction, and the repetition of a conventional name is the
    // price of never merging `a/b` + `package b` with `a` + `package b`.
    let lib = extract("pkg/sub/a.go", "package sub\n\nfunc Public() {}\n");
    assert_eq!(lib.namespace, ["pkg", "sub", "sub"]);
    // A file no import reaches is still no root: what keeps a package alive is
    // its exported surface and its importers, never its own existence.
    assert!(lib.roots.is_empty());
    // The directory says WHICH `util` this is, so two of them never pool.
    let one = extract("a/util/h.go", "package util\n\nfunc h() {}\n");
    let two = extract("b/util/h.go", "package util\n\nfunc h() {}\n");
    assert_ne!(one.namespace, two.namespace);
    // The external test package of a directory is a namespace of its own: it
    // may name only what the package exports.
    let external = extract("pkg/sub/a_test.go", "package sub_test\n");
    assert_eq!(external.namespace, ["pkg", "sub", "sub_test"]);
    // A file at the root declares the clause alone.
    assert_eq!(extract("m.go", "package main\n").namespace, ["main"]);
}

#[test]
fn the_internal_fence_and_generated_files() {
    // `internal/` is the language's own fence: an exported name there reaches
    // the tree above the fence and no further.
    let internal = extract("internal/util/h.go", "package util\n\nfunc Helper() {}\n");
    assert_eq!(decl(&internal, "Helper").reach, Reach::Directory { up: 2 });
    // The toolchain's banner is REPORTED, anchored at both ends; what it means
    // is the engine's. Everything else the file says is said in full.
    let generated = extract(
        "pkg/api.pb.go",
        "// Code generated by protoc-gen-go. DO NOT EDIT.\n\npackage pkg\n\nimport \"fmt\"\n\nfunc dead() { fmt.Println(used) }\n",
    );
    assert!(generated.roots.is_empty());
    assert_eq!(generated.markers.len(), 1);
    assert_eq!(generated.markers[0].on, MarkerTarget::File);
    assert_eq!(generated.markers[0].path, "generated");
    assert_eq!(
        generated.markers[0].args,
        ["// Code generated by protoc-gen-go. DO NOT EDIT."]
    );
    assert_eq!(decl(&generated, "dead").reach, Reach::Namespace { up: 0 });
    assert!(generated.references.iter().any(|r| r.name == "used"));
    assert!(
        generated
            .imports
            .iter()
            .any(|i| matches!(&i.target, ImportTarget::Package(p) if p == "fmt"))
    );
    // A comment that merely mentions the words is not the banner.
    let prose = extract(
        "pkg/note.go",
        "// This file was Code generated once. DO NOT EDIT lightly.\n\npackage pkg\n",
    );
    assert!(prose.markers.is_empty());
}

#[test]
fn the_grammars_fields_say_what_they_say() {
    let ev = extract(
        "gram/gram.go",
        r#"
package gram

var (
	grouped  = 1
	Exported = 2
)

const one, two = 3, 4

var a, b int

func gram() {}

type owner struct{}

func (o owner) Method() {}

func (o *owner) Pointer() {}

func use() {
	x := grouped
	for k, v := range []int{} {
		_ = k
		_ = v
	}
	_ = x
}
"#,
    );
    let names: Vec<&str> = ev.declarations.iter().map(|d| d.name.as_str()).collect();
    // A grouped `var` block hides its specs under a `var_spec_list`.
    assert!(
        names.contains(&"grouped") && names.contains(&"Exported"),
        "{names:?}"
    );
    // A multi-name spec declares every name and no separator: the grammar
    // labels the commas of a `const` with the `name` field too.
    assert!(
        names.contains(&"one") && names.contains(&"two"),
        "{names:?}"
    );
    assert!(names.contains(&"a") && names.contains(&"b"), "{names:?}");
    assert!(
        !names.contains(&","),
        "a comma is not a declaration: {names:?}"
    );

    let refs: Vec<&str> = ev.references.iter().map(|r| r.name.as_str()).collect();
    // The package clause names the namespace, not a declaration.
    assert!(
        !refs.contains(&"gram"),
        "the package clause is not a use of the function that shares its name: {refs:?}"
    );
    // A receiver's type is part of the type's own definition — Go requires it
    // to be declared in this package.
    assert!(
        !refs.contains(&"owner"),
        "a method receiver is not a use of its type: {refs:?}"
    );
    // A `:=` binds on the left and reads on the right: `x := grouped` gives
    // one reference to `grouped` and none to `x`, and the later `_ = x` is the
    // only reference `x` has.
    assert_eq!(
        refs.iter().filter(|r| **r == "x").count(),
        1,
        "the binding is not a use, the read below it is: {refs:?}"
    );
    assert!(refs.contains(&"grouped"), "the right side reads: {refs:?}");
    // Same for a range clause's own names: one reference each, from the body.
    for bound in ["k", "v"] {
        assert_eq!(
            refs.iter().filter(|r| **r == bound).count(),
            1,
            "a range clause binds {bound} rather than naming it: {refs:?}"
        );
    }
}
