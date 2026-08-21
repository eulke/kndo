# RFC 0014 — Distribution, Licensing & Release Pipeline

**Status:** Draft · **Depends on:** ADR 0006 (single static binary, zero-config), ADR 0007
(product name `kndo`), RFC 0006 (output schema, exit codes), RFC 0010 (`kndo-action`) ·
**Ships:** M6

Adapted from a working distribution plan drafted for a sibling project (Yunta), reshaped around
kndo's actual crate layout and the decisions ADR 0006/0007 already made. Two sections are left
open on purpose — they're calls only the author can make, not engineering decisions.

## 0. Two open items, before anything else

**License mismatch — needs a decision now, not at release time.** The repository's `LICENSE`
file is the full Apache-2.0 text, but every crate's `Cargo.toml` (`license.workspace = true` →
workspace `license = "MIT"`) declares MIT. Whichever one is wrong, it's wrong *today*, in the
crate metadata that would ship to crates.io as-is. This RFC recommends **Apache-2.0** (§2), for
the same three reasons the Yunta plan gave: an express patent grant (what corporate legal
reviews before approving a tool), maximum permissiveness for adoption, and it's the de facto
default for Rust tooling (often dual MIT/Apache-2.0, a valid variant if broader interoperability
is wanted). But the fix is one line in `Cargo.toml` plus reconciling `LICENSE` — trivial to
apply once the *choice* is confirmed, not before.

**Ownership & context — the author's call, not an engineering one.** If kndo is written on
personal time under a personal account, the pre-first-release checklist is: confirm the
employment agreement's IP/invention-assignment clause doesn't reach a personal OSS project (get
it in writing if the text is ambiguous), and keep the separation clean and demonstrable (personal
account, personal time, no commits routed through employer infrastructure). If the situation is
different, this section doesn't apply and should say what does. Either way this blocks the
*first public release*, not the engineering in the rest of this RFC — nothing below depends on
the answer.

## 1. License & repo governance

