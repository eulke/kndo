# ADR 0006 — Single static binary, zero-config by default

**Status:** Proposed · **Date:** 2026-08-18

## Context
Target users include pre-commit hooks, CI containers, and AI agents — environments where every
installation step, runtime dependency, or required config file halves adoption. "Fácil de
entender y de utilizar" is a product requirement, not a nicety.

## Decision
- One statically-linked binary per platform; no runtime dependencies; installable via
  cargo, homebrew, an npm shim, and curl script. The binary embeds all first-party adapters,
  plugins, and grammars.
- `kndo check` must produce a correct, useful report in any repo with **zero configuration**:
  languages auto-detected by adapters' claims, ecosystems auto-detected by plugin predicates,
  ignores inherited from `.gitignore`. `kndo.toml` only ever *tunes*.
- Defaults are part of the contract: changing a default (weights, thresholds, severities) is a
  breaking change for scores/exit codes and follows semver like the JSON schema.

## Consequences
- Binary size grows with each embedded grammar (~roughly 1–3 MB each) — accepted; size is not a
  product constraint, startup time is.
- Auto-detection must be introspectable or it becomes magic: `kndo doctor` (RFC 0006 §2) is the
  mandatory companion, showing what activated and why.
- Feature-gated builds (`--no-default-features` + per-adapter features) remain possible for
  embedders who want a smaller binary.

## Alternatives
Plugin-download-on-demand model (à la ESLint) — rejected: network in pre-commit/CI, supply-chain
surface, cold-start unpredictability.
