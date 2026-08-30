# Writing external components

kndo's ABI is `kndo:vocab@1.0.0` — the WIT package under [`../wit/`](../wit/): one
`types` interface (the vocabulary, mirroring the native contract field for field)
and three worlds. A component targets exactly one world:

| World | You provide | The engine gets |
| --- | --- | --- |
| `adapter` | a language: extraction, import resolution, manifest roots/packages/dependency names, unit mates | one more first-class language |
| `plugin` | graph roots the language cannot know, advisory findings | one more plugin, containment included |
| `coverage-ingester` | parsed records from a coverage report | test-execution evidence for `untested` |

Drop the built component into `<project>/.kndo/plugins/*.wasm` and the shipped
`kndo` binary picks it up on the next run — presence is the opt-in; the file name
does not matter, the world it targets does. External components always compose
AFTER the built-ins: an external adapter cannot take a built-in language's files,
and the built-in coverage ingester answers first when both can. A component that
fails to load never vanishes silently — it becomes a `diagnostic` line on every
report of that session.

The three reference guests under [`guests/`](guests/) are the worked examples;
each is built exactly the way yours will be.

## An adapter is the same code, natively or here

`kndo-contract` and `kndo-sdk` compile to `wasm32-unknown-unknown`, so an external
adapter implements the REAL `LanguageAdapter` — the same trait, the same
`EvidenceSink`, the same `ResolveContext` queries a native adapter uses — and
exports it in one line:

```rust
use kndo_contract::adapter::LanguageAdapter;

#[derive(Default)]
struct MyAdapter { /* … */ }

impl LanguageAdapter for MyAdapter { /* spec, extract, resolve, … */ }

kndo_sdk::export_adapter!(MyAdapter);
```

The SDK rebuilds the resolve context from the host's enumeration imports, so
`cx.contains(...)`, `cx.package(...)`, `cx.package_of(...)` answer from the same
project data as natively. Everything your sink writes is replayed host-side
through another real `EvidenceSink` under your spec's declared streams — clamps,
undeclared-stream drops and the pairing rule apply to you exactly as to a
built-in, and every drop is a visible diagnostic on your file.

`kmini-adapter` is the reference: a whole invented language in one file,
hand-scanned — nothing about the ABI requires tree-sitter.

## Plugins and ingesters speak the wire records

Their native trait lives in the engine crate, which does not cross to `wasm32`,
so these two worlds are implemented against the generated bindings directly (see
`probe-plugin` and `records-ingester`):

- **plugin** — export `spec`, `mutates-graph`, `contribute-roots`,
  `report-findings`; import `graph-paths`, `graph-contains`, `read-file`.
  `mutates-graph` is mandatory and load-bearing: `true` means your roots apply
  AND the persisted graph cache is bypassed while you are active; an
  advisory-only plugin answers `false` or turns incremental analysis off for
  everyone. Findings land under `plugin:<coordinate>/<rule>` for rules your spec
  declared; anything misdirected — an undeclared rule, a target the graph does
  not hold, a root from a `false`-answering component — is dropped with a
  described line on your contribution, never silently and never applied.
  `read-file` is scoped to your spec's `requested-file-access` globs and metered
  by the per-plugin budget (200 files / 8 MiB); the host prefetches every
  declared match before instantiating you, so the budget is charged by
  declaration, not by demand.
- **coverage-ingester** — export `spec` and `ingest`; no imports. The host finds
  the report through your spec's `requested-file-access` entries (root-relative
  report paths, tried in order — run output is gitignored, so discovery never
  sees it), pushes you the bytes, and maps the records you return against the
  project. You never see source files; return what the report STATES.

## Identity, activation, budgets

- Your `coordinate` is identity and provenance (`github.com/you/thing` style).
  The `kndo:` namespace is built-ins only — a component claiming it is rejected
  at load.
- Activation is evaluated the same way for every plugin, external or built-in:
  `always`, or any of `file-exists(glob)` / `manifest-dependency(name)` — the
  names every claiming adapter reports from the project's own manifests. An
  empty rule list means "reachable only through another plugin's
  `dependencies`".
- Every guest call runs on a fresh instance with 50M fuel and a 256 MiB memory
  ceiling. A trap, an exhausted budget, or an over-grown memory all end the same
  way: that call contributes nothing, and the run continues. Fuel is
  instruction-counted, never wall-clock — your component behaves identically on
  a loaded machine and an idle one, which the byte-identity gates require.

## Building

```sh
cargo build --release --target wasm32-unknown-unknown
```

with `crate-type = ["cdylib"]`, then componentize the core module — the
`wit-component` crate does it in-process, no extra CLI:

```rust
wit_component::ComponentEncoder::default().module(&core_wasm)?.encode()?
```

(`cargo xtask pin-abi` in this repository is exactly that loop over the
reference guests.)

## Prove it the way the built-ins are proven

Every built-in plugin carries a baseline-then-plugin proof (the
`builtin_plugin_proofs` gate): a fixture run WITHOUT the component establishing
what fires, the run WITH it changing exactly what the component claims to
change, and the contribution asserted in full — applied counts, described drops,
budget state. Hold your component to the same bar; the compliance suite in
`kndo-host-wasm/tests/` shows the shape. The pinned copies of the reference
guests under [`compat/`](compat/) are the compatibility matrix: they run against
every head of the host, unrebuilt, so the promise "your binary keeps working" is
a build-breaking fact rather than a sentence. The WIT may still evolve in place
before the first public release; each such change re-pins the references in the
same commit, and the ABI freezes at that release.
