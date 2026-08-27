# Findings reference

A finding's `category` is a pure **verdict** — it means the same thing whatever it lands on.
What it landed on travels separately in `subject_kind` (`function`, `class`, `file`,
`directory`, `dependency`, `enum-member`, …), and every verdict belongs to one fixed
**group** that drives report ordering:

| Group | Verdicts | Nature |
|---|---|---|
| `defect` | `undeclared`, `version-skew`, `private-type-leak`, `unresolved` | something is broken or lying |
| `waste` | `unused`, `test-only`, `duplicate`, `internal-only` | something can be removed or narrowed |
| `risk` | `crap`, `cyclic`, `untested`, `deep-import` | dangerous to change |
| `hygiene` | `stale` | kndo's own bookkeeping |

Two axes ride on every finding:

- **Severity** (`error` / `warning` / `info`) — how urgent. Drives `--fail-on`.
- **Confidence** (`certain` / `probable` / `possible`) — how sure the evidence is.
  Reachability follows the weakest link: anything alive only through a wildcard or dynamic
  construct is *live-possible*, never *dead-possible* — **dead is always certain**.

When a verdict holds uniformly, kndo reports the **widest node once**: a fully dead file is
one `unused` file finding, not forty symbol findings; a directory whose every file carries
the same verdict is one finding; a dead workspace package is one finding. A single file that
doesn't share the verdict — including one that is merely out of scope (vendored, generated,
unclaimed) — blocks the rollup for all of its ancestors, so "delete this folder" is only ever
claimed when it is actually safe.

The accusing verdicts are held to a **zero-false-positive** bar: when static analysis
genuinely cannot know (reflection with computed names, code launched as a subprocess, build
configurations that hide files from a single parse), kndo degrades toward silence or lowers
confidence — never toward accusation. Each section below states its own limits.

