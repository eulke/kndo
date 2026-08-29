# Working in this repo

`kndo-core`'s architecture is deliberate: normative contracts, an `Engine` facade, an
"ignorance rule" for language-agnosticism. The rules below keep code and contracts in sync
with that architecture.

## Contract first

`internal/CONTRACTS.md` is normative. A PR that changes a contractual signature (a
public trait, `Engine`'s public methods, a §-numbered type) without updating the doc in the
same PR is incomplete — not "follow-up docs." The contract never describes code that doesn't
exist, and code never implements contract-affecting behavior the contract doesn't mention.

## Keep internal/ current

Every document `internal/README.md` indexes is normative for the subsystem it covers, the same
way `internal/CONTRACTS.md` is normative for the core traits. A PR that changes the
behavior one of them describes updates that document in the same PR. A document never describes
behavior the code doesn't have, and code never implements documented behavior the matching
document doesn't mention. `internal/detection-gaps.md` is a live reference cited by
`kndo.toml` and `kndo:allow` pragmas, not a design document, and isn't covered by this rule the
same way.

## Fachada: frontends import only the root re-exports

`kndo-cli` and any future frontend (`kndo serve`/MCP, LSP, GUI) import only from
`kndo::<Name>` — the root re-exports `kndo-core`/`kndo` publish, never a frontend-facing
internal module (`kndo::engine::X`, `kndo::vocab::X`, `kndo_core::...` directly). If a
frontend needs a piece of data or logic that isn't exported yet, that's a PR to core: add
the field to `RunResult`, export the helper, add it to the root re-export list — never a
local re-derivation or a reach into an internal module. `sort_findings_for_display` is the
precedent to follow: group ordering and grade-boundary logic live once, in core, and every
frontend calls it rather than keeping its own copy.

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

## Comments

A comment states what is true now — a non-obvious WHY, an invariant, a constraint — never a
contrast with an earlier state ("this used to be X", "no longer", "added for Y") and never a
promise about the future. If deleting "used to"/"previously"/"no longer" leaves a comment
meaningless, rewrite it as a present invariant or delete it. The same rule applies equally to
`///`/`//!` rustdoc and inline `//` comments: public API documentation states what callers can
rely on today, not the history of how it got there.

Write a comment only where the code alone would leave a reader stuck on a non-obvious WHY —
not to restate what a well-named function or type already says. A codebase with fewer, sharper
comments is easier to trust than one with a comment on every block: readers stop reading
comments once enough of them are noise.

## Config

All defaults and all precedence between config sources live in `config::EffectiveConfig`.
Never write `unwrap_or(SomeConfig::default().field)` or a second merge site outside it — merge
logic duplicated across two places drifts silently out of sync.

## Finding identity is a stability contract

`Group`, `Category`, `SubjectKind` string values and the field order passed into
`finding_id`/`FindingIdParts` are contract, not implementation detail — baselines,
suppressions, and every adapter's `expected.json` depend on them being stable. A
conformance fixture diff is either a bug in your change or a deliberate, documented
contract change (call it out in the PR) — it is never something to fix by regenerating the
fixture.

## Tests

Mocks and builders come from `kndo-core`'s `testkit` feature (`MockAdapter`, and friends) —
never a hand-rolled local mock adapter or a duplicate toy DSL. Use `tempfile` for any
temporary directory a test needs, never a hand-rolled name under `std::env::temp_dir()` —
parallel test execution can collide on a fixed name.

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

## The release surface: one producer, and nothing unverified until a tag

**One producer for the release artifact.** `xtask::package` owns the target table, the
artifact's name and its layout. `release.yml` calls `cargo xtask package`; it never builds an
archive itself. `xtask/tests/release_channels.rs` checks the four consumers — `install.sh`,
`action/action.yml`, the Homebrew template, the install docs — against that single definition,
never against each other and never by eye. If you change what a release produces, change it
there; if you add a consumer, add it to that test.

**A mechanism whose first run is the release is not verified.** CI exercises `git-cliff`'s
release-body rendering, `install.sh`'s install, the musl build, and the full suite on macOS and
Windows on every push — the same things a release needs, run before any tag exists. Before
adding a step that only runs during a release, build what exercises it beforehand — a step with
nothing exercising it first is the thing to fix. Deliberate exceptions are written down with
their measurement, not left silent: `cargo xtask bench` is not a CI gate because its baseline is
machine-specific (CONTRIBUTING "Benchmarks" has the numbers), and `epoch_deadline` is not
enabled because wall-clock cutoffs would break the determinism gates (`kndo-plugin-api`'s
`engine.rs`).

**Config the engine does not read is not shipped.** `config::LIVE_TABLES` is what `parse`
actually reads; `kndo init`'s template is checked against it. A commented-out key is still a
promise. Wire it or leave it out.

## Verify on the toolchain CI uses, and with the targets CI installs

"clippy is clean locally" is a claim about one toolchain. CI installs `stable`, which moves; a
container can sit several releases behind. If a CI job disagrees with a local run, compare
`rustc --version` **before** looking for anything subtler; `cargo +<version>` reproduces it.

The same holds for targets. The workspace suite builds real WASM components at run time, so a
job that runs it needs `targets: wasm32-unknown-unknown` on its toolchain step — without it the
build dies with "can't find crate for `core`". A step that always fails and a step that never
runs look the same from a distance; both are worse than no step, because the job list says the
invariant is covered.

## Gates that must never regress

These are checked by name in CI — the `gates` job in `.github/workflows/ci.yml` runs one step
per entry below, and its first step fails if any of them has been renamed or deleted, which a
bulk `cargo test --workspace` cannot notice. Adding an entry here means adding its step there;
the two lists are one list. All of them must stay green on every PR that touches
graph/cache/analysis — `doc_links`, whose subject is Markdown, on every PR that touches a
`.md` — and `check-doc-freshness`, whose subject is `internal/`'s own coverage, on every PR
whose diff touches a path in its table:

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
  makes it easy to delete by accident.
- `builtin_plugin_proofs` (`crates/kndo/tests/builtin_plugin_proofs.rs`) — **every built-in
  plugin has a baseline-then-plugin proof**, the standard `docs/src/plugins/authoring.md` already
  demands of anyone writing one: the fixture's findings fire without the plugin, exactly those
  disappear with it, unrelated dead code stays reported, and `PluginContribution` matches down
  to `dropped`. `every_built_in_plugin_is_proven_here` closes the file against
  `default_plugins()`, so a new built-in without a proof fails the suite — the same posture as
  `Plugin::mutates_graph()` having no default. A plugin nothing asserts is a plugin nothing
  notices breaking, and the effect of one is measured in findings that silently return.

- `doc_links` (`crates/kndo/tests/doc_links.rs`) — **every relative Markdown link in the
  repository resolves.** A link is the author asserting a path exists, and moving a document
  means updating what points at it in the same commit. Deliberately links only: prose paths
  carry too many false positives on this repository to be worth checking (examples from other
  repositories, invented illustrations, paths that exist in a *user's* project), so an analysis
  firing on those would be noise. The scanner blanks code spans first — a path inside backticks
  is quoted, not claimed.

- `check-doc-freshness` (`cargo xtask check-doc-freshness`, `xtask/src/doc_freshness.rs`) — **a
  diff that touches a source path with real design-doc coverage also touches the `internal/`
  document that covers it**, and a failure names the exact untouched document next to the path
  that needed it, never just "docs are stale." The path → document table
  (`doc_freshness::DOC_COVERAGE`) is deliberately small: a path absent from it carries no
  obligation, rather than forcing every file in the workspace to justify its silence. Needs the
  real base-vs-head diff to run, which a plain `cargo test` cannot discover without a git
  command — the `gates` job fetches the PR's base branch and runs it via `GITHUB_BASE_REF`, the
  environment variable GitHub Actions sets for a `pull_request` event; a push or non-PR run has
  no base to diff against and the step does not run.

No PR should weaken or skip one of these to get green.
