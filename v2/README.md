# v2 seed — M-1 groundwork

This directory is the pre-work for the kndo greenfield rebuild ("M-1" in the redesign
plan): everything the v2 needs harvested from v1 before its first real commit. The v2 is
built **in this repository, in place** — v1 becomes the read-only quarry it is being
harvested from. Product name: **kndo** (confirmed 2026-08-29).

The full redesign report (diagnosis with evidence, architecture, contracts, data shapes,
plan M-1→M6) lives as a published page; the decisions extracted from it start in
`DECISIONS.md` here.

## Contents

| Path | What it is |
|---|---|
| `CLAUDE.md` | The day-0 agent judgment file for the v2 — destined verbatim for the repo root at M0. |
| `DECISIONS.md` | Append-only decision log, seeded with the start decisions and the v1 decisions worth carrying (each with its measurement). |
| `EXPERIMENTS.md` | The measure-first backlog: every candidate feature with its prior data, including the ones v1's measurements killed (so nobody rebuilds them). |
| `corpus/corpus.toml` | The oracle corpus: 8 pinned open-source repos spanning the 6 languages, chosen because v1's own precision incidents anchor them. |
| `oracle/` | v1's findings over the corpus (the reference baseline for "same-or-better, every difference explained") + the script that regenerates them. |
| `harvest/` | Fixture inventory of the 9 v1 adapters + the script that exports them for v2 vendoring. |
| `spikes/fingerprint/` | The structural-fingerprint derive spike and its verdict (the one mechanism v2's cache depends on that had never been measured). |
| `harness/` | The agent working harness: conventional-commit hook, a git wrapper that blocks stash/checkout-as-verification, and the verify-without-destroying helper. |

## M-1 exit criteria — status

- [x] v2 seed committed on the existing repo (owner decision 2026-08-29: no new repository;
      greenfield in place).
- [x] Corpus pinned by commit with licenses reviewed (`corpus/corpus.toml`).
- [x] v1 oracle recorded and versioned (`oracle/`).
- [x] Fingerprint spike with a written verdict (`spikes/fingerprint/VERDICT.md`).
- [x] Harness ready to activate before v2's first push (`harness/install.sh`).

## M0 — skeleton status

The v2 workspace lives at `v2/` (this directory) beside the v1 it replaces, until the
root swap; `cargo` commands run from here, on the pinned toolchain
(`rust-toolchain.toml`, 1.98.0 — local and CI identical by construction).

- `crates/kndo-cli` — the `kndo` binary at hello-world size: enough artifact for the
  release loop to be real before any analysis code can hide its failures.
- `crates/kndo-gates` — the gate registry `.github/workflows/v2.yml` is **generated**
  from (`cargo xtask gen-ci`); the `generated_ci_is_current` gate fails the build when
  the committed workflow and the registry drift. One spelling of the invariant list.
- `xtask` — `gen-ci`, `package` (build + tar.gz + sha256), `verify-artifact`
  (checksum-verify, extract, run — in Rust, so linux/macos/windows run the identical
  install check).
- CI (`v2.yml`): lint (fmt, clippy, conventional-commit), test matrix on linux/macos
  (Windows deferred by owner decision — tree-sitter-scss upstream; see DECISIONS.md),
  named gates, and package+verify-install on both platforms.
- CI verification pending an external blocker: the account's Actions credits are
  exhausted until 2026-09-01 (every job dies in seconds, v1's CI on main included);
  a scheduled re-fire via workflow_dispatch closes M0 when credits reset.
- Exit-criterion adaptation, recorded in `DECISIONS.md`: the ceremonial `v0.0.0-rc` tag
  is deferred — v1's release.yml fires on any `v*` tag — and the package+install matrix
  proves the same loop on every push instead.

## M1 — status

Complete (CI green pending only the credits reset, like M0):

- `kndo-contract` grew the adapter side: `LanguageAdapter`, the M1 `AdapterSpec`
  (id, `semantics_version`, claim globs, `emits` — the pairing rule's declaration),
  `ResolveContext`, `Finding`/`Severity` with the canonical sort.
- `kndo-core`: the Session/Snapshot engine — deterministic discovery (gitignore-aware,
  path-sorted), claim by spec globs, parallel extraction through the evidence cache
  (key folds adapter id + semantics_version + CONTRACT_FINGERPRINT + emits + content
  hash), two-phase assembly, `Analysis::requires()` + `AbstentionScope` partition, and
  the `unused` analysis. Reports carry no wall-clock fields by design.
- `kndo-testkit`: `MockAdapter` (the `.kmock` DSL) + `TempProject` — contract-only.
- Four new named gates, workflow regenerated: `dogfood_kndo_reports_nothing_on_itself`
  (trivially zero until a real adapter lands — active from day 1),
  `warm_and_cold_runs_are_byte_identical`, `threads_one_and_many_are_byte_identical`,
  `incremental_and_full_assembly_are_byte_identical` (M1 form: change replayed over
  cached evidence ≡ from-scratch; the surgical graph patch tightens it in M2).

## M2 — status

Complete (CI verification pending only the credits reset, like M0/M1). The second
slice landed the four remaining pieces:

- **Manifest/root capabilities**: `AdapterSpec` declares manifest globs;
  `LanguageAdapter::roots` anchors `package.json` entries (`main`/`module`/`browser`/
  `bin`, `exports` AND `imports` leaves, wildcard entries via
  `ResolveContext::files_with_prefix`, `.d.ts` companions, npm-`scripts` sources) on
  `GraphFile.anchored` — engine-side, never inside cached evidence.
  `LanguageAdapter::packages` links workspace bare imports to sibling entries.
  Convention roots (shebang, test dirs/names, config files) are path-conditional
  extraction evidence, so the evidence cache key folds the path. Per-import targeting
  replaced the M1 bind-to-every-target approximation; `require()`/dynamic `import()`
  count as imports. Corpus effect: vite 771→702, lodash 50→18, with
  `corpus-findings/COMPARISON.md` explaining every remaining difference vs the
  oracle.
- **Persisted graph cache with surgical patching** (`.kndo/cache/graph.bin`, keyed by
  fingerprint + `GRAPH_SEMANTICS_VERSION` + the adapter set): content-only changes
  patch in place; a moved file set or manifest falls back to full assembly from
  cached evidence. The incremental gate proves patched ≡ full across content-change,
  add, and delete.
- **Harvested-fixture conformance**: the 22 v1 js fixture projects replay through the
  v2 engine under the new `adapter_conformance_fixtures_are_byte_identical` gate,
  each report pinned byte-for-byte and regenerated only deliberately.
- **The corpus CI job**: `corpus/clone.sh` fetches the pins shallowly (cached by
  `corpus.toml` hash), `cargo xtask corpus` measures, and `git diff --exit-code`
  holds `corpus-findings/` equal to the committed measurement — the same three
  commands exercised locally before the job landed.

Earlier in M2 (first slice):

- `crates/kndo-toolkit` — the adapters' paved road: tree-sitter parse/span/text/walk
  helpers whose behavior is grammar-independent (v1 carried five verbatim copies of
  `parse`).
- `crates/kndo-adapter-ts` — the first real language, id `js-ts` (v1's id, so oracle
  comparisons line up): TS/TSX/JS/JSX/MJS/CJS through the tree-sitter-typescript
  grammars. Top-level declarations with reach and export aliases (`exported_as` carries
  what importers actually bind — `export default`, `export { a as b }`), class members
  owned by id, every ESM import shape (including `ReexportAll`, the shape `export *
  from` taught the contract), keep-alive-biased references, comment spans, and relative
  resolution in Node/TS candidate order. Extraction degrades through diagnostics —
  never fails.
- `crates/kndo` — the facade and composition root: frontends import `kndo::<Name>`
  only; `default_adapters()` is the one list of what a stock run speaks.
- The engine grew `Analysis::abstains(graph)`: `unused` abstains whole-run on a graph
  with zero roots (`NoRootsAnywhere`) instead of accusing every file — root evidence
  arrives with the manifest capabilities. Member declarations are judged against the
  whole reachable graph's references (dispatch is not lexical).
- The dogfood gate now runs the REAL default adapter set over this repository (the
  repo's own JS surface is claimed; zero findings via honest evidence, with the
  no-roots abstention on record).
- `cargo xtask corpus` + `corpus-findings/` — the first v2 measurement over the pinned
  corpus, versioned: vite 1,553 files claimed, 3,491 declarations, 87,573 references,
  1,565 resolved import edges, byte-identical across runs.

## M3 — status

Complete (CI verification pending only the credits reset, like M0–M2):

- **Reachability by colors**, computed once per run and shared through `RunContext`:
  production/test/tooling flood from each file's own roots over resolved edges.
- **test-only** (Info/Probable): files only the test color reaches, with a file that
  carries its own Test root exempt. **untested** (Info): coverage speaks first —
  ingested lcov (`lcov.info`, `coverage/lcov.info`; FN/FNDA before DA, because a
  declaration line executes at module load) makes uncovered functions `Certain`;
  where coverage is silent, the graph heuristic (no test-rooted file references it)
  judges at `Probable`. Both abstain without test evidence. **duplicate** (Info):
  byte-identical files by content hash, structural Type-1/2 function clones by
  winnowed fingerprint-set equality (toolkit-owned winnowing, kind-normalized
  leaves, corpus-measured 60-token floor).
- **Central suppression** (§6.6 realized): adapters report comment spans; the engine
  owns the `kndo:allow` / `kndo:allow-file` grammar, categories validate through
  `Category::parse`, and a stale allow is a Warning — except over categories the run
  did not JUDGE (abstained or not yet built), the flicker rule that this repository's
  own v1 pragmas exercised the day it landed.
- **Baseline**: `.kndo/baseline.json`, written whole by `Session::write_baseline`;
  reports split findings into new/`fixed`/`baselined` and the gate counts new only.
  `RunOutcome::Refused` completes the exit-code story (0 pass / 1 findings / 2
  refused).
- **Schema, generated and validated**: `cargo xtask gen-schema` derives
  `schemas/report.schema.json` from the types (schemars behind the `schema`
  feature); the `report_schema_is_generated_and_valid` gate holds it current AND
  validates a live fixture report plus the versioned vite corpus report against it.
  `SCHEMA` moved to `kndo-v2/m3`.
- Two new named gates (nine total): `dogfood_zero_means_measured` — every dogfood
  abstention is accepted in writing, so zero findings can never quietly mean
  un-judged — and the schema gate above.
- Corpus (M3 exit numbers, all deltas explained in `corpus-findings/COMPARISON.md`):
  vite 1,023 (unused 702, duplicate 174 vs oracle 179, test-only 122, untested 25),
  lodash 21, Alamofire 5 duplicates; dogfood still zero through honest evidence.

## M4 — status

- **M4.a — contract growth pack**: `Resolution::Files` (a directory as the imported
  unit), `ImportShape::Glob`, `PackageEntry.entry` optional (entry-less ecosystems),
  `ResolveContext::package_of`, and `import_targets` as one-to-many
  (`GRAPH_SEMANTICS_VERSION` 2). Corpus held byte-identical — shape only.
- **M4.b — the Rust adapter, done**: `kndo-adapter-rust` (id `rust`, tree-sitter-rust)
  — binary reach (`pub` in any form is nameable beyond the file), `mod` edges with
  `#[path]` redirects and alias substitution, the full `use`-tree (globs, groups,
  aliases, reexports; each plain leaf a binding+namespace pair), qualified paths as
  deduplicated inline imports, crate roots by ancestor scan, Cargo.toml targets and
  conventions as roots, `#[test]`/linkage-attribute/`fn main` declaration roots,
  winnowing metrics, comment spans with doc-slash stripping. 25 v1 fixtures replay
  byte-pinned in the conformance gate beside the 22 js ones.
- **The dogfood now judges v2 itself**: the root `.ignore` quarries v1 and the
  ACCEPTED abstention list is empty — zero findings, every analysis judging, and the
  33 findings first contact produced each became a fix (core's duplicated
  `line_starts`, test harnesses promoted to `kndo-testkit`, the member rule's
  re-export blindness) or a recorded gap.
- **untested** decides by test reachability at file granularity where coverage is
  silent (v1 parity; manifest-anchored entries are wiring), coverage per-function
  `Certain` unchanged; three js fixtures regenerated under the documented change.
- **M4.c — the Go adapter, done**: `kndo-adapter-go` (id `go`, tree-sitter-go) —
  the package as the unit (`LanguageAdapter::unit_mates` declares what a file
  sees without an import — the engine draws the edges and pools references;
  module-path-prefix resolution to `Resolution::Files`), capitalization as
  reach, library-mode roots with the `internal/` fence, generated-file and
  structural-interface never-accuse rules. Eight v1 fixtures replay byte-pinned;
  the conformance gate now walks three corpora (22 js + 25 rust + 8 go).
- **M4.d — the ladder, measured and deferred**: 8,026 oracle findings depend on
  visibility-ladder knowledge (internal-only 7,983, private-type-leak 43) — the
  largest unbuilt category — but its consumer (the internal-only analysis) does
  not exist, and M4's own evidence says the missing dimension is scope SHAPE
  (Go's Directory reference scope, Rust's module-tree bindings — both landed as
  their own constructs). The ladder arrives with internal-only, rung+shape from
  the start; EXPERIMENTS and DECISIONS carry the measurement.
- **M4.e — the dist experiment pays**: a manifest entry naming absent built
  output maps to its `src/` source, existence-gated. vite test-only 122 → 16
  (oracle 27) — the milestone's biggest recorded distortion closed by one
  deliberate mapping; lodash and all 56 conformance fixtures byte-identical.
- Corpus at M4 exit: ripgrep 143 (unused 3 — all verified true at the source,
  duplicate 135, test-only 3, untested 2) against oracle 149; gin 108 (all
  duplicate; unused 0, matching the oracle's 0); vite 895 (unused 699, duplicate
  174, test-only 16, untested 6); lodash 21. Deltas named in COMPARISON.md.
  Three languages pass conformance (22 js + 25 rust + 8 go fixtures); the
  multi-language corpus is stable at the pins — the two-weeks criterion is
  wall-clock and owner-tracked from here.

## M5 — status (in progress)

- **M5.a — the two facade frontends, done**: the `kndo` CLI is real (`check` and
  `baseline`, `--json`, `--fail-on`, exit codes 0/1/2; library-with-thin-main so
  tests and the dogfood reach it through imports) and `kndo-serve` exists at
  skeleton size (MCP over stdio, one `check` tool returning the same Report
  envelope). The facade rule they prove is now the tenth named gate:
  `frontends_import_only_the_facade` — a frontend's production dependencies
  contain exactly one kndo crate. The release install-check runs `kndo
  --version`; the whole package/verify loop re-exercised locally.
- Pending: native plugins (M5.b), the WASM ABI with its compat matrix (M5.c),
  the external reference adapter from `.kndo/plugins/` (M5.d), close-out (M5.e).
- Owner decision (in DECISIONS.md): internal-only — and the visibility ladder
  with it — lands in M6.

## Name verification (2026-08-29)

- npm: `kndo` still taken by an unrelated DeFi package (unchanged since the v1 check);
  `kndo-cli` free — the npm shim keeps that name.
- crates.io and Homebrew: unreachable from this session's proxy (403); the v1 check
  (2026-08-18: crates.io free, brew unverifiable) stands as most recent. Re-check both
  from an unproxied machine before M0 reserves anything.

Note for kndo-on-kndo: the repository's `.ignore` quarries v1's source (and the frozen
M-1 spike) so the dogfood judges v2 — the kndo being built — while git tracks everything;
same mechanism and reasoning as the fixture-corpus exclusion. The quarry entries retire
at the root swap.
