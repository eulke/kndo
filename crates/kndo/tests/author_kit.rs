//! The author kit's whole promised loop (RFC 0017 §7, second pass), end to end and held to
//! the workspace's own bar: `kndo::author::scaffold` must produce a crate that COMPILES
//! against the vendored WIT, `kndo::author::build` must turn it into a loadable component
//! with one call, and `kndo::verify` must accept the result — for both kinds. A template
//! that drifts from the ABI breaks this test, not a third-party author's afternoon.

use std::path::Path;

fn scaffold_build_verify(
    kind: kndo::author::ComponentKind,
    dir_name: &str,
) -> kndo::verify::VerifyReport {
    let root = tempfile::tempdir().expect("temp scaffold root");
    let crate_dir = root.path().join(dir_name);

    let created = kndo::author::scaffold(&crate_dir, kind).expect("scaffold");
    for expected in ["Cargo.toml", "src/lib.rs", "README.md", ".gitignore"] {
        assert!(
            created.iter().any(|c| c == expected),
            "scaffold must create {expected}: {created:?}"
        );
    }
    // The vendored WIT is the host's own — byte-identical, not a stale copy.
    let wit_rel = created
        .iter()
        .find(|c| c.starts_with("wit/"))
        .expect("scaffold must vendor the WIT");
    assert_eq!(
        std::fs::read_to_string(crate_dir.join(wit_rel)).unwrap(),
        kind.wit()
    );

    // A second scaffold into the same directory must refuse, not clobber.
    assert!(kndo::author::scaffold(&crate_dir, kind).is_err());

    let artifact = kndo::author::build(&crate_dir).expect("kndo plugin build");
    assert!(artifact.is_file());

    kndo::verify::verify(&artifact).expect("the built component must pass verify")
}

#[test]
fn a_scaffolded_plugin_builds_and_verifies() {
    let report = scaffold_build_verify(kndo::author::ComponentKind::Plugin, "my-demo-plugin");
    assert_eq!(report.kind, kndo::verify::VerifiedKind::Plugin);
    assert_eq!(report.id, "github.com/you/my-demo-plugin");
}

#[test]
fn a_scaffolded_adapter_builds_and_verifies() {
    let report = scaffold_build_verify(kndo::author::ComponentKind::Adapter, "my-demo-adapter");
    assert_eq!(report.kind, kndo::verify::VerifiedKind::Adapter);
    assert_eq!(report.id, "my-demo-adapter");
    // The synthesized sample derived from the template's `**/*.<name>` glob must have been
    // claimed — proving the scaffold's claim/extract pair actually runs, not just loads.
    assert!(
        report
            .fixture
            .iter()
            .any(|l| l.contains("adapter my-demo-adapter claimed 1 file(s)")),
        "{:?}",
        report.fixture
    );
}

#[test]
fn verify_in_project_runs_against_the_authors_own_fixture() {
    // Reuses the plugin scaffold as the component under test; the "author fixture" is a tiny
    // project containing a marker file the generic fixture would never have. The drive must
    // see the author's tree (the file count in the check proves the copy happened) and must
    // not mutate the source directory.
    let root = tempfile::tempdir().expect("temp scaffold root");
    let crate_dir = root.path().join("proj-fixture-plugin");
    kndo::author::scaffold(&crate_dir, kndo::author::ComponentKind::Plugin).expect("scaffold");
    let artifact = kndo::author::build(&crate_dir).expect("build");

    let fixture = tempfile::tempdir().expect("temp author fixture");
    std::fs::write(fixture.path().join("package.json"), "{\"name\":\"fx\"}\n").unwrap();
    std::fs::write(fixture.path().join("routes-marker.txt"), "hello\n").unwrap();

    let report =
        kndo::verify::verify_in_project(&artifact, fixture.path()).expect("verify --project");
    assert_eq!(report.kind, kndo::verify::VerifiedKind::Plugin);
    assert!(
        !fixture.path().join(".kndo").exists(),
        "the author's fixture directory must never be mutated"
    );
    // And pointing at a non-directory is a plain error, not a panic.
    assert!(kndo::verify::verify_in_project(&artifact, Path::new("/nonexistent-dir")).is_err());
}
