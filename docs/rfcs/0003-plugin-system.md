# RFC 0003 — Plugin System

**Status:** Accepted · **Depends on:** RFC 0001, 0002 · **Normative contracts:** [contracts/core-traits.md](../contracts/core-traits.md), [contracts/wasm-abi.md](../contracts/wasm-abi.md) (the external-tier ABI's concrete v1 shape, adapters only — see that document's own scope notes)

## 1. Why plugins exist

The adapter boundary (RFC 0002) deliberately excludes everything a language *ecosystem* adds on
top of the language: frameworks, test runners, coverage formats, org conventions. Those things
change fast, are opinionated, and are optional — exactly what should be **pluggable and
composable** rather than baked into adapters. Plugins are how kndo learns them.

Guiding rule: **adapters describe what code *is*; plugins describe what an ecosystem *means* by it.**

## 2. Extension points

A plugin implements one or more of these hooks (trait `Plugin`, normative in contracts):

| Hook | Runs | Typical use |
|------|------|-------------|
| `classify_file` | graph assembly, phase 2 | adjust a file's role/origin beyond language defaults (e.g. `*.stories.tsx` → tooling) |
| `contribute_roots` | graph assembly, after phase 3b | framework entry points: Next.js `pages/**`, Spring `@Component`, AWS Lambda handlers, `#[test]`-like macros of alt test frameworks |
| `contribute_edges` | graph assembly, after phase 3b | edges invisible to the language: DI wiring, route-string → handler, Angular template → class, CSS class names used from HTML templates |
| `annotate_symbols` | graph assembly, after phase 3b | mark symbols "externally consumed" (public SDK surface, FFI, serialization targets like `@JsonProperty`/serde fields) |
| `ingest_coverage` | pre-analysis | parse a coverage format (lcov, cobertura, JaCoCo, llvm-cov) into per-function coverage (RFC 0005 §10) |
| `suppress` | reporting | domain-specific suppression (e.g. migration files are exempt from dead-code) |

Plugins **cannot**: define new node/edge kinds, mutate other plugins' output, read arbitrary
files (they request file access through the host, which enforces scope — landed for
`contribute_roots`/`contribute_edges`/`annotate_symbols` as `ContentView`, RFC 0016 §5; `.read()`
answers only paths matching the plugin's own declared `requested_file_access` globs, budgeted,
never source files the language graph already covers), or veto core analyses. This keeps the
graph semantics owned by the core and results reproducible.

