# Contributing

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

## Branching — Gitflow

- `main` — always releasable; only tagged versions land here; merges only from `release/*` or
  `hotfix/*`.
- `develop` — integration branch; default base for new work.
- `feature/<short-name>` — branches off `develop`, merges back via PR.
- `release/<version>` — cut from `develop` to stabilize before a release; merges to `main` and
  back to `develop`.
- `hotfix/<short-name>` — branches off `main` for urgent fixes; merges to `main` and `develop`.

Delete a branch once merged.
