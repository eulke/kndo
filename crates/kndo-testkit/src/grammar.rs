//! What the grammar OFFERS, and what the adapter does with it.
//!
//! An adapter reads a tree the grammar shapes. Where the two disagree, the
//! disagreement is silent: a name the grammar puts somewhere the extractor
//! never looks enters no stream, and the run reports one finding fewer with
//! nothing anywhere saying so. Six of the nine-adapter audit's findings were
//! exactly that, and none of them had a gate.
//!
//! A [`GrammarLedger`] is the second reader, in the shape
//! [`super::transcript::ToolTranscript`] gives a manifest: the grammar's own
//! `node-types.json` says where names go, the ledger says what the adapter
//! does at each of those places, and a run over the adapter's fixtures grades
//! the answer. What the grammar states and the ledger omits is a silence; what
//! the ledger claims and the run refutes is a lie.
//!
//! # What counts as a place a name goes
//!
//! A NAME POSITION is a field whose DECLARED types include one of the
//! grammar's name kinds — the grammar itself saying "a name goes here", not
//! "an expression, which could be a name". Expression slots are the reference
//! walk's, unconditionally and without a row: everything that is not
//! subtracted is a use. A dedicated name slot is the opposite — it is where
//! the adapter must have an opinion, because a name there is a declaration, a
//! binding, or a use, and only the adapter knows which.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use kndo_contract::evidence::{FileEvidence, ImportShape};
use kndo_contract::vocab::Span;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use tree_sitter::Node;

/// One place the grammar puts a name: a kind's field whose declared types
/// include a name kind. Spelled `kind.field` — neither half can hold a `.`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub kind: SmolStr,
    pub field: SmolStr,
}

impl Position {
    pub fn new(kind: impl Into<SmolStr>, field: impl Into<SmolStr>) -> Position {
        Position {
            kind: kind.into(),
            field: field.into(),
        }
    }

    /// `kind.field` back into its halves; `None` for anything else.
    pub fn parse(s: &str) -> Option<Position> {
        let (kind, field) = s.split_once('.')?;
        (!kind.is_empty() && !field.is_empty() && !field.contains('.'))
            .then(|| Position::new(kind, field))
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.kind, self.field)
    }
}

/// What the adapter does with the name that stands in one position. Exhaustive
/// by construction: a name is read, or subtracted, or used — there is no
/// fourth thing to do with it, and no way to do two.
///
/// Only `Visited` explains itself. The run confirms it positively — evidence
/// came out of the position and carries the name — so nothing is left to
/// argue. The other two are absences of evidence, and an absence is what every
/// grammar silence the audit found looked like from the outside, so each
/// carries the argument for why this one is right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coverage {
    /// The extractor READS this position: what stands here becomes a
    /// declaration, an import, a marker or a relation.
    Visited,
    /// The name here BINDS, and the extractor states nothing about it — it is
    /// subtracted from the reference walk and enters no stream. A parameter, a
    /// keyword argument, a label. `because` says what it binds and why the
    /// engine does not model it.
    Seat { because: String },
    /// Nothing reads it and nothing subtracts it: the name flows to the
    /// reference walk as a use, and `because` says why that is right.
    ///
    /// `Ignored` is the one verdict a position can reach with nobody having
    /// written a line of adapter code for it — `Visited` and `Seat` both take
    /// deliberate work — which is why it is the one that must be argued for in
    /// writing. `@name` reads the argument from the ledger's `[reasons]`,
    /// which is where a reason goes the moment a second position shares it.
    Ignored { because: String },
}

impl Coverage {
    fn word(&self) -> &'static str {
        match self {
            Coverage::Visited => "visited",
            Coverage::Seat { .. } => "seat",
            Coverage::Ignored { .. } => "ignored",
        }
    }
}

/// Every place one grammar puts a name, and what the adapter does there.
#[derive(Debug, Clone)]
pub struct GrammarLedger {
    /// The grammar this ledger was written against, as the crate spells it.
    /// Prose: the inventory is recomputed from `NODE_TYPES` on every run, so
    /// this documents rather than pins.
    pub grammar: SmolStr,
    /// The kinds that spell a name in this grammar. Authored, because a
    /// grammar may spell one anything (`simple_identifier`, `tag_name`), and
    /// checked from both sides: every entry must exist in the grammar, and
    /// every kind a reference was actually read from must be listed.
    pub names: BTreeSet<SmolStr>,
    /// Reasons more than one position gives, written once and named. A row
    /// says `@key`; the second verbatim copy of a reason is what promotes it
    /// here.
    pub reasons: BTreeMap<SmolStr, String>,
    pub positions: BTreeMap<Position, Coverage>,
}

