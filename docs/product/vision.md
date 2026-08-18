# kondo — Product Vision

**Status:** Draft · **Owner:** eulke · **Last updated:** 2026-08-18

## 1. The problem

Codebases accumulate waste faster than ever. Refactors leave orphaned files behind, dependencies
outlive the feature that needed them, copy-paste spreads, and — increasingly — AI coding agents
generate plausible code that nothing actually uses. Each individual leftover is cheap; the
aggregate is expensive: slower builds, slower onboarding, misleading search results, larger attack
surface, and agents that read dead code as if it were live and compound the mess ("AI slop").

Existing tools are siloed per language (knip, depcheck, deadcode, ts-prune, jscpd…), each with its
own config, output format, and blind spots. None of them answers the question a team — or an agent
about to edit the repo — actually has: **"what in this project is no longer earning its place?"**

## 2. The product

kondo is a **single, fast, multi-language static analyzer** focused on *waste detection and
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
| CRAP score | Which functions are complex **and** untested — Change Risk Anti-Patterns? |
| Project health | One number (0–100) + grade summarizing the above, trendable over time. |

### Product principles

1. **Fast enough to never skip.** Warm incremental runs complete in **< 500 ms** on large repos so
   kondo can live in a pre-commit hook. Speed is a feature, not an optimization.
2. **Languages are pluggable; the core is language-blind.** The core defines a language-neutral
   vocabulary (files, symbols, references, entry points). Language adapters translate source code
   into that vocabulary. Adding a language never touches the core.
3. **Language-adjacent knowledge lives in plugins.** Framework conventions (React, Spring, ...),
   coverage-report formats, and organization-specific rules are plugins layered on top of
   adapters — the adapter handles only what the language spec defines.
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
   engine — an agent running kondo after generating code gets machine-checkable feedback that
   its additions are wired in and nothing became orphaned.
7. **The graph is a product, not just an implementation detail.** Having paid for a whole-project
   semantic graph, kondo exposes it read-only as navigation verbs (find, describe, uses, used-by,
   trace, impact) so exploration costs milliseconds and bounded output instead of context-window
   budget — precise answers replace speculative file reading.
8. **Adoptable in brownfield repos.** A baseline file lets legacy findings be acknowledged so only
   *new* waste fails the hook. Health improves incrementally, never via a big-bang cleanup.

## 3. Users & primary flows

| User | Flow |
|------|------|
| Developer | `kondo check --staged` in pre-commit: blocks the commit only for waste *introduced or caused by* the staged changes. |
| Developer | `kondo check` locally: full report, explore findings, `kondo explain <id>`. |
| CI | `kondo-action` on every PR: gates the merge on new findings and upserts one sticky comment with the delta and health movement (RFC 0010); any other CI consumes the same JSON. |
| AI agent | Runs kondo after edits; consumes JSON to verify its new code is reachable, deleted code freed dependencies, no duplication introduced. |
| AI agent | Navigates via the graph instead of grep-and-read: `kondo find/describe/uses/used-by/trace/impact` answer "who uses this?", "why is this alive?", "what breaks if I delete it?" in bounded, verifiable calls (RFC 0007). |
| Tech lead | Health score and per-category trends over time; CRAP hotspot list for refactor planning. |

## 4. Supported languages (initial)

JavaScript/TypeScript (incl. JSX/TSX), Go, Java, Kotlin, Swift, Rust, JSON, CSS (incl. SCSS/LESS
variants). The adapter contract is the extension point; a new language lands as a new adapter
crate with zero core changes (see [RFC 0002](../rfcs/0002-language-adapters.md)).

## 5. Non-goals

- **Not a linter/formatter.** No style rules, no autofix of formatting. (ESLint, gofmt, ktlint own that.)
- **Not a type checker or compiler.** kondo never blocks on code that doesn't compile; it degrades gracefully.
- **Not a security scanner.** No CVE/vulnerability analysis (though removing unused dependencies shrinks the surface).
- **Not a coverage tool.** Coverage is *ingested* from existing reports for CRAP, never measured (ADR 0005).
- **No IDE integration in 1.0.** CLI + JSON/SARIF first; LSP server is a possible post-1.0 layer.

## 6. Success criteria

- Warm incremental run: **p95 < 500 ms** on a 5k-file repo; cold full run p95 < 10 s (parallel).
- False-positive rate on "unused" findings low enough that teams keep `--staged` blocking enabled
  (target: < 2% of findings marked as wrong via suppressions in dogfooding repos).
- A new language adapter can be built by a third party against the published contract without
  patching the core.
- kondo runs on itself in its own pre-commit hook from milestone M2 onward (dogfooding).
