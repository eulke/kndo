//! `duplicate` — RFC 0005 §6, both halves. **Exact file duplicates**: byte-identical files
//! (subject `file`, below). **Structural clones** (`find_duplicate_functions`): Type-1/Type-2
//! callable-body clones over the adapters' winnowing fingerprints
//! (`ProjectGraph::function_metrics` — normalized token streams, so reformatting, comments,
//! and renamed identifiers/literals don't hide a copy), grouped transitively by Jaccard
//! similarity through a shared-fingerprint index, same-language only, one `info` finding per
//! group with every instance in `related`. Generated/vendored files are exempt from the
//! structural half (a generator copying itself is its own business) but NOT from the exact
//! half — see below for why.
//!
//! Exact half: byte-identical files already share the
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
use crate::vocab::{Confidence, SymbolId};

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
            related: Vec::new(),
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

// ---------------------------------------------------------------- structural clones (§6)

/// Two callables are clones when their winnowing fingerprint sets overlap this much
/// (Jaccard). 0.8 catches Type-1/Type-2 clones with light drift while a genuinely different
/// function body — even one solving a similar problem — falls far below it.
const CLONE_JACCARD: f64 = 0.8;

/// A fingerprint shared by more than this many callables is boilerplate shape (getters,
/// trivial delegations), not copy-paste evidence — its postings list is skipped when pairing
/// candidates, keeping the candidate set near-linear instead of quadratic on common shapes.
const MAX_POSTING: usize = 20;

