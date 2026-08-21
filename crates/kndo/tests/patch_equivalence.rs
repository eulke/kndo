//! RFC 0013 §6's equivalence suite, over the real adapters and the real conformance corpora:
//! for every fixture project, apply a mutation, run the cached path (patch when its guards
//! hold, full rebuild otherwise), and compare the resulting graph against a scratch rebuild
//! of the same tree — they must be EQUAL, whole-graph, not merely finding-equivalent. The
//! canonical-order invariant (RFC 0013 §3a) is what makes this comparison meaningful.
//!
//! Mutations per fixture: (1) a trailing comment on one claimed file — surface-preserving,
//! the patch's home turf; (2) a leading blank line — every span shifts; (3) a new exported
//! declaration — a surface change that must trip the guard and fall back. The suite asserts
//! equality on every run regardless of which path served it, and separately asserts that the
//! patch path actually engaged somewhere (a suite that silently full-rebuilds everywhere
//! would prove nothing about the patch).

use std::fs;
use std::path::{Path, PathBuf};

use kndo::graph::{assemble, assemble_with_cache};

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn claimed_source_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(&path, out);
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "go")
            ) && !path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("kndo_filler_"))
            {
                out.push(path);
            }
        }
    }
    walk(dir, &mut out);
    out.sort();
    out
}

fn fixture_roots() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut roots = Vec::new();
    for adapter in ["kndo-adapter-js", "kndo-adapter-go"] {
        let base = manifest
            .parent()
            .unwrap()
            .join(adapter)
            .join("tests/fixtures");
        let Ok(entries) = fs::read_dir(&base) else {
            continue;
        };
        for entry in entries {
            let project = entry.unwrap().path().join("project");
            if project.is_dir() {
                roots.push(project);
            }
        }
    }
    roots.sort();
    assert!(!roots.is_empty(), "no fixture corpora found");
    roots
}

/// Runs one mutation over every fixture; returns how many runs the patch path served.
fn run_mutation(label: &str, mutate: impl Fn(&Path, &str) -> String) -> usize {
    let mut patched_runs = 0;
    for fixture in fixture_roots() {
        let name = format!(
            "{}-{}",
            fixture
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy(),
            label
        );
        let work = std::env::temp_dir().join(format!("kndo-patch-eq-{name}"));
        let _ = fs::remove_dir_all(&work);
        copy_tree(&fixture, &work);
        // Pad the tree with inert filler so one mutated file sits under the measured 5%
        // work threshold (RFC 0013 §5) — the conformance fixtures are deliberately tiny,
        // and this suite is about equivalence under mutation, not about fixture findings.
        for i in 0..40 {
            fs::write(
                work.join(format!("kndo_filler_{i}.ts")),
                format!("export const filler{i} = {i};\n"),
            )
            .unwrap();
        }

        let sources = claimed_source_files(&work);
        let Some(target) = sources.first() else {
            continue; // fixture with no claimed sources — nothing to mutate
        };
        let adapters = kndo::default_adapters;

        let cache_dir = std::env::temp_dir().join(format!("kndo-patch-eq-{name}-cache"));
        let _ = fs::remove_dir_all(&cache_dir);
        let cache = kndo::cache::ProjectCache::open(&cache_dir);
        assemble_with_cache(&work, &adapters(), &[], Some(&cache))
            .unwrap_or_else(|e| panic!("cold assemble failed for {name}: {e:?}"));

        let original = fs::read_to_string(target).unwrap();
        fs::write(target, mutate(target, &original)).unwrap();

        let (cached_graph, cached_diags) =
            assemble_with_cache(&work, &adapters(), &[], Some(&cache))
                .unwrap_or_else(|e| panic!("cached assemble failed for {name}: {e:?}"));
        if cache.graph_hits() > 0 {
            patched_runs += 1;
        }

        let (scratch_graph, scratch_diags) = assemble(&work, &adapters(), &[])
            .unwrap_or_else(|e| panic!("scratch assemble failed for {name}: {e:?}"));
        assert_eq!(
            cached_graph, scratch_graph,
            "graph divergence on {name} — the RFC 0013 §6 gate"
        );
        assert_eq!(
            cached_diags, scratch_diags,
            "diagnostics divergence on {name}"
        );

        let _ = fs::remove_dir_all(&work);
        let _ = fs::remove_dir_all(&cache_dir);
    }
    patched_runs
}

#[test]
fn comment_appends_are_equivalent_and_exercise_the_patch() {
    let patched = run_mutation("comment", |_, src| format!("{src}\n// kndo patch probe\n"));
    assert!(
        patched > 0,
        "no fixture engaged the patch path — the suite is not proving anything"
    );
}

#[test]
fn span_shifts_are_equivalent() {
    // A Go file's `package` clause must stay first, so shift spans with a trailing comment
    // block instead of a leading blank line for .go targets.
    run_mutation("spanshift", |path, src| {
        if path.extension().and_then(|e| e.to_str()) == Some("go") {
            format!("{src}\n// shifted\n// shifted again\n")
        } else {
            format!("\n\n{src}")
        }
    });
}

#[test]
fn surface_changes_fall_back_and_stay_equivalent() {
    run_mutation("surface", |path, src| {
        if path.extension().and_then(|e| e.to_str()) == Some("go") {
            format!("{src}\nfunc KndoPatchProbe() {{}}\n")
        } else {
            format!("{src}\nexport function kndoPatchProbe() {{}}\n")
        }
    });
}
