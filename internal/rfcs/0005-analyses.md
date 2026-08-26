# RFC 0005 — Analyses & Metrics

**Status:** Accepted · **Depends on:** RFC 0001, 0002, 0004

All analyses are pure functions over the Project Graph (+ optional enrichments such as coverage).
Each finding carries: stable id, category, severity, confidence, location(s), evidence, and a
remediation hint (schema in [contracts/output-schema.md](../contracts/output-schema.md)).

**Taxonomy rule — a category is a verdict; the subject is a facet; the reporting level is a
rollup.** Three orthogonal things, kept orthogonal:

1. **Category = verdict, nothing else**: `unused`, `test-only`, `untested`, `undeclared`,
   `unresolved`, `version-skew`, `duplicate`, `internal-only`, `private-type-leak`, `cyclic`,
   `deep-import`, `crap`, `stale` (the normative registry lives in
   [output-schema.md §6](../contracts/output-schema.md)). A verdict means the same thing
   whatever it lands on — there is no `unused-file` vs `unused-code`: both are `unused`.
2. **Subject = the `subject_kind` facet**: the kind of graph node the verdict landed on — `file`,
   `directory`, `package`, `dependency`, `import`, `suppression`, or any symbol kind (`function`, `type-alias`,
   `enum-member`, `css-rule`…). Configuration and suppressions target a bare category or
   `category:subject` (e.g. `unused:enum-member`, `test-only:dependency`); knip-style
   `unused-type` ≡ `unused:type-alias`.
3. **Reporting level = widest uniform node**: when a verdict holds for every symbol in a file
   *and* for the file node itself, kndo emits **one** finding on the file (subject `file`), not
   N symbol findings; when it holds for every file in a directory, one finding on the directory;
   when it holds for a whole workspace package, one finding on the package (RFC 0011 §6).
   Rollup is presentation of the same facts, not a different verdict — "test-only file" is the
   `test-only` verdict reported at file granularity.

4. **Group = the verdict's nature, fixed per category**: every verdict belongs to exactly one
   presentation group, declared here — never configurable, never guessed by renderers:

   | Group | Verdicts | Meaning for the reader |
   |-------|----------|------------------------|
   | `defect` | `unresolved`, `undeclared`, `version-skew`, `private-type-leak` | something is broken or lying — fix it |
   | `waste` | `unused`, `test-only`, `duplicate`, `internal-only` | something can be removed, consolidated, or narrowed |
   | `risk` | `crap`, `cyclic`, `untested`, `deep-import` | something is dangerous to change — refactor or test it |
   | `hygiene` | `stale` | kndo's own bookkeeping is outdated |

   Groups drive ordering and sectioning in every renderer (defects before waste before risk
   before hygiene — see RFC 0006 §3) and give consumers a stable coarse filter. A future verdict
   must declare its group on arrival (e.g. candidate `layer-violation` → `risk`,
   `redundant-export-binding` → `waste`); new groups are additive and rare.

Intentional absences are documented decisions, not oversights: there is no `tooling-only`
verdict (tooling reachability is healthy).

## 1. Reachability foundation

Most detections derive from one computation. Roots are partitioned by `RootKind`:

- **Production roots** — language-defined entry points (`main`, published/public API of a library,
  package `exports`/`bin`) + plugin-contributed roots (framework handlers, DI-registered beans…).
- **Test roots** — test functions/files (language role detection + test-framework plugins).
- **Tooling roots** — build/config scripts (webpack.config, build.gradle, migrations…): they keep
  their imports alive but are not production code themselves.

