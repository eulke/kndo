//! Full assembly: claim/extract (phase 1), file/package nodes (phase 2), reference
//! resolution + linking (phase 3), the graph cache key, and the `assemble*` entry
//! points. Split from the original `graph.rs` verbatim - pure code motion.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::path::Path;

use rayon::prelude::*;

use crate::adapter::{
    Diagnostic, DiagnosticLevel, FileClaim, ImportSpec, LanguageAdapter, ManifestFacts,
    ProjectPath, RawRootTarget, Resolution, ResolveCtx, SourceFile,
};
use crate::discovery::{self, DiscoveryError};
use crate::vocab::{
    Confidence, DependencyId, Edge, EdgeKind, FileId, NodeRef, PackageId, Provenance, SymbolId,
};
use smol_str::SmolStr;

#[allow(unused_imports)]
use super::*;

/// One file's claim + extracted facts, plus which adapter produced them (by index into the
/// `adapters` slice passed to [`assemble`] — stable for the duration of one assembly call).
pub(crate) struct Claimed {
    pub(crate) claim: FileClaim,
    pub(crate) facts: crate::adapter::FileFacts,
    pub(crate) adapter_index: usize,
    /// The span-normalized surface signature, computed once per (adapter, content) in phase 1's
    /// parallel pass — cached and fresh facts get it identically.
    pub(crate) surface_sig: [u8; 32],
}

/// Phase 1's per-file work — claim, fetch-or-extract facts (facts cache first), compute the
/// surface signature — shared verbatim by the parallel full build and the incremental
/// patch's re-extraction of changed files.
pub(crate) fn claim_and_extract(
    df: &discovery::DiscoveredFile,
    adapters: &[Box<dyn LanguageAdapter>],
    cache: Option<&crate::cache::ProjectCache>,
    discovered: &discovery::DiscoveredTree,
) -> Result<Option<Claimed>, Diagnostic> {
    let Some((adapter_index, claim)) = adapters
        .iter()
        .enumerate()
        .find_map(|(i, a)| a.claim(&df.path).map(|c| (i, c)))
    else {
        return Ok(None); // no adapter claims it — still a valid, factless File node
    };
    let descriptor = adapters[adapter_index].descriptor();
    if let Some(facts) = cache.and_then(|c| {
        c.get(
            descriptor.id.as_str(),
            descriptor.facts_schema_version,
            &df.content_hash,
        )
    }) {
        let surface_sig = surface_signature(
            descriptor.id.as_str(),
            descriptor.facts_schema_version,
            &claim,
            &facts,
        );
        return Ok(Some(Claimed {
            claim,
            facts,
            adapter_index,
            surface_sig,
        }));
    }
    let content = discovered.read(&df.path).map_err(|e| Diagnostic {
        level: DiagnosticLevel::Warn,
        path: Some(df.path.clone()),
        message: format!(
            "claimed by {} but unreadable at extraction time ({e})",
            claim.language
        ),
        span: None,
    })?;
    let source = SourceFile {
        path: &df.path,
        content: &content,
    };
    let facts = adapters[adapter_index].extract(&source);
    if let Some(c) = cache {
        c.put(
            descriptor.id.as_str(),
            descriptor.facts_schema_version,
            &df.content_hash,
            &facts,
        );
    }
    let surface_sig = surface_signature(
        descriptor.id.as_str(),
        descriptor.facts_schema_version,
        &claim,
        &facts,
    );
    Ok(Some(Claimed {
        claim,
        facts,
        adapter_index,
        surface_sig,
    }))
}

/// One file's phase-3b output, merged deterministically in FileId order.
pub(crate) struct ResolvedFile {
    pub(crate) edges: Vec<Edge>,
    /// `(name, confidence, span, from, provenance)` — becomes `ImportsDependency` in the
    /// merge once the name has a deterministic id.
    pub(crate) dep_imports: Vec<(
        SmolStr,
        Confidence,
        crate::adapter::Span,
        FileId,
        Provenance,
    )>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) suppressions: Vec<(FileId, crate::adapter::RawSuppression)>,
}

// Whether a declaration in `decl_file` at `scope` is visible to a reference site in
// `site_file`. Scopes nest (File ⊂ Unit ⊂ Package ⊂ Public), so each arm
// accepts everything the narrower one would: a Unit-scoped Go method is visible to its own
// file whether or not the adapter set a unit key.
pub(crate) fn scope_contains_site(
    scope: crate::adapter::VisibilityScope,
    decl_file: usize,
    site_file: usize,
    file_unit: &[Option<SmolStr>],
    files: &[FileNode],
) -> bool {
    use crate::adapter::VisibilityScope::*;
    match scope {
        File => decl_file == site_file,
        Unit => {
            decl_file == site_file
                || matches!(
                    (&file_unit[decl_file], &file_unit[site_file]),
                    (Some(a), Some(b)) if a == b
                )
        }
        Package => files[decl_file].package == files[site_file].package,
        Public => true,
    }
}

/// Everything phase 3b's per-file resolution reads — immutable once the symbol tables are
/// built. A named struct (not captured locals) because the incremental patch
/// builds the same tables from the snapshot and calls the same [`resolve_file`]: one
/// resolution semantics, two data sources, zero drift.
pub(crate) struct ResolveTables<'a> {
    pub(crate) files: &'a [FileNode],
    pub(crate) file_index: &'a HashMap<ProjectPath, FileId>,
    pub(crate) symbols: &'a [SymbolNode],
    pub(crate) symbol_by_name_per_file: &'a [HashMap<SmolStr, SymbolId>],
    pub(crate) symbol_by_qualified_per_file: &'a [HashMap<String, SymbolId>],
    /// Extra declarations sharing a qualified selector already in the single-slot table
    /// (cfg-alternated twin impls: two `Data.from_path`) — populated only on collision.
    pub(crate) qualified_twins_per_file: &'a [HashMap<String, Vec<SymbolId>>],
    /// Per-file member-type facts: (owner, member) → the base type the
    /// access yields — what a dotted qualifier pointer resolves its hops through.
    pub(crate) member_types_per_file: &'a [MemberTypeIndex],
    pub(crate) symbol_by_name_per_unit: &'a HashMap<SmolStr, HashMap<SmolStr, SymbolId>>,
    pub(crate) member_by_name: &'a HashMap<SmolStr, Vec<SymbolId>>,
    pub(crate) file_unit: &'a [Option<SmolStr>],
    pub(crate) unit_name_by_file: &'a [Option<SmolStr>],
    pub(crate) ladders:
        &'a std::collections::BTreeMap<SmolStr, Vec<crate::adapter::VisibilityRung>>,
    /// Workspace executable name → entry file (the invoked-program rule) — what
    /// a file's `invoked_executables` names resolve against. See [`executable_name_index`].
    pub(crate) executable_by_name: &'a HashMap<SmolStr, FileId>,
    pub(crate) ctx: &'a ResolveCtx<'a>,
}

/// Every package's named executable targets as one name → entry-file index. First
/// declaration wins on a duplicate name (deterministic — packages iterate in discovery
/// order); an entry naming a file outside the discovered tree drops out, same as surfaces.
pub(crate) fn executable_name_index(
    packages: &[PackageNode],
    file_index: &HashMap<ProjectPath, FileId>,
) -> HashMap<SmolStr, FileId> {
    let mut index: HashMap<SmolStr, FileId> = HashMap::default();
    for exe in packages.iter().flat_map(|p| &p.executables) {
        if let Some(&file) = file_index.get(&exe.entry) {
            index.entry(exe.name.clone()).or_insert(file);
        }
    }
    index
}

