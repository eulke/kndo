//! A span of a file written in another language is that language's to read:
//! the host reports it, the engine hands it to the extension claiming the
//! language's suffix, and what it reads lands in the host's evidence at the
//! host's offsets — judged, resolved and addressed as the host file's own.

mod common;

use common::{keeper_kinds, reported};
use kndo::Category;
use kndo_testkit::{MockPlugin, TempProject};

/// The kmock language beside the kdoc documents that embed it.
fn run(project: &TempProject) -> kndo::Snapshot {
    common::analyze(
        project,
        vec![Box::new(MockPlugin::new()), Box::new(MockPlugin::hosting())],
    )
}

fn evidence<'s>(snap: &'s kndo::Snapshot, path: &str) -> &'s kndo_contract::evidence::FileEvidence {
    &snap
        .graph
        .files
        .iter()
        .find(|f| f.path.as_str() == path)
        .expect("the file is in the graph")
        .evidence
}

#[test]
fn a_regions_evidence_is_the_hosts_at_the_hosts_offsets() {
    let p = TempProject::new();
    let page = "prose the document keeps to itself\n\
                <<kmock module\n\
                import ./lib { shared }\n\
                call shared\n\
                fn dead\n\
                >>\n\
                more prose\n";
    p.file("kmock.pkg", "unit site library roots=src\n")
        .file("src/page.kdoc", page)
        .file("src/lib.kmock", "pub fn shared\n");
    let snap = run(&p);

    // The region's declaration is the page's, and its span points into the page.
    let evidence = evidence(&snap, "src/page.kdoc");
    let dead = evidence
        .declarations
        .iter()
        .find(|d| d.name == "dead")
        .expect("the region's declaration lands in the host's evidence");
    assert_eq!(
        &page[dead.span.start as usize..dead.span.end as usize],
        "fn dead"
    );
    assert_eq!(evidence.embedded.len(), 1);
    assert_eq!(
        &page[evidence.embedded[0].span.start as usize..evidence.embedded[0].span.end as usize],
        "import ./lib { shared }\ncall shared\nfn dead\n"
    );

    // The region's import is resolved by kmock, not by the document's language:
    // the edge exists, nothing is unresolved, and `shared` is kept by it.
    assert!(evidence.imports[0].embedded_in.is_some());
    assert!(reported(&snap, &Category::UNRESOLVED).is_empty());
    let keepers = keeper_kinds(&snap, "src/lib.kmock#shared");
    assert!(keepers.contains(&"binding".to_string()), "{keepers:?}");

    // Judged as the page's own: the dead function is reported on the page.
    assert_eq!(reported(&snap, &Category::UNUSED), ["src/page.kdoc — dead"]);
}

#[test]
fn a_region_of_a_language_nothing_claims_is_left_unread_and_said_so() {
    let p = TempProject::new();
    p.file("kmock.pkg", "unit site library roots=src\n")
        .file("src/page.kdoc", "<<mystery module\nwhatever this is\n>>\n");
    let snap = run(&p);
    let evidence = evidence(&snap, "src/page.kdoc");
    assert_eq!(evidence.embedded.len(), 1, "the region is still reported");
    assert!(evidence.declarations.is_empty() && evidence.imports.is_empty());
    assert!(
        evidence
            .diagnostics
            .iter()
            .any(|d| d.message.contains("`mystery`")),
        "{:?}",
        evidence.diagnostics
    );
}

#[test]
fn the_hosts_declared_streams_bound_what_a_region_may_say() {
    // kmock declares comments; a kdoc document declares none. A comment in the
    // region is dropped without a word: the host's declaration is the file's.
    let p = TempProject::new();
    p.file("kmock.pkg", "unit site library roots=src\n").file(
        "src/page.kdoc",
        "<<kmock module\n# a comment the document never declared\npub fn shown\n>>\n",
    );
    let snap = run(&p);
    let evidence = evidence(&snap, "src/page.kdoc");
    assert!(evidence.comments.is_empty());
    assert!(
        evidence.diagnostics.is_empty(),
        "{:?}",
        evidence.diagnostics
    );
    assert_eq!(evidence.declarations.len(), 1);
}
