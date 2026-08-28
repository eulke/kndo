//! `duplicate` — both halves. **Exact file duplicates**: byte-identical files
//! (subject `file`, below). **Structural clones** (`find_duplicate_functions`): Type-1/Type-2
//! callable-body clones over the adapters' winnowing fingerprints
//! (`ProjectGraph::function_metrics` — normalized token streams, so reformatting, comments,
//! and renamed identifiers/literals don't hide a copy), grouped transitively by Jaccard
//! similarity through a shared-fingerprint index, same-language only, one `info` finding per
//! group with every instance in `related`. Generated/vendored files are exempt from the
//! structural half (a generator copying itself is its own business), and so are test-role
//! files and sub-file test regions (structural clones target production code — test-shape
//! symmetry is expected, the same exemption `crap`/`untested` apply); neither carve-out
//! touches the exact half — see below for why.
//!
//! Exact half: byte-identical files already share the
//! blake3 content hash discovery computes for the cache, so this is free: no extraction, no
//! adapter needed at all. That last point matters — this is the one analysis that runs over
//! *unclaimed* files too (images, binaries, configs no adapter recognizes), because those are
//! exactly what token-based clone detection can never see and the exact half's stated target
//! ("copy-pasted configs, images, and any other asset"). One finding groups every copy of one
//! content, not one finding per pair.
//!
//! Deliberately no Generated/Vendored exemption here, unlike
//! `unused`/`test-only`: the byte-identical rule has no such carve-out, and origin isn't even known
//! for the unclaimed files this analysis exists to cover (`FileClass` requires a claim). The
//! one floor applied — empty files — isn't a policy carve-out either: "duplicate content" is
//! vacuous when there's no content, and virtually every real repo has many genuinely-empty
//! files (`.gitkeep`, stub configs) that would otherwise collapse into one giant,
//! unactionable finding spanning unrelated directories.

use rustc_hash::FxHashMap as HashMap;

use crate::analysis::{finding_id, FindingIdParts};
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{Category, Confidence, Group, SubjectKind, SymbolId};