/// A disagreement between the grammar, the ledger and the run.
#[derive(Debug, Clone)]
pub enum Silence {
    /// `names` lists a kind the grammar does not have.
    NotAKind(SmolStr),
    /// A reference was read from a kind the ledger does not call a name.
    UnnamedName(SmolStr),
    /// The grammar offers this position and the ledger says nothing.
    Unlisted(Position),
    /// The ledger names a position the grammar no longer offers.
    Vanished(Position),
    /// `Ignored` with nothing in `because`.
    Unexplained(Position),
    /// `Ignored` naming a reason `[reasons]` does not hold.
    NoSuchReason(Position, SmolStr),
    /// A reason no position names, or one only a single position names — the
    /// second copy is what promotes a reason, and the first is a row.
    UnusedReason(SmolStr, usize),
    /// A reason two positions state in full instead of naming once.
    UncountedCopy(String),
    /// Declared `Visited`, witnessed, and no occurrence ever became evidence.
    NoEvidence(Position, usize),
    /// Declared `Seat`, and an occurrence entered the reference stream anyway.
    NotBound(Position, usize),
    /// Declared `Ignored`, witnessed, and no occurrence ever became a use.
    NotAUse(Position, usize),
}

impl std::fmt::Display for Silence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Silence::NotAKind(k) => {
                write!(f, "`names` lists `{k}`, which this grammar has no kind for")
            }
            Silence::UnnamedName(k) => write!(
                f,
                "a reference was read from `{k}`, which `names` does not list — the inventory \
                 was computed without it, so every position holding one is missing"
            ),
            Silence::Unlisted(p) => write!(
                f,
                "{p}: the grammar puts a name here and the ledger says nothing about it"
            ),
            Silence::Vanished(p) => write!(f, "{p}: no such position in this grammar"),
            Silence::Unexplained(p) => write!(f, "{p}: ignored with no reason given"),
            Silence::NoSuchReason(p, r) => {
                write!(
                    f,
                    "{p}: names the reason `{r}`, which `[reasons]` does not hold"
                )
            }
            Silence::UnusedReason(r, 0) => {
                write!(f, "`[reasons]` holds `{r}`, which no position names")
            }
            Silence::UnusedReason(r, _) => write!(
                f,
                "`[reasons]` holds `{r}`, which one position names — a reason belongs in its \
                 row until a second position shares it"
            ),
            Silence::UncountedCopy(text) => write!(
                f,
                "two positions spell the same reason instead of naming one: {text:?}"
            ),
            Silence::NoEvidence(p, n) => write!(
                f,
                "{p}: declared visited, and none of its {n} occurrences produced a declaration, \
                 an import, a marker or a relation"
            ),
            Silence::NotBound(p, n) => write!(
                f,
                "{p}: declared a seat, and {n} of its occurrences entered the reference stream \
                 as uses"
            ),
            Silence::NotAUse(p, n) => write!(
                f,
                "{p}: declared ignored, and none of its {n} occurrences reached the reference \
                 walk — the name is being dropped, not used"
            ),
        }
    }
}

// ------------------------------------------------------------------- on disk

#[derive(Serialize, Deserialize)]
struct OnDisk {
    grammar: SmolStr,
    names: BTreeSet<SmolStr>,
    #[serde(default)]
    reasons: BTreeMap<SmolStr, String>,
    #[serde(default)]
    visited: BTreeSet<String>,
    #[serde(default)]
    seat: BTreeMap<String, String>,
    #[serde(default)]
    ignored: BTreeMap<String, String>,
}

impl GrammarLedger {
    /// Reads `grammar.toml`. Panics with the file's name on anything malformed
    /// — a ledger is authored, and a typo in one is a bug in the commit that
    /// wrote it, never a runtime condition to carry.
    pub fn read(path: &Path) -> GrammarLedger {
        let text =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let disk: OnDisk =
            toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut positions = BTreeMap::new();
        let mut put = |s: &str, c: Coverage| {
            let p = Position::parse(s)
                .unwrap_or_else(|| panic!("{}: `{s}` is not a `kind.field`", path.display()));
            if let Some(prev) = positions.insert(p.clone(), c) {
                panic!(
                    "{}: {p} is listed twice, first as {}",
                    path.display(),
                    prev.word()
                );
            }
        };
        for s in &disk.visited {
            put(s, Coverage::Visited);
        }
        for (s, because) in &disk.seat {
            put(
                s,
                Coverage::Seat {
                    because: because.clone(),
                },
            );
        }
        for (s, because) in &disk.ignored {
            put(
                s,
                Coverage::Ignored {
                    because: because.clone(),
                },
            );
        }
        GrammarLedger {
            grammar: disk.grammar,
            names: disk.names,
            reasons: disk.reasons,
            positions,
        }
    }

