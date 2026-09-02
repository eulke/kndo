# Contributing

The judgment layer — which floor a fact lives on, which version knob a change turns,
what a comment may say — is `CLAUDE.md`; the executable law is the gate registry in
`kndo-gates`, and CI's steps are rendered from it. This file is the mechanics:
commits, building, fixtures, the corpus, coverage, benchmarks, releases.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`,
imperative, no trailing period; CI lints the subject. The body says what changed and why
when the subject cannot; the design rationale belongs in `DECISIONS.md`, dated, with its
measurement. A fixture, baseline or output-schema change is a contract change and the
commit says so.

```
feat(js-ts): a workflow step's launched file is a root
fix(core): a one-line function has no body line to read
docs(decisions): gen-stdlib is dead, with the number
```

## Building and verifying

The toolchain is pinned by `rust-toolchain.toml`; rustup installs it on first use, so
local and CI runs are the same compiler by construction. The workspace suite builds real
WASM components while it runs, which needs the target CI installs:

```sh
rustup target add wasm32-unknown-unknown
```

The inner loop is per crate — `cargo check -p kndo-core`, `cargo test -p kndo-adapter-go` —
and the three checks CI runs are the ones to run before pushing:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

The gates are one test file: `cargo test -p kndo-gates --test gates`. `cargo xtask gen-ci`
renders `.github/workflows/ci.yml` and `release.yml` from the registry; a gate fails
when the committed workflow is stale, so regenerate after touching the registry or the
templates.

A full debug build of this workspace with debuginfo is large — it filled a 25 GB
allowance once; `CARGO_PROFILE_DEV_DEBUG=0` keeps the same build under 10 GB. A local
convenience for constrained machines, never a repository default.

## Conformance fixtures

Each adapter's `tests/fixtures/<name>/` holds a `project/` tree and an `expected.json`:
the real engine over the project, the report byte-identical to the file. They are
deliberately flawed corpora — dead files, phantom dependencies, cycles — so the root
`.ignore` keeps them out of kndo's analysis of its own repository; git tracks them like
any file (an exclusion in `.gitignore` would also govern `git add`, and a new fixture
would silently never reach CI).

The gate holds a floor count per corpus; a new fixture raises the floor in the same
commit. To regenerate:

```sh
KNDO_CONFORMANCE=overwrite cargo test -p kndo-gates --test gates adapter_conformance
```

then read the diff. A diff is either a bug in your change or a deliberate contract
change explained in the commit and in `DECISIONS.md` — never a way to make a failing gate
pass.

## The corpus

`corpus/corpus.toml` pins the oracle repositories at exact commits; `corpus/clone.sh
<dir>` fetches them; `cargo xtask corpus --corpus-dir <dir>` runs the default engine over
every clone and rewrites `corpus-findings/`. Any change that claims to improve findings
carries its delta there, decomposed in `corpus-findings/COMPARISON.md` against the oracle.

## Coverage

`crap` and the Certain rung of `untested` need a coverage report; with none ingested they
abstain, and the report says so. The built-in ingesters read `lcov.info` or
`coverage/lcov.info` (and the Cobertura, JaCoCo and Go coverprofile locations) on the next
`kndo check`.

Delete the report before running the gates. `dogfood_zero_means_measured` pins the
standing abstention set of kndo on its own repository — `crap` abstaining for want of a
report is part of it — so a report left at the root flips that gate for a reason that has
nothing to do with your change.

## Benchmarks

`cargo xtask bench` builds a release `kndo` and measures end-to-end wall time (process
start to rendered JSON) over generated fixtures at 1k, 5k and 50k files, five scenarios
each, against `xtask/perf-baseline.json`. `--sizes 1k` is the quick one; `--gate` fails on a
regression (both >10% and >10 ms over baseline); `--update-baseline` re-records.

The baseline is one machine's numbers and does not travel: compare a run before and a run
after your change, on the same machine, and leave the committed file alone unless the
reference machine changed. It is deliberately not a CI job — ephemeral runners would
compare numbers that were never comparable. What the committed table says about the
shape of a run: warm scenarios are the cache read plus discovery and hashing (50k files
in about 1.1 s), cold is parsing and analysis (about 4.4 s at 50k), and `--staged` is two
full analyses over two archived trees, so it costs more than a cold run at every size.

## Releases

A release is a `v*` tag: the `release` workflow (dispatchable with a tag as the escape
hatch) packages every row of the target table through `cargo xtask package`, publishes the
archives with notes rendered by git-cliff, and updates the Homebrew tap template. The
table — targets, binary name, artifact name and layout — lives once, in
`kndo_gates::release`; `release_channels.rs` reads the installer, the Action, the Homebrew
template and the install page against it. Locally, the same loop is:

```sh
cargo xtask package --tag v0.0.0-local --out-dir dist
cargo xtask verify-artifact --dir dist
```

Every step of the release runs in CI on every push before any tag exists: the musl
package and its static check, the installer over a local HTTP server, the notes.

## Windows

Windows is in both CI matrices. The one grammar whose build script MSVC refused is
vendored under `vendor/` with a single portable flag; `vendor/README.md` records the
deviation, and the workspace reaches it through `[patch.crates-io]`.

## Branching

Work on a branch, push, and let CI run; conventional subjects keep the changelog
renderable. Rewriting history on a branch someone else has checked out is never the
answer to a conflict.