/// One file's phase-3b contributions (imports, bindings, references, dynamics, diagnostics,
/// suppressions) — the parallel full build and the incremental patch both call this.
pub(crate) fn resolve_file(
    i: usize,
    facts: &crate::adapter::FileFacts,
    adapter: &dyn LanguageAdapter,
    t: &ResolveTables<'_>,
) -> ResolvedFile {
    let ResolveTables {
        files,
        file_index,
        symbols,
        symbol_by_name_per_file,
        symbol_by_qualified_per_file,
        qualified_twins_per_file,
        member_types_per_file: _,
        symbol_by_name_per_unit,
        member_by_name,
        file_unit,
        unit_name_by_file,
        ladders,
        executable_by_name,
        ctx,
    } = t;
    let file_id = FileId(i as u32);
    let provenance = || Provenance::Adapter(adapter.descriptor().id.clone());
    let mut out = ResolvedFile {
        edges: Vec::new(),
        dep_imports: Vec::new(),
        diagnostics: Vec::new(),
        suppressions: Vec::new(),
    };

    // Invoked-program edges: a declared subprocess invocation of a workspace
    // executable target, resolved by name against every manifest's declarations. The name
    // and the target are both declared facts → Certain; an unknown name emits nothing
    // (silence, never a guess).
    for name in &facts.invoked_executables {
        if let Some(&to) = executable_by_name.get(name) {
            out.edges.push(Edge {
                owner: file_id,
                kind: EdgeKind::InvokesFile {
                    from: NodeRef::File(file_id),
                    to,
                },
                confidence: Confidence::Certain,
                source: provenance(),
                span: None,
            });
        }
    }

    // Local name -> target symbol, from this file's import bindings — the fact that lets a
    // `RawReference` to an *imported* name resolve cross-file instead of only same-file.
    let mut bound_symbols: HashMap<SmolStr, SymbolId> = HashMap::default();
    // Qualifier -> resolved in-repo target file: the import's explicit
    // `local_alias`, or — unaliased — the *target's own* declared `unit_name`. This is
    // where the dir≠package problem dissolves: only assembly holds both sides, so the
    // qualifier for `gopkg.in/yaml.v3`-style imports comes from the target's `package`
    // clause, never from a guess about the specifier. First import wins on a duplicate
    // qualifier (Go rejects that program anyway — deterministic either way).
    // The bool is settling strength: a qualifier from an explicit alias or the target's
    // own declared unit name SETTLES resolution on a member miss (the name lives there or
    // nowhere); one derived from a specifier's last path segment is weaker provenance — a
    // same-named type in scope is entirely possible — so a miss falls through to the
    // in-scope/duck ladder instead (a tail-derived qualifier that settled a member miss
    // could bind the access to the wrong file and kill a live method).
    let mut qualifier_targets: HashMap<SmolStr, (FileId, bool)> = HashMap::default();
    // Units whose every top-level name this file sees bare (`RawImport::module_names_visible`
    // — Swift's `import SomeKit`): the bare-name fallback consults these unit tables after
    // the file's own, at Certain — it is the language's scoping rule, not a guess.
    let mut visible_units: Vec<SmolStr> = Vec::new();

    for imp in &facts.imports {
        let spec = ImportSpec {
            specifier: imp.specifier.clone(),
            from: files[i].path.clone(),
        };
        // A workspace-member resolution is BOTH targets at once: the
        // concrete internal file (reachability is real, cross-package) and the named
        // dependency (the declaration contract is real too — undeclared siblings are
        // phantom internal dependencies, declared-but-unimported ones are unused) —
        // EXCEPT within the importing file's own package (`same_package`), where no
        // self-declaration contract exists to validate: file edge and bindings only.
        // Stdlib: not a graph node — there is nothing to point an edge at. Unresolved:
        // resolution is intentionally incomplete right now (self-reference imports,
        // exports maps); turning it into a finding is the future `unresolved`
        // analysis's job, not assembly's.
        let (file_target, dep_target) = match adapter.resolve(&spec, ctx) {
            Resolution::File(path, confidence) => (Some((path, confidence)), None),
            Resolution::Dependency(name, confidence) => (None, Some((name, confidence))),
            Resolution::WorkspaceMember {
                name,
                target,
                confidence,
                same_package,
            } => (
                Some((target, confidence)),
                (!same_package).then_some((name, confidence)),
            ),
            Resolution::Stdlib | Resolution::Unresolved => (None, None),
        };

        if let Some((path, confidence)) = file_target {
            // Resolvers only ever match against `ctx`'s known-files set, so this
            // must be Some — defensive skip, not a silent contract violation, if not.
            if let Some(&to) = file_index.get(&path) {
                out.edges.push(Edge {
                    owner: file_id,
                    kind: EdgeKind::ImportsFile { from: file_id, to },
                    confidence,
                    source: provenance(),
                    span: Some(imp.span),
                });
                if imp.module_names_visible {
                    if let Some(unit) = &file_unit[to.0 as usize] {
                        if !visible_units.contains(unit) {
                            visible_units.push(unit.clone());
                        }
                    }
                }
                for binding in &imp.bindings {
                    let exported_name = binding
                        .imported
                        .clone()
                        .unwrap_or_else(|| SmolStr::new("default"));
                    // Same-file first; then the target file's own unit (package-scoped
                    // languages, `FileFacts::unit`) — a Go import names a
                    // *package* (a directory of files), and `Resolution::File`'s target is
                    // necessarily just one representative file in it (resolution has no
                    // multi-file target), so the symbol a qualified access binds
                    // to may live in any of that directory's other files.
                    let symbol_id = symbol_by_name_per_file[to.0 as usize]
                        .get(&exported_name)
                        .or_else(|| {
                            file_unit[to.0 as usize].as_ref().and_then(|unit| {
                                symbol_by_name_per_unit
                                    .get(unit)
                                    .and_then(|t| t.get(&exported_name))
                            })
                        })
                        .copied();
                    if let Some(symbol_id) = symbol_id {
                        bound_symbols.insert(binding.local.clone(), symbol_id);
                    }
                }
                let qualifier = imp
                    .local_alias
                    .clone()
                    .or_else(|| unit_name_by_file[to.0 as usize].clone())
                    .map(|q| (q, true))
                    // A path specifier (Rust inline module paths, Java static-import
                    // classes) puts its LAST segment in scope as the qualifier at the use
                    // site: `kndo_core::discovery::find_files_named(..)` reaches
                    // `discovery`'s file under the qualifier `discovery`, and a
                    // single-segment specifier (`selfy` from a path call `selfy::api()` —
                    // a package's own tests naming it by package name) is its own last
                    // segment — without this the reference's scope_context matched nothing
                    // and the whole path fell to the duck fallback (a path call into the
                    // resolved file would bind zero references and its target would read
                    // as dead). Non-settling (see qualifier_targets).
                    .or_else(|| {
                        imp.specifier
                            .rsplit("::")
                            .next()
                            .map(|q| (SmolStr::new(q), false))
                    });
                if let Some((q, settles)) = qualifier {
                    qualifier_targets.entry(q).or_insert((to, settles));
                }
                // The namespace escaped static tracking (`ns[key]`, ns passed
                // along) — every symbol in the target is plausibly used
                // ("wildcard over that namespace's exports").
                if imp.opaque_namespace_use {
                    out.edges.push(Edge {
                        owner: file_id,
                        kind: EdgeKind::Wildcard { from: to },
                        confidence: Confidence::Possible,
                        source: provenance(),
                        span: Some(imp.span),
                    });
                }
            }
        }
        if let Some((name, confidence)) = dep_target {
            // The dependency CLAIM is only as strong as the import that makes it: a
            // Possible-tier reconstructed module import (Rust locals-covered roots) must
            // not surface as a Certain dependency accusation downstream (`undeclared`
            // ignores the Possible tier).
            let confidence = confidence.min(imp.confidence);
            out.dep_imports
                .push((name, confidence, imp.span, file_id, provenance()));
        }
    }

    // Edge attribution: a reference carrying `within` is attributed to the
    // enclosing symbol it executes inside — resolved against this file's own declarations
    // (bare names, then the qualified member table, same convention as member root
    // targets). **Any miss falls back to file attribution — today's over-approximation,
    // the safe direction** (regression-tested; this fallback is the design's load-bearing
    // safety property). With symbol attribution, a dead function's calls don't keep
    // its callees alive: the execution rule ("a symbol-attributed reference
    // fires only when its symbol is reached") plus its module-load rule make transitive
    // death visible. `within: None` — module-level code, and every adapter that doesn't
    // emit the field — keeps file attribution: load-time references fire when the file
    // loads.
    //
    // Resolution order for the *target*: bound (imported) names first, then same-file
    // declarations, then same-unit siblings (`FileFacts::unit` — Go's package-scoped
    // visibility, absent for file-scoped languages) — real JS/TS can't have both of the
    // first two share a name at module scope, so that ordering is never actually contested
    // by valid code, just a defensive default; the unit fallback is the one genuinely load-
    // bearing case (a sibling file in the same Go package, no import involved at all).
    // No lookup models block/parameter shadowing: a same-named local could (incorrectly,
    // but safely — see module docs) resolve to an unrelated declaration.
    for reference in &facts.references {
        let from = reference
            .within
            .as_ref()
            .and_then(|within| {
                symbol_by_name_per_file[i]
                    .get(within)
                    .or_else(|| symbol_by_qualified_per_file[i].get(within.as_str()))
            })
            .map(|&s| NodeRef::Symbol(s))
            .unwrap_or(NodeRef::File(file_id));

        // Qualified references: `q.name` where `q` matches an import
        // qualifier resolves `name` inside that target (its own declarations, then its
        // unit siblings — a Go import names a package, and the symbol may live in any of
        // the package's files, then the target's member table under `q` itself — an alias
        // can name a TYPE in the target, and then `name` is that type's member: Rust's
        // `Thing::from_parts()` through `use crate::thing::Thing`) at Certain. Hit or
        // miss, a matched qualifier *settles* resolution — the name lives in that target
        // or nowhere; this file's own tables are never candidates. A qualifier matching
        // no import is a receiver expression (`t.helper()`): the name is a member access
        // by construction, so it skips the free-name tables and goes straight to the
        // duck-typed member fallback below — otherwise a same-file free function
        // sharing the member's name would (incorrectly, if safely) capture the
        // reference.
        let mut is_receiver_access = false;
        if let Some(q) = &reference.scope_context {
            // A weak (tail-derived) qualifier whose target yields nothing does NOT settle:
            // the name may belong to an in-scope type instead, so it takes the in-scope
            // ladder below exactly as an unregistered qualifier would (settling the miss
            // to the wrong file would kill a live method).
            let qualified_targets = qualifier_targets
                .get(q)
                .and_then(|&(target_file, settles)| {
                    let t = target_file.0 as usize;
                    let bare = symbol_by_name_per_file[t]
                        .get(&reference.name)
                        .or_else(|| {
                            file_unit[t].as_ref().and_then(|unit| {
                                symbol_by_name_per_unit
                                    .get(unit)
                                    .and_then(|tab| tab.get(&reference.name))
                            })
                        })
                        .copied();
                    let targets = match bare {
                        Some(s) => vec![s],
                        // The alias may name a TYPE rather than a module: resolve the
                        // qualifier itself as a symbol in the target (its bare table
                        // includes the re-export fixpoint's aliases, so a barrel-routed
                        // type lands on its original), then look the member up in the
                        // file where that symbol actually lives — `Mode::Standard`
                        // through `use crate::opts::{Mode}` reaches
                        // `types.rs`'s member table via `opts/mod.rs`'s alias. Twins
                        // included: cfg-alternated impls both own the selector.
                        None => symbol_by_name_per_file[t]
                            .get(q.as_str())
                            .map(|&type_symbol| {
                                let home = symbols[type_symbol.0 as usize].file.0 as usize;
                                qualified_member_targets(
                                    &symbol_by_qualified_per_file[home],
                                    &qualified_twins_per_file[home],
                                    format!("{q}.{}", reference.name).as_str(),
                                )
                            })
                            .unwrap_or_default(),
                    };
                    (!targets.is_empty() || settles).then_some(targets)
                });
            match qualified_targets {
                Some(targets) => {
                    for to in targets {
                        out.edges.push(Edge {
                            owner: file_id,
                            kind: EdgeKind::References {
                                from,
                                to,
                                kind: reference.kind,
                            },
                            confidence: Confidence::Certain,
                            source: provenance(),
                            span: Some(reference.span),
                        });
                    }
                    continue;
                }
                None => {
                    // Not an alias — but a NAME IN SCOPE used as a qualifier refers to
                    // that symbol itself: an imported binding (`Mode::Standard`
                    // after `use …::{…, Mode, …}`) or a same-file declaration (an
                    // adapter that typed a receiver rewrites `args.helper()` to qualifier
                    // `Args`, which may be declared right here). A DOTTED qualifier is a
                    // chained pointer (`Config.separator`): the
                    // receiver is the value that member YIELDS, resolved hop by hop
                    // through the member-type facts. Either way the members live in the
                    // resolved symbol's home file, keyed by its ORIGINAL name. A hit is
                    // Certain; a miss does NOT settle — a name in scope is a value/type,
                    // not a closed namespace, so an unknown member is a dynamic-looking
                    // access and falls through to the duck-typed fallback exactly like a
                    // receiver expression.
                    let targets = if q.contains('.') {
                        let (targets, yielded_types) =
                            chained_member_targets(q, &reference.name, &bound_symbols, i, t);
                        // Reaching a value THROUGH a member uses its type from this file —
                        // every resolved hop's type, not just the last (without these edges
                        // a type consumed only via fields read as file-local and
                        // `internal-only` advised narrowing it).
                        for ty in yielded_types {
                            out.edges.push(Edge {
                                owner: file_id,
                                kind: EdgeKind::References {
                                    from,
                                    to: ty,
                                    kind: crate::vocab::RefKind::Read,
                                },
                                confidence: Confidence::Certain,
                                source: provenance(),
                                span: Some(reference.span),
                            });
                        }
                        targets
                    } else {
                        in_scope_member_targets(q, &reference.name, &bound_symbols, i, t)
                    };
                    if !targets.is_empty() {
                        for to in targets {
                            out.edges.push(Edge {
                                owner: file_id,
                                kind: EdgeKind::References {
                                    from,
                                    to,
                                    kind: reference.kind,
                                },
                                confidence: Confidence::Certain,
                                source: provenance(),
                                span: Some(reference.span),
                            });
                        }
                        continue;
                    }
                    is_receiver_access = true;
                }
            }
        }

        let target = if is_receiver_access {
            None
        } else {
            bound_symbols
                .get(&reference.name)
                .or_else(|| symbol_by_name_per_file[i].get(&reference.name))
                .or_else(|| {
                    file_unit[i].as_ref().and_then(|unit| {
                        symbol_by_name_per_unit
                            .get(unit)
                            .and_then(|t| t.get(&reference.name))
                    })
                })
                .or_else(|| {
                    visible_units.iter().find_map(|unit| {
                        symbol_by_name_per_unit
                            .get(unit)
                            .and_then(|t| t.get(&reference.name))
                    })
                })
                .copied()
        };
        if let Some(to) = target {
            out.edges.push(Edge {
                owner: file_id,
                kind: EdgeKind::References {
                    from,
                    to,
                    kind: reference.kind,
                },
                confidence: Confidence::Certain,
                source: provenance(),
                span: Some(reference.span),
            });
            continue;
        }

        // Duck-typed member fallback (implementing the ladder rule
        // "duck-typed method with one candidate → probable"): an unresolved name that
        // matches member declarations plausibly targets any of them — extraction has no
        // receiver types, so honesty lives in the confidence, not in a guess. The
        // plausible set is scoped by each candidate's own declared
        // visibility: a member is a candidate iff its visibility scope *contains this reference
        // site* — an unexported Go method (scope Unit) only for sites in its own unit, a
        // public member (scope Public) project-wide. A rung the ladder doesn't cover
        // (index out of range, no ladder declared) counts as Public — the conservative
        // wider mapping: over-approximating who may see a member only adds keep-alive
        // edges. Cross-language candidates are excluded (a bare-name site never plausibly
        // calls another language's member — same reasoning as the ladder-index guard).
        // One candidate ⇒ Probable, several ⇒ Possible each — all get edges (conservative
        // keep-alive; dead-is-certain is untouched, since a member no call-site anywhere
        // matches still has zero edges).
        let candidates: Vec<SymbolId> = member_by_name
            .get(&reference.name)
            .map(|all| {
                all.iter()
                    .copied()
                    .filter(|&m| {
                        let sym = &symbols[m.0 as usize];
                        let j = sym.file.0 as usize;
                        if files[j].language != files[i].language {
                            return false;
                        }
                        let scope = files[j]
                            .language
                            .as_ref()
                            .and_then(|lang| ladders.get(lang))
                            .and_then(|ladder| ladder.get(sym.visibility.0 as usize))
                            .map(|rung| rung.scope)
                            .unwrap_or(crate::adapter::VisibilityScope::Public);
                        scope_contains_site(scope, j, i, file_unit, files)
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !candidates.is_empty() {
            let confidence = if candidates.len() == 1 {
                Confidence::Probable
            } else {
                Confidence::Possible
            };
            for to in candidates {
                out.edges.push(Edge {
                    owner: file_id,
                    kind: EdgeKind::References {
                        from, // same within-or-file attribution as the exact-match path
                        to,
                        kind: reference.kind,
                    },
                    confidence,
                    source: provenance(),
                    span: Some(reference.span),
                });
            }
        }
    }

    // Dynamic constructs → wildcard edges ("one mechanism, not two").
    // Un-narrowed (`eval`, `require(expr)` with no static prefix): a `Wildcard` edge from
    // this file — reachability expands it over the file's own symbols at `possible`.
    // Narrowed (`import(`./locales/${x}`)` → that directory): the plausible target set is
    // the directory's files instead, expressed with existing edge kinds — a `possible`
    // ImportsFile edge to every discovered file under the directory (unclaimed ones
    // included: a dynamically-loaded .json is a real target), plus a `Wildcard` edge
    // *from each target*, because a dynamically-imported module is consumed opaquely —
    // no binding names exist, so every symbol in it is plausibly used. Without that
    // second edge the target files would be alive but their exported symbols still
    // certain-dead: exactly the false positive the narrowing exists to prevent.
    for dynamic in &facts.dynamics {
        match dynamic.narrowed_to.as_deref().filter(|d| !d.is_empty()) {
            Some(dir) => {
                for (j, file) in files.iter().enumerate() {
                    if j == i || !package_owns(dir, core_dirname(file.path.0.as_str())) {
                        continue;
                    }
                    let target = FileId(j as u32);
                    out.edges.push(Edge {
                        owner: file_id,
                        kind: EdgeKind::ImportsFile {
                            from: file_id,
                            to: target,
                        },
                        confidence: Confidence::Possible,
                        source: provenance(),
                        span: Some(dynamic.span),
                    });
                    out.edges.push(Edge {
                        owner: file_id,
                        kind: EdgeKind::Wildcard { from: target },
                        confidence: Confidence::Possible,
                        source: provenance(),
                        span: Some(dynamic.span),
                    });
                }
            }
            // Empty-string narrowing would prefix-match the whole project — treat it as
            // the adapter meaning "no narrowing" rather than "everything".
            None => out.edges.push(Edge {
                owner: file_id,
                kind: EdgeKind::Wildcard { from: file_id },
                confidence: Confidence::Possible,
                source: provenance(),
                span: Some(dynamic.span),
            }),
        }
    }

    for d in &facts.diagnostics {
        out.diagnostics.push(Diagnostic {
            level: d.level,
            path: Some(files[i].path.clone()),
            message: d.message.clone(),
            span: d.span,
        });
    }

    for s in &facts.suppressions {
        out.suppressions.push((file_id, s.clone()));
    }

    out
}

/// One file's declaration-derived emissions — Declares edges, library/role export promotions
///, in-source roots, and function metrics — given the file's facts and its
/// already-assigned contiguous symbol run starting at `first_symbol`. THE single emitter for
/// this logic: the full build's pass B and the incremental patch both call it,
/// so the two paths cannot drift.
pub(crate) struct DeclarationEmissions {
    pub(crate) edges: Vec<Edge>,
    pub(crate) metrics: Vec<(SymbolId, SymbolMetrics)>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_file_declarations(
    i: usize,
    facts: &crate::adapter::FileFacts,
    adapter_id: &str,
    first_symbol: u32,
    symbols: &[SymbolNode],
    bare_table: &HashMap<SmolStr, SymbolId>,
    qualified_table: &HashMap<String, SymbolId>,
    library_root_files: &HashMap<FileId, Confidence>,
    role_root_files: &HashMap<FileId, crate::vocab::RootKind>,
    ladder: Option<&[crate::adapter::VisibilityRung]>,
) -> DeclarationEmissions {
    let file_id = FileId(i as u32);
    let provenance = || Provenance::Adapter(SmolStr::new(adapter_id));
    let mut edges = Vec::new();
    let mut metrics = Vec::new();

    for (d, decl) in facts.declarations.iter().enumerate() {
        let symbol_id = SymbolId(first_symbol + d as u32);
        edges.push(Edge {
            kind: EdgeKind::Declares {
                file: file_id,
                symbol: symbol_id,
            },
            confidence: Confidence::Certain,
            source: provenance(),
            span: Some(decl.span),
            owner: file_id,
        });

        // A constructor is engaged by *naming its type* (`new Foo()`, Swift's `Foo(...)`) —
        // no reference ever binds to the `<init>` symbol itself, so its liveness follows its
        // container's: a Certain References edge from the
        // container keeps the constructor — and everything its body references, like fields
        // assigned only in constructors — exactly as alive as the type, and exactly as dead.
        if decl.kind == crate::vocab::SymbolKind::Constructor {
            if let Some(&container) = decl.member_of.as_deref().and_then(|n| bare_table.get(n)) {
                edges.push(Edge {
                    kind: EdgeKind::References {
                        from: NodeRef::Symbol(container),
                        to: symbol_id,
                        kind: crate::vocab::RefKind::Call,
                    },
                    confidence: Confidence::Certain,
                    source: provenance(),
                    span: Some(decl.span),
                    owner: file_id,
                });
            }
        }

        // In-source Test roots, DERIVED: a declaration inside a test region
        // (`FileFacts::test_spans`) is test infrastructure — `#[test]` fns and everything in
        // a `#[cfg(test)]` module alike. The spans are the single producer-side declaration;
        // adapters never emit these roots themselves, so the two representations cannot
        // drift. Certain: the gate is declared in source, the runner is the consumer.
        if span_in_test_region(&facts.test_spans, decl.span) {
            edges.push(Edge {
                kind: EdgeKind::Root {
                    kind: crate::vocab::RootKind::Test,
                    target: NodeRef::Symbol(symbol_id),
                },
                confidence: Confidence::Certain,
                source: provenance(),
                span: Some(decl.span),
                owner: file_id,
            });
        }

        // Library-mode promotion: this file is a manifest-declared production
        // root and this symbol is exported from it, so it's part of the package's public
        // API — a production root in its own right, not just "alive because the file is."
        // Library-mode promotion is gated on surface transitivity: a
        // declaration is consumable API only if a re-export chain can actually carry it
        // outside the package — `pub(crate)`/`internal` satisfy `exported` yet are
        // definitionally walled in, so promoting them would fabricate surface.
        if decl.exported && rung_surface_transitive(ladder, decl.visibility) {
            if let Some(&confidence) = library_root_files.get(&file_id) {
                edges.push(Edge {
                    kind: EdgeKind::Root {
                        kind: crate::vocab::RootKind::Production,
                        target: NodeRef::Symbol(symbol_id),
                    },
                    confidence,
                    source: provenance(),
                    span: Some(decl.span),
                    owner: file_id,
                });
            }
        }

        // Role-derived promotion: a config file's exports ARE its interface to the tool that
        // loads it, and a test file's exports may be shared fixtures — the consumer is
        // outside the graph either way. Test-role files promote EVERY declaration, not just
        // exported ones: test frameworks reach members reflectively — XCTest and
        // JUnit discover `internal`/package-visible test methods inside test classes by
        // naming convention, so an unexported member of a test file being "unreachable" is
        // the runner's edge missing from the graph, never dead code. Tooling stays
        // exported-only.
        if let Some(&kind) = role_root_files.get(&file_id) {
            if kind == crate::vocab::RootKind::Test || decl.exported {
                edges.push(Edge {
                    kind: EdgeKind::Root {
                        kind,
                        target: NodeRef::Symbol(symbol_id),
                    },
                    confidence: Confidence::Probable,
                    source: provenance(),
                    span: Some(decl.span),
                    owner: file_id,
                });
            }
        }
    }

    // Callable shapes: adapter names resolve exactly like root targets — bare
    // table first, then the qualified member table; a no-match is dropped silently.
    for fm in &facts.functions {
        let resolved = bare_table
            .get(fm.symbol.as_str())
            .or_else(|| qualified_table.get(fm.symbol.as_str()));
        if let Some(&symbol_id) = resolved {
            metrics.push((
                symbol_id,
                SymbolMetrics {
                    cyclomatic: fm.cyclomatic,
                    loc: fm.loc,
                    token_count: fm.token_count,
                    fingerprints: fm.fingerprints.clone(),
                },
            ));
        }
    }

    // In-source roots (RawRoot), as distinct from manifest-declared ones: they target
    // something *within* the file being extracted, never a different file. Resolved
    // against ALL of this file's declarations sharing the name — a single-slot table
    // lookup silently dropped legitimate twins (two `impl Add for Stats` blocks each
    // declare `Stats.add`/`Stats.Output`; the adapter roots both, so both must land).
    // The index is built lazily, once, only when a Declaration-targeted root exists.
    let mut root_name_index: Option<HashMap<String, Vec<u32>>> = None;
    for root in &facts.roots {
        // Root-kind cap, the in-source half of phase 2.58: the adapter's "fn main is an
        // entry point" is a language fact; WHO invokes it is the file's role, and
        // `role_root_files` maps exactly the Test/Tooling files (Production files are
        // absent). Because this emitter is shared, the incremental patch inherits the
        // identical behavior for free.
        let kind = if root.kind == crate::vocab::RootKind::Production {
            role_root_files.get(&file_id).copied().unwrap_or(root.kind)
        } else {
            root.kind
        };
        let mut emit_root = |target: NodeRef| {
            let span = match target {
                NodeRef::Symbol(s) => Some(symbols[s.0 as usize].span),
                NodeRef::File(_) => None,
            };
            edges.push(Edge {
                kind: EdgeKind::Root { kind, target },
                confidence: root.confidence,
                source: provenance(),
                span,
                owner: file_id,
            });
        };
        match &root.target {
            RawRootTarget::WholeFile => emit_root(NodeRef::File(file_id)),
            RawRootTarget::Declaration(name) => {
                let index = root_name_index
                    .get_or_insert_with(|| declaration_name_index(&facts.declarations));
                for &d in index.get(name.as_str()).map(Vec::as_slice).unwrap_or(&[]) {
                    emit_root(NodeRef::Symbol(SymbolId(first_symbol + d)));
                }
            }
        }
    }

    DeclarationEmissions { edges, metrics }
}

/// Insert into a file's single-slot qualified table, preserving twins: a selector already
/// present keeps last-wins semantics in the table (unchanged), and the displaced
/// declaration lands in the twins side-map — cfg-alternated impls legitimately declare
/// `Data.from_path` twice, and a qualified reference targets whichever is compiled.
pub(crate) fn insert_qualified(
    table: &mut HashMap<String, SymbolId>,
    twins: &mut HashMap<String, Vec<SymbolId>>,
    key: String,
    id: SymbolId,
) {
    match table.entry(key) {
        std::collections::hash_map::Entry::Occupied(mut slot) => {
            let displaced = std::mem::replace(slot.get_mut(), id);
            twins.entry(slot.key().clone()).or_default().push(displaced);
        }
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert(id);
        }
    }
}

/// A plain in-scope qualifier (`Mode::Standard`, or a receiver typed `Args`): the
/// symbol the name resolves to — an import binding or a same-file declaration — owns the
/// member in its home file.
pub(crate) fn in_scope_member_targets(
    q: &str,
    name: &str,
    bound_symbols: &HashMap<SmolStr, SymbolId>,
    i: usize,
    t: &ResolveTables<'_>,
) -> Vec<SymbolId> {
    let in_scope = bound_symbols
        .get(q)
        .or_else(|| t.symbol_by_name_per_file[i].get(q));
    match in_scope {
        Some(&bound) => {
            let owner = &t.symbols[bound.0 as usize];
            let home = owner.file.0 as usize;
            qualified_member_targets(
                &t.symbol_by_qualified_per_file[home],
                &t.qualified_twins_per_file[home],
                format!("{}.{}", owner.name, name).as_str(),
            )
        }
        None => Vec::new(),
    }
}

/// A dotted qualifier pointer `Base.member` (the cross-file tier): the
/// reference's receiver is the value that member YIELDS. Every hop is a declared-annotation
/// fact — the base name in the reference's scope, `yields` from the OWNER's home file's
/// member-type facts, the yielded type name resolved in that same home (annotations mean
/// what they mean where they were written), and the final member in the yielded type's
/// home, twins included. Any miss yields no targets — duck fallback, never a settle.
pub(crate) fn chained_member_targets(
    pointer: &str,
    name: &str,
    bound_symbols: &HashMap<SmolStr, SymbolId>,
    i: usize,
    t: &ResolveTables<'_>,
) -> (Vec<SymbolId>, Vec<SymbolId>) {
    let mut segments = pointer.split('.');
    let base = segments.next().unwrap_or("");
    let base_symbol = bound_symbols
        .get(base)
        .or_else(|| t.symbol_by_name_per_file[i].get(base));
    let Some(&base_symbol) = base_symbol else {
        return (Vec::new(), Vec::new());
    };
    let mut current = base_symbol;
    let mut yielded_types = Vec::new();
    for segment in segments {
        let Some(next) = chain_hop(current, segment, bound_symbols, i, t) else {
            return (Vec::new(), yielded_types);
        };
        current = next;
        yielded_types.push(next);
    }
    let home = t.symbols[current.0 as usize].file.0 as usize;
    let members = qualified_member_targets(
        &t.symbol_by_qualified_per_file[home],
        &t.qualified_twins_per_file[home],
        format!("{}.{}", t.symbols[current.0 as usize].name, name).as_str(),
    );
    (members, yielded_types)
}

/// One hop of a chained pointer: the member's declared yields on the CURRENT type — the
/// indexed type parameter when the segment carries a `?N` projection marker — resolved to a type
/// symbol. The yielded type name resolves where the annotation was WRITTEN (the owner's
/// home: its declarations and re-export aliases), then in the reference site's own scope —
/// the home's import bindings are resolve-time-local and invisible here, but the common
/// shape (the field's type imported by the very file doing the access) makes the site's
/// bindings the right stand-in.
pub(crate) fn chain_hop(
    current: SymbolId,
    segment: &str,
    bound_symbols: &HashMap<SmolStr, SymbolId>,
    i: usize,
    t: &ResolveTables<'_>,
) -> Option<SymbolId> {
    let (member, projection) = split_projection(segment);
    let owner = &t.symbols[current.0 as usize];
    let home = owner.file.0 as usize;
    let fact = t.member_types_per_file[home].get(&(owner.name.clone(), SmolStr::new(member)))?;
    let type_name = hop_type_name(fact, projection)?;
    resolve_annotation_name(type_name, home, bound_symbols, i, t)
}

/// A segment's projection marker, purely structural: `member` → none, `member?` → parameter
/// 0, `member?N` → parameter N. WHICH parameter an operation extracts is the emitting
/// adapter's knowledge — the core only selects.
pub(crate) fn split_projection(segment: &str) -> (&str, Option<usize>) {
    let Some((member, index)) = segment.split_once('?') else {
        return (segment, None);
    };
    match projection_index(index) {
        Some(n) => (member, Some(n)),
        None => (segment, None),
    }
}

/// The marker's parameter index: bare `?` is shorthand for `?0`.
pub(crate) fn projection_index(index: &str) -> Option<usize> {
    if index.is_empty() {
        Some(0)
    } else {
        index.parse().ok()
    }
}

/// The type a hop lands on: the projected type parameter under a `?N` marker, the yielded
/// type itself otherwise.
pub(crate) fn hop_type_name(
    fact: &(SmolStr, Vec<SmolStr>),
    projection: Option<usize>,
) -> Option<&SmolStr> {
    match projection {
        Some(n) => fact.1.get(n),
        None => Some(&fact.0),
    }
}

/// An annotation's type name resolved where it was written (the home's declarations and
/// re-export aliases), then in the reference site's own scope (see [`chain_hop`]'s doc).
pub(crate) fn resolve_annotation_name(
    type_name: &SmolStr,
    home: usize,
    bound_symbols: &HashMap<SmolStr, SymbolId>,
    i: usize,
    t: &ResolveTables<'_>,
) -> Option<SymbolId> {
    t.symbol_by_name_per_file[home]
        .get(type_name.as_str())
        .or_else(|| bound_symbols.get(type_name.as_str()))
        .or_else(|| t.symbol_by_name_per_file[i].get(type_name.as_str()))
        .copied()
}

/// One file's member-type facts as a lookup: (owner, member) → (yields, yields_params).
pub(crate) type MemberTypeIndex = HashMap<(SmolStr, SmolStr), (SmolStr, Vec<SmolStr>)>;

pub(crate) fn index_member_types(entries: &[crate::adapter::RawMemberType]) -> MemberTypeIndex {
    entries
        .iter()
        .map(|m| {
            (
                (m.owner.clone(), m.member.clone()),
                (m.yields.clone(), m.yields_params.clone()),
            )
        })
        .collect()
}

/// Every declaration a qualified selector names in `home` — the table's winner plus any
/// displaced twins. Empty when the selector names nothing there.
pub(crate) fn qualified_member_targets(
    qualified: &HashMap<String, SymbolId>,
    twins: &HashMap<String, Vec<SymbolId>>,
    key: &str,
) -> Vec<SymbolId> {
    let mut targets: Vec<SymbolId> = qualified.get(key).copied().into_iter().collect();
    if let Some(extra) = twins.get(key) {
        targets.extend(extra.iter().copied());
    }
    targets
}

/// Every declaration of a file keyed by its root-target selector — the bare name for free
/// declarations, `Owner.name` for members — with EVERY declaration index per key (twins
/// preserved: multiple trait impls legitimately re-declare the same member selector).
pub(crate) fn declaration_name_index(
    declarations: &[crate::adapter::Declaration],
) -> HashMap<String, Vec<u32>> {
    let mut index: HashMap<String, Vec<u32>> = HashMap::default();
    for (d, decl) in declarations.iter().enumerate() {
        let key = match &decl.member_of {
            Some(owner) => format!("{owner}.{}", decl.name),
            None => decl.name.to_string(),
        };
        index.entry(key).or_default().push(d as u32);
    }
    index
}

/// The span-normalized surface signature: everything about a file that OTHER
/// files' resolution — or this file's own derived-id stability — can depend on, hashed;
/// bodies, spans, references, metrics, and suppressions excluded, so they move freely under
/// the patch guard. serde+bincode gives an unambiguous
/// byte layout without hand-rolled framing.
pub(crate) fn surface_signature(
    adapter_id: &str,
    facts_schema_version: u32,
    claim: &FileClaim,
    facts: &crate::adapter::FileFacts,
) -> [u8; 32] {
    /// One import's surface tuple: specifier, kind, side_effect_only, type_only,
    /// confidence, bindings (local, imported), reexported, opaque_namespace_use, local_alias,
    /// in-test-region (derived from `FileFacts::test_spans` containment — see the View site).
    type ImportView<'a> = (
        &'a str,
        &'a crate::adapter::ImportKind,
        bool,
        bool,
        Confidence,
        Vec<(&'a str, Option<&'a str>)>,
        bool,
        bool,
        Option<&'a str>,
        bool,
    );

    /// One declaration's surface tuple: name, kind, exported, visibility, member_of,
    /// nested_scope, visibility_inherited (the scope-shape facts feed visibility verdicts
    /// the way the level itself does — see the View site).
    type DeclarationView<'a> = (
        &'a str,
        &'a crate::vocab::SymbolKind,
        bool,
        crate::adapter::VisibilityLevel,
        Option<&'a str>,
        bool,
        bool,
    );

    #[derive(serde::Serialize)]
    struct View<'a> {
        adapter_id: &'a str,
        facts_schema_version: u32,
        language: &'a str,
        class: crate::vocab::FileClass,
        detected_origin: Option<crate::vocab::FileOrigin>,
        unit: Option<&'a str>,
        unit_name: Option<&'a str>,
        declarations: Vec<DeclarationView<'a>>,
        imports: Vec<ImportView<'a>>,
        roots: Vec<(
            crate::vocab::RootKind,
            &'a crate::adapter::RawRootTarget,
            Confidence,
        )>,
        dynamics: Vec<(&'a str, Option<&'a str>)>,
        /// Member-type facts are cross-file resolution inputs: a changed
        /// field/return annotation changes what other files' chained qualifiers resolve to.
        member_types: Vec<(&'a str, &'a str, &'a str, Vec<&'a str>)>,
    }
    let view = View {
        adapter_id,
        facts_schema_version,
        language: claim.language.as_str(),
        class: claim.class,
        detected_origin: facts.detected_origin,
        unit: facts.unit.as_deref(),
        unit_name: facts.unit_name.as_deref(),
        declarations: facts
            .declarations
            .iter()
            .map(|d| {
                (
                    d.name.as_str(),
                    &d.kind,
                    d.exported,
                    d.visibility,
                    d.member_of.as_deref(),
                    // Scope-shape facts feed visibility verdicts the way the level itself
                    // does — a declaration moving in or out of a nested scope (or its
                    // container) must decline the patch.
                    d.nested_scope,
                    d.visibility_inherited,
                )
            })
            .collect(),
        imports: facts
            .imports
            .iter()
            .map(|i| {
                (
                    i.specifier.as_str(),
                    &i.kind,
                    i.side_effect_only,
                    i.type_only,
                    i.confidence,
                    i.bindings
                        .iter()
                        .map(|b| (b.local.as_str(), b.imported.as_deref()))
                        .collect(),
                    i.reexported,
                    i.opaque_namespace_use,
                    i.local_alias.as_deref(),
                    // Span-derived but span-*stable*: pure reformatting preserves whether an
                    // import sits inside a test region; a move across the boundary changes
                    // resolution-relevant behavior (phase 2.55's demotion, hygiene's site
                    // role) and must decline the patch.
                    span_in_test_region(&facts.test_spans, i.span),
                )
            })
            .collect(),
        roots: facts
            .roots
            .iter()
            .map(|r| (r.kind, &r.target, r.confidence))
            .collect(),
        dynamics: facts
            .dynamics
            .iter()
            .map(|d| (d.reason.as_str(), d.narrowed_to.as_deref()))
            .collect(),
        member_types: facts
            .member_types
            .iter()
            .map(|m| {
                (
                    m.owner.as_str(),
                    m.member.as_str(),
                    m.yields.as_str(),
                    m.yields_params.iter().map(SmolStr::as_str).collect(),
                )
            })
            .collect(),
    };
    let bytes = bincode::serialize(&view).unwrap_or_default();
    *blake3::hash(&bytes).as_bytes()
}

/// Directory part of a project-relative path (`""` for root-level files). A private duplicate
/// of `kndo-adapter-toolkit::paths::dirname` — trivial string logic, but the core cannot depend
/// on an adapter-side crate (the ignorance rule runs both directions: adapters depend on the
/// core, never the reverse). `pub(crate)`: sibling modules (analyses doing their own directory
/// reasoning, e.g. `unused`'s rollup) reuse it rather than re-deriving the same logic.
pub(crate) fn core_dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// A manifest's resolved target files (`PackageNode::targets` / `WorkspaceMember::targets`):
/// the union of its root targets (bins) and import entries (lib/main/module/exports),
/// deduped in declaration order. Shared by the full-build member index and the package-node
/// builder so both carry the same anchors.
pub(crate) fn manifest_targets(
    facts: &crate::adapter::ManifestFacts,
) -> Vec<crate::adapter::ProjectPath> {
    let mut targets: Vec<crate::adapter::ProjectPath> = Vec::new();
    for path in facts
        .roots
        .iter()
        .map(|r| &r.target)
        .chain(facts.resolved_entries.iter().map(|(path, _)| path))
    {
        if !targets.contains(path) {
            targets.push(path.clone());
        }
    }
    targets
}

/// Does `ancestor_dir` govern (contain, at any depth, or equal) `dir`? The empty (project-root)
/// dir governs everything. Doubles as both the nearest-manifest-ancestor test (this
/// module's own use) and a generic "is A an ancestor-or-self of B" check other analyses reuse
/// (e.g. `unused`'s directory rollup, deciding whether a narrower rollup is already covered by
/// a wider one).
pub(crate) fn package_owns(manifest_dir: &str, file_dir: &str) -> bool {
    manifest_dir.is_empty()
        || file_dir == manifest_dir
        || file_dir
            .strip_prefix(manifest_dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Phase 2b's worker: re-roles a production file as test when its path, relative to the
/// directory of its owning package's manifest, starts with one of the claiming adapter's
/// `package_test_dirs` — the package-relative half of test-dir classification (the anywhere-
/// in-the-path half stays in claim-time patterns). The implicit no-manifest package anchors
/// at the project root, so a manifest-less tree keeps its root `tests/` convention.
/// Promotion only — any file already test- or tooling-role keeps that verdict, so a nested
/// package's sources sitting under an ancestor's `tests/` tree (already Production by
/// ownership) is the case this exists to protect, and a demotion could never be right.
pub(crate) fn promote_package_relative_test_roles<'a>(
    files: &mut [FileNode],
    packages: &[PackageNode],
    package_test_dirs_of: impl Fn(usize) -> Option<&'a [SmolStr]>,
) {
    for (i, file) in files.iter_mut().enumerate() {
        let Some(dirs) = package_test_dirs_of(i).filter(|d| !d.is_empty()) else {
            continue;
        };
        let Some(class) = &mut file.class else {
            continue;
        };
        if class.role != crate::vocab::FileRole::Production {
            continue;
        }
        let manifest_dir = packages[file.package.0 as usize]
            .manifest
            .as_ref()
            .map(|m| core_dirname(m.0.as_str()))
            .unwrap_or("");
        let path = file.path.0.as_str();
        let relative = if manifest_dir.is_empty() {
            path
        } else {
            // Ownership (phase 2a) guarantees the prefix; a mismatch means a caller bug,
            // and skipping is the conservative answer.
            match path
                .strip_prefix(manifest_dir)
                .and_then(|rest| rest.strip_prefix('/'))
            {
                Some(rest) => rest,
                None => continue,
            }
        };
        // First *directory* segment only: a file literally named like a test dir
        // (`tests` with no extension, say) has no trailing `/` and never matches.
        if let Some((first_dir, _)) = relative.split_once('/') {
            if dirs.iter().any(|d| d == first_dir) {
                class.role = crate::vocab::FileRole::Test;
            }
        }
    }
}

/// Bumped whenever the *persisted* shape of a graph snapshot changes in a way that isn't
/// already covered by an adapter's own `facts_schema_version` — e.g. a new node/edge kind, or
/// an assembly-algorithm change that could produce a different graph from the same facts. Feeds
/// [`compute_graph_key`] (the "core graph-schema version"); a bump here invalidates
/// every project's cached `graph.bin` on the next run, same as any other key-input change.
pub const GRAPH_SCHEMA_VERSION: u32 = 28; // bump whenever the persisted snapshot shape (rkyv layouts included) or the assembly semantics that derive a graph from the same facts change

/// The graph snapshot's cache key (`cache.rs`'s `graph.bin`): a single digest
/// folding in the *whole* discovered file set (every path + content hash — this already
/// subsumes "manifest hashes," since a manifest is just one more discovered file, and a
/// plugin's content-channel reads too, since a `ContentView` never answers a
/// path outside this same discovered set), each registered adapter's id and facts-schema
/// version, each registered *graph-mutating* plugin's identity (id,
/// declared version, and — WASM only — component content hash, [`Plugin::content_hash`]), and
/// [`GRAPH_SCHEMA_VERSION`] itself. A kndo config hash is deliberately not folded in:
/// every knob the config subsystem reads (`crate::config`) acts strictly post-assembly —
/// analysis tuning, report filtering, thread counts — so the same tree assembles to the
/// same graph under any config, and a config edit must not evict a valid snapshot.
///
/// Every variable-length field (paths, adapter/plugin ids) is length-prefixed before its bytes
/// so the scheme is unambiguous by construction, not merely collision-resistant by luck of the
/// input distribution — two different file sets can never fold to the same byte stream before
/// hashing.
pub(crate) fn compute_graph_key(
    discovered_files: &[discovery::DiscoveredFile],
    adapters: &[Box<dyn LanguageAdapter>],
    graph_mutating_plugins: &[&dyn crate::plugin::Plugin],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&GRAPH_SCHEMA_VERSION.to_le_bytes());
    fold_discovered_files(&mut hasher, discovered_files);
    fold_adapter_versions(&mut hasher, adapters);
    fold_plugin_identities(&mut hasher, graph_mutating_plugins);
    *hasher.finalize().as_bytes()
}

/// Identity digest of a graph-mutating plugin set alone — the same
/// [`fold_plugin_identities`] term [`compute_graph_key`] folds, standalone. Stored with every
/// snapshot and checked by the incremental patch: plugin *edges* are
/// provenance-tagged and re-derived by the patch, but `classify_file` overrides are baked
/// into `FileNode.class` untagged, so a snapshot is patchable only under the identical
/// plugin set. The empty set folds to a fixed, non-zero digest — "no plugins" is itself an
/// identity, distinguishable from any real set.
pub(crate) fn plugin_set_digest(graph_mutating_plugins: &[&dyn crate::plugin::Plugin]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    fold_plugin_identities(&mut hasher, graph_mutating_plugins);
    *hasher.finalize().as_bytes()
}

/// `discovered_files` is already sorted by path (discovery.rs's own determinism invariant), so
/// this fold is stable across runs regardless of filesystem walk order.
pub(crate) fn fold_discovered_files(
    hasher: &mut blake3::Hasher,
    discovered_files: &[discovery::DiscoveredFile],
) {
    for f in discovered_files {
        let path_bytes = f.path.0.as_bytes();
        hasher.update(&(path_bytes.len() as u32).to_le_bytes());
        hasher.update(path_bytes);
        hasher.update(&f.content_hash);
    }
}

pub(crate) fn fold_adapter_versions(
    hasher: &mut blake3::Hasher,
    adapters: &[Box<dyn LanguageAdapter>],
) {
    let mut adapter_versions: Vec<(String, u32)> = adapters
        .iter()
        .map(|a| {
            let d = a.descriptor();
            (d.id.to_string(), d.facts_schema_version)
        })
        .collect();
    adapter_versions.sort();
    for (id, version) in &adapter_versions {
        hasher.update(&(id.len() as u32).to_le_bytes());
        hasher.update(id.as_bytes());
        hasher.update(&version.to_le_bytes());
    }
}

// Sorted by id: `graph_mutating_plugins` is already the caller's `sorted_plugins` slice
// (assembled sorted-by-id for the deterministic hook order), but re-sorting a small
// collection here costs nothing and doesn't make this function trust a caller invariant it
// can't see.
pub(crate) fn fold_plugin_identities(
    hasher: &mut blake3::Hasher,
    graph_mutating_plugins: &[&dyn crate::plugin::Plugin],
) {
    let mut plugin_identities: Vec<(String, String, Option<[u8; 32]>)> = graph_mutating_plugins
        .iter()
        .map(|p| {
            let d = p.descriptor();
            (d.id.to_string(), d.version.to_string(), p.content_hash())
        })
        .collect();
    plugin_identities.sort();
    for (id, version, content_hash) in &plugin_identities {
        hasher.update(&(id.len() as u32).to_le_bytes());
        hasher.update(id.as_bytes());
        hasher.update(&(version.len() as u32).to_le_bytes());
        hasher.update(version.as_bytes());
        fold_optional_content_hash(hasher, *content_hash);
    }
}

// A present/absent content hash must fold differently than an all-zero one would — an explicit
// tag byte, not a sentinel value that could collide with a real hash.
pub(crate) fn fold_optional_content_hash(
    hasher: &mut blake3::Hasher,
    content_hash: Option<[u8; 32]>,
) {
    match content_hash {
        Some(h) => {
            hasher.update(&[1u8]);
            hasher.update(&h);
        }
        None => {
            hasher.update(&[0u8]);
        }
    }
}

/// Discovers, claims, extracts, resolves, and links — the full assembly pipeline up to
/// (not including) analyses. Diagnostics accumulate rather than abort: a graph that omits one
/// unreadable file's facts is far more useful than no graph at all. Always cold
/// (no facts cache consulted) — see [`assemble_with_cache`] for the warm path.
pub fn assemble(
    root: &Path,
    adapters: &[Box<dyn LanguageAdapter>],
    plugins: &[Box<dyn crate::plugin::Plugin>],
) -> Result<(ProjectGraph, Vec<Diagnostic>), DiscoveryError> {
    assemble_with_cache(root, adapters, plugins, None)
}

/// Same pipeline as [`assemble`], additionally consulting/populating a facts
/// cache: a file whose content hash already has a cached-and-current entry skips
/// re-parsing entirely, which is the warm path's dominant win since parsing dominates cold-run
/// cost. `cache: None` is exactly [`assemble`]'s behavior — this must hold
/// byte-for-byte, since `--no-cache` ≡ cached results is a correctness gate.
pub fn assemble_with_cache(
    root: &Path,
    adapters: &[Box<dyn LanguageAdapter>],
    plugins: &[Box<dyn crate::plugin::Plugin>],
    cache: Option<&crate::cache::ProjectCache>,
) -> Result<(ProjectGraph, Vec<Diagnostic>), DiscoveryError> {
    let assembled = assemble_from_source(
        &discovery::TreeSource::Directory(root),
        adapters,
        plugins,
        cache,
    )?;
    // This convenience entry point persists inline — only the engine's own path defers the
    // write to a background thread (it owns a place to join it; callers here don't).
    if let Some(pending) = &assembled.pending_snapshot {
        pending.persist_now(
            &assembled.graph,
            &assembled.extraction_diagnostics,
            &assembled.plugin_diagnostics,
        );
    }
    let mut diagnostics = assembled.discovery_diagnostics;
    diagnostics.extend(assembled.extraction_diagnostics);
    diagnostics.extend(assembled.plugin_diagnostics);
    Ok((assembled.graph, diagnostics))
}

/// [`assemble_with_cache`] over any [`discovery::TreeSource`] — a directory, or a git tree-ish
/// read in memory (diff modes). Everything past discovery is source-blind:
/// identical content produces identical facts, hashes, ids, and findings whether the bytes came
/// from disk or the object database.
pub fn assemble_from_source(
    source: &discovery::TreeSource<'_>,
    adapters: &[Box<dyn LanguageAdapter>],
    plugins: &[Box<dyn crate::plugin::Plugin>],
    cache: Option<&crate::cache::ProjectCache>,
) -> Result<AssembledGraph, DiscoveryError> {
    // Only graph-mutating plugins (`Plugin::mutates_graph`) participate in assembly at all —
    // both in the hook call sites below AND in the cache/patch bypass decision. Filtering here,
    // at the single entry point, is what makes the declaration self-enforcing: a plugin
    // claiming `false` never has its hooks called, so it can't be the reason a cached graph
    // is stale. Deterministic call order — no ordering-constraints field exists on
    // `PluginDescriptor`, so id order is the rule:
    // sort by id once, reused by every hook site below instead of re-sorting per phase.
    let mut sorted_plugins: Vec<&dyn crate::plugin::Plugin> = plugins
        .iter()
        .filter(|p| p.mutates_graph())
        .map(|p| p.as_ref())
        .collect();
    sorted_plugins.sort_by(|a, b| a.descriptor().id.cmp(&b.descriptor().id));
    let sorted_plugins = sorted_plugins.as_slice();

    let mut phase_start = std::time::Instant::now();
    let mut timings: Vec<(&'static str, u64)> = Vec::new();
    let mut tick = |label: &'static str, start: &mut std::time::Instant| {
        timings.push((label, start.elapsed().as_micros() as u64));
        *start = std::time::Instant::now();
    };
    let known_blob_hashes = cache.map(|c| c.load_blob_hashes()).unwrap_or_default();
    let stat_index = cache.and_then(|c| c.load_stat_index());
    tick("sidecar-load", &mut phase_start);
    let mut discovered =
        discovery::discover_source(source, &known_blob_hashes, stat_index.as_ref())?;
    // Discovery diagnostics are always the fresh walk's — they never enter the
    // snapshot, whose stored diagnostics are extraction + manifest only (the producers warm
    // paths skip). One composition rule for hit, patch, and full alike.
    let mut discovery_diagnostics = std::mem::take(&mut discovered.diagnostics);
    discovery_diagnostics.sort_unstable();
    if let Some(cache) = cache {
        // Persist fresh (git blob → blake3) pairs immediately — the graph-snapshot hit below
        // returns early, and the sidecar must grow even on runs that never reach extraction.
        cache.save_blob_hashes(&discovered.new_blob_hashes);
        // Same for the stat sidecar, rewritten wholesale (the current
        // file set IS the index) — but only when something actually changed: on a no-op run
        // every entry matched, and rewriting a 50k-entry sidecar costs more than the stat
        // fast path saves.
        let index_current = stat_index
            .as_ref()
            .is_some_and(|i| i.is_current_for(&discovered.stat_entries));
        if !index_current {
            cache.save_stat_index(&discovered.stat_entries, discovered.stat_written_at_ns);
        }
    }
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // The graph-snapshot fast path: if every input the key folds in —
    // the whole discovered file set, each registered adapter's identity/version, each
    // graph-mutating plugin's identity, and the graph schema itself — matches the
    // last snapshot exactly, skip claim/extract/resolve/link entirely and hand back the
    // persisted graph. Any mismatch (a single changed byte anywhere is enough) is a plain miss;
    // there's no partial reuse yet, only all-or-nothing.
    let graph_key = compute_graph_key(&discovered.files, adapters, sorted_plugins);
    let current_plugin_digest = plugin_set_digest(sorted_plugins);
    tick("discovery", &mut phase_start);
    // Neither fast path bypasses on plugins:
    //
    // - **Snapshot reuse**: plugin identity (id, declared version, and — WASM only —
    //   component content hash) folds into `graph_key` above. Any input a plugin's hooks
    //   could react to — every source/config file its content channel might read is already
    //   part of `discovered.files`, hence already in the key — or the plugin binary/component
    //   itself changing, already changes the key, so a snapshot written under one key can
    //   never be served back for a run whose plugin set (or any of its content-channel-visible
    //   inputs) differs.
    // - **Patch reuse**: `try_patch` strips every `Provenance::Plugin` edge and the
    //   `externally_consumed` set from the previous snapshot, splices the source change, and
    //   re-runs the plugin round (`run_plugin_round` — the same function the full build calls)
    //   against the patched graph, so no plugin contribution ever rides a patch unrevised. The
    //   one plugin effect that can't be stripped — `classify_file` overrides baked into
    //   `FileNode.class` untagged — is covered by the snapshot's stored plugin-set digest:
    //   `try_patch` refuses when the set changed, and `classify_file` is path-only, so under an
    //   identical set its decisions are identical too.
    //
    // `sorted_plugins` is already filtered to `mutates_graph()` plugins, not the raw registry —
    // a coverage-only plugin (the lcov ingester, say) costs neither fast path anything.
    if let Some(cache) = cache {
        if let Some((graph, graph_diagnostics)) = cache.get_graph(&graph_key) {
            tick("snapshot-load", &mut phase_start);
            // The finding round runs on the WARM path too: findings are
            // output, not graph state — nothing persisted, nothing to go stale.
            let (plugin_findings, finding_diagnostics) =
                run_finding_round(&graph, &discovered, plugins);
            return Ok(AssembledGraph {
                graph,
                discovery_diagnostics,
                extraction_diagnostics: graph_diagnostics,
                plugin_diagnostics: Vec::new(),
                plugin_findings,
                finding_diagnostics,
                pending_snapshot: None,
                timings,
                plugin_contributions: None,
            });
        }
        tick("snapshot-probe", &mut phase_start);
        // On a key miss, try the incremental patch off the previous snapshot —
        // any guard failure falls through to the full rebuild below, the one fallback.
        if let Some((graph, extraction_diagnostics, plugin_diagnostics, plugin_contributions)) =
            try_patch(
                &discovered,
                adapters,
                sorted_plugins,
                current_plugin_digest,
                cache,
            )
        {
            tick("patch", &mut phase_start);
            let pending_snapshot = cache
                .graph_writer(graph_key, current_plugin_digest)
                .map(PendingSnapshot);
            let (plugin_findings, finding_diagnostics) =
                run_finding_round(&graph, &discovered, plugins);
            return Ok(AssembledGraph {
                graph,
                discovery_diagnostics,
                extraction_diagnostics,
                plugin_diagnostics,
                plugin_findings,
                finding_diagnostics,
                pending_snapshot,
                timings,
                plugin_contributions: Some(plugin_contributions),
            });
        }
        tick("patch-probe", &mut phase_start);
    }

    let known_files: HashSet<ProjectPath> =
        discovered.files.iter().map(|f| f.path.clone()).collect();

    // Phase 1 — claim + extract, in parallel. rayon's collect preserves input order (the
    // path-sorted order discovery already established), so the FileId assignment in phase 2
    // stays deterministic regardless of which file's extraction happens to finish first
    // (parallel compute, deterministic reduce). A facts-cache hit/miss changes
    // only *how* `facts` is obtained, never the order or shape of this collection — cached and
    // freshly-extracted facts are indistinguishable to every phase downstream.
    let outcomes: Vec<Result<Option<Claimed>, Diagnostic>> = discovered
        .files
        .par_iter()
        .map(|df| claim_and_extract(df, adapters, cache, &discovered))
        .collect();

    let mut claimed_per_file: Vec<Option<Claimed>> = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        match outcome {
            Ok(c) => claimed_per_file.push(c),
            Err(d) => {
                diagnostics.push(d);
                claimed_per_file.push(None);
            }
        }
    }

    // Phase 1b — claim + extract manifests, in parallel, mirroring phase 1. A manifest never
    // gets a `FileClaim`/language of its own (manifests are never claimed as
    // source) — it contributes `ManifestFacts` through this wholly separate path. Entry-point
    // resolution only needs the known-files index, not declared dependencies (manifest
    // roots are always filesystem-relative, never a bare-package lookup), so a plain `ctx`
    // suffices here — the dependency-augmented one phase 3 needs is built after this collects.
    let manifest_ctx = ResolveCtx::new(&known_files);
    let manifest_outcomes: Vec<Result<Option<(usize, ManifestFacts)>, Diagnostic>> = discovered
        .files
        .par_iter()
        .map(|df| {
            // Every claiming adapter extracts (a `build.gradle` is Java's AND Kotlin's — the
            // module's `.kt` files under Gradle's shared source sets are the second claimer's
            // to promote; first-claimer-only would silently drop Kotlin's library
            // surface). The FIRST claimer owns package identity/topology; later
            // claimers contribute only their `roots`, deduplicated.
            let claimers: Vec<usize> = adapters
                .iter()
                .enumerate()
                .filter(|(_, a)| a.claim_manifest(&df.path))
                .map(|(i, _)| i)
                .collect();
            let Some(&adapter_index) = claimers.first() else {
                return Ok(None);
            };
            let content = discovered.read(&df.path).map_err(|e| Diagnostic {
                level: DiagnosticLevel::Warn,
                path: Some(df.path.clone()),
                message: format!("manifest unreadable at extraction time ({e})"),
                span: None,
            })?;
            let source = SourceFile {
                path: &df.path,
                content: &content,
            };
            let mut facts = adapters[adapter_index].extract_manifest(&source, &manifest_ctx);
            for &other in &claimers[1..] {
                let extra = adapters[other].extract_manifest(&source, &manifest_ctx);
                for root in extra.roots {
                    if !facts
                        .roots
                        .iter()
                        .any(|r| r.target == root.target && r.kind == root.kind)
                    {
                        facts.roots.push(root);
                    }
                }
            }
            Ok(Some((adapter_index, facts)))
        })
        .collect();

    let mut manifests_per_file: Vec<Option<(usize, ManifestFacts)>> =
        Vec::with_capacity(manifest_outcomes.len());
    for outcome in manifest_outcomes {
        match outcome {
            Ok(m) => manifests_per_file.push(m),
            Err(d) => {
                diagnostics.push(d);
                manifests_per_file.push(None);
            }
        }
    }

    tick("extract", &mut phase_start);

    // Manifest unit overrides: per-target unit assignment the path convention
    // can't derive — SwiftPM's `.target(name:, path:)`. Longest matching prefix wins;
    // applied only where extraction left `unit` unset (the convention, where it fired,
    // already told the truth).
    let mut unit_overrides: Vec<(crate::adapter::ProjectPath, SmolStr)> = manifests_per_file
        .iter()
        .flatten()
        .flat_map(|(_, m)| m.unit_overrides.iter().cloned())
        .collect();
    unit_overrides.sort_by_key(|a| std::cmp::Reverse(a.0 .0.len()));
    let override_unit = |path: &str| -> Option<SmolStr> {
        unit_overrides
            .iter()
            .find(|(prefix, _)| {
                path.strip_prefix(prefix.0.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            })
            .map(|(_, unit)| unit.clone())
    };

    // Phase 2 — assign FileId (already the discovery-sorted index) and build File nodes.
    let mut files = Vec::with_capacity(discovered.files.len());
    let mut file_index =
        HashMap::with_capacity_and_hasher(discovered.files.len(), Default::default());
    for (i, df) in discovered.files.iter().enumerate() {
        let file_id = FileId(i as u32);
        file_index.insert(df.path.clone(), file_id);
        let (language, class, unit) = match &claimed_per_file[i] {
            Some(c) => {
                // Content-derived origin override: extraction saw the bytes,
                // claim only saw the path — the content wins on the origin axis. Applied here,
                // before role-derived roots (phase 2.6) and every analysis, so all origin
                // exemptions see the corrected value. Role is never content-corrected.
                let mut class = c.claim.class;
                if let Some(origin) = c.facts.detected_origin {
                    class.origin = origin;
                }
                // Plugin classify_file: ecosystem convention beats the language
                // default (`*.stories.tsx` -> tooling). Sorted-by-id plugin order (the
                // determinism rule — no topological-ordering-constraints field
                // exists); each plugin sees the
                // prior one's answer, so a later plugin can refine an earlier one's override.
                for plugin in sorted_plugins {
                    if let Some(overridden) = plugin.classify_file(&df.path, class) {
                        class = overridden;
                    }
                }
                (
                    Some(c.claim.language.clone()),
                    Some(class),
                    c.facts
                        .unit
                        .clone()
                        .or_else(|| override_unit(df.path.0.as_str())),
                )
            }
            None => (None, None, None),
        };
        let test_spans = match &claimed_per_file[i] {
            Some(c) => {
                let mut spans = c.facts.test_spans.clone();
                spans.sort_unstable(); // canonical order invariant
                spans
            }
            None => Vec::new(),
        };
        let string_call_sites = match &claimed_per_file[i] {
            Some(c) => {
                let mut sites = c.facts.string_call_args.clone();
                sites.sort_unstable(); // canonical order invariant
                sites
            }
            None => Vec::new(),
        };
        files.push(FileNode {
            path: df.path.clone(),
            content_hash: df.content_hash,
            language,
            class,
            package: PackageId(0), // patched in phase 2a once ownership is computed
            unit,
            test_spans,
            string_call_sites,
        });
    }

    // Phase 2a — packages and ownership: one implicit `Package` covering
    // whatever no real manifest's subtree claims (index 0 — "a repo with no manifest at all
    // is one implicit Package" generalizes to "the part of any repo no manifest governs"),
    // plus one `Package` per manifest found. Ownership is nearest-manifest-ancestor, resolved
    // by trying manifest directories deepest-first so a nested manifest shadows its parent.
    let mut packages = vec![PackageNode {
        manifest: None,
        name: None,
        private: false,
        declares_surface: false,
        surface: Vec::new(),
        workspace_entry: None,
        targets: Vec::new(),
        executables: Vec::new(),
        resolves_dependency_usage: true,
    }];
    let mut manifest_package: Vec<Option<PackageId>> = vec![None; manifests_per_file.len()];
    for (i, slot) in manifests_per_file.iter().enumerate() {
        if let Some((adapter_index, facts)) = slot {
            let package_id = PackageId(packages.len() as u32);
            // The declared surface as FileIds: entries naming files outside the
            // discovered tree (published build artifacts in a source checkout) drop out here —
            // an absent surface file can never be imported in-repo, so nothing is lost.
            let surface = facts
                .resolved_entries
                .iter()
                .filter_map(|(path, _)| file_index.get(path).copied())
                .collect();
            packages.push(PackageNode {
                manifest: Some(files[i].path.clone()),
                name: facts.package_name.clone(),
                private: facts.private,
                declares_surface: facts.declares_surface,
                surface,
                workspace_entry: facts.resolved_entries.first().cloned(),
                targets: manifest_targets(facts),
                executables: facts.executables.clone(),
                resolves_dependency_usage: adapters[*adapter_index]
                    .descriptor()
                    .resolves_dependency_usage,
            });
            manifest_package[i] = Some(package_id);
        }
    }
    let mut manifest_dirs: Vec<(String, PackageId)> = manifest_package
        .iter()
        .enumerate()
        .filter_map(|(i, pkg)| {
            pkg.map(|id| (core_dirname(files[i].path.0.as_str()).to_string(), id))
        })
        .collect();
    manifest_dirs.sort_by_key(|(dir, _)| std::cmp::Reverse(dir.len()));
    for file in files.iter_mut() {
        let file_dir = core_dirname(file.path.0.as_str());
        file.package = manifest_dirs
            .iter()
            .find(|(manifest_dir, _)| package_owns(manifest_dir, file_dir))
            .map(|&(_, id)| id)
            .unwrap_or(PackageId(0));
    }

    // Phase 2b — package-relative test-dir promotion (`AdapterDescriptor::package_test_dirs`):
    // runs right after ownership because the manifest that owns the file is the anchor the
    // convention binds to, and before phases 2.5/2.55/2.6 so every role consumer sees the
    // corrected value. Full-build only, like phase 2.55's demotion: the patch path preserves
    // `FileNode::class` and declines whenever any promotion input could move (file set,
    // manifests, claims are all patch guards).
    let adapter_package_test_dirs: Vec<Vec<SmolStr>> = adapters
        .iter()
        .map(|a| a.descriptor().package_test_dirs)
        .collect();
    promote_package_relative_test_roles(&mut files, &packages, |i| {
        claimed_per_file[i]
            .as_ref()
            .map(|c| adapter_package_test_dirs[c.adapter_index].as_slice())
    });

    // Phase 2.5 — manifest roots and declared dependencies, sequentially in file-discovery
    // order for determinism (same reasoning as phase 3 below). Declared dependencies feed the
    // stdlib-shadowing precedence rule that phase 3's resolver calls check against.
    let mut edges = Vec::new();
    let mut declared_dependency_names: HashSet<SmolStr> = HashSet::default();
    let mut declared_dependencies: Vec<DeclaredDependency> = Vec::new();
    let mut script_invoked_dependencies: HashSet<(PackageId, SmolStr)> = HashSet::default();
    // Every file a manifest names as a production root, at that root's own confidence — used
    // after phase 3a to promote the file's *exported* symbols to production roots too
    // ("Published/library: its public API is a production root — external consumers
    // exist by definition"). Keyed by file, keeping the strongest confidence when more than
    // one manifest field roots the same file (e.g. both `main` and an `exports` leaf).
    let mut library_root_files: HashMap<FileId, Confidence> = HashMap::default();
    // The shared version pool `inherited` dependencies resolve against (Cargo:
    // `[workspace.dependencies]`) — gathered across every manifest up front, since a member
    // manifest's inherited dep can be discovered before the manifest that declares the pool
    // (file-discovery order isn't "root first"). Adapter-agnostic on purpose: this loop only
    // ever asks "does some manifest's `workspace_dependencies` know this name," never
    // anything about Cargo specifically.
    let mut workspace_dependency_versions: HashMap<&SmolStr, &SmolStr> = HashMap::default();
    for slot in manifests_per_file.iter() {
        let Some((_, facts)) = slot else { continue };
        for dep in &facts.workspace_dependencies {
            workspace_dependency_versions.insert(&dep.name, &dep.version_req);
        }
    }
    for (i, slot) in manifests_per_file.iter().enumerate() {
        let Some((adapter_index, facts)) = slot else {
            continue;
        };
        let provenance = || Provenance::Adapter(adapters[*adapter_index].descriptor().id.clone());
        // Set alongside this manifest's own PackageNode a few lines above — always Some here.
        let package = manifest_package[i].unwrap_or(PackageId(0));
        for dep in &facts.dependencies {
            declared_dependency_names.insert(dep.name.clone());
            // `inherited` deps carry a placeholder `version_req` (the real value lives in
            // whichever manifest declared the shared pool) — resolve it now, before
            // version-skew or any other consumer ever sees it. If no pool entry exists
            // (defensive: no manifest declared one, or the name isn't in it), fall back to
            // the placeholder as-is — no worse than today's unresolved behavior.
            let version_req = if dep.inherited {
                workspace_dependency_versions
                    .get(&dep.name)
                    .map(|v| (*v).clone())
                    .unwrap_or_else(|| dep.version_req.clone())
            } else {
                dep.version_req.clone()
            };
            declared_dependencies.push(DeclaredDependency {
                package,
                manifest: files[i].path.clone(),
                name: dep.name.clone(),
                version_req,
                scope: dep.scope,
            });
        }
        for name in &facts.script_invoked_names {
            script_invoked_dependencies.insert((package, name.clone()));
        }
        for root in &facts.roots {
            // Resolved against `manifest_ctx`'s known-files set, so this must be Some —
            // defensive skip, not a silent contract violation, if not (same stance as the
            // `Resolution::File` lookup in phase 3).
            if let Some(&target) = file_index.get(&root.target) {
                edges.push(Edge {
                    owner: FileId(i as u32),
                    kind: EdgeKind::Root {
                        kind: root.kind,
                        target: NodeRef::File(target),
                    },
                    confidence: root.confidence,
                    source: provenance(),
                    span: None, // manifest-declared root: a marker, no extraction-time span
                });
                if root.kind == crate::vocab::RootKind::Production {
                    let entry = library_root_files.entry(target).or_insert(root.confidence);
                    *entry = (*entry).max(root.confidence);
                }
            }
        }
        for d in &facts.diagnostics {
            diagnostics.push(Diagnostic {
                level: d.level,
                path: Some(files[i].path.clone()),
                message: d.message.clone(),
                span: d.span,
            });
        }
    }

    // Phase 2.55 — test-gated module demotion (the whole-file case of
    // `FileFacts::test_spans`): `#[cfg(test)] mod tests;` puts an entire *file* behind a
    // test gate, which the path-based claim cannot see. A claimed-production file becomes
    // test-role when at least one module-linking import (side-effect import binding a module
    // name — Rust's `mod foo;` / `#[path]`) reaches it from inside a test region and NO
    // module link reaches it from production code. Runs before phase 2.6 so the demoted file
    // gets its Test root and every role consumer downstream sees the corrected value. Patch
    // parity: the per-import "test-gated" bit is part of the surface signature, so any change
    // to the gating declines the patch and the preserved `FileNode::class` stays truthful.
    // Gated on any-test-spans-present: corpora without sub-file tests skip the pass entirely.
    if claimed_per_file
        .iter()
        .flatten()
        .any(|c| !c.facts.test_spans.is_empty())
    {
        let ctx = ResolveCtx::new(&known_files);
        // Per target: (reached from a test region, reached from production).
        let mut links: HashMap<FileId, (bool, bool)> = HashMap::default();
        for (i, slot) in claimed_per_file.iter().enumerate() {
            let Some(c) = slot else { continue };
            for imp in c
                .facts
                .imports
                .iter()
                .filter(|imp| imp.side_effect_only && imp.local_alias.is_some())
            {
                let spec = crate::adapter::ImportSpec {
                    specifier: imp.specifier.clone(),
                    from: files[i].path.clone(),
                };
                let Resolution::File(path, _) = adapters[c.adapter_index].resolve(&spec, &ctx)
                else {
                    continue;
                };
                let Some(&target) = file_index.get(&path) else {
                    continue;
                };
                let entry = links.entry(target).or_insert((false, false));
                if span_in_test_region(&c.facts.test_spans, imp.span) {
                    entry.0 = true;
                } else {
                    entry.1 = true;
                }
            }
        }
        for (target, (from_test, from_production)) in links {
            if from_test && !from_production {
                if let Some(class) = &mut files[target.0 as usize].class {
                    if class.role == crate::vocab::FileRole::Production {
                        class.role = crate::vocab::FileRole::Test;
                    }
                }
            }
        }
    }

    // Phase 2.58 — root-kind cap: a manifest-declared Production root whose target file the
    // adapter classified Test/Tooling takes the role's kind. The manifest says "this is an
    // entry point" — a language fact, kept; the role says WHO consumes it — the project
    // fact that decides the KIND. A tooling bin (xtask) is a tooling entry point, not
    // production surface; reporting its internals as "production-reachable but untested"
    // would be a false statement. Runs after 2.55 so a content-demoted Test role is
    // honored, and before 2.6/3a so every downstream consumer — reachability, library-root
    // promotion, untested — sees the capped kind. Deliberately exempt, by evidence
    // hierarchy: plugin-contributed roots (targeted consumer knowledge, materialized after
    // both cap sites) and surface promotions ("production API re-exports this"); instead
    // the cap removes the demoted file from `library_root_files`, so no promotion chain
    // ever *starts* from a tooling/test bin — one reached by a genuine production
    // re-export chain legitimately re-enters later.
    for edge in &mut edges {
        let EdgeKind::Root {
            kind,
            target: NodeRef::File(f),
        } = &mut edge.kind
        else {
            continue;
        };
        if *kind != crate::vocab::RootKind::Production {
            continue; // non-Production kinds are never touched (a Test root is real)
        }
        let capped = match files[f.0 as usize].class.map(|c| c.role) {
            Some(crate::vocab::FileRole::Test) => crate::vocab::RootKind::Test,
            Some(crate::vocab::FileRole::Tooling) => crate::vocab::RootKind::Tooling,
            _ => continue,
        };
        *kind = capped;
        library_root_files.remove(f);
    }

    // Phase 2.6 — role-derived roots (literally): "Test roots — test
    // functions/files (language role detection…)"; "Tooling roots — build/config scripts
    // (webpack.config…)". The adapter's role classification *is* the seed for these two root
    // kinds — the runner/tool that consumes the file lives outside the graph, so the file's
    // existence under the convention is the whole evidence. `Probable`, not certain: a
    // convention names the file, nothing declares it (same reasoning as `exports`-map leaves).
    // Production roots stay manifest/API-driven (phase 2.5) — never role-derived. Reads the
    // *node's* class, not the raw claim — phase 2.55's demotion and the origin
    // override are already applied there.
    let mut role_root_files: HashMap<FileId, crate::vocab::RootKind> = HashMap::default();
    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        let Some(class) = files[i].class else {
            continue;
        };
        let kind = match class.role {
            crate::vocab::FileRole::Test => crate::vocab::RootKind::Test,
            crate::vocab::FileRole::Tooling => crate::vocab::RootKind::Tooling,
            crate::vocab::FileRole::Production => continue,
        };
        let file_id = FileId(i as u32);
        edges.push(Edge {
            owner: file_id,
            kind: EdgeKind::Root {
                kind,
                target: NodeRef::File(file_id),
            },
            confidence: Confidence::Probable,
            source: Provenance::Adapter(adapters[claimed.adapter_index].descriptor().id.clone()),
            span: None, // role-derived root: the convention names the file, nothing spans it
        });
        role_root_files.insert(file_id, kind);
    }

    // Each claimed language's visibility ladder, off its claiming adapter's
    // descriptor — keyed by claim language (what `FileNode::language` stores), BTreeMap for
    // deterministic order. Only languages with at least one claimed file appear: an unused
    // adapter's ladder is dead data. Built before phase 3 because the member fallback (3b)
    // scopes its candidates by ladder rung.
    let mut ladders: std::collections::BTreeMap<SmolStr, Vec<crate::adapter::VisibilityRung>> =
        std::collections::BTreeMap::new();
    let mut cycle_policies: std::collections::BTreeMap<SmolStr, crate::adapter::CyclePolicy> =
        std::collections::BTreeMap::new();
    for slot in claimed_per_file.iter().flatten() {
        let descriptor = adapters[slot.adapter_index].descriptor();
        ladders
            .entry(slot.claim.language.clone())
            .or_insert(descriptor.visibility_ladder);
        cycle_policies
            .entry(slot.claim.language.clone())
            .or_insert(descriptor.cycle_policy);
    }

    // Phase 3a — symbols (Declares edges) and in-source roots, sequentially in FileId order.
    // Split from imports/references (phase 3b) because resolving a reference or an import
    // binding to *another* file's symbol needs that file's symbol table already built —
    // forward references (file 0 importing from file 5) are the common case, not an edge case,
    // so every file's declarations must exist before any file's imports are resolved.
    let mut symbols = Vec::new();
    let mut function_metrics: Vec<(SymbolId, SymbolMetrics)> = Vec::new();
    let mut symbol_by_name_per_file: Vec<HashMap<SmolStr, SymbolId>> =
        vec![HashMap::default(); claimed_per_file.len()];
    // Package-scoped (not file-scoped) resolution, for languages where it's the ordinary case
    // rather than an edge case (Go's directory-is-the-package visibility unit — see
    // `FileFacts::unit`'s doc). `None` for every file whose adapter doesn't set `unit` (JS/TS
    // today), so this is purely additive: those files never populate or consult these two maps.
    let mut file_unit: Vec<Option<SmolStr>> = vec![None; claimed_per_file.len()];
    let mut patch_meta: Vec<FilePatchMeta> = vec![FilePatchMeta::default(); claimed_per_file.len()];
    let mut symbol_by_name_per_unit: HashMap<SmolStr, HashMap<SmolStr, SymbolId>> =
        HashMap::default();
    // Member declarations (`member_of: Some(..)`) resolve on a separate track:
    // an unqualified reference must never `certain`-resolve to a member (bare member names
    // collide across owners by construction — `T.get` and `U.get` are both just `get`), so
    // members stay OUT of the exact-name tables above and live here, name → every same-named
    // member project-wide; phase 3b's duck-typed fallback narrows the set per site by each
    // candidate's declared visibility scope. Qualified lookup (for `RawRoot` targets naming
    // `Owner.name`) gets its own exact table.
    let mut member_by_name: HashMap<SmolStr, Vec<SymbolId>> = HashMap::default();
    let mut symbol_by_qualified_per_file: Vec<HashMap<String, SymbolId>> =
        vec![HashMap::default(); claimed_per_file.len()];
    let mut qualified_twins_per_file: Vec<HashMap<String, Vec<SymbolId>>> =
        vec![HashMap::default(); claimed_per_file.len()];
    // Workspace-member index: every *named* manifest in the graph, keyed by
    // package name, with its directory and adapter-resolved primary entry — what lets a bare
    // specifier (`@org/ui`) resolve to the sibling's internal files instead of an external
    // dependency. Built from manifest facts, consumed by import resolution — strictly after
    // manifest extraction, so no circularity. Duplicate names keep the first in
    // file-discovery order (deterministic); a repo with two same-named manifests is broken
    // in ways no resolution order fixes.
    let mut workspace_member_index: HashMap<SmolStr, crate::adapter::WorkspaceMember> =
        HashMap::default();
    for (i, slot) in manifests_per_file.iter().enumerate() {
        let Some((_, facts)) = slot else { continue };
        let Some(name) = &facts.package_name else {
            continue;
        };
        workspace_member_index
            .entry(name.clone())
            .or_insert_with(|| crate::adapter::WorkspaceMember {
                dir: SmolStr::new(core_dirname(files[i].path.0.as_str())),
                entry: facts.resolved_entries.first().cloned(),
                targets: manifest_targets(facts),
            });
    }

    // Unit reverse-index (Java) — see try_patch's identical
    // construction for why this mirrors the patch path byte-for-byte.
    let mut unit_index: HashMap<SmolStr, Vec<ProjectPath>> = HashMap::default();
    for f in &files {
        if let Some(u) = &f.unit {
            unit_index
                .entry(u.clone())
                .or_default()
                .push(f.path.clone());
        }
    }
    for fs in unit_index.values_mut() {
        fs.sort();
    }
    let ctx = ResolveCtx::new(&known_files)
        .with_declared_dependencies(&declared_dependency_names)
        .with_workspace_members(&workspace_member_index)
        .with_units(&unit_index);

    // Phase 2.7 — library-surface expansion (completing the library mode): a
    // package-surface file's *whole-surface* re-exports — `pub mod x;` in Rust, `export *
    // from './x'` in a published JS package: `reexported` with no named bindings — extend the
    // surface into the target file, transitively to a fixpoint. Each expansion emits a
    // production Root edge for the target file, owned by the re-exporting file: reachability
    // consumes it directly, pass B's export promotion picks the target up from
    // `library_root_files` exactly like a manifest-named root, and the incremental patch
    // re-derives membership from the kept edges. Without this, any library whose
    // API lives behind a public module tree — every real Rust crate — reads as dead.
    {
        let mut work: Vec<FileId> = {
            let mut v: Vec<FileId> = library_root_files.keys().copied().collect();
            v.sort();
            v
        };
        while let Some(f) = work.pop() {
            let Some(claimed) = &claimed_per_file[f.0 as usize] else {
                continue;
            };
            let confidence = library_root_files[&f];
            let adapter = &adapters[claimed.adapter_index];
            for imp in claimed
                .facts
                .imports
                .iter()
                .filter(|i| i.reexported && i.bindings.is_empty())
            {
                let spec = ImportSpec {
                    specifier: imp.specifier.clone(),
                    from: files[f.0 as usize].path.clone(),
                };
                let target_path = match adapter.resolve(&spec, &ctx) {
                    Resolution::File(path, _) => path,
                    Resolution::WorkspaceMember { target, .. } => target,
                    _ => continue,
                };
                let Some(&target) = file_index.get(&target_path) else {
                    continue;
                };
                edges.push(Edge {
                    kind: EdgeKind::Root {
                        kind: crate::vocab::RootKind::Production,
                        target: NodeRef::File(target),
                    },
                    confidence,
                    source: Provenance::Adapter(adapter.descriptor().id.clone()),
                    span: Some(imp.span),
                    owner: f,
                });
                use std::collections::hash_map::Entry;
                match library_root_files.entry(target) {
                    Entry::Vacant(slot) => {
                        slot.insert(confidence);
                        work.push(target);
                    }
                    Entry::Occupied(mut slot) => {
                        let merged = (*slot.get()).max(confidence);
                        slot.insert(merged);
                    }
                }
            }
        }
    }

    // Pass A — tables + symbol nodes, sequentially in FileId order (SymbolId assignment is
    // order itself). Emissions (Declares edges, promotions, in-source roots, metrics) moved
    // to pass B below so the *same* emitter serves the full build and the incremental patch
    // (one source of truth, no drift between paths).
    let mut symbol_range_per_file: Vec<(u32, u32)> = vec![(0, 0); claimed_per_file.len()];
    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        // The FILE NODE's unit, not the raw facts' — phase 2 already applied the manifest
        // unit overrides there (SwiftPM `path:` targets); reading facts here would leave
        // the resolution tables blind to exactly the files the override exists for.
        file_unit[i] = files[i].unit.clone();
        let start = symbols.len() as u32;

        for decl in &claimed.facts.declarations {
            let symbol_id = SymbolId(symbols.len() as u32);
            match &decl.member_of {
                None => {
                    symbol_by_name_per_file[i].insert(decl.name.clone(), symbol_id);
                    if let Some(unit) = &file_unit[i] {
                        symbol_by_name_per_unit
                            .entry(unit.clone())
                            .or_default()
                            .insert(decl.name.clone(), symbol_id);
                    }
                }
                Some(owner) => {
                    member_by_name
                        .entry(decl.name.clone())
                        .or_default()
                        .push(symbol_id);
                    insert_qualified(
                        &mut symbol_by_qualified_per_file[i],
                        &mut qualified_twins_per_file[i],
                        format!("{owner}.{}", decl.name),
                        symbol_id,
                    );
                }
            }
            symbols.push(SymbolNode {
                file: FileId(i as u32),
                name: decl.name.clone(),
                kind: decl.kind.clone(),
                span: decl.span,
                exported: decl.exported,
                visibility: decl.visibility,
                member_of: decl.member_of.clone(),
                signature_span: decl.signature_span,
                implicitly_invoked: decl.implicitly_invoked,
                nested_scope: decl.nested_scope,
                visibility_inherited: decl.visibility_inherited,
            });
        }
        // CJS default alias (FileFacts::default_export_alias): `module.exports = local` —
        // a consumer's whole-module `default` binding resolves to the local, vacant-only
        // exactly like re-export aliases, and persisted in patch_meta so the incremental
        // path reconstructs the same table.
        if let Some(local) = &claimed.facts.default_export_alias {
            if let Some(&sym) = symbol_by_name_per_file[i].get(local) {
                if !symbol_by_name_per_file[i].contains_key("default") {
                    symbol_by_name_per_file[i].insert(SmolStr::new("default"), sym);
                    patch_meta[i].reexport_aliases.push(AliasEntry {
                        name: SmolStr::new("default"),
                        symbol: sym,
                    });
                }
            }
        }
        symbol_range_per_file[i] = (start, symbols.len() as u32);
    }

    // Pass B — per-file declaration emissions, via the shared emitter (also the patch's).
    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        let out = emit_file_declarations(
            i,
            &claimed.facts,
            adapters[claimed.adapter_index].descriptor().id.as_str(),
            symbol_range_per_file[i].0,
            &symbols,
            &symbol_by_name_per_file[i],
            &symbol_by_qualified_per_file[i],
            &library_root_files,
            &role_root_files,
            files[i]
                .language
                .as_deref()
                .and_then(|l| ladders.get(l))
                .map(|l| l.as_slice()),
        );
        edges.extend(out.edges);
        function_metrics.extend(out.metrics);
    }

    // Phase 3a-bis — re-export aliasing (`export {a} from './b'`, `export type {a} from
    // './b'`): a barrel's re-exported bindings become resolvable as *its own* exports too, not
    // merely usable inside it ("Barrel files… resolved through, transparently").
    // Must run for every file before phase 3b resolves any file's import bindings — the same
    // forward-reference reasoning as the 3a/3b split, one level deeper.
    //
    // Resolved to a **fixpoint**: rounds over every unresolved re-export
    // binding in (file, import, binding) order until a round makes no progress. The result is
    // the least fixpoint — order-independent, so barrels chaining through other barrels
    // resolve regardless of discovery order, and
    // a re-export cycle simply never resolves (no progress ⇒ termination). Collision rule,
    // deliberate: a name already present in a file's table — its own declaration, or an
    // earlier-in-order alias — wins over a later alias (`or_insert` semantics — a re-export
    // must never stomp a same-named own declaration).
    struct PendingReexport {
        source_file: usize,
        target: FileId,
        exported_name: SmolStr,
        local: SmolStr,
        span: crate::adapter::Span,
        adapter_index: usize,
    }
    let mut pending: Vec<PendingReexport> = Vec::new();
    let mut glob_reexports: Vec<(usize, FileId, crate::adapter::Span, usize)> = Vec::new();
    for (i, slot) in claimed_per_file.iter().enumerate() {
        let Some(claimed) = slot else { continue };
        let adapter = &adapters[claimed.adapter_index];
        for imp in claimed.facts.imports.iter().filter(|imp| imp.reexported) {
            let spec = ImportSpec {
                specifier: imp.specifier.clone(),
                from: files[i].path.clone(),
            };
            // A re-export resolves through to a file target whether the specifier was
            // relative (`./b`) or a workspace-member name (`@org/ui`) — the aliasing works
            // off the concrete target file either way. (The member's ImportsDependency side
            // is phase 3b's job when it re-resolves this same import.)
            let target = match adapter.resolve(&spec, &ctx) {
                Resolution::File(path, _) => path,
                Resolution::WorkspaceMember { target, .. } => target,
                _ => continue,
            };
            let Some(&target) = file_index.get(&target) else {
                continue;
            };
            if imp.bindings.is_empty() && imp.opaque_namespace_use {
                // A re-exported GLOB (`pub use x::*`, `export * from './x'`): no binding
                // names exist up front — the names are whatever the target *exports*, so
                // the aliasing enumerates the target's table inside the fixpoint instead.
                if FileId(i as u32) != target {
                    glob_reexports.push((i, target, imp.span, claimed.adapter_index));
                }
                continue;
            }
            for binding in &imp.bindings {
                pending.push(PendingReexport {
                    source_file: i,
                    target,
                    exported_name: binding
                        .imported
                        .clone()
                        .unwrap_or_else(|| SmolStr::new("default")),
                    local: binding.local.clone(),
                    span: imp.span,
                    adapter_index: claimed.adapter_index,
                });
            }
        }
    }
    let mut resolved_reexport: Vec<bool> = vec![false; pending.len()];
    loop {
        let mut progress = false;
        for (b, reexport) in pending.iter().enumerate() {
            if resolved_reexport[b] {
                continue;
            }
            let Some(&original_symbol) =
                symbol_by_name_per_file[reexport.target.0 as usize].get(&reexport.exported_name)
            else {
                continue; // maybe next round, once the target's own aliases resolve
            };
            resolved_reexport[b] = true;
            progress = true;
            let i = reexport.source_file;
            if let std::collections::hash_map::Entry::Vacant(slot) =
                symbol_by_name_per_file[i].entry(reexport.local.clone())
            {
                slot.insert(original_symbol);
                patch_meta[i].reexport_aliases.push(AliasEntry {
                    name: reexport.local.clone(),
                    symbol: original_symbol,
                });
                // The barrel itself is a manifest-declared production root, so everything it
                // re-exports is part of the package's public API too — same
                // promotion phase 3a already applies to the barrel's *own* declarations,
                // extended through re-export indirection.
                if let Some(&confidence) = library_root_files.get(&FileId(i as u32)) {
                    edges.push(Edge {
                        owner: FileId(reexport.source_file as u32),
                        kind: EdgeKind::Root {
                            kind: crate::vocab::RootKind::Production,
                            target: NodeRef::Symbol(original_symbol),
                        },
                        confidence,
                        source: Provenance::Adapter(
                            adapters[reexport.adapter_index].descriptor().id.clone(),
                        ),
                        span: Some(reexport.span),
                    });
                }
            }
        }
        // Glob re-exports alias the target's *exported* surface wholesale, under the same
        // or_insert collision rule — first alias of a name wins, later alternates (two
        // `#[path]`-alternated mods glob-re-exported through one barrel) stay reachable
        // through the glob's own Wildcard edge instead. Re-enumerated every round so a
        // chained barrel's freshly-resolved aliases propagate (same least-fixpoint reasoning
        // as the binding loop above).
        for &(source, target, span, adapter_index) in &glob_reexports {
            let target_exports: Vec<(SmolStr, SymbolId)> = symbol_by_name_per_file
                [target.0 as usize]
                .iter()
                .filter(|(_, &sym)| symbols[sym.0 as usize].exported)
                .map(|(name, &sym)| (name.clone(), sym))
                .collect();
            for (name, original_symbol) in target_exports {
                if let std::collections::hash_map::Entry::Vacant(slot) =
                    symbol_by_name_per_file[source].entry(name.clone())
                {
                    slot.insert(original_symbol);
                    progress = true;
                    patch_meta[source].reexport_aliases.push(AliasEntry {
                        name,
                        symbol: original_symbol,
                    });
                    if let Some(&confidence) = library_root_files.get(&FileId(source as u32)) {
                        edges.push(Edge {
                            owner: FileId(source as u32),
                            kind: EdgeKind::Root {
                                kind: crate::vocab::RootKind::Production,
                                target: NodeRef::Symbol(original_symbol),
                            },
                            confidence,
                            source: Provenance::Adapter(
                                adapters[adapter_index].descriptor().id.clone(),
                            ),
                            span: Some(span),
                        });
                    }
                }
            }
        }
        if !progress {
            break;
        }
    }

    // Every file's declared unit name (the qualifier default) as a plain slice —
    // resolve_file consumes this instead of reaching into other files' facts, which is what
    // lets the incremental patch feed it from the snapshot.
    let unit_name_by_file: Vec<Option<SmolStr>> = claimed_per_file
        .iter()
        .map(|s| s.as_ref().and_then(|c| c.facts.unit_name.clone()))
        .collect();

    // Phase 3b — imports, import-bindings, references, and diagnostics. Every file's symbol
    // table is complete now (phase 3a), so cross-file lookups are safe regardless of
    // discovery order — which is also what makes this phase embarrassingly parallel
    // ("per-import resolution... resolved concurrently"): every table it reads
    // is immutable by now, and each file's contributions collect into a private
    // `ResolvedFile` merged below in FileId order (parallel compute, deterministic
    // reduce). `DependencyId` assignment stays in the sequential merge — ids are
    // first-appearance-in-file-order, exactly as the sequential loop assigned them.
    let mut dependencies = Vec::new();
    let mut dep_index: HashMap<SmolStr, DependencyId> = HashMap::default();
    let mut suppressions: Vec<(FileId, crate::adapter::RawSuppression)> = Vec::new();

    // Member-type facts, indexed per file for chained-pointer resolution.
    let member_types_per_file: Vec<MemberTypeIndex> = claimed_per_file
        .iter()
        .map(|slot| {
            slot.as_ref()
                .map(|c| index_member_types(&c.facts.member_types))
                .unwrap_or_default()
        })
        .collect();

    let executable_by_name = executable_name_index(&packages, &file_index);
    let tables = ResolveTables {
        files: &files,
        file_index: &file_index,
        symbols: &symbols,
        symbol_by_name_per_file: &symbol_by_name_per_file,
        symbol_by_qualified_per_file: &symbol_by_qualified_per_file,
        qualified_twins_per_file: &qualified_twins_per_file,
        member_types_per_file: &member_types_per_file,
        symbol_by_name_per_unit: &symbol_by_name_per_unit,
        member_by_name: &member_by_name,
        file_unit: &file_unit,
        unit_name_by_file: &unit_name_by_file,
        ladders: &ladders,
        executable_by_name: &executable_by_name,
        ctx: &ctx,
    };
    let resolved_files: Vec<Option<ResolvedFile>> = claimed_per_file
        .par_iter()
        .enumerate()
        .map(|(i, slot)| {
            let claimed = slot.as_ref()?;
            Some(resolve_file(
                i,
                &claimed.facts,
                &*adapters[claimed.adapter_index],
                &tables,
            ))
        })
        .collect();

    for resolved in resolved_files.into_iter().flatten() {
        edges.extend(resolved.edges);
        for (name, confidence, span, from, source) in resolved.dep_imports {
            let to = *dep_index.entry(name.clone()).or_insert_with(|| {
                let id = DependencyId(dependencies.len() as u32);
                dependencies.push(DependencyNode { name: name.clone() });
                id
            });
            edges.push(Edge {
                owner: from,
                kind: EdgeKind::ImportsDependency { from, to },
                confidence,
                source,
                span: Some(span),
            });
        }
        diagnostics.extend(resolved.diagnostics);
        suppressions.extend(resolved.suppressions);
    }

    // Plugin graph-mutation hooks, factored into `run_plugin_round` so the
    // incremental patch re-derives contributions through the identical code
    // path. Runs right here — Pass A's symbol tables and every file's references (phase 3b,
    // just merged above) are both stable, and nothing downstream (the canonical sort,
    // `ProjectGraph` construction) has run yet, so a plugin's target-by-name lookups see the
    // real, final graph and its contributions fold into the one sort below rather than
    // needing a second pass.
    let plugin_round = run_plugin_round(
        &files,
        &symbols,
        &file_index,
        &symbol_by_name_per_file,
        &symbol_by_qualified_per_file,
        &packages,
        &edges,
        &discovered,
        sorted_plugins,
    );
    edges.extend(plugin_round.edges);
    let externally_consumed = plugin_round.externally_consumed;
    let plugin_implicitly_invoked = plugin_round.implicitly_invoked;
    let plugin_diagnostics = plugin_round.diagnostics;
    // The round just ran, so its counts are the current audit record — written
    // even when empty (no plugins → an empty record replaces any stale one).
    if let Some(cache) = cache {
        cache.record_plugin_contributions(&plugin_round.contributions);
    }
    let plugin_contributions = plugin_round.contributions;

    // Canonical order: edge and diagnostic order is *data*, not construction
    // history. Two semantically identical graphs must be identical vectors — the property the
    // patched ≡ full-rebuild gate compares, and the property that keeps tie-breaks (e.g.
    // cyclic's strongest-edge-per-pair evidence pick) independent of which assembly path or
    // parallel schedule produced the graph. One total comparator, derived field order.
    edges.sort_unstable();
    diagnostics.sort_unstable();
    // Same canonical-order rule for the remaining order-bearing vectors:
    // stable sorts, so same-key entries keep facts order — identical on both build paths.
    function_metrics.sort_by_key(|(id, _)| *id);
    suppressions.sort_by_key(|(f, _)| *f);

    // Per-file patch metadata — surface signatures and unit names from phase
    // 1's facts; the re-export aliases were recorded by the 3a-bis fixpoint above.
    for (i, slot) in claimed_per_file.iter().enumerate() {
        if let Some(claimed) = slot {
            patch_meta[i].surface_sig = Some(claimed.surface_sig);
            patch_meta[i].unit_name = claimed.facts.unit_name.clone();
            patch_meta[i].member_types = claimed.facts.member_types.clone();
        }
    }

    let graph = ProjectGraph {
        files,
        symbols,
        dependencies,
        declared_dependencies,
        script_invoked_dependencies,
        packages,
        edges,
        suppressions,
        visibility_ladders: ladders.into_iter().collect(),
        cycle_policies: cycle_policies.into_iter().collect(),
        function_metrics,
        patch_meta,
        externally_consumed,
        plugin_implicitly_invoked,
        file_index,
    };
    // The snapshot is NOT written here (cache persist happens off the critical
    // path) — the freshly assembled graph hands back the key, and the engine defers the
    // serialize + write to a background thread that overlaps with analysis and rendering.
    // Written even with graph-mutating plugins registered: `graph_key` folds
    // in every such plugin's identity (see the read-side comment above), so a snapshot written
    // here can only ever be served back to a run with the identical plugin set over the
    // identical inputs — a later plugin-less run can never silently inherit contributions.
    // Persisting unconditionally
    // is also what makes the read-side snapshot fast path actually fire on a
    // plugin-bearing project's *second* run, not just prove itself safe in the abstract.
    let pending_snapshot = cache
        .and_then(|c| c.graph_writer(graph_key, current_plugin_digest))
        .map(PendingSnapshot);
    tick("resolve+link", &mut phase_start);
    let (plugin_findings, finding_diagnostics) = run_finding_round(&graph, &discovered, plugins);
    Ok(AssembledGraph {
        graph,
        discovery_diagnostics,
        extraction_diagnostics: diagnostics,
        plugin_diagnostics,
        plugin_findings,
        finding_diagnostics,
        pending_snapshot,
        timings,
        plugin_contributions: Some(plugin_contributions),
    })
}

