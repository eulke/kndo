//! The incremental patch: on a graph-snapshot key miss, reuse the previous snapshot
//! when every guard holds (same schema/plugin digest, same file set, no manifest
//! change, small dirty set, unchanged surface signatures). Byte-identical to the
//! full rebuild by construction - enforced by the equivalence suite.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::adapter::{
    Diagnostic, ImportSpec, LanguageAdapter, ProjectPath, Resolution, ResolveCtx,
};
use crate::discovery::{self};
use crate::vocab::{
    Confidence, DependencyId, Edge, EdgeKind, FileId, NodeRef, Provenance, SymbolId,
};
use smol_str::SmolStr;

#[allow(unused_imports)]
use super::*;

/// The incremental patch. Applies when the previous snapshot exists, the file
/// **set** is unchanged, no changed file is a manifest, at most 30% of files changed, and
/// every changed claimed file's surface signature is unchanged — in which case the dirty set
/// is exactly the changed files (no resolution input moved). Everything else returns
/// `None` and the caller full-rebuilds: one fallback, always correct. Every guard runs
/// BEFORE any mutation — a half-patched graph must be unrepresentable.
///
/// The correctness obligation: the returned graph is byte-identical to what the full
/// rebuild of the same tree produces — enforced by the equivalence suite, made possible by
/// the canonical-order invariant and by sharing the exact per-file machinery
/// ([`claim_and_extract`], [`emit_file_declarations`], and phase 3b's three passes
/// [`resolve_imports`] / [`link_module_bindings`] / [`resolve_references`]) with the full path.
/// `(graph, extraction diagnostics, plugin diagnostics, plugin contributions)` — named only to
/// keep the 4-tuple under clippy's type-complexity lint; callers still destructure it
/// positionally.
type PatchOutcome = (
    ProjectGraph,
    Vec<Diagnostic>,
    Vec<Diagnostic>,
    Vec<crate::plugin::PluginContribution>,
);

