# RFC 0005 — Analyses & Metrics

**Status:** Draft · **Depends on:** RFC 0001, 0002, 0004

All analyses are pure functions over the Project Graph (+ optional enrichments such as coverage).
Each finding carries: stable id, category, severity, confidence, location(s), evidence, and a
remediation hint (schema in [contracts/output-schema.md](../contracts/output-schema.md)).

## 1. Reachability foundation

Most detections derive from one computation. Roots are partitioned by `RootKind`:

- **Production roots** — language-defined entry points (`main`, published/public API of a library,
  package `exports`/`bin`) + plugin-contributed roots (framework handlers, DI-registered beans…).
- **Test roots** — test functions/files (language flavor detection + test-framework plugins).
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

## 2. `unused-code` — dead symbols

`unreachable` symbols. Severity: warning. Evidence: the symbol, why nothing reaches it, nearest
former consumer if known from the findings snapshot. Confidence downgrades if any wildcard edge
could plausibly target it (name exposed to reflection/serialization, plugin annotations, FFI).

## 3. `test-only-code` — non-productive code

Symbols/files colored `test-only`, excluding test-flavored files themselves and declared test
utilities (`testkit`/`fixtures` conventions, configurable). This is the "you built it, tests
enshrined it, production never came" detector — the finding explicitly lists the test roots that
keep the symbol alive, so deleting code + its tests together becomes mechanical.
Default severity: info (candidate to raise to warning — open question #3).

## 4. `unused-file` — orphan files

Files with no incoming import/reference edge and no root. Subsumes asset/config orphans via
cross-language edges (CSS, JSON). Generated/vendored flavors are exempt by default.

## 5. `unused-dependency` — manifest waste

For each `ManifestDependency` with scope `prod`: unused if no `File imports Package` edge from a
production- or tooling-reachable file resolves to it. Dev-scoped deps check against all files.
Adapter-provided package mappings handle subpath imports, type-only packages (`@types/*` bound to
their runtime package), and side-effect-only imports (`import "polyfill"` counts as usage).
Also detects the inverse, `undeclared-dependency`: imports that resolve to a package absent from
the manifest (phantom deps via hoisting). Severity: warning; error in `--strict`.

## 6. `duplicate-code` — structural clones

Token-based fingerprinting over adapter-normalized token streams (identifiers/literals
canonicalized ⇒ catches Type-1 and Type-2 clones; Type-3/semantic clones are out of scope for 1.0):

- Granularity: function/method bodies and top-level blocks ≥ `min-tokens` (default 50).
- Winnowing fingerprints into a global index; matches only within the same language.
- Finding groups all instances, largest group first; evidence shows the shared shape.

Severity: info by default (duplication is sometimes deliberate); the *metric* (duplication %)
always feeds health regardless of severity.

## 7. `crap` — Change Risk Anti-Patterns

Per function/method, with `comp` = cyclomatic complexity (adapter-extracted) and `cov` = fraction
of the function's statements covered:

```
CRAP(m) = comp(m)² × (1 − cov(m))³ + comp(m)
```

- Coverage comes from ingested reports (plugins, ADR 0005). No report ⇒ `cov` unknown ⇒ kondo
  reports **CRAPload with cov=0** but flags results "coverage: none" (configurable to skip).
- Threshold: findings for `CRAP > 30` (standard), configurable. Test code is exempt.
- Output ranks the CRAP hotspot list — the refactor-next queue.

## 8. `health` — project health score

A 0–100 composite, deterministic and documented so trends are meaningful:

```
health = 100 − Σ category_penalty
category_penalty = weight × saturating_ratio(category)
```

| Category | Ratio basis | Default weight |
|----------|-------------|----------------|
| unused code | dead symbols / total symbols | 25 |
| unused deps | unused / declared | 15 |
| unused files | orphan files / total files | 10 |
| test-only code | test-only symbols / total symbols | 10 |
| duplication | duplicated tokens / total tokens | 20 |
| CRAP | CRAPload above threshold, normalized | 20 |

`saturating_ratio` maps each raw ratio through a per-category curve (documented constants) so a
single bad file can't zero the score and improvements near zero still show. Grades: A ≥ 90,
B ≥ 80, C ≥ 65, D ≥ 50, F below. Output always shows the per-category breakdown and, in diff
modes, the delta caused by the change. Weights are configurable; defaults are the contract.

## 9. Suppression model

- Inline: a language-comment pragma `kondo:allow <category> [reason]` on the declaration.
- Baseline: `.kondo/baseline.json` acknowledges existing findings at adoption time (RFC 0006 §6).
- Config: per-glob category disables (e.g. `examples/**` exempt from unused-code).
  All suppressions are themselves counted and reported (`suppressed: N`) — hidden waste is
  still waste, and a stale suppression (target finding gone) becomes an info finding.

## 10. Candidate rules for debate

Statically derivable, deliberately **not** committed for 1.0 — each needs a yes/no:

| Candidate | Signal | Notes |
|-----------|--------|-------|
| `cyclic-dependencies` | SCCs in the file/package import graph | cheap on existing graph; strong health signal |
| `orphan-export` | exported but never imported inside an app package | subset of unused-code; maybe its own category for clarity |
| `barrel-abuse` | re-export files that fan out huge subgraphs | JS/TS-specific; hurts tree-shaking and kondo precision |
| `layer-violation` | user-declared layering rules (`ui -/-> db`) | needs config DSL; high value in monorepos |
| `oversized-unit` | file/function LOC & complexity ceilings | borders on linting — keep? |
| `dead-feature-flag` | flag constants that are constant-true/false | needs flag-system plugins |
| `stale-suppression` | suppression whose finding no longer exists | already implied by §9 — promote to rule? |
| `duplicate-asset` | identical files by content hash | trivial via blake3; catches copy-pasted configs/images |
| `unused-css-variable` | `--var` declared, never `var()`-consumed | fits CSS adapter naturally |

Acceptance bar for any rule, present or future: derivable from the graph, zero-config by default,
< 5% false-positive rate on the dogfood corpus, and explainable in one sentence.
