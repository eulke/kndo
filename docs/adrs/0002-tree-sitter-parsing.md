# ADR 0002 — tree-sitter as the universal parsing layer

**Status:** Accepted · **Date:** 2026-08-18

## Context
Eight languages at launch, "adding a language must be easy", parsing must tolerate broken code
(pre-commit runs on work-in-progress), and extraction is on the hot path.

## Decision
All first-party adapters parse with **tree-sitter** grammars. The adapter contract does not
mandate tree-sitter (an adapter only owes `FileFacts`), but the shared adapter toolkit
(`kndo-adapter-toolkit`: query helpers, token normalization, complexity walker) is built around
it, making tree-sitter the paved road.

## Consequences
- Uniform mental model: an adapter is largely grammar + a set of tree-sitter queries + resolver
  logic. Error-tolerant parsing comes for free; grammars exist for all launch languages.
- Precision ceiling: tree-sitter is syntactic. Semantic facts (type-directed dispatch, implicit
  imports) are approximated → this is why edges carry `Confidence` (RFC 0002 §5) instead of
  pretending precision. A post-1.0 "deep mode" may layer compiler-grade resolvers per language.
- Grammar versions pin into each adapter's `facts_schema_version`, keeping cache invalidation
  honest when grammars update.

## Alternatives
Per-language native parsers (SWC, syn, go/parser…): faster and more precise individually, but
N toolchains, N AST shapes, and no shared query layer — the "easy new language" goal dies.
Chosen escape hatch: the contract permits a native-parser adapter where it matters, without
changing the core.
