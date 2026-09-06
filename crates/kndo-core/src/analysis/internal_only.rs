//! Declared wider than it is used: a declaration whose every use sits inside a
//! narrower reach than the one it declares could take the language's keyword
//! for that reach. One claim, read off the language's ladder
//! ([`kndo_contract::extension::Ladder`]): the declaration stands on a rung —
//! its namespace, its unit, exported — and its uses need a narrower one — the
//! declaration that owns it, or its file. The advice names the narrowest step
//! between the two that the declaration's shape can take; a language that
//! spells nothing between them gets no advice, because the advice would name a
//! keyword that does not exist. The engine never knows which language it is
//! judging: the rungs come from the evidence, the pools from the scope forest,
//! the words from the ladder.
//!
//! The bounded rungs (namespace, unit) are disqualified by any use across their
//! POOL — the only files that can legally resolve the name — pooling reachable
//! files. The Exported rung has no pool: an exported name is nameable from
//! anywhere, so its disqualifier is total — a binding import, or a same-named
//! reference in ANY other claimed file, reachable or not, because an
//! unreachable file still compiles against the export it spells (vite's
//! `__tests_dts__` type-tests proved that vice). And it is judged only where
//! the export is nobody's outside: an ecosystem that publishes through entries
//! ([`kndo_contract::extension::PublishedSurface::Entries`]), or a unit that
//! publishes nothing — an executable, a test set, a library its manifest keeps
//! private. Where every export is published, an exported declaration is the
//! outside world's however it is used inside. A whole-file-rooted file is
//! exempt on that rung: an entry's exports are that surface, and a test's are
//! its runner's.
//!
//! `Probable`/`Info`, derived from this analysis's own evidence: the residuals
//! below `Certain` are reflection and dynamic access (out of static scope
//! everywhere in kndo), name-pool collisions, and — for the Exported rung —
//! importers in files no adapter claims. `unused` outranks it: a declaration
//! nothing uses at all is dead, not demotable.

use super::{Analysis, AnalysisContext, RunContext};
use crate::navigate::Pool;
use kndo_contract::evidence::{
    Declaration, EvidenceStream, FileEvidence, ImportShape, Reach, RootTarget, SymbolKind,
};
use kndo_contract::extension::{PublishedSurface, Rung};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::vocab::{Category, Confidence};
use std::collections::{BTreeMap, BTreeSet};

pub struct InternalOnly;

