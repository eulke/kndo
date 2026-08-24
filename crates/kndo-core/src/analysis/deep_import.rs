//! `deep-import` — contract-gated boundary erosion (group `risk`): an import
//! that bypasses a provider package's *declared* entry-point surface depends on internal file
//! layout someone explicitly drew a boundary around. Three design rules:
//!
//! 1. **Contract-gated, zero-config, self-opting**: fires only when the provider
//!    `declares_surface` (an `exports` map or equivalent — `PackageNode::declares_surface`).
//!    No declared surface = no declared boundary = no finding, so monorepos where sibling deep
//!    imports are accepted practice never see noise, and open-subpath packages (`lodash/fp`)
//!    are never accused. Boundaries enforced *unconditionally at build time* (Go `internal/`)
//!    never reach here at all — Go declares no surface, so the gate stays closed for the
//!    language whose compiler already does this job.
//! 2. **One finding per (consumer package → provider package) pair**, subject `package` —
//!    that pair is the unit a migration is planned in; N line-level findings are not. Sites
//!    are listed in the message (capped, elided counted).
//! 3. **Computed remediation, by case**: kndo has the graph, so the finding says which case
//!    the touched symbols are. A symbol whose owning file is reachable from the provider's own
//!    surface (through the provider's internal import/re-export chain) is also available
//!    publicly — "switch the specifier"; one that isn't is genuinely internal — "add the
//!    subpath to the surface, or extract it". The mechanism is file-granular (a re-export
//!    produces an `ImportsFile` edge from the entry to the defining file, so
//!    entry-reachability of the owning file is the computable proxy for "re-exported
//!    publicly"); per-symbol export-surface tracking can only sharpen it later.
//!
//! **External providers** (a dependency's `dist/internal/x`): same definition, but the gate
//! needs the *provider's own manifest*, which lives outside the discovered tree
//! (`node_modules/` is not walked — a discovery bound), so `declares_surface` is
//! simply unknowable and the gate stays closed — the design's own safe direction, not a
//! special case. Evaluating it would need a provider-manifest peek at resolution time;
//! honestly absent, not silently half-done here.
//!
//! Severity: warning — the gate means the provider explicitly declared the contract being
//! bypassed. Confidence: the strongest underlying edge's (a pair backed by one `Certain` deep
//! edge is certainly a deep-importing pair, however many `Possible` edges ride along).

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::{BTreeMap, VecDeque};

use crate::analysis::{finding_id, package_discriminator, package_label};
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, EdgeKind, FileId, FileOrigin, NodeRef, PackageId};

struct PairEvidence {
    /// Deep sites, as (consumer file, provider target file) — deduped, sorted for output.
    sites: Vec<(FileId, FileId)>,
    confidence: Confidence,
}

