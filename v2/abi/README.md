# Writing external extensions

kndo's ABI is `kndo:vocab@1.0.0` — the WIT package under [`../wit/`](../wit/): one
`types` interface (the vocabulary, mirroring the native contract field for field)
and ONE world, `extension`. There is one species: an extension declares
everything it does in its spec and implements the hooks for the capabilities it
declared, in whatever combination —

| Cluster | Spec fields | Hooks | Gate |
| --- | --- | --- | --- |
| extraction | `extensions`, `claims`, `emits`, `manifests` | `extract`, `resolve`, `roots`, `packages`, `manifest-dependencies`, `unit-mates` | claims |
| conduct | `activation`, `mutates-graph`, `dependencies`, `requested-file-access`, `rules` | `contribute-roots`, `report-findings` | activation (+ `mutates-graph` for roots) |
| ingestion | `reads-reports` | `ingest` | activation |

A language is an extension with the extraction cluster; a framework plugin is
one with conduct; a coverage ingester is one with ingestion — and one component
may hold several clusters (the `acme-framework` reference guest speaks its own
file format AND contributes graph conduct; the old three-world taxonomy could
not hold that in one component, which is why it is gone).

Drop the built component into `<project>/.kndo/plugins/*.wasm` and the shipped
`kndo` binary picks it up on the next run — presence is the opt-in, and there is
ONE load path: the loader reads your spec and routes; it never guesses. External
extensions always compose AFTER the built-ins: an external cannot take a
built-in language's claims, and the built-in coverage ingester answers first
when both can. A component that fails to load never vanishes silently — it
becomes a `diagnostic` line on every report of that session.

The reference guests under [`guests/`](guests/) are the worked examples; each is
built exactly the way yours will be.

## You implement the real trait — every cluster alike

`kndo-contract` and `kndo-sdk` compile to `wasm32-unknown-unknown`, so an
external extension implements the REAL `Extension` trait — the same trait, the
same `EvidenceSink`, `ResolveContext`, `ConductSink` and content scope a
built-in uses — and exports it in one line:

```rust
use kndo_contract::extension::{Extension, ExtensionSpec};

#[derive(Default)]
struct MyExtension { /* … */ }

impl Extension for MyExtension { /* spec, plus the hooks you declared */ }

kndo_sdk::export_extension!(MyExtension);
```

The spec goes through the same two-stage builder: identity and extraction first;
`.conduct(activation, MutatesGraph::Yes|No)` is the key that unlocks `.rule`,
`.dependencies`, `.requested_file_access` and `.reads_reports` — declaring
conduct without deciding its gates does not compile. The SDK rebuilds the
resolve context from the host's enumeration imports, hands conduct hooks a graph
and content view backed by the conduct imports, and stubs the hooks you leave
unimplemented (the host never calls a hook your spec does not declare, so a stub
is dead bytes, never dead design).

Everything your extraction sink writes is replayed host-side through another
real `EvidenceSink` under your spec's declared streams — clamps,
undeclared-stream drops and the pairing rule apply to you exactly as to a
built-in. Everything your conduct sink writes lands under the engine's own
containment: findings under `ext:<coordinate>/<rule>` for rules your spec
declared, roots only if your spec says `mutates-graph` (which also bypasses the
persisted graph cache while you are active) — anything misdirected drops with a
described line on your contribution, never silently and never applied.
`ingest` receives report bytes the ENGINE located through your `reads-reports`
paths and returns records — what the report STATES; you never see source files.

## Phase discipline

Extraction hooks are pure in (file bytes, spec version); conduct hooks see the
assembled graph and your scoped content; `ingest` sees only its bytes. In native
code and through the SDK this is structural — a hook's signature carries only
what its phase provides. The raw wire imports are phase-scoped by the HOST: a
hand-rolled guest that calls a conduct import during extraction (see the
`rude-probe` guest) gets a named trap — `phase contract violation` — surfaced as
a diagnostic, never an answer.

## Identity, activation, budgets

- Your `coordinate` is identity and provenance (`github.com/you/thing` style).
  The `kndo:` namespace is built-ins only — a component claiming it is rejected
  at load.
- Activation gates conduct and ingestion; claims gate extraction. `always`, or
  any of `file-exists(glob)` / `manifest-dependency(name)`; the hand-written
  empty rule list is the dependency-only posture — reachable only through
  another extension's `dependencies`, the path for a framework that is an
  INDIRECT dependency of the projects it serves.
- Every guest call runs on a fresh instance with 50M fuel and a 256 MiB memory
  ceiling. A trap, an exhausted budget, or an over-grown memory all end the same
  way: that call contributes nothing, and the run continues. Fuel is
  instruction-counted, never wall-clock — your component behaves identically on
  a loaded machine and an idle one, which the byte-identity gates require.
- Conduct content is prefetched through the engine's own scoped view (200 files
  / 8 MiB per extension), so your budget is charged by declaration, not demand;
  a cut is reported on your contribution.

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

Every built-in conducting extension carries a baseline-then-plugin proof (the
`builtin_plugin_proofs` gate): a fixture run WITHOUT the component establishing
what fires, the run WITH it changing exactly what the component claims to
change, and the contribution asserted in full. Hold your extension to the same
bar; the compliance suite in `kndo-host-wasm/tests/` shows the shape. The four
pinned reference components under [`compat/`](compat/) are the compatibility
matrix: they run against every head of the host, unrebuilt, so the promise
"your binary keeps working" is a build-breaking fact rather than a sentence.
The WIT may still evolve in place before the first public release; each such
change re-pins the references in the same commit, and the ABI freezes at that
release.