**Landed (M5).** The first four hooks are wired into `graph::assemble_from_source`, not just
declared on the trait: `classify_file` runs inline in phase 2's file-node build, right after RFC
0012 §7's content-derived origin correction and before role-derived roots — a plugin's answer is
what every downstream role/origin exemption sees. The other three run once, together, right
after phase 3b's reference-resolution merge (symbol tables and every adapter-emitted reference
are both stable by then) and before the canonical edge sort, through a read-only `GraphView`
(borrows the graph's own `files`/`symbols` vectors, no copy) and three typed sinks
(`RootSink`/`EdgeSink`/`AnnotationSink`). A plugin never names a target by internal id — every
sink call takes a `PluginTarget` (a `ProjectPath` plus an optional bare-or-`Owner.name` symbol
name), resolved core-side against the same bare/qualified lookup tables `RawRoot`/`RawReference`
already resolve against; an unresolvable target is dropped silently, the same miss behavior an
adapter's own facts already have. `contribute_edges` only ever produces a `References` edge
(plugins can't mint new edge kinds, per this section's own rule above); `annotate_symbols`'
marks land in a new `ProjectGraph::externally_consumed: Vec<SymbolId>` field, consumed as the
exemption RFC 0005 §7 already documented for `internal-only`/`private-type-leak` before there
was anything to populate it. Every contributed edge/root/annotation is attributed
`Provenance::Plugin(id)`.

`suppress` remains undeclared/unwired — RFC 0016 §7 evaluated it and decided cut, not deferred:
no shipped component needs domain-specific suppression (every exemption `kndo:nextjs`/
`kndo:express` need is reachable through `classify_file`/`contribute_roots` narrowing instead).
A real use case reopens this as a new, additive hook; none exists today.

## 3. Packaging & distribution

Two tiers (decision in ADR 0003):

1. **Built-in plugins** — first-party, compiled into the binary, enabled by auto-detection or
   config. The initial set targets the dominant ecosystems of the supported languages
   (examples, each its own doc before implementation): `react`, `nextjs`, `jest/vitest`,
   `spring`, `junit`, `gradle-conventions`, `swiftui`, `coverage-lcov`, `coverage-jacoco`.
   **Landed so far**: `kndo:coverage-lcov` (in-core), and the first two convention plugins of
   RFC 0015 §6 phase 4 — `kndo:nextjs` ([plugins/nextjs.md](../plugins/nextjs.md)) and
   `kndo:express` ([plugins/express.md](../plugins/express.md)), each its own crate
   (`crates/kndo-plugin-{nextjs,express}`), feature-gated in the `kndo` distribution crate
   (`plugin-nextjs`/`plugin-express`, on by default) and gated at composition by their own
   `activation` rules (§4).
2. **External plugins** — WASM components implementing the same hooks over a versioned ABI
   (`kndo-plugin-api`), loaded from `.kndo/plugins/` or a configured path. Sandboxed (no fs/net;
   host-mediated file access), with per-file fuel/time limits so a plugin cannot break the 500 ms
   budget — a plugin that exceeds its budget is disabled for the run and reported as a diagnostic.
   **Shipped for both `LanguageAdapter` and `Plugin` (M5, v1 —
   [contracts/wasm-abi.md](../contracts/wasm-abi.md)):** `.kndo/plugins/*.wasm` adapters and
   plugins auto-discover through `kndo::open` from the *same* directory — each `.wasm` file is
   tried against both loaders, and wasmtime's own component type-checking rejects whichever
   doesn't match, so nothing needs a naming convention to say which ABI a file targets. Both are
   indistinguishable from a compiled-in adapter/plugin to the `Engine`. `Plugin`'s bridge is
   bidirectional (the guest calls back into two host-provided graph queries,
   `wasm-abi.md` §5.1) — a materially different shape from the adapter ABI's one-directional
   three flat functions, built once real internal demand existed for it (this session), not
   speculatively ahead of it.

   `.kndo/plugins/` is per-project — dropping a file there and having it live is the whole
   opt-in. External `Plugin`s and, since RFC 0016 §4, `LanguageAdapter`s alike additionally
   auto-discover from a **global** directory installed once per machine, so neither has to be
   copied into every project that wants it (§4 covers how a globally installed component decides
   *which* projects that is).

Both tiers use the same trait; built-ins are simply statically linked. Third parties can therefore
prototype a plugin natively and ship it as WASM unchanged.

## 4. Activation & configuration

- **Auto-detection**: a plugin declares detection predicates in prose
  (`PluginDescriptor.detection`, e.g. "package.json depends on `react`") for humans —
  `kndo doctor` (RFC 0006) shows these, but they are never evaluated. `PluginDescriptor.activation`
  is the machine-checkable counterpart (M5, v1 — `wasm-abi.md` §5.4): a list of `FileExists(glob)`
  / `ManifestDependency(name)` rules, cheap filesystem-only checks against the project root, no
  guest code run to decide.
  - **Project-local** `.kndo/plugins/*.wasm` is unconditional — the file being there already is
    the opt-in, `activation` plays no role.
  - **Built-ins with non-empty `activation` are gated by it too** (RFC 0015 §6 phase 1's
    consequence): a built-in convention plugin must never run — or cost the graph-cache bypass —
    on a project it doesn't match. A built-in with *no* rules stays always-on.
  - **`dependencies` implication** (RFC 0015 §3): an active plugin's declared dependency
    coordinates activate any present-but-inactive candidate, as a fixpoint — wrapper chains
    compose without the project declaring the wrapped frameworks. RFC 0015 is normative for
    identity (`kndo:` reserved namespace, source coordinates) and installation.
  - **Globally installed** plugins (§3's global directory — `<XDG data dir>/kndo/plugins`,
    overridable via `KNDO_PLUGIN_DIR`) are filtered through `activation` before they even join
    composition: any single matching rule activates the plugin for that project; an *empty*
    `activation` list means "no known structural signal," so a globally installed plugin with
    none never self-activates — the zero-false-positive default is silence, not a guess.
  - `ManifestDependency` scans every `package.json`/`Cargo.toml` under the project root
    (`kndo_core::discovery::find_files_named`, the same gitignore-aware walker every other
    analysis uses — `node_modules` excluded exactly like everywhere else), not just the root's
    own: a monorepo package the root manifest says nothing about must still activate a plugin
    it genuinely depends on — kndo's monorepo support isn't a special case anywhere else (RFC
    0012 §8/§10), so this couldn't be either. `LanguageAdapter` has the identical `activation`
    field and identical global-tier gating since RFC 0016 §4 (landed) — a globally installed
    adapter is filtered through it exactly like a plugin, with one difference: no
    `dependencies`-implication fixpoint for adapters (nothing has ever needed cross-adapter
    activation; the field rides the wire dormant, same posture as its native reservation).
  - `kndo doctor` reports every global candidate — activated or skipped, with the exact rule
    that did or didn't fire (`kndo::global_plugin_candidates`, separate from `Engine::doctor`,
    which only ever sees the final composed set). `kndo plugin install/list/remove` (RFC 0015
    §4, landed) populates the same directory from GitHub releases with checksum + identity
    verification and a `plugins.lock`; a manual `cp` still works — `kndo plugin list` reports
    such files as hand-installed.
