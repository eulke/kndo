# v1 fixture harvest — manifest

The conformance fixtures encode v1's entire false-positive hunt (corpus run 7,029→3,229
findings in one pass; the 24-repo audit's ~30 semantics fixes; guava's parent-pom
incident). They are the most expensive asset to recreate and the v2 vendors them as its
regression floor: each v2 adapter starts by replaying its language's fixture corpus
through the new `EvidenceSink` and must reproduce the expected findings.

Inventory at v1 commit `98fe860`:

| Adapter | expected.json | test files | Notes |
|---|---|---|---|
| kndo-adapter-rust | 25 | 127 | Largest corpus; includes cfg-alternated impls, `#[path]` mods |
| kndo-adapter-js | 22 | 107 | ESM + CJS worlds; package.json workspaces/exports fixtures |
| kndo-adapter-go | 8 | 39 | unit/unit_parent package model |
| kndo-adapter-java | 6 | 30 | Maven parent-pom inheritance (the guava class) |
| kndo-adapter-kotlin | 5 | 23 | Multi-line-body constraint (upstream grammar bug, documented) |
| kndo-adapter-css | 4 | 25 | Pairs with JS importer fixtures |
| kndo-adapter-swift | 4 | 24 | Restored corpus (was lost once to a stash mishap) |
| kndo-adapter-json | 2 | 12 | Manifests-are-not-claimed pair property |
| kndo-adapter-html | 0 | 0 | Documented gap in v1 — v2's HTML adapter must create its corpus |
| **Total** | **76** | **387** | |

Also harvested by `harvest-fixtures.sh`:
- `crates/kndo-adapter-*/src/stdlib.txt` — generated stdlib data (kndo-stdlib v1 format).
- `crates/kndo-plugin-api/tests/compat/` — the pinned ABI reference components.
- `examples/` — the five reference WASM guests (adapter, hooks, coverage, two
  dependency-implication wrappers).

Run `./harvest-fixtures.sh <v1-checkout> <output-dir>` to export everything into a
layout the v2 vendors under `testkit/corpus/<language>/`.
