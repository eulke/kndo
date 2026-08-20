# Roadmap

**Status:** Draft · Milestones ship in order; each has explicit exit criteria. Docs are updated in
the same PR as the code they describe — a milestone isn't done if its documents lie.

## M0 — Definition ✅ (completed 2026-08-18)
All RFCs/ADRs/contracts reached `Accepted` after debate; open questions resolved or explicitly
deferred with reasons of record.

**Exit (met):** contracts signed off (`core-traits.md`, `output-schema.md`); candidate-rule
table (RFC 0005 §13) fully triaged — no candidate left undecided; warm-budget feasibility
validated by [spike 0001](spikes/0001-performance.md) (73 ms measured vs 500 ms budget);
product name decided (ADR 0007).

## M1 — Skeleton + first language (JS/TS), full-scan only
Workspace layout (`kndo-cli`, `kndo-core`, `kndo-adapter-toolkit`, `kndo-adapter-js`),
graph model per contracts, discovery, extraction, resolution driver, conformance harness.
Package model per RFC 0011 with npm/pnpm/yarn workspaces (the single-package repo is the n = 1
case, so this is foundation, not a feature).
Analyses: `unused` (symbols, files, dependencies — with file/directory rollup), `undeclared`,
`version-skew` (manifest-only, rides the RFC 0011 package model), and `duplicate` on
byte-identical files (subject `file` — free on content hashes).
Output: human (RFC 0009 visual language, incl. degradation tiers), JSON v1.0.0, and agent
format v1 (output-schema §9) — all emitted through the `Engine` facade (contracts §5 — the CLI
is a frontend from day one, not a shortcut). No cache yet (cold runs only).

**Exit:** correct findings on fixture corpus + 3 real OSS TS repos; `kndo check` on kndo's own
JS-free repo returns cleanly; JSON validates against generated schema.

## M2 — Cache, incrementality, diff modes, pre-commit ✅ (completed 2026-08-20)
`.kndo/` cache (ADR 0004), warm-run algorithm, `--staged`/`--diff` with derived-effects delta
(RFC 0004 §6) reading git trees in memory (no tempfiles — content-addressed blob-hash sidecar
makes a warm diff's cost proportional to what changed, not repo size), baseline + suppressions,
exit codes, `kndo init` hook installer, `kndo doctor`. First navigation verbs over the warm graph
(RFC 0007): `find`, `describe`, `uses`, `used-by`, `trace` with query JSON envelopes,
multi-selector support and `kndo query` (composite JSONL queries, single graph load).
`--threads` + one global rayon pool (RFC 0008 §5).

**Exit (met):** warm p95 measured well under the 500 ms budget on the 5k-file benchmark repo
(full ~40 ms, `--staged` ~70 ms, `--diff` ~14 ms, a single changed file ~84 ms — all warm,
release build); `--threads 1` ≡ `--threads N` byte-identical output (test-enforced);
`--no-cache` ≡ cached results verified on the fixture matrix and the 5k benchmark; kndo runs in
kndo's own pre-commit hook (dogfooding is live on this repo's real commits, not simulated).

**Deferred out of M2** (RFC 0004 §4 kept as the accepted target design; see its "Implementation
status" note): the patch/dirty-region incremental algorithm (§4 step 4, §5's incremental
reachability-color BFS) — the current all-or-nothing rebuild already meets the warm budget at
benchmark scale, so building the patch algorithm wasn't required to close this milestone; revisit
if a larger real repo's rebuild cost grows past budget. Also deferred: wiring the above gates into
an actual CI workflow (measured manually this session — no CI/multi-core infra was available to
automate it) and the ≥ 3× scaling-at-8-cores check, which needs that same infra.

## M3 — Reachability semantics complete + second language (Go)
`test-only` (three-color reachability), `untested` (its inverse — same coloring passes),
tooling roots, wildcard-edge conservatism, confidence surfacing, library mode, `internal-only`
and `private-type-leak` (both directions of the visibility-mismatch comparison, RFC 0005 §7),
`deep-import` (contract-gated boundary erosion with pair-level rollup, RFC 0011 §4),
`delta_origin` introduced/derived on diff findings (RFC 0004 §6). Go adapter proves the contract fits a second language
without core changes — any needed contract change happens *here*, cheaply. Navigation completes:
liveness traces (`trace X` from roots), `used-by --split-by-color`, and `kndo impact`
(incl. `--if-deleted` simulation, reusing the derived-effects machinery).

**Exit:** Go conformance corpus passes; a deliberately-injected "only tests call this" fixture is
caught in both languages; the RFC 0007 §5 agent workflow (find → used-by → impact → check)
runs end to end on a fixture; contract diffs (if any) documented in updated contracts + ADR.