A declared root's *kind* is capped by its file's role: a `main`/manifest-bin entry point in a
Tooling-role file (an `xtask/` binary) is a Tooling root, in a Test-role file a Test root —
the adapter states the language fact ("this is an entry point"), the role decides who
consumes it. Plugin-contributed roots are exempt (targeted consumer knowledge outranks a
directory convention), and so are library-surface promotions ("the production API re-exports
this") — though no promotion chain ever *starts* from a capped file.

**Plugin-contributed edges are liveness evidence, never architecture evidence (RFC 0017
§5.4).** A plugin's `References`/`ReferencesFile` contributions feed reachability — where a
false positive can only *suppress* findings, the safe direction — and are ignored by `cyclic`
and by every analysis that would *create* a finding from an edge's existence. This is a
contract, not an implementation accident: it is what lets the platform accept
lower-precision, convention-derived edges from third-party plugins without ever risking the
zero-false-positive bar.

Two reachability passes (production-only roots; then all roots) assign every symbol/file a color:

| Color | Meaning |
|-------|---------|
| `production` | reachable from a production root |
| `test-only` | reachable only from test roots |
| `tooling-only` | reachable only from tooling roots |
| `unreachable` | reachable from nothing |

**Color and confidence resolution.** Two things are easy to leave ambiguous and both resolve
from one algorithm: what happens when a node is reachable from roots of *different kinds*, and
what confidence a node gets when reachability depends on *non-certain* edges.

For each `RootKind` κ and threshold τ ∈ {certain, probable, possible} — strength order
certain > probable > possible, so "edges at least as strong as τ" shrinks as τ strengthens —
define `R(κ, τ)`: the nodes reachable from κ-roots using only edges at least as strong as τ.
`R(κ, possible)` therefore uses every edge regardless of confidence and is the largest set;
`R(κ, certain)` uses only certain edges and is the smallest. Root edges carry their own
confidence too (RFC 0001 §3) and seed the traversal at that strength — a plugin-contributed
root nobody is fully sure about only participates from `probable` onward, like any other edge.

Wildcard edges are not a separate mechanism — they are folded into this same computation: a
`Wildcard { from }` edge expands into `possible`-confidence edges from that file to its
**plausible target set** (same-file symbols, symbols a plugin marked externally-consumed via
`annotate_symbols`, and whatever an adapter's `DynamicUse` reason narrows the scope to — a
partial string prefix narrows it, a bare `eval` does not). One mechanism, not two.

**Attribution and the module-load rule (RFC 0012 §4).** Reference edges are attributed to the
declared symbol they execute *inside* when the adapter supplies it (`RawReference::within` —
contracts §2), and to the file otherwise (module-level code, and adapters that don't emit the
field). Two rules govern the traversal:

- **Execution rule:** a symbol-attributed reference fires only when its symbol is reached — a
  dead function's calls keep nothing alive, so transitive death is visible.
- **Module-load rule:** reaching a symbol also reaches its owning file, at the same τ — using
  a symbol loads its module, so the file's load-time (`within: None`) references and its
  `ImportsFile` edges fire. This is also what makes symbol-only roots work (Go's `func
  main`/`init`/exported-declaration promotion, docs/adapters/go.md §0, §2 — no manifest-level
  entry file to root alongside them): `R(κ, τ)`'s traversal enqueues a reached symbol's owning
  file alongside the symbol.
- **Invoked-program rule:** an `InvokesFile` edge — a file executing another file **as a
  program**, the process boundary no import crosses (a test running its own workspace binary
  via `env!("CARGO_BIN_EXE_…")`, resolved through the manifest's named executable targets,
  `ManifestFacts::executables` × `FileFacts::invoked_executables`) — traverses to the target
  file AND to every Production `Root` target declared inside it, each at the weaker of the
  invocation's and the root's own confidence. Importing a module runs only its load-time
  code; *executing* a program runs its entry point, so the invoked binary's whole call tree
  inherits the invoker's colors. Non-Production roots inside the invoked file stay out
  (running the binary runs neither its inline tests nor its tooling entries). Like
  `ReferencesFile`, this is liveness evidence only — no finding-creating analysis reads it,
  so a false edge can only ever suppress findings.
- **Machinery-dispatch rule:** a member the adapter marked `implicitly_invoked`
  (contracts §2) is exercised by the language's own machinery whenever its OWNER is used —
  an operator (`==` → `eq`), a formatting hook (`{}` → `fmt`), a destructor (scope end →
  `drop`), a loop protocol (`for` → `next`). The call site never writes the method's name,
  which is exactly why no reference edge can exist for it. Reachability derives an implicit
  owner → member edge at `Probable` (using the type IS plausibly using the hook — degrade
  toward silence), the owner resolved by `member_of` in the member's own file, twins
  included; an unreached owner propagates nothing. WHICH traits/protocols qualify is each
  adapter's curated knowledge (Rust: the stdlib fmt hooks, operators, `Drop`, `Hash`,
  `Iterator`, `Future`, `FromStr`, `Error` — docs/adapters/rust.md §2); name-called trait
  methods (`.clone()`, `.into()`) stay out — the duck fallback already reaches those. This
  composes with (not replaces) the dispatch rule's `Probable` Production roots on
  trait-impl members: the root keeps a hook alive with zero owner usage, the implicit edge
  lets it *inherit the owner's colors* — which is what `untested` (§9) needs to stop
  calling a formatting hook a test-blind spot when its type is test-covered. The rule reads
  TWO sources of the same fact: the adapter's `Declaration::implicitly_invoked` (the
  language's own machinery) and a plugin's `mark_implicitly_invoked` annotation (a
  *framework's* machinery — serde calling `serialize`; `kndo:serde`, docs/plugins/serde.md
  — third-party dispatch the language adapter must never learn about). A plugin normally
  reaches that annotation through `AnnotationSink::mark_machinery_impls`, which matches its
  curated trait table against `SymbolNode::implements` — the trait whose implementation
  declares the member, an adapter-supplied fact the core carries and never interprets
  (core-traits.md). Same rule, same two sources; the plugin brings only the table.
- **Implement-dispatch rule:** calling through a trait IS plausibly executing every
  implementation — the vtable, as declared. For each `RefKind::Implement` edge
  (`impl Trait for T` emits one from the implementing type's symbol to the trait's), every
  member of the TRAIT fans out to the implementing type's same-named member in the impl's
  own file (the edge's owner — an impl block need not share its type's file), at
  `Probable`. Fully derived from facts already in the graph — no trait lists in the core; a
  trait member nothing reaches propagates nothing, an Implement edge whose source fell back
  to file attribution contributes nothing, and an out-of-repo trait (serde) never resolves
  an Implement edge at all (that gap is the `kndo:serde` plugin's, above). This is what
  turns one test exercising a `&dyn Flag` call site into test-reachability for every flag
  impl: the site resolves to the trait's member (the dyn receiver's *declared* type), and
  the fan-out inherits from there.

File attribution (the pre-RFC-0012 behavior, still what a `within`-less adapter gets) is the
deliberate over-approximation: everything a live file references stays alive. Every
degradation — unresolvable `within`, absent field — falls back to it, never the other way.

Every node's `(color, confidence)` comes from the first matching rule, in this fixed order:

1. `production` — if the node ∈ `R(production, possible)`; confidence = the strongest τ for
   which it's still in `R(production, τ)`.
2. `test-only` — same test, against `R(test, τ)`.
3. `tooling-only` — same test, against `R(tooling, τ)`.
4. `unreachable` — the node is in `R(κ, possible)` for **no** κ: zero evidence at *any*
   confidence tier, from *any* root kind, wildcard-expansion included.

Color precedence deliberately outranks confidence: a node *possibly* production must never be
offered up for deletion just because it is *certainly also* test-only — "maybe still used for
real" beats "definitely only used by tests" for what a reader should do next.

**Consequence: dead is always `certain`.** Rule 4 fires only when a node has no evidence at
*any* tier, so an `unused` finding's confidence is always `certain` — there is no "probably
dead". A node with any evidence, however weak, is colored **alive** (rules 1–3) at that weak
confidence instead: a symbol reachable only through a `possible` edge is production-*possible*,
never test-only-*probable* or dead-*probable*. `test-only`/`internal-only`/etc. findings, unlike
`unused`, do inherit sub-certain confidence — they fire on nodes that *are* reachable, just not
from the root kind that would make them safe.

*Example:* symbol `S` is called from `src/prod/x.ts` (a production root) through a duck-typed
dispatch (`probable`), and imported directly from `tests/y.test.ts` (`certain`). `S` is in
`R(production, probable)` but not `R(production, certain)`, and rule 1 fires before rule 2 is
even checked ⇒ color `production`, confidence `probable` — no `unused` finding, and
`kndo describe S` reports "production (probable), kept alive by `src/prod/x.ts:12`
(duck-typed call)".

**Library mode:** for library packages the public API is a production root by definition —
kndo will not call exported API "unused" just because the repo doesn't call it. Within an
unpublished application package, however, `export` is *not* a root; an exported-but-never-imported
symbol is still dead. Adapters/manifests decide which mode applies per package.

## 2. `unused` — unreachable code, files & dependencies

`unreachable` symbols. Severity: warning. Evidence: the symbol, why nothing reaches it, nearest
former consumer if known from the findings snapshot. Confidence downgrades if any wildcard edge
could plausibly target it (name exposed to reflection/serialization, plugin annotations, FFI).

**Member granularity.** The analysis descends into type members: methods, fields, and enum
members are symbols in their own right (`subject_kind` facets `method`, `field`, `enum-member`),
so "class member nothing calls" and "enum variant nothing references" are ordinary
`unused` findings. Dynamic dispatch is handled through the graph, not guessed around:
adapters emit implements/overrides references, so an interface/trait method implementation is
alive whenever the interface method is reached; where dispatch is not statically resolvable, the
member's liveness evidence is at best `probable` and findings demote accordingly.

## 3. `test-only` — non-productive code

Nodes colored `test-only`, excluding test-role files themselves and declared test utilities
(`testkit`/`fixtures` conventions, configurable). This is the "you built it, tests enshrined it,
production never came" detector — the finding explicitly lists the test roots that keep the node
alive, so deleting code + its tests together becomes mechanical.
Default severity: **info** (decided; revisit at M6 with dogfooding data before any raise to
warning — changing it is a defaults-contract change, ADR 0006).

## 4. File & directory subjects (rollup, not new categories)

`unused` and `test-only` apply to file nodes like any other node: a file with no incoming edge
and no root is `unused` (subject `file`) — this subsumes asset/config orphans via cross-language
edges (CSS, JSON); a production-role file whose every incoming edge comes from test roots is
`test-only` (subject `file`). Per the rollup rule (§ taxonomy), the file finding *replaces* the
per-symbol findings it summarizes, and a directory whose every file carries the same verdict
rolls up once more (subject `directory`) — "you can delete this whole folder" is one finding,
not fifty. Generated and vendored origins are exempt by default.

## 5. Dependency & import hygiene

For each `ManifestDependency` with scope `prod`, classify by its importers:

| Importers | Finding |
|-----------|---------|
| none | `unused` (subject `dependency`) — declared, never imported |
| only test-role / test-only-reachable files | `test-only` (subject `dependency`) — belongs in dev scope, not shipped weight |
| at least one production- or tooling-reachable file | used (no finding) |

The neutral scope taxonomy is `prod | dev | build | peer | optional` (adapters map ecosystem
scopes onto it — npm `peerDependencies`, Cargo `build-dependencies`, Gradle configurations):

- **dev / build** — checked against all files; unused only if *nothing* imports them.
- **peer** — a contract with the consumer, not a usage claim: exempt from `unused`
  (an un-imported peer is at most an info-level note), and never `test-only`.
- **optional** — runtime-conditional by design: findings demote to `possible` confidence, below
  the default report floor.

Adapter-provided package mappings handle subpath imports, type-only packages (`@types/*` bound to
their runtime package), and side-effect-only imports (`import "polyfill"` counts as usage).
Internal workspace dependencies get the same verdicts with boundary-aware remediation
(RFC 0011 §4).

Two further import-side findings:

- `undeclared` (subject `dependency`) — an import resolves to a package absent from the manifest
  (phantom deps via hoisting/transitivity). Severity: warning; error in `--strict`.
- `version-skew` (subject `dependency`) — the same external dependency declared with diverging
  version requirements across workspace packages (RFC 0011): three packages pinning three
  `lodash` versions is an inconsistency someone will debug eventually. Manifest-only detection,
  zero-config; evidence lists every declaring manifest with its requirement. Severity: warning.
- `deep-import` (subject `package`) — an import bypassing a provider's *declared* entry-point
  surface. Applies to workspace siblings **and external dependencies alike** — a plain
  single-package app importing `some-lib/dist/internal/x` gets the finding when `some-lib`
  declares an `exports` map. Contract-gated, pair-level rollup, computed remediation — full
  design in RFC 0011 §4.
- `unresolved` (subject `import`) — a relative/internal import specifier that resolves to no file
  (`Resolution::Unresolved` after all adapters decline): almost always a broken path or a missed
  rename. Failed *package* resolution surfaces as `undeclared` instead, never twice.
  Severity: error (it is a defect, not waste) — but confidence-gated: dynamic specifiers demote
  to `possible` and drop below the default report floor.

## 6. `duplicate` — structural clones

Token-based fingerprinting over adapter-normalized token streams (identifiers/literals
canonicalized ⇒ catches Type-1 and Type-2 clones; Type-3/semantic clones are out of scope for 1.0):

- Granularity: callable **shapes** ≥ `min-tokens` (default 50). A declaration contributes its
  own body plus one shape per callable nested inside it that clears the same floor
  (`MetricsSyntax::nested_callable_kinds` names the node kinds per language); a promoted
  shape's tokens leave the enclosing stream, which keeps one `FN` in their place. This is what
  makes N call sites passing the same callback report on the callback, where the duplication
  is, instead of on N otherwise-different callers. A nested callable *below* the floor stays
  part of its owner's body — promoting it would leave both halves under the floor and delete
  real findings (measured on the field corpus: 83 clone participants).
- Winnowing fingerprints into a global index; matches only within the same language.
- **A body that only constructs a value is not clone-eligible.** There the normalization
  inverts: a construction expression has no control flow, its structure IS the field list the
  type declaration dictates, and the only authored content is the field values — exactly what
  `ID`/`LIT` erases. Two constructions of one type therefore fingerprint alike by definition of
  the type, not by evidence of copying, and the false family grows with how *central* the type
  is. `MAX_POSTING` already concedes the same belief using popularity as the proxy; this names
  the cause. All-or-nothing (a function that constructs *and* does work has authored
  structure), not configurable (`min-tokens` is a floor on size, and this is not about size),
  and needing no carve-out for a construction carrying a callback — that callback is its own
  shape and is not exempt. `MetricsSyntax::construction_kinds` is empty for languages where
  construction is an ordinary call (Kotlin, Swift), which keeps today's behaviour there rather
  than having the adapter guess.
- Finding groups all instances, largest group first; evidence shows the shared shape.
- **Structural clones target production code**: test-role files and sub-file test regions
  (`FileFacts::test_spans`) are exempt, unconditionally — the same two-level exemption `crap`
  and `untested` apply. Parallel arrange-act-assert bodies across a fixture matrix are the
  *point* of table-shaped tests, not waste. The exact-file-duplicate half keeps its
  no-carve-out rule.

Severity: info by default (duplication is sometimes deliberate); the *metric* (duplication %)
always feeds health regardless of severity.

**Exact file duplicates.** Byte-identical files — same blake3 content hash, computed anyway for
the cache — are the same `duplicate` verdict with subject `file`: copy-pasted configs, images,
and any other asset that token-based clone detection cannot see (binaries included). One finding
groups all copies. Costing nothing beyond hashing, this lands in M1, ahead of structural clones.

## 7. Visibility mismatch: `internal-only` & `private-type-leak`

Both directions of one comparison — a symbol's **declared** visibility against what its usage
**requires** — using the language's visibility ladder (adapter-declared: private → file →
package/crate → public). *The ladder's concrete contract form (rungs as `(scope, label)` data
on the adapter descriptor), the generalized tightest-sufficient algorithm, and
`private-type-leak`'s implementation design (`RefKind` tagging + signature spans) are
specified in RFC 0012 §§5–6.*

**`internal-only`** (declared > required; group `waste`): the analysis computes the **tightest
sufficient visibility** — the lowest ladder level that still covers the origin of every incoming
reference. Declared above it ⇒ finding; the remediation names the concrete change in the
language's own terms (`private`, `pub(crate)`, unexported lowercase name), supplied by the
adapter.

**`private-type-leak`** (declared < required; group `defect`): a public/exported symbol whose
signature references a type of *lower* visibility — the API promises a type its consumers cannot
name. Derivable directly from `TypeUse` edges crossing visibility levels downward; no new
vocabulary. Severity: warning in library-mode packages (a lying public API), info in app
packages. Remediation offers both directions: export the type, or narrow the symbol.

Covers your whole ladder of cases uniformly: exported symbol referenced only within its own file
(`internal-only:function`), public member used only inside its own type (`internal-only:method` —
"should be private"), Rust `pub` used only in-crate ("should be `pub(crate)`").

Exemptions: library-mode public API (roots are externally consumed by definition), symbols
plausibly targeted by wildcard edges (demote to `possible`), and symbols plugins mark as
externally consumed via `annotate_symbols` (FFI, serialization, DI). Severity: info.

## 8. `cyclic` — dependency cycles

Strongly connected components (Tarjan) of size ≥ 2 in the file-import graph, and in the
package/module graph where manifests define units. **One finding per cycle**, not per
participant (rollup spirit): anchored at the cycle's most-referenced node, with a shortest
cycle path in `related` as the evidence chain. Incrementally, SCCs are recomputed only within
the dirty region's weakly connected component.

Cycle tolerance is a language fact, so adapters declare it per graph level and defaults stay
honest: severity `warning` where the ecosystem treats cycles as hazards (JS/TS file cycles —
init-order bugs); idiomatic levels (Rust modules within a crate) and impossible levels (Go
package cycles — the compiler already forbids them) alike emit nothing — an idiomatic cycle
is true information about legal structure, and information is never dressed up as a defect.
A mixed-language cycle reports iff any participant's language calls it a hazard.

## 9. `untested` — static test-blind spots

The exact inverse of `test-only`, computed from the same coloring passes at zero extra cost:
symbols **production-reachable but reachable from no test root whatsoever** — not "low
coverage" (dynamic, needs a report) but "no test even *imports* this, transitively". CRAP tells
you complex code is poorly covered *if* you feed it coverage; `untested` finds the blind spots
statically, zero-config, day one.

- Active only when the project has test roots at all — a repo without tests gets one diagnostic,
  not a thousand findings.
- Severity: info. Subject granularity and rollup as usual (an entire untested file or package
  rolls up). Confidence demotes through wildcard edges like every reachability verdict.
- Evidence: the production roots that reach the symbol (proof it matters) and the nearest tested
  neighbor (where a test could start).

## 10. `crap` — Change Risk Anti-Patterns

Per function/method, with `comp` = cyclomatic complexity (adapter-extracted) and `cov` = fraction
of the function's statements covered:

```
CRAP(m) = comp(m)² × (1 − cov(m))³ + comp(m)
```

- Coverage comes from ingested reports (plugins, ADR 0005). No report at all ⇒ the analysis
  is **skipped with one diagnostic** — the coverage factor would be a guess for every function
  at once, not a measurement, and a category-wide guess is noise, not risk (the same posture
  `untested` takes for a project with no test roots); the health score's crap axis contributes
  zero penalty with the absence reported explicitly. A report that doesn't instrument a
  particular function ⇒ **cov = 0**, flagged "coverage: none" — pessimistic per function, and
  the message says why.
- Threshold: findings for `CRAP > 30` (standard), configurable. Test code is exempt.
- Scored per callable **shape** (§6): a substantial closure carries its own complexity and its
  own coverage rather than its enclosing function's, and the finding points at the closure.
  Reporting nested shapes is not optional once they exist — a 40-branch closure inside a
  two-branch function scores 40 on the closure and 2 on the function, so skipping them would
  delete the risk from the report entirely. Identity: the closure's ordinal within its
  declaration, never its line, so a baseline survives edits above it.
- Output ranks the CRAP hotspot list — the refactor-next queue.

## 11. `health` — project health score

A 0–100 composite, deterministic and documented so trends are meaningful:

```
health = 100 − Σ category_penalty
category_penalty = weight × saturating_ratio(category)
```

| Category | Ratio basis | Default weight |
|----------|-------------|----------------|
| unused code | dead symbols / total symbols | 25 |
| unused deps | misdeclared (unused, test-only, undeclared) / declared | 15 |
| unused files | orphan files / total files | 10 |
| test-only code | test-only symbols / total symbols | 10 |
| duplication | duplicated tokens / total tokens | 20 |
| CRAP | CRAPload above threshold, normalized | 20 |
| cycles | files participating in cycles / total files | 5 |
| excess visibility | internal-only symbols / exported symbols | 5 |
| test blind spots | untested production symbols / production symbols | 5 |

`saturating_ratio` maps each raw ratio through a per-category curve (documented constants) so a
single bad file can't zero the score and improvements near zero still show. Grades: A ≥ 90,
B ≥ 80, C ≥ 65, D ≥ 50, F below. Output always shows the per-category breakdown and, in diff
modes, the delta caused by the change. Weights are configurable; defaults are the contract.

**Landed (M4) — the documented constants.** The curve is `saturating_ratio(r) = min(r /
saturation, 1)`: linear near zero, full weight at the saturation point. Defaults (the
contract until the config file lands):

| Category | Saturation (ratio = full weight) |
|----------|----------------------------------|
| unused code | 0.25 |
| unused deps | 0.5 |
| unused files | 0.25 |
| test-only code | 0.25 |
| duplication | 0.3 |
| CRAP | 0.5 |
| cycles | 0.25 |
| excess visibility | 0.5 |
| test blind spots | 0.5 |

Ratio definitions where the table above leaves room: *duplication* counts the tokens of every
clone-group member beyond its canonical (lexicographically first) instance — the copies you'd
delete, not the one you'd keep; *CRAP*'s raw ratio is `Σ max(0, CRAP(m) − threshold) /
(threshold × functions)` (average threshold-excess per function, in threshold units), with
`crapload = Σ CRAP(m)` over threshold-exceeding functions reported alongside; *cycles* counts
only files in tolerance-**reported** cycles (§8 — `Impossible`/idiomatic-skip cycles are not
penalties); *test blind spots* is skipped entirely when the project has no test roots, the
same honesty gate as §9's diagnostic. Health is computed before baseline and suppression are
applied — the score measures the codebase, not how much of it has been acknowledged away.
The score floors at 0 (weights sum to 115).

## 12. Suppression model

- Inline: `kndo:allow <category>[:<subject>] [reason]` in a comment on/above the declaration,
  or `kndo:allow-file …` for file scope. **Adapters extract** the pragmas (comment syntax is
  language-defined — `FileFacts.suppressions`, contracts §2.1); the **core validates and binds**
  them: a declaration-scoped allow covers the symbol and everything it declares. Suppression
  *marks* findings, never deletes them — analyses compute the full set first, then pragmas match
  against it, so an actively-suppressing pragma can never be `stale` and deleting a stale pragma
  can never resurrect a finding (no allow/stale flicker loop; contracts §2.1). `stale` itself is
  not inline-suppressible.
- Baseline: `.kndo/baseline.json` acknowledges existing findings at adoption time (RFC 0006 §6).
- Config: per-glob disables of categories or `category:subject` pairs (e.g. `examples/**` exempt
  from `unused`; `unused:enum-member` off globally for codebases with wire-format enums).
  All suppressions are themselves counted and reported (`suppressed: N`) — hidden waste is
  still waste, and a stale suppression (target finding gone) becomes an info finding.

## 13. Candidate rules — triage log & open candidates

Triage of 2026-08-18 (earlier promotions: `internal-only` → §7, `cyclic` → §8):

| Candidate | Decision |
|-----------|----------|
| `duplicate-asset` | **Adopted** — folded into `duplicate` with subject `file` (§6): byte-identical files, free on the blake3 hashes we already compute; lands M1 |
| `orphan-export` | **Dropped, subsumed** — fully covered by `internal-only` (§7), whose remediation ("lower the visibility") is strictly better than "maybe delete the export" |
| `unused-css-variable` | **Dropped, subsumed** — falls out as `unused:css-variable` once the CSS adapter extracts custom-property declarations and `var()` uses, which is already in its §RFC 0002 scope |
| `stale` on suppressions | **Already committed** — in the category registry; **landed in M6** (core `suppression.rs`: unknown-category, unbound, matched-nothing, and `kndo:allow stale` meta-suppression, all `info`/`hygiene` on the pragma's own span) |
| `barrel-abuse` | **Plugin territory** — a JS/TS ecosystem convention, not a language or graph fact; belongs to the js ecosystem plugin, post-1.0 |
| `dead-feature-flag` | **Plugin territory** — the useful version needs flag-system knowledge (LaunchDarkly, Unleash…); a constant-propagation assist in the core may follow real plugin demand |
| `layer-violation` | **Deferred post-1.0** — needs a layering-rules config DSL and does nothing under zero config; parking lot |
| `oversized-unit` | **Deferred post-1.0** — borders the linting non-goal; `crap` already covers the risky (untested) half |

Second triage, 2026-08-18:

| Candidate | Decision |
|-----------|----------|
| `private-type-leak` | **Adopted** into 1.0 (§7): zero new vocabulary — falls out of `TypeUse` edges crossing visibility downward; group `defect`; lands M3 with the visibility machinery |
| `redundant-export-binding` | **Deferred post-1.0**: requires modeling export *bindings* as contract entities distinct from symbols — real vocabulary cost for moderate value; parking lot |

| `deep-import` | ~~Deferred~~ → **Adopted** (superseding decision, same day): the contract-gate design removes the noise objection that motivated deferral — the finding fires only against a provider package that *declares* an explicit surface, so accepted-practice monorepos never see it. Full spec in RFC 0011 §4; group `risk`, pair-level rollup, computed remediation; lands M3 |

No candidates remain open. Future proposals enter through this table with the §13 acceptance
bar; every row above is a decision of record.
| `hollow-test` | test root whose forward closure reaches zero production symbols | the anti-slop "this test tests nothing real" detector (mocks-only tests); **open** — needs a named exemption mechanism for every legitimate zero-production-reach case (pure-assertion/property tests, contract tests against an external service) before it can claim zero FP, not just a low rate |
| `speculative-abstraction` | interface/trait with exactly one implementation and at most one consumer | YAGNI materialized; trivially derivable from `Implement` edges; `probable` confidence (DI/test seams exempt via plugin annotations, library-mode public abstractions exempt); group `waste` |

**Deliberately out of core: stale TODOs.** Detecting aged/orphaned TODO comments requires comment
extraction plus non-graph data (git blame age, issue-tracker state). That breaks the pure
static-graph model; if wanted, it is a plugin with its own data sources, not an analysis.

Acceptance bar for any rule, present or future: derivable from the graph, zero-config by default,
**zero false positives** — not a rate, a hard bar — and explainable in one sentence. A false
positive already costs trust the rule can't earn back, so "rare" is not good enough. Any case
where soundness can't be guaranteed statically (reflection/serialization, dynamic dispatch,
macro-generated consumers, framework conventions the adapter doesn't model) must demote the
finding below the report floor or exclude the case outright — never ship it at reduced
probability. A rule that cannot reach zero FP by this mechanism does not ship, full stop, and
does not get a row in this table either — a rule with a known unclosable blind spot is not a
candidate to triage, it is a rule that has already failed the bar.

**Scope of the bar (RFC 0018).** Everything above covers kndo's own categories — the bare
names in output-schema §6. Plugin-contributed findings (categories under the `plugin:` prefix,
group `convention`) are third-party verdicts the host cannot audit; they live on a separate,
advisory-by-default severity channel, structurally distinguishable by prefix, and are excluded
from the health score and every gate unless the user opts a specific coordinate in via
`[plugins.gate]`. The zero-FP statement neither covers them nor is diluted by them.
