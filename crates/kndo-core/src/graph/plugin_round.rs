//! The plugin round: contribute_roots/contribute_edges/annotate_symbols plus the
//! finding round, resolved through the same name tables adapter facts use.

use rustc_hash::FxHashMap as HashMap;

use crate::adapter::{Diagnostic, ProjectPath};
use crate::discovery::{self};
use crate::vocab::{Edge, EdgeKind, FileId, NodeRef, SymbolId};
use smol_str::SmolStr;

#[allow(unused_imports)]
use super::*;

/// Resolves a plugin-named [`crate::plugin::PluginTarget`] against the same bare/qualified
/// symbol tables phase 3b's own reference resolution
/// reads — the exact two-step fallback [`emit_file_declarations`]'s `bare_table`/
/// `qualified_table` already use for `RawRoot`. A path or name that doesn't resolve returns
/// `None`; callers drop it silently, the same miss behavior an adapter's own `RawRoot`/
/// `RawReference` already has.
pub(crate) fn resolve_plugin_target(
    target: &crate::plugin::PluginTarget,
    file_index: &HashMap<ProjectPath, FileId>,
    symbol_by_name_per_file: &[HashMap<SmolStr, SymbolId>],
    symbol_by_qualified_per_file: &[HashMap<String, SymbolId>],
) -> Option<NodeRef> {
    let file_id = *file_index.get(&target.path)?;
    match &target.symbol {
        None => Some(NodeRef::File(file_id)),
        Some(name) => {
            let idx = file_id.0 as usize;
            symbol_by_name_per_file
                .get(idx)
                .and_then(|t| t.get(name))
                .or_else(|| {
                    symbol_by_qualified_per_file
                        .get(idx)
                        .and_then(|t| t.get(name.as_str()))
                })
                .copied()
                .map(NodeRef::Symbol)
        }
    }
}

/// Everything one plugin graph-mutation round produces — kept apart from adapter output
/// because the two have different reuse fates: adapter edges/diagnostics
/// persist and patch incrementally; plugin output is discarded and re-derived whole.
pub(crate) struct PluginRound {
    pub(crate) edges: Vec<Edge>,
    pub(crate) externally_consumed: Vec<SymbolId>,
    pub(crate) implicitly_invoked: Vec<SymbolId>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Per-plugin resolved-contribution counts, in the round's own id-sorted
    /// call order — recorded to the cache as the last-run audit record for `kndo doctor`.
    pub(crate) contributions: Vec<crate::plugin::PluginContribution>,
}

