# ADR 0007 — Product name: `kndo`

**Status:** Accepted · **Date:** 2026-08-18

## Context
The working name `kondo` collides with an existing OSS tool on crates.io (a project-artifact
cleaner) — thematically adjacent, guaranteeing permanent installation confusion (`cargo install
kondo` would fetch the other tool), and likely a Homebrew clash. The product needs one name that
works everywhere: crate, binary, config file, cache dir, pragma, env vars, CI action.

## Decision
The product is **`kndo`** — used uniformly for everything: crates (`kndo`, `kndo-core`,
`kndo-cli`, `kndo-adapter-*`, `kndo-plugin-api`), the binary, `kndo.toml`, `.kndo/`,
`kndo:allow` pragmas, `KNDO_*` env vars, `kndo-action`. Availability verified 2026-08-18:

| Registry | `kndo` | Notes |
|----------|--------|-------|
| crates.io | ✅ free | `kndo-core`, `kndo-cli` also free — the registry that matters most |
| npm | ❌ taken (unrelated DeFi package) | shim publishes as **`kndo-cli`**, installing a binary named `kndo` (npm allows bin ≠ package name) |
| Homebrew | no formula found | first-formula-wins; unverifiable at decision time (API unreachable), risk low for a coined word |

A happy accident: the finding-id prefix in the output schema was already `kndo-` — finding ids
(`kndo-a3f81c92e5d4`) now carry the product name natively.

## Consequences
- The GitHub repository should be renamed `eulke/kondo` → `eulke/kndo` (redirects preserved by
  GitHub); docs already use `kndo` throughout.
- The npm bare name is the one asymmetry (`npm i -g kndo-cli` vs `cargo install kndo`); install
  docs must state it prominently.
- Reserve the names early (empty placeholder crates + the npm package + the GitHub org if
  desired) — availability is only real once claimed.