    /// Writes the ledger back in the shape [`GrammarLedger::read`] takes.
    pub fn write(&self, path: &Path) {
        let disk = OnDisk {
            grammar: self.grammar.clone(),
            names: self.names.clone(),
            reasons: self.reasons.clone(),
            visited: self.by(|c| matches!(c, Coverage::Visited)),
            seat: self.reasoned(|c| match c {
                Coverage::Seat { because } => Some(because),
                _ => None,
            }),
            ignored: self.reasoned(|c| match c {
                Coverage::Ignored { because } => Some(because),
                _ => None,
            }),
        };
        let body = toml::to_string_pretty(&disk).expect("a ledger serializes");
        std::fs::write(path, body).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }

    fn by(&self, keep: impl Fn(&Coverage) -> bool) -> BTreeSet<String> {
        self.positions
            .iter()
            .filter(|(_, c)| keep(c))
            .map(|(p, _)| p.to_string())
            .collect()
    }

    fn reasoned<'c>(
        &'c self,
        pick: impl Fn(&'c Coverage) -> Option<&'c String>,
    ) -> BTreeMap<String, String> {
        self.positions
            .iter()
            .filter_map(|(p, c)| pick(c).map(|b| (p.to_string(), b.clone())))
            .collect()
    }

    /// Every name position one grammar OFFERS: a field whose declared types
    /// include one of `names`. `node_types` is the grammar crate's
    /// `NODE_TYPES`; several may be folded in (TypeScript ships `typescript`
    /// and `tsx`, css ships css and scss) and the offer is their union.
    pub fn offered(node_types: &[&str], names: &BTreeSet<SmolStr>) -> BTreeSet<Position> {
        let mut out = BTreeSet::new();
        for source in node_types {
            for node in Self::named_nodes(source) {
                let Some(fields) = node.get("fields").and_then(|f| f.as_object()) else {
                    continue;
                };
                for (field, spec) in fields {
                    let holds_name = spec
                        .get("types")
                        .and_then(|t| t.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|t| t.get("type").and_then(|t| t.as_str()))
                        .any(|t| names.contains(t));
                    if holds_name {
                        out.insert(Position::new(
                            node["type"].as_str().expect("a node has a type"),
                            field.as_str(),
                        ));
                    }
                }
            }
        }
        out
    }

    /// Every named kind the grammar declares.
    pub fn kinds(node_types: &[&str]) -> BTreeSet<SmolStr> {
        node_types
            .iter()
            .flat_map(|s| Self::named_nodes(s))
            .filter_map(|n| n.get("type").and_then(|t| t.as_str()).map(SmolStr::new))
            .collect()
    }

    fn named_nodes(source: &str) -> Vec<serde_json::Value> {
        let all: Vec<serde_json::Value> =
            serde_json::from_str(source).expect("node-types.json parses");
        all.into_iter()
            .filter(|n| n.get("named").and_then(|b| b.as_bool()).unwrap_or(false))
            .collect()
    }

