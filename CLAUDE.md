# Working in this repo

This file exists because `kndo-core`'s architecture is deliberate — normative contracts,
an `Engine` facade, an "ignorance rule" for language-agnosticism — and every rule below
codifies a divergence that actually happened between that architecture and the code, found
during a full ergonomics audit (see `internal/contracts/core-traits.md` and the git history
on `claude/core-api-ergonomics-architecture-983pom`). Follow these to keep it from
happening again.

## Contract first

`internal/contracts/core-traits.md` is normative. If a PR changes a contractual signature
(a public trait, `Engine`'s public methods, a §-numbered type) without updating the doc in
the same PR, the PR is incomplete — not "follow-up docs." Never leave the contract
describing code that doesn't exist (we once had `Engine::explain` documented and never
implemented) or code describing behavior the contract doesn't mention.

## Fachada: frontends import only the root re-exports

`kndo-cli` and any future frontend (`kndo serve`/MCP, LSP, GUI) import only from
`kndo::<Name>` — the root re-exports `kndo-core`/`kndo` publish, never a frontend-facing
internal module (`kndo::engine::X`, `kndo::vocab::X`, `kndo_core::...` directly). If a
frontend needs a piece of data or logic that isn't exported yet, that's a PR to core: add
the field to `RunResult`, export the helper, add it to the root re-export list — never a
local re-derivation or a reach into an internal module. `sort_findings_for_display` is the
precedent to follow. (We once had CLI-side copies of group ordering and grade-boundary
tables that silently drifted from core's own logic — that's the failure mode this rule
exists to prevent.)

## Ignorance rule

Core never names a language. If a feature seems to need `if language == "go"` in core, the
real gap is missing vocabulary — extend `vocab.rs`/the adapter contract instead of
conditioning on identity.

## One source per concept

Group/category display order, confidence labels, grade thresholds, severity ranking — any
of these is data core exports, never a constant copied into a frontend or another crate. If
you find yourself copying a constant or table between crates, it belongs in core (frontend
concerns) or the adapter toolkit (adapter concerns), not duplicated at the call site.

## Cache invalidation: pick the right knob, not every knob

Three constants, three questions, and they are not interchangeable:

- **`cache::ENTRY_FORMAT_VERSION`** — a type in the facts contract changed shape (`FileFacts`
  and anything reachable from it: `Declaration`, `FunctionMetrics`, …). ONE bump; it is folded
  into the facts entries *and* the graph key.
- **`AdapterDescriptor::facts_schema_version`** — one adapter changed what it emits (new roots,
  corrected spans, a different claim rule). Bump that adapter only.
- **`GRAPH_SCHEMA_VERSION`** — the persisted graph's own shape (rkyv layouts) or the assembly
  semantics that derive a graph from the same facts.

Bumping every adapter for a core-contract change is the failure this exists to prevent: the
same fact spelled six-plus times, silently under-invalidating the moment someone bumps five of
six. If you are about to edit more than one `facts_schema_version` in a single change, you want
`ENTRY_FORMAT_VERSION` instead.

## Errors

Use `thiserror` enums with `Display` impls. Do not introduce a new `Result<_, String>` —
error messages that reach JSON envelopes or CLI output should come from a typed error, not
ad hoc string formatting.

## Config

All defaults and all precedence between config sources live in `config::EffectiveConfig`.
Don't write `unwrap_or(SomeConfig::default().field)` or a second merge site outside it —
that's exactly how the engine ended up with the same merge logic duplicated in two places.

## Finding identity is a stability contract

`Group`, `Category`, `SubjectKind` string values and the field order passed into
`finding_id`/`FindingIdParts` are contract, not implementation detail — baselines,
suppressions, and every adapter's `expected.json` depend on them being stable. A
conformance fixture diff is either a bug in your change or a deliberate, documented
contract change (call it out in the PR) — it is never something to fix by regenerating the
fixture.

## Tests

Mocks and builders come from `kndo-core`'s `testkit` feature (`MockAdapter`, and friends) —
don't hand-roll a local mock adapter or a duplicate toy DSL. Use `tempfile` for any
temporary directory a test needs; never a hand-rolled name under `std::env::temp_dir()`
(name collisions under parallel test execution are a real, previously-hit race).

## Adapters and the toolkit

Before writing a helper in an adapter crate, check `kndo-adapter-toolkit` first. A helper
that's genuinely universal (same behavior regardless of grammar — text extraction, node
lookup, metrics plumbing, path helpers) belongs in the toolkit, gated by a Cargo feature if
it pulls in a grammar-specific dependency (see the toolkit's `jvm-xml` feature for the
pattern). Boilerplate copied into two or more adapters unchanged is a promotion candidate,
not a pattern to keep copying a third time. The reverse also holds: something that's
genuinely grammar-specific (parsing/dumping a particular tree-sitter grammar) stays in its
own adapter — don't force a shared abstraction over unrelated grammars just because two
adapters happen to need "a parse function."

## Plugins

Implement `Plugin::mutates_graph()` deliberately — it has no default. Returning `true` when
a plugin doesn't actually mutate the graph silently disables incremental patching for every
project that plugin runs on; returning `false` when it does mutate causes correctness bugs.
Decide it, don't default it.

## Gates that must never regress

These are checked by name in CI — the `gates` job in `.github/workflows/ci.yml` runs one step
per entry below, and its first step fails if any of them has been renamed or deleted, which a
bulk `cargo test --workspace` cannot notice. Adding an entry here means adding its step there;
the two lists are one list. All of them must stay green on every PR that touches
graph/cache/analysis:

- `patch_equivalence` — full assembly and incremental patch produce identical graphs.
- cache equivalence — a cached run and `--no-cache` produce byte-identical output.
- `--threads 1` and the default thread count produce identical, deterministic output.
- Every adapter's `tests/conformance.rs` fixtures stay byte-identical unless the PR is a
  deliberate, documented contract change.
- `dogfood` (`crates/kndo/tests/dogfood.rs`) — **kndo on kndo reports nothing.** Zero is the
  standing state, not a target: the `ACCEPTED` list in that file is empty, and an entry added to
  it needs a written reason in the same commit. Its second test is the one that keeps the first
  honest — no analysis other than `crap` may abstain, because a change that quietly stopped an
  analysis from judging would make a zero-findings gate *easier* to pass, which is the one
  failure direction a dogfood gate must not have.
- `plugin_dependency_implication` / `adapter_dependency_implication` — **a plugin named in
  another plugin's `dependencies` activates even when its own rules never match.** This is the
  only path to a plugin whose framework is an *indirect* dependency: a company framework that
  uses Express internally is never `express` in its users' manifests, so `kndo:express` can
  never self-activate there. No plugin we ship uses it, and it must exist anyway — that is what
  makes it easy to delete by accident. It already nearly went: a descriptor constructor that
  hid four of `PluginDescriptor`'s six fields made the field invisible in every built-in, and
  nothing failed.
- `builtin_plugin_proofs` (`crates/kndo/tests/builtin_plugin_proofs.rs`) — **every built-in
  plugin has a baseline-then-plugin proof**, the standard `docs/src/plugins/authoring.md` already
  demands of anyone writing one: the fixture's findings fire without the plugin, exactly those
  disappear with it, unrelated dead code stays reported, and `PluginContribution` matches down
  to `dropped`. `every_built_in_plugin_is_proven_here` closes the file against
  `default_plugins()`, so a new built-in without a proof fails the suite — the same posture as
  `Plugin::mutates_graph()` having no default. A plugin nothing asserts is a plugin nothing
  notices breaking, and the effect of one is measured in findings that silently return.

No PR should weaken or skip one of these to get green.
