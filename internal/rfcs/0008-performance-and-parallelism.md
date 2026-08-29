# RFC 0008 — Performance & Parallelism

**Status:** Accepted · **Depends on:** RFC 0001, 0004 · **Related:** ADR 0001 (Rust), ADR 0004 (cache)

## 1. Principles

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

## 2. Parallelism map

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

## 3. Data layout for speed

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

## 4. Determinism under parallelism

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

## 5. Thread pool policy

- Default pool size: physical cores (not logical — hyperthread gains are negligible for this
  workload and hurt tail latency on laptops).
- Overrides: `--threads N` flag > `KNDO_THREADS` env > config `[performance] threads`.
- One global rayon pool per process, built unconditionally by `Engine::open` on every run,
  ahead of any work (§6) — plugins and adapters never spawn their own threads (contract rule;
  WASM plugins are single-threaded by sandbox).
- `--threads 1` is a first-class supported mode (debugging, determinism checks, CI runners with
  noisy neighbors).

## 6. Adaptive execution

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

## 7. Enforcement & tooling

- **Benchmark suite** (`cargo xtask bench`, deliberately not a CI gate): fixture repos at
  1k / 5k / 50k files; measured per scenario: cold full, warm no-op, warm 1-file change, warm
  100-file change, `--staged` on a realistic diff. Budgets: warm p95 < 500 ms @ 5k (the
  contract), cold < 10 s @ 5k; `--gate` fails the build on a >10% regression against the
  recorded baseline (`internal/perf-baseline.json`). That baseline records one machine and does
  not travel, so the gate is run by a human — before and after a change expected to cost time,
  on that same machine — never automatically: ephemeral CI runners of varying hardware would
  fail it for reasons unrelated to any real regression (CONTRIBUTING.md "Benchmarks").
- **Scaling check**: warm 100-file scenario must show ≥ 3× speedup at 8 cores vs 1 core (guards
  against silent serialization creeping in behind a lock) — read by hand alongside the benchmark
  suite above, same non-CI posture.
- **Microbenchmarks** (criterion) for the named hot paths: hash-and-compare pass, BFS round,
  winnowing window, snapshot load. Not gates; trend-tracked.
- **Profiling discipline**: optimizations land with a benchmark delta in the PR description, or
  they don't land — "should be faster" is not evidence.

## 8. Non-goals

- **No resident daemon for the budget.** A warm daemon (post-1.0 idea, ADR 0004) may make kndo
  *even* faster, but the 500 ms contract must hold from a cold process — pre-commit can't depend
  on a daemon being alive.
- **No `unsafe` for speed** outside the vetted dependencies (rkyv, memmap); kndo's own code
  stays safe Rust until a profile proves a specific bottleneck, decided case by case via ADR.
- **No speculative background work** (pre-warming, watching): kndo does nothing between
  invocations by design; that's what keeps it trustworthy in CI and pre-commit.
