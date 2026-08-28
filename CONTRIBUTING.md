# Contributing

See `CLAUDE.md` at the repo root for the architectural working rules (facade imports,
error/config discipline, finding-identity stability, the toolkit-vs-adapter split, the
non-negotiable equivalence gates) — it's written for coding agents but applies equally to
human contributors.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`.

- Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
- Subject: imperative mood, ≤72 chars, no trailing period.
- Body: optional, **1–2 lines max**. State what changed and why if it's not obvious from the
  subject — not a design rationale. Extended discussion belongs in the PR description or the
  relevant `internal/` design document, not the commit body. (Design documents live in
  `internal/`; `docs/` is user documentation.)

```
feat(adapter-js): extract export surface from CJS module.exports
fix(cache): invalidate facts on grammar version bump
docs(guide): clarify baseline workflow in the user guide
```

## Building & local development

This is a large workspace (wasmtime + cranelift, eight tree-sitter grammars) — a full
`cargo build`/`cargo test --workspace` is not the fast inner loop. Prefer, in order:

- **`cargo check`** (or `cargo check -p <crate>`) while iterating — no codegen, catches type
  errors fastest.
- **`cargo clippy --workspace --all-targets --all-features`** before pushing — the same lint
  gate CI runs; still no full codegen.
- **`-p <crate>`** to build/test only the crate you're changing (e.g.
  `cargo test -p kndo-adapter-swift`) instead of the whole workspace — the other seven
  adapter crates and `kndo-core` don't need to rebuild when you're only touching one adapter.
- **`cargo build`** / **`cargo test --workspace`** as the final, full-fidelity check before a
  PR — this is what CI actually gates on.

The repo's `[profile.dev]` (root `Cargo.toml`) already keeps your own crates fast to
recompile while optimizing dependencies you don't edit (wasmtime/cranelift, the tree-sitter
grammars) so `cargo test`'s real parsing work over fixtures isn't slow — nothing further is
required to get that.

**Optional: a faster linker.** Rebuilds here are link-time-bound as much as compile-time-bound
(wasmtime is a large linked dependency). If you're on Linux or macOS and want a faster linker
for local iteration, opt in via your own **user-global** Cargo config
(`~/.cargo/config.toml`, *not* anything committed to this repo — contributors on Windows, or
without the linker installed, would otherwise have their builds broken by a repo-committed
default):

```toml
# ~/.cargo/config.toml — your own machine only, never commit this to the repo.

# Linux, with mold installed (https://github.com/rui314/mold):
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "link-arg=-fuse-ld=mold"]

# macOS, with a recent lld (e.g. via `brew install llvm`; mold doesn't support macOS):
[target.aarch64-apple-darwin]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]
```

Install `mold` (Linux, via your package manager) or make sure `lld` is on `PATH` (macOS)
before adding this — an unresolvable `-fuse-ld` flag breaks every build on that machine. This
is a per-contributor convenience, never a repo default.

## Conformance fixtures

Each adapter's `tests/fixtures/<name>/` holds a `project/` tree and an `expected.json`; the
harness runs the real engine over the project and asserts the findings match exactly. They are
deliberately-flawed corpora — dead code, phantom dependencies, cycles — so they must be
excluded from kndo's analysis of its own repo, but they are ordinary tracked files as far as
git is concerned.

That exclusion lives in **`.ignore`**, not `.gitignore`. The `ignore` crate (and ripgrep, and
fd) read `.ignore`; git does not. Putting it in `.gitignore` also governs `git add` for
untracked files, which silently skips a newly added fixture — it passes locally and is simply
absent from CI. If you add a fixture and `git status` doesn't show it, that is the bug to look
for.

A fixture change is never a way to make a failing test pass. `expected.json` is a contract:
a diff there is either a bug in your change or a deliberate, documented contract change
explained in the commit message.

**Cross-language fixtures live in `crates/kndo/tests/fixtures/`**, same format, run through
`kndo::default_adapters()` so every adapter is registered at once. An adapter's own suite
structurally cannot cover what happens BETWEEN adapters: the jquery/Jazzy misattribution — a
generated `.js` under `docs/` in a Swift repo charged its bare imports to `Package.swift`, and
every Swift repo in the field audit reported a phantom dependency for it — was invisible to
both the Swift suite (no JS adapter to claim the file) and the JS suite (no `Package.swift` to
misattribute to). If your change touches file→package ownership, manifest claiming, or
anything that reads `FileNode::language`, that is the suite to extend.

## Coverage

kndo's own `crap` analysis (complexity × untestedness) runs only when a coverage report is
present — with none ingested it skips with a diagnostic instead of guessing. To give it (and
yourself) real data locally:

```sh
cargo install cargo-llvm-cov          # once
cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info
```

That runs the full test suite instrumented and leaves `lcov.info` at the repo root — one of
kndo's well-known coverage paths (`coverage/lcov.info` is the other; Cobertura XML, JaCoCo
XML and Go coverprofile reports are ingested the same way in projects that produce those,
and `[plugins.<id>] report` in kndo.toml points at custom locations), picked up on the next
`kndo check`. Reports older than 7 days are ignored with a diagnostic (stale certainty is
worse than absence) — just regenerate. `lcov.info` and `coverage/` are gitignored; CI
generates its own report in the test job, so the self-check there always runs
coverage-aware. The WASM guest builds some integration tests spawn strip
`RUSTFLAGS`/`CARGO_ENCODED_RUSTFLAGS` themselves, so the instrumented run works end to end.

## Benchmarks

`cargo xtask bench` builds a release `kndo` and measures end-to-end wall time over generated
fixtures at 1k / 5k / 50k files, five scenarios each, against the recorded baseline in
`internal/perf-baseline.json`. Without `--gate` it reports and exits clean; with `--gate` a
regression fails the build. `--sizes 1k` alone is the quick one.

**Run it before and after a change you expect to cost time, on the same machine, and compare
those two runs — not either one against the committed baseline.** That baseline records one
machine, and it does not travel. Measured: on a container quite unlike the one it was recorded
on, the same unmodified tree reported `1k/cold-full` **22% faster** and `1k/warm-noop` **108%
slower** in a single run. Not noise, and not contradictory — cold time is dominated by parsing
and analysis, warm time by process startup and cache reads, and different hardware moves those
in opposite directions. `--update-baseline` re-records it for your machine; that is a local
convenience, so leave the committed numbers alone unless the reference machine itself changed.

**This is deliberately not a CI job**, and the measurement above is why: on ephemeral runners
of varying hardware, the gate would compare numbers that were never comparable and fail for
reasons unrelated to any change. Unlike the release-notes generation — which nothing ever
exercised before a tag, unattended, which is why CI runs it now — the benchmark has a human
present every time it runs, and a broken harness surfaces to that human in seconds. A perf gate
worth having needs a dedicated, stable machine, which is a decision about infrastructure rather
than about this workflow file.

## Branching — Gitflow

- `main` — always releasable; only tagged versions land here; merges only from `release/*` or
  `hotfix/*`.
- `develop` — integration branch; default base for new work.
- `feature/<short-name>` — branches off `develop`, merges back via PR.
- `release/<version>` — cut from `develop` to stabilize before a release; merges to `main` and
  back to `develop`.
- `hotfix/<short-name>` — branches off `main` for urgent fixes; merges to `main` and `develop`.

Delete a branch once merged.