impl Analysis for InternalOnly {
    fn id(&self) -> &'static str {
        "internal-only"
    }

    fn category(&self) -> Category {
        Category::INTERNAL_ONLY
    }

    fn abstains(&self, run: &RunContext<'_>) -> Option<super::AbstentionReason> {
        // Same posture as unused: a rootless graph judges nothing.
        let any_root = run.graph.files.iter().any(|f| f.is_rooted());
        (!run.graph.files.is_empty() && !any_root)
            .then_some(super::AbstentionReason::NoRootsAnywhere)
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph();
        let reachable = |i: usize| cx.run.reach.any(i);

        // Names bound out of each file, and names referenced OUTSIDE each file —
        // one pass; a name in either set is used beyond its declaration site.
        // Two sets: the bounded rungs pool reachable importers (their pool is
        // enumerated); the Exported rung pools EVERY importer, because even an
        // unreachable file compiles against what it imports.
        let mut bound_names: BTreeSet<(u32, &str)> = BTreeSet::new();
        let mut bound_all: BTreeSet<(u32, &str)> = BTreeSet::new();
        let mut per_file_refs: Vec<BTreeSet<&str>> = Vec::with_capacity(g.files.len());
        // The same names, narrowed to those read FROM something — a member
        // access. A file whose adapter does not declare the stream reports
        // none, and `qualifies` below is what keeps that absence from reading
        // as "this file accesses no members".
        let mut per_file_accesses: Vec<BTreeSet<&str>> = Vec::with_capacity(g.files.len());
        // The same accesses with what they were read from — `(receiver, name)`
        // — which is what tells `Owner.create()` from another class's.
        let mut per_file_accesses_on: Vec<BTreeSet<(&str, &str)>> =
            Vec::with_capacity(g.files.len());
        let mut per_file_types: Vec<BTreeSet<&str>> = Vec::with_capacity(g.files.len());
        let mut qualifies: Vec<bool> = Vec::with_capacity(g.files.len());
        for f in &g.files {
            per_file_refs.push(
                f.evidence
                    .references
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect(),
            );
            per_file_accesses.push(
                f.evidence
                    .references
                    .iter()
                    .filter(|r| r.on.is_some())
                    .map(|r| r.name.as_str())
                    .collect(),
            );
            per_file_accesses_on.push(
                f.evidence
                    .references
                    .iter()
                    .filter_map(|r| r.on.as_ref().map(|on| (on.as_str(), r.name.as_str())))
                    .collect(),
            );
            per_file_types.push(
                f.evidence
                    .declarations
                    .iter()
                    .filter(|d| matches!(d.kind, SymbolKind::Type))
                    .map(|d| d.name.as_str())
                    .collect(),
            );
            qualifies.push(f.evidence.declared.contains(EvidenceStream::Qualifiers));
        }
        // How many claimed files spell each name at all — the Exported rung's
        // total-absence check: given a use in its own file, a count of two or
        // more means someone else names it too.
        let mut name_files: BTreeMap<&str, u32> = BTreeMap::new();
        for refs in &per_file_refs {
            for name in refs {
                *name_files.entry(name).or_insert(0) += 1;
            }
        }
        for (i, f) in g.files.iter().enumerate() {
            let from_reachable = reachable(i);
            for (import, targets) in f.evidence.imports.iter().zip(&f.import_targets) {
                for &t in targets {
                    match &import.shape {
                        ImportShape::Bindings(bs) | ImportShape::Reexport(bs) => {
                            for b in bs {
                                bound_all.insert((t, b.imported.as_str()));
                                if from_reachable {
                                    bound_names.insert((t, b.imported.as_str()));
                                }
                            }
                        }
                        // A namespace/glob importer may use anything: treat the
                        // whole target as used from outside.
                        _ => {
                            bound_all.insert((t, ""));
                            if from_reachable {
                                bound_names.insert((t, ""));
                            }
                        }
                    }
                }
            }
        }

        let mut out = Vec::new();
        for (i, f) in g.files.iter().enumerate() {
            if !cx.measured[i] || !reachable(i) {
                continue;
            }
            // The ladder is the one fact this analysis reads about a language;
            // a language that states none gets no advice.
            let Some(caps) = cx.run.capabilities_of(&f.adapter) else {
                continue;
            };
            let ladder = &caps.ladder;
            if ladder.is_empty() {
                continue;
            }
            // An export is nobody's outside where the ecosystem publishes
            // through entries, or where the unit compiling the file publishes
            // nothing at all.
            let export_is_internal = caps.published_surface == PublishedSurface::Entries
                || f.unit
                    .is_some_and(|u| !g.project.units[u as usize].published);
            // The whole file was namespace-imported: anything here may be used.
            // The Exported rung honors even an unreachable such importer.
            let bounded_open = !bound_names.contains(&(i as u32, ""));
            let exported_open = !bound_all.contains(&(i as u32, ""));
            // An entry, test, or tooling file, or a file on its unit's
            // published surface: its exports ARE an outside surface (a
            // manifest's consumers, a runner), so the Exported rung stays
            // silent for the whole file.
            let whole_file_rooted =
                f.published || f.roots().any(|r| matches!(r.target, RootTarget::WholeFile));
            let rooted: BTreeSet<usize> = f
                .roots()
                .filter_map(|r| match &r.target {
                    RootTarget::Declaration(id) => Some(id.index()),
                    _ => None,
                })
                .collect();
            for (id, d) in f.evidence.declarations_with_ids() {
                let d_ix = id.index();
                // The rung the declaration stands on, and the pool its bounded
                // reach names. Silence for a reach with no rung (an adapter's
                // own token), an unbounded pool, and an exported name in an
                // ecosystem that publishes every export.
                let (declared, pool): (Rung, Option<&[u32]>) = match &d.reach {
                    Reach::Namespace { .. }
                    | Reach::Unit { .. }
                    | Reach::Directory { .. }
                    | Reach::Named { .. }
                    | Reach::Heirs { .. } => {
                        if !bounded_open {
                            continue;
                        }
                        // A heirs member of an exported owner, in a unit that
                        // publishes its exports, is published surface: a
                        // subtype outside the tree may name it, as with
                        // `public`.
                        if matches!(d.reach, Reach::Heirs { .. })
                            && !export_is_internal
                            && d.owner.is_some_and(|o| {
                                cx.run.index.effective(g, i, o.index()) == Reach::Exported
                            })
                        {
                            continue;
                        }
                        // The pool is the effective reach's — a bounded member
                        // of a file-private owner pools its file — and the
                        // rung the declared one's, since the word is the
                        // declaration's own.
                        let Pool::Files(files) = cx.run.index.pool_for(g, i, d_ix) else {
                            continue;
                        };
                        let Some(rung) = d.reach.rung() else {
                            continue;
                        };
                        (rung, Some(files))
                    }
                    Reach::Exported => {
                        // Owned members wait for their own demand; the floor is
                        // the file's own top-level surface.
                        if !export_is_internal
                            || !exported_open
                            || whole_file_rooted
                            || d.owner.is_some()
                        {
                            continue;
                        }
                        (Rung::Exported, None)
                    }
                    // Owner and File are the floor, a token names no rung, a
                    // reach that is its owner's has no word of its own, and a
                    // reach this build does not know is the widest one.
                    _ => continue,
                };
                // A rooted declaration is used from outside the graph's sight.
                if rooted.contains(&d_ix) || d.owner.is_some_and(|o| rooted.contains(&o.index())) {
                    continue;
                }
                // A member that sits on a promised surface cannot narrow: an
                // overrider needs it at least this visible, and a member that
                // itself overrides is fixed by what it overrides. Both
                // directions of the same fact, which is why one relation
                // stream answers both.
                if let Some(owner) = d.owner {
                    let owner = f.evidence.declarations[owner.index()].name.as_str();
                    if cx.run.index.is_overridden(owner, d.name.as_str())
                        || cx
                            .run
                            .index
                            .witnessed_type(owner, d.name.as_str())
                            .is_some()
                    {
                        continue;
                    }
                }
                // Used INSIDE its own file at all? If not, `unused` owns it —
                // unless the pool holds a use, which the package extent below
                // reads.
                let own_use = per_file_refs[i].contains(d.name.as_str());
                // A heirs member granted its package too, used from the
                // package and from no subtype: the package's rung. Any member
                // used from its subtypes alone: the heirs' rung — what
                // `protected` is for, whatever package the subtype sits in.
                let mut within_package = false;
                let mut within_heirs = false;
                let used_beyond = match pool {
                    // Any use beyond the file disqualifies: a binding importer,
                    // or a reference in another file of the POOL — the only
                    // files that can legally resolve the name. Same-named
                    // references OUTSIDE the pool cannot be this symbol, so
                    // they neither keep nor disqualify. (A bounded method
                    // reached through a public supertype stays exported by its
                    // own modifiers, so it never sits here.)
                    Some(region) => {
                        // A MEMBER is reached from a stranger's file through an
                        // access (`queue.head`), an import binding, or the
                        // inheritance that puts it in that file's own scopes.
                        // A bare name there is a different declaration — a
                        // local, a parameter, a same-named member of something
                        // else — and reading it as a use is how a package with
                        // a common word in it silences every advisory. A
                        // nested TYPE is exempt: it is named bare, after an
                        // import or from its own package.
                        let owner = d
                            .owner
                            .map(|o| f.evidence.declarations[o.index()].name.as_str());
                        let heirs = match owner {
                            Some(owner) if !matches!(d.kind, SymbolKind::Type) => {
                                Some(cx.run.index.subtypes(owner))
                            }
                            _ => None,
                        };
                        let names_it = |j: usize| -> bool {
                            if !per_file_refs[j].contains(d.name.as_str()) {
                                return false;
                            }
                            let Some(heirs) = &heirs else { return true };
                            // The stream is the referencing file's to declare;
                            // without it, that file's bare names still count.
                            !qualifies[j]
                                || per_file_accesses[j].contains(d.name.as_str())
                                || heirs.iter().any(|t| per_file_types[j].contains(t.as_str()))
                        };
                        let bound = bound_names.contains(&(i as u32, d.name.as_str()))
                            || d.exported_as
                                .as_ref()
                                .is_some_and(|a| bound_names.contains(&(i as u32, a.as_str())));
                        let users: Vec<u32> = region
                            .iter()
                            .copied()
                            .filter(|&j| {
                                j as usize != i && reachable(j as usize) && names_it(j as usize)
                            })
                            .collect();
                        if !own_use && users.is_empty() {
                            continue;
                        }
                        let is_heir = |j: &u32| -> bool {
                            heirs.as_ref().is_some_and(|heirs| {
                                heirs
                                    .iter()
                                    .any(|t| per_file_types[*j as usize].contains(t.as_str()))
                            })
                        };
                        // Positive advice rests on uses this declaration's for
                        // sure: an access read from the owner or one of its
                        // heirs by name. A receiver that is anything else — a
                        // local, another class with a same-named member —
                        // keeps the member alive and says nothing about where
                        // it is used.
                        let names_owner = |j: &u32| -> bool {
                            let Some(owner) = owner else { return false };
                            per_file_accesses_on[*j as usize].iter().any(|(on, n)| {
                                *n == d.name.as_str()
                                    && (*on == owner
                                        || heirs
                                            .as_ref()
                                            .is_some_and(|h| h.iter().any(|t| t == on)))
                            })
                        };
                        within_heirs = !bound && !users.is_empty() && users.iter().all(is_heir);
                        within_package = !bound
                            && !users.is_empty()
                            && !users.iter().any(is_heir)
                            && users.iter().all(names_owner)
                            && matches!(
                                d.reach,
                                Reach::Heirs {
                                    and_namespace: true
                                }
                            )
                            && cx.run.index.namespace_pool(i, 0).is_some_and(|ns| {
                                users.iter().all(|u| ns.binary_search(u).is_ok())
                            });
                        bound || (!users.is_empty() && !within_package && !within_heirs)
                    }
                    // Total absence for the Exported rung: any binding importer
                    // (reachable or not), or the name spelled in ANY other
                    // claimed file.
                    None => {
                        if !own_use {
                            continue;
                        }
                        bound_all.contains(&(i as u32, d.name.as_str()))
                            || d.exported_as
                                .as_ref()
                                .is_some_and(|a| bound_all.contains(&(i as u32, a.as_str())))
                            || name_files.get(d.name.as_str()).copied().unwrap_or(0) >= 2
                    }
                };
                if used_beyond {
                    continue;
                }
                // The rung the uses need, and the step the language spells for
                // it below the declared one — none, and the advice would name
                // a keyword this declaration cannot take.
                let enclosing = enclosing_owner(&f.evidence, d_ix);
                let extent = match enclosing {
                    _ if within_heirs => Rung::Heirs,
                    _ if within_package => Rung::Namespace,
                    Some(_) => Rung::Owner,
                    None => Rung::File,
                };
                // The owner's cap already holds the uses: the word between the
                // cap and the declaration changes nothing anyone can name, so
                // only an extent below the cap is advice.
                if cx
                    .run
                    .index
                    .effective(g, i, d_ix)
                    .rung()
                    .is_some_and(|cap| extent >= cap)
                {
                    continue;
                }
                let Some(step) = ladder.step_down(declared, extent, d.owner.is_some()) else {
                    continue;
                };
                let declared_word = ladder.word(declared);
                let noun = match d.kind {
                    SymbolKind::Function => "function",
                    SymbolKind::Method => "method",
                    SymbolKind::Type => "type",
                    _ => "declaration",
                };
                let within = match enclosing {
                    _ if within_heirs => match &d.owner {
                        Some(o) => format!(
                            "`{}` and its subtypes",
                            f.evidence.declarations[o.index()].name
                        ),
                        None => "its subtypes".to_string(),
                    },
                    _ if within_package => "its own package".to_string(),
                    Some(owner) => format!("`{}`", owner.name),
                    None if declared == Rung::Exported => {
                        "its own file and nothing else in the tree imports or names it".to_string()
                    }
                    None => "its own file".to_string(),
                };
                out.push(Finding::new(
                    Category::INTERNAL_ONLY,
                    Severity::Info,
                    Confidence::Probable,
                    f.evidence.subject_of(&f.path, id),
                    "",
                    format!(
                        "declared `{declared_word}`, but every use is within {within} — `{}` would \
                         suffice for this {noun}",
                        step.word
                    ),
                ));
            }
        }
        out
    }
}