pub fn find_deep_imports(graph: &ProjectGraph) -> Vec<Finding> {
    // BTreeMap: deterministic pair order without a post-sort on ids alone.
    let mut pairs: BTreeMap<(u32, u32), PairEvidence> = BTreeMap::new();
    let mut seen_sites: HashSet<(u32, u32, u32, u32)> = HashSet::default();

    for edge in &graph.edges {
        let EdgeKind::ImportsFile { from, to } = edge.kind else {
            continue;
        };
        let consumer_file = &graph.files[from.0 as usize];
        let provider_file = &graph.files[to.0 as usize];
        let consumer = consumer_file.package;
        let provider = provider_file.package;
        if consumer == provider {
            continue; // intra-package imports are the package's own business
        }
        let provider_pkg = &graph.packages[provider.0 as usize];
        if !provider_pkg.declares_surface {
            continue; // no declared boundary — the contract gate (rule 1)
        }
        if provider_pkg.surface.contains(&to) {
            continue; // lands on the declared surface — the sanctioned path
        }
        let Some(class) = consumer_file.class else {
            continue; // unclaimed consumer — out of scope, not a verdict
        };
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue; // generated/vendored consumers are not accused (codebase-wide rule)
        }

        let key = (consumer.0, provider.0);
        let entry = pairs.entry(key).or_insert(PairEvidence {
            sites: Vec::new(),
            confidence: edge.confidence,
        });
        entry.confidence = entry.confidence.max(edge.confidence);
        if seen_sites.insert((consumer.0, provider.0, from.0, to.0)) {
            entry.sites.push((from, to));
        }
    }

    let mut findings = Vec::new();
    for ((consumer, provider), mut evidence) in pairs {
        let consumer = PackageId(consumer);
        let provider = PackageId(provider);
        evidence.sites.sort_by(|a, b| {
            let pa = (
                graph.files[a.0 .0 as usize].path.0.as_str(),
                graph.files[a.1 .0 as usize].path.0.as_str(),
            );
            let pb = (
                graph.files[b.0 .0 as usize].path.0.as_str(),
                graph.files[b.1 .0 as usize].path.0.as_str(),
            );
            pa.cmp(&pb)
        });

        let deep_targets: HashSet<FileId> = evidence.sites.iter().map(|&(_, t)| t).collect();
        let reachable = surface_reachable_files(graph, provider);
        let (public_syms, internal_syms) =
            touched_symbols(graph, consumer, &deep_targets, &reachable);

        let provider_name = package_label(graph, provider);
        let consumer_name = package_label(graph, consumer);
        let touched = public_syms + internal_syms;

        let remediation = if internal_syms == 0
            && (touched > 0 || all_reachable(&deep_targets, &reachable))
        {
            format!(
                "everything touched is also reachable through {provider_name}'s public surface — switch the specifiers to import {provider_name} directly"
            )
        } else {
            format!(
                "the internals are not reachable through {provider_name}'s public surface — add the subpaths to its declared surface (exports map), or extract them into a shared package"
            )
        };
        let sites_list = site_summary(graph, &evidence.sites);
        let symbols_clause = if touched > 0 {
            format!(", touching {internal_syms} internal symbol(s) ({public_syms} also public)")
        } else {
            String::new()
        };

        let consumer_disc = package_discriminator(graph, consumer);
        let provider_disc = package_discriminator(graph, provider);
        findings.push(Finding {
            advisory: false,
            id: finding_id(
                "deep-import",
                "package",
                &consumer_disc,
                &provider_disc,
                "",
            ),
            category: "deep-import".to_string(),
            group: "risk".to_string(),
            subject_kind: "package".to_string(),
            severity: Severity::Warning,
            confidence: evidence.confidence,
            message: format!(
                "{consumer_name} deep-imports {provider_name} at {} site(s){symbols_clause}, bypassing its declared surface ({sites_list}) — {remediation}",
                evidence.sites.len(),
            ),
            location: Location {
                // The consumer's manifest — the file where the dependency relationship is
                // owned and where a migration starts (same anchor choice as `undeclared`).
                path: graph.packages[consumer.0 as usize].manifest.clone(),
                range: None,
                symbol: Some(provider_name.clone()),
                package: graph.package_name(consumer).map(str::to_string),
            },
            related: Vec::new(),
            delta: None,
            delta_origin: None,
        });
    }
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    findings
}

/// Provider files reachable from the provider's declared surface through its own
/// `ImportsFile` edges — the file-granular "available through the public entry" set (module
/// doc, rule 3): a re-export chain from the entry produces exactly these edges.
fn surface_reachable_files(graph: &ProjectGraph, provider: PackageId) -> HashSet<FileId> {
    let mut adjacency: HashMap<FileId, Vec<FileId>> = HashMap::default();
    for edge in &graph.edges {
        if let EdgeKind::ImportsFile { from, to } = edge.kind {
            if graph.files[from.0 as usize].package == provider
                && graph.files[to.0 as usize].package == provider
            {
                adjacency.entry(from).or_default().push(to);
            }
        }
    }
    let mut reachable: HashSet<FileId> = HashSet::default();
    let mut queue: VecDeque<FileId> = graph.packages[provider.0 as usize]
        .surface
        .iter()
        .copied()
        .collect();
    while let Some(file) = queue.pop_front() {
        if !reachable.insert(file) {
            continue;
        }
        if let Some(next) = adjacency.get(&file) {
            queue.extend(next.iter().copied());
        }
    }
    reachable
}

