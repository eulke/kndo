# kondo — Documentation

> **kondo** is a multi-language static analyzer that finds what your codebase no longer needs:
> unused dependencies, dead code, duplicated code, unused files, and code that only tests keep alive.
> It reports overall project health, is fast enough to run as a pre-commit hook, and is designed
> to be consumed by both humans and AI agents to prevent "AI slop" from accumulating.

This directory is the single source of truth for product and technical decisions.
**Nothing gets coded until it is specified here.** Documents are iterated in-place via PRs.

## Document map

| Area | Document | Status |
|------|----------|--------|
| Product | [product/vision.md](product/vision.md) | Draft |
| Architecture | [rfcs/0001-architecture.md](rfcs/0001-architecture.md) | Draft |
| Language adapters | [rfcs/0002-language-adapters.md](rfcs/0002-language-adapters.md) | Draft |
| Plugin system | [rfcs/0003-plugin-system.md](rfcs/0003-plugin-system.md) | Draft |
| Graph, cache & incrementality | [rfcs/0004-graph-and-cache.md](rfcs/0004-graph-and-cache.md) | Draft |
| Analyses & metrics | [rfcs/0005-analyses.md](rfcs/0005-analyses.md) | Draft |
| CLI, output & config | [rfcs/0006-cli-and-output.md](rfcs/0006-cli-and-output.md) | Draft |
| Graph navigation & query | [rfcs/0007-graph-navigation.md](rfcs/0007-graph-navigation.md) | Draft |
| Performance & parallelism | [rfcs/0008-performance-and-parallelism.md](rfcs/0008-performance-and-parallelism.md) | Draft |
| Human interface (CLI rendering) | [rfcs/0009-human-interface.md](rfcs/0009-human-interface.md) | Draft |
| CI: GitHub Action & PR reporting | [rfcs/0010-ci-github-action.md](rfcs/0010-ci-github-action.md) | Draft |
| Workspaces & monorepos | [rfcs/0011-workspaces-and-monorepos.md](rfcs/0011-workspaces-and-monorepos.md) | Draft |
| Core contracts (traits) | [contracts/core-traits.md](contracts/core-traits.md) | Draft |
| Output schema (JSON) | [contracts/output-schema.md](contracts/output-schema.md) | Draft |
| Roadmap | [ROADMAP.md](ROADMAP.md) | Draft |

### ADRs (Architecture Decision Records)

| # | Decision | Status |
|---|----------|--------|
| [0001](adrs/0001-rust-for-the-core.md) | Rust for the core | Accepted |
| [0002](adrs/0002-tree-sitter-parsing.md) | tree-sitter as the universal parsing layer | Proposed |
| [0003](adrs/0003-adapter-linking-strategy.md) | First-party adapters compiled in; third-party plugins via WASM | Proposed |
| [0004](adrs/0004-cache-format.md) | Cache: content-addressed binary snapshot in `.kondo/` | Proposed |
| [0005](adrs/0005-coverage-ingestion.md) | Coverage is ingested, never measured, for CRAP | Proposed |
| [0006](adrs/0006-single-binary-zero-config.md) | Single static binary, zero-config by default | Proposed |

## Conventions

- **RFCs** describe *systems* — they are living documents, updated as designs evolve.
- **ADRs** record *decisions* — immutable once `Accepted`; superseded by new ADRs, never edited.
- **Contracts** are the normative interfaces (Rust traits, JSON schemas). Code must match them;
  changing a contract requires updating the document in the same PR.
- Status values: `Draft` → `Review` → `Accepted` (RFCs), `Proposed` → `Accepted` / `Superseded` (ADRs).
- Documents are written in English (lingua franca for OSS and agent consumption); discussion can
  happen in any language.

## Open questions (tracked for debate)

1. Name collision: an OSS tool named `kondo` (tidies build artifacts) already exists on crates.io —
   keep the name for the binary or pick a distinct crate name?
2. Candidate-rule triage is done (RFC 0005 §13); still open: `redundant-export-binding` and
   `private-type-leak` — yes/no each.
3. Default severity for `test-only code` findings — warn or info?
4. Navigation verbs: flat (`kondo uses`) vs namespaced (`kondo graph uses`) — RFC 0007 §8.
