# kndo — Documentation

> **kndo** is a multi-language static analyzer that finds what your codebase no longer needs:
> unused dependencies, dead code, duplicated code, unused files, and code that only tests keep alive.
> It reports overall project health, is fast enough to run as a pre-commit hook, and is designed
> to be consumed by both humans and AI agents to prevent "AI slop" from accumulating.

This directory is the single source of truth for product and technical decisions.
**Nothing gets coded until it is specified here.** Documents are iterated in-place via PRs.

## Document map

| Area | Document | Status |
|------|----------|--------|
| Product | [product/vision.md](product/vision.md) | Accepted |
| Architecture | [rfcs/0001-architecture.md](rfcs/0001-architecture.md) | Accepted |
| Language adapters | [rfcs/0002-language-adapters.md](rfcs/0002-language-adapters.md) | Accepted |
| Plugin system | [rfcs/0003-plugin-system.md](rfcs/0003-plugin-system.md) | Accepted |
| Graph, cache & incrementality | [rfcs/0004-graph-and-cache.md](rfcs/0004-graph-and-cache.md) | Accepted |
| Analyses & metrics | [rfcs/0005-analyses.md](rfcs/0005-analyses.md) | Accepted |
| CLI, output & config | [rfcs/0006-cli-and-output.md](rfcs/0006-cli-and-output.md) | Accepted |
| Graph navigation & query | [rfcs/0007-graph-navigation.md](rfcs/0007-graph-navigation.md) | Accepted |
| Performance & parallelism | [rfcs/0008-performance-and-parallelism.md](rfcs/0008-performance-and-parallelism.md) | Accepted |
| Human interface (CLI rendering) | [rfcs/0009-human-interface.md](rfcs/0009-human-interface.md) | Accepted |
| CI: GitHub Action & PR reporting | [rfcs/0010-ci-github-action.md](rfcs/0010-ci-github-action.md) | Accepted |
| Workspaces & monorepos | [rfcs/0011-workspaces-and-monorepos.md](rfcs/0011-workspaces-and-monorepos.md) | Accepted |
| Precise reference semantics & visibility | [rfcs/0012-reference-semantics-and-visibility.md](rfcs/0012-reference-semantics-and-visibility.md) | Accepted |
| Incremental graph patch | [rfcs/0013-incremental-graph-patch.md](rfcs/0013-incremental-graph-patch.md) | Accepted |
| Distribution & release | [rfcs/0014-distribution-and-release.md](rfcs/0014-distribution-and-release.md) | Accepted · mechanism unexercised (§7 punch list) |
| Plugin identity & dependencies | [rfcs/0015-plugin-identity-and-dependencies.md](rfcs/0015-plugin-identity-and-dependencies.md) | Accepted |
| Uniform component model | [rfcs/0016-uniform-component-model.md](rfcs/0016-uniform-component-model.md) | Accepted (all phases landed) |
| Plugin platform: second pass | [rfcs/0017-plugin-platform-second-pass.md](rfcs/0017-plugin-platform-second-pass.md) | Accepted (all five §8 phases landed) |
| Spike: warm-budget validation | [spikes/0001-performance.md](spikes/0001-performance.md) | Done |
| Adapter spec: JS/TS | [adapters/js-ts.md](adapters/js-ts.md) | Draft (M1 working spec) |
| Adapter spec: Go | [adapters/go.md](adapters/go.md) | Draft (M3 working spec) |
| Core contracts (traits) | [contracts/core-traits.md](contracts/core-traits.md) | Accepted |
| Output schema (JSON) | [contracts/output-schema.md](contracts/output-schema.md) | Accepted |
| Roadmap | [ROADMAP.md](ROADMAP.md) | Accepted |

### ADRs (Architecture Decision Records)

| # | Decision | Status |
|---|----------|--------|
| [0001](adrs/0001-rust-for-the-core.md) | Rust for the core | Accepted |
| [0002](adrs/0002-tree-sitter-parsing.md) | tree-sitter as the universal parsing layer | Accepted |
| [0003](adrs/0003-adapter-linking-strategy.md) | First-party adapters compiled in; third-party plugins via WASM | Accepted |
| [0004](adrs/0004-cache-format.md) | Cache: content-addressed binary snapshot in `.kndo/` | Accepted |
| [0005](adrs/0005-coverage-ingestion.md) | Coverage is ingested, never measured, for CRAP | Accepted |
| [0006](adrs/0006-single-binary-zero-config.md) | Single static binary, zero-config by default | Accepted |
| [0007](adrs/0007-product-name.md) | Product name: `kndo` | Accepted |

## Conventions

- **RFCs** describe *systems* — they are living documents, updated as designs evolve.
- **ADRs** record *decisions* — immutable once `Accepted`; superseded by new ADRs, never edited.
- **Contracts** are the normative interfaces (Rust traits, JSON schemas). Code must match them;
  changing a contract requires updating the document in the same PR.
- Status values: `Draft` → `Review` → `Accepted` (RFCs), `Proposed` → `Accepted` / `Superseded` (ADRs).
- Documents are written in English (lingua franca for OSS and agent consumption); discussion can
  happen in any language.

## Open questions

**M0 is closed — none remain.** Resolution log:

1. Name → `kndo` everywhere (ADR 0007). *Pending actions outside this repo: rename the GitHub
   repository, reserve crates.io/npm names.*
2. Candidate rules → fully triaged (RFC 0005 §13); `deep-import`, briefly deferred, was adopted
   same-day once the contract-gate design removed the noise objection (RFC 0011 §4).
3. `test-only` default severity → **info**, revisit at M6 with dogfooding data (RFC 0005 §3).
4. Navigation verbs → **flat** (`kndo uses`), per RFC 0007 §8 draft stance, now decision.

Per-document open questions that remain (RFC 0007 §8 items 2–3, RFC 0009 §8, adapter spec §7)
carry explicit draft stances that are the decision of record until implementation experience
argues otherwise — revisiting one is a normal PR against the doc, not a blocker.