/// Structural Type-1/Type-2 clones over `ProjectGraph::function_metrics` (RFC 0005 §6):
/// candidates pair through a shared-fingerprint index (same language only), confirm by
/// Jaccard similarity, and group transitively — one `info` finding per clone group, every
/// instance in `related`.
/// Findings plus the redundant clone instances — every group member beyond its
/// lexicographically-first canonical one, with its normalized token count. `health`'s
/// "duplicated tokens" numerator (RFC 0005 §11): the canonical copy is the one you'd keep, so
/// only the copies beyond it count as duplicated.
pub fn find_duplicate_functions(graph: &ProjectGraph) -> (Vec<Finding>, Vec<(SymbolId, u32)>) {
    use crate::vocab::FileOrigin;
    use std::collections::HashSet;

    // Eligible instances: fingerprinted callables in authored, claimed files.
    struct Instance<'g> {
        symbol: SymbolId,
        path: &'g str,
        language: &'g str,
        token_count: u32,
        fingerprints: &'g [u64],
    }
    let mut instances: Vec<Instance> = Vec::new();
    for (symbol_id, metrics) in &graph.function_metrics {
        if metrics.fingerprints.is_empty() {
            continue; // under the min-tokens gate — too small to meaningfully clone-match
        }
        let symbol = &graph.symbols[symbol_id.0 as usize];
        let file = &graph.files[symbol.file.0 as usize];
        let Some(class) = file.class else { continue };
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue;
        }
        let Some(language) = file.language.as_deref() else {
            continue;
        };
        instances.push(Instance {
            symbol: *symbol_id,
            path: file.path.0.as_str(),
            language,
            token_count: metrics.token_count,
            fingerprints: &metrics.fingerprints,
        });
    }

    // Shared-fingerprint index → candidate pairs (same language), then Jaccard-confirm.
    let mut postings: HashMap<(&str, u64), Vec<usize>> = HashMap::new();
    for (i, inst) in instances.iter().enumerate() {
        for &fp in inst.fingerprints {
            postings.entry((inst.language, fp)).or_default().push(i);
        }
    }
    let mut parent: Vec<usize> = (0..instances.len()).collect();
    fn find(parent: &mut Vec<usize>, x: usize) -> usize {
        if parent[x] != x {
            let root = find(parent, parent[x]);
            parent[x] = root;
        }
        parent[x]
    }
    let mut checked: HashSet<(usize, usize)> = HashSet::new();
    for list in postings.values() {
        if list.len() < 2 || list.len() > MAX_POSTING {
            continue;
        }
        for (a_pos, &a) in list.iter().enumerate() {
            for &b in &list[a_pos + 1..] {
                if !checked.insert((a, b)) {
                    continue;
                }
                let sa: HashSet<u64> = instances[a].fingerprints.iter().copied().collect();
                let sb: HashSet<u64> = instances[b].fingerprints.iter().copied().collect();
                let inter = sa.intersection(&sb).count();
                let union = sa.len() + sb.len() - inter;
                if union > 0 && inter as f64 / union as f64 >= CLONE_JACCARD {
                    let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                    if ra != rb {
                        parent[rb] = ra;
                    }
                }
            }
        }
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..instances.len() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    let mut findings = Vec::new();
    let mut duplicated: Vec<(SymbolId, u32)> = Vec::new();
    for members in groups.into_values() {
        if members.len() < 2 {
            continue;
        }
        // (path, qualified) per instance, lexicographic — the first is the anchor.
        let mut named: Vec<(String, String, usize)> = members
            .iter()
            .map(|&i| {
                let symbol = &graph.symbols[instances[i].symbol.0 as usize];
                (instances[i].path.to_string(), symbol.qualified_name(), i)
            })
            .collect();
        named.sort();
        for (_, _, i) in &named[1..] {
            duplicated.push((instances[*i].symbol, instances[*i].token_count));
        }
        let selectors: Vec<String> = named.iter().map(|(p, q, _)| format!("{p}#{q}")).collect();
        let (_, anchor_name, anchor_idx) = &named[0];
        let anchor_symbol = &graph.symbols[instances[*anchor_idx].symbol.0 as usize];
        let anchor_file = &graph.files[anchor_symbol.file.0 as usize];
        let facet = anchor_symbol.kind.facet().to_string();

        let related = named
            .iter()
            .map(|(_, q, i)| {
                let s = &graph.symbols[instances[*i].symbol.0 as usize];
                crate::engine::RelatedLocation {
                    role: "clone".to_string(),
                    path: graph.files[s.file.0 as usize].path.clone(),
                    range: Some(s.span),
                    note: Some(q.clone()),
                }
            })
            .collect();

        let shown: Vec<&str> = selectors.iter().map(String::as_str).collect();
        findings.push(Finding {
            id: finding_id("duplicate", &facet, "", "", &selectors.join("\u{1}")),
            category: "duplicate".to_string(),
            group: "waste".to_string(),
            subject_kind: facet.clone(),
            severity: Severity::Info, // §6: info by default — duplication is sometimes deliberate
            confidence: Confidence::Certain,
            message: format!(
                "{} structurally identical {facet}s (identifiers/literals aside): {} — extract the shared implementation",
                named.len(),
                summarize(&shown),
            ),
            location: Location {
                path: Some(anchor_file.path.clone()),
                range: Some(anchor_symbol.span),
                symbol: Some(anchor_name.clone()),
                package: graph.package_name(anchor_file.package).map(str::to_string),
            },
            related,
            delta: None,
            delta_origin: None,
        });
    }
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    duplicated.sort();
    (findings, duplicated)
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
            unit: None,
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

    // ------------------------------------------------- structural clones (§6)

    use crate::adapter::VisibilityLevel;
    use crate::graph::{SymbolMetrics, SymbolNode};
    use crate::vocab::{FileClass, FileId, FileOrigin, FileRole, SymbolId, SymbolKind};

    fn claimed_file(path: &str) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass {
                role: FileRole::Production,
                origin: FileOrigin::Authored,
            }),
            package: crate::vocab::PackageId(0),
            unit: None,
        }
    }

    fn callable(file: u32, name: &str) -> SymbolNode {
        SymbolNode {
            file: FileId(file),
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: crate::adapter::Span {
                start: (1, 1),
                end: (5, 1),
            },
            exported: true,
            visibility: VisibilityLevel(1),
            member_of: None,
            signature_span: None,
        }
    }

    fn metrics(fingerprints: Vec<u64>) -> SymbolMetrics {
        SymbolMetrics {
            cyclomatic: 2,
            loc: 5,
            token_count: 60,
            fingerprints,
        }
    }

    #[test]
    fn identical_fingerprint_sets_group_into_one_finding() {
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.ts"), claimed_file("b.ts")],
            vec![callable(0, "one"), callable(1, "two")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), metrics(vec![1, 2, 3, 4, 5])),
            (SymbolId(1), metrics(vec![1, 2, 3, 4, 5])),
        ]);
        let findings = find_duplicate_functions(&graph).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "duplicate");
        assert_eq!(findings[0].subject_kind, "function");
        assert_eq!(findings[0].severity, Severity::Info);
        assert_eq!(findings[0].related.len(), 2, "every instance in related");
        assert!(findings[0].message.contains("a.ts#one"));
        assert!(findings[0].message.contains("b.ts#two"));
    }

    #[test]
    fn near_identical_sets_above_the_threshold_still_group() {
        // 9 of 10 shared → Jaccard 9/11 ≈ 0.818 ≥ 0.8.
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.ts"), claimed_file("b.ts")],
            vec![callable(0, "one"), callable(1, "two")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), metrics(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10])),
            (SymbolId(1), metrics(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 11])),
        ]);
        assert_eq!(find_duplicate_functions(&graph).0.len(), 1);
    }

    #[test]
    fn dissimilar_sets_do_not_group() {
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.ts"), claimed_file("b.ts")],
            vec![callable(0, "one"), callable(1, "two")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), metrics(vec![1, 2, 3, 4, 5])),
            (SymbolId(1), metrics(vec![1, 6, 7, 8, 9])),
        ]);
        assert!(find_duplicate_functions(&graph).0.is_empty());
    }

    #[test]
    fn generated_instances_are_exempt() {
        let mut gen = claimed_file("gen.ts");
        gen.class = Some(FileClass {
            role: FileRole::Production,
            origin: FileOrigin::Generated,
        });
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.ts"), gen],
            vec![callable(0, "one"), callable(1, "two")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), metrics(vec![1, 2, 3, 4, 5])),
            (SymbolId(1), metrics(vec![1, 2, 3, 4, 5])),
        ]);
        assert!(find_duplicate_functions(&graph).0.is_empty());
    }

    #[test]
    fn ungated_small_functions_never_match() {
        // Empty fingerprint vectors (under the adapter's min-tokens gate) are skipped, not
        // treated as vacuously identical.
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.ts"), claimed_file("b.ts")],
            vec![callable(0, "one"), callable(1, "two")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), metrics(vec![])),
            (SymbolId(1), metrics(vec![])),
        ]);
        assert!(find_duplicate_functions(&graph).0.is_empty());
    }

    #[test]
    fn transitive_groups_collapse_to_one_finding() {
        // a≈b and b≈c: one group of three, not two pair findings.
        let graph = ProjectGraph::for_test(
            vec![
                claimed_file("a.ts"),
                claimed_file("b.ts"),
                claimed_file("c.ts"),
            ],
            vec![callable(0, "f"), callable(1, "g"), callable(2, "h")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), metrics(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10])),
            (SymbolId(1), metrics(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10])),
            (SymbolId(2), metrics(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 11])),
        ]);
        let findings = find_duplicate_functions(&graph).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].related.len(), 3);
    }
}
