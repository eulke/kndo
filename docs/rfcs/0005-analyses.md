# RFC 0005 — Analyses & Metrics

**Status:** Draft · **Depends on:** RFC 0001, 0002, 0004

All analyses are pure functions over the Project Graph (+ optional enrichments such as coverage).
Each finding carries: stable id, category, severity, confidence, location(s), evidence, and a
remediation hint (schema in [contracts/output-schema.md](../contracts/output-schema.md)).

**Taxonomy rule — a category is a verdict; the subject is a facet; the reporting level is a
rollup.** Three orthogonal things, kept orthogonal:

1. **Category = verdict, nothing else**: `unused`, `test-only`, `undeclared`, `unresolved`,
   `duplicate`, `crap`, `stale`. A verdict means the same thing whatever it lands on — there is
   no `unused-file` vs `unused-code`: both are `unused`.
2. **Subject = the `subject_kind` facet**: the kind of graph node the verdict landed on — `file`,
   `directory`, `dependency`, `import`, `suppression`, or any symbol kind (`function`, `type-alias`,
   `enum-member`, `css-rule`…). Configuration and suppressions target a bare category or
   `category:subject` (e.g. `unused:enum-member`, `test-only:dependency`); knip-style
   `unused-type` ≡ `unused:type-alias`.
3. **Reporting level = widest uniform node**: when a verdict holds for every symbol in a file
   *and* for the file node itself, kondo emits **one** finding on the file (subject `file`), not
   N symbol findings; when it holds for every file in a directory, one finding on the directory.
   Rollup is presentation of the same facts, not a different verdict — "test-only file" is the
   `test-only` verdict reported at file granularity.

4. **Group = the verdict's nature, fixed per category**: every verdict belongs to exactly one
   presentation group, declared here — never configurable, never guessed by renderers:

   | Group | Verdicts | Meaning for the reader |
   |-------|----------|------------------------|
   | `defect` | `unresolved`, `undeclared` | something is broken or lying — fix it |
   | `waste` | `unused`, `test-only`, `duplicate`, `internal-only` | something can be removed, consolidated, or narrowed |
   | `risk` | `crap`, `cyclic` | something is dangerous to change — refactor or test it |
   | `hygiene` | `stale` | kondo's own bookkeeping is outdated |

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

Two reachability passes (production-only roots; then all roots) assign every symbol/file a color:

| Color | Meaning |
|-------|---------|
| `production` | reachable from a production root |
| `test-only` | reachable only from test roots |
| `tooling-only` | reachable only from tooling roots |
| `unreachable` | reachable from nothing |

Wildcard edges (dynamic constructs, RFC 0002 §5) make their source's reachable set conservative:
anything plausibly targeted is kept alive at `possible` confidence rather than reported dead.

**Library mode:** for library packages the public API is a production root by definition —
kondo will not call exported API "unused" just because the repo doesn't call it. Within an
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
Default severity: info (candidate to raise to warning — open question #3).

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

Two further import-side findings:

- `undeclared` (subject `dependency`) — an import resolves to a package absent from the manifest
  (phantom deps via hoisting/transitivity). Severity: warning; error in `--strict`.
- `unresolved` (subject `import`) — a relative/internal import specifier that resolves to no file
  (`Resolution::Unresolved` after all adapters decline): almost always a broken path or a missed
  rename. Failed *package* resolution surfaces as `undeclared` instead, never twice.
  Severity: error (it is a defect, not waste) — but confidence-gated: dynamic specifiers demote
  to `possible` and drop below the default report floor.

## 6. `duplicate` — structural clones

Token-based fingerprinting over adapter-normalized token streams (identifiers/literals
canonicalized ⇒ catches Type-1 and Type-2 clones; Type-3/semantic clones are out of scope for 1.0):

- Granularity: function/method bodies and top-level blocks ≥ `min-tokens` (default 50).
- Winnowing fingerprints into a global index; matches only within the same language.
- Finding groups all instances, largest group first; evidence shows the shared shape.

Severity: info by default (duplication is sometimes deliberate); the *metric* (duplication %)
always feeds health regardless of severity.

## 7. `internal-only` — excess visibility

A symbol's declared visibility exceeds its observed use. For every symbol declared above the
minimum visibility, the analysis computes the **tightest sufficient visibility**: the lowest
level on the language's visibility ladder (adapter-declared — e.g. private → file → package/crate
→ public) that still covers the origin of every incoming reference. Declared > sufficient ⇒
finding; the remediation names the concrete change in the language's own terms (`private`,
`pub(crate)`, unexported lowercase name), supplied by the adapter.

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
init-order bugs), `info` where they are idiomatic (Rust modules within a crate), and impossible
levels are skipped outright (Go package cycles — the compiler already forbids them).

