//! Extraction against inline sources: capitalization reach, entry and test roots,
//! the never-declared method class, every import spelling, the synthetic package
//! edge, library-mode and generated-file rules, reference exclusions, and comment
//! spans.

use kndo_adapter_go::GoAdapter;
use kndo_contract::evidence::{
    FileEvidence, ImportShape, ImportTarget, Reach, RootKind, RootTarget, SymbolKind,
};

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
    assert_eq!(decl(&ev, "private").reach, Reach::Private);
    assert_eq!(decl(&ev, "Config").kind, SymbolKind::Type);
    assert_eq!(decl(&ev, "Config").reach, Reach::Exported);
    assert_eq!(decl(&ev, "secret").reach, Reach::Private);
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
        "package pkg\n\nfunc TestA(t *testing.T) {}\n",
    );
    assert!(
        test.roots
            .iter()
            .any(|r| r.kind == RootKind::Test && matches!(r.target, RootTarget::WholeFile))
    );
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
fn imports_in_every_spelling_plus_the_package_edge() {
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
    // The synthetic package edge ties siblings together.
    assert!(matches!(
        &import(&ev, ".").shape,
        ImportShape::Bindings(b) if b.is_empty()
    ));
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
fn library_mode_internal_fence_and_generated_files() {
    // A non-internal library package is importable by other modules: published
    // surface, whole-file Production root.
    let lib = extract("pkg/a.go", "package pkg\n\nfunc Public() {}\n");
    assert!(
        lib.roots
            .iter()
            .any(|r| r.kind == RootKind::Production && matches!(r.target, RootTarget::WholeFile))
    );
    // `internal/` is the language's own fence — no root.
    let internal = extract("internal/util/h.go", "package util\n\nfunc Helper() {}\n");
    assert!(internal.roots.is_empty());
    // Generated code declares nothing accusable; its imports and references stay.
    let generated = extract(
        "pkg/api.pb.go",
        "// Code generated by protoc-gen-go. DO NOT EDIT.\n\npackage pkg\n\nimport \"fmt\"\n\nfunc dead() { fmt.Println(used) }\n",
    );
    assert!(generated.declarations.is_empty());
    assert!(generated.references.iter().any(|r| r.name == "used"));
    assert!(
        generated
            .imports
            .iter()
            .any(|i| matches!(&i.target, ImportTarget::Package(p) if p == "fmt"))
    );
}