    /// The ledger against the grammar and the run: every position either side
    /// knows is listed, nothing listed that neither knows, every name kind
    /// real, every absence argued for, and every claim the run can reach borne
    /// out.
    ///
    /// The two sides answer different halves. The grammar states the positions
    /// it DECLARES a name kind for; where it declares a supertype instead
    /// (`_type`, `_expression`), only the run can say a name stood there. A
    /// position either one names is a position the ledger owes a verdict.
    pub fn check(&self, node_types: &[&str], witness: &Witness) -> Vec<Silence> {
        let kinds = Self::kinds(node_types);
        let mut offered = Self::offered(node_types, &self.names);
        offered.extend(witness.positions().cloned());
        let mut out: Vec<Silence> = self
            .names
            .iter()
            .filter(|n| !kinds.contains(*n))
            .map(|n| Silence::NotAKind(n.clone()))
            .collect();
        out.extend(
            offered
                .iter()
                .filter(|p| !self.positions.contains_key(p))
                .map(|p| Silence::Unlisted(p.clone())),
        );
        out.extend(
            self.positions
                .keys()
                .filter(|p| !offered.contains(*p))
                .map(|p| Silence::Vanished(p.clone())),
        );
        let mut named: BTreeMap<&str, usize> = BTreeMap::new();
        let mut spelled: BTreeMap<&str, usize> = BTreeMap::new();
        for (position, coverage) in &self.positions {
            let (Coverage::Ignored { because } | Coverage::Seat { because }) = coverage else {
                continue;
            };
            match because.strip_prefix('@') {
                _ if because.trim().is_empty() => out.push(Silence::Unexplained(position.clone())),
                Some(key) => {
                    *named.entry(key).or_default() += 1;
                    if !self.reasons.contains_key(key) {
                        out.push(Silence::NoSuchReason(position.clone(), SmolStr::new(key)));
                    }
                }
                None => *spelled.entry(because.as_str()).or_default() += 1,
            }
        }
        out.extend(
            self.reasons
                .keys()
                .map(|k| (k, named.get(k.as_str()).copied().unwrap_or(0)))
                .filter(|(_, n)| *n < 2)
                .map(|(k, n)| Silence::UnusedReason(k.clone(), n)),
        );
        out.extend(
            spelled
                .iter()
                .filter(|(_, n)| **n > 1)
                .map(|(text, _)| Silence::UncountedCopy((*text).to_string())),
        );
        out.extend(witness.verdicts(self));
        out
    }
}

// ------------------------------------------------------------------ the run

/// What one position's occurrences actually did, summed over the files read.
#[derive(Debug, Default, Clone, Copy)]
pub struct Seen {
    pub occurrences: usize,
    /// Occurrences a reference covers and spells — the name reached the walk.
    /// Covers rather than equals: an adapter may report a qualified use at the
    /// whole path's span, and the segment standing in the position is the same
    /// name either way.
    pub as_use: usize,
    /// Occurrences some declaration, import, marker or relation claims by
    /// name — the extractor read the position itself. Independent of
    /// [`Seen::as_use`]: an annotation's name is a marker and a use of the
    /// annotation type at once.
    pub as_evidence: usize,
}

/// The run, accumulated: every position's occurrences across every file read,
/// and the kinds references were actually read from.
#[derive(Debug, Default)]
pub struct Witness {
    seen: BTreeMap<Position, Seen>,
    reference_kinds: BTreeSet<SmolStr>,
}

impl Witness {
    /// Reads one file: the parsed tree beside the evidence the adapter emitted
    /// from the same bytes. Every node standing in a name position is asked
    /// one question — what became of it. The positions come from the tree and
    /// the ledger's `names`, never from the ledger's own list, so an empty
    /// ledger still witnesses what a full one would.
    pub fn read(
        &mut self,
        root: Node<'_>,
        source: &[u8],
        ev: &FileEvidence,
        ledger: &GrammarLedger,
    ) {
        let uses: Vec<(Span, SmolStr)> = ev
            .references
            .iter()
            .map(|r| (r.span, r.name.clone()))
            .collect();
        let claimed = claimed_names(ev);
        each_field(root, &mut |parent, node, field| {
            if !ledger.names.contains(node.kind()) {
                return;
            }
            let position = Position::new(parent.kind(), field);
            let span = Span::new(node.start_byte() as u32, node.end_byte() as u32);
            let text = std::str::from_utf8(&source[node.byte_range()]).unwrap_or_default();
            let seen = self.seen.entry(position).or_default();
            seen.occurrences += 1;
            // Both, independently: an annotation's name is a marker AND a
            // reference to the annotation type, and a position that is read
            // structurally is not thereby kept out of the walk.
            if uses.iter().any(|(s, n)| s.contains(&span) && n == text) {
                seen.as_use += 1;
            }
            if claimed.iter().any(|(s, n)| s.contains(&span) && n == text) {
                seen.as_evidence += 1;
            }
        });
        for r in &ev.references {
            if let Some(n) = named_node_at(root, r.span) {
                self.reference_kinds.insert(SmolStr::new(n.kind()));
            }
        }
    }

    /// How many of the ledger's positions any file actually exercised.
    pub fn witnessed(&self) -> usize {
        self.seen.values().filter(|s| s.occurrences > 0).count()
    }

    /// What each position's occurrences did, for an author deciding a verdict.
    pub fn seen(&self) -> &BTreeMap<Position, Seen> {
        &self.seen
    }

