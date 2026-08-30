
## 2026-08-30 — Coverage is an extension; core is mechanism (owner direction)

Coverage the FEATURE never belonged to the engine — core exposes the tools for
an ingester to exist and judges what any of them delivers. The cut that makes
the dependency arrows say so:

- `Coverage`, `FileCoverage`, `assemble` and `line_starts` moved into
  `kndo-core`'s own `coverage` module: the format-blind mapping half (records →
  line tables → span answers) is engine semantics, and `line_starts` was
  already shared with suppression. Core's dependency on the coverage crate is
  GONE — the engine now depends on the contract alone, and no format name
  appears anywhere in it.
- `kndo-coverage` is now THE lcov extension and only that: the parser (format
  knowledge) plus the built-in `LcovPlugin` behind the same `Extension` trait,
  absorbed from `kndo-plugin-coverage`, which is deleted (it had shrunk to a
  35-line shell after the unification). One crate, contract-only, compiled
  natively as the built-in and to wasm32 by the reference guest — the "one
  parser, never drift by prose" promise intact and now covering the spec too.
- `parse_lcov` (the one-step convenience) died with its last callers — the
  engine-driven split IS the only path now, so the old split-equals-one-step
  equivalence test dissolved into each half's own tests.
- The facade re-export of `Coverage`/`FileCoverage` is dropped: no frontend
  ever consumed it, and findings — not raw coverage — are the frontier.

The boundary this deliberately does NOT move: the `untested` JUDGMENT stays a
first-party core analysis (gate-eligible, abstains without evidence).
Extensions contribute evidence; core judges — that is the containment model,
and moving the judgment out would demote `untested` to an advisory `ext:`
category. Verified as pure code motion: full suite green, zero drift in
fixtures, corpus reports and schema.
