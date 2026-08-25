//! `cyclic` — dependency cycles (group `risk`): strongly connected components of
//! size ≥ 2 in the file-import graph, and in the package graph where manifests define units.
//! **One finding per cycle**, not per participant (rollup spirit), anchored at the cycle's
//! most-referenced node, with a shortest cycle path in `related` as the evidence chain — the
//! first finding to populate that field.
//!
//! Cycle tolerance is a **language fact**, declared by each adapter as data
//! ([`CyclePolicy`], carried onto `ProjectGraph::cycle_policies` like the visibility
//! ladders): `Hazard → warning` (JS/TS — init-order bugs); `Idiomatic` and `Impossible`
//! alike emit **nothing**. An idiomatic cycle (Rust modules within a crate) is true
//! information about legal, routine structure — kndo does not dress information up as a
//! defect; an impossible one (Go — the compiler forbids import cycles) could only be a
//! resolution artifact. Both degrade to silence, never to accusation. A mixed-language
//! cycle is reported iff *any* participant's language calls it a hazard — the hazard is
//! real for that language; participants whose language declares no policy (unclaimed
//! files) contribute none, and a cycle with no hazard participant emits nothing.
//!
//! Level interplay: a file cycle whose participants all live in one package is a file-level
//! finding. A file cycle *spanning* real packages is reported at package level only — the
//! package finding is its rollup (spanning packages P and Q implies package edges both ways,
//! so the package SCC necessarily exists), and two findings for one loop would be noise. A
//! cycle touching the implicit package (files no manifest owns) stays file-level: the
//! implicit package isn't a manifest-defined unit, so there is no package finding to roll
//! into. Package-level cycles are computed over real (manifest-backed) packages only.
//!
//! Confidence: the weakest edge on the *reported* evidence path — a cycle is only as real as
//! its weakest link, the same honesty rule `trace` applies to paths. Exemption: a cycle every
//! one of whose participants is generated/vendored is skipped (nobody authored it); one with
//! any authored participant fires — the authored code is in the loop too.
//!
//! Incremental recomputation within the dirty region's weakly connected component
//! is an optimization for the incremental-analysis mode kndo doesn't have — every
//! analysis today recomputes fully per run, and Tarjan is O(V+E), noise next to assembly.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::adapter::{CyclePolicy, CycleTolerance, Span};
use crate::analysis::{finding_id, package_discriminator, package_label, FindingIdParts};
use crate::engine::{Finding, Location, RelatedLocation, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{
    Category, Confidence, EdgeKind, FileId, FileOrigin, Group, PackageId, SubjectKind,
};

/// Findings plus the set of files participating in any *reported* (hazard) cycle —
/// `health`'s "files participating in cycles" numerator. The set includes files whose
/// file-level cycle rolled up into a package-level finding (they still sit in a real cycle);
/// it excludes cycles with no hazard participant — idiomatic and impossible alike — and
/// all-generated/vendored ones, exactly like the findings themselves, keeping health's
/// promise that a tolerated cycle is never a penalty.
pub fn find_cycles(graph: &ProjectGraph) -> (Vec<Finding>, HashSet<FileId>) {
    let mut findings = Vec::new();
    let mut participants: HashSet<FileId> = HashSet::default();

    // ------------------------------------------------------------- file level
    // Adjacency over ImportsFile edges, keeping the strongest-confidence edge per (from, to)
    // pair for evidence.
    let mut file_adj: HashMap<u32, Vec<u32>> = HashMap::default();
    let mut file_edge: HashMap<(u32, u32), (Confidence, Option<Span>)> = HashMap::default();
    for edge in &graph.edges {
        if let EdgeKind::ImportsFile { from, to } = edge.kind {
            if from == to {
                continue; // a self-import is not a cycle between modules
            }
            let entry = file_edge
                .entry((from.0, to.0))
                .or_insert((edge.confidence, edge.span));
            if edge.confidence > entry.0 {
                *entry = (edge.confidence, edge.span);
            }
            file_adj.entry(from.0).or_default().push(to.0);
        }
    }

    for scc in tarjan(&file_adj) {
        if scc.len() < 2 {
            continue;
        }
        let files: Vec<FileId> = scc.iter().map(|&i| FileId(i)).collect();

        // Origin exemption: a loop nobody authored is nobody's finding.
        let any_authored = files.iter().any(|f| {
            graph.files[f.0 as usize]
                .class
                .is_some_and(|c| c.origin == FileOrigin::Authored)
        });
        if !any_authored {
            continue;
        }

        // Participation is judged before the package rollup below: a file in a cross-package
        // cycle is still in a cycle, even though its finding reports at package level. Only
        // *reported* (hazard) cycles count — health's cycles axis promises "a cycle the
        // language declares impossible or idiomatic isn't a penalty", and this is where
        // that promise is kept.
        if cycle_reported(graph, &files, |p| p.file_cycles) {
            participants.extend(files.iter().copied());
        }

        // Spanning ≥ 2 real packages → the package-level finding is this cycle's rollup.
        let real_packages: HashSet<PackageId> = files
            .iter()
            .map(|f| graph.files[f.0 as usize].package)
            .filter(|p| graph.packages[p.0 as usize].manifest.is_some())
            .collect();
        let touches_implicit = files.iter().any(|f| {
            graph.packages[graph.files[f.0 as usize].package.0 as usize]
                .manifest
                .is_none()
        });
        if real_packages.len() >= 2 && !touches_implicit {
            continue;
        }

        if !cycle_reported(graph, &files, |p| p.file_cycles) {
            continue; // no participant's language calls this level a hazard — silence
        }

        // Anchor: most referenced within the cycle (in-degree from cycle members), ties to
        // the lexicographically-first path so ids and output stay deterministic. In-degree is
        // precomputed in one pass over the SCC's own adjacency — the per-candidate scan over
        // every edge this replaced was O(|SCC| × |edges|), a 13-second wall on a synthetic
        // 5k-file component.
        let in_cycle: HashSet<u32> = scc.iter().copied().collect();
        let mut indegree: HashMap<u32, usize> = HashMap::default();
        for &from in &in_cycle {
            for to in file_adj.get(&from).map(Vec::as_slice).unwrap_or(&[]) {
                if in_cycle.contains(to) {
                    *indegree.entry(*to).or_default() += 1;
                }
            }
        }
        let anchor = *files
            .iter()
            .max_by_key(|f| {
                (
                    indegree.get(&f.0).copied().unwrap_or(0),
                    std::cmp::Reverse(graph.files[f.0 as usize].path.0.as_str()),
                )
            })
            .unwrap();

        let path_hops = shortest_cycle(anchor.0, &file_adj, &in_cycle);
        let confidence = path_hops
            .windows(2)
            .filter_map(|w| file_edge.get(&(w[0], w[1])).map(|(c, _)| *c))
            .min()
            .unwrap_or(Confidence::Certain);

        let anchor_file = &graph.files[anchor.0 as usize];
        let rendered: Vec<&str> = path_hops
            .iter()
            .map(|&i| graph.files[i as usize].path.0.as_str())
            .collect();
        let related = path_hops
            .windows(2)
            .map(|w| RelatedLocation {
                role: "cycle-hop".to_string(),
                path: graph.files[w[0] as usize].path.clone(),
                range: file_edge.get(&(w[0], w[1])).and_then(|(_, s)| *s),
                note: Some(format!("imports {}", graph.files[w[1] as usize].path.0)),
            })
            .collect();

        findings.push(Finding {
            advisory: false,
            id: finding_id(FindingIdParts {
                category: &Category::CYCLIC,
                subject_kind: &SubjectKind::FILE,
                path: &cycle_key(graph, &files),
                symbol_path: "",
                discriminator: "",
            }),
            category: Category::CYCLIC,
            group: Group::Risk,
            subject_kind: SubjectKind::FILE,
            severity: Severity::Warning,
            confidence,
            message: format!(
                "{} files form an import cycle ({}) — break it by extracting the shared \
                 piece into its own module or inverting one of the imports",
                files.len(),
                rendered.join(" → "),
            ),
            location: Location {
                path: Some(anchor_file.path.clone()),
                range: None,
                symbol: None,
                package: graph.package_name(anchor_file.package).map(str::to_string),
            },
            related,
            delta: None,
            delta_origin: None,
        });
    }

    // ------------------------------------------------------------- package level
    // Derived package graph (real packages only): P → Q when any file in P imports a file in
    // Q. Evidence keeps one representative file edge per package edge.
    let mut pkg_adj: HashMap<u32, Vec<u32>> = HashMap::default();
    let mut pkg_edge: HashMap<(u32, u32), (Confidence, FileId, FileId)> = HashMap::default();
    for edge in &graph.edges {
        if let EdgeKind::ImportsFile { from, to } = edge.kind {
            let p = graph.files[from.0 as usize].package;
            let q = graph.files[to.0 as usize].package;
            if p == q
                || graph.packages[p.0 as usize].manifest.is_none()
                || graph.packages[q.0 as usize].manifest.is_none()
            {
                continue;
            }
            let entry = pkg_edge
                .entry((p.0, q.0))
                .or_insert((edge.confidence, from, to));
            if edge.confidence > entry.0 {
                *entry = (edge.confidence, from, to);
            }
            pkg_adj.entry(p.0).or_default().push(q.0);
        }
    }

    for scc in tarjan(&pkg_adj) {
        if scc.len() < 2 {
            continue;
        }
        let packages: Vec<PackageId> = scc.iter().map(|&i| PackageId(i)).collect();
        let participant_files: Vec<FileId> = pkg_edge
            .iter()
            .filter(|((p, q), _)| scc.contains(p) && scc.contains(q))
            .map(|(_, &(_, from, _))| from)
            .collect();
        if !cycle_reported(graph, &participant_files, |p| p.package_cycles) {
            continue;
        }

        let in_cycle: HashSet<u32> = scc.iter().copied().collect();
        let anchor = *packages
            .iter()
            .max_by_key(|p| {
                let indegree = pkg_edge
                    .keys()
                    .filter(|(from, to)| *to == p.0 && in_cycle.contains(from))
                    .count();
                (indegree, std::cmp::Reverse(package_label(graph, **p)))
            })
            .unwrap();

        let path_hops = shortest_cycle(anchor.0, &pkg_adj, &in_cycle);
        let confidence = path_hops
            .windows(2)
            .filter_map(|w| pkg_edge.get(&(w[0], w[1])).map(|(c, ..)| *c))
            .min()
            .unwrap_or(Confidence::Certain);
        let rendered: Vec<String> = path_hops
            .iter()
            .map(|&i| package_label(graph, PackageId(i)))
            .collect();
        let related = path_hops
            .windows(2)
            .filter_map(|w| {
                pkg_edge
                    .get(&(w[0], w[1]))
                    .map(|&(_, from, to)| RelatedLocation {
                        role: "cycle-hop".to_string(),
                        path: graph.files[from.0 as usize].path.clone(),
                        range: None,
                        note: Some(format!(
                            "{} imports {} ({})",
                            package_label(graph, PackageId(w[0])),
                            package_label(graph, PackageId(w[1])),
                            graph.files[to.0 as usize].path.0
                        )),
                    })
            })
            .collect();

        let anchor_pkg = &graph.packages[anchor.0 as usize];
        let discriminator: Vec<String> = {
            let mut d: Vec<String> = packages
                .iter()
                .map(|&p| package_discriminator(graph, p))
                .collect();
            d.sort();
            d
        };
        findings.push(Finding {
            advisory: false,
            id: finding_id(FindingIdParts {
                category: &Category::CYCLIC,
                subject_kind: &SubjectKind::PACKAGE,
                path: &discriminator.join("\u{1}"),
                symbol_path: "",
                discriminator: "",
            }),
            category: Category::CYCLIC,
            group: Group::Risk,
            subject_kind: SubjectKind::PACKAGE,
            severity: Severity::Warning,
            confidence,
            message: format!(
                "{} packages form a dependency cycle ({}) — cycles between workspace \
                 packages break publish ordering and standalone installs",
                packages.len(),
                rendered.join(" → "),
            ),
            location: Location {
                path: anchor_pkg.manifest.clone(),
                range: None,
                symbol: None,
                package: graph.package_name(anchor).map(str::to_string),
            },
            related,
            delta: None,
            delta_origin: None,
        });
    }

    findings.sort_by(|a, b| a.id.cmp(&b.id));
    (findings, participants)
}

/// Whether any participant's language declares this level a `Hazard` — the only stance
/// that reports (always `Severity::Warning`, with the prescriptive advice: the hazard is
/// real for that language). `Idiomatic` and `Impossible` participants alike contribute
/// nothing, and so do participants whose language declares no policy. A boolean rather
/// than a severity fold on purpose: an earlier fold over `Severity::max` silently picked
/// the *least* severe stance in mixed cycles (`Severity`'s derived `Ord` is worst-first),
/// and a predicate cannot express "which severity won" — the bug is unrepresentable.
fn cycle_reported(
    graph: &ProjectGraph,
    participants: &[FileId],
    level: impl Fn(&CyclePolicy) -> CycleTolerance,
) -> bool {
    participants.iter().any(|f| {
        graph.files[f.0 as usize]
            .language
            .as_deref()
            .and_then(|lang| graph.cycle_policy_for(lang))
            .is_some_and(|policy| level(&policy) == CycleTolerance::Hazard)
    })
}

/// The cycle's stable identity: its sorted participant paths — line-position-free, so
/// reformatting never changes the id, and adding an unrelated file elsewhere doesn't either.
fn cycle_key(graph: &ProjectGraph, files: &[FileId]) -> String {
    let mut paths: Vec<&str> = files
        .iter()
        .map(|f| graph.files[f.0 as usize].path.0.as_str())
        .collect();
    paths.sort_unstable();
    paths.join("\u{1}")
}

/// Iterative Tarjan over a `u32`-keyed adjacency map — returns every SCC (singletons
/// included; callers filter by size).
fn tarjan(adjacency: &HashMap<u32, Vec<u32>>) -> Vec<Vec<u32>> {
    #[derive(Default, Clone)]
    struct NodeState {
        index: Option<u32>,
        lowlink: u32,
        on_stack: bool,
    }
    let mut nodes: Vec<u32> = adjacency
        .iter()
        .flat_map(|(k, vs)| std::iter::once(*k).chain(vs.iter().copied()))
        .collect();
    nodes.sort_unstable();
    nodes.dedup();

    let mut state: HashMap<u32, NodeState> = HashMap::default();
    let mut stack: Vec<u32> = Vec::new();
    let mut next_index = 0u32;
    let mut sccs: Vec<Vec<u32>> = Vec::new();

    // Explicit work stack (node, next-child cursor) — recursion depth would be the longest
    // import chain, which real projects can make deep enough to matter.
    for &root in &nodes {
        if state.get(&root).and_then(|s| s.index).is_some() {
            continue;
        }
        let mut work: Vec<(u32, usize)> = vec![(root, 0)];
        while let Some(&mut (v, ref mut cursor)) = work.last_mut() {
            if *cursor == 0 {
                let s = state.entry(v).or_default();
                s.index = Some(next_index);
                s.lowlink = next_index;
                s.on_stack = true;
                next_index += 1;
                stack.push(v);
            }
            let children = adjacency.get(&v).map(Vec::as_slice).unwrap_or(&[]);
            if let Some(&w) = children.get(*cursor) {
                *cursor += 1;
                match state.get(&w).and_then(|s| s.index) {
                    None => work.push((w, 0)),
                    Some(w_index) => {
                        if state.get(&w).is_some_and(|s| s.on_stack) {
                            let v_low = state.get(&v).map(|s| s.lowlink).unwrap_or(0);
                            state.get_mut(&v).unwrap().lowlink = v_low.min(w_index);
                        }
                    }
                }
            } else {
                let (v_low, v_index) = {
                    let s = &state[&v];
                    (s.lowlink, s.index.unwrap())
                };
                work.pop();
                if let Some(&mut (parent, _)) = work.last_mut() {
                    let p_low = state[&parent].lowlink;
                    state.get_mut(&parent).unwrap().lowlink = p_low.min(v_low);
                }
                if v_low == v_index {
                    let mut scc = Vec::new();
                    while let Some(w) = stack.pop() {
                        state.get_mut(&w).unwrap().on_stack = false;
                        scc.push(w);
                        if w == v {
                            break;
                        }
                    }
                    scc.sort_unstable();
                    sccs.push(scc);
                }
            }
        }
    }
    sccs
}

/// Shortest cycle through `start` inside the SCC (BFS back to the start), returned as
/// `[start, …, start]` — the evidence path, not an enumeration of the whole component.
fn shortest_cycle(
    start: u32,
    adjacency: &HashMap<u32, Vec<u32>>,
    in_scc: &HashSet<u32>,
) -> Vec<u32> {
    use std::collections::VecDeque;
    let mut parent: HashMap<u32, u32> = HashMap::default();
    let mut queue = VecDeque::new();
    queue.push_back(start);
    while let Some(v) = queue.pop_front() {
        for &w in adjacency.get(&v).map(Vec::as_slice).unwrap_or(&[]) {
            if !in_scc.contains(&w) {
                continue;
            }
            if w == start {
                let mut path = vec![start];
                let mut cur = v;
                let mut rev = Vec::new();
                while cur != start {
                    rev.push(cur);
                    cur = parent[&cur];
                }
                path.extend(rev.into_iter().rev());
                path.push(start);
                return path;
            }
            if !parent.contains_key(&w) && w != start {
                parent.insert(w, v);
                queue.push_back(w);
            }
        }
    }
    vec![start, start] // defensive: an SCC ≥ 2 always has a cycle through every member
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::graph::{FileNode, PackageNode, ProjectGraph};
    use crate::vocab::{Edge, FileClass, FileRole, Provenance};
    use smol_str::SmolStr;

    fn file(path: &str, package: u32) -> FileNode {
        file_lang(path, package, "mock")
    }

    fn file_lang(path: &str, package: u32, lang: &str) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new(lang)),
            class: Some(FileClass {
                role: FileRole::Production,
                origin: FileOrigin::Authored,
            }),
            package: PackageId(package),
            unit: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
        }
    }

    fn imports(from: u32, to: u32) -> Edge {
        Edge {
            owner: crate::vocab::FileId(0),
            kind: EdgeKind::ImportsFile {
                from: FileId(from),
                to: FileId(to),
            },
            confidence: Confidence::Certain,
            source: Provenance::Adapter(SmolStr::new("mock")),
            span: Some(Span {
                start: (1, 1),
                end: (1, 10),
            }),
        }
    }

    fn one_package_graph(files: Vec<FileNode>, edges: Vec<Edge>) -> ProjectGraph {
        ProjectGraph::for_test(files, vec![], vec![], edges)
    }

    #[test]
    fn a_two_file_cycle_is_one_warning_finding_with_the_path_in_related() {
        let graph = one_package_graph(
            vec![file("a.ts", 0), file("b.ts", 0)],
            vec![imports(0, 1), imports(1, 0)],
        );
        let (findings, participants) = find_cycles(&graph);
        assert_eq!(findings.len(), 1, "one finding per cycle, not per member");
        assert_eq!(
            participants.len(),
            2,
            "both files feed health's cycles axis"
        );
        let f = &findings[0];
        assert_eq!(f.category, "cyclic");
        assert_eq!(f.group, crate::vocab::Group::Risk);
        assert_eq!(f.subject_kind, "file");
        assert_eq!(f.severity, Severity::Warning); // mock policy: Hazard
        assert!(f.message.contains("2 files"));
        assert_eq!(f.related.len(), 2, "two hops close a two-node cycle");
        assert!(f.related.iter().all(|r| r.role == "cycle-hop"));
    }

    #[test]
    fn acyclic_imports_yield_nothing() {
        let graph = one_package_graph(
            vec![file("a.ts", 0), file("b.ts", 0), file("c.ts", 0)],
            vec![imports(0, 1), imports(1, 2)],
        );
        assert!(find_cycles(&graph).0.is_empty());
    }

    #[test]
    fn a_self_import_is_not_a_cycle() {
        let graph = one_package_graph(vec![file("a.ts", 0)], vec![imports(0, 0)]);
        assert!(find_cycles(&graph).0.is_empty());
    }

    #[test]
    fn two_disjoint_cycles_are_two_findings() {
        let graph = one_package_graph(
            vec![
                file("a.ts", 0),
                file("b.ts", 0),
                file("c.ts", 0),
                file("d.ts", 0),
            ],
            vec![imports(0, 1), imports(1, 0), imports(2, 3), imports(3, 2)],
        );
        assert_eq!(find_cycles(&graph).0.len(), 2);
    }

    #[test]
    fn impossible_tolerance_skips_the_level_entirely() {
        let graph = one_package_graph(
            vec![file("a.go", 0), file("b.go", 0)],
            vec![imports(0, 1), imports(1, 0)],
        )
        .with_cycle_policies(vec![(
            SmolStr::new("mock"),
            CyclePolicy {
                file_cycles: CycleTolerance::Impossible,
                package_cycles: CycleTolerance::Impossible,
            },
        )]);
        assert!(find_cycles(&graph).0.is_empty());
    }

    #[test]
    fn idiomatic_tolerance_skips_the_level_entirely() {
        let graph = one_package_graph(
            vec![file("a.rs", 0), file("b.rs", 0)],
            vec![imports(0, 1), imports(1, 0)],
        )
        .with_cycle_policies(vec![(
            SmolStr::new("mock"),
            CyclePolicy {
                file_cycles: CycleTolerance::Idiomatic,
                package_cycles: CycleTolerance::Idiomatic,
            },
        )]);
        let (findings, participants) = find_cycles(&graph);
        assert!(
            findings.is_empty(),
            "an idiomatic cycle is legal structure, not a finding: {findings:?}"
        );
        assert!(
            participants.is_empty(),
            "a tolerated cycle is never a health penalty either"
        );
    }

    #[test]
    fn a_mixed_hazard_and_idiomatic_cycle_is_a_warning() {
        // The case the old Severity::max fold got wrong (derived Ord is worst-first, so the
        // least severe stance won): one hazard participant must keep the cycle reported.
        let graph = one_package_graph(
            vec![file_lang("a.ts", 0, "haz"), file_lang("b.rs", 0, "idio")],
            vec![imports(0, 1), imports(1, 0)],
        )
        .with_cycle_policies(vec![
            (
                SmolStr::new("haz"),
                CyclePolicy {
                    file_cycles: CycleTolerance::Hazard,
                    package_cycles: CycleTolerance::Hazard,
                },
            ),
            (
                SmolStr::new("idio"),
                CyclePolicy {
                    file_cycles: CycleTolerance::Idiomatic,
                    package_cycles: CycleTolerance::Idiomatic,
                },
            ),
        ]);
        let (findings, participants) = find_cycles(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert!(
            findings[0].message.contains("break it by extracting"),
            "{}",
            findings[0].message
        );
        assert_eq!(
            participants.len(),
            2,
            "the idiomatic-language file still sits in a reported cycle"
        );
    }

    #[test]
    fn a_mixed_idiomatic_and_impossible_cycle_emits_nothing() {
        let graph = one_package_graph(
            vec![file_lang("a.rs", 0, "idio"), file_lang("b.go", 0, "imp")],
            vec![imports(0, 1), imports(1, 0)],
        )
        .with_cycle_policies(vec![
            (
                SmolStr::new("idio"),
                CyclePolicy {
                    file_cycles: CycleTolerance::Idiomatic,
                    package_cycles: CycleTolerance::Idiomatic,
                },
            ),
            (
                SmolStr::new("imp"),
                CyclePolicy {
                    file_cycles: CycleTolerance::Impossible,
                    package_cycles: CycleTolerance::Impossible,
                },
            ),
        ]);
        let (findings, participants) = find_cycles(&graph);
        assert!(findings.is_empty());
        assert!(participants.is_empty());
    }

    #[test]
    fn hazard_tolerance_keeps_the_prescriptive_advice() {
        let graph = one_package_graph(
            vec![file("a.ts", 0), file("b.ts", 0)],
            vec![imports(0, 1), imports(1, 0)],
        );
        let findings = find_cycles(&graph).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert!(
            findings[0].message.contains("break it by extracting"),
            "{}",
            findings[0].message
        );
    }

    #[test]
    fn an_all_generated_cycle_is_exempt() {
        let mut a = file("gen/a.ts", 0);
        let mut b = file("gen/b.ts", 0);
        for f in [&mut a, &mut b] {
            f.class = Some(FileClass {
                role: FileRole::Production,
                origin: FileOrigin::Generated,
            });
        }
        let graph = one_package_graph(vec![a, b], vec![imports(0, 1), imports(1, 0)]);
        assert!(find_cycles(&graph).0.is_empty());
    }

    fn real_package(name: &str) -> PackageNode {
        PackageNode {
            workspace_entry: None,
            targets: Vec::new(),
            executables: Vec::new(),
            manifest: Some(ProjectPath(SmolStr::new(format!("{name}/package.json")))),
            name: Some(SmolStr::new(name)),
            private: false,
            declares_surface: false,
            surface: Vec::new(),
            resolves_dependency_usage: true,
            manifest_claim_languages: Vec::new(),
        }
    }

    fn implicit_package() -> PackageNode {
        PackageNode {
            workspace_entry: None,
            targets: Vec::new(),
            executables: Vec::new(),
            manifest: None,
            name: None,
            private: false,
            declares_surface: false,
            surface: Vec::new(),
            resolves_dependency_usage: true,
            manifest_claim_languages: Vec::new(),
        }
    }

    #[test]
    fn a_cross_package_cycle_reports_at_package_level_only() {
        // a (pkg 1) ⇄ b (pkg 2): the file loop spans two real packages — one package finding,
        // no file finding (the package finding is its rollup).
        let graph = ProjectGraph::for_test(
            vec![file("p/a.ts", 1), file("q/b.ts", 2)],
            vec![],
            vec![],
            vec![imports(0, 1), imports(1, 0)],
        )
        .with_packages(vec![
            implicit_package(),
            real_package("p"),
            real_package("q"),
        ]);
        let findings = find_cycles(&graph).0;
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].subject_kind, "package");
        assert_eq!(findings[0].severity, Severity::Warning);
        assert!(findings[0].message.contains("2 packages"));
        assert_eq!(findings[0].related.len(), 2);
    }

    #[test]
    fn a_package_cycle_without_a_file_cycle_is_still_found() {
        // P → Q via a→x, Q → P via y→b: no file-level SCC, but the package graph loops.
        let graph = ProjectGraph::for_test(
            vec![
                file("p/a.ts", 1),
                file("p/b.ts", 1),
                file("q/x.ts", 2),
                file("q/y.ts", 2),
            ],
            vec![],
            vec![],
            vec![imports(0, 2), imports(3, 1)],
        )
        .with_packages(vec![
            implicit_package(),
            real_package("p"),
            real_package("q"),
        ]);
        let findings = find_cycles(&graph).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].subject_kind, "package");
    }

    #[test]
    fn a_cycle_touching_the_implicit_package_stays_file_level() {
        // a (implicit) ⇄ b (real pkg): no package finding exists to roll into.
        let graph = ProjectGraph::for_test(
            vec![file("a.ts", 0), file("p/b.ts", 1)],
            vec![],
            vec![],
            vec![imports(0, 1), imports(1, 0)],
        )
        .with_packages(vec![implicit_package(), real_package("p")]);
        let findings = find_cycles(&graph).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].subject_kind, "file");
    }

    #[test]
    fn confidence_is_the_weakest_edge_on_the_evidence_path() {
        let mut weak = imports(1, 0);
        weak.confidence = Confidence::Possible;
        let graph = one_package_graph(
            vec![file("a.ts", 0), file("b.ts", 0)],
            vec![imports(0, 1), weak],
        );
        let findings = find_cycles(&graph).0;
        assert_eq!(findings[0].confidence, Confidence::Possible);
    }

    #[test]
    fn finding_id_is_stable_and_position_free() {
        let g1 = one_package_graph(
            vec![file("a.ts", 0), file("b.ts", 0)],
            vec![imports(0, 1), imports(1, 0)],
        );
        // Same cycle, files declared in a different order (different FileIds).
        let g2 = one_package_graph(
            vec![file("b.ts", 0), file("a.ts", 0)],
            vec![imports(0, 1), imports(1, 0)],
        );
        assert_eq!(find_cycles(&g1).0[0].id, find_cycles(&g2).0[0].id);
    }

    #[test]
    fn three_node_cycle_reports_the_shortest_loop_as_evidence() {
        // a → b → c → a plus a shortcut b → a: the anchor's shortest loop is 2 hops, and the
        // finding still counts all 3 participants.
        let graph = one_package_graph(
            vec![file("a.ts", 0), file("b.ts", 0), file("c.ts", 0)],
            vec![imports(0, 1), imports(1, 2), imports(2, 0), imports(1, 0)],
        );
        let findings = find_cycles(&graph).0;
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("3 files"));
        assert!(
            findings[0].related.len() <= 3,
            "evidence is a shortest loop, not the whole component"
        );
    }
}
