# Performance, Workspaces & Release

Execution performance and parallelism (RFC 0008), the workspace/monorepo model (RFC 0011), and
the distribution and release pipeline (RFC 0014), plus the warm-run budget spike that validated
RFC 0008's numbers against a synthetic repo. Each section below was originally its own document;
they are merged here per the consolidation recorded in
`.wayfinder/tickets/33-consolidation-decision.md`.

## RFC 0008: Performance and parallelism

**Status:** Accepted · **Depends on:** RFC 0001, 0004 · **Related:** ADR 0001 (Rust), ADR 0004 (cache)

### 1. Principles

1. **Parallel by default, serial by exception.** Every phase runs on all cores unless a
   documented reason says otherwise. The serial sections that remain (final graph patch, findings
   diff) are kept small enough that Amdahl's law cannot eat the budget.
2. **Determinism is non-negotiable.** Same inputs ⇒ byte-identical output, at any thread count,
   any scheduling. Parallelism that would trade determinism for speed is rejected — a pre-commit
   tool that flickers is a tool that gets uninstalled. Pattern everywhere: *parallel compute,
   deterministic reduce* (§4).
3. **The budget is measured, not a CI gate.** The RFC 0001 §5 phase budgets are checked by
   `cargo xtask bench` against fixture repos, with a regression threshold behind `--gate` (§7).
   Deliberately not wired into CI: the recorded baseline is one machine's numbers and does not
   travel across ephemeral runners, so the check is run by a human, on a stable machine, before
   and after a change expected to cost time (CONTRIBUTING.md "Benchmarks").
4. **Adaptive, not maximal — the design intent, not yet the engine's behavior.** Parallelism has
   fixed costs (pool wake-up, work splitting) that a small warm run pays regardless: the engine
   does not currently vary execution strategy by input size (§6). Speed is measured end-to-end,
   not by core utilization.

### 2. Parallelism map

| Phase | Strategy | Serial remainder |
|-------|----------|------------------|
| Discovery | parallel ignore-aware directory walk (ripgrep-style `ignore` crate); stat/hash candidates in parallel (blake3 is internally parallel for big files) | final change-set assembly (tiny) |
| Extraction | `rayon` par-iter over changed files; each file = one task (parse + extract facts); independent by construction — adapters are pure per-file | none |
| Cache load | mmap the graph snapshot (zero-copy, ADR 0004); facts fetches are read-only and concurrent | header validation |
| Resolution | per-import resolution over a sharded read-only path index; cross-language claims resolved concurrently | graph patch application (§4) |
| Plugins | independent plugins run concurrently within each hook stage; per-plugin fuel budgets already bound the tail | ordered sink merge (§4) |
| Analyses | **inter-analysis only**: independent analyses run concurrently through one registry-level `par_iter` (`analysis::run_all`, `analysis/mod.rs`) | each analysis's own body — reachability's BFS, duplicate detection's fingerprinting, CRAP's per-function pass — runs single-threaded; no analysis parallelizes internally today |
| Reporting | render is single-pass over sorted findings | — (fast by design) |
| Cache persist | **off the critical path**: snapshot written *after* results are printed, before exit; atomic temp-file + rename, crash-safe | — |

Exit latency note: the user sees findings at "report done", not "persist done" — persisting after
reporting buys ~100 ms of perceived latency for free. A killed process loses only cache warmth,
never correctness.

### 3. Data layout for speed

- **Interned everything**: paths, symbol names, specifiers → `u32` ids. Graph algorithms touch
  integers, not strings; strings exist only at extraction (in) and reporting (out).
- **Struct-of-arrays graph**: edges stored as columnar CSR-style adjacency (offsets + targets),
  rebuilt per snapshot. BFS over a contiguous `u32` column is cache-line friendly — reachability's
  BFS runs single-threaded today (§2), but the layout is what would make a frontier-parallel
  version worth building later.
- **Arena per run**: nodes/edges allocated in bump arenas, freed wholesale; no per-node
  allocation or refcounting on the hot path.
- **FxHash / no default SipHash** for internal maps (no untrusted-key DoS concern inside our own
  interned ids).
- These are internal (stability tier: Internal, contracts §5) — free to evolve under profiling.

### 4. Determinism under parallelism

The scheduling-dependent parts must never leak into ids, ordering, or output:

- **Two-phase id assignment**: parallel stages *collect* results keyed by content (path, symbol
  path), then a deterministic pass sorts and assigns interned ids. Ids never depend on completion
  order.
- **Deterministic reduce**: every parallel fold merges via associative, order-normalized
  operations (e.g. findings sorted by (category, path, span) before diffing/rendering; plugin
  sink contributions merged in declared plugin order, not arrival order).
