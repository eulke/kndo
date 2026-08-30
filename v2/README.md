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
  `ResolveCtx`, `Finding`/`Severity` with the canonical sort.
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

## Name verification (2026-08-29)

- npm: `kndo` still taken by an unrelated DeFi package (unchanged since the v1 check);
  `kndo-cli` free — the npm shim keeps that name.
- crates.io and Homebrew: unreachable from this session's proxy (403); the v1 check
  (2026-08-18: crates.io free, brew unverifiable) stands as most recent. Re-check both
  from an unproxied machine before M0 reserves anything.

Note for kndo-on-kndo: `v2/` is listed in the repository's `.ignore` so v1's own analysis
skips the seed (spike code, oracle JSON) while git tracks all of it — same mechanism and
reasoning as the fixture corpus.