- **Explicit config** (`kndo.toml`) can force-enable/disable and pass plugin-scoped options:

```toml
[plugins.nextjs]
enabled = true            # override auto-detection
app-dir = "src/app"

[plugins.coverage-lcov]
report = "coverage/lcov.info"
max-age = "7d"            # stale reports are ignored (with a diagnostic), not trusted
```

## 5. Determinism & trust

- Plugin execution order is deterministic (topological by declared ordering constraints, then
  name). Same inputs ⇒ same graph ⇒ same findings. **Landed interim rule (M5):** `PluginDescriptor`
  has no ordering-constraints field yet, so `assemble_from_source` sorts registered plugins by
  `id` once and reuses that order for every hook — real, but not yet the full topological rule
  this section names; a plugin depending on another's contribution being visible through
  `GraphView` needs the ordering-constraints field before that's expressible. Open item, not
  silently assumed solved.
- **Plugin identity (id + version + content hash for WASM) participates in the cache key —
  landed (RFC 0016 §6).** Every graph-mutating plugin (`mutates_graph()`, the trait default;
  `LcovPlugin` — coverage ingestion only — declares `false` and was never gated by any of
  this) folds its identity into `compute_graph_key` alongside every discovered file's own
  content hash and every adapter's id/version. That second part is what makes the plugin case
  safe without extra machinery: a `ContentView` never answers a path outside the discovered
  file set, so anything a plugin's content channel could read was already part of the key
  before plugin identity was. Two consequences, split from the single blanket bypass this
  section originally described:
  - **The graph-snapshot cache is now reusable** for a graph-mutating plugin — any input that
    could change its contribution (source, a content-channel-read config file, or the plugin's
    own version/component bytes) already changes the key, so a stale or cross-project match is
    structurally impossible, not merely avoided by policy.
  - **The incremental patch stays bypassed.** `try_patch` splices only the *changed* files'
    facts into the *previous* snapshot's graph and never re-invokes `contribute_roots`/
    `contribute_edges`/`annotate_symbols` — the key-folding argument doesn't extend to an
    incremental splice the way it does to an all-or-nothing key match. Extending patch reuse
    to plugins is real, undone future work (RFC 0016 §6).

  `mutates_graph()`'s self-enforcing rule (assembly only calls the four hooks on plugins
  claiming `true`) is unchanged and remains what the whole scheme is built on.
- External plugins are untrusted code: sandbox as above, and findings they influenced are
  attributed (`"sources": ["plugin:nextjs"]` in JSON output) for auditability. The
  `Provenance::Plugin(id)` attribution itself is landed (every edge/root/annotation a plugin
  contributes carries it); JSON output surfacing it as `sources` is a separate, not-yet-done
  rendering step.

## 6. What is *not* a plugin

- Languages (adapters have a richer contract: parsing, resolution, manifests). RFC 0016
  converges the two kinds' *operational* surfaces (identity, activation, installation) into
  one component model without merging the traits — the semantic split stays.
- Output formats — reporting stays in the core for schema stability; new formats are core PRs.
- Custom *analyses* over the graph: deliberately post-1.0. First we stabilize the graph schema,
  then we can expose a query/analysis API safely. Tracked as an open question in the ROADMAP.