Once §0's license question is resolved: `LICENSE` (already Apache-2.0 text — reconcile or swap),
`NOTICE`, `CONTRIBUTING.md` (already exists), `CODE_OF_CONDUCT.md`, `SECURITY.md` (a private
vulnerability-reporting channel) present from the first public release. Rejected alternatives,
same reasoning as the Yunta plan: strong copyleft (unnecessary friction for a tool that runs
alongside proprietary code) and source-available/BSL-style licenses (undercut the product's own
pitch — a verifiable audit engine is a harder sell if it can't be freely inspected and run).

## 2. Distribution channels, in implementation order

1. **GitHub Releases with precompiled binaries** — the primary channel. Linux x86_64/arm64
   static (musl — runs on any distro without depending on the host's glibc), macOS x86_64/arm64,
   Windows x86_64. Every release publishes checksums and generated notes.
2. **One-line installer** (`curl … | sh`) — detects platform, downloads the matching artifact,
   verifies the checksum, installs to `~/.local/bin`. Versioned in-repo, served from GitHub raw
   or a project domain (§4.2).
3. **`cargo install kndo-cli`** — for anyone with a Rust toolchain already. Per ADR 0007 the
   binary crate is `kndo-cli` (the `kndo` name itself is the library/distribution crate,
   `crates/kndo`), so this is the one asymmetry to state prominently in install docs: `cargo
   install kndo-cli` gets you the `kndo` binary, not `cargo install kndo`.
4. **Homebrew** — a personal tap (`<user>/tap`) first; homebrew-core only once there's real
   adoption to justify it. ADR 0007 found no existing formula at decision time — unverified risk,
   not a blocker.
5. **`kndo-action` / a leaner `setup-kndo`.** RFC 0010's `kndo-action` already resolves and
   downloads a pinned binary internally as part of running + reporting — that covers the "gate
   PRs" use case end to end. A separate, minimal `setup-kndo` (mirroring `actions/setup-node`:
   installs and caches the binary, adds it to `PATH`, nothing else) is worth adding *if* real
   usage shows people want `kndo` as a plain step in their own custom workflow rather than
   through the bundled reporting action — not assumed necessary here, a real follow-up decision
   once `kndo-action` ships and gets used.
6. **Container image** — `kndo` + git + a minimal toolchain, for pipelines that prefer an image
   to an install step.
7. **winget / scoop** — only if real Windows-user demand shows up.

This ordering matches ADR 0006's existing commitment ("installable via cargo, homebrew, an npm
shim, and curl script") and M6's current one-liner — this RFC is that bullet's actual plan, not
a change of direction. The npm shim ADR 0007 already priced in (`npm i -g kndo-cli`, since the
bare `kndo` name is taken by an unrelated package) slots in as a variant of channel 3, published
alongside it rather than as a separate pipeline stage.

## 3. Release pipeline mechanics

The only human gesture is pushing a tag `vX.Y.Z`. **The GitHub Release is the source of truth**
— no secondary channel compiles anything; every one of them consumes that release's artifacts.
If a secondary channel's job fails, the release still ships and that channel alone retries: a
stale tap is an isolated problem, never a broken release.

### 3.1 Jobs

**`test`** (gates everything else): `cargo test --workspace --all-features`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`, and —
kndo's own equivalent of Yunta's "factory packs pass end-to-end" gate — `kndo check` running
clean against kndo's own dogfood corpus (the same real-CLI check the local pre-commit hook
already runs on every commit in this repo, RFC 0010's pre-GA dogfood workflow). If this fails,
there is no release.

**`build`** (5-target matrix, parallel):

| Target | Runner | How |
|---|---|---|
| `x86_64-unknown-linux-musl` | ubuntu | `cross build --release` |
| `aarch64-unknown-linux-musl` | ubuntu | `cross build --release` |
| `x86_64-apple-darwin` | macos | native `cargo build --release` |
| `aarch64-apple-darwin` | macos | native `cargo build --release` |
| `x86_64-pc-windows-msvc` | windows | native `cargo build --release` |

musl for Linux because it produces a real static binary independent of the host's glibc; macOS
and Windows build on their own runners because cross-compiling to them costs more than it saves.
Each job packages `kndo-<version>-<target>.tar.gz` (`.zip` on Windows) with the binary,
`LICENSE`, and `README.md`.

**`release`** (depends on all 5 builds): collects artifacts, computes SHA-256 into
`checksums.txt`, generates notes, creates the GitHub Release. Notes generation from conventional
commits is the Yunta plan's mechanism — kndo would need to adopt (or confirm it already follows)
a commit-message convention for this to apply verbatim; flagged here rather than assumed.

**Secondary jobs** (depend on `release`, parallel with each other):

- **`publish-crates`** — publishes to crates.io **in dependency order**, waiting for index
  availability between steps (crates.io rejects a crate whose dependencies aren't indexed yet):
  `kndo-core` → `kndo-adapter-toolkit` → each `kndo-adapter-*` and `kndo-plugin-api` → `kndo` →
  `kndo-cli`. Requires a shared workspace version and exact-version internal dependencies (the
  workspace already uses `version.workspace = true` throughout, so this is close to free).
- **`update-tap`** — renders the Homebrew formula from a template (version, artifact URLs and
  SHAs already resolved) and commits it to the tap repo with a dedicated token. The formula only
  downloads the binary; it never compiles on the user's machine.
- **`publish-container`** — multi-arch (amd64/arm64) build from a minimal base image, **copying
  the already-built binary** (never recompiling), plus git and a minimal toolchain. Publishes to
  GHCR tagged `latest`, `X.Y.Z`, and `X.Y`.

### 3.2 The installer (`install.sh`)

One versioned POSIX script, served from GitHub raw or a project domain:

1. Detect platform via `uname -s`/`uname -m`, map to a target (`Darwin/arm64` →
   `aarch64-apple-darwin`, `Linux/x86_64` → `x86_64-unknown-linux-musl`, …). An unsupported
   platform errors with the supported list.
2. Resolve version: `latest` from the releases API, or `KNDO_VERSION` for a pinned install.
3. Download the artifact and `checksums.txt`.
4. **Verify SHA-256 before extracting** — a mismatch aborts without writing anything.
5. Extract to `~/.local/bin` (or `KNDO_INSTALL_DIR`) — no sudo, nothing under a system directory.
6. If that directory isn't on `PATH`, print the exact line to add to the shell profile.
7. Close with `kndo --version`, suggesting `kndo doctor` as the next command — which already
   exists and already answers "what did it detect and why" (ADR 0006's own mandatory companion).

Requirements: idempotent (reinstalling replaces cleanly), no dependency beyond
`curl`/`tar`/`shasum`, and `set -eu` — any failing step halts the install rather than leaving the
system half-configured.

### 3.3 GitHub Action

RFC 0010 already specifies `kndo-action`'s own behavior in full — nothing in this RFC changes
it. If `setup-kndo` (§2 item 5) gets built, it follows the same shape other `setup-*` actions
use: composite `action.yml` (shell steps, no JS/container), an Actions-cache lookup keyed by
`kndo-<version>-<runner-os>-<arch>`, and mutable major-version tags (`v1` → latest `v1.x`) since
that's what workflows actually pin to.

## 4. Versioning & release quality

The binary is semver. M6's own Exit criteria already name **three contract surfaces** needing
declared semver 1.0 commitments — this RFC reads that as: the JSON output schema
(`schema_version`, RFC 0006 §4 — already independently versioned from `kndo_version` today), the
WASM ABI packages (`kndo:adapter@0.1.0` / `kndo:plugin@0.1.0`, wasm-abi.md §8 — already
independently versioned from each other and from the binary), and the CLI surface itself
(flags, exit codes, `kndo.toml` schema). All three already follow "versioned independently, N
and N-1 supported" in spirit; this milestone is declaring the 1.0 commitment on top of a pattern
already in place, not inventing one.

Non-negotiable per release: install verified in a clean container per platform, `kndo doctor`
suggested as the first post-install command, and the dogfood/benchmark suite green in the `test`
job (§3.1) — kndo's own equivalent of "factory packs pass end-to-end."

## 5. Public presence

A README that shows a real `kndo check` run before anything else (mirrors the existing product
instinct: zero-config first, configuration only as tuning — ADR 0006), a static docs site
generated from the same contract docs already in this repo (M6 already lists this), and
first-party ecosystem plugins (when built) living in their own repos — physical separation
reinforcing that the product is the engine, not a bundle.

## 6. Sustainability — explicitly out of scope here

The Yunta reference plan pairs its distribution RFC with a monetization angle (a separate
team-server project, kept fully outside the free/open engine). Nothing here assumes kndo has, or
wants, an analogous plan — that's a business decision with no engineering dependency on anything
above, and isn't invented on its behalf. If/when there's an answer, it belongs in its own
section, added deliberately, not backfilled from a template.