Plugin-contributed findings (`plugin:<coordinate>/<rule>`, group `convention`) are
third-party verdicts **outside** that statement: advisory by default, they never affect exit
codes unless you opt them in — see [Plugins](plugins.md#plugin-findings).

---

## unused

**Group** waste · **severity** warning · **confidence** always `certain` ·
**subjects** symbols, files, directories, dependencies.

Unreachable from every root: no production, test, or tooling entry point reaches it through
any import, reference, or plugin-contributed edge.

```text
◦ unused src/billing/tax.ts:41  calcLegacyTax() is unreachable from any production or test root
◦ unused (dependency) package.json  date-fns is declared but never imported
◦ unused (directory) src/legacy/  every file under src/legacy/ is unreachable (12 files)
```

**Fix:** delete it. For a dependency, remove the manifest entry. For a directory finding,
the whole subtree is safe to remove — the rollup only fires when *every* file under it
(recursively) is unused and nothing out-of-scope hides there.

**What keeps things out of `unused`:**

- Reachability from any root, at any confidence tier — a symbol alive only through a dynamic
  or wildcard construct is *live-possible*, not dead.
- Library mode: in a publishable package, your exported public surface is itself a root —
  public API is never "unused" just because nothing in-repo calls it.
- Framework conventions contributed by plugins (routes, convention files, serialization
  hooks) — see [Plugins](plugins.md).
- A symbol referenced only from already-dead code is still `unused` (a dead function's calls
  keep nothing alive) — transitive death is reported as ordinary `unused` findings.
- Symbols in a file that is itself unreachable are *not* reported separately — the file-level
  finding replaces the per-symbol ones it summarizes.

**Dependencies** get a dedicated classification per declaring package:

| Situation | Verdict |
|---|---|
| declared, nothing imports it | `unused` |
| `prod`/`optional` scope, imported only from test code | `test-only` (move to dev-dependencies) |
| `peer` scope | exempt — a peer dependency is a contract with the consumer, not a usage claim |
| `dev`/`build` scope | only ever `unused` (being test/tooling-only is what those scopes are *for*) |
| `optional` scope | same verdicts as `prod`, at `possible` confidence (runtime-conditional by design) |

A tool invoked from manifest scripts (`"test": "xo && ava"`) counts as used — nothing
`import`s a CLI binary, so script invocations are folded in as tooling usage rather than
flagging every CLI dev-dependency.

**Limits of the analysis:**

- Generated and vendored files are exempt (they're not yours to delete); files no adapter
  claims (unknown extensions) can be part of the graph via other adapters' references but
  never accused on their own.
- A file kept alive **only** because something links it as an asset — a template's
  `<link href>`, a framework config naming it by path — is *served*, not *used*: the evidence
  says its bytes ship and names none of its symbols, so its symbols are not judged one by one.
  The file itself is judged normally, and any other evidence at all (an import, an invocation,
  a root, a reference to one of its symbols) puts it back in ordinary jurisdiction.
- Code invoked **out-of-band** — a binary run as a subprocess by tests or scripts with
  dynamically constructed paths, an entry point referenced only from infrastructure kndo
  doesn't read — has no edge for kndo to see. Built-in conventions (manifest `scripts`,
  framework plugins) cover the common cases; for the rest, use a
  [suppression](suppressions.md) or a [plugin](plugin-authoring.md) that contributes the root.
- Reflection with computed names (`Class.forName(prefix + name)`) cannot be resolved
  statically. Known reflective *contracts* are modeled explicitly per language (test
  discovery, serialization hooks, dispatch through implemented interfaces); arbitrary
  computed lookups are not, and code reachable only that way needs a suppression or plugin
  annotation.
- For Java and Kotlin, **dependency** usage is not resolvable statically (an import's package
  has no reliable mapping to its build coordinate without resolving the classpath, which kndo
  never does) — dependency findings are skipped for those languages with one diagnostic,
  rather than flooding false `unused` claims. Symbol/file findings are unaffected.

## test-only

**Group** waste · **severity** info · **confidence** inherited from the evidence path ·
**subjects** symbols, files, directories, dependencies.

Reachable — but only from test roots. You built it, tests enshrined it, production never
came. It ships (or its dependency installs) with no production consumer.

```text
◦ test-only src/util/csv.ts:8  exportCsv() is reachable only from test roots
◦ test-only (dependency) package.json  supertest is prod-scoped but only test code imports it
```

**Fix:** either delete the code together with its tests, or — if it's genuinely a test
utility — move it to a test/fixtures area. For a dependency, move it to the dev scope.

Details and limits:

- Test files themselves are exempt (a test being test-only is what a test is), as are
  generated/vendored files.
- Confidence is the reachability evidence's own: reachable only through a `probable` edge
  from a test root means `test-only (probable)`. Directory rollups take the weakest
  confidence among the grouped files.
- A `prod`-scoped dependency imported only under in-file test regions (such as Rust
  `#[cfg(test)]` modules) is classified exactly as if the imports lived in test files.
- The same out-of-band limits as `unused` apply: if production reaches the code through a
  channel kndo cannot see, the finding is a false alarm to suppress or teach via a plugin.

## untested

**Group** risk · **severity** info · **confidence** from the reachability evidence ·
**subjects** symbols, files, directories.

Production-reachable code that **no test reaches, even transitively** — a static test blind
spot. This needs no coverage report: it is computed from the same reachability passes as
`test-only`, answering the inverse question ("reached by production roots, unreached by every
test root").

```text
▲ untested src/payments/refund.ts:12  refundOrder() is production-reachable but no test reaches it
```

**Fix:** add a test that exercises it — the finding resolves the moment any test imports it,
directly or transitively.

Details and limits:

- Active **only when the project has test roots at all**: a repository without tests gets one
  diagnostic, not a thousand findings.
- "A test reaches it" is import/reference reachability, not execution: a test that reaches a
  module transitively silences `untested` for everything it reaches, even if assertions never
  touch it. For *executed*-line truth, ingest a coverage report and watch
  [`crap`](#crap) instead — the two are deliberately complementary.
- Tests that exercise code **without importing it** — driving a compiled binary as a
  subprocess, hitting a running server over the network — leave no static edge; such code
  reads as untested to this analysis even when your coverage report says otherwise. Coverage
  ingestion (see [Health & coverage](health.md)) is the accurate signal there.

## internal-only

**Group** waste · **severity** info · **confidence** `certain` (demoted to `possible` when
only dynamic/wildcard evidence points wider) · **subjects** symbols.

Declared visibility wider than any real usage requires. kndo computes the **tightest
sufficient visibility** — the lowest rung of the language's own visibility ladder that still
covers every incoming reference — and if the declaration sits above it, the remediation names
the language's own narrower keyword.

```text
◦ internal-only src/graph/node.rs:88  NodeArena::compact is pub but only used within its crate — pub(crate) suffices
```

**Fix:** narrow the declaration to the suggested level (`pub(crate)`, `private`, `internal`,
`fileprivate`, … — whatever your language calls that rung).

Details and limits:

- Exemptions: symbols that are roots themselves (a library's public API, exported test
  fixtures, tooling-config exports) and symbols a plugin marked **externally consumed** (FFI,
  serialization, a public SDK surface) — consumed-by-definition is never accused.
- A symbol with zero references, or one that is outright unreachable, is `unused`'s verdict —
  kndo never tells you to narrow the visibility of code it's telling you to delete.
- Two visibility rungs that mean the same scope never accuse each other (Java's `protected`
  vs `public` both mean "visible outside the package" to a static analyzer — no evidence
  could distinguish them).
- Languages with no visibility ladder (JSON, CSS; external WebAssembly adapters that don't
  declare one) are skipped entirely — silence over a guessed ladder.

## private-type-leak

**Group** defect · **severity** warning in library packages, info in app packages ·
**confidence** `certain` evidence only · **subjects** callable symbols.

The inverse of `internal-only`: a public/exported symbol whose **signature** references a
type of *lower* visibility — the API promises a type its consumers cannot name.

```text
✗ private-type-leak src/api/mod.rs:14  pub fn open() returns crate-private struct Conn — consumers cannot name it
```

**Fix:** export the type, or narrow the API to match.

Details and limits:

- Only the signature is a promise: a private type used *inside* a public function's body is
  ordinary encapsulation and never flagged.
- Fires only on callables — a struct/class whose *field* leaks a private type is not accused,
  because visibility is declared per member and accusing the whole type body would produce
  false positives on exported-type/unexported-field designs.
- The effective surface is computed through ownership: an exported-looking method on an
  unexported class is not public API.
- Only `certain`-confidence type-use evidence accuses (fallback name-matching routinely picks
  the wrong same-named type); test code is exempt; cross-language pairs are skipped
  (visibility rungs only mean anything within one language's ladder).
- Symbols a plugin marked externally consumed are exempt, same as `internal-only`.

## deep-import

**Group** risk · **severity** warning · **confidence** the strongest underlying edge's ·
**subject** package (one finding per consumer-package → provider-package pair).

An import that bypasses a package's **declared** entry-point surface (`exports` map or
equivalent) and reaches into its internal file layout.

```text
▲ deep-import (package) packages/web  imports 3 internal files of @acme/core, bypassing its exports surface
```

**Fix:** the finding computes it for you, per touched file:

- if what you're importing is also reachable through the provider's public surface — switch
  the import specifier to the public path;
- if it's genuinely internal — add the subpath to the provider's surface, or extract it.

Details and limits:

- **Contract-gated and zero-config**: fires only when the provider *declares* a surface. No
  declared surface, no declared boundary, no finding — monorepos where sibling deep imports
  are accepted practice see no noise, and open-subpath packages are never accused.
- Sites are listed in the message (capped, with an elision count); the pair — not each import
  line — is the unit a migration is planned in.
- Go never produces this finding: the compiler already forbids importing another module's
  `internal/`, and kndo doesn't duplicate a boundary the build enforces unconditionally.
- External dependencies (`node_modules/…/dist/internal/x`) are not evaluated: their manifests
  live outside the discovered tree, so whether they declare a surface is unknowable and the
  gate stays closed — the safe direction.

## undeclared

**Group** defect · **severity** warning · **subjects** imports.

Code imports a package that the importing file's **own package** never declares — a phantom
dependency that only resolves today because hoisting or transitive resolution happens to make
it reachable, and breaks on a clean install elsewhere.

```text
✗ undeclared src/api/client.ts:3  lodash is imported but not declared by package @acme/web
```

**Fix:** declare the dependency in the importing package's manifest (or delete the import).

Details: attribution is per owning package (nearest manifest ancestor) — in a monorepo, a
dependency declared by sibling package B does not excuse package A using it. In a
single-manifest project this collapses to a simple global check.

## version-skew

**Group** defect · **severity** warning · **subjects** dependencies.

The same external dependency declared with **diverging version requirements** across the
workspace's manifests — an inconsistency someone will debug eventually.

```text
✗ version-skew (dependency) lodash  declared as ^4.17.21 (packages/web), 4.16.0 (packages/api), ^3.10.1 (tools/scripts)
```

**Fix:** align the requirements (or centralize them, if your build tool supports catalog-style
version management).

Details: a pure manifest-to-manifest comparison — no import edges involved, so it is fully
precise for every language, including those where dependency *usage* can't be resolved.

## cyclic

**Group** risk · **severity** per language (below) · **confidence** the weakest edge on the
reported path · **subjects** files, packages.

Dependency cycles: strongly-connected components of two or more nodes in the file-import
graph, and in the package graph where manifests define real packages. **One finding per
cycle** — anchored at the cycle's most-referenced node, with a shortest cycle path attached
as the evidence chain — never one finding per participant.

```text
▲ cyclic src/state/store.ts:1  4 files form an import cycle
   └ evidence: store.ts → actions.ts → selectors.ts → store.ts
```

**Fix:** break the loop — extract the shared piece both sides need, or invert one of the
dependencies. The evidence path shows the shortest loop to cut.

Cycle tolerance is a **language fact**, not a global opinion:

| Language stance | Severity |
|---|---|
| hazardous (JavaScript/TypeScript — module-initialization-order bugs are real) | warning |
| idiomatic (mutual imports within one compilation unit, where the language tolerates them) | skipped: legal, routine structure — reporting it would dress information up as a defect |
| impossible (Go — the compiler rejects import cycles) | skipped: a "cycle" there could only be a resolution artifact |

A mixed-language cycle is reported (warning) iff any participant's language calls it
hazardous — the hazard is real for that language; with no hazard participant it emits
nothing. A file cycle
that spans real packages is reported once at package level (the file finding would be
redundant); one confined to a single package stays file-level. A cycle whose every
participant is generated/vendored is skipped — nobody authored it.

## duplicate

**Group** waste · **severity** info (duplication is sometimes deliberate) ·
**subjects** files, functions.

Two halves:

- **Exact file duplicates** — byte-identical files, detected from content hashes. This is the
  one analysis that also covers files no language adapter claims (images, configs, assets):
  exactly the files token-based clone detection can never see. One finding groups *every*
  copy of one content. Empty files are exempt (`.gitkeep` armies are not a finding).
- **Structural clones** — callable bodies that are identical up to formatting, comments, and
  renamed identifiers/literals, detected over normalized token fingerprints and grouped
  transitively by similarity. Same-language only, one finding per clone group with every
  instance listed as evidence. Very small functions (under the token floor, 50 by default)
  don't participate — a three-line getter matching another three-line getter is not a copy.

A callable whose body is **only a value construction** — one struct/object literal, one
constructor call — does not participate. Such bodies match each other by definition of the
type they build, not by evidence of copying: the comparison deliberately ignores identifiers
and literals, and for a construction those *are* the authored content, leaving only the field
list the type dictates. The more central the type, the louder the false group. A function that
constructs *and* does something else is ordinary code and still participates.

A **closure is measured in its own right**, not folded into the function that contains it, so
five call sites passing the same callback are reported on the callback — where the duplication
actually lives — rather than on five otherwise-different functions. The same token floor
decides this: a closure too small to carry clone evidence stays part of its owner's body.

```text
◦ duplicate src/utils/retry.ts:10  parseRetryAfter duplicates 2 other functions (structural clone group)
   └ instance: src/http/backoff.ts:22
   └ instance: src/queue/redelivery.ts:31
```

**Fix:** extract one implementation and delete the copies — the evidence list is the work
list.

Details: generated/vendored files are exempt from the structural half (a generator copying
itself is its own business), and so is test code — test files and `#[cfg(test)]`-style
regions inside production files; parallel arrange-act-assert bodies across a fixture matrix
are the point of table-shaped tests, not waste. Neither exemption touches the exact half — a
byte-identical vendored blob duplicated five times is still five copies of the same bytes.

## crap

**Group** risk · **severity** warning · **subjects** callables.

Change Risk Anti-Patterns: per function,

```text
CRAP(m) = comp(m)² × (1 − cov(m))³ + comp(m)
```

where `comp` is cyclomatic complexity and `cov` is the covered fraction of the function's
instrumented lines from your **ingested** coverage reports. Findings fire above the score
threshold of 30.

A substantial closure is scored **as its own callable**: its complexity is its own and no
longer its owner's, its coverage is read from its own lines, and the finding points at the
closure. Such a finding names the enclosing function and the closure's position within it
(`app.ts#configure (nested callable #2)`) — the position, not a line number, so acknowledging
it in a baseline survives edits above it.

```text
▲ crap src/parser/expr.ts:120  parseExpression: complexity 24, coverage none — score 599.0, above the threshold of 30
```

**Fix:** two levers, by construction of the formula — cover it (coverage crushes the score
cubically) or simplify it (complexity grows it quadratically). Covering is usually the
cheaper first move.

Details and limits:

- Coverage is **ingested, never measured**: kndo never runs your tests. Drop an `lcov.info`
  at a well-known path and it's picked up automatically — see
  [Health & coverage](health.md#coverage-ingestion) for paths, freshness rules, and how
  line-level data maps onto functions.
- No coverage report ingested at all ⇒ the whole analysis is skipped, with one diagnostic
  saying so — without a report the coverage factor would be a guess for every function at
  once, not a measurement.
- A report that doesn't instrument a particular function ⇒ that function's score is computed
  pessimistically with `cov = 0` and the message says `coverage: none` — the functions most
  likely to be the problem are never silently skipped.
- Test code is exempt (a test's own coverage is meaningless), as are generated/vendored
  files (not yours to refactor).

## stale

**Group** hygiene · **severity** info · **confidence** `certain` ·
**subject** suppression.

A `kndo:allow` pragma that isn't doing its job, located at the pragma itself, with the most
specific of four verdicts:

1. it names a category that doesn't exist (with a did-you-mean hint);
2. it targets `stale` itself — stale findings are not inline-suppressible, acknowledge them
   via the baseline instead;
3. it attaches to no declaration — nothing starts on its line or the line after;
4. it bound correctly but matched zero findings this run — the issue it acknowledged is gone.

**Fix:** delete (or re-aim) the pragma. Because staleness is judged against the complete
pre-suppression finding set, an actively-suppressing pragma can never be stale, and deleting
a stale pragma can never resurrect a finding.

See [Suppressions & baseline](suppressions.md) for the pragma syntax and binding rules.

## unresolved

Registered in the category vocabulary (you may name it in suppressions and consumers must
accept it in the schema) for imports that resolve to nothing — but **no current analysis
emits it**: an import that fails to resolve today simply produces no edge. The name is
reserved so its arrival is an additive change.

---

## The category registry

The complete, closed set of core categories:
`unused`, `test-only`, `untested`, `undeclared`, `unresolved`, `version-skew`, `duplicate`,
`internal-only`, `private-type-leak`, `cyclic`, `deep-import`, `crap`, `stale`.
New categories are additive (minor schema bump); consumers must ignore unknown ones.
The `plugin:` prefix and the `convention` group are reserved for
[plugin findings](plugins.md#plugin-findings) — no core category may claim either.