## 9. `crap` — Change Risk Anti-Patterns

Per function/method, with `comp` = cyclomatic complexity (adapter-extracted) and `cov` = fraction
of the function's statements covered:

```
CRAP(m) = comp(m)² × (1 − cov(m))³ + comp(m)
```

- Coverage comes from ingested reports (plugins, ADR 0005). No report ⇒ `cov` unknown ⇒ kondo
  reports **CRAPload with cov=0** but flags results "coverage: none" (configurable to skip).
- Threshold: findings for `CRAP > 30` (standard), configurable. Test code is exempt.
- Output ranks the CRAP hotspot list — the refactor-next queue.

## 10. `health` — project health score

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

`saturating_ratio` maps each raw ratio through a per-category curve (documented constants) so a
single bad file can't zero the score and improvements near zero still show. Grades: A ≥ 90,
B ≥ 80, C ≥ 65, D ≥ 50, F below. Output always shows the per-category breakdown and, in diff
modes, the delta caused by the change. Weights are configurable; defaults are the contract.

## 11. Suppression model

- Inline: a language-comment pragma `kondo:allow <category>[:<subject>] [reason]` on the declaration.
- Baseline: `.kondo/baseline.json` acknowledges existing findings at adoption time (RFC 0006 §6).
- Config: per-glob disables of categories or `category:subject` pairs (e.g. `examples/**` exempt
  from `unused`; `unused:enum-member` off globally for codebases with wire-format enums).
  All suppressions are themselves counted and reported (`suppressed: N`) — hidden waste is
  still waste, and a stale suppression (target finding gone) becomes an info finding.

## 12. Candidate rules for debate

Statically derivable, deliberately **not** committed for 1.0 — each needs a yes/no:

| Candidate | Signal | Notes |
|-----------|--------|-------|
| `orphan-export` | exported but never imported inside an app package | subset of `unused`; maybe its own verdict for clarity |
| `barrel-abuse` | re-export files that fan out huge subgraphs | JS/TS-specific; hurts tree-shaking and kondo precision |
| `layer-violation` | user-declared layering rules (`ui -/-> db`) | needs config DSL; high value in monorepos |
| `oversized-unit` | file/function LOC & complexity ceilings | borders on linting — keep? |
| `dead-feature-flag` | flag constants that are constant-true/false | needs flag-system plugins |
| `stale` on suppressions | suppression whose finding no longer exists | already implied by §11 — promote to rule? |
| `duplicate-asset` | identical files by content hash | trivial via blake3; catches copy-pasted configs/images |
| `unused-css-variable` | `--var` declared, never `var()`-consumed | fits CSS adapter naturally |
| `redundant-export-binding` | one symbol exported under multiple bindings where some binding has zero consumers | language-neutral form of JS "redundant default/named export"; also covers Rust `pub use` re-exports |
| `private-type-leak` | public symbol whose signature references a non-exported type | API hygiene; derivable from type-reference edges |

**Deliberately out of core: stale TODOs.** Detecting aged/orphaned TODO comments requires comment
extraction plus non-graph data (git blame age, issue-tracker state). That breaks the pure
static-graph model; if wanted, it is a plugin with its own data sources, not an analysis.

Acceptance bar for any rule, present or future: derivable from the graph, zero-config by default,
< 5% false-positive rate on the dogfood corpus, and explainable in one sentence.
