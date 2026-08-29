# kndo — Documentation

> **kndo** is a multi-language static analyzer that finds what your codebase no longer needs:
> unused dependencies, dead code, duplicated code, unused files, and code that only tests keep alive.
> It reports overall project health, is fast enough to run as a pre-commit hook, and is designed
> to be consumed by both humans and AI agents to prevent "AI slop" from accumulating.

This directory is the single source of truth for product and technical decisions.
**Nothing gets coded until it is specified here.** Documents are iterated in-place via PRs.

## Product vision

### The problem

Codebases accumulate waste faster than ever. Refactors leave orphaned files behind, dependencies
outlive the feature that needed them, copy-paste spreads, and — increasingly — AI coding agents
generate plausible code that nothing actually uses. Each individual leftover is cheap; the
aggregate is expensive: slower builds, slower onboarding, misleading search results, larger attack
surface, and agents that read dead code as if it were live and compound the mess ("AI slop").

Existing tools are siloed per language (knip, depcheck, deadcode, ts-prune, jscpd…), each with its
own config, output format, and blind spots. None of them answers the question a team — or an agent
about to edit the repo — actually has: **"what in this project is no longer earning its place?"**

kndo is a **single, fast, multi-language static analyzer** focused on *waste detection and
project health*, not on style. One binary, one config (optional), one output schema.

### Core detections (1.0)

| Detection | Question it answers |
|-----------|--------------------|
| Unused dependencies | Which declared packages does no code import? |
| Unused code | Which symbols (functions, classes, types…) are unreachable from any entry point? |
| Unused files | Which files does nothing import or reference? |
| Test-only code | Which "production" code is only ever reached from tests? (candidate for deletion) |
| Test-blind spots | Which production code does no test even reach, transitively? (static, no coverage report needed) |
| Version skew | Which dependency is declared at diverging versions across workspace packages? |
| Duplicate code | Which blocks are structural copies of each other — and which files are byte-identical? |
| Excess visibility | Which symbols are exported/public but could be private? |
| Private type leaks | Which public APIs reference types their consumers cannot name? |
| Dependency cycles | Which files/modules form import cycles? |
| Boundary erosion | Which imports reach into a package's internals — a workspace sibling's or an external dependency's — bypassing its declared surface? |
| CRAP score | Which functions are complex **and** untested — Change Risk Anti-Patterns? |
| Project health | One number (0–100) + grade summarizing the above, trendable over time. |

### Product principles

1. **Fast enough to never skip.** Warm incremental runs complete in **< 500 ms** on large repos so
   kndo can live in a pre-commit hook. Speed is a feature, not an optimization.
2. **Languages are pluggable; the core is language-blind.** The core defines a language-neutral
   vocabulary (files, symbols, references, entry points). Language adapters translate source code
   into that vocabulary. Adding a language never touches the core.
3. **Language-adjacent knowledge lives in plugins.** Framework conventions (React, Spring, ...),
   coverage-report formats, and organization-specific rules are plugins layered on top of
   adapters — the adapter handles only what the language spec defines. Plugins aren't limited to
   contributing roots, edges, and annotations to core-owned verdicts: a plugin can emit its own
   first-class findings under its `plugin:<coordinate>/<rule>` namespace, sectioned apart from
   core output and advisory to the exit-code gate unless a team opts a rule in (RFC 0018).
4. **Honest about uncertainty.** Static analysis of dynamic languages cannot be perfect. Every
   finding carries a confidence level; dynamic constructs lower confidence instead of producing
   false "definitely dead" claims.
5. **Changes are judged by their blast radius.** Analyzing a diff means analyzing everything the
   diff *affects*: if your change makes a distant symbol become used (or unused), that shows up in
   the report even though you never touched that file.
6. **Dual audience: humans and agents.** Human output is a readable terminal summary. Programs
   get a stable, versioned JSON schema (plus SARIF). LLM agents get a third first-class format:
   token-frugal deterministic text (`--format agent`) that keeps every machine anchor (ids,
   selectors, explicit elision) at a fraction of JSON's token cost. All come from the same
   engine — an agent running kndo after generating code gets machine-checkable feedback that
   its additions are wired in and nothing became orphaned.