/// A deferred graph-cache write, detached from [`crate::cache::GraphSnapshotWriter`]'s own
/// type so `AssembledGraph`'s public field doesn't leak a `cache`-module implementation type
/// across the `graph`/`cache` module boundary. Every caller persists through the same
/// `persist_now` — [`assemble_with_cache`]'s inline write and `Engine`'s backgrounded one
/// alike — so there is exactly one code path that turns a pending snapshot into bytes on disk.
pub struct PendingSnapshot(crate::cache::GraphSnapshotWriter);

impl PendingSnapshot {
    pub fn persist_now(
        &self,
        graph: &ProjectGraph,
        diagnostics: &[Diagnostic],
        plugin_diagnostics: &[Diagnostic],
    ) {
        self.0.write(graph, diagnostics, plugin_diagnostics);
    }
}

/// [`assemble_from_source`]'s result: the graph, assembly-time diagnostics (exactly what a
/// snapshot stores and a warm hit replays), and — on a snapshot miss with a writable cache —
/// the deferred writer the engine schedules off the critical path.
pub struct AssembledGraph {
    pub graph: ProjectGraph,
    /// Always the fresh walk's — never stored, never replayed.
    pub discovery_diagnostics: Vec<Diagnostic>,
    /// Extraction + manifest diagnostics — what the snapshot stores and warm paths replay
    /// (their producers were skipped). Canonically sorted, like the edges.
    pub extraction_diagnostics: Vec<Diagnostic>,
    /// The plugin round's diagnostics (content-budget cutoffs), stored in the snapshot's own
    /// partition (the patch discards and re-derives them, so they can't share a
    /// vector with extraction diagnostics that persist). Empty on the snapshot fast path —
    /// there `extraction_diagnostics` already carries the merged replay.
    pub plugin_diagnostics: Vec<Diagnostic>,
    /// The finding round's output — namespaced third-party verdicts, computed fresh
    /// on EVERY path (cold, patch, warm hit; findings are output, never persisted). The engine
    /// maps these to `Finding`s under the advisory-channel rules.
    pub plugin_findings: Vec<crate::plugin::ProtoFinding>,
    /// The finding round's own diagnostics (undeclared rules, noise-cap truncation) — like
    /// `discovery_diagnostics`, always the fresh run's, never stored or replayed.
    pub finding_diagnostics: Vec<Diagnostic>,
    pub pending_snapshot: Option<PendingSnapshot>,
    /// Assembly sub-phase wall times `(phase, µs)` — merged into `RunResult::timings` so
    /// `--verbose` shows where assembly goes (discovery+hash, snapshot load, extract incl.
    /// facts-cache fetches, resolve+link). Empty on the snapshot fast path except its two
    /// entries.
    pub timings: Vec<(&'static str, u64)>,
    /// This call's own plugin-round audit record — `Some` on the two paths that actually ran
    /// `run_plugin_round` (full build, patch), `None` on the snapshot-hit fast path (nothing
    /// ran; the cache's own sidecar, from whichever prior run last executed the round, is still
    /// accurate — the graph it describes is byte-identical to this one). The engine falls back
    /// to that sidecar exactly when this is `None`, so a fresh `--no-cache` run — which has no
    /// sidecar to fall back to, but always takes the full-build path — still reports real data.
    pub plugin_contributions: Option<Vec<crate::plugin::PluginContribution>>,
}
