# ADRs

## ADR 0001: Rust for the core

**Status:** Accepted · **Date:** 2026-08-18

### Context
kndo's defining constraint is a < 500 ms warm run (pre-commit path) over multi-thousand-file
repos, distributed as a tool users install once and trust everywhere (macOS/Linux/Windows, CI
containers, developer laptops).

### Decision
Implement kndo in Rust: single static binary, no runtime, rayon for data-parallel extraction,
first-class tree-sitter bindings, mature WASM hosting (wasmtime) for the plugin tier, memory
safety for a tool that parses untrusted input.

### Consequences
- Fast cold starts (~ms) — essential for pre-commit; trivial distribution (one file per platform,
  installable via cargo/brew/npm shim/curl).
- Adapter/plugin authors targeting the WASM tier can use other languages; only first-party
  (compiled-in) adapters must be Rust.
- Slower iteration than a scripting language — mitigated by settling contracts in docs first
  (this repo's process) and by the conformance-fixture harness catching regressions cheaply.

### Alternatives
Go (fine, but weaker parser/WASM ecosystem and no rayon-grade data parallelism story);
TypeScript/Node (startup + memory costs incompatible with the 500 ms contract, would also make
the analyzer share a runtime with the code it analyzes).

## ADR 0002: tree-sitter as the universal parsing layer

**Status:** Accepted · **Date:** 2026-08-18

### Context
Eight languages at launch, "adding a language must be easy", parsing must tolerate broken code
(pre-commit runs on work-in-progress), and extraction is on the hot path.

### Decision
All first-party adapters parse with **tree-sitter** grammars. The adapter contract does not
mandate tree-sitter (an adapter only owes `FileFacts`), but the shared adapter toolkit
(`kndo-adapter-toolkit`: query helpers, token normalization, complexity walker) is built around
it, making tree-sitter the paved road.

### Consequences
- Uniform mental model: an adapter is largely grammar + a set of tree-sitter queries + resolver
  logic. Error-tolerant parsing comes for free; grammars exist for all launch languages.
- Precision ceiling: tree-sitter is syntactic. Semantic facts (type-directed dispatch, implicit
  imports) are approximated → this is why edges carry `Confidence` (RFC 0002 §5) instead of
  pretending precision. A post-1.0 "deep mode" may layer compiler-grade resolvers per language.
- Grammar versions pin into each adapter's `facts_schema_version`, keeping cache invalidation
  honest when grammars update.

### Alternatives
Per-language native parsers (SWC, syn, go/parser…): faster and more precise individually, but
N toolchains, N AST shapes, and no shared query layer — the "easy new language" goal dies.
Chosen escape hatch: the contract permits a native-parser adapter where it matters, without
changing the core.

## ADR 0003: First-party adapters compiled in; third-party extensions via WASM

**Status:** Accepted · **Date:** 2026-08-18

### Context
Languages and plugins must be pluggable (RFC 0002/0003), but Rust has no stable native ABI, and
`dlopen` plugins would break the single-static-binary distribution story, complicate cross-platform
support, and execute untrusted code unsandboxed.

### Decision
Two tiers behind the same traits:
1. **Compiled-in** — the eight launch adapters and first-party plugins are crates statically
   linked into the `kndo` binary, selected/activated at runtime.
2. **WASM components** — third-party adapters and plugins target `kndo-plugin-api`, a versioned
   WIT/component-model ABI hosted by wasmtime, sandboxed (no ambient fs/net; host-mediated file
   access; fuel + time budgets).

The native traits are the source of truth; the WASM ABI is a generated bridge over them, so an
extension can be developed natively and shipped as WASM without rewrites.

### Consequences
- Hot path stays native and monomorphized — WASM cost is only paid by repos that add external
  extensions, and the sandbox protects the 500 ms budget (over-budget extension ⇒ disabled + diagnostic).
- The ABI must be versioned and conservative from day one; it ships in M5 (ROADMAP), after the
  contracts have survived several first-party adapters.
- "Pluggable" never means "recompile kndo": external languages are possible without a core PR —
  but first-party quality bar stays higher (conformance fixtures required for both tiers).

### Alternatives
`dlopen`/cdylib (ABI fragility, no sandbox); subprocess extensions with an IPC protocol (clean
isolation but per-file IPC overhead threatens the budget; kept as a fallback idea if WASM proves
limiting); everything-WASM including first-party (pointless overhead on the default path).

### Implementation status

Both tiers' WASM bridges are implemented; see
[CONTRACTS.md](CONTRACTS.md#wasm-abi) §5 for the current, normative ABI surface
and compliance requirements. This ADR records the decision, not the implementation's state.

## ADR 0004: Cache: content-addressed binary snapshots under `.kndo/`

**Status:** Accepted · **Date:** 2026-08-18

### Context
The 500 ms warm budget allots ~100 ms to loading the previous graph (RFC 0001 §5). The cache must
be disposable, per-clone, corruption-tolerant, and keyed so that any input change invalidates
exactly its dependents (RFC 0004 §3).

### Decision
- **Location:** `.kndo/cache/` at project root, gitignored; only `baseline.json` (a sibling,
  not in `cache/`) is committed.
- **Hashing:** blake3 for all content addressing (parallel, collision-safe; also used for
  duplicate-asset detection so hashes are computed once).
- **Serialization:** `rkyv` (zero-copy archival) for graph snapshots (`graphs/<key>.bin` —
  content-addressed like facts entries, so the several tree states diff modes assemble each run
  coexist instead of evicting one another), so loading is mmap + validate rather than
  deserialize; `bincode` for small per-file facts entries where zero-copy buys nothing.
  Findings are not persisted — diff modes recompute both the before- and after-tree findings
  from their (warm) graph snapshots each invocation, then diff in memory (RFC 0004 §6). Every
  artifact carries `(magic, core schema version, writer version)`; any mismatch ⇒ silently
  rebuild that layer (cold), never migrate in place.
- **Blob-hash sidecar:** `blob-hashes.bin`, a `git blob id → blake3` map grown on every
  git-tree discovery. Sound because a git blob id is itself a content address (same id ⇒ same
  bytes ⇒ same blake3); a hit lets diff modes skip streaming a blob's content entirely — the
  dominant warm-diff cost — with bytes re-fetched lazily only on a facts-cache miss.
  Corrupt/absent ⇒ empty map: everything is re-fetched, slower but never wrong.
- **Concurrency:** single-writer advisory lock; concurrent runs degrade to read-only cache use.

### Consequences
- Branch switches and stashes stay warm (content addressing ignores paths' mtimes and history).
- rkyv couples layout to exact type definitions — acceptable because the cache is explicitly
  disposable and versioned; no migration code will ever be written.
- Facts store is pruned by LRU cap (default 256 MB) to bound disk usage on long-lived clones.

### Alternatives
SQLite (robust, but row-oriented access pattern and query layer add latency and a C dependency
for no query need we have); JSON/MessagePack snapshots (parse cost blows the budget on large
graphs); OS-level shared daemon keeping the graph hot (post-1.0 idea; a daemon complicates the
trust and lifecycle story and shouldn't be *required* to meet the budget).

## ADR 0005: Coverage is ingested, never measured

**Status:** Accepted · **Date:** 2026-08-18

### Context
CRAP (RFC 0005 §10) needs per-function coverage. Measuring coverage requires *executing* tests —
incompatible with a < 500 ms static tool and with kndo's non-goal of being a coverage tool.

### Decision
kndo consumes existing coverage reports via `ingest_coverage` plugins (RFC 0003 §2). Shipped
formats (the `kndo-plugin-coverage` built-ins): **lcov** (lingua franca: jest/vitest/nyc,
llvm-cov, gcov), **Cobertura XML** (coverage.py, .NET, istanbul's cobertura reporter),
**JaCoCo XML** (Java/Kotlin), and **Go coverprofile** (`go test -coverprofile`); other
formats load as external WASM components through the `coverage-ingester` world. Reports are
located by config (`[plugins.<id>] report`, globs included for monorepos) or well-known
paths, matched to files by path (host-side root and package-table rebasing lands absolute
and module-qualified report paths), mapped to functions by line ranges.

Freshness policy: a report older than `max-age` (default 7 days; `[plugins.<id>] max-age`
overrides per plugin) is ignored **with a diagnostic** — stale certainty is worse than
declared uncertainty. (A report referencing missing files simply matches nothing — silence,
never a wrong file.) Without usable coverage, CRAP degrades as specified in RFC 0005 §10
(cov = 0, flagged `coverage: none`).

### Consequences
- kndo stays static and fast; teams get CRAP "for free" if any coverage already runs in CI.
- Coverage staleness/precision is inherited from the producer (line-level lcov ⇒ statement-level
  approximation of `cov(m)`); we report the source + age so consumers can judge.
- New formats are plugins — no core changes.

### Alternatives
Running tests to measure coverage (breaks budget and scope); requiring no coverage and using
complexity alone (loses the entire point of CRAP — the risk is complexity *times* untestedness).

## ADR 0006: Single static binary, zero-config by default

**Status:** Accepted · **Date:** 2026-08-18

### Context
Target users include pre-commit hooks, CI containers, and AI agents — environments where every
installation step, runtime dependency, or required config file halves adoption. "Fácil de
entender y de utilizar" is a product requirement, not a nicety.

### Decision
- One statically-linked binary per platform; no runtime dependencies; installable via
  cargo, homebrew, an npm shim, and curl script. The binary embeds all first-party adapters,
  plugins, and grammars.
- `kndo check` must produce a correct, useful report in any repo with **zero configuration**:
  languages auto-detected by adapters' claims, ecosystems auto-detected by plugin predicates,
  ignores inherited from `.gitignore`. `kndo.toml` only ever *tunes*.
- Defaults are part of the contract: changing a default (weights, thresholds, severities) is a
  breaking change for scores/exit codes and follows semver like the JSON schema.

### Consequences
- Binary size grows with each embedded grammar (~roughly 1–3 MB each) — accepted; size is not a
  product constraint, startup time is.
- Auto-detection must be introspectable or it becomes magic: `kndo doctor` (RFC 0006 §2) is the
  mandatory companion, showing what activated and why.
- Feature-gated builds (`--no-default-features` + per-adapter features) remain possible for
  embedders who want a smaller binary.

### Alternatives
Plugin-download-on-demand model (à la ESLint) — rejected: network in pre-commit/CI, supply-chain
surface, cold-start unpredictability.

## ADR 0007: Product name: `kndo`

**Status:** Accepted · **Date:** 2026-08-18

### Context
The working name `kondo` collides with an existing OSS tool on crates.io (a project-artifact
cleaner) — thematically adjacent, guaranteeing permanent installation confusion (`cargo install
kondo` would fetch the other tool), and likely a Homebrew clash. The product needs one name that
works everywhere: crate, binary, config file, cache dir, pragma, env vars, CI action.

### Decision
The product is **`kndo`** — used uniformly for everything: crates (`kndo`, `kndo-core`,
`kndo-cli`, `kndo-adapter-*`, `kndo-plugin-api`), the binary, `kndo.toml`, `.kndo/`,
`kndo:allow` pragmas, `KNDO_*` env vars, `kndo-action`. Availability verified 2026-08-18:

| Registry | `kndo` | Notes |
|----------|--------|-------|
| crates.io | ✅ free | `kndo-core`, `kndo-cli` also free — the registry that matters most |
| npm | ❌ taken (unrelated DeFi package) | shim publishes as **`kndo-cli`**, installing a binary named `kndo` (npm allows bin ≠ package name) |
| Homebrew | no formula found | first-formula-wins; unverifiable at decision time (API unreachable), risk low for a coined word |

A happy accident: the finding-id prefix in the output schema was already `kndo-` — finding ids
(`kndo-a3f81c92e5d4`) now carry the product name natively.

### Consequences
- The GitHub repository should be renamed `eulke/kondo` → `eulke/kndo` (redirects preserved by
  GitHub); docs already use `kndo` throughout.
- The npm bare name is the one asymmetry (`npm i -g kndo-cli` vs `cargo install kndo`); install
  docs must state it prominently.
- Reserve the names early (empty placeholder crates + the npm package + the GitHub org if
  desired) — availability is only real once claimed.
