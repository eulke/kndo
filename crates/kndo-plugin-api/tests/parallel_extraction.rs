//! Proves the WASM adapter bridge's performance-parity contract: `claim`/`extract` run
//! concurrently on a pool of guest instances — the property that lets graph assembly's rayon
//! extraction phase parallelize a WASM adapter's files exactly like a compiled-in adapter's.
//! A serializing bridge (one `Mutex<GuestState>` around every call) would still *pass*
//! the correctness half of this test, so it also asserts the pool observably grew
//! (`instances_created() > 1`), which a serializing bridge cannot produce.
//!
//! Uses the pinned compat component (`tests/compat/adapter-v1.wasm`) — no guest build, and it
//! doubles as proof that a v1 component runs unmodified under the pooled execution model
//! (per-call purity is the contract; the facts cache relies on it).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use kndo_core::adapter::{LanguageAdapter, ProjectPath, SourceFile};
use smol_str::SmolStr;

fn pinned_adapter() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat/adapter-v1.wasm")
}

const FIXTURE: &str = "pub fn main() {\n    helper();\n}\nfn helper() {\n}\nfn dead() {\n}\n";

/// The observable shape of one extraction (`FileFacts` doesn't derive `PartialEq`):
/// declaration names in order, reference names in order, root count, diagnostic count —
/// enough that any cross-instance nondeterminism or trap would show.
fn signature(facts: &kndo_core::adapter::FileFacts) -> (Vec<String>, Vec<String>, usize, usize) {
    (
        facts
            .declarations
            .iter()
            .map(|d| d.name.to_string())
            .collect(),
        facts
            .references
            .iter()
            .map(|r| r.name.to_string())
            .collect(),
        facts.roots.len(),
        facts.diagnostics.len(),
    )
}

#[test]
fn concurrent_extraction_uses_a_grown_pool_and_stays_deterministic() {
    let adapter = Arc::new(
        kndo_plugin_api::WasmAdapter::load(&pinned_adapter()).expect("loading pinned adapter"),
    );

    let path = ProjectPath(SmolStr::new("program.kdemo"));
    let reference = signature(&adapter.extract(&SourceFile {
        path: &path,
        content: FIXTURE.as_bytes(),
    }));
    assert_eq!(
        reference.0,
        vec!["main", "helper", "dead"],
        "the fixture snippet has three declarations"
    );

    // Every thread parks at the barrier before its first extract, so all of them hold a
    // checked-out instance simultaneously — the pool MUST grow past one.
    let threads = 8;
    let barrier = Arc::new(Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let adapter = Arc::clone(&adapter);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let mut results = Vec::new();
                for i in 0..16 {
                    let p = ProjectPath(SmolStr::new(format!("file{i}.kdemo")));
                    assert!(adapter.claim(&p).is_some());
                    results.push(adapter.extract(&SourceFile {
                        path: &p,
                        content: FIXTURE.as_bytes(),
                    }));
                }
                results
            })
        })
        .collect();

    for handle in handles {
        for facts in handle.join().expect("worker thread panicked") {
            assert_eq!(
                signature(&facts),
                reference,
                "every concurrent extraction must equal the serial reference — per-call \
                 purity is the contract the pool relies on"
            );
        }
    }

    assert!(
        adapter.instances_created() > 1,
        "8 barrier-synchronized concurrent calls must force pool growth; a serializing \
         bridge would report exactly 1"
    );
    assert!(
        adapter.instances_created() <= threads + 1,
        "the pool must grow to actual concurrency and no further: {}",
        adapter.instances_created()
    );
}
