# Working in this repo

The law of this repo is executable: the gates (`kndo-gates` enumerates them) and the
registries (config keys, categories, and CI steps are generated from them). This file is
the judgment layer — the decisions no gate can make for you. When it disagrees with a
gate, the gate wins and this file is stale: fix it in the same PR. The best fate of any
line here is to be retired by a type or a gate that makes it unnecessary.

## Measure first

Any work that claims to improve findings — a new analysis, a plugin, an adapter
capability, a precision fix — starts with the corpus experiment, and the number decides.
A killed idea's deliverable is a `DECISIONS.md` entry with its number, so nobody
rediscovers and rebuilds it. A design without a measurement attached is a guess wearing a
spec's clothes: measure, then design around what you measured.

## Which knob

Two deliberate version knobs exist; shape changes use neither.

- A contract type changed shape → already done: `CONTRACT_FINGERPRINT` moved on its own.
- This adapter now emits different evidence from the same source → bump its
  `semantics_version!`.
- The same evidence now assembles into a different graph → bump
  `GRAPH_SEMANTICS_VERSION`.

About to bump two places for one fact? Wrong knob — stop, and re-derive which of the
three sentences above describes your change.

## Where a fact lives

Walk down and take the first floor that fits:

1. Two crates would otherwise each carry it → `kndo-contract`.
2. Every adapter would write it identically → `kndo-toolkit`; test machinery →
   `kndo-testkit`.
3. A frontend needs it → export it through the facade: a PR to core, never a local
   re-derivation.
4. Only this crate cares → private, right here.

Copying a constant, table, or ordering between crates is the signal you picked the wrong
floor — move the fact instead of finishing the copy.

## A rule is born with its gate

A new norm lands in the same commit as the test that enforces it, or as a doc-comment on
the code it governs — those are the two homes. This file grows judgment only; a list
that mirrors code belongs in a registry that generates the copies.

## The second copy promotes

The second verbatim copy is the promotion moment — not the third. Decide by behavior:
identical for a grammar it has never seen → toolkit; grammar knowledge → it stays in its
adapter, next to the grammar.

## Reach for the type

- A second `bool` lands in a struct → make the states an enum.
- Two fields must agree → merge them into the one type that cannot disagree.
- A `String` is compared for equality → newtype it.
- A doc-comment lists invalid combinations → the shape is wrong; make them
  unrepresentable.
- Adjacent same-typed parameters → a struct or a builder.

Evidence flows in through sinks and out through exhaustive types. When an API needs
prose to explain which fields matter, redesign the API, not the prose.

## The ignorance rule

Core never names a language. The moment `if language == X` looks necessary, the
vocabulary is missing a concept: add an `ExtensionSpec` capability — with a default, a
named consumer in core, and a conformance case. All three, or it doesn't merge.

## Where language knowledge lives

A language needs something new? Take the first floor that fits:

1. A fact about ONE FILE's content — something extraction sees → an evidence stream:
   a sink method paired with its `EvidenceStream` declaration; the pair ships
   together, so absence stays typed and analyses abstain instead of guessing.
2. A fact about THE LANGUAGE itself — true for every file (visibility rungs, cycle
   idioms, builtin member types) → `ExtensionSpec` data, with a default that reproduces
   pre-capability behavior, a named core consumer, and a conformance case.
3. A fact about THE PROJECT around the file — what exists, what manifests declare →
   a `ResolveContext` query, engine-provided.
4. A mechanism a second adapter would copy verbatim → the toolkit.

Wrong-floor signals: an analysis branching on an extension coordinate; an extension parsing what
a manifest or another adapter already parsed; a capability whose consumer you cannot
name.

## Determinism

Same tree ⇒ byte-identical output, at any thread count, on any machine. Order-dependent
logic consumes sorted inputs; analysis runs on fuel and evidence, and wall-clock time
stays outside the engine. When two runs differ, the run is the bug — the gate stays as
it is.

## Contract changes are loud

Finding identity, cache keys, the output schema, and conformance fixtures are contracts:
a fixture diff is either your bug or a deliberate change, called out in the PR and in
`DECISIONS.md`. Green-by-regeneration is how baselines break in silence.

## Comments

A comment records a present invariant or a non-obvious why. The diff's story — what
changed, what it used to be, why it's correct — lives in the commit message; if deleting
"used to / no longer" empties a comment, delete the comment.

## Verify without destroying

Prove a test fails without its fix using a file copy or a second worktree;
`checkout`/`stash` are navigation, never verification. The toolchain is pinned: local
and CI run the same compiler by construction, and a toolchain bump is its own PR with
the full suite.

## Reality before promises

A target, format, or channel earns its row in the docs or the release table by being
exercised in CI first. Fixtures for ingested formats are captured from real producers. A
new rule's first run is against the foreign corpus — our own fixtures only prove we
agree with ourselves.

## Scope belongs to the owner

Deliver the plan at its stated scope. A milestone, gate, or design piece is cut only by
the owner's explicit decision, recorded in `DECISIONS.md` — never quietly by whoever
builds it. Too big? Say so and propose the cut; a half-shipped feature nobody decided to
halve is worse than either whole or absent.

## Hygiene

English everywhere in the repo. Conventional commits — CI lints them. `DECISIONS.md` is
append-only, one dated entry per decision with its measurement, and code never cites it
by number.