pub fn find_duplicate_files(graph: &ProjectGraph) -> Vec<Finding> {
    let empty_hash: [u8; 32] = blake3::hash(b"").into();

    let mut by_hash: HashMap<[u8; 32], Vec<&str>> = HashMap::default();
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
        // "the same duplicate-content situation," not a new finding (unlike most other
        // findings, which anchor to one file's own path).
        let discriminator = blake3::Hash::from(hash).to_hex().to_string();
        findings.push(Finding {
            advisory: false,
            id: finding_id(FindingIdParts {
                category: &Category::DUPLICATE,
                subject_kind: &SubjectKind::FILE,
                path: "",
                symbol_path: "",
                discriminator: &discriminator,
            }),
            category: Category::DUPLICATE,
            group: Group::Waste,
            subject_kind: SubjectKind::FILE,
            severity: Severity::Info, // info by default — duplication is sometimes deliberate
            confidence: Confidence::Certain,
            message: format!(
                "{} identical files share the same content: {}",
                paths.len(),
                summarize(&paths)
            ),
            // Anchored on the lexicographically-first copy, with every copy — that one
            // included — in `related`: the same shape the symbol facet below uses, so a
            // consumer reads one convention for both. The message may summarize; `related`
            // never does, which is what makes the summary safe.
            //
            // The anchor is presentation, NOT identity: `id` above stays keyed on the content
            // hash with an empty path, so renaming one copy while the group survives is still
            // the same finding rather than a new one.
            location: Location {
                path: Some(crate::adapter::ProjectPath(smol_str::SmolStr::new(
                    paths[0],
                ))),
                ..Location::default()
            },
            related: paths
                .iter()
                .map(|path| crate::engine::RelatedLocation {
                    role: "clone".to_string(),
                    path: crate::adapter::ProjectPath(smol_str::SmolStr::new(*path)),
                    range: None,
                    note: None,
                })
                .collect(),
            rolled_up: None,
            sources: Vec::new(),
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

// ---------------------------------------------------------------- structural clones

/// Two callables are clones when their winnowing fingerprint sets overlap this much
/// (Jaccard). 0.8 catches Type-1/Type-2 clones with light drift while a genuinely different
/// function body — even one solving a similar problem — falls far below it.
const CLONE_JACCARD: f64 = 0.8;

/// A fingerprint shared by more than this many callables is boilerplate shape (getters,
/// trivial delegations), not copy-paste evidence — its postings list is skipped when pairing
/// candidates, keeping the candidate set near-linear instead of quadratic on common shapes.
const MAX_POSTING: usize = 20;

/// Structural Type-1/Type-2 clones over `ProjectGraph::function_metrics`:
/// candidates pair through a shared-fingerprint index (same language only), confirm by
/// Jaccard similarity, and group transitively — one `info` finding per clone group, every
/// instance in `related`.
/// Findings plus the redundant clone instances — every group member beyond its
/// lexicographically-first canonical one, with its normalized token count. `health`'s
/// "duplicated tokens" numerator: the canonical copy is the one you'd keep, so
/// only the copies beyond it count as duplicated.
pub fn find_duplicate_functions(
    graph: &ProjectGraph,
    min_tokens: u32,
) -> (Vec<Finding>, Vec<(SymbolId, u32)>) {
    use crate::vocab::{Category, FileOrigin, Group, SubjectKind};
    use rustc_hash::FxHashSet as HashSet;

    // Eligible instances: fingerprinted callables in authored, claimed files.
    struct Instance<'g> {
        symbol: SymbolId,
        /// The SHAPE's own extent and ordinal — a closure has no `SymbolNode`, so these are
        /// the only things separating two clones that live inside one function.
        shape_span: crate::vocab::Span,
        shape_ordinal: u16,
        path: &'g str,
        language: &'g str,
        token_count: u32,
        fingerprints: &'g [u64],
    }
    let mut instances: Vec<Instance> = Vec::new();
    for (symbol_id, metrics) in &graph.function_metrics {
        if metrics.fingerprints.is_empty() {
            continue; // under the extraction floor — too small to meaningfully clone-match
        }
        if metrics.token_count < min_tokens {
            continue; // under the configured min-tokens gate (only ever at or above the
                      // extraction floor — smaller functions carry no fingerprints at all)
        }
        let symbol = &graph.symbols[symbol_id.0 as usize];
        let file = &graph.files[symbol.file.0 as usize];
        let Some(class) = file.class else { continue };
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue;
        }
        // Structural clones target production code: test files — and sub-file test regions
        // inside production files — are exempt, the same two-level exemption `crap` and
        // `untested` apply. Parallel arrange-act-assert bodies across a fixture matrix are
        // the *point* of table-shaped tests, not waste; filtering here (the fingerprinting
        // stage) also keeps test tokens out of the postings index and out of health's
        // duplicated-tokens numerator.
        if class.role == crate::vocab::FileRole::Test {
            continue;
        }
        if crate::graph::span_in_test_region(&file.test_spans, symbol.span) {
            continue;
        }
        // Exempt by provenance of the SHAPE, where the exemptions above are by provenance of
        // the file. Structural clones are evidence of copy-paste; a body that only constructs
        // a value has no such evidence to give. The fingerprint's normalization inverts there:
        // it erases the field VALUES — the entire authored content — and keeps the field list,
        // which the type declaration dictates. Every construction of one type therefore groups
        // with every other, a false family whose size grows with how CENTRAL the type is, and
        // acting on that advice is what once produced a constructor hiding a contract struct's
        // field list. `MAX_POSTING` already concedes the same point for shapes shared 20+ ways,
        // using popularity as the proxy; this names the cause instead of counting.
        //
        // Not configurable, exactly like the exemptions above: `min-tokens` is a floor on size,
        // and this is not a question of size.
        if metrics.body_is_construction {
            continue;
        }
        let Some(language) = file.language.as_deref() else {
            continue;
        };
        instances.push(Instance {
            symbol: *symbol_id,
            shape_span: metrics.shape_span,
            shape_ordinal: metrics.shape_ordinal,
            path: file.path.0.as_str(),
            language,
            token_count: metrics.token_count,
            fingerprints: &metrics.fingerprints,
        });
    }

    // Shared-fingerprint index → candidate pairs (same language), then Jaccard-confirm.
    let mut postings: HashMap<(&str, u64), Vec<usize>> = HashMap::default();
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
    let mut checked: HashSet<(usize, usize)> = HashSet::default();
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

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::default();
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
        // (path, qualified, shape start) per instance, lexicographic — the first is the
        // anchor. The third component is not decoration: two closures inside one function
        // share a path AND a qualified name, and without it their order would depend on
        // discovery, breaking the `--threads 1` determinism gate.
        let mut named: Vec<(String, String, (u32, u32), usize)> = members
            .iter()
            .map(|&i| {
                let symbol = &graph.symbols[instances[i].symbol.0 as usize];
                (
                    instances[i].path.to_string(),
                    symbol.qualified_name(),
                    instances[i].shape_span.start,
                    i,
                )
            })
            .collect();
        named.sort();
        for (_, _, _, i) in &named[1..] {
            duplicated.push((instances[*i].symbol, instances[*i].token_count));
        }
        // The finding's IDENTITY. A nested shape appends its ordinal — stable under every edit
        // above it, unlike the line — and ordinal 0 appends nothing, so every group that
        // existed before closures were split keeps its id byte-for-byte.
        let selectors: Vec<String> = named
            .iter()
            .map(|(p, q, _, i)| match instances[*i].shape_ordinal {
                0 => format!("{p}#{q}"),
                n => format!("{p}#{q}#nested{n}"),
            })
            .collect();
        // What the reader sees. An ordinal is the right thing to put in an ID and the wrong
        // thing to show a human — "the second closure in `wire`" is not something you can go
        // and look at — so a nested shape is shown by the line it starts on instead. The same
        // split W7b made one level up: identity stays stable, prose becomes findable.
        let displayed: Vec<String> = named
            .iter()
            .map(|(p, q, start, i)| match instances[*i].shape_ordinal {
                0 => format!("{p}#{q}"),
                _ => format!("{p}#{q}:{}", start.0),
            })
            .collect();
        // What the MESSAGE shows. Two identical members of one type declared in different
        // impl blocks (a trait's and the type's own, or two `#[cfg]` alternates) produce the
        // same `path#Owner.name` twice, and the reader cannot tell which is which — while
        // `related` below has carried their distinct spans all along.
        //
        // The distinguisher goes here and NOT into `selectors`, which is the finding's
        // identity: a line number in an id churns the baseline every time anything above the
        // clone moves. Identity stays stable, prose becomes readable.
        let group_symbols: Vec<&crate::graph::SymbolNode> = named
            .iter()
            .map(|(_, _, _, i)| &graph.symbols[instances[*i].symbol.0 as usize])
            .collect();
        let group_starts: Vec<(u32, u32)> = named.iter().map(|(_, _, start, _)| *start).collect();
        let labels = disambiguated(&displayed, &group_symbols, &group_starts);
        let (_, anchor_name, _, anchor_idx) = &named[0];
        let anchor_symbol = &graph.symbols[instances[*anchor_idx].symbol.0 as usize];
        let anchor_file = &graph.files[anchor_symbol.file.0 as usize];
        let facet = anchor_symbol.kind.facet().to_string();

        let related = named
            .iter()
            .map(|(_, q, _, i)| {
                let s = &graph.symbols[instances[*i].symbol.0 as usize];
                crate::engine::RelatedLocation {
                    role: "clone".to_string(),
                    path: graph.files[s.file.0 as usize].path.clone(),
                    // The shape's range, not the symbol's: two closures in one function would
                    // otherwise both point at the function's opening line.
                    range: Some(instances[*i].shape_span),
                    note: Some(q.clone()),
                }
            })
            .collect();

        let shown: Vec<&str> = labels.iter().map(String::as_str).collect();
        findings.push(Finding {
            advisory: false,
            id: finding_id(FindingIdParts {
                category: &Category::DUPLICATE,
                subject_kind: &SubjectKind::new(facet.as_str()),
                path: "",
                symbol_path: "",
                discriminator: &selectors.join("\u{1}"),
            }),
            category: Category::DUPLICATE,
            group: Group::Waste,
            subject_kind: SubjectKind::new(facet.as_str()),
            severity: Severity::Info, // info by default — duplication is sometimes deliberate
            confidence: Confidence::Certain,
            message: format!(
                "{} structurally identical {facet}s (identifiers/literals aside): {} — extract the shared implementation",
                named.len(),
                summarize(&shown),
            ),
            location: Location {
                path: Some(anchor_file.path.clone()),
                range: Some(instances[*anchor_idx].shape_span),
                symbol: Some(anchor_name.clone()),
                package: graph.package_name(anchor_file.package).map(str::to_string),
            },
            related,
            rolled_up: None,
            sources: Vec::new(),
            delta: None,
            delta_origin: None,
        });
    }
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    duplicated.sort();
    (findings, duplicated)
}