7. **The graph is a product, not just an implementation detail.** Having paid for a whole-project
   semantic graph, kndo exposes it read-only as navigation verbs (find, describe, uses, used-by,
   trace, impact) so exploration costs milliseconds and bounded output instead of context-window
   budget — precise answers replace speculative file reading.
8. **Adoptable in brownfield repos.** A baseline file lets legacy findings be acknowledged so only
   *new* waste fails the hook. Health improves incrementally, never via a big-bang cleanup.

### Users & primary flows

| User | Flow |
|------|------|
| Developer | `kndo check --staged` in pre-commit: blocks the commit only for waste *introduced or caused by* the staged changes. |
| Developer | `kndo check` locally: full report, explore findings, `kndo explain <id>`. |
| CI | `kndo-action` on every PR: gates the merge on new findings and upserts one sticky comment with the delta and health movement (RFC 0010). |
| AI agent | Runs kndo after edits; consumes JSON to verify its new code is reachable, deleted code freed dependencies, no duplication introduced. |
| AI agent | Navigates via the graph instead of grep-and-read: `kndo find/describe/uses/used-by/trace/impact` answer "who uses this?", "why is this alive?", "what breaks if I delete it?" in bounded, verifiable calls (RFC 0007). |
| Tech lead | Health score and per-category trends over time; CRAP hotspot list for refactor planning. |

### Supported languages (initial)