/// The plugin graph-mutation hooks: `contribute_roots`/`contribute_edges`/
/// `annotate_symbols`, once per registered graph-mutating plugin, in id-sorted order. One
/// function for both build paths: the full build runs it after phase 3b's
/// reference merge, the incremental patch after splicing — in both cases `files`/`symbols`
/// and the name tables are in their final, identical state, which is what makes the patched
/// graph byte-identical to a full rebuild's. Hooks are deterministic functions of the graph
/// and the discovered tree; nothing here reads clocks, randomness, or other plugins' output.
#[allow(clippy::too_many_arguments)] // one parameter per graph facet the view exposes; a bundle struct would just rename the list
pub(crate) fn run_plugin_round(
    files: &[FileNode],
    symbols: &[SymbolNode],
    file_index: &HashMap<ProjectPath, FileId>,
    symbol_by_name_per_file: &[HashMap<SmolStr, SymbolId>],
    symbol_by_qualified_per_file: &[HashMap<String, SymbolId>],
    packages: &[PackageNode],
    edges: &[Edge],
    discovered: &discovery::DiscoveredTree,
    sorted_plugins: &[&dyn crate::plugin::Plugin],
) -> PluginRound {
    let mut round = PluginRound {
        edges: Vec::new(),
        externally_consumed: Vec::new(),
        implicitly_invoked: Vec::new(),
        diagnostics: Vec::new(),
        contributions: Vec::new(),
    };
    if sorted_plugins.is_empty() {
        // The `GraphView` index build is one more O(symbols) pass, negligible next to
        // extraction's own — but there's no reason to pay it for the common zero-plugin case.
        return round;
    }
    let view = crate::plugin::GraphView::new(files, symbols, file_index, packages, edges);
    let tables = PluginTargetTables {
        symbols,
        file_index,
        symbol_by_name_per_file,
        symbol_by_qualified_per_file,
    };
    for plugin in sorted_plugins {
        let descriptor = plugin.descriptor();
        let provenance = crate::vocab::Provenance::Plugin(descriptor.id.clone());
        // One ContentView per plugin per round, scoped to that plugin's own
        // declared globs — budget and the at-most-one cutoff diagnostic are per plugin,
        // never shared across plugins (one component exceeding its budget must not starve
        // another's legitimate reads).
        let content = crate::plugin::ContentView::new(
            discovered,
            descriptor.id.clone(),
            &descriptor.requested_file_access,
        );

        // Counted by delta around each collector: only contributions that *resolved* (made
        // it into the round) count — a sink item whose target missed changed nothing, and the
        // audit record's job is to say what a plugin actually asserted into the graph. The
        // misses themselves are recorded too (`dropped`): silent in the graph by contract,
        // but the author kit's whole debugging story for "contributed 0 roots".
        let mut dropped = Vec::new();
        let edges_before = round.edges.len();
        collect_plugin_roots(
            *plugin,
            &view,
            &content,
            &provenance,
            &tables,
            &mut round,
            &mut dropped,
        );
        let roots_after = round.edges.len();
        collect_plugin_edges(
            *plugin,
            &view,
            &content,
            &provenance,
            &tables,
            &mut round,
            &mut dropped,
        );
        let edges_after = round.edges.len();
        let annotations_before = round.externally_consumed.len() + round.implicitly_invoked.len();
        collect_plugin_annotations(*plugin, &view, &content, &tables, &mut round, &mut dropped);
        let annotations_after = round.externally_consumed.len() + round.implicitly_invoked.len();
        round.contributions.push(crate::plugin::PluginContribution {
            id: descriptor.id.to_string(),
            roots: (roots_after - edges_before) as u32,
            edges: (edges_after - roots_after) as u32,
            annotations: (annotations_after - annotations_before) as u32,
            dropped,
        });
        if let Some(diagnostic) = content.take_diagnostic() {
            round.diagnostics.push(diagnostic);
        }
    }
    round.externally_consumed.sort_unstable();
    round.externally_consumed.dedup();
    round.implicitly_invoked.sort_unstable();
    round.implicitly_invoked.dedup();
    // Same canonical-order rule the adapter-side vectors follow — these are
    // persisted (the snapshot's plugin partition) and compared by the equivalence gate.
    round.diagnostics.sort_unstable();
    round
}

/// The lookup state [`resolve_plugin_target`] and edge-owner derivation need, bundled so the
/// per-hook collectors below stay under the workspace's own CRAP gate without seven-argument
/// signatures.
pub(crate) struct PluginTargetTables<'a> {
    pub(crate) symbols: &'a [SymbolNode],
    pub(crate) file_index: &'a HashMap<ProjectPath, FileId>,
    pub(crate) symbol_by_name_per_file: &'a [HashMap<SmolStr, SymbolId>],
    pub(crate) symbol_by_qualified_per_file: &'a [HashMap<String, SymbolId>],
}

impl PluginTargetTables<'_> {
    fn resolve(&self, target: &crate::plugin::PluginTarget) -> Option<NodeRef> {
        resolve_plugin_target(
            target,
            self.file_index,
            self.symbol_by_name_per_file,
            self.symbol_by_qualified_per_file,
        )
    }

    /// The file whose change invalidates this contribution under the patch (the
    /// `Edge.owner` discipline): the target's own file.
    fn owner_of(&self, node: NodeRef) -> FileId {
        match node {
            NodeRef::File(f) => f,
            NodeRef::Symbol(s) => self.symbols[s.0 as usize].file,
        }
    }
}

/// Cap on recorded miss descriptions per plugin per round — enough to debug with, bounded so
/// a pathological component can't bloat the audit record.
pub(crate) const DROPPED_CAP: usize = 25;

/// Record one unresolved sink target, respecting [`DROPPED_CAP`] (the cap entry itself says
/// how much was elided — no silent truncation).
pub(crate) fn record_dropped(
    dropped: &mut Vec<String>,
    what: &str,
    target: &crate::plugin::PluginTarget,
) {
    if dropped.len() < DROPPED_CAP {
        let t = match &target.symbol {
            Some(symbol) => format!("{}#{symbol}", target.path.0),
            None => target.path.0.to_string(),
        };
        dropped.push(format!("{what} `{t}` did not resolve"));
    } else if dropped.len() == DROPPED_CAP {
        dropped.push("(further unresolved targets elided)".to_string());
    }
}

