//! `duplicate` — exact file duplicates (RFC 0005 §6). Byte-identical files already share the
//! blake3 content hash discovery computes for the cache, so this is free: no extraction, no
//! adapter needed at all. That last point matters — this is the one M1 analysis that runs over
//! *unclaimed* files too (images, binaries, configs no adapter recognizes), because those are
//! exactly what token-based clone detection can never see and what §6 names as the target
//! ("copy-pasted configs, images, and any other asset"). One finding groups every copy of one
//! content, not one finding per pair.
//!
//! Deliberately no Generated/Vendored exemption here, unlike `unused`/`test-only` (RFC 0005
//! §4): §6 states the byte-identical rule with no such carve-out, and origin isn't even known
//! for the unclaimed files this analysis exists to cover (`FileClass` requires a claim). The
//! one floor applied — empty files — isn't a policy carve-out either: "duplicate content" is
//! vacuous when there's no content, and virtually every real repo has many genuinely-empty
//! files (`.gitkeep`, stub configs) that would otherwise collapse into one giant,
//! unactionable finding spanning unrelated directories.

use std::collections::HashMap;

use crate::analysis::finding_id;
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::Confidence;

pub fn find_duplicate_files(graph: &ProjectGraph) -> Vec<Finding> {
    let empty_hash: [u8; 32] = blake3::hash(b"").into();

    let mut by_hash: HashMap<[u8; 32], Vec<&str>> = HashMap::new();
    for file in &graph.files {
        if file.content_hash == empty_hash {
            continue;
        }
        by_hash
            .entry(file.content_hash)
            .or_default()
            .push(file.path.0.as_str());
    }

    let mut findings = Vec::new();
    for (hash, mut paths) in by_hash {
        if paths.len() < 2 {
            continue;
        }
        paths.sort_unstable();
        // The content hash — not any one member's path — is this finding's true, stable
        // identity: renaming one copy while the rest of the group survives unchanged is still
        // "the same duplicate-content situation," not a new finding (unlike every other M1
        // finding so far, which anchors to one file's own path).
        let discriminator = blake3::Hash::from(hash).to_hex().to_string();
        findings.push(Finding {
            id: finding_id("duplicate", "file", "", "", &discriminator),
            category: "duplicate".to_string(),
            group: "waste".to_string(),
            subject_kind: "file".to_string(),
            severity: Severity::Info, // RFC 0005 §6: info by default — duplication is sometimes deliberate
            confidence: Confidence::Certain,
            message: format!(
                "{} identical files share the same content: {}",
                paths.len(),
                summarize(&paths)
            ),
            // Spans every copy — no single path is *the* location (the message lists them
            // all); expressing that properly is `related`, not built yet.
            location: Location::default(),
            delta: None,
            delta_origin: None,
        });
    }
    findings
}

fn summarize(paths: &[&str]) -> String {
    if paths.len() <= 3 {
        paths.join(", ")
    } else {
        format!(
            "{}, {}, {} and {} more",
            paths[0],
            paths[1],
            paths[2],
            paths.len() - 3
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::graph::FileNode;
    use smol_str::SmolStr;

    fn hash_of(bytes: &[u8]) -> [u8; 32] {
        blake3::hash(bytes).into()
    }

    fn file(path: &str, content: &[u8]) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: hash_of(content),
            language: None,
            class: None,
            package: crate::vocab::PackageId(0),
        }
    }

    #[test]
    fn two_identical_files_produce_one_finding() {
        let files = vec![file("a.png", b"same bytes"), file("b.png", b"same bytes")];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let findings = find_duplicate_files(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "duplicate");
        assert_eq!(findings[0].group, "waste");
        assert_eq!(findings[0].subject_kind, "file");
        assert!(findings[0].message.contains("a.png"));
        assert!(findings[0].message.contains("b.png"));
    }

    #[test]
    fn three_identical_files_still_group_into_one_finding() {
        let files = vec![
            file("a.png", b"same"),
            file("b.png", b"same"),
            file("c.png", b"same"),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let findings = find_duplicate_files(&graph);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.starts_with("3 identical files"));
    }

    #[test]
    fn distinct_content_produces_no_finding() {
        let files = vec![file("a.png", b"one"), file("b.png", b"two")];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        assert!(find_duplicate_files(&graph).is_empty());
    }

    #[test]
    fn unclaimed_files_are_in_scope() {
        // Unlike `unused`, this analysis exists specifically to cover files no adapter claims
        // (class: None) — images and binaries chief among them.
        let files = vec![file("logo.png", b"bytes"), file("logo-copy.png", b"bytes")];
        assert!(files.iter().all(|f| f.class.is_none()));
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        assert_eq!(find_duplicate_files(&graph).len(), 1);
    }

    #[test]
    fn empty_files_are_exempt() {
        let files = vec![file("a.txt", b""), file("b.txt", b"")];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        assert!(find_duplicate_files(&graph).is_empty());
    }

    #[test]
    fn single_copy_is_not_a_finding() {
        let files = vec![file("a.png", b"unique")];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        assert!(find_duplicate_files(&graph).is_empty());
    }

    #[test]
    fn finding_id_is_stable_across_runs() {
        let files = vec![file("a.png", b"same"), file("b.png", b"same")];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let a = find_duplicate_files(&graph);
        let b = find_duplicate_files(&graph);
        assert_eq!(a[0].id, b[0].id);
    }

    #[test]
    fn finding_id_is_unaffected_by_a_member_rename() {
        let before = vec![file("old-name.png", b"same"), file("b.png", b"same")];
        let after = vec![file("new-name.png", b"same"), file("b.png", b"same")];
        let g1 = ProjectGraph::for_test(before, vec![], vec![], vec![]);
        let g2 = ProjectGraph::for_test(after, vec![], vec![], vec![]);
        assert_eq!(
            find_duplicate_files(&g1)[0].id,
            find_duplicate_files(&g2)[0].id,
            "the group's identity is its content, not any one member's path"
        );
    }
}