/// Display labels for one group's instances: the plain selector where it is already unique,
/// and a distinguisher appended where it is not. `symbols` is aligned with `selectors`.
///
/// The distinguisher is the trait whose implementation declares the member — the fact that
/// tells a reader what they actually want to know ("the `Serialize` one, not the inherent
/// one") and that survives every edit above it. Where the graph has no trait to name, or
/// where both instances share one (two `#[cfg]` alternates of the same impl), it falls back
/// to `starts` — each shape's OWN start line, which always separates them. For a declaration's
/// own shape that is its declaration line, exactly as before closures were split out; for a
/// nested callable it is the only thing that can separate it from its siblings.
fn disambiguated(
    selectors: &[String],
    symbols: &[&crate::graph::SymbolNode],
    starts: &[(u32, u32)],
) -> Vec<String> {
    let collides = |i: usize| selectors.iter().filter(|s| *s == &selectors[i]).count() > 1;
    let with_trait = |i: usize| {
        symbols[i]
            .implements
            .as_ref()
            .map(|t| format!("{} (impl {t})", selectors[i]))
    };

    (0..selectors.len())
        .map(|i| {
            if !collides(i) {
                return selectors[i].clone();
            }
            // The trait only helps if it separates this instance from every other one
            // sharing its plain selector.
            let mine = with_trait(i);
            let separates = mine.is_some()
                && !(0..selectors.len())
                    .any(|j| j != i && selectors[j] == selectors[i] && with_trait(j) == mine);
            match (mine, separates) {
                (Some(label), true) => label,
                _ => format!("{}:{}", selectors[i], starts[i].0),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::graph::FileNode;
    use crate::vocab::Span;
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
            unit_parent: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
            string_attr_args: Vec::new(),
        }
    }

    #[test]
    fn two_identical_files_produce_one_finding() {
        let files = vec![file("a.png", b"same bytes"), file("b.png", b"same bytes")];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let findings = find_duplicate_files(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "duplicate");
        assert_eq!(findings[0].group, crate::vocab::Group::Waste);
        assert_eq!(findings[0].subject_kind, "file");
        assert!(findings[0].message.contains("a.png"));
        assert!(findings[0].message.contains("b.png"));
    }

    #[test]
    fn every_copy_is_addressable_without_reading_the_message() {
        // Five copies, so the message summarizes ("and 2 more") and the prose alone cannot
        // name them all. `related` must still carry every one, and the finding must have an
        // anchor: this is the whole difference between a finding an agent can act on and one
        // it has to parse English to understand.
        let files = vec![
            file("z/e.bin", b"same"),
            file("a/b.bin", b"same"),
            file("m/c.bin", b"same"),
            file("a/a.bin", b"same"),
            file("q/d.bin", b"same"),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let findings = find_duplicate_files(&graph);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];

        // The message truncates — which is fine, and exactly why the rest matters.
        assert!(f.message.contains("and 2 more"), "{}", f.message);

        // Anchor: the lexicographically-first copy, matching the symbol facet's convention.
        assert_eq!(
            f.location.path.as_ref().map(|p| p.0.as_str()),
            Some("a/a.bin")
        );

        // `related` names every copy, anchor included, in sorted order.
        let related: Vec<&str> = f.related.iter().map(|r| r.path.0.as_str()).collect();
        assert_eq!(
            related,
            vec!["a/a.bin", "a/b.bin", "m/c.bin", "q/d.bin", "z/e.bin"]
        );
        assert!(f.related.iter().all(|r| r.role == "clone"));
    }

    #[test]
    fn the_anchor_is_presentation_and_the_id_stays_content_keyed() {
        // Renaming one copy must not mint a new finding: the id is keyed on the content hash,
        // not on the anchor path, even though the anchor moves.
        let before = ProjectGraph::for_test(
            vec![file("a.bin", b"same"), file("b.bin", b"same")],
            vec![],
            vec![],
            vec![],
        );
        let after = ProjectGraph::for_test(
            vec![file("aaa.bin", b"same"), file("b.bin", b"same")],
            vec![],
            vec![],
            vec![],
        );
        let (before, after) = (find_duplicate_files(&before), find_duplicate_files(&after));
        assert_eq!(before[0].id, after[0].id);
        assert_ne!(before[0].location.path, after[0].location.path);
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

    // ------------------------------------------------- structural clones

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
            unit_parent: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
            string_attr_args: Vec::new(),
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
            implicitly_invoked: false,
            nested_scope: false,
            visibility_inherited: false,
            visible_in_unit: None,
            implements: None,
            markers: Vec::new(),
        }
    }

    /// A member of `owner` declared at `line`, optionally inside an `impl` of `trait_name`.
    fn member(
        file: u32,
        owner: &str,
        name: &str,
        line: u32,
        trait_name: Option<&str>,
    ) -> SymbolNode {
        SymbolNode {
            member_of: Some(SmolStr::new(owner)),
            implements: trait_name.map(SmolStr::new),
            span: crate::adapter::Span {
                start: (line, 1),
                end: (line + 4, 1),
            },
            kind: SymbolKind::Method,
            ..callable(file, name)
        }
    }

    /// A declaration's own shape at line 1 — matching `callable`'s span. A test whose symbols
    /// sit at meaningful lines must use `shape` and repeat them: in production a declaration's
    /// own `shape_span` IS its declaration span, and the labels depend on it.
    fn metrics(fingerprints: Vec<u64>) -> SymbolMetrics {
        shape(fingerprints, 0, (1, 1))
    }

    /// A declaration whose whole body is one value construction.
    fn construction(fingerprints: Vec<u64>) -> SymbolMetrics {
        SymbolMetrics {
            body_is_construction: true,
            ..shape(fingerprints, 0, (1, 1))
        }
    }

    /// A shape at an explicit position: `shape_ordinal` 0 is the declaration's own, 1..N a
    /// callable nested inside it — several of which may share one `SymbolId`.
    fn shape(fingerprints: Vec<u64>, shape_ordinal: u16, start: (u32, u32)) -> SymbolMetrics {
        SymbolMetrics {
            shape_span: Span {
                start,
                end: (start.0 + 3, 1),
            },
            shape_ordinal,
            cyclomatic: 2,
            loc: 5,
            token_count: 60,
            fingerprints,
            body_is_construction: false,
        }
    }

    #[test]
    fn construction_bodied_callables_are_not_clone_eligible() {
        // Two `descriptor()` methods whose whole body is one struct literal fingerprint alike
        // by definition of the type — normalization keeps the field list the type dictates and
        // erases the values, which are the entire authored content.
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.mock"), claimed_file("b.mock")],
            vec![callable(0, "descriptor"), callable(1, "descriptor")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), construction(vec![1, 2, 3])),
            (SymbolId(1), construction(vec![1, 2, 3])),
        ]);
        assert!(find_duplicate_functions(&graph, 0).0.is_empty());
    }

    #[test]
    fn a_body_that_constructs_and_also_branches_still_groups() {
        // The exemption is all-or-nothing on purpose: a function that constructs AND does work
        // has authored structure, and copy-paste of it is what `duplicate` exists to find.
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.mock"), claimed_file("b.mock")],
            vec![callable(0, "build"), callable(1, "make")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), metrics(vec![1, 2, 3])),
            (SymbolId(1), metrics(vec![1, 2, 3])),
        ]);
        assert_eq!(find_duplicate_functions(&graph, 0).0.len(), 1);
    }

    #[test]
    fn a_callback_passed_to_a_construction_is_still_reported() {
        // The pair this exemption must NOT swallow, and the reason it needs no narrowing
        // predicate: the constructing shapes are exempt, the callbacks they carry are their
        // own shapes and are not.
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.mock"), claimed_file("b.mock")],
            vec![callable(0, "wire_a"), callable(1, "wire_b")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            // The two constructing bodies are IDENTICAL — without the exemption they would
            // group too, and this assertion would see two findings instead of one.
            (SymbolId(0), construction(vec![9, 9])),
            (SymbolId(0), shape(vec![1, 2, 3], 1, (4, 9))),
            (SymbolId(1), construction(vec![9, 9])),
            (SymbolId(1), shape(vec![1, 2, 3], 1, (6, 9))),
        ]);
        let findings = find_duplicate_functions(&graph, 0).0;
        assert_eq!(
            findings.len(),
            1,
            "the constructions are exempt, the callbacks are not: {findings:#?}"
        );
        assert!(
            findings[0].message.contains("wire_a:4"),
            "{}",
            findings[0].message
        );
    }

    #[test]
    fn two_identical_closures_in_different_functions_group_on_the_closures() {
        // The case the split exists for: `Foo::new(|x| { …same forty tokens… })` at two sites.
        // Before it, the closure's tokens belonged to whichever function enclosed them, and
        // what grouped (if anything did) was the enclosing pair — never the thing copied.
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.mock"), claimed_file("b.mock")],
            vec![callable(0, "setup_a"), callable(1, "setup_b")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), shape(vec![9, 9], 0, (1, 1))),
            (SymbolId(0), shape(vec![1, 2, 3], 1, (4, 9))),
            (SymbolId(1), shape(vec![7, 7], 0, (1, 1))),
            (SymbolId(1), shape(vec![1, 2, 3], 1, (6, 9))),
        ]);
        let findings = find_duplicate_functions(&graph, 0).0;
        assert_eq!(findings.len(), 1, "{findings:#?}");
        let ranges: Vec<_> = findings[0]
            .related
            .iter()
            .filter_map(|r| r.range.map(|s| s.start))
            .collect();
        assert_eq!(
            ranges,
            vec![(4, 9), (6, 9)],
            "`related` must point at the closures, not at their owners' opening lines"
        );
    }

    #[test]
    fn two_closures_inside_one_function_stay_distinguishable() {
        // One symbol, two shapes: `path#name` is identical for both and the graph has no
        // trait to name, so the label falls back to each SHAPE's own line — the symbol's own
        // span would print the same number twice.
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.mock")],
            vec![callable(0, "wire")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), shape(vec![1, 2, 3], 1, (4, 9))),
            (SymbolId(0), shape(vec![1, 2, 3], 2, (12, 9))),
        ]);
        let findings = find_duplicate_functions(&graph, 0).0;
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert!(
            findings[0].message.contains("a.mock#wire:4"),
            "{}",
            findings[0].message
        );
        assert!(
            findings[0].message.contains("a.mock#wire:12"),
            "{}",
            findings[0].message
        );
    }

    #[test]
    fn a_groups_id_is_unchanged_when_no_shape_is_nested() {
        // Ordinal 0 appends nothing to the selector, so every clone group that existed before
        // closures were split out keeps its id — no baseline churns on this change.
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.mock"), claimed_file("b.mock")],
            vec![callable(0, "one"), callable(1, "two")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), metrics(vec![1, 2, 3])),
            (SymbolId(1), metrics(vec![1, 2, 3])),
        ]);
        let findings = find_duplicate_functions(&graph, 0).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].id,
            finding_id(FindingIdParts {
                category: &Category::DUPLICATE,
                subject_kind: &SubjectKind::new("function"),
                path: "",
                symbol_path: "",
                discriminator: "a.mock#one\u{1}b.mock#two",
            })
        );
    }

    #[test]
    fn colliding_labels_name_the_impl_block_they_came_from() {
        // Gap §5: one type, one method name, two impl blocks — the generated-vs-hand-written
        // split. Both instances used to print the same `path#Owner.name`, leaving the reader
        // no way to tell them apart.
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.rs")],
            vec![
                member(0, "View", "poll", 10, Some("Host")),
                member(0, "View", "poll", 40, None),
            ],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), shape(vec![1, 2, 3, 4, 5], 0, (10, 1))),
            (SymbolId(1), shape(vec![1, 2, 3, 4, 5], 0, (40, 1))),
        ]);
        let findings = find_duplicate_functions(&graph, 50).0;
        assert_eq!(findings.len(), 1);
        let m = &findings[0].message;
        assert!(
            m.contains("a.rs#View.poll (impl Host)"),
            "the trait is what a reader wants: {m}"
        );
        assert!(
            m.contains("a.rs#View.poll:40"),
            "the one with no trait to name falls back to its line: {m}"
        );
    }

    #[test]
    fn two_alternates_of_one_trait_impl_fall_back_to_their_lines() {
        // `#[cfg(unix)]` and `#[cfg(windows)]` impls of the SAME trait: the trait separates
        // them from nothing, so the label has to reach for something that does.
        let graph = ProjectGraph::for_test(
            vec![claimed_file("a.rs")],
            vec![
                member(0, "View", "poll", 10, Some("Host")),
                member(0, "View", "poll", 40, Some("Host")),
            ],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), shape(vec![1, 2, 3, 4, 5], 0, (10, 1))),
            (SymbolId(1), shape(vec![1, 2, 3, 4, 5], 0, (40, 1))),
        ]);
        let m = find_duplicate_functions(&graph, 50).0[0].message.clone();
        assert!(m.contains("a.rs#View.poll:10"), "{m}");
        assert!(m.contains("a.rs#View.poll:40"), "{m}");
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
        let findings = find_duplicate_functions(&graph, 50).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "duplicate");
        assert_eq!(findings[0].subject_kind, "function");
        assert_eq!(findings[0].severity, Severity::Info);
        assert_eq!(findings[0].related.len(), 2, "every instance in related");
        assert!(findings[0].message.contains("a.ts#one"));
        assert!(findings[0].message.contains("b.ts#two"));
    }

    #[test]
    fn a_raised_min_tokens_gate_excludes_smaller_functions() {
        // The same clone pair (token_count 60): matched under the default floor, out of
        // scope when the configured gate rises above their size.
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
        assert_eq!(find_duplicate_functions(&graph, 50).0.len(), 1);
        let (findings, duplicated) = find_duplicate_functions(&graph, 100);
        assert!(findings.is_empty());
        assert!(duplicated.is_empty());
    }

    #[test]
    fn test_role_files_are_exempt_from_structural_clones() {
        // The same fingerprint set, one instance in a test-role file: no group forms, and the
        // production instance alone contributes nothing to the duplicated-token list.
        let mut test_file = claimed_file("tests/a.ts");
        test_file.class = Some(FileClass {
            role: FileRole::Test,
            origin: FileOrigin::Authored,
        });
        let graph = ProjectGraph::for_test(
            vec![test_file, claimed_file("b.ts")],
            vec![callable(0, "one"), callable(1, "two")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), metrics(vec![1, 2, 3, 4, 5])),
            (SymbolId(1), metrics(vec![1, 2, 3, 4, 5])),
        ]);
        let (findings, duplicated) = find_duplicate_functions(&graph, 50);
        assert!(findings.is_empty());
        assert!(duplicated.is_empty());
    }

    #[test]
    fn test_regions_inside_production_files_are_exempt_from_structural_clones() {
        // A `#[cfg(test)]`-style region (FileFacts::test_spans) exempts the callable inside
        // it — same rule as crap/untested, at span granularity; the same pair with both
        // instances in production code still groups (the guard).
        let mut prod_with_region = claimed_file("a.ts");
        prod_with_region.test_spans = vec![crate::adapter::Span {
            start: (1, 1),
            end: (10, 999),
        }];
        let graph = ProjectGraph::for_test(
            vec![prod_with_region, claimed_file("b.ts")],
            vec![callable(0, "one"), callable(1, "two")],
            vec![],
            vec![],
        )
        .with_function_metrics(vec![
            (SymbolId(0), metrics(vec![1, 2, 3, 4, 5])),
            (SymbolId(1), metrics(vec![1, 2, 3, 4, 5])),
        ]);
        assert!(find_duplicate_functions(&graph, 50).0.is_empty());

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
        assert_eq!(find_duplicate_functions(&graph, 50).0.len(), 1);
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
        assert_eq!(find_duplicate_functions(&graph, 50).0.len(), 1);
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
        assert!(find_duplicate_functions(&graph, 50).0.is_empty());
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
        assert!(find_duplicate_functions(&graph, 50).0.is_empty());
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
        assert!(find_duplicate_functions(&graph, 50).0.is_empty());
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
        let findings = find_duplicate_functions(&graph, 50).0;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].related.len(), 3);
    }
}