#[allow(clippy::too_many_arguments)] // the collector trio shares one call shape; a bundle struct would just rename the list
pub(crate) fn collect_plugin_roots(
    plugin: &dyn crate::plugin::Plugin,
    view: &crate::plugin::GraphView<'_>,
    content: &crate::plugin::ContentView<'_>,
    provenance: &crate::vocab::Provenance,
    tables: &PluginTargetTables<'_>,
    round: &mut PluginRound,
    dropped: &mut Vec<String>,
) {
    let mut root_sink = crate::plugin::RootSink::default();
    plugin.contribute_roots(view, content, &mut root_sink);
    for root in root_sink.items {
        let Some(target) = tables.resolve(&root.target) else {
            record_dropped(dropped, "root target", &root.target);
            continue;
        };
        round.edges.push(Edge {
            kind: EdgeKind::Root {
                kind: root.kind,
                target,
            },
            confidence: root.confidence,
            source: provenance.clone(),
            span: None,
            owner: tables.owner_of(target),
        });
    }
}

#[allow(clippy::too_many_arguments)] // same shape as collect_plugin_roots
pub(crate) fn collect_plugin_edges(
    plugin: &dyn crate::plugin::Plugin,
    view: &crate::plugin::GraphView<'_>,
    content: &crate::plugin::ContentView<'_>,
    provenance: &crate::vocab::Provenance,
    tables: &PluginTargetTables<'_>,
    round: &mut PluginRound,
    dropped: &mut Vec<String>,
) {
    let mut edge_sink = crate::plugin::EdgeSink::default();
    plugin.contribute_edges(view, content, &mut edge_sink);
    for contributed in edge_sink.items {
        let Some(from) = tables.resolve(&contributed.from) else {
            record_dropped(dropped, "edge source", &contributed.from);
            continue;
        };
        let kind = match tables.resolve(&contributed.to) {
            // A symbol target is an ordinary reference.
            Some(NodeRef::Symbol(to)) => EdgeKind::References {
                from,
                to,
                kind: contributed.kind,
            },
            // A `to` naming a whole file (no `symbol` set) is the file-liveness
            // edge — the template/asset shape. The contributed `RefKind` doesn't apply to a
            // file target and is dropped; liveness is the whole semantics.
            Some(NodeRef::File(to)) => EdgeKind::ReferencesFile { from, to },
            None => {
                record_dropped(dropped, "edge target", &contributed.to);
                continue;
            }
        };
        round.edges.push(Edge {
            kind,
            confidence: contributed.confidence,
            source: provenance.clone(),
            span: None,
            owner: tables.owner_of(from),
        });
    }
}

/// The noise ceiling: maximum findings per rule per run. Truncation is loud (a
/// diagnostic), never silent.
pub(crate) const FINDINGS_PER_RULE_CAP: usize = 500;