- **Fixed tie-breaks**: any "first wins" rule (cross-language resolution claims) is defined over
  the sorted candidate list, not over race outcomes.
- **Enforcement**: the CI fixture matrix runs `--threads 1` vs `--threads N` and asserts
  byte-identical JSON output (alongside the existing `--no-cache` ≡ cached equivalence from
  RFC 0004 §5).

### 5. Thread pool policy

- Default pool size: physical cores (not logical — hyperthread gains are negligible for this
  workload and hurt tail latency on laptops).
- Overrides: `--threads N` flag > `KNDO_THREADS` env > config `[performance] threads`.
- One global rayon pool per process, built unconditionally by `Engine::open` on every run,
  ahead of any work (§6) — plugins and adapters never spawn their own threads (contract rule;
  WASM plugins are single-threaded by sandbox).
- `--threads 1` is a first-class supported mode (debugging, determinism checks, CI runners with
  noisy neighbors).

### 6. Adaptive execution

**Not implemented.** Warm pre-commit runs typically touch < 20 files, and fixed parallelism
costs (pool spin-up ~1–3 ms, task splitting, cache-line contention) can exceed the work itself
at that size — the design called for a work-item threshold (16 files to extract, tuned by
benchmarks) below which extraction and resolution would run inline on the main thread and the
pool would never initialize, with chunk sizes scaling with work items per core between threshold
and saturation. None of this is wired up: `Engine::open` calls `ensure_thread_pool`
unconditionally on every run (`crates/kndo-core/src/engine.rs`), and file extraction always
dispatches through `par_iter()` (`crates/kndo-core/src/graph/assemble.rs`) regardless of how many
files changed. A warm run touching one file pays the same pool spin-up and work-splitting cost as
a large batch. Were this built, the decision would need to be by measured work items, never
wall-clock feedback loops — adaptivity must also be deterministic (it changes performance, never
output; asserted by the §4 matrix).

### 7. Enforcement & tooling

- **Benchmark suite** (`cargo xtask bench`, deliberately not a CI gate): fixture repos at
  1k / 5k / 50k files; measured per scenario: cold full, warm no-op, warm 1-file change, warm
  100-file change, `--staged` on a realistic diff. Budgets: warm p95 < 500 ms @ 5k (the
  contract), cold < 10 s @ 5k; `--gate` fails the build on a >10% regression against the
  recorded baseline (`internal/perf-baseline.json`). That baseline records one machine and does
  not travel, so the gate is run by a human — before and after a change expected to cost time, on
  that same machine — never automatically: ephemeral CI runners of varying hardware would fail it
  for reasons unrelated to any real regression (CONTRIBUTING.md "Benchmarks").
- **Scaling check**: warm 100-file scenario must show ≥ 3× speedup at 8 cores vs 1 core (guards
  against silent serialization creeping in behind a lock) — read by hand alongside the benchmark
  suite above, same non-CI posture.
- **Microbenchmarks** (criterion) for the named hot paths: hash-and-compare pass, BFS round,
  winnowing window, snapshot load. Not gates; trend-tracked.
- **Profiling discipline**: optimizations land with a benchmark delta in the PR description, or
  they don't land — "should be faster" is not evidence.

### 8. Non-goals

- **No resident daemon for the budget.** A warm daemon (post-1.0 idea, ADR 0004) may make kndo
  *even* faster, but the 500 ms contract must hold from a cold process — pre-commit can't depend
  on a daemon being alive.
- **No `unsafe` for speed** outside the vetted dependencies (rkyv, memmap); kndo's own code
  stays safe Rust until a profile proves a specific bottleneck, decided case by case via ADR.
- **No speculative background work** (pre-warming, watching): kndo does nothing between
  invocations by design; that's what keeps it trustworthy in CI and pre-commit.

## RFC 0011: Workspaces and monorepos

**Status:** Accepted · **Depends on:** RFC 0001, 0002, 0004, 0005 · **Changes:** graph vocabulary (contracts §1)

### 1. Problem

The repos where kndo matters most are not single-package projects: they are npm/pnpm workspaces,
Cargo workspaces, Go multi-module repos, Gradle multi-project builds — often several ecosystems
in one tree (JS frontend + Go backend). Until now the docs said "library mode per package"
without defining what a package *is*, who owns each file, or what happens at package boundaries.
This RFC defines that model.

### 2. Vocabulary: `Package` is the workspace unit; the external thing is a `Dependency`