/// The outermost declaration owning `decl`, when every same-file reference to
/// its name sits inside that declaration's span — the shape under which the
/// owner's own keyword (`private`) suffices, since a nested declaration shares
/// its enclosing declaration's private members. A use anywhere else in the
/// file, or no owner at all, needs the file's rung.
fn enclosing_owner(evidence: &FileEvidence, decl: usize) -> Option<&Declaration> {
    let mut top = evidence.declarations[decl].owner?;
    while let Some(above) = evidence.declarations[top.index()].owner {
        top = above;
    }
    let owner = &evidence.declarations[top.index()];
    let name = evidence.declarations[decl].name.as_str();
    evidence
        .references
        .iter()
        .filter(|r| r.name == name)
        .all(|r| owner.span.contains(&r.span))
        .then_some(owner)
}

#[cfg(test)]
mod tests {
    use super::enclosing_owner;
    use kndo_contract::evidence::Reach;
    use kndo_contract::evidence::{EvidenceSink, EvidenceStreams, RefKind, SymbolKind};
    use kndo_contract::vocab::Span;

    #[test]
    fn the_owner_extent_holds_only_while_every_use_sits_inside_the_outermost_owner() {
        let mut sink = EvidenceSink::new(1_000, EvidenceStreams::none());
        let outer = sink.declaration(
            "Outer",
            SymbolKind::Type,
            Span::new(0, 100),
            Reach::Exported,
        );
        let inner = sink.declaration("Inner", SymbolKind::Type, Span::new(10, 60), Reach::Owner);
        sink.member_of(inner, outer);
        let m = sink.declaration(
            "m",
            SymbolKind::Method,
            Span::new(20, 30),
            Reach::Namespace { up: 0 },
        );
        sink.member_of(m, inner);
        // A use from the enclosing type's own body — outside `Inner`, inside `Outer`.
        sink.reference("m", RefKind::Call, Span::new(80, 81));
        let ev = sink.finish();
        assert_eq!(
            enclosing_owner(&ev, m.index()).map(|d| d.name.as_str()),
            Some("Outer"),
            "nested declarations share their outermost owner's private members"
        );

        let mut sink = EvidenceSink::new(1_000, EvidenceStreams::none());
        let outer = sink.declaration(
            "Outer",
            SymbolKind::Type,
            Span::new(0, 100),
            Reach::Exported,
        );
        let m = sink.declaration(
            "m",
            SymbolKind::Method,
            Span::new(20, 30),
            Reach::Namespace { up: 0 },
        );
        sink.member_of(m, outer);
        sink.reference("m", RefKind::Call, Span::new(50, 51));
        // A second top-level declaration in the same file names it too.
        sink.reference("m", RefKind::Call, Span::new(150, 151));
        let ev = sink.finish();
        assert!(
            enclosing_owner(&ev, m.index()).is_none(),
            "a use beyond the owner needs the file's rung"
        );

        let mut sink = EvidenceSink::new(1_000, EvidenceStreams::none());
        let f = sink.declaration(
            "f",
            SymbolKind::Function,
            Span::new(0, 10),
            Reach::Unit { up: 0 },
        );
        sink.reference("f", RefKind::Call, Span::new(5, 6));
        let ev = sink.finish();
        assert!(
            enclosing_owner(&ev, f.index()).is_none(),
            "no owner, no owner extent"
        );
    }
}