/// The finding round: every plugin with declared rules gets
/// `contribute_findings` over the same R1-scoped view the mutation hooks see. Runs AFTER
/// assembly on every path — cold build, incremental patch, and warm snapshot hit alike —
/// because findings are *output*, not graph state: nothing here is persisted, so nothing can
/// go stale. Zero rule-declaring plugins costs exactly one `rules()` sweep and nothing else.
pub(crate) fn run_finding_round(
    graph: &ProjectGraph,
    discovered: &discovery::DiscoveredTree,
    plugins: &[Box<dyn crate::plugin::Plugin>],
) -> (Vec<crate::plugin::ProtoFinding>, Vec<Diagnostic>) {
    let mut with_rules: Vec<(
        &dyn crate::plugin::Plugin,
        Vec<crate::plugin::RuleDescriptor>,
    )> = plugins
        .iter()
        .map(|p| (p.as_ref(), p.rules()))
        .filter(|(_, rules)| !rules.is_empty())
        .collect();
    if with_rules.is_empty() {
        return (Vec::new(), Vec::new());
    }
    with_rules.sort_by(|a, b| a.0.descriptor().id.cmp(&b.0.descriptor().id));

    // The same bare/qualified resolution environment the mutation round uses, rebuilt from
    // the graph (the patch path's own rebuild shape) so this works identically on paths
    // where assembly's live tables don't exist (warm snapshot hits).
    let (by_name, by_qualified) = finding_name_tables(graph);
    let view = crate::plugin::GraphView::new(
        &graph.files,
        &graph.symbols,
        &graph.file_index,
        &graph.packages,
        &graph.edges,
    );

    let mut findings = Vec::new();
    let mut diagnostics = Vec::new();
    for (plugin, mut rules) in with_rules {
        let descriptor = plugin.descriptor();
        // The rule-name charset is enforced at the declaration: an invalidly named rule is
        // excluded whole (its emissions then fall out as undeclared) — never silently.
        rules.retain(|r| {
            let valid = crate::plugin::is_valid_rule_name(&r.name);
            if !valid {
                diagnostics.push(finding_round_diagnostic(format!(
                    "plugin '{}' declares invalidly named rule '{}' (lower-kebab \
                     [a-z0-9-]+ required) — excluded",
                    descriptor.id, r.name
                )));
            }
            valid
        });
        let content = crate::plugin::ContentView::new(
            discovered,
            descriptor.id.clone(),
            &descriptor.requested_file_access,
        );
        let mut sink = crate::plugin::FindingSink::default();
        plugin.contribute_findings(&view, &content, &mut sink);
        collect_plugin_findings(
            &descriptor.id,
            &rules,
            sink,
            &FindingTables {
                graph,
                by_name: &by_name,
                by_qualified: &by_qualified,
            },
            &mut findings,
            &mut diagnostics,
        );
        if let Some(diagnostic) = content.take_diagnostic() {
            diagnostics.push(diagnostic);
        }
    }
    // Canonical order (the rule applied to output): identical runs produce
    // identical vectors regardless of plugin registration order.
    findings.sort_by(|a, b| {
        (&a.category, &a.path.0, &a.symbol, &a.message).cmp(&(
            &b.category,
            &b.path.0,
            &b.symbol,
            &b.message,
        ))
    });
    diagnostics.sort_unstable();
    (findings, diagnostics)
}

/// The finding round's resolution environment, bundled (same reason as [`PluginTargetTables`]).
pub(crate) struct FindingTables<'a> {
    pub(crate) graph: &'a ProjectGraph,
    pub(crate) by_name: &'a [HashMap<SmolStr, SymbolId>],
    pub(crate) by_qualified: &'a [HashMap<String, SymbolId>],
}

/// Bare + qualified symbol tables from the graph alone — the subset of the patch path's
/// rebuild the finding round needs (aliases included, from `patch_meta`).
#[allow(clippy::type_complexity)]
pub(crate) fn finding_name_tables(
    graph: &ProjectGraph,
) -> (
    Vec<HashMap<SmolStr, SymbolId>>,
    Vec<HashMap<String, SymbolId>>,
) {
    let mut by_name: Vec<HashMap<SmolStr, SymbolId>> = vec![HashMap::default(); graph.files.len()];
    let mut by_qualified: Vec<HashMap<String, SymbolId>> =
        vec![HashMap::default(); graph.files.len()];
    for (idx, sym) in graph.symbols.iter().enumerate() {
        index_symbol_name(sym, SymbolId(idx as u32), &mut by_name, &mut by_qualified);
    }
    for (i, meta) in graph.patch_meta.iter().enumerate() {
        for alias in &meta.reexport_aliases {
            by_name[i].entry(alias.name.clone()).or_insert(alias.symbol);
        }
    }
    (by_name, by_qualified)
}

pub(crate) fn index_symbol_name(
    sym: &SymbolNode,
    id: SymbolId,
    by_name: &mut [HashMap<SmolStr, SymbolId>],
    by_qualified: &mut [HashMap<String, SymbolId>],
) {
    let i = sym.file.0 as usize;
    match &sym.member_of {
        None => {
            by_name[i].insert(sym.name.clone(), id);
        }
        Some(owner) => {
            by_qualified[i].insert(format!("{owner}.{}", sym.name), id);
        }
    }
}