The graph node previously named `Package` (a declared external dependency) is renamed
**`Dependency`** — aligning it with the `dependency` subject kind and the dependency verdicts
that already used that word. **`Package`** now means what developers mean by it: a
manifest-defined unit of the workspace (npm package, crate, Go module, Gradle subproject, SPM
target). Contracts §1 is updated accordingly (`DependencyId`, `ImportsDependency`,
`Resolution::Dependency`). Renaming now is free; living with "package sometimes means lodash"
forever is not.

### 3. The model

- **Project** — the analyzed root. Contains one or more Packages.
- **Package** — one manifest + the file tree it governs. Adapters already parse manifests
  (RFC 0002 §2.5); `ManifestFacts` now also carries *identity and topology*: package name,
  workspace membership declarations (`workspaces` globs, `[workspace] members`, `go.work` uses,
  `settings.gradle` includes), publish signals (`private: true`, `publishConfig`, registry
  metadata), and entry points.
- **Ownership** — every file belongs to exactly **one** Package: the nearest manifest ancestor
  (adapters may refine where a toolchain's own rule differs). A repo with no manifest at all is
  one implicit Package — the single-project case is the monorepo model with n = 1, not a
  separate mode.
- **Derived edges** — `Package depends-on Package` edges are derived from cross-package file
  edges and manifest declarations; they power package-level queries (`kndo uses pkg:ui`),
  package rollups (§6) and package-level cycles (`cyclic` subject `package`).

### 4. Resolution & boundary enforcement

An import specifier may now resolve to a third target: an **internal package** (`workspace:*`
deps, path dependencies, tsconfig path aliases into a sibling, Go replace directives, Cargo
path deps). Resolution yields the concrete internal file — real edges, full reachability across
packages — *and* kndo validates the boundary contract both ways:

| Manifest vs reality | Finding |
|--------------------|---------|
| internal dep declared, no import resolves into that package | `unused` (subject `dependency`) — same verdict, remediation says "remove the workspace dep" |
| import resolves into a sibling package not declared in the importer's manifest | `undeclared` (subject `dependency`) — phantom internal dependency; breaks publishability and build graphs |

This table — and `Resolution::WorkspaceMember`'s edge derivation generally (both `ImportsFile`
*and* `ImportsDependency`, contracts §2) — assumes the ecosystem has a per-sibling declaration
contract to validate in the first place (npm's `workspaces`/`dependencies` entries, Cargo's
`[dependencies]` path entries). Not every language does: a Go module's own subpackages need no
`require` entry (a module cannot require itself), so an adapter resolving its own module's
internal imports uses plain `Resolution::File` instead — full `ImportsFile` reachability, no
`ImportsDependency` edge, and correctly no `undeclared` finding for a contract that doesn't exist
(docs/adapters/go.md §3). `WorkspaceMember` stays reserved for resolutions where a real
declaration contract exists to validate — a `go.work` sibling *module* (once supported) would
still be `WorkspaceMember`, since that's a genuinely separate module Go's own tooling tracks by
name via `go.work`'s `use` directives, just not via `require`.

#### The `deep-import` verdict (group `risk`, M3) — internal *and* external providers

An import that bypasses a provider package's declared entry points erodes a boundary someone
explicitly drew. **The provider does not have to be a workspace sibling** — the verdict covers
both cases with one definition, because the mechanism is identical:

- **Internal provider** (workspace sibling): `@org/app` importing `@org/ui/src/private/x`
  instead of `@org/ui` — the consumer now depends on the sibling's internal file layout, and
  the import breaks outright if the provider is ever published.
- **External provider** (a dependency): `import { helper } from "some-lib/dist/internal/utils"`
  — common in perfectly ordinary single-package apps. It often *works* only because a bundler
  is lax where Node's `exports` enforcement would refuse, and it breaks silently on the next
  library upgrade. The dependency's own manifest (already read for resolution) supplies the
  declared surface.

Three design rules keep the verdict signal, not noise:

1. **Contract-gated, so it is zero-config and self-opting.** The finding fires **only when the
   provider declares an explicit surface** (an `exports` map or the language's equivalent,
   reported by the adapter — from the sibling's manifest or the dependency's own). No declared
   surface = no declared boundary = no finding: monorepos where sibling deep imports are
   accepted practice never see noise, and `lodash/fp` is not a finding (lodash declares no
   `exports` map — its subpaths are deliberately open). Boundaries whose enforcement is
   *unconditional at build time* (Go `internal/`) are skipped outright; **partially** enforced
   boundaries (Node `exports`, which bundlers and legacy TS resolution routinely bypass) are
   exactly where the finding earns its keep — "works in webpack today, breaks in Node/jest
   tomorrow".
2. **One finding per (consumer package → provider) pair** — subject `package`, rollup spirit:
   "`@org/app` deep-imports `@org/ui` at 23 sites, touching 4 internal symbols", with sites and
   symbols in the evidence (capped, `elided` counted). That pair is the unit a migration is
   planned in; 23 line-level findings are not.