pub(crate) fn try_patch(
    discovered: &discovery::DiscoveredTree,
    adapters: &[Box<dyn LanguageAdapter>],
    sorted_plugins: &[&dyn crate::plugin::Plugin],
    current_plugin_digest: [u8; 32],
    cache: &crate::cache::ProjectCache,
) -> Option<PatchOutcome> {
    let crate::cache::LoadedSnapshot {
        mut graph,
        mut extraction_diagnostics,
        plugin_diagnostics: _, // stale — the plugin round below re-derives its diagnostics whole
        plugin_set_digest: snapshot_plugin_digest,
        graph_schema_version: snapshot_schema_version,
    } = cache.latest_graph()?;

    // ---- guards, in cheapest-first order ----
    // A snapshot assembled under different semantics cannot be patched: unchanged files'
    // edges ride the patch verbatim, so old-semantics edges would survive into a graph the
    // new binary claims as its own. The keyed warm path folds the schema version into the
    // key; this latest-pointer path is keyless, so the snapshot carries its own provenance.
    if snapshot_schema_version != GRAPH_SCHEMA_VERSION {
        return None;
    }
    // A changed plugin set means the snapshot's `FileNode.class` values may
    // carry another set's `classify_file` overrides — untagged and unstrippable, unlike the
    // provenance-tagged edges below. Full rebuild once; the snapshot key would miss anyway.
    if snapshot_plugin_digest != current_plugin_digest {
        return None;
    }
    if graph.files.len() != discovered.files.len() {
        return None;
    }
    if graph
        .files
        .iter()
        .zip(&discovered.files)
        .any(|(old, new)| old.path != new.path)
    {
        return None; // adds/removes/renames renumber FileIds — the honest fallback
    }
    let changed: Vec<usize> = graph
        .files
        .iter()
        .zip(&discovered.files)
        .enumerate()
        .filter(|(_, (old, new))| old.content_hash != new.content_hash)
        .map(|(i, _)| i)
        .collect();
    if changed.is_empty() {
        return None; // identical tree would have hit the snapshot key — defensive
    }
    if changed.len() * 20 > graph.files.len() {
        // Measured, not assumed: the patch's fixed costs (snapshot load, table rebuild)
        // stop beating the saved resolution once more than ~5% of files changed, so a
        // larger dirty set full-rebuilds instead.
        return None;
    }
    for &c in &changed {
        if adapters
            .iter()
            .any(|a| a.claim_manifest(&discovered.files[c].path))
        {
            return None; // manifests feed global inputs — full rebuild
        }
    }

    // Re-extract every changed claimed file and check its surface signature. Still no
    // mutation: any failure here must leave nothing behind.
    struct ChangedFile {
        index: usize,
        claimed: Option<Claimed>,
    }
    let mut changed_files: Vec<ChangedFile> = Vec::with_capacity(changed.len());
    for &c in &changed {
        match claim_and_extract(&discovered.files[c], adapters, Some(cache), discovered) {
            Err(_) => return None, // unreadable at extraction — let the full path diagnose
            Ok(None) => {
                if graph.files[c].language.is_some() {
                    return None; // claim is path-based; a flip here means a stale snapshot
                }
                changed_files.push(ChangedFile {
                    index: c,
                    claimed: None,
                });
            }
            Ok(Some(claimed)) => {
                if graph.patch_meta[c].surface_sig != Some(claimed.surface_sig) {
                    return None; // some resolution input moved — full rebuild
                }
                changed_files.push(ChangedFile {
                    index: c,
                    claimed: Some(claimed),
                });
            }
        }
    }

    // Symbol runs must be contiguous per file (the full build constructs them that way) and
    // each changed file's run must align 1:1 with its fresh declarations. The signature
    // already implies alignment — but SymbolIds are load-bearing, so verify, never trust.
    let mut symbol_range: Vec<(u32, u32)> = vec![(0, 0); graph.files.len()];
    {
        let mut last_file: Option<u32> = None;
        for (idx, sym) in graph.symbols.iter().enumerate() {
            let f = sym.file.0;
            match last_file {
                Some(prev) if f == prev => symbol_range[f as usize].1 = idx as u32 + 1,
                Some(prev) if f < prev => return None, // non-contiguous — stale/corrupt
                _ => {
                    if symbol_range[f as usize].1 != 0 {
                        return None; // a second run for the same file — non-contiguous
                    }
                    symbol_range[f as usize] = (idx as u32, idx as u32 + 1);
                }
            }
            last_file = Some(f);
        }
    }
    for cf in &changed_files {
        let Some(claimed) = &cf.claimed else { continue };
        let (start, end) = symbol_range[cf.index];
        let decls = &claimed.facts.declarations;
        if (end - start) as usize != decls.len() {
            return None;
        }
        for (d, decl) in decls.iter().enumerate() {
            let sym = &graph.symbols[start as usize + d];
            if sym.name != decl.name
                || sym.kind != decl.kind
                || sym.exported != decl.exported
                || sym.visibility != decl.visibility
                || sym.member_of != decl.member_of
                || sym.nested_scope != decl.nested_scope
                || sym.visibility_inherited != decl.visibility_inherited
            {
                return None;
            }
        }
    }

    // ---- every guard passed: mutation begins ----
    // First: discard every plugin contribution — provenance-tagged edges and the
    // wholly plugin-derived `externally_consumed` set — before anything below reads the edge
    // list. Order matters beyond hygiene: the library-root scan further down derives roots
    // from KEPT edges, and in the full build it runs on adapter data only (plugins haven't
    // run yet at that point); a surviving plugin Root edge here could masquerade as a library
    // root and break byte-identity. The round re-runs at the end, on the patched graph.
    graph
        .edges
        .retain(|e| !matches!(e.source, crate::vocab::Provenance::Plugin(_)));
    graph.externally_consumed.clear();
    graph.plugin_implicitly_invoked.clear();

    let changed_set: HashSet<u32> = changed.iter().map(|&c| c as u32).collect();
    let changed_paths: HashSet<ProjectPath> = changed
        .iter()
        .map(|&c| graph.files[c].path.clone())
        .collect();

    for cf in &changed_files {
        let c = cf.index;
        graph.files[c].content_hash = discovered.files[c].content_hash;
        if let Some(claimed) = &cf.claimed {
            let (start, _) = symbol_range[c];
            for (d, decl) in claimed.facts.declarations.iter().enumerate() {
                let sym = &mut graph.symbols[start as usize + d];
                sym.span = decl.span;
                sym.signature_span = decl.signature_span;
            }
            // Test-region extents move with every edit, same as symbol spans; the *gating*
            // of imports (the only cross-file consequence) is surface-signature-guarded, so
            // refreshing the extents here keeps crap/health/hygiene containment exact while
            // `FileNode::class` (phase 2.55's demotion included) stays valid untouched.
            let mut spans = claimed.facts.test_spans.clone();
            spans.sort_unstable();
            graph.files[c].test_spans = spans;
            // Same body-level refresh for string call sites: a changed
            // literal or a new call flows through the patch, and the plugin round below
            // reads the current values off the FileNode.
            let mut sites = claimed.facts.string_call_args.clone();
            sites.sort_unstable();
            graph.files[c].string_call_sites = sites;
            graph.patch_meta[c].surface_sig = Some(claimed.surface_sig);
        }
    }

    // Remove everything the changed files own — exact, thanks to Edge.owner.
    graph.edges.retain(|e| !changed_set.contains(&e.owner.0));
    let mut function_metrics = std::mem::take(&mut graph.function_metrics);
    function_metrics.retain(|(id, _)| !changed_set.contains(&graph.symbols[id.0 as usize].file.0));
    graph
        .suppressions
        .retain(|(f, _)| !changed_set.contains(&f.0));
    extraction_diagnostics.retain(|d| d.path.as_ref().is_none_or(|p| !changed_paths.contains(p)));

    // ---- rebuild the resolution environment from the snapshot (everything derivable) ----
    let known_files: HashSet<ProjectPath> = graph.files.iter().map(|f| f.path.clone()).collect();
    let declared_dependency_names: HashSet<SmolStr> = graph
        .declared_dependencies
        .iter()
        .map(|d| d.name.clone())
        .collect();
    let mut workspace_member_index: HashMap<SmolStr, crate::adapter::WorkspaceMember> =
        HashMap::default();
    for pkg in &graph.packages {
        let (Some(manifest), Some(name)) = (&pkg.manifest, &pkg.name) else {
            continue;
        };
        workspace_member_index
            .entry(name.clone())
            .or_insert_with(|| crate::adapter::WorkspaceMember {
                dir: SmolStr::new(core_dirname(manifest.0.as_str())),
                entry: pkg.workspace_entry.clone(),
                targets: pkg.targets.clone(),
            });
    }
    // Unit reverse-index (Java: an import specifier there IS a
    // unit value directly), so resolution needs unit → declaring files, not just the forward
    // per-file `unit` already carried on `FileNode`. The surface-signature guard already
    // ensures a changed file's `unit` never silently drifts under the patch, so
    // reading `graph.files`' current state here stays byte-identical to a full rebuild.
    let units = build_unit_indexes(&graph.files);
    let ctx = ResolveCtx::new(&known_files)
        .with_declared_dependencies(&declared_dependency_names)
        .with_workspace_members(&workspace_member_index)
        .with_units(&units.by_unit)
        .with_package_units(&units.by_package, &units.file_package);

    let files_len = graph.files.len();
    let mut symbol_by_name_per_file: Vec<HashMap<SmolStr, SymbolId>> =
        vec![HashMap::default(); files_len];
    let mut symbol_by_qualified_per_file: Vec<HashMap<String, SymbolId>> =
        vec![HashMap::default(); files_len];
    let mut qualified_twins_per_file: Vec<HashMap<String, Vec<SymbolId>>> =
        vec![HashMap::default(); files_len];
    let mut symbol_by_name_per_unit: HashMap<SmolStr, HashMap<SmolStr, SymbolId>> =
        HashMap::default();
    // Mirrors the full build's construction exactly — `patch_equivalence` requires it.
    let mut symbol_twins_per_unit: HashMap<SmolStr, HashMap<SmolStr, Vec<SymbolId>>> =
        HashMap::default();
    let mut member_by_name: HashMap<SmolStr, Vec<SymbolId>> = HashMap::default();
    for (idx, sym) in graph.symbols.iter().enumerate() {
        let i = sym.file.0 as usize;
        let id = SymbolId(idx as u32);
        match &sym.member_of {
            None => {
                symbol_by_name_per_file[i].insert(sym.name.clone(), id);
                if let Some(unit) = &graph.files[i].unit {
                    let slot = symbol_by_name_per_unit.entry(unit.clone()).or_default();
                    if let Some(displaced) = slot.insert(sym.name.clone(), id) {
                        symbol_twins_per_unit
                            .entry(unit.clone())
                            .or_default()
                            .entry(sym.name.clone())
                            .or_default()
                            .push(displaced);
                    }
                }
            }
            Some(owner) => {
                member_by_name.entry(sym.name.clone()).or_default().push(id);
                insert_qualified(
                    &mut symbol_by_qualified_per_file[i],
                    &mut qualified_twins_per_file[i],
                    format!("{owner}.{}", sym.name),
                    id,
                );
            }
        }
    }
    // Aliases go in after declarations, vacant-only — the same outcome pass A + the fixpoint
    // produce (declarations always precede aliases there too).
    for (i, meta) in graph.patch_meta.iter().enumerate() {
        for alias in &meta.reexport_aliases {
            symbol_by_name_per_file[i]
                .entry(alias.name.clone())
                .or_insert(alias.symbol);
        }
    }
    let file_unit: Vec<Option<SmolStr>> = graph.files.iter().map(|f| f.unit.clone()).collect();
    let unit_parents = crate::graph::assemble::unit_parent_index(&graph.files);
    let unit_name_by_file: Vec<Option<SmolStr>> = graph
        .patch_meta
        .iter()
        .map(|m| m.unit_name.clone())
        .collect();
    // Member-type facts from the persisted patch metadata — valid under
    // the surface-signature guard for changed files too (an annotation change declines the
    // patch), same reasoning as `unit_name`.
    let member_types_per_file: Vec<MemberTypeIndex> = graph
        .patch_meta
        .iter()
        .map(|m| index_member_types(&m.member_types))
        .collect();
    let ladders: std::collections::BTreeMap<SmolStr, Vec<crate::adapter::VisibilityRung>> =
        graph.visibility_ladders.iter().cloned().collect();
    // Straight from the descriptors, exactly as the full build does: these are the adapter's
    // own facts about its language, so they are never stale in the way a persisted table
    // could be, and building them the same way in both paths is what keeps the two from
    // drifting. Only languages the graph actually claims contribute.
    let mut builtin_member_types: std::collections::BTreeMap<SmolStr, MemberTypeIndex> =
        std::collections::BTreeMap::new();
    for adapter in adapters {
        let descriptor = adapter.descriptor();
        if ladders.contains_key(&descriptor.id)
            || graph
                .files
                .iter()
                .any(|f| f.language.as_deref() == Some(&descriptor.id))
        {
            builtin_member_types
                .entry(descriptor.id.clone())
                .or_insert_with(|| index_member_types(&descriptor.builtin_member_types));
        }
    }

    // Library roots for the changed files, from the KEPT (manifest-owned) edges — a changed
    // file's own in-source production roots were just removed and regenerate below.
    let mut library_root_files: HashMap<FileId, Confidence> = HashMap::default();
    for e in &graph.edges {
        if let EdgeKind::Root {
            kind: crate::vocab::RootKind::Production,
            target: NodeRef::File(f),
        } = e.kind
        {
            if changed_set.contains(&f.0) {
                let entry = library_root_files.entry(f).or_insert(e.confidence);
                *entry = (*entry).max(e.confidence);
            }
        }
    }
    let mut role_root_files: HashMap<FileId, crate::vocab::RootKind> = HashMap::default();
    for cf in &changed_files {
        if cf.claimed.is_some() {
            if let Some(class) = graph.files[cf.index].class {
                let kind = match class.role {
                    crate::vocab::FileRole::Test => Some(crate::vocab::RootKind::Test),
                    crate::vocab::FileRole::Tooling => Some(crate::vocab::RootKind::Tooling),
                    crate::vocab::FileRole::Production => None,
                };
                if let Some(kind) = kind {
                    role_root_files.insert(FileId(cf.index as u32), kind);
                }
            }
        }
    }

    // ---- regenerate the changed files' contributions, via the SAME machinery as the full
    // build (emit_file_declarations + resolve_imports/link_module_bindings/resolve_references) ----
    let mut new_edges: Vec<Edge> = Vec::new();
    let mut new_metrics: Vec<(SymbolId, SymbolMetrics)> = Vec::new();
    let mut resolved_outputs: Vec<(ImportResolution, ResolvedFile)> = Vec::new();
    let mut changed_imports: Vec<(usize, ImportResolution)> = Vec::new();
    let mut new_patch_meta_bindings: Vec<(usize, Vec<crate::graph::ModuleBinding>)> = Vec::new();
    {
        let executable_by_name = executable_name_index(&graph.packages, &graph.file_index);
        let tables = ResolveTables {
            files: &graph.files,
            file_index: &graph.file_index,
            symbols: &graph.symbols,
            symbol_by_name_per_file: &symbol_by_name_per_file,
            symbol_by_qualified_per_file: &symbol_by_qualified_per_file,
            qualified_twins_per_file: &qualified_twins_per_file,
            member_types_per_file: &member_types_per_file,
            symbol_by_name_per_unit: &symbol_by_name_per_unit,
            symbol_twins_per_unit: &symbol_twins_per_unit,
            member_by_name: &member_by_name,
            file_unit: &file_unit,
            unit_parents: &unit_parents,
            unit_name_by_file: &unit_name_by_file,
            ladders: &ladders,
            builtin_member_types: &builtin_member_types,
            executable_by_name: &executable_by_name,
            ctx: &ctx,
        };
        for cf in &changed_files {
            let Some(claimed) = &cf.claimed else { continue };
            let c = cf.index;
            let file_id = FileId(c as u32);
            let adapter = &adapters[claimed.adapter_index];
            let adapter_id = adapter.descriptor().id;

            // Phase 2.6's role-derived file root (owned by the file, hence removed above).
            if let Some(&kind) = role_root_files.get(&file_id) {
                new_edges.push(Edge {
                    kind: EdgeKind::Root {
                        kind,
                        target: NodeRef::File(file_id),
                    },
                    confidence: Confidence::Probable,
                    source: Provenance::Adapter(adapter_id.clone()),
                    span: None,
                    owner: file_id,
                });
            }
            // Phase 3a emissions, shared emitter.
            let em = emit_file_declarations(
                c,
                &claimed.facts,
                adapter_id.as_str(),
                symbol_range[c].0,
                &graph.symbols,
                &symbol_by_name_per_file[c],
                &library_root_files,
                &role_root_files,
                graph.files[c]
                    .language
                    .as_deref()
                    .and_then(|l| ladders.get(l))
                    .map(|l| l.as_slice()),
            );
            new_edges.extend(em.edges);
            new_metrics.extend(em.metrics);
            // Phase 3a-bis's promotions: the aliases themselves are unchanged under the guard
            // (persisted, order-independent state); only the edges — owned by this file and
            // removed above — regenerate, spans refreshed from the fresh imports.
            if let Some(&confidence) = library_root_files.get(&file_id) {
                for alias in &graph.patch_meta[c].reexport_aliases {
                    let span = claimed
                        .facts
                        .imports
                        .iter()
                        .filter(|imp| imp.reexported)
                        .find(|imp| imp.bindings.iter().any(|b| b.local == alias.name))
                        .map(|imp| imp.span);
                    new_edges.push(Edge {
                        kind: EdgeKind::Root {
                            kind: crate::vocab::RootKind::Production,
                            target: NodeRef::Symbol(alias.symbol),
                        },
                        confidence,
                        source: Provenance::Adapter(adapter_id.clone()),
                        span,
                        owner: file_id,
                    });
                }
            }
            // Phase 2.7's surface-expansion edges (owned by this file, removed above):
            // membership itself is stable under the guard — kept edges from other owners
            // still mark this file (and its targets stay members through edges THEY own) —
            // so only the edges this file emits regenerate, from its unchanged re-exports.
            if let Some(&confidence) = library_root_files.get(&file_id) {
                for imp in claimed
                    .facts
                    .imports
                    .iter()
                    .filter(|i| i.reexported && i.bindings.is_empty())
                {
                    let spec = ImportSpec {
                        specifier: imp.specifier.clone(),
                        from: graph.files[c].path.clone(),
                    };
                    let target = match adapter.resolve(&spec, &ctx) {
                        Resolution::File(path, _) => path,
                        Resolution::WorkspaceMember { target, .. } => target,
                        _ => continue,
                    };
                    let Some(&target) = graph.file_index.get(&target) else {
                        continue;
                    };
                    new_edges.push(Edge {
                        kind: EdgeKind::Root {
                            kind: crate::vocab::RootKind::Production,
                            target: NodeRef::File(target),
                        },
                        confidence,
                        span: Some(imp.span),
                        source: Provenance::Adapter(adapter_id.clone()),
                        owner: file_id,
                    });
                }
            }
            // Phase 3b pass one, shared resolver. References wait for the hop below.
            changed_imports.push((c, resolve_imports(c, &claimed.facts, &**adapter, &tables)));
        }

        // Phase 3b pass one-and-a-half — the qualifier hop, over the SAME table shape the
        // full build builds. Unchanged files' bindings come from the snapshot
        // (`FilePatchMeta::module_bindings`, persisted for exactly this); changed files' come
        // from the pass just run. Correctness rests on a guard that already exists: a file
        // whose imports moved has a different surface signature, and the patch refuses those
        // outright — so an unchanged file's persisted table can never be stale here.
        let mut bindings_by_file: Vec<HashMap<SmolStr, FileId>> = graph
            .patch_meta
            .iter()
            .map(|meta| {
                meta.module_bindings
                    .iter()
                    .map(|b| (b.name.clone(), b.target))
                    .collect()
            })
            .collect();
        let (changed_indexes, mut changed_resolutions): (Vec<usize>, Vec<ImportResolution>) =
            std::mem::take(&mut changed_imports).into_iter().unzip();
        for (&c, resolution) in changed_indexes.iter().zip(changed_resolutions.iter()) {
            bindings_by_file[c] = resolution
                .module_bindings
                .iter()
                .map(|b| (b.name.clone(), b.target))
                .collect();
        }
        link_module_bindings(&mut changed_resolutions, &bindings_by_file);

        // Phase 3b pass two, in the same changed-file order pass one ran in.
        let mut claims_by_index: HashMap<usize, &Claimed> = HashMap::default();
        for cf in &changed_files {
            if let Some(claimed) = &cf.claimed {
                claims_by_index.insert(cf.index, claimed);
            }
        }
        for (c, imports) in changed_indexes.into_iter().zip(changed_resolutions) {
            let Some(claimed) = claims_by_index.get(&c) else {
                continue;
            };
            let adapter = &adapters[claimed.adapter_index];
            let references = resolve_references(c, &claimed.facts, &**adapter, &tables, &imports);
            new_patch_meta_bindings.push((c, imports.module_bindings.clone()));
            resolved_outputs.push((imports, references));
        }
    }
    for (c, bindings) in new_patch_meta_bindings {
        graph.patch_meta[c].module_bindings = bindings;
    }

    // ---- apply, then restore the canonical order ----
    let mut dep_index: HashMap<SmolStr, DependencyId> = graph
        .dependencies
        .iter()
        .enumerate()
        .map(|(i, d)| (d.name.clone(), DependencyId(i as u32)))
        .collect();
    graph.edges.extend(new_edges);
    for (imports, out) in resolved_outputs {
        // Imports before references, per file — the order the full build merges them in, and
        // the order `DependencyId` assignment depends on.
        graph.edges.extend(imports.edges);
        graph.edges.extend(out.edges);
        for (name, confidence, span, from, source) in imports.dep_imports {
            let to = *dep_index.entry(name.clone()).or_insert_with(|| {
                let id = DependencyId(graph.dependencies.len() as u32);
                graph
                    .dependencies
                    .push(DependencyNode { name: name.clone() });
                id
            });
            graph.edges.push(Edge {
                kind: EdgeKind::ImportsDependency { from, to },
                confidence,
                source,
                span: Some(span),
                owner: from,
            });
        }
        extraction_diagnostics.extend(out.diagnostics);
        graph.suppressions.extend(out.suppressions);
    }
    function_metrics.extend(new_metrics);
    function_metrics.sort_by_key(|(id, _)| *id);
    graph.function_metrics = function_metrics;

    // Re-run the plugin round against the patched graph — the same function the
    // full build calls, over the same state shape it would see there (files/symbols in final
    // form, name tables current, aliases included), so every contribution and its diagnostics
    // re-derive exactly as a full rebuild would derive them. The stale contributions were
    // stripped at mutation start; nothing plugin-produced ever rides a patch unrevised.
    let round = run_plugin_round(
        &graph.files,
        &graph.symbols,
        &graph.file_index,
        &symbol_by_name_per_file,
        &symbol_by_qualified_per_file,
        &graph.packages,
        &graph.edges,
        discovered,
        sorted_plugins,
    );
    graph.edges.extend(round.edges);
    graph.externally_consumed = round.externally_consumed;
    graph.plugin_implicitly_invoked = round.implicitly_invoked;
    let plugin_diagnostics = round.diagnostics;
    // The patch re-ran the round, so it refreshes the audit record exactly like
    // a full build would.
    cache.record_plugin_contributions(&round.contributions);

    graph.edges.sort_unstable();
    graph.suppressions.sort_by_key(|(f, _)| *f);
    extraction_diagnostics.sort_unstable();

    cache.count_graph_hit(); // the previous snapshot genuinely served this run
    Some((
        graph,
        extraction_diagnostics,
        plugin_diagnostics,
        round.contributions,
    ))
}