JavaScript/TypeScript (incl. JSX/TSX), Go, Java, Kotlin, Swift, Rust, JSON, CSS (incl. SCSS/LESS
variants), HTML. The adapter contract is the extension point; a new language lands as a new
adapter crate with zero core changes (see [RFC 0002: Language adapters](ARCHITECTURE.md#rfc-0002-language-adapters)).

### Non-goals

- **Not a linter/formatter.** No style rules, no autofix of formatting. (ESLint, gofmt, ktlint own that.)
- **Not a type checker or compiler.** kndo never blocks on code that doesn't compile; it degrades gracefully.
- **Not a security scanner.** No CVE/vulnerability analysis (though removing unused dependencies shrinks the surface).
- **Not a coverage tool.** Coverage is *ingested* from existing reports for CRAP, never measured (ADR 0005).
- **No IDE integration in 1.0.** CLI + JSON/SARIF first; LSP server is a possible post-1.0 layer.

### Success criteria

- Warm incremental run: **p95 < 500 ms** on a 5k-file repo; cold full run p95 < 10 s (parallel).
- False-positive rate on "unused" findings low enough that teams keep `--staged` blocking enabled
  (target: < 2% of findings marked as wrong via suppressions in dogfooding repos).
- A new language adapter can be built by a third party against the published contract without
  patching the core.
- kndo runs on itself in its own pre-commit hook from milestone M2 onward (dogfooding).

## Document map

`internal/` is a fixed set of 10 top-level documents. Each RFC/ADR/contract/adapter spec that
used to be its own file is now a section — headed `## RFC 000N: ...`, `## ADR 000N: ...`, etc.,
keeping its original number — inside one of the files below. Existing citations like "RFC 0012
§8" stay valid: look up RFC 0012's section inside the file that now contains it (see the table).

| Document | Contents | Status |
|----------|----------|--------|
| [README.md](README.md) | This file: product vision, document map, conventions, open questions. | — |
| [ROADMAP.md](ROADMAP.md) | Milestone log (M0–…), exit criteria per milestone. Updated on its own cadence, separate from the design docs. | Draft (in progress) |
| [ADRS.md](ADRS.md) | The 7 Architecture Decision Records (ADR 0001–0007), one section each. | Accepted |
| [CONTRACTS.md](CONTRACTS.md) | The 3 normative contracts: core traits, output schema (JSON), WASM ABI. | Accepted |
| [ADAPTERS.md](ADAPTERS.md) | Per-language adapter specs — the 8 shipped languages plus HTML, one section each. | Draft (each section carries its own milestone) |
| [ARCHITECTURE.md](ARCHITECTURE.md) | RFC 0001 (system architecture), RFC 0002 (language-adapter contract), RFC 0016 (uniform component model). | Accepted |
| [PLUGINS.md](PLUGINS.md) | RFC 0003 (plugin system), RFC 0015 (plugin identity & dependencies), RFC 0017 (plugin platform, second pass), RFC 0018 (plugin findings). | Accepted |
| [GRAPH-CACHE-AND-ANALYSES.md](GRAPH-CACHE-AND-ANALYSES.md) | RFC 0004 (graph & cache), RFC 0013 (incremental graph patch), RFC 0005 (analyses & metrics), RFC 0012 (reference semantics & visibility). | Accepted |
| [CLI-OUTPUT-AND-INTERFACE.md](CLI-OUTPUT-AND-INTERFACE.md) | RFC 0006 (CLI & output), RFC 0007 (graph navigation & query), RFC 0009 (human interface), RFC 0010 (CI: GitHub Action & PR reporting). | Accepted (RFC 0010: Implemented, dogfooded by `ci.yml`) |
| [PERFORMANCE-WORKSPACES-AND-RELEASE.md](PERFORMANCE-WORKSPACES-AND-RELEASE.md) | RFC 0008 (performance & parallelism), RFC 0011 (workspaces & monorepos), RFC 0014 (distribution & release), plus the warm-budget spike as an appendix. | Accepted, except RFC 0014 (mechanism unexercised — §7 punch list) |

[`internal/detection-gaps.md`](detection-gaps.md) sits outside this table on purpose: it is a
live operational reference — the running log of kndo's own known recall/precision gaps, cited by
`kndo.toml` `[[rule]]` entries and inline `kndo:allow` pragmas across the codebase — not a design
document describing a system. It gets appended to as gaps are found and fixed; it doesn't go
through the RFC/ADR lifecycle below.

## Conventions

- **RFCs** describe *systems* — they are living documents, updated as designs evolve. Each now
  lives as a numbered section inside one of the files above rather than its own file, but the
  numbering and lifecycle are unchanged.
- **ADRs** record *decisions* — immutable once `Accepted`; superseded by new ADRs, never edited.
  All 7 live as sections in [ADRS.md](ADRS.md).
- **Contracts** are the normative interfaces (Rust traits, JSON schemas, WASM ABI), collected in
  [CONTRACTS.md](CONTRACTS.md). Code must match them; changing a contract requires updating the
  document in the same PR.
- Status values: `Draft` → `Review` → `Accepted` (RFCs), `Proposed` → `Accepted` / `Superseded`
  (ADRs). A document's own section header carries its status; the table above summarizes.
- Documents are written in English (lingua franca for OSS and agent consumption); discussion can
  happen in any language.

## Open questions

**M0 is closed — none remain.** Resolution log:

1. Name → `kndo` everywhere (ADR 0007, in [ADRS.md](ADRS.md)). *Pending actions outside this
   repo: rename the GitHub repository, reserve crates.io/npm names.*
2. Candidate rules → fully triaged (RFC 0005 §13, in
   [GRAPH-CACHE-AND-ANALYSES.md](GRAPH-CACHE-AND-ANALYSES.md)); `deep-import`, briefly deferred,
   was adopted same-day once the contract-gate design removed the noise objection (RFC 0011 §4,
   in [PERFORMANCE-WORKSPACES-AND-RELEASE.md](PERFORMANCE-WORKSPACES-AND-RELEASE.md)).
3. `test-only` default severity → **info**, revisit at M6 with dogfooding data (RFC 0005 §3, in
   [GRAPH-CACHE-AND-ANALYSES.md](GRAPH-CACHE-AND-ANALYSES.md)).
4. Navigation verbs → **flat** (`kndo uses`), per RFC 0007 §8 draft stance, now decision (in
   [CLI-OUTPUT-AND-INTERFACE.md](CLI-OUTPUT-AND-INTERFACE.md)).

Per-document open questions that remain (RFC 0007 §8 items 2–3, RFC 0009 §8 — both in
[CLI-OUTPUT-AND-INTERFACE.md](CLI-OUTPUT-AND-INTERFACE.md) — and each adapter spec's own §7, in
[ADAPTERS.md](ADAPTERS.md)) carry explicit draft stances that are the decision of record until
implementation experience argues otherwise — revisiting one is a normal PR against the doc, not a
blocker.