3. **Computed remediation, by case.** kndo has the graph, so the finding says which case each
   symbol is. Provider-internal *and* also reachable via the public surface → "switch the
   specifier to `@org/ui`" — trivially safe, the first candidate for `kndo clean` auto-fix
   post-1.0 (applies to external providers too when the symbol is re-exported publicly).
   Genuinely internal, internal provider → the exact subpath export to add (`"./testing"`), or
   extraction to a shared package. Genuinely internal, **external** provider → you don't own
   the surface: use the public equivalent, request the export upstream, or vendor the code —
   stated in that order.

Severity: warning (the gate means the provider explicitly declared the contract being
bypassed). Finding confidence = the underlying edge's confidence. The edges themselves are
recorded from M1 regardless (reachability must stay correct — deep-imported code *is* used);
the verdict lands in M3. The external-provider case is also listed with the dependency-hygiene
findings (RFC 0005 §5), since that is where a single-package app will meet it.

**Landed (M3), with one recorded boundary:** the internal-provider case is implemented
end-to-end — `ManifestFacts::{declares_surface, resolved_entries}` flow onto
`PackageNode::{declares_surface, surface}` at assembly, and `analysis/deep_import.rs`
implements all three rules (gate, pair rollup with capped site evidence, computed remediation
— the also-public-vs-genuinely-internal split computed per touched symbol via file-granular
surface-reachability, since a re-export chain from the entry is exactly an `ImportsFile`
path). The **external-provider case cannot fire yet**, by the gate's own logic rather than a
special case: evaluating it requires the provider's *own* manifest, which lives outside the
discovered tree (`node_modules/` is not walked — RFC 0008's discovery bounds), so
`declares_surface` is unknowable and the gate stays closed — silence, the safe direction.
Making it fire needs a provider-manifest peek at resolution time (its own
discovery/cache/purity design pass); tracked in ROADMAP as the remaining half of this
verdict, not silently absorbed.

### 5. Roots & library mode are per-package decisions

Each Package independently resolves its mode from manifest signals:

- **Published/library** (`private` absent, publish metadata, or a lib target): its public API
  is a production root — external consumers exist by definition. "Public API" is computed
  precisely (M6): manifest-declared entry files' surface-transitive exports, extended through
  whole-surface re-exports (`pub mod`/`export *` — assembly's library-surface fixpoint) and
  named re-exports (barrel indirection), and completed by the **surface-member closure**: a
  surface type's members whose rung is `surface_transitive` (RFC 0012 §6) are surface too — a
  `pub` method of a re-exported struct is consumer-callable API even with zero in-package
  references. Capped rungs (`pub(crate)`, Swift `internal`, Go `internal/` exports) never
  join the surface, keeping `unused`/`internal-only` at full precision for them.
- **Private/app** (`private: true`, bins, app targets): exports are *not* roots; an export is
  alive only if a real edge (same package or sibling) consumes it. Cross-package consumption
  keeps internal-package exports honest without root-inflation — this is where monorepo dead
  code hides, and it is exactly the `internal-only`/`unused` machinery already specified, now
  fed with correct roots.

**Not implemented.** A per-package config override — forcing a mode when manifest signals lie,
or skipping a category for one package — is conceivable but does not exist in any form today:
`config::LIVE_TABLES` carries no `package` table, `kndo init`'s template has no such section,
and there is no `PackageMode` type. Manifest signals are the only input; there is no override.

### 6. Verdicts at package granularity

The rollup ladder (RFC 0005 taxonomy rule 3) gains its natural top rung:
**symbol → file → directory → package.** A Package none of whose files are reachable rolls up
to one `unused` finding with subject `package` ("this whole workspace member is dead");
a Package consumed only by tests rolls up to `test-only:package`. `subject_kind` gains
`package`; findings gain an optional `package` field (owning package name) so CI and agents can
partition results without path arithmetic.

`kndo health` reports the global score plus a per-package breakdown (`--by-package`); weights
and formula are unchanged — the package axis is a *grouping* of the same penalties, not a new
metric.

### 7. Mixed ecosystems

Nothing special, by construction: Packages of different languages coexist as siblings; ownership
is per-manifest; dependency verdicts are per-manifest already; cross-language edges (RFC 0002
§4) compose with cross-package resolution unchanged. The Go backend importing nothing from the
JS frontend simply produces no edges between those packages.

### 8. Scoping runs & queries

- `kndo check [PATHS…]` already scopes by path; paths align with package boundaries naturally.
- Selectors: `pkg:<name>` addresses a Package; the external-dependency selector becomes
  `dep:<name>` (RFC 0007 updated). `describe pkg:@org/ui`, `used-by pkg:@org/ui`,
  `trace pkg:app pkg:legacy` work like any node.
- Diff modes need no changes: derived effects already cross package boundaries because edges do.

### 9. Cache & invalidation

Package topology is part of the graph key already (manifest content hashes, RFC 0004 §3) —
adding/removing/renaming a workspace member invalidates precisely through its manifest hash.
Per-package facts need no partitioning: the facts cache is content-addressed and
package-agnostic.

### 10. Non-goals (1.0)

Task-runner integration (Nx/Turbo/Bazel graphs are *build* graphs; kndo derives its own from
code), affected-package CI splitting (consumers can compute it from `Package depends-on` edges
in the JSON), tsconfig project-references deep integration (parking lot), versioning/release
concerns (changesets et al. own that).

## RFC 0014: Distribution and release

**Status:** Mechanism implemented, unexercised · **Depends on:** ADR 0006 (single static binary,
zero-config), ADR 0007 (product name `kndo`), RFC 0006 (output schema, exit codes), RFC 0010
(`kndo-action`) · **Ships:** M6

§§0-1 (license, governance) are done and live in this repo. §§2-3's pipeline (`.github/
workflows/ci.yml`, `.github/workflows/release.yml`, `install.sh`, `cliff.toml`,
`packaging/docker/Dockerfile`, `packaging/homebrew/kndo.rb.tmpl`) is written and passes static
validation (`actionlint`, shellcheck, YAML parse — see §7) but has never run for real: nobody has
pushed a `v*` tag yet. That first real tag push — a deliberate act, not something to trigger
speculatively — is this pipeline's actual test.

Adapted from a working distribution plan drafted for a sibling project (Yunta), reshaped around
kndo's actual crate layout and the decisions ADR 0006/0007 already made.

### 0. License: settled — Apache-2.0

The workspace `Cargo.toml` declared `license = "MIT"` while the repository's own `LICENSE` file
was the full Apache-2.0 text — a real mismatch that would have shipped false crate metadata to
crates.io as-is. Resolved: **Apache-2.0**, confirmed by the author. `Cargo.toml` now matches
`LICENSE`; `NOTICE` added (`Copyright 2026 The kndo Authors` — the generic "Project Authors"
phrasing common to Apache-2.0 projects that don't want to hardcode one individual's legal name;
swap it for a specific holder if that's preferred). `CODE_OF_CONDUCT.md` (Contributor Covenant
2.1) and `SECURITY.md` (GitHub's private vulnerability-reporting flow, no maintainer email
exposed) added — both link to `eulke/kondo` (the repo's *current* name — the same rule
`Cargo.toml`'s own `repository` field follows: GitHub's post-rename redirect makes the old name
resolve forever once the rename happens, while the new name 404s until it does).

**Ownership & context — confirmed.** kndo is a personal project: personal account, personal
device, on the author's own time, intended to go public once finished for anyone to use. That
settles the separation half of the pre-first-release checklist (personal account, personal time,
no commits routed through employer infrastructure — already true throughout this repo's
history). One item stays the author's own to verify, not something confirmed here one way or
the other: if there's an employment agreement in the picture, its IP/invention-assignment clause
should be checked against a personal OSS project before the first public release (get it in
writing if the text is ambiguous). This blocks only the *first public release*, not the
engineering in the rest of this RFC.

### 1. License & repo governance

`LICENSE` (Apache-2.0, now matching `Cargo.toml`), `NOTICE`, `CODE_OF_CONDUCT.md`, `SECURITY.md`,
and `CONTRIBUTING.md` (already existed) all present. Apache-2.0 over the alternatives, same
reasoning as the Yunta plan: an express patent grant (what corporate legal reviews before
approving a tool), maximum permissiveness for adoption, and it's the de facto default for Rust
tooling — versus strong copyleft (unnecessary friction for a tool that runs alongside proprietary
code) and source-available/BSL-style licenses (undercut the product's own pitch — a verifiable
audit engine is a harder sell if it can't be freely inspected and run).

### 2. Distribution channels, in implementation order

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

### 3. Release pipeline mechanics

The only human gesture is pushing a tag `vX.Y.Z`. **The GitHub Release is the source of truth**
— no secondary channel compiles anything; every one of them consumes that release's artifacts.
If a secondary channel's job fails, the release still ships and that channel alone retries: a
stale tap is an isolated problem, never a broken release.

#### 3.1 Jobs

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
Each job packages `kndo-<tag>-<target>.tar.gz` (`.zip` on Windows) with the binary, `LICENSE`,
and `README.md`, all nested under a single directory named for the archive — so every consumer
strips exactly one component.

**The table above is `xtask::package::TARGETS`, and the artifact's name and layout are
`xtask::package::artifact`.** This is not a description of what the workflow does; the workflow
calls `cargo xtask package`, and `xtask/tests/release_channels.rs` checks the workflow matrix,
the installer, the Action, the Homebrew template and the install docs against that one
definition. The arrangement it replaced had five independent spellings, of which three were
wrong at the same time — the Action asked for `-unknown-linux-gnu` triples no release has ever
built and dropped the tag's leading `v`; the installer extracted correctly and then took the
binary from the extraction root, where the staged directory means it is not; the docs'
copy-pasteable one-liner had both faults. `<tag>` is the git tag verbatim, `v` included: that is
what the producer writes, so a consumer that strips it names a file that does not exist.

**`release`** (depends on all 5 builds): collects artifacts, computes SHA-256 into
`checksums.txt`, generates notes via [git-cliff](https://git-cliff.org/) (confirmed: the tool
Yunta uses for the same job), creates the GitHub Release. git-cliff is driven by a `cliff.toml`
at the repo root and categorizes commits by type/scope prefix (`feat:`, `fix:`, `docs(scope):`,
…) into changelog sections — but kndo's own history isn't there yet: of the last 140 commits,
only 63 (~45%) carry a conventional-commit prefix; the rest are plain descriptive subjects
(`Add WASM bridge for the Plugin graph-mutation hooks`, `M5: adapter CSS + SCSS …`). Two
consequences, not one silent assumption:

- Going forward, commits should carry a real `type(scope):` prefix if their subjects are meant
  to sort into git-cliff's changelog groups — this is a discipline change for the rest of M6 and
  beyond, not retroactive.
- `cliff.toml`'s `commit_parsers` needs an explicit catch-all group (typically mapped to
  "Other" or "Miscellaneous") for the un-prefixed history already in the repo — rewriting past
  commit messages to force them into the convention is out of scope (rewrites shared history);
  the config should absorb the mixed reality, not paper over it by assuming a clean log.

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

#### 3.2 The installer (`install.sh`)

One versioned POSIX script, served from GitHub raw or a project domain:

1. Detect platform via `uname -s`/`uname -m`, map to a target (`Darwin/arm64` →
   `aarch64-apple-darwin`, `Linux/x86_64` → `x86_64-unknown-linux-musl`, …). An unsupported
   platform errors with the supported list.
2. Resolve version: `latest` from the releases API, or `KNDO_VERSION` for a pinned install.
3. Download the artifact and `checksums.txt` from the project's releases, or from
   `KNDO_BASE_URL` — an internal mirror or a staging directory served over HTTP. That override
   is also the seam CI installs through: the `install-from-artifact` job builds the musl binary,
   packages it with the same `xtask package` a release runs, serves the result over localhost
   and installs from it, so the layout contract is exercised end to end on every push instead of
   for the first time on a tag. It asserts the corrupt case too — a tampered archive must abort
   and leave nothing behind.
4. **Verify SHA-256 before extracting** — a mismatch aborts without writing anything.
5. Extract to `~/.local/bin` (or `KNDO_INSTALL_DIR`), stripping the archive's staged directory —
   no sudo, nothing under a system directory.
6. If that directory isn't on `PATH`, print the exact line to add to the shell profile.
7. Close with `kndo --version`, suggesting `kndo doctor` as the next command — which already
   exists and already answers "what did it detect and why" (ADR 0006's own mandatory companion).

Requirements: idempotent (reinstalling replaces cleanly), no dependency beyond
`curl`/`tar`/`shasum`, and `set -eu` — any failing step halts the install rather than leaving the
system half-configured.

#### 3.3 GitHub Action

RFC 0010 already specifies `kndo-action`'s own behavior in full — nothing in this RFC changes
it. If `setup-kndo` (§2 item 5) gets built, it follows the same shape other `setup-*` actions
use: composite `action.yml` (shell steps, no JS/container), an Actions-cache lookup keyed by
`kndo-<version>-<runner-os>-<arch>`, and mutable major-version tags (`v1` → latest `v1.x`) since
that's what workflows actually pin to.

### 4. Versioning & release quality

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

### 5. Public presence

A README that shows a real `kndo check` run before anything else (mirrors the existing product
instinct: zero-config first, configuration only as tuning — ADR 0006), a static docs site
generated from the same contract docs already in this repo (M6 already lists this), and
first-party ecosystem plugins (when built) living in their own repos — physical separation
reinforcing that the product is the engine, not a bundle.

### 6. Sustainability — explicitly out of scope here

The Yunta reference plan pairs its distribution RFC with a monetization angle (a separate
team-server project, kept fully outside the free/open engine). Nothing here assumes kndo has, or
wants, an analogous plan — that's a business decision with no engineering dependency on anything
above, and isn't invented on its behalf. If/when there's an answer, it belongs in its own
section, added deliberately, not backfilled from a template.

### 7. Implementation status & what's still needed

Landed this pass: `.github/workflows/ci.yml` (test/clippy/fmt/dogfood gate, also `workflow_call`-
reusable so `release.yml` doesn't duplicate it), `.github/workflows/release.yml` (the full §3.1
job graph: `test` → `build` (5-target matrix) → `release` (checksums + git-cliff notes +
`softprops/action-gh-release`) → `publish-crates` / `update-tap` / `publish-container` in
parallel), `install.sh` (§3.2 — manually exercised end to end against a faked local release:
download, checksum-verify, extract, PATH check, and the checksum-mismatch abort path, all
correct), `cliff.toml` (§3.1's catch-all group for un-prefixed history), `packaging/docker/
Dockerfile`, `packaging/homebrew/kndo.rb.tmpl`. All workflow YAML passes `actionlint` (including
its shellcheck pass over every `run:` block) clean.

**Not yet, and can't be from here — this is the punch list before a tag push does anything real:**

- **`publish = false`** (workspace `Cargo.toml`, ADR 0007: "until 1.0") — deliberate, still true.
  `publish-crates` will fail immediately, by design, until this flips.
- **No internal dependency declares a `version =`** — every one is `{ path = "../foo" }` alone.
  crates.io requires a version on every dependency of a published crate, path or not. Needs
  fixing (mechanically simple — `version = "0.1.0"` alongside each `path =`) whenever the
  workspace actually approaches publishable, alongside flipping `publish`.
- **Secrets that don't exist yet**: `CARGO_REGISTRY_TOKEN` (crates.io, for `publish-crates`).
  `HOMEBREW_TAP_TOKEN` (a PAT with push access to the tap repo, for `update-tap`) —
  `publish-container` needs no extra secret, it authenticates to GHCR with the workflow's own
  built-in `GITHUB_TOKEN`.
- **The tap repo itself doesn't exist**: `update-tap` pushes to `eulke/homebrew-tap`, assumed
  already created. Not created here — a new public repo is a real, visible action, the author's
  call to make when ready, same posture as the pending `eulke/kndo` rename.
- **Untested against a real GitHub Actions run.** Static validation (`actionlint`, shellcheck,
  YAML parse, and a local dry-run of `install.sh`'s core logic against a faked release) is real
  verification, but it is not the same as a live run — cross-compilation quirks, GHCR
  permissions, and the `build-contexts` multi-arch Docker path in particular are the pieces most
  likely to need a real push to shake out. The first `v*` tag push is that test; it should be a
  deliberate act by the author; the pipeline aborting or partially failing on that first real run
  is expected and fine — that's what the "one bad channel doesn't break the release" design is
  for (§3).

### 3.3 Windows is not a release target — decided by attempting the build

The target table shipped `x86_64-pc-windows-msvc` from the start, and **nothing had ever built
it**. The `cross-platform` CI job added in this milestone did, and kndo's own code never got as
far as compiling:

```
cl : Command line error D8021 : invalid numeric argument '/Wno-unused-parameter'
error occurred in cc-rs: ... tree-sitter-scss ... scanner.c
```

`tree-sitter-scss` 1.0.0's build script is three lines of `cc::Build` with
`c_config.flag("-Wno-unused-parameter")` — unconditional, not `flag_if_supported`, no MSVC
branch. MSVC's `cl.exe` rejects it. Checked: it is the **only** grammar in the tree that does
this (every other `tree-sitter-*` build script is clean under MSVC), and 1.0.0 is the only
version published.

So a tag push would have run the five-target matrix and failed on Windows, after the other four
succeeded — the exact failure mode this RFC's "one producer, nothing that first runs at tag
time" rule exists to prevent, caught by the job that rule asked for.

**Decision: drop the target.** Vendoring the grammar with a corrected build script would work
and was considered; it trades a permanent maintenance obligation (a C grammar copied into this
repository, diverging from upstream forever) for a platform with no measured demand yet. WSL runs
the Linux musl archive unchanged, which is what the install docs now say.

What went with it, because none of it had a user left: the `Zip` archive variant, the `write_zip`
producer, the `zip` dependency, and the `.exe` binary-name branch. `Archive` stays a type rather
than collapsing into nothing — "a target declares how it is packed" is the shape the four
consumers agree with, and it is where a second format slots back in.

Restoring Windows is not a row in the table: it needs the grammar problem solved, an archive
format back, and the installer's platform detection extended.
`every_released_target_is_a_tar_gz_named_kndo` fails first and says so.

## Spike 0001: Performance validation

**Date:** 2026-08-18 · **Status:** Done · **Validates:** RFC 0001 §5, ADR 0004, RFC 0008
**Code:** `spikes/perf/` (disposable; this report is the durable artifact)

### Setup

Synthetic repo: **5 000 TypeScript files, 37.7 MB** (~8 KB/file: ~9 imports, 15 interfaces,
30 functions each — realistic import locality). Synthetic graph: **250k symbols, 1.0M edges**
(50 syms/file, 4 edges/sym, small-world locality). Hardware: **4-core** Linux container
(conservative vs. developer laptops; numbers below are the second run, warm fs cache — first
run within 15%).

### Measured vs. budget

| Phase | RFC 0001 §5 budget (warm) | Measured | Verdict |
|-------|--------------------------|----------|---------|
| Discovery (stat-scan, no rehash) | 80 ms | 21.5 ms | ✅ 3.7× headroom |
| Extraction (20-file diff, sequential) | 100 ms | 42.1 ms | ✅ 2.4× |
| Graph load (mmap CSR, 5 MB, touch all pages) | 100 ms | **1.1 ms** | ✅ 90× |
| Analyses (recolor via BFS) | 150 ms | 8.6 ms | ✅ 17× |
| **Warm composite** | **500 ms** | **73.3 ms** | ✅ **6.8× headroom** |

Cold path (build-the-cache run): walk 12 + read 68 + blake3 6 + parse-all 3 038 + graph build 8
+ persist 35 + 2×full-BFS 19 ≈ **3.2 s** against the < 10 s budget. ✅

### Key findings

1. **The 500 ms contract is comfortably physical.** Even on 4 cores with everything sequential
   in the warm path, the composite is ~75 ms — the budget survives a 3–4× underestimate in
   extraction realism (see risks) and still holds on weaker hardware.
2. **mmap CSR load is effectively free (1 ms for 1M edges).** ADR 0004's zero-copy bet is
   validated — and even the fallback (bincode deserialize: 5.7 ms) fits the 100 ms line 17×
   over, so the cache format choice is not on the critical path of the promise.
3. **Full reachability is cheap enough that incrementality is a luxury, not a lifeline.**
   A complete 250k-symbol/1M-edge recolor costs 8–15 ms. The dirty-region machinery (RFC 0004
   §5) still matters for extraction (don't re-parse) but analysis-side, the §5 fallback
   ("recompute fully past 30% dirty") could fire on *every* run and stay in budget. This
   de-risks the trickiest code in RFC 0004: analysis incrementality can ship late (or
   partially) without endangering the contract.
4. **Parsing dominates the cold path** (3.0 s of 3.2 s; ~1 650 files/s/4-cores with a full-tree
   walk). Cold time scales linearly with repo size: a 50k-file repo extrapolates to ~32 s cold
   — over the 10 s aspiration at that scale, but cold runs happen once per clone; acceptable,
   and grammar-level extraction (queries instead of naive full walks) has known optimizations.
5. **blake3 hashes the entire 37.7 MB repo in 6 ms** — content addressing costs nothing;
   `--staged` hashing of a few files is unmeasurable.

### Risks the spike does NOT retire

- **Extraction realism:** the proxy visits every tree node; real extraction runs tree-sitter
  *queries* + fact construction, plausibly 2–3× the walk. Even 3× on the 20-file warm diff
  (~125 ms) fits, but M1's conformance fixtures must re-measure with real `FileFacts`.
- **Graph size realism:** 5 MB adjacency vs. a real snapshot with interned strings, spans,
  findings — likely 30–80 MB. mmap load scales with *touched* pages (stays ~free); bincode
  would scale linearly (still fits at ~50 ms). Watch at M2.
- **Resolution cost is unmeasured** (specifier → target over an index). Budgeted inside the
  150 ms analyses line; needs measurement when the resolver exists (M1).
- **Single-core CI runners:** warm composite is nearly sequential already (~75 ms holds);
  cold extrapolates to ~12 s — the M2 benchmark suite should include a `--threads 1` cold run
  to keep this honest.

### Decisions fed back into the docs

- RFC 0001 §5 budget table: **validated, unchanged** (annotated as spike-checked).
- RFC 0004: incremental analysis remains specified, but implementation may land as
  "full recolor always" first (finding 3) — correctness gates (`--no-cache` ≡ cached) unchanged.
- ADR 0004 (rkyv/mmap): confirmed; bincode remains an acceptable fallback for small artifacts.