/// Counts provider symbols the consumer package actually references inside the deep-imported
/// files, split into (also-reachable-via-surface, genuinely-internal) by their owning file's
/// membership in `reachable`.
fn touched_symbols(
    graph: &ProjectGraph,
    consumer: PackageId,
    deep_targets: &HashSet<FileId>,
    reachable: &HashSet<FileId>,
) -> (usize, usize) {
    let mut public = HashSet::default();
    let mut internal = HashSet::default();
    for edge in &graph.edges {
        let EdgeKind::References { from, to, .. } = edge.kind else {
            continue;
        };
        let origin = match from {
            NodeRef::File(f) => f,
            NodeRef::Symbol(s) => graph.symbols[s.0 as usize].file,
        };
        if graph.files[origin.0 as usize].package != consumer {
            continue;
        }
        let symbol_file = graph.symbols[to.0 as usize].file;
        if !deep_targets.contains(&symbol_file) {
            continue;
        }
        if reachable.contains(&symbol_file) {
            public.insert(to);
        } else {
            internal.insert(to);
        }
    }
    (public.len(), internal.len())
}

fn all_reachable(deep_targets: &HashSet<FileId>, reachable: &HashSet<FileId>) -> bool {
    deep_targets.iter().all(|t| reachable.contains(t))
}