/// One plugin's sink → resolved, namespaced [`ProtoFinding`]s. Host-enforced
/// namespacing: the category is assembled from the plugin's *registered* id here — a
/// guest never supplies a category. An emitted rule that wasn't declared is dropped with a
/// diagnostic (declaration is the contract); an unresolvable target is dropped silently
/// (the uniform sink miss behavior).
pub(crate) fn collect_plugin_findings(
    plugin_id: &SmolStr,
    rules: &[crate::plugin::RuleDescriptor],
    sink: crate::plugin::FindingSink,
    tables: &FindingTables<'_>,
    findings: &mut Vec<crate::plugin::ProtoFinding>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut per_rule: HashMap<SmolStr, usize> = HashMap::default();
    let mut unknown_rules: std::collections::BTreeSet<SmolStr> = Default::default();
    for item in sink.items {
        let Some(rule) = rules.iter().find(|r| r.name == item.rule) else {
            unknown_rules.insert(item.rule);
            continue;
        };
        let count = per_rule.entry(rule.name.clone()).or_insert(0);
        *count += 1;
        if *count > FINDINGS_PER_RULE_CAP {
            continue; // the cap diagnostic below says how much was cut
        }
        if let Some(proto) = resolve_finding(plugin_id, rule, item, tables) {
            findings.push(proto);
        }
    }
    report_finding_drops(plugin_id, unknown_rules, per_rule, diagnostics);
}

/// The loud halves of the declaration contract and the noise ceiling — nothing here is
/// silent.
pub(crate) fn report_finding_drops(
    plugin_id: &SmolStr,
    unknown_rules: std::collections::BTreeSet<SmolStr>,
    per_rule: HashMap<SmolStr, usize>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for rule in unknown_rules {
        diagnostics.push(finding_round_diagnostic(format!(
            "plugin '{plugin_id}' emitted findings under undeclared rule '{rule}' — dropped \
             (rules must be declared via Plugin::rules)"
        )));
    }
    for (rule, count) in per_rule {
        if count > FINDINGS_PER_RULE_CAP {
            diagnostics.push(finding_round_diagnostic(format!(
                "plugin '{plugin_id}' rule '{rule}' emitted {count} findings — capped at \
                 {FINDINGS_PER_RULE_CAP}, {} dropped (the noise ceiling)",
                count - FINDINGS_PER_RULE_CAP
            )));
        }
    }
}

pub(crate) fn finding_round_diagnostic(message: String) -> Diagnostic {
    Diagnostic {
        level: crate::adapter::DiagnosticLevel::Warn,
        path: None,
        message,
        span: None,
    }
}

pub(crate) fn resolve_finding(
    plugin_id: &SmolStr,
    rule: &crate::plugin::RuleDescriptor,
    item: crate::plugin::ContributedFindingItem,
    tables: &FindingTables<'_>,
) -> Option<crate::plugin::ProtoFinding> {
    let node = resolve_plugin_target(
        &item.target,
        &tables.graph.file_index,
        tables.by_name,
        tables.by_qualified,
    )?;
    let (file, symbol, span, subject_kind) = match node {
        NodeRef::File(f) => (
            &tables.graph.files[f.0 as usize],
            None,
            None,
            "file".to_string(),
        ),
        NodeRef::Symbol(s) => {
            let sym = &tables.graph.symbols[s.0 as usize];
            (
                &tables.graph.files[sym.file.0 as usize],
                Some(sym.name.clone()),
                Some(sym.span),
                sym.kind.facet().to_string(),
            )
        }
    };
    Some(crate::plugin::ProtoFinding {
        category: format!("plugin:{plugin_id}/{}", rule.name),
        plugin_id: plugin_id.clone(),
        rule: rule.name.clone(),
        severity: rule.severity,
        confidence: item.confidence,
        message: item.message,
        path: file.path.clone(),
        symbol,
        span,
        subject_kind,
        package: tables.graph.package_name(file.package).map(str::to_string),
    })
}

pub(crate) fn collect_plugin_annotations(
    plugin: &dyn crate::plugin::Plugin,
    view: &crate::plugin::GraphView<'_>,
    content: &crate::plugin::ContentView<'_>,
    tables: &PluginTargetTables<'_>,
    round: &mut PluginRound,
    dropped: &mut Vec<String>,
) {
    let mut annotation_sink = crate::plugin::AnnotationSink::default();
    plugin.annotate_symbols(view, content, &mut annotation_sink);
    for target in annotation_sink.externally_consumed {
        if let Some(NodeRef::Symbol(id)) = tables.resolve(&target) {
            round.externally_consumed.push(id);
        } else {
            record_dropped(dropped, "annotation target", &target);
        }
    }
    for target in annotation_sink.implicitly_invoked {
        if let Some(NodeRef::Symbol(id)) = tables.resolve(&target) {
            round.implicitly_invoked.push(id);
        } else {
            record_dropped(dropped, "annotation target", &target);
        }
    }
}