    /// Every position any file exercised.
    pub fn positions(&self) -> impl Iterator<Item = &Position> {
        self.seen
            .iter()
            .filter(|(_, s)| s.occurrences > 0)
            .map(|(p, _)| p)
    }

    /// The ledger's claims against what the run did. Positions no file
    /// exercised are not graded — a grammar offers positions no fixture writes
    /// — which is what [`Witness::witnessed`] is for reporting.
    fn verdicts(&self, ledger: &GrammarLedger) -> Vec<Silence> {
        let mut out: Vec<Silence> = Vec::new();
        out.extend(
            self.reference_kinds
                .iter()
                .filter(|k| !ledger.names.contains(*k))
                .map(|k| Silence::UnnamedName(k.clone())),
        );
        for (position, seen) in &self.seen {
            if seen.occurrences == 0 {
                continue;
            }
            match ledger.positions.get(position) {
                Some(Coverage::Visited) if seen.as_evidence == 0 => {
                    out.push(Silence::NoEvidence(position.clone(), seen.occurrences));
                }
                Some(Coverage::Seat { .. }) if seen.as_use > 0 => {
                    out.push(Silence::NotBound(position.clone(), seen.as_use));
                }
                Some(Coverage::Ignored { .. }) if seen.as_use == 0 => {
                    out.push(Silence::NotAUse(position.clone(), seen.occurrences));
                }
                _ => {}
            }
        }
        out
    }

    /// The verdict the run supports for each position it witnessed — what
    /// `--overwrite` writes into a fresh ledger, leaving every `because` for
    /// the author.
    pub fn observed(&self) -> BTreeMap<Position, Coverage> {
        self.seen
            .iter()
            .filter(|(_, s)| s.occurrences > 0)
            .map(|(p, s)| {
                let coverage = if s.as_evidence > 0 {
                    Coverage::Visited
                } else if s.as_use > 0 {
                    Coverage::Ignored {
                        because: String::new(),
                    }
                } else {
                    Coverage::Seat {
                        because: String::new(),
                    }
                };
                (p.clone(), coverage)
            })
            .collect()
    }
}

/// Every child that stands in a NAMED FIELD of its parent, anywhere under
/// `node` — the grammar's own statement of which slot each child fills.
fn each_field<'t>(node: Node<'t>, out: &mut impl FnMut(Node<'t>, Node<'t>, &'static str)) {
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return;
    }
    loop {
        let child = cursor.node();
        if let Some(field) = cursor.field_name() {
            out(node, child, field);
        }
        each_field(child, out);
        if !cursor.goto_next_sibling() {
            return;
        }
    }
}

/// The smallest named node covering exactly `span`, if any.
fn named_node_at<'t>(root: Node<'t>, span: Span) -> Option<Node<'t>> {
    root.named_descendant_for_byte_range(span.start as usize, span.end as usize)
        .filter(|n| n.start_byte() as u32 == span.start && n.end_byte() as u32 == span.end)
}

/// Every name some non-reference evidence claims, beside the span it claims it
/// in. A name position is `Visited` when one of these covers it and spells it.
fn claimed_names(ev: &FileEvidence) -> Vec<(Span, SmolStr)> {
    let mut out: Vec<(Span, SmolStr)> = Vec::new();
    for d in &ev.declarations {
        out.push((d.span, d.name.clone()));
        if let Some(alias) = &d.exported_as {
            out.push((d.span, alias.clone()));
        }
    }
    for i in &ev.imports {
        for segment in segments(i.target.as_written()) {
            out.push((i.span, segment));
        }
        match &i.shape {
            ImportShape::Bindings(bs) | ImportShape::Reexport(bs) => {
                for b in bs {
                    out.push((i.span, b.imported.clone()));
                    out.push((i.span, b.local.clone()));
                }
            }
            ImportShape::Namespace { local } => out.push((i.span, local.clone())),
            _ => {}
        }
    }
    for m in &ev.markers {
        for segment in segments(&m.path) {
            out.push((m.span, segment));
        }
        for a in &m.args {
            out.push((m.span, a.clone()));
        }
    }
    for r in &ev.relations {
        out.push((r.span, r.to.name.clone()));
    }
    out
}

/// A dotted, slashed or `::`-joined name and each of its parts — a marker
/// written `pytest.fixture` claims `pytest` and `fixture` as well as itself.
fn segments(name: &str) -> Vec<SmolStr> {
    let mut out = vec![SmolStr::new(name)];
    out.extend(
        name.split(['.', '/', ':'])
            .filter(|s| !s.is_empty())
            .map(SmolStr::new),
    );
    out
}
