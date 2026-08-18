# ADR 0004 — Cache: content-addressed binary snapshots under `.kondo/`

**Status:** Proposed · **Date:** 2026-08-18

## Context
The 500 ms warm budget allots ~100 ms to loading the previous graph (RFC 0001 §5). The cache must
be disposable, per-clone, corruption-tolerant, and keyed so that any input change invalidates
exactly its dependents (RFC 0004 §3).

## Decision
- **Location:** `.kondo/cache/` at project root, gitignored; only `baseline.json` (a sibling,
  not in `cache/`) is committed.
- **Hashing:** blake3 for all content addressing (parallel, collision-safe; also used for
  duplicate-asset detection so hashes are computed once).
- **Serialization:** `rkyv` (zero-copy archival) for `graph.bin` / `findings.bin` so loading is
  mmap + validate rather than deserialize; `bincode` for small per-file facts entries where
  zero-copy buys nothing. Every artifact carries `(magic, core schema version, writer version)`;
  any mismatch ⇒ silently rebuild that layer (cold), never migrate in place.
- **Concurrency:** single-writer advisory lock; concurrent runs degrade to read-only cache use.

## Consequences
- Branch switches and stashes stay warm (content addressing ignores paths' mtimes and history).
- rkyv couples layout to exact type definitions — acceptable because the cache is explicitly
  disposable and versioned; no migration code will ever be written.
- Facts store is pruned by LRU cap (default 256 MB) to bound disk usage on long-lived clones.

## Alternatives
SQLite (robust, but row-oriented access pattern and query layer add latency and a C dependency
for no query need we have); JSON/MessagePack snapshots (parse cost blows the budget on large
graphs); OS-level shared daemon keeping the graph hot (post-1.0 idea; a daemon complicates the
trust and lifecycle story and shouldn't be *required* to meet the budget).