/// `consumer.ts → provider/internal.ts` pairs, capped at 3 with an elided count
/// ("sites and symbols in the evidence (capped, elided counted)").
fn site_summary(graph: &ProjectGraph, sites: &[(FileId, FileId)]) -> String {
    let rendered: Vec<String> = sites
        .iter()
        .take(3)
        .map(|&(f, t)| {
            format!(
                "{} → {}",
                graph.files[f.0 as usize].path.0, graph.files[t.0 as usize].path.0
            )
        })
        .collect();
    if sites.len() > 3 {
        format!("{}, +{} more", rendered.join(", "), sites.len() - 3)
    } else {
        rendered.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::graph::{FileNode, PackageNode, SymbolNode};
    use crate::vocab::{Edge, FileClass, FileRole, Provenance, SymbolKind};
    use smol_str::SmolStr;

    fn file(path: &str, package: u32) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
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

    fn package(name: &str, declares_surface: bool, surface: Vec<FileId>) -> PackageNode {
        PackageNode {
            workspace_entry: None,
            targets: Vec::new(),
            executables: Vec::new(),
            manifest: Some(ProjectPath(SmolStr::new(format!("{name}/package.json")))),
            name: Some(SmolStr::new(name)),
            private: false,
            declares_surface,
            surface,
            resolves_dependency_usage: true,
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
            span: None,
        }
    }

    /// Two packages: consumer (1) with app.ts; provider (2) with entry.ts (surface) and
    /// internal.ts.
    fn two_package_graph(
        declares_surface: bool,
        edges: Vec<Edge>,
        symbols: Vec<SymbolNode>,
    ) -> ProjectGraph {
        let files = vec![
            file("app/main.ts", 1),
            file("ui/entry.ts", 2),
            file("ui/internal.ts", 2),
        ];
        ProjectGraph::for_test(files, symbols, vec![], edges).with_packages(vec![
            implicit_package(),
            package("app", false, Vec::new()),
            package("ui", declares_surface, vec![FileId(1)]),
        ])
    }

    #[test]
    fn deep_import_into_a_declared_surface_package_is_a_finding() {
        let graph = two_package_graph(true, vec![imports(0, 2)], vec![]);
        let findings = find_deep_imports(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "deep-import");
        assert_eq!(findings[0].group, "risk");
        assert_eq!(findings[0].subject_kind, "package");
        assert_eq!(findings[0].severity, Severity::Warning);
        assert!(findings[0].message.contains("app deep-imports ui"));
        assert!(findings[0].message.contains("app/main.ts → ui/internal.ts"));
    }

    #[test]
    fn no_declared_surface_means_no_finding() {
        // The contract gate (rule 1): same deep edge, provider declares nothing.
        let graph = two_package_graph(false, vec![imports(0, 2)], vec![]);
        assert!(find_deep_imports(&graph).is_empty());
    }

    #[test]
    fn importing_the_surface_itself_is_the_sanctioned_path() {
        let graph = two_package_graph(true, vec![imports(0, 1)], vec![]);
        assert!(find_deep_imports(&graph).is_empty());
    }

    #[test]
    fn intra_package_deep_paths_are_the_package_s_own_business() {
        let graph = two_package_graph(true, vec![imports(1, 2)], vec![]);
        assert!(find_deep_imports(&graph).is_empty());
    }

    #[test]
    fn many_sites_roll_up_to_one_pair_finding_with_elision() {
        let files = vec![
            file("app/a.ts", 1),
            file("app/b.ts", 1),
            file("app/c.ts", 1),
            file("app/d.ts", 1),
            file("ui/entry.ts", 2),
            file("ui/internal.ts", 2),
        ];
        let edges = vec![imports(0, 5), imports(1, 5), imports(2, 5), imports(3, 5)];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges).with_packages(vec![
            implicit_package(),
            package("app", false, Vec::new()),
            package("ui", true, vec![FileId(4)]),
        ]);
        let findings = find_deep_imports(&graph);
        assert_eq!(findings.len(), 1, "one finding per pair, not per site");
        assert!(findings[0].message.contains("4 site(s)"));
        assert!(findings[0].message.contains("+1 more"));
    }

    #[test]
    fn remediation_says_switch_when_the_deep_file_is_surface_reachable() {
        // entry.ts re-exports internal.ts (an intra-provider ImportsFile edge), so everything
        // deep-imported is also available through the public surface.
        let graph = two_package_graph(true, vec![imports(0, 2), imports(1, 2)], vec![]);
        let findings = find_deep_imports(&graph);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].message.contains("switch the specifiers"),
            "{}",
            findings[0].message
        );
    }

    #[test]
    fn remediation_says_export_or_extract_when_genuinely_internal() {
        let graph = two_package_graph(true, vec![imports(0, 2)], vec![]);
        let findings = find_deep_imports(&graph);
        assert!(
            findings[0].message.contains("add the subpaths"),
            "{}",
            findings[0].message
        );
    }

    #[test]
    fn touched_symbols_are_counted_and_split_by_surface_reachability() {
        let symbols = vec![SymbolNode {
            file: FileId(2),
            name: SmolStr::new("secretFn"),
            kind: SymbolKind::Function,
            span: Default::default(),
            exported: true,
            visibility: crate::adapter::VisibilityLevel(1),
            member_of: None,
            signature_span: None,
            implicitly_invoked: false,
            nested_scope: false,
            visibility_inherited: false,
        }];
        let mut edges = vec![imports(0, 2)];
        edges.push(Edge {
            owner: crate::vocab::FileId(0),
            kind: EdgeKind::References {
                from: NodeRef::File(FileId(0)),
                to: crate::vocab::SymbolId(0),
                kind: crate::vocab::RefKind::Read,
            },
            confidence: Confidence::Certain,
            source: Provenance::Adapter(SmolStr::new("mock")),
            span: None,
        });
        let graph = two_package_graph(true, edges, symbols);
        let findings = find_deep_imports(&graph);
        assert!(
            findings[0]
                .message
                .contains("1 internal symbol(s) (0 also public)"),
            "{}",
            findings[0].message
        );
    }

    #[test]
    fn possible_confidence_edges_yield_a_possible_finding() {
        let mut edge = imports(0, 2);
        edge.confidence = Confidence::Possible;
        let graph = two_package_graph(true, vec![edge], vec![]);
        let findings = find_deep_imports(&graph);
        assert_eq!(findings[0].confidence, Confidence::Possible);
    }

    #[test]
    fn generated_consumers_are_not_accused() {
        let mut consumer = file("app/gen.ts", 1);
        consumer.class = Some(FileClass {
            role: FileRole::Production,
            origin: FileOrigin::Generated,
        });
        let files = vec![consumer, file("ui/entry.ts", 2), file("ui/internal.ts", 2)];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![imports(0, 2)])
            .with_packages(vec![
                implicit_package(),
                package("app", false, Vec::new()),
                package("ui", true, vec![FileId(1)]),
            ]);
        assert!(find_deep_imports(&graph).is_empty());
    }

    #[test]
    fn finding_id_is_stable_per_pair() {
        let graph = two_package_graph(true, vec![imports(0, 2)], vec![]);
        let a = find_deep_imports(&graph);
        let b = find_deep_imports(&graph);
        assert_eq!(a[0].id, b[0].id);
    }
}
