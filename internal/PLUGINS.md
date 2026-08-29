# Plugins

## RFC 0003: Plugin system

**Status:** Accepted · **Depends on:** RFC 0001, 0002 · **Normative contracts:** [contracts/core-traits.md](CONTRACTS.md#core-traits), [contracts/wasm-abi.md](CONTRACTS.md#wasm-abi) (the external-tier ABI's concrete v1 shape, adapters only — see that document's own scope notes)

### 1. Why plugins exist

The adapter boundary (RFC 0002) deliberately excludes everything a language *ecosystem* adds on
top of the language: frameworks, test runners, coverage formats, org conventions. Those things
change fast, are opinionated, and are optional — exactly what should be **pluggable and
composable** rather than baked into adapters. Plugins are how kndo learns them.

Guiding rule: **adapters describe what code *is*; plugins describe what an ecosystem *means* by it.**

### 2. Extension points

A plugin implements one or more of these hooks (trait `Plugin`, normative in contracts):

| Hook | Runs | Typical use |
|------|------|-------------|
| `classify_file` | graph assembly, phase 2 | adjust a file's role/origin beyond language defaults (e.g. `*.stories.tsx` → tooling) |
| `contribute_roots` | graph assembly, after phase 3b | framework entry points: Next.js `pages/**`, Spring `@Component`, AWS Lambda handlers, `#[test]`-like macros of alt test frameworks |
| `contribute_edges` | graph assembly, after phase 3b | edges invisible to the language: DI wiring, route-string → handler, Angular template → class, CSS class names used from HTML templates |
| `annotate_symbols` | graph assembly, after phase 3b | mark symbols "externally consumed" (public SDK surface, FFI, serialization targets like `@JsonProperty`/serde fields) |
| `ingest_coverage` | pre-analysis | parse a coverage format (lcov, cobertura, JaCoCo, llvm-cov) into per-function coverage (RFC 0005 §10) |
| `suppress` | reporting | domain-specific suppression (e.g. migration files are exempt from dead-code) |
| `rules` | declaration only, before any hook runs | declares the rule names/severities `contribute_findings` may emit under, so `kndo doctor`/`kndo plugin verify` can show them and gate config can validate against real names (RFC 0018) |
| `contribute_findings` | after assembly, every path (cold, patch, warm snapshot hit) | third-party verdicts, not graph facts: findings namespaced `plugin:<coordinate>/<rule>` on the advisory severity channel (RFC 0018) |

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

### 3. Packaging & distribution

Two tiers (decision in ADR 0003):

1. **Built-in plugins** — first-party, compiled into the binary, enabled by auto-detection or
   config. The initial set targets the dominant ecosystems of the supported languages
   (examples, each its own doc before implementation): `react`, `nextjs`, `jest/vitest`,
   `spring`, `junit`, `gradle-conventions`, `swiftui`, `coverage-lcov`, `coverage-jacoco`.
   **Landed so far**: ten built-in plugin crates under `crates/kndo-plugin-*`, each feature-gated
   in the `kndo` distribution crate (`plugin-<name>`, on by default) and gated at composition by
   their own `activation` rules (§4) — an ecosystem list driven by measured gaps (RFC 0015 §6
   phase 4 onward) rather than the initial guess above, and already past it:
   - `kndo:nextjs` ([plugins/nextjs.md](../docs/src/plugins/nextjs.md)) — Next.js's
     file-system router.
   - `kndo:express` ([plugins/express.md](../docs/src/plugins/express.md)) — Express's
     imperative route registration.
   - `kndo:serde` ([plugins/serde.md](../docs/src/plugins/serde.md)) — serde's
     (de)serialization traits as machinery dispatch.
   - `kndo:rkyv` ([plugins/rkyv.md](../docs/src/plugins/rkyv.md)) — rkyv's traits, the same
     machinery-dispatch shape as serde.
   - `kndo:info-plist` ([plugins/info-plist.md](../docs/src/plugins/info-plist.md)) — an Apple
     bundle's `Info.plist` naming classes as strings.
   - `kndo:thymeleaf` ([plugins/thymeleaf.md](../docs/src/plugins/thymeleaf.md)) — a Spring
     Boot app's controller-to-template view layer.
   - `kndo:libsass-maven-plugin`
     ([plugins/libsass-maven-plugin.md](../docs/src/plugins/libsass-maven-plugin.md)) — a
     Maven Sass build's compiled-and-committed output.
   - `kndo:uikit` ([plugins/uikit.md](../docs/src/plugins/uikit.md)) — the classes, outlets
     and actions an Interface Builder storyboard wires up.
   - `kndo:wasmtime` ([plugins/wasmtime.md](../docs/src/plugins/wasmtime.md)) —
     `wasmtime::component::bindgen!`-generated trait dispatch.
   - `kndo:coverage-lcov`/`kndo:coverage-cobertura`/`kndo:coverage-jacoco`/`kndo:coverage-go`
     (one crate, `crates/kndo-plugin-coverage`, [plugins/coverage.md](../docs/src/plugins/coverage.md))
     — coverage ingesters, one per report format.
2. **External plugins** — WASM components implementing the same hooks over a versioned ABI
   (`kndo-plugin-api`), loaded from `.kndo/plugins/` or a configured path. Sandboxed (no fs/net;
   host-mediated file access), with per-file fuel/time limits so a plugin cannot break the 500 ms
   budget — a plugin that exceeds its budget is disabled for the run and reported as a diagnostic.
   **Shipped for both `LanguageAdapter` and `Plugin` (M5, v1 —
   [contracts/wasm-abi.md](CONTRACTS.md#wasm-abi)):** `.kndo/plugins/*.wasm` adapters and
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

### 4. Activation & configuration

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
    field and identical global-tier gating since RFC 0016 §4 (landed), and since RFC 0017 §6
    the identical `dependencies`-implication fixpoint too — literally the same code, generified
    over kind-neutral candidate identities, so the two composition paths cannot drift.
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

The coverage pair above is live: `report` (string or array, globs allowed) *replaces* the
descriptor's well-known list, and `max-age` (`"7d"`/`"24h"`/integer days) overrides the
freshness default per plugin — bare keys name built-ins without the `kndo:` prefix, quoted
keys match full ids. `enabled`/plugin-specific options (`app-dir`) remain future work and
parse as inert. `ingest_coverage` is also WASM-bridged now, via the `coverage-ingester`
world (a third sibling world; see `plugin.wit`).

### 5. Determinism & trust

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
  - **The incremental patch re-derives instead of bypassing (RFC 0017 §3, landed).**
    `try_patch` strips every `Provenance::Plugin` edge and the `externally_consumed` set from
    the previous snapshot, splices the source change, and re-runs the plugin round — the same
    function the full build calls — against the patched graph, byte-identical by the
    equivalence gate. One guard: the snapshot stores the plugin-set identity digest, and a
    changed set full-rebuilds once (`classify_file` overrides are baked into `FileNode.class`
    untagged, so they can't be stripped — but they're path-only, so under an identical set
    they're identical too).

  `mutates_graph()`'s self-enforcing rule (assembly only calls the four hooks on plugins
  claiming `true`) is unchanged and remains what the whole scheme is built on.
- External plugins are untrusted code: sandbox as above, and findings they influenced are
  attributed (`"sources": ["plugin:nextjs"]` in JSON output) for auditability. The
  `Provenance::Plugin(id)` attribution itself is landed (every edge/root/annotation a plugin
  contributes carries it); JSON output surfacing it as `sources` is a separate, not-yet-done
  rendering step.

### 6. What is *not* a plugin

- Languages (adapters have a richer contract: parsing, resolution, manifests). RFC 0016
  converges the two kinds' *operational* surfaces (identity, activation, installation) into
  one component model without merging the traits — the semantic split stays.
- Output formats — reporting stays in the core for schema stability; new formats are core PRs.
- A general query/analysis API against the graph schema itself: still open — the schema isn't
  stabilized as a public query surface, and this remains tracked as an open question in the
  ROADMAP. What *did* land, narrower and on purpose ([RFC 0018](#rfc-0018-plugin-contributed-findings),
  accepted): `Plugin::rules`/`contribute_findings` let a plugin emit its own verdicts — findings,
  not graph facts — over the same read-only `GraphView`/`ContentView` the graph-mutation hooks
  get, namespaced `plugin:<coordinate>/<rule>` on an advisory severity channel it can never
  escalate into a core finding or (without explicit user opt-in) the exit code. A plugin still
  cannot mutate the graph through this path, touch a core analysis's own output, or define new
  node/edge kinds — "custom analyses, unrestricted" is not what shipped, and remains the open
  item above.

## RFC 0015: Plugin identity and dependencies

**Status:** Accepted, phased (§6) · **Depends on:** RFC 0003 (plugin system, activation — §4),
ADR 0003 (WASM linking), RFC 0014 (distribution posture: git-first, no central infrastructure) ·
**Ships:** M6

### 1. The problem, from a real scenario

A company builds an internal framework that wraps Next.js, which wraps React, which the JS
ecosystem's own conventions already partially cover. The company writes a kndo plugin for its
framework in a private repo. Three things must work:

1. A project that only declares `@company/framework` in its manifest must get the *whole
   chain's* conventions — the framework plugin's, Next's, React's — without declaring any of
   them, because the project's author doesn't even know the chain (that's the wrapper's job).
2. The company plugin must be able to reference kndo's own built-in plugins and third-party
   plugins from other sources, unambiguously.
3. "Reference by name" must not be a land grab: with flat names, anyone can publish a `.wasm`
   whose descriptor says `id: "react"` — and then *which* react activates is undefined.

RFC 0003 §4's `activation` rules answer "does *this* plugin apply to *this* project?" but say
nothing about plugins composing. This RFC adds exactly that, and nothing else. The practical
author-facing companion (toolchain, project setup, testing shape, maintenance checklist) is
[docs/src/plugins/authoring.md](../docs/src/plugins/authoring.md).

### 2. Identity: the coordinate IS the id (the Go-modules move)

A plugin's id is not a name to be looked up — it is the coordinate it can be fetched from:

- **External plugins**: a source coordinate, `github.com/<owner>/<repo>` (host part extensible
  later; GitHub is v1). Optionally version-qualified where a version is expressible
  (`@v1.2.0`). Because the identity is the location, there is nothing to squat, no registry
  authority to assign names, and no ambiguity: two "react conventions" plugins are
  `github.com/a/react-conventions` and `github.com/b/react-conventions` — different ids, both
  installable, no conflict.
- **Built-ins**: the reserved `kndo:` namespace — `kndo:coverage-lcov`, `kndo:nextjs`,
  `kndo:express`. The loader **rejects** any external component whose descriptor claims a
  `kndo:`-prefixed id: the namespace is not claimable, so referencing a built-in from any
  external plugin is always unambiguous.

**Identity binding**: whenever a component is fetched *by* a coordinate (§4), the descriptor it
reports must declare exactly that coordinate as its `id`, or it is rejected. Nothing can
impersonate an id it wasn't fetched from. (A hand-dropped `.wasm` in `.kndo/plugins/` skips
this check — its presence in the project is already the trust decision, same as today.)

Existing ids migrate: `coverage-lcov` → `kndo:coverage-lcov`. The demo/example plugins keep
plain ids (`hooks-demo`) — legal for hand-dropped files, but such an id can never be the target
of a dependency (§3), which is the point: depending on something requires it to be fetchable.

### 3. `dependencies`: one field, two coupled effects

```text
PluginDescriptor {
    id:           "github.com/company/framework-plugin",
    activation:   [ManifestDependency("@company/framework")],
    dependencies: ["github.com/company/other-framework-plugin", "kndo:nextjs"],
}
```

A dependency is *a plugin whose conventions are part of this plugin's own* — the wrapper
relationship. Declaring one has exactly two effects:

1. **Install-time closure** (§4): installing the plugin installs its dependencies,
   transitively. `kndo:*` entries resolve as no-ops (compiled in).
2. **Activation implication**: when a plugin is active, every dependency that is *present*
   (installed globally, dropped project-locally, or built-in) becomes active too — computed as
   a fixpoint over the present set, so chains compose to any depth:
   `company-framework → other-framework → kndo:express` all activate when the project matches
   only the company plugin's own rule. Cycles are harmless (set semantics — no ordering is
   implied, because plugins still never consume each other's output; execution order remains
   the existing sorted-by-id interim rule).

   **This is not a convenience — for the wrapper case it is the only path there is.** A company
   framework that uses Express internally does not put `express` in its users' manifests; it
   puts `@company/framework` there. So `kndo:express`'s own
   `ManifestDependency("express")` rule can *never* fire in such a project, no matter how much
   Express is really running. Without the implication, that project's Express conventions are
   unreachable — not degraded, unreachable. Pinned end-to-end, external component naming a
   built-in, in `crates/kndo/tests/plugin_dependency_implication.rs`, with
   `examples/kndo-plugin-wrapper-demo` as the real component; the adapter tier's mirror is
   `adapter_dependency_implication.rs`. Both are named in CLAUDE.md's never-regress list,
   because no plugin we ship exercises the field and an unused mechanism is the easy one to
   delete.

Deliberately **one field, not two** ("requires" vs "implies"): since plugins cannot read each
other's contributions — structurally, `GraphView` exposes only adapter-built facts and sinks go
to the core — the *only* coherent meaning of inter-plugin dependency is "co-activate and
co-install". Splitting it would invent a distinction with no behavioral difference to hang it
on. For the same reason there are **no version constraints between plugins**: there is no ABI
between them to be compatible about. Version selection exists only at install time (§4), per
coordinate, not per edge.

**A missing dependency is never a runtime error.** If `kndo:nextjs` names a built-in, it's
always present. If `github.com/x/y` isn't installed (hand-managed setups), the plugin still
runs; `kndo doctor` reports the exact missing coordinate and the install command that fixes it.
Degradation is legible — "those conventions aren't being analyzed" — never a crash, matching
every other absence in the product.

**Over-activation is accepted within a declared closure**: a frontend-only project using the
company framework will also activate the express-conventions dependency, which will find no
express-shaped symbols and contribute nothing. That is the author-curated cost of the wrapper
declaring its user-facing surface — bounded by the closure, unlike lockfile inference (§5),
which is unbounded.

### 4. `kndo plugin install` — registry semantics without a registry service

```text
kndo plugin install github.com/company/framework-plugin@v2
kndo plugin list
kndo plugin remove github.com/company/framework-plugin
```

Git-first, mirroring RFC 0014's release posture (the sibling Yunta project's pack design
reached the same conclusion independently: "git-first, lockfile always"):

1. Resolve the coordinate to a GitHub release of that repo (`@vX.Y.Z` names the tag; bare
   coordinate = latest release). The release must carry a `.wasm` asset and a checksum file —
   the same artifact convention RFC 0014 §3 uses for kndo itself.
2. Download; verify the checksum; verify identity binding (§2): descriptor id == coordinate.
3. Read `dependencies`; recurse. `kndo:*` → no-op. Already-installed coordinate at a
   compatible version → no-op.
4. Write the `.wasm` files into the existing global plugin directory (RFC 0003 §4 /
   wasm-abi.md §5.5) plus a lockfile beside them (`plugins.lock`: coordinate → version →
   sha256) making the installed set reproducible and auditable.

**Version conflicts, minimal v1 policy**: one installed copy per coordinate. If an install
would require two incompatible versions of the same coordinate (different explicit tags), the
install **fails with both requirers named** — kndo does not guess. No dependency solving: there
is no inter-plugin ABI that would justify it.

**Private repos work with zero extra machinery** — the fetch uses the user's existing git/
GitHub credentials, which is precisely the company-framework scenario. A central registry
would have required private hosting; coordinates make privacy the repo's own access control.

**Landed** (`kndo::plugin_install`, the `kndo` crate's `plugin-install` feature, on by
default). Implementation decisions worth pinning:

- Release shape enforced literally: exactly one `.wasm` asset (ambiguity is an error naming
  every candidate) plus `checksums.txt` in `sha256sum` line format. Assets download through
  the API asset URL with `Accept: application/octet-stream` — the one form that carries
  auth for private repos; credentials are `GITHUB_TOKEN`/`GH_TOKEN` from the environment.
- The whole transaction stages first and commits last: any checksum, identity, conflict, or
  fetch failure anywhere in the closure leaves the directory and lockfile untouched.
- `plugins.lock` maps coordinate → `{version, sha256, file}`; the on-disk name is the
  coordinate with `/` → `__` (`github.com__owner__repo.wasm`), reversible because `__` cannot
  appear in a GitHub owner/repo name. Files present but not in the lock are reported by
  `kndo plugin list` as hand-installed, never hidden and never touched by `remove`.
- The version-conflict rule has a cross-transaction twin: an explicit tag that disagrees with
  the locked version fails, naming the installed version and the requirer — `remove` first if
  the change is intended. Bare (untagged) requests are compatible with anything installed.
- A `kndo:*` dependency that this build does *not* compile in is a warning in the install
  report (and a doctor line thereafter), not an error — §3's never-fatal rule applied at
  install time too.
- Network and component-probing are injected edges (`ReleaseSource` + a probe fn), so every
  policy above is proven by unit tests without either, plus one integration test driving the
  real WASM probe: a genuine component with a plain id fetched by coordinate trips identity
  binding and installs nothing (`crates/kndo/tests/plugin_install_probe.rs`).

### 5. Rejected: lockfile-transitive activation

Considered and rejected as the mechanism for the wrapper case (matching `ManifestDependency`
against the project's lockfile closure instead of its declared dependencies):

- Public frameworks already don't need it: Next declares React as a **peer dependency**, so
  every real Next project declares `react` directly — direct matching fires today.
- A lockfile cannot distinguish "framework re-exports React to its users" from "some CLI tool
  uses React internally" — activating on transitive presence is guessing, and this project's
  standard is silence over a guess (RFC 0005 §13).
- It is unbounded (thousands of transitive packages), where a declared `dependencies` closure
  is bounded and author-curated.
- Per-ecosystem lockfile parsers (three formats in JS alone) are real permanent surface.

If a concrete case ever appears that `dependencies` cannot express, this gets rediscussed with
that case on the table — not before.

### 6. Phases

1. **`Plugin::mutates_graph()`** — landed with this RFC's first commit: prerequisite hygiene
   (a coverage-only plugin must not cost the cache; a built-in convention plugin must not cost
   every non-matching project a full rebuild — see RFC 0003 §5's updated note and the
   regression tests in `graph.rs`).
2. **Identity + `dependencies` + fixpoint activation**: descriptor field (native + WIT),
   `kndo:` namespace reservation enforced at load, activation fixpoint in the composition
   layer, `kndo doctor` showing dependency chains and missing coordinates. Semantics complete
   and fully testable without any network code.
3. **`kndo plugin install/list/remove`**: the fetch/verify/lockfile machinery of §4 —
   landed, see §4's implementation notes.
4. **First real built-ins**: `kndo:nextjs` (file-system routing roots, special exports — the
   flagship, spec: [docs/src/plugins/nextjs.md](../docs/src/plugins/nextjs.md)) and `kndo:express`
   (script-launched entry files the import graph can't see — honest spec:
   [docs/src/plugins/express.md](../docs/src/plugins/express.md); express is imperative, so its convention
   surface is real but modest, and `views/**` templates turned out to be *unclaimed* files —
   invisible to the graph, hence producing no findings to suppress — so they're documented out
   of scope rather than covered). Both gated by their own `activation` rules — a built-in
   convention plugin must never run (or cost cache bypass) on a project that doesn't match.

### 7. Explicitly out of scope

- A central registry service (hosting, accounts, moderation) — coordinates make it
  unnecessary; revisit only if discoverability demands a *directory* (which can be a static
  page, not a service).
- Version constraints or ordering constraints between plugins — no inter-plugin ABI exists to
  justify either. RFC 0003 §5's ordering-constraints gap stays open, unchanged by this RFC.
- Plugins consuming other plugins' contributions — still structurally impossible, still
  deliberate.
- Signing beyond checksums (sigstore-style provenance) — worth a look post-1.0; checksums +
  identity binding + the WASM sandbox are the v1 trust story.

## RFC 0017: Plugin platform, second pass

**Status:** Accepted (design), phased (§8) · **Depends on:** RFC 0003 (plugin system), RFC
0004 (cache), RFC 0013 (incremental patch), RFC 0015 (identity & installation), RFC 0016
(uniform component model) · **Ships:** pre-publication — every phase lands before kndo is
public, because §2's whole argument is that these changes are cheap now and ABI-visible later

### 1. The question this RFC answers

RFC 0016 closed every gap it set out to close, and judged each proposed API extension by one
bar: *a real, landed consumer in this repository*. That bar was right for building the
product — it kept speculative surface off a contract that will freeze at 1.0. It is the wrong
bar for operating a **platform**. kndo will be published; plugin authors will be third
parties whose needs cannot be enumerated by reading this codebase. Three of RFC 0016's own
"decided against, no consumer" outcomes (§7) and one of its honest scope cuts (§6's
patch-path bypass) look different under that criterion, and this RFC re-decides them
deliberately rather than by drift:

1. The **read surface** (`GraphView`) was widened only where a first-party plugin could
   delete code. A platform's read surface must instead be *complete over the graph's stable
   data model* — §2 makes that the design criterion and §5 derives the API from it.
2. The **incremental patch** stayed bypassed for any graph-mutating plugin — acceptable when
   plugins were rare, a standing performance cliff once installing plugins is normal. §3
   removes the bypass structurally, not with a trust-me flag.
3. The **WASM bridge re-instantiates its guest three times per round** — a documented cost
   artifact that already deformed one API (the content budget is keyed by path specifically
   to avoid triple-charging). Changing it after third-party components exist is an
   ABI-visible behavior change; changing it now is an implementation detail. §4 does it now.
4. The **author experience is product surface.** Internally kndo has a compliance suite,
   fixtures, and a dogfood gate; a third-party author has a prose doc. §7 ships the kit and
   turns the ABI compatibility promise from a sentence into a CI job.

`AdapterDescriptor.dependencies` — dormant since RFC 0016 §8 phase 0 — completes the adapter
side's parity under the same platform criterion (§6): a wrapper adapter (a Vue-style superset
language needing its base language's adapter present) is exactly the kind of third-party
composition the platform cannot foresee but must not preclude.

### 2. The platform criterion

**Read APIs are designed by closure over the data model; write APIs stay demand-gated.**
The asymmetry is deliberate and load-bearing:

- A *read* API can at worst return data the graph already holds. Withholding it doesn't
  protect correctness — it just forces plugin authors to re-derive graph facts badly (scan
  paths by hand, re-parse source with regexes) or abandon their idea. Every stable fact the
  graph commits to (paths, names, kinds, package topology, adapter-derived edges) is
  therefore queryable, natively and over WASM, without a per-item demand argument.
- A *write* API extends what a component can make kndo believe, and every extension carries
  the zero-false-positive burden (RFC 0005). Writes stay conservative: the only widening this
  RFC makes (§5.4's file-target edges) comes with an explicit containment rule.

Two contract rules keep the complete read surface correct and free:

**R1 — Plugins read the adapter graph, never each other.** Every query answers from
adapter-derived data (`Provenance::Adapter`) plus the plugin's own inputs; contributions from
other plugins are invisible. This is RFC 0003's existing "plugins cannot consume each other's
output" rule extended to the new queries — without it, results would depend on registration
order and composition, and determinism (identical runs regardless of which other plugins are
installed) would silently break. A plugin sees the same world alone or alongside fifty
others.

**R2 — Indexes are lazy.** Reverse-edge and call-site indexes build on first use, once per
round, O(edges) — a plugin that never queries them costs nothing, and the zero-plugin run
costs exactly what it costs today. No budget applies to graph queries: they touch memory
already loaded, never disk.

### 3. Incremental patch with plugins: strip & re-run — Landed

RFC 0016 §6 folded plugin identity into the snapshot key but left `try_patch` (RFC 0013)
bypassed whenever a graph-mutating plugin is registered, because composing a plugin's hook
output with a partial re-derivation was an unproven claim. The proof turns out not to be
needed — the contributions can be discarded and re-derived instead:

1. **Every plugin contribution is strippable.** Contributed edges carry
   `Provenance::Plugin(id)`; `externally_consumed` is a standalone, wholly plugin-derived
   vector. Neither is entangled with adapter data.
2. **`classify_file` is patch-stable.** It is path-only (`&ProjectPath` + current class, no
   content), and the patch already guarantees the path set is unchanged — so the class
   overrides baked into `FileNode.class` (the one plugin effect with no provenance tag)
   cannot differ from a full rebuild's, *provided the plugin set itself is unchanged*.
3. **The patch path already rebuilds everything a plugin round needs** — the per-file symbol
   name tables, the file index, and it holds the `DiscoveredTree` for `ContentView`.

So: the plugin round factors out of `assemble_from_source` into one function used by both
build paths. `try_patch` strips all `Provenance::Plugin` edges, clears
`externally_consumed`, applies the source patch exactly as today, then re-runs the round
against the patched graph. Plugin hooks are deterministic functions of (graph, discovered
content); both are current post-patch, so the result is byte-identical to a full rebuild —
and RFC 0013's patched ≡ full-rebuild equivalence gate now proves that *with plugins
registered*, mechanically, not by argument.

**One new guard:** the snapshot records the registered plugin set's identity digest (the same
id/version/content-hash fold RFC 0016 §6 put in the graph key), and the patch only proceeds
when it matches the current set — closing the `classify_file` hole in (2): a changed plugin
set means the baked-in class overrides may be stale, so that run full-rebuilds once (which
the snapshot key would have forced anyway).

Cost: patch as today + one plugin round (O(files + symbols) + budget-capped content reads) —
milliseconds. The performance story becomes uniform: a plugin-bearing project pays for its
plugins' hooks, never again for their *presence*.

### 4. One guest instance per round — Landed

The plugin bridge instantiates its guest before each hook — three instantiations per plugin
per round. It was the simplest correct thing when zero external plugins existed, and it
already shaped an API: the content budget is keyed by distinct path, not by call,
specifically because the bridge re-fetches the same glob set three times.

This RFC changes the contract to **one instantiation per plugin per round**: the three
graph-mutation hooks run against the same instance, in declaration order
(`contribute-roots`, `contribute-edges`, `annotate-symbols`). What a guest may now observe:

- **State persists across the three hooks of one round.** A guest may compute something in
  `contribute-roots` and reuse it in `contribute-edges`. This is a widening — code written
  for the old contract (stateless request/response, which is what `wit-bindgen` produces by
  default) behaves identically.
- **State never persists across rounds or runs.** The instance is dropped when the round
  ends. No plugin can accumulate cross-run memory; determinism per round is unchanged.

The per-path budget keying stays — it is the right semantics regardless (a component's read
scope shouldn't depend on how many hooks look at the same file) — but its original
motivation dissolves. wasm-abi §5 documents the new lifecycle; the compliance suite grows a
test proving state visibly carries between hooks in one round and visibly resets across
rounds. Doing this *before* publication is the point: today it is an internal change with an
internal test; after third parties ship components, it would be a behavior migration.

### 5. The complete read surface — Landed

*(Landing note, one honest deviation: the route/template edge detections §5.4's draft named
as first consumers were **not** built, because both plugins' own specs prove them effect-free
today — `views/**` templates are unclaimed files, outside the `unused` verdict's scope
entirely (express.md §1: "the edge would have nothing to connect to"), and Next.js pages are
already `Certain` roots that no route edge can make more alive. Building detections whose
specs prove they change zero findings would be the exact speculative theater this project
rejects. What landed is the complete mechanism they — and the real future consumers: a
feature-flag plugin (`dead-feature-flag` is on the ROADMAP), DI-container wiring, CSS classes
used from HTML templates once an HTML adapter exists — need: call-site facts, file-target
plugin edges with the §5.4 containment rule, and every query below, each proven end to end
through the reference guest and, for `packages()`, a first-party consumer that deleted real
code: `kndo:express` now derives its app roots from the graph's package topology instead of
path-scanning the whole file list.)*

Derived by closure over what the graph stably holds (§2), exposed natively on `GraphView`
and over WASM as **additive imports with new record types** — existing WIT records are
frozen (growing a record is a breaking change in the component model; adding imports is the
same forward-compatible evolution `read-file` already used, declared in wasm-abi §8).

#### 5.1 Files and symbols, completed

`wasm-file-info` (path/role/origin) and `wasm-symbol-info` (name/kind/exported/member-of)
stay as they are. New imports return the rest of what `FileNode`/`SymbolNode` commit to:

```
file-details:   func(path: string) -> option<wasm-file-details>
                  // language, unit, package root (as a path), test-span count
symbol-details: func(path: string, symbol: string) -> option<wasm-symbol-details>
                  // visibility rung name, span
```

Natively these are already reachable (`files()` yields `&FileNode`); the imports close the
WASM gap. Internal IDs stay off the surface everywhere, as always.

#### 5.2 Package topology

```
packages:   func() -> list<wasm-package-info>       // manifest path?, name?, root dir
package-of: func(path: string) -> option<wasm-package-info>
```

Natively: `GraphView::packages()` / `package_of(&ProjectPath)` over the graph's
`PackageNode` table (RFC 0011 §3's total ownership — every file maps to exactly one).
First-party proof: `kndo:express`'s app roots *are* the graph's package manifests — its
path scan deletes; `kndo:nextjs` consumes the `package.json` half of its anchors and keeps
its `next.config.*` scan (correctly: a Next config is not a package boundary and core will
not pretend it is).

#### 5.3 Edges

```
imports-of:    func(path: string) -> list<string>          // ImportsFile, outgoing
importers-of:  func(path: string) -> list<string>          // ImportsFile, incoming
references-to: func(path: string, symbol: string) -> list<wasm-ref-site>
                 // from-path, from-symbol?, ref-kind, confidence
```

Natively: same three on `GraphView`, backed by R2's lazy reverse index. Answers obey R1:
adapter-provenance edges only. Results are path/name-shaped and sorted (determinism), never
edge indices.

#### 5.4 Call-site facts — structured, adapter-extracted

The RFC 0016 §5 detections that stayed out (`res.render` → template edges, route-string →
page edges) stayed out for a *mechanism* reason: reading claimed source through the content
channel is a boundary violation, and — worse — re-scanning source with regexes under a
200-file budget is bad engineering when tree-sitter already parsed every file. The right
mechanism: **adapters extract one generic, ecosystem-blind fact** — call sites whose
argument is a string literal:

```
FileFacts.string_call_args: Vec<(callee_dotted_path, literal, span)>
// e.g. ("res.render", "index", span) · ("app.get", "/users", span) · ("require", "./x", span)
```

The graph persists it; `GraphView::string_call_sites_in(path)` (native) and
`call-sites-in: func(path)` (WASM) expose it. Plugins interpret: `kndo:express` derives
`res.render("x")` → an edge to the matching file under its views directory; `kndo:nextjs`
derives `<Link href>` / `router.push` literals → edges to the matching page file. Zero
re-parsing, no budget interaction, cache-native (facts cache), and the adapter stays
framework-blind — it extracted "a call with a string-literal argument," not "an Express
route". JS-TS implements it first (the consumers' language); the field is optional per
adapter, defaulting empty like `test_spans`. Costs one `GRAPH_SCHEMA_VERSION` bump and a
JS-adapter `facts_schema_version` bump — one clean cache invalidation.

**File-target plugin edges, and the containment rule.** Template/page edges target files,
which the plugin edge sink today rejects (References requires a symbol target). The sink
widens to accept file targets, mapped to a plugin-owned file-to-file edge — and the rule
that keeps the zero-FP bar intact gets written into RFC 0005 alongside it: **plugin edges
are liveness evidence, never architecture evidence.** They feed reachability (rescuing a
file or symbol from `unused` — a false positive here only *suppresses* findings, the safe
direction) and are ignored by `cyclic` and every other analysis that would *create* a
finding from an edge's existence. `cyclic` already consumes only `ImportsFile`; the rule
makes that a contract, not an accident.

#### 5.5 What deliberately stays out

The content channel keeps its budget and its outside-the-graph posture — §5.4 removes the
pressure to bend it. `classify_file` still takes no `ContentView` (its once-per-file-across-
all-components execution model hasn't changed, and content-based classification remains
adapter territory via `detected_origin`). And no query exposes another plugin's
contributions (R1) — that boundary is what keeps the platform deterministic.

### 6. Adapter dependencies, evaluated

`AdapterDescriptor.dependencies` (dormant since RFC 0016 §8 phase 0, riding the WIT wire
since §4) gains RFC 0015 §3's exact semantics, mirroring `compose_plugins`:

- **Co-activation fixpoint, global tier only**: an *active* global adapter activates every
  dependency present in the global directory, to any depth. Project-local and compiled-in
  adapters stay unconditional, exactly like plugins.
- **Co-installation**: `kndo plugin install` closes over adapter dependencies the same way
  it closes over plugin dependencies (the probe already carries them; the closure is
  kind-agnostic).
- **Doctor parity**: `ResolvedAdapter` gains `dependencies`; `AdapterResolution` gains
  `missing_dependencies`; `kndo doctor` renders both, mirroring the plugin sections.

The motivating shape is the wrapper adapter — a `.vue`/`.astro`-style superset language whose
own extraction degrades without its base language's adapter present. Proof against real
components, not mocks: a second example crate (`examples/kndo-adapter-wrapper-demo`)
declaring `dependencies = [<the demo adapter's id>]`, exercised by an activation-fixpoint
integration test. **Out of scope, stated:** cross-kind dependencies (a plugin depending on an
adapter or vice versa) — that is a coordinate-namespace unification with its own questions;
nothing here precludes it.

### 7. The author kit, and compatibility as a CI fact

- **`kndo plugin verify <component.wasm>`**: runs the public half of the compliance suite
  locally — loads the component, checks the descriptor (id shape, reserved-namespace
  rejection, activation rules well-formed), drives every hook against a small fixture
  project, and reports what the component contributed. The same checks CI runs on kndo's own
  examples, packaged for an author's inner loop; no wasm toolchain knowledge beyond "build a
  component" required.
- **A starting template**: an authoring skeleton (manifest, WIT bindings, one hook, one
  test) documented in `docs/src/plugins/authoring.md` — the distance from "idea" to "component
  that passes `verify`" is the ecosystem's activation energy.
- **The compat matrix**: wasm-abi §8 promises a v1 component works against every
  v1-compatible host *indefinitely*. That becomes a CI job: committed, pinned, pre-built v1
  components (the demo adapter and hooks-demo plugin as built today) run against the HEAD
  host on every push. The promise breaks the build instead of the ecosystem.
- **Auditability**: `kndo doctor` reports per-plugin contribution counts (roots, edges,
  annotations) from the last run — the observable half of the threat model, which also gets
  written down explicitly: a malicious component can *lie about graph facts* (at worst
  suppressing findings); it cannot read outside its declared globs, cannot write, cannot
  reach the network.

### 8. Phases

1. **Patch with plugins (§3) — Landed.** `run_plugin_round` factored out of
   `assemble_from_source` and called by both build paths; `try_patch` strips
   `Provenance::Plugin` edges + `externally_consumed`, splices, re-runs the round, and
   refuses on a plugin-set digest mismatch (the digest and a plugin-diagnostics partition
   now live in the snapshot, format v2). Landing this surfaced and fixed a real RFC 0016 §6
   regression: snapshots never persisted `externally_consumed` — safe while no snapshot was
   ever written with plugins registered, silently dropping `annotate_symbols` exemptions
   (RFC 0005 §7) on every warm hit once writes became unconditional. Proven by
   `the_patch_re_derives_plugin_contributions_instead_of_bypassing` (a content-channel
   marker flip, invisible to every adapter guard, must surface through a patch — plus
   byte-equality against a scratch rebuild), `a_changed_plugin_set_refuses_the_patch_and_rebuilds`,
   and `externally_consumed_round_trips_through_the_snapshot`.
2. **Persistent instance (§4) — Landed.** `contribute-roots` opens the round with a fresh
   instance, `contribute-edges` reuses it, `annotate-symbols` reuses it and closes the round
   by dropping it (unconditionally — success or trap); fuel is re-armed to `FUEL_PER_CALL`
   before every hook, keeping per-call budget semantics exactly; an out-of-order hook gets a
   defensively fresh instance, never another round's state. Made observable through the
   reference guest's `staged_`/`fresh_` scenarios (state must carry roots → edges within a
   round; a second round on the same `WasmPlugin` must start clean), asserted by the
   compliance suite's `external_wasm_plugin_hooks_affect_a_real_check`.
3. **Read surface (§5) — Landed.** Call-site facts (`FileFacts.string_call_args`, JS-TS
   extracting first; persisted onto `FileNode`, refreshed by the patch), the native
   `GraphView` v2 (`packages`/`package_of`, `imports_of`/`importers_of`/`references_to` with
   R1+R2, `string_call_sites_in`, plus the bulk accessors host bridges snapshot from), the
   eight additive WASM imports with their new records, `EdgeKind::ReferencesFile` + the
   file-target edge sink mapping + reachability consumption + the RFC 0005 containment rule,
   and `kndo:express` consuming `packages()`. One shared invalidation:
   `GRAPH_SCHEMA_VERSION` 15, facts `ENTRY_FORMAT_VERSION` 2, js `facts_schema_version` 9.
   Proven by `GraphView` unit tests (R1 included), JS extraction tests, and the compliance
   suite's `linked_`/`sited_` guest round trips. See the §5 landing note for the one honest
   deviation (the moot route/template detections).
4. **Adapter dependencies (§6) — Landed.** The fixpoint is literally shared, not mirrored:
   `compose_plugins`'s implication machinery was generified over a kind-neutral
   `CandidateIdentity { id, dependencies }` (`crates/kndo/src/lib.rs`'s `activation` module)
   and `compose_adapters` now seeds all three tiers with `Option<ActivationReason>`
   (project-local → `ProjectLocal`, compiled-in → `BuiltinAlwaysOn` — both unconditional,
   activation only ever gates the global tier for adapters — global → its own rules), runs
   the same `imply_fixpoint`/`collect_missing`, and filters the composed global set by the
   post-fixpoint state. `ResolvedAdapter` carries `dependencies` +
   `active: Option<ActivationReason>`, `AdapterResolution`/`GlobalAdapterCandidate` and the
   engine's `DoctorAdapterInfo` gained the §6-promised fields, and `kndo doctor` renders
   adapter dependencies, missing adapter dependencies, and reason-aware candidate status
   ("active (dependency of X)") through one status helper shared with the plugin sections.
   Co-installation needed zero code: the install worklist always recursed over
   `ProbedDescriptor.dependencies`, which `wasm_probe`'s adapter arm fills identically to
   its plugin arm. Proven against real components by
   `examples/kndo-adapter-wrapper-demo` (`id: "kwrap"`, `dependencies: ["kdemo"]`) in
   `crates/kndo/tests/adapter_dependency_implication.rs`: with only the wrapper's marker
   present, kdemo joins composition as `ImpliedBy("kwrap")` and its analysis genuinely fires.
5. **Author kit + compat matrix (§7) — Landed.** `kndo plugin verify <component.wasm>`
   (`kndo::verify`, CLI presentation only): the discovery loaders decide the kind, the
   descriptor is reported with lint-grade warnings (plain-name id, empty activation), and the
   hooks are driven for real — the component dropped project-local into a synthesized fixture
   project, one genuine full check, contributions read back from the audit record. That audit
   record is the doctor deliverable: `run_plugin_round` counts what each plugin *resolved*
   into the graph (roots, edges, annotations), both build paths persist it as a tiny cache
   sidecar (`plugin-contributions.json` — warm snapshot hits change nothing, so the record
   stays accurate without living in the snapshot), and `kndo doctor` renders it
   ("plugin contributions (last recorded run)"). The compat matrix is
   `crates/kndo-plugin-api/tests/compat_matrix.rs` over **pre-built components committed**
   under `tests/compat/` (the reference adapter and hooks-demo plugin as built at landing),
   loaded and hook-driven with no wasm toolchain in the loop, plus its own named CI job; the
   shared `tests/harness/mini_adapter.rs` keeps its fixture identical to the compliance
   suite's. The authoring skeleton already lived in authoring.md §3 — it gained the `verify`
   inner loop, and the threat model is now explicit as wasm-abi.md §9 (residual risk: lying
   about graph facts, i.e. suppressed findings — mitigated by provenance tags and the doctor
   audit record; never exfiltration or code execution).

### 9. Explicitly out of scope

- **Plugin-contributed findings.** The most-demanded future capability — third-party *rules*
  — deliberately gets its own design cycle (RFC 0018, committed to as a draft): it touches
  the zero-false-positive promise itself and must be namespaced (`plugin:<coordinate>/<rule>`),
  severity-channeled, and attributed so it can never dilute the core guarantee. Nothing in
  this RFC's surface blocks it; §5's read completeness is its prerequisite.
- **Cross-kind dependencies** (§6's note).
- **Version constraints / a dependency solver** — RFC 0015 §3's rejection stands until a
  live ecosystem produces real pain.
- **A plugin registry.** Coordinates are already registry-shaped (RFC 0015 §2); GitHub
  releases remain the only channel for now.

## RFC 0018: Plugin-contributed findings

**Status:** Accepted & landed (the acceptance bar in §6 is met: the fixture suite
`crates/kndo-core/tests/plugin_findings.rs` exercises declare → emit → render → baseline →
suppress → gate opt-in end to end, the compliance suite drives the same path over the WASM
boundary, and the zero-FP rescoping shipped in the same change — RFC 0005 §9's scope note and
output-schema §2/§6) · **Depends on:** RFC 0003 (plugin system), RFC 0005 (analyses & the
zero-false-positive standard), RFC 0006 (CLI & gating), RFC 0015 (identity), RFC 0016/0017
(component model & read surface)

**Landing notes — the open questions of the draft, decided:**

1. **Config surface (was §5.1):** `[plugins.gate]` as specified in §2.2 — a flat table of
   `"<coordinate>"` / `"<coordinate>/<rule>"` → `"off" | "error" | "warning" | "info"`,
   per-rule key winning. It is the FIRST and only part of `kndo.toml` the core reads (the
   RFC 0006 §7 config subsystem remains unimplemented; this table is safe to read in
   isolation because it affects only the severity-channel mapping, never the graph or any
   cached artifact). Path-scoped gating rides on the existing suppression machinery instead.
2. **Rule versioning (was §5.2):** confirmed — baseline entries keyed on a renamed/retired
   rule's category age out as stale, identical to a deleted core finding. Nothing special
   was needed.
3. **Budget (was §5.3):** one fuel budget for the hook (`FUEL_PER_CALL`, like every hook);
   per-rule sub-budgets stay unbuilt until a real component needs them.
4. **Noise ceiling (was §5.4):** yes — 500 findings per rule per run, truncation reported as
   a run diagnostic (mirroring the content channel's budget-plus-diagnostic shape).
5. **Severity per finding (decided during design):** a finding has NO severity of its own —
   its severity IS its rule's declared severity, one per rule, so the wire record carries
   none and a guest cannot vary it per emission.
6. **WASM evolution (decided during implementation):** a world's exports are mandatory, so
   the new exports live on a second world in the same package — `plugin-findings` = `plugin`
   + `rules` + `contribute-findings`. The host probes `plugin-findings` first and falls back
   to `plugin`; every already-built v1 component keeps working unchanged (the pinned compat
   matrix proves it), and a findings-capable component is never silently demoted.
7. **Execution point (decided during implementation):** the finding round runs POST-assembly
   on every path — cold build, incremental patch, and warm snapshot hit — because findings
   are output, not graph state: nothing is persisted, so nothing can go stale, and no
   snapshot format changed. A findings-only plugin declares `mutates_graph = false` and
   costs the graph fast paths nothing.

### 1. The capability, and why it is its own RFC

Everything plugins can do today shapes what the core analyses conclude — roots, edges,
classifications, annotations feed RFC 0005's own rules, and the *verdicts* stay kndo's.
The most-demanded next step for a published platform is the opposite: third-party **rules** —
"flag every `api.call('v1/…')` as deprecated", "this company's DI annotations must not be on
private classes", "translations missing from this catalog are findings". The read surface
those rules need already exists (RFC 0017 §5 made it complete over the graph's data model —
its stated prerequisite for this RFC). What does *not* exist is a way for a plugin's verdict
to reach the output.

This is deliberately not a bullet point on another RFC because it touches the one promise
everything else is built on: **core findings have zero false positives** (RFC 0005 §1). A
host cannot audit a third-party rule's precision — it can't even state what the rule *means*.
So the design problem is not "how do plugin findings get emitted" (a sink and a hook, §4) but
"how does an unauditable verdict coexist with an audited guarantee without diluting it". The
whole RFC is the containment design; the plumbing is an afternoon.

### 2. Design pillars

#### 2.1 Namespace: `plugin:<coordinate>/<rule>`

A plugin finding's `category` is `plugin:<plugin coordinate>/<rule-name>` — e.g.
`plugin:github.com/acme/kndo-deprecations/v1-api`. Three properties, all load-bearing:

- **Structural distinguishability.** A consumer (human, CI, agent, SARIF ingester) can
  partition core findings from plugin findings with a prefix test, no registry lookup. The
  bare categories of output-schema §6 remain exactly the set the zero-FP statement covers;
  the statement itself gets rescoped in prose to "categories without the `plugin:` prefix" —
  which changes nothing about its content today.
- **Attribution is the id.** The coordinate inside the category is RFC 0015 §2's fetchable
  identity — a finding always names the component that asserted it, in the field consumers
  already section by. `rule-name` is the plugin's own sub-identifier (lower-kebab, same
  charset as core categories).
- **Host-enforced.** The sink (§4) prepends the prefix from the plugin's own registered
  descriptor id; a plugin cannot emit a bare category, an off-namespace category, or another
  plugin's namespace, by construction rather than by review. (Reserved `kndo:` built-ins get
  `plugin:kndo:<name>/<rule>` — built-ins asserting convention rules are held to the same
  channel, not smuggled into the core guarantee.)

`group` for plugin findings is a new additive group, `convention` — output-schema §6 already
obligates consumers to render unknown groups rather than drop them, so this degrades
correctly on old consumers.

#### 2.2 The severity channel: advisory by default, gating by explicit opt-in

Plugin findings carry the same `severity` vocabulary (`error`/`warning`/`info`) as declared
by the rule — but **the exit-code gate ignores them by default**. `--fail-on warning` (the
pre-commit and CI shape, RFC 0006 §5) continues to mean "fail on core warnings"; a plugin
finding influences the exit code only when the user opts that rule (or plugin) into gating in
`kndo.toml`:

```toml
[plugins.gate]
"github.com/acme/kndo-deprecations" = "warning"   # whole plugin: gate at its declared severities
"github.com/acme/kndo-deprecations/v1-api" = "off" # per-rule override wins
```

The reasoning is the same as wasm-abi §9's threat model, extended one step: today a malicious
or sloppy component can only *suppress* findings; a finding-emitting component could *break
builds* — a much louder failure mode, and one that would make "install a plugin" a
riskier decision than this platform wants it to be. With the channel split, installing a
finding-emitting plugin is safe by default (its output is visible, sectioned, attributed —
and inert to exit codes), and making it enforceable is the same kind of deliberate,
per-coordinate trust decision as installing it was. Health (RFC 0005 §11) likewise excludes
plugin findings from the score — the score is a claim about the codebase under kndo's own
standards, not under any installed plugin's.

#### 2.3 What a plugin finding must carry

The sink accepts only well-formed verdicts, validated host-side:

- **A resolvable target** — the same `PluginTarget` (path, optional symbol) vocabulary as
  every other sink, resolved against the graph; a finding on a nonexistent target is dropped
  silently (the uniform miss behavior, RFC 0003 §2). Locations in the output are therefore
  always real graph objects with real spans where the target has one.
- **`confidence`** — required, same three-level vocabulary (RFC 0005 §1). The docs hold
  plugin authors to the same "silence over a guess" discipline the authoring guide already
  states; the field makes the claim inspectable even though the host can't verify it.
- **`message`** — the human sentence; and the **rule name** (§2.1), which doubles as the
  SARIF `rule.id` (namespaced, so third-party rules never collide with core ones in a SARIF
  store — output-schema §7's mapping needs no change beyond the longer id).
- **Finding id** — the standard recipe (output-schema §5) over the namespaced category, so
  baseline entries, suppressions, and agent fix-assertions work unchanged.

`kndo:allow` inline suppressions and the baseline already key on category + location, so both
mechanisms work for plugin findings with zero new machinery — a team can baseline a newly
installed plugin's existing findings exactly like a newly enabled core analysis.

#### 2.4 Determinism and the cache

Rule evaluation is a pure function of the graph and the plugin's content-channel reads —
both already folded into the graph cache key (RFC 0016 §6). Findings are computed *after*
assembly (they are output, not graph state), so the incremental patch is untouched: no new
strip-and-rerun obligation, no snapshot format change. R1 (RFC 0017 §2) applies as-is: rules
read adapter-derived data plus their own inputs, never other plugins' contributions, so a
finding set is identical whatever else is installed. The per-plugin audit record (RFC 0017
§7) gains a `findings` count alongside roots/edges/annotations.

### 3. What this deliberately does not allow

- **No severity `error` in the gate without opt-in** — and even opted-in, a plugin rule
  cannot exceed the severity it declared (a config can lower, never raise).
- **No core-category emission** (§2.1's host-enforced prefix) and **no mutation of core
  findings**: a plugin cannot edit, suppress, or reclassify a core finding — suppression
  stays a *user* action (inline comments, config, baseline). The existing mutation hooks
  already let a plugin prevent core findings honestly, by changing what the graph says.
- **No cross-plugin visibility** (R1) and **no plugin-to-plugin finding composition** —
  "meta-rules over other plugins' findings" would reintroduce composition-order dependence.
- **No auto-fixes.** A finding may *describe* a fix in prose; machine-applicable edits are a
  separate capability with a separate trust model, out of scope.

### 4. Mechanism sketch (informative until this RFC is accepted)

- Native: a fourth read-only hook, `contribute_findings(&GraphView, &ContentView, &mut
  FindingSink)`, default empty; `FindingSink::add(rule, target, severity, confidence,
  message)`. Not a graph-mutation hook: `mutates_graph` stays false for a findings-only
  plugin, preserving its snapshot fast path.
- WASM: one additive export on the `kndo:plugin` world (same evolution shape as every RFC
  0017 §5 addition — old components simply lack it; the host probes and skips), fuel-bounded
  per call like every other hook.
- Descriptor: rules are declared up front — `rules: list<rule-descriptor>` (name, one-line
  description, declared severity) — so `kndo doctor` and `kndo plugin verify` can show what
  a component *may* assert before it ever runs, and the gate config can be validated against
  declared rule names.
- Output: findings render in their own section (RFC 0009's human renderer sections by group —
  `convention` sorts after core groups), carry `delta` in diff modes like any finding, and
  flow to SARIF/agent formats unchanged.

### 5. Open questions (as drafted — each is decided in the landing notes above)

1. **Config surface.** Is `[plugins.gate]` the right shape, or does gating belong in the
   existing `[[rule]]` per-path override system (RFC 0006 §7) so path-scoped opt-in works
   from day one?
2. **Rule versioning.** When a plugin upgrade renames or retires a rule, what happens to
   baseline entries keyed on the old namespaced category? (Probably: they age out as stale,
   the same as a deleted core finding — needs confirming against RFC 0006 §6's baseline
   semantics.)
3. **Budget.** One fuel budget for `contribute_findings`, or a per-rule sub-budget so one
   pathological rule can't starve its siblings within the same component?
4. **Noise ceiling.** Whether the host should cap findings-per-rule-per-run (with a
   truncation diagnostic) so a buggy rule cannot flood the report — leaning yes, mirroring
   the content channel's budget-plus-diagnostic shape.

### 6. Acceptance bar (met — see the Status block)

This RFC graduates from Draft when (a) a real third-party-shaped rule — written as a fixture
plugin, not hypothesized — exercises the full path (declare → emit → render → baseline →
suppress → gate opt-in) and (b) the zero-FP rescoping lands in output-schema/RFC 0005 in the
same change, so the guarantee's wording and the mechanism ship atomically. Until then, the
namespace `plugin:` and the group `convention` are **reserved** — nothing else may claim
either.
