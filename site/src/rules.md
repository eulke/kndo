# The rules

A finding's `category` is a pure **verdict** — it means the same thing whatever it lands on.
What it landed on travels separately in `subject_kind` (`function`, `file`, `dependency`,
`enum-member`, …), and every verdict belongs to one fixed **group** that drives report
ordering: `defect` → `waste` → `risk` → `hygiene`.

When a verdict holds uniformly, kndo reports the **widest node once** — a fully dead file is
one `unused:file` finding, not forty symbol findings; a dead directory is one finding; a dead
workspace package is one finding.

## defect — something is broken or lying

| Verdict | Meaning |
|---|---|
| `unresolved` | an import resolves to nothing (`./transpor` — a typo'd path) |
| `undeclared` | code imports a package the manifest never declares (a phantom dependency riding on hoisting/transitivity) |
| `version-skew` | one dependency declared at diverging version requirements across workspace manifests |
| `private-type-leak` | a public signature references a type its consumers cannot name — export the type, or narrow the API |

## waste — something can be removed or narrowed

| Verdict | Meaning |
|---|---|
| `unused` | unreachable from every entry point: dependencies nothing imports, symbols nothing references, files nothing reaches |
| `test-only` | "production" code only ever reached from tests — it ships, but only tests need it |
| `duplicate` | structurally identical code blocks (identifiers/literals aside), and byte-identical files |
| `internal-only` | declared visibility wider than any real usage requires — `pub` that could be `pub(crate)`, `public` that could be `private`; the remediation names the language's own tighter level |

## risk — dangerous to change

| Verdict | Meaning |
|---|---|
| `crap` | Change Risk Anti-Pattern: complex **and** untested (`comp² × (1 − cov)³ + comp`); coverage is ingested from your reports when present, assumed absent otherwise |
| `cyclic` | import cycles, judged per language (a hazard in JS, impossible-by-compiler in Go, idiomatic-tolerated where the ecosystem says so) |
| `untested` | production-reachable code no test reaches, even transitively — static test blind spots, no coverage report needed |
| `deep-import` | an import bypassing a package's declared surface (`exports` map et al) into its internals |

## hygiene — kndo's own bookkeeping

| Verdict | Meaning |
|---|---|
| `stale` | a suppression that no longer suppresses anything: unknown category, attached to no declaration, or its acknowledged issue is gone — delete the pragma |

## Confidence, and the zero-false-positive stance

Every finding carries `certain`, `probable`, or `possible` confidence. Reachability follows
the weakest link: anything alive only through a wildcard or dynamic construct is
*live-possible*, never *dead-possible* — **dead is always certain**. The accusing verdicts
are held to a zero-false-positive bar, enforced by a recurring hunt across real open-source
repos; when static analysis genuinely cannot know (reflection, external protocol machinery,
macro-generated call sites), kndo degrades toward silence, never toward accusation.

Plugin-contributed findings (`plugin:<coordinate>/<rule>`, group `convention`) are third-party
verdicts **outside** that statement: advisory by default, they never affect exit codes unless
you opt them into the gate — see [Plugins](plugins.md).