**Progress so far:** `untested` and `internal-only` implemented and dogfooded (RFC 0005 §7, §9).
Go adapter landed (`kndo-adapter-go`) — claiming, extraction, manifest, and resolution modules,
conformance fixtures, real-CLI-dogfooded against hand-built multi-file/multi-package projects
(both `unused` and `test-only` correctly cross the language boundary — the exit criterion's own
"only tests call this" case verified working for Go, not just JS). The contract genuinely needed
extending twice, both documented (contracts §2, RFC 0005 §1, RFC 0011 §4): `FileFacts::unit` for
package-scoped (not file-scoped) reference resolution, and a `ResolveCtx::files_in_dir` query for
languages whose import unit is a directory rather than a single file — plus one real, previously-
latent core bug the second language's shape exposed and fixed (`analysis/reachability.rs`: a
symbol-only root, the common case in Go, never propagated reachability to the file-attributed
references its own file makes). `deep-import`'s verdict and `kndo impact` remain — each needs
its own real design pass (subpath+surface tracking; a new navigation verb), not attempted as a
rushed add-on to this pass.

RFC 0012 (Precise Reference Semantics & Visibility) then landed its first six stages here:
`member_of` + the duck-typed member-call fallback (killing the unused:method false positive),
the Go `dir#package` unit key, symbol-granular reference attribution (`within`) in the core
and **both** adapters — transitive death is now visible in Go and JS/TS alike, pinned by
conformance fixtures in each — §5's `RefKind` tagging + callable `signature_span`s feeding
the `private-type-leak` analysis (RFC 0005 §7's second direction), likewise fixture-pinned in
both languages, and §6's ladder-as-data: adapters declare `(scope, label)` visibility rungs,
assembly carries them onto the graph, and three consumers read them — `internal-only`'s
tightest-sufficient rung (closing go.md's documented same-package under-reporting, fixture
`internal-only-unit/`), `private-type-leak`'s scope comparison, and the member fallback's
visibility-scoped candidacy. §7 (`detected_origin`) closed go.md's other documented trait
limitation: extraction now corrects the origin axis from content (Go's `DO NOT EDIT.` banner,
JS `@generated` markers, via the toolkit's bounded `ContentMarkers` scanner), so generated
files are exempt from findings even when their paths look authored — fixture `generated-file/`
in both languages, negative-control dogfooded. Remaining RFC 0012 stages (§§9–10:
qualified-reference resolution, go.work) are the current in-order work queue.

## M4 — Duplication, CRAP, health
`duplicate` (winnowing index, incremental), `cyclic` (SCCs with per-language tolerance),
`crap` + lcov/JaCoCo ingestion plugins (ADR 0005), `health` score + `kndo health`, SARIF output.

**Exit:** duplication findings stable under reformatting (Type-2); CRAP hotlist matches manual
audit on a real repo; health deltas shown in diff modes.

## M5 — Remaining languages + plugin system GA
Adapters: Java, Kotlin, Swift, Rust, JSON, CSS (order: Java → Kotlin share infra; Rust; Swift;
CSS+JSON close cross-language edges). First-party ecosystem plugins for detected frameworks
(initial set per RFC 0003 §3). **WASM plugin/adapter ABI** (`kndo-plugin-api`) published with a
sample external plugin + compliance suite.

**Exit:** all eight launch languages pass conformance; a third-party demo adapter (not in-tree)
runs against the released binary; budget still holds with all adapters active.

## M6 — 1.0 hardening
False-positive hunt across dogfood corpus (target < 2%, vision §6), schema/ABI freeze, docs site,
install channels (brew/cargo/npm shim/curl), **`kndo-action` GA** (RFC 0010: sticky PR comment,
annotations, SARIF opt-in — dogfooded on kndo's own PRs from M2 via a pre-GA workflow),
`stale` (suppressions) rule, error-message polish.

**Exit:** semver 1.0 commitments declared for the three contract surfaces; two external repos
adopt kndo in pre-commit and stay enabled for 2 weeks.

## Post-1.0 parking lot
`kndo clean` (guided auto-removal), watch mode / LSP, custom analyses over a stable graph API,
`kndo serve` exposing the navigation verbs 1:1 as MCP tools (RFC 0007 §7), an arbitrary graph
query language, deep mode (compiler-grade resolvers), historical trend service, HTML report,
tsconfig project-references deep integration, deferred rules from the RFC 0005 §13 triage
(`layer-violation`, `oversized-unit`, `redundant-export-binding`) and ecosystem-plugin rules (`barrel-abuse`,
`dead-feature-flag`, churn×complexity hotspots via a git plugin, overlapping-dependency
knowledge base, config-key drift), and **divergent clones** — near-identical clones where one
copy got a fix the other didn't ("the bug you fixed here still lives there"), the natural
Type-3 extension of the winnowing index and the headline candidate for the first post-1.0
release.

## Standing rules

- A language lands only via the adapter contract; a core `if language == X` reverts the PR.
- Every milestone keeps the benchmark green from M2 onward — performance regressions block merge.
- Contracts change only with the corresponding doc updated in the same PR.
