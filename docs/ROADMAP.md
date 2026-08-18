# Roadmap

**Status:** Draft · Milestones ship in order; each has explicit exit criteria. Docs are updated in
the same PR as the code they describe — a milestone isn't done if its documents lie.

## M0 — Definition (now)
All RFCs/ADRs/contracts in this directory reach `Accepted` after debate. Open questions in
[README](README.md) resolved or explicitly deferred.

**Exit:** sign-off on contracts (`core-traits.md`, `output-schema.md`); candidate-rule table
(RFC 0005 §13) triaged into 1.0 / later / rejected.

## M1 — Skeleton + first language (JS/TS), full-scan only
Workspace layout (`kondo-cli`, `kondo-core`, `kondo-adapter-toolkit`, `kondo-adapter-js`),
graph model per contracts, discovery, extraction, resolution driver, conformance harness.
Package model per RFC 0011 with npm/pnpm/yarn workspaces (the single-package repo is the n = 1
case, so this is foundation, not a feature).
Analyses: `unused` (symbols, files, dependencies — with file/directory rollup), `undeclared`,
`version-skew` (manifest-only, rides the RFC 0011 package model), and `duplicate` on
byte-identical files (subject `file` — free on content hashes).
Output: human (RFC 0009 visual language, incl. degradation tiers), JSON v1.0.0, and agent
format v1 (output-schema §9) — all emitted through the `Engine` facade (contracts §5 — the CLI
is a frontend from day one, not a shortcut). No cache yet (cold runs only).

**Exit:** correct findings on fixture corpus + 3 real OSS TS repos; `kondo check` on kondo's own
JS-free repo returns cleanly; JSON validates against generated schema.

## M2 — Cache, incrementality, diff modes, pre-commit
`.kondo/` cache (ADR 0004), warm-run algorithm, dirty-region analysis, `--staged`/`--diff` with
derived-effects delta (RFC 0004 §6), baseline + suppressions, exit codes, `kondo init` hook
installer, `kondo doctor`. First navigation verbs over the warm graph (RFC 0007): `find`,
`describe`, `uses`, `used-by`, `trace` with query JSON envelopes, multi-selector support and
`kondo query` (composite JSONL queries, single graph load).

**Exit:** **warm p95 < 500 ms** on the 5k-file benchmark repo (CI-enforced benchmark, navigation
verbs included); RFC 0008 gates live: `--threads 1` ≡ `--threads N` byte-identical output,
≥ 3× scaling at 8 cores on the 100-file scenario, > 10% regressions block merge; `--no-cache` ≡
cached results on fixture matrix; kondo runs in kondo's own pre-commit (dogfooding begins).

## M3 — Reachability semantics complete + second language (Go)
`test-only` (three-color reachability), `untested` (its inverse — same coloring passes),
tooling roots, wildcard-edge conservatism, confidence surfacing, library mode, `internal-only`
(visibility ladders ride the same reference-origin machinery), `delta_origin`
introduced/derived on diff findings (RFC 0004 §6). Go adapter proves the contract fits a second language
without core changes — any needed contract change happens *here*, cheaply. Navigation completes:
liveness traces (`trace X` from roots), `used-by --split-by-color`, and `kondo impact`
(incl. `--if-deleted` simulation, reusing the derived-effects machinery).

**Exit:** Go conformance corpus passes; a deliberately-injected "only tests call this" fixture is
caught in both languages; the RFC 0007 §5 agent workflow (find → used-by → impact → check)
runs end to end on a fixture; contract diffs (if any) documented in updated contracts + ADR.

## M4 — Duplication, CRAP, health
`duplicate` (winnowing index, incremental), `cyclic` (SCCs with per-language tolerance),
`crap` + lcov/JaCoCo ingestion plugins (ADR 0005), `health` score + `kondo health`, SARIF output.

**Exit:** duplication findings stable under reformatting (Type-2); CRAP hotlist matches manual
audit on a real repo; health deltas shown in diff modes.

## M5 — Remaining languages + plugin system GA
Adapters: Java, Kotlin, Swift, Rust, JSON, CSS (order: Java → Kotlin share infra; Rust; Swift;
CSS+JSON close cross-language edges). First-party ecosystem plugins for detected frameworks
(initial set per RFC 0003 §3). **WASM plugin/adapter ABI** (`kondo-plugin-api`) published with a
sample external plugin + compliance suite.

**Exit:** all eight launch languages pass conformance; a third-party demo adapter (not in-tree)
runs against the released binary; budget still holds with all adapters active.

## M6 — 1.0 hardening
False-positive hunt across dogfood corpus (target < 2%, vision §6), schema/ABI freeze, docs site,
install channels (brew/cargo/npm shim/curl), **`kondo-action` GA** (RFC 0010: sticky PR comment,
annotations, SARIF opt-in — dogfooded on kondo's own PRs from M2 via a pre-GA workflow),
`stale` (suppressions) rule, error-message polish.

**Exit:** semver 1.0 commitments declared for the three contract surfaces; two external repos
adopt kondo in pre-commit and stay enabled for 2 weeks.

## Post-1.0 parking lot
`kondo clean` (guided auto-removal), watch mode / LSP, custom analyses over a stable graph API,
`kondo serve` exposing the navigation verbs 1:1 as MCP tools (RFC 0007 §7), an arbitrary graph
query language, deep mode (compiler-grade resolvers), historical trend service, HTML report,
tsconfig project-references deep integration, deferred rules from the RFC 0005 §13 triage
(`layer-violation`, `oversized-unit`) and ecosystem-plugin rules (`barrel-abuse`,
`dead-feature-flag`, churn×complexity hotspots via a git plugin, overlapping-dependency
knowledge base, config-key drift), and **divergent clones** — near-identical clones where one
copy got a fix the other didn't ("the bug you fixed here still lives there"), the natural
Type-3 extension of the winnowing index and the headline candidate for the first post-1.0
release.

## Standing rules

- A language lands only via the adapter contract; a core `if language == X` reverts the PR.
- Every milestone keeps the benchmark green from M2 onward — performance regressions block merge.
- Contracts change only with the corresponding doc updated in the same PR.
