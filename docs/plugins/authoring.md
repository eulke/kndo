# Authoring kndo Plugins — the complete guide

**Status:** Normative-adjacent — this guide is the *practical* companion to the contracts it
links; where they disagree, [contracts/wasm-abi.md](../contracts/wasm-abi.md) (the ABI),
[contracts/core-traits.md](../contracts/core-traits.md) §3 (the `Plugin` trait), RFC 0003 (the
plugin system) and RFC 0015 (identity, dependencies, installation) win.

Everything here is exercised by real code in this repository: the two reference guests
(`examples/kndo-plugin-demo`, `examples/kndo-plugin-hooks-demo`) and the compliance suites that
build and run them from source on every test run. When in doubt, read those — they are the
guide's executable form.

## 1. Two extension kinds — pick the right one first

kndo has two external extension points, one `.wasm` file implements exactly one of them:

| | **Language adapter** (`kndo:adapter@0.1.0`) | **Plugin** (`kndo:plugin@0.1.0`) |
|---|---|---|
| Answers | "what does this *language* mean?" | "what does this *ecosystem/framework* mean?" |
| Claims files | yes (`claim` by path, then `extract`s facts from content) | never — reads the already-built graph |
| Direction | one-way: kndo calls you | bidirectional: you also query kndo (`list-files`, `symbols-in`) |
| Typical use | a language kndo doesn't ship (a DSL, a config format) | framework conventions: entry points nothing imports, wiring invisible to the language, symbols consumed from outside |
| Reference guest | `examples/kndo-plugin-demo` | `examples/kndo-plugin-hooks-demo` |

Rule of thumb: if your knowledge is about *files with an extension kndo doesn't understand*,
write an adapter. If it's about *code kndo already parses whose liveness/meaning the language
alone can't see* (routes, DI, templates-by-string-name, framework-consumed exports), write a
plugin. This guide covers plugins; adapters share the toolchain (§3) and differ per
wasm-abi.md §§1–3.

## 2. What a plugin can and cannot do

A plugin gets exactly four graph-mutation hooks (RFC 0003 §2) plus its descriptor:

- **`classify-file(path, current) -> option<file-class>`** — override a file's role/origin
  (`*.stories.tsx` → tooling, a generated-by-your-framework path → generated). Sees one file
  at a time, no graph queries.
- **`contribute-roots() -> list<contributed-root>`** — declare entry points: things alive even
  though nothing imports them (a Next-style page, a DI-registered bean, a route handler your
  framework discovers by convention).
- **`contribute-edges() -> list<contributed-edge>`** — declare references invisible to the
  language: template → component, route table → handler, config key → class. Every contributed
  edge is a `references` edge; plugins cannot mint new edge kinds.
- **`annotate-symbols() -> list<plugin-target>`** — mark symbols *externally consumed* (public
  SDK surface, FFI, serialization targets): exempts them from `internal-only`/
  `private-type-leak` narrowing suggestions.

`contribute-roots`/`contribute-edges`/`annotate-symbols` may call the host imports:
`list-files()` (every claimed file: path, role, origin), `symbols-in(path)` (each symbol:
name, kind, exported, member-of), `read-file(path) -> option<list<u8>>` (RFC 0016 §5's
content channel), and RFC 0017 §5's complete read surface — `packages()` / `package-of(path)`
(RFC 0011 package topology), `file-details(path)` (language, unit, package root),
`symbol-details(path, symbol)` (visibility rung, span), `imports-of(path)` /
`importers-of(path)` (file-import edges, both directions), `references-to(path, symbol)`
(every site referencing a symbol), and `call-sites-in(path)` (string-literal call sites the
adapter extracted — `res.render("index")`, `flags.isEnabled("checkout-v2")` — the fact to
build framework conventions on instead of ever re-parsing source). Everything answers from
adapter-derived data only: you never see another plugin's contributions, so your results
can't depend on what else is installed. `classify-file` gets none of these — it sees one
file at a time and nothing else.

**Edges can target whole files.** A `contributed-edge` whose `to` has no `symbol` becomes a
file-liveness edge ("if `from` is alive, that file is in use" — the template/asset shape).
Liveness is its entire meaning: it can rescue a file from `unused`, and it is ignored by
every analysis that would create a finding from an edge's existence, so a wrong edge can
never produce a false positive — only hide a true one, which is still a reason to be sure.

**`read-file` is scoped, not general.** It only ever answers a path matching *your own*
`requested-file-access` globs (declare them on your descriptor — an empty list means every
call returns `none`), and it's for files the language graph doesn't already claim and parse:
configs, manifests, templates. Reading a source file the adapter itself claims to
second-guess it — parsing a `.tsx` yourself instead of trusting `symbols-in` — is out of
contract even though nothing stops you mechanically; kndo's own `kndo:express` (reads
`package.json`'s `main`/`scripts`) and `kndo:nextjs` (reads `next.config.*` for a literal
`pageExtensions` array, no JS evaluation) are the reference examples, and both fall back to
their pre-channel behavior on anything they can't read or parse — never a guess. Reads are
budgeted (a generous but real per-run cap on distinct paths and total bytes); going over cuts
your plugin off from further reads for the rest of that run, not the run itself.

**That is the entire visible universe.** Still not exposed in v1: symbol spans, existing
edges, imports, annotations/attributes claimed-file content. If your convention needs
something the view doesn't carry (e.g. Java annotations for a Spring-style plugin), that's a
`GraphView` extension to propose upstream — not something to work around.

Targets are **named, never id-addressed**: `plugin-target { path, symbol: option<string> }`,
where `symbol` is a bare name or `Owner.name`. A target that doesn't resolve is **dropped
silently** — the same miss behavior adapters have. Consequence for you: contribute only facts
you are certain about. kndo's standard for its own analyses is zero false positives; a plugin
that roots things speculatively degrades every verdict downstream of it, and findings your
contributions influence are attributed to your plugin id (`Provenance::Plugin`).

What a plugin can never do, by construction: touch the filesystem or network *directly* (no
WASI is linked — a component importing WASI **fails to instantiate**; every byte you see
through `read-file` was matched against your own declared globs and fetched host-side, not a
live syscall you make), see another plugin's contributions, create new finding categories, or
crash the run — each hook call has a fuel budget (50M units); a trap or exhaustion degrades to
"this plugin contributed nothing this round", never a failed `kndo check`.

**Cost model you must know:** your plugin costs its hooks' own runtime, never a cache
penalty. Both of kndo's fast paths work with plugins registered (wasm-abi.md §5.4): the
snapshot cache folds your identity — id, version, and your component's own content hash —
into its key, and the incremental patch strips your previous contributions and re-runs your
hooks against the patched graph (RFC 0017 §3). An unchanged re-run costs a project the same
whether your plugin is installed or not. Activation rules (§5) still matter for a different
reason: a plugin that doesn't apply to a project shouldn't run its hooks there at all.

**Instance lifecycle (RFC 0017 §4):** your component is instantiated once per graph-mutation
round — `contribute-roots` first, then `contribute-edges` and `annotate-symbols` against the
same instance, which is dropped when `annotate-symbols` returns. You may keep state in
statics across the three hooks of one round (compute something in `contribute-roots`, reuse
it in `contribute-edges`); you can never carry state across rounds or runs — don't try, the
drop is unconditional. `classify-file` runs on separate, view-less instances at an earlier
pipeline phase; share nothing with it.

## 3. Toolchain & project setup

Requirements: Rust with the `wasm32-unknown-unknown` target (`rustup target add
wasm32-unknown-unknown`). No cargo-component, no wasm-tools, no WASI SDK.

```toml
# Cargo.toml
[package]
name = "my-framework-plugin"
version = "0.1.0"        # your version — see §7
edition = "2021"
publish = false           # this ships as a .wasm release asset, not a crate

[lib]
crate-type = ["cdylib"]   # required: a linkable wasm module, not an rlib

[dependencies]
wit-bindgen = "0.57"

[profile.release]
opt-level = "s"           # size over speed — the binary is a distribution artifact
lto = true
```

```rust
// src/lib.rs
use wit_bindgen as _; // marks the dep used — the macro below is a fully-qualified path

wit_bindgen::generate!({
    // Vendor kndo's WIT file into your repo (wit/plugin.wit) and point at it. It is the ABI
    // contract — copy it verbatim from the kndo version you target; do not edit it.
    path: "wit/plugin.wit",
    world: "plugin",
});

use crate::kndo::plugin::types::*;

struct MyPlugin;

impl Guest for MyPlugin {
    fn descriptor() -> PluginDescriptor {
        PluginDescriptor {
            id: "github.com/you/my-framework-plugin".to_string(), // §4 — the coordinate
            version: "0.1.0".to_string(),
            detection: vec!["package.json depends on @you/framework".to_string()],
            requested_file_access: Vec::new(),
            activation: vec![ActivationRule::ManifestDependency(
                "@you/framework".to_string(),
            )],
            dependencies: Vec::new(), // §6
        }
    }

    fn classify_file(_path: String, _current: FileClass) -> Option<FileClass> {
        None // hooks you don't need: return the neutral value, cost ~zero
    }

    fn contribute_roots() -> Vec<ContributedRoot> {
        let mut roots = Vec::new();
        for file in list_files() {
            // your convention here — e.g. files under a routes/ dir are entry points
            if file.path.starts_with("routes/") {
                roots.push(ContributedRoot {
                    target: PluginTarget { path: file.path.clone(), symbol: None },
                    kind: RootKind::Production,
                    confidence: Confidence::Certain, // only when it IS certain — see §2
                });
            }
        }
        roots
    }

    fn contribute_edges() -> Vec<ContributedEdge> { Vec::new() }
    fn annotate_symbols() -> Vec<PluginTarget> { Vec::new() }
}

export!(MyPlugin);
```

Build and componentize (what kndo's own compliance suites do — no external tool):

```bash
cargo build --release --target wasm32-unknown-unknown
```

```rust
// a tiny xtask/build script, or do it in CI — wit-component as a library:
let core = std::fs::read("target/wasm32-unknown-unknown/release/my_framework_plugin.wasm")?;
let component = wit_component::ComponentEncoder::default().module(&core)?.encode()?;
std::fs::write("my-framework-plugin.wasm", component)?;
```

(`cargo component build` produces the same artifact if you prefer the tool.)

Then point kndo at the artifact (RFC 0017 §7 — the author kit's inner loop):

```bash
kndo plugin verify my-framework-plugin.wasm
```

`verify` loads the component through the exact loaders `kndo::open` discovery uses, reports
which world accepted it and everything its descriptor declares, warns about the legal-but-
probably-wrong shapes this guide calls out (plain-name ids, empty `activation`), and then
drives every hook for real: your component is dropped project-local into a synthesized
fixture project and a genuine full check runs — what you contributed comes back from the
run's audit record. Zero contributions on the generic fixture is a note, not a failure; for
convention-specific behavior, follow §8's baseline-then-plugin fixture shape.

## 4. Identity: your id IS your coordinate (RFC 0015 §2)

- **External plugins**: `id` must be the source coordinate the plugin can be fetched from —
  `github.com/<owner>/<repo>`. Identity = location: nothing to squat, no name collisions, and
  it's what other plugins' `dependencies` entries and the installer resolve against. When the
  installer (RFC 0015 §4) fetches your coordinate, it verifies your descriptor declares exactly
  that id — an impersonating component is rejected.
- **`kndo:` is reserved** for built-ins (`kndo:coverage-lcov`, `kndo:nextjs`, …). The host
  rejects any external component claiming it — your plugin will simply fail to load.
- A plain name (`"my-thing"`) is legal only for hand-dropped `.kndo/plugins/` files, and can
  never be the target of a `dependencies` edge. Use a real coordinate from day one.

## 5. Activation: when does your plugin run?

Three tiers, three rules (RFC 0003 §4 + RFC 0015 §3):

1. **Project-local** (`.kndo/plugins/your.wasm` in a repo): always active. Presence is the
   opt-in; `activation` is ignored.
2. **Globally installed** (the per-machine directory — `~/.local/share/kndo/plugins` on Linux,
   platform equivalents elsewhere, `KNDO_PLUGIN_DIR` override): active only if one of your
   `activation` rules matches the project, or an active plugin depends on you. An **empty**
   `activation` list never self-activates globally — declare real rules.
3. **Implied**: an active plugin listing your coordinate in `dependencies` activates you,
   transitively.

Two rule forms, both cheap filesystem checks evaluated before your code ever runs:

- `FileExists(glob)` — some file under the project root matches (`"next.config.*"`).
- `ManifestDependency(name)` — any `package.json`/`Cargo.toml` in the project (monorepo
  packages included, `node_modules` excluded) declares that dependency.

Write the tightest rule that is *always* true for projects your conventions apply to. Remember
the cost model (§2): every project you activate on pays full rebuilds. `kndo doctor` shows every
candidate with the exact rule that fired or didn't — your users' first debugging stop, and
yours.

## 6. Dependencies between plugins (RFC 0015 §3)

If your plugin wraps another ecosystem — your company framework re-exports Next.js — declare
it:

```rust
dependencies: vec!["kndo:nextjs".to_string()],
```

Semantics, exactly two and nothing more: installing you installs them (once the installer
lands), and *your* activation activates them, transitively. There are **no version constraints
and no ordering** between plugins — plugins cannot read each other's contributions, so there is
no inter-plugin ABI to be compatible about. List only plugins whose conventions genuinely
surface to *your* users (the wrapper relationship) — every listed dependency activates on every
project you activate on, and pays the §2 cost there. A dependency that isn't installed is never
an error: your plugin still runs, and `kndo doctor` names the missing coordinate.

## 7. Versioning & compatibility — the contract you're building against

Three versions matter, and they are independent:

1. **Your `descriptor().version`** — yours entirely. Semver recommended; it's shown in
   `kndo doctor` and (once the installer lands) selected by `@vX.Y.Z` git tags on your repo.
   Tag releases; the tag is what users pin.
2. **The WIT package version** (`kndo:plugin@0.1.0` — first line of the WIT file). This is the
   ABI. kndo's promise: **at kndo 1.0 this freezes** — a component built against a frozen
   package version keeps working against every compatible host indefinitely; a breaking change
   means a new package version, never a silent reinterpretation (wasm-abi.md §8). **Before
   kndo 1.0, honesty over comfort: `0.1.0` may still evolve in place** (it has — `activation`
   and `dependencies` were added to the descriptor record after the first cut), and a record
   gaining a field is a break for already-built components. Pre-1.0 plugin authors should
   expect to re-vendor the WIT and rebuild against new kndo releases. This is exactly what the
   M6 "schema/ABI freeze" milestone ends. The compatibility promise itself is CI-enforced, not
   aspirational: pre-built, committed v1 components run against the HEAD host on every push
   (`crates/kndo-plugin-api/tests/compat_matrix.rs` — RFC 0017 §7), so a host change that
   would break your already-shipped binary breaks kndo's own build first.
3. **kndo's own binary version** — irrelevant to you beyond which WIT version it hosts.

Maintenance checklist per kndo release, until the freeze: diff your vendored `wit/plugin.wit`
against the release's; if changed, re-vendor, rebuild, re-tag. After the freeze: nothing, until
a `kndo:plugin@0.2.0` ever exists — and `0.1.0` components keep working even then.

## 8. Testing your plugin

- **First**: `kndo plugin verify your-component.wasm` (§3) — load, descriptor lint, and a
  generic fixture drive in one command, before you build any fixture of your own.
- **Locally, end to end**: build + componentize (§3), drop the `.wasm` into a test project's
  `.kndo/plugins/`, run `kndo check` and `kndo doctor` there. Doctor shows whether you loaded,
  activated, and why — plus, after a run, what every plugin actually contributed (roots,
  edges, annotations: the RFC 0017 §7 audit record). For the global tier, point
  `KNDO_PLUGIN_DIR` at a scratch directory.
- **Assertion-style**: make a fixture project exhibiting your conventions, run kndo *without*
  your plugin (baseline — the findings your plugin should fix must actually fire, or your test
  is vacuous), then *with* it, and assert the delta. This is precisely how kndo's own
  `plugin_compliance.rs` and `external_plugin.rs` suites work — copy their shape.
- **What "correct" means**: the zero-false-positive discipline applies to you too. A root you
  contribute keeps code alive forever; if your convention has exceptions, use
  `Confidence::Probable`/`Possible` instead of `Certain`, or don't contribute the fact at all —
  silence over a guess.

## 9. Distributing

Today: hand your users the `.wasm` (they drop it in `.kndo/plugins/` or the global directory).

To be installable by `kndo plugin install <coordinate>` (RFC 0015 §4, shipped): publish GitHub
releases on the repo your coordinate names, tagged `vX.Y.Z`, carrying exactly one componentized
`.wasm` release asset plus a `checksums.txt` (`sha256sum` format) — the same artifact
convention kndo itself releases under (RFC 0014 §3). The installer verifies the checksum, then
identity binding: your descriptor's `id` must equal the coordinate the user typed, or the
install is refused (§4 above). Private repos need only a `GITHUB_TOKEN`/`GH_TOKEN` in the
user's environment — their existing GitHub credential, nothing plugin-specific.

## 10. Worked references in this repository

- `examples/kndo-plugin-hooks-demo` — a complete plugin: all four hooks, host-import queries,
  activation rule, empty dependencies. ~110 lines.
- `examples/kndo-plugin-demo` — a complete adapter for an invented language.
- `examples/kndo-adapter-wrapper-demo` — a wrapper adapter whose whole point is
  `dependencies: ["kdemo"]` (RFC 0017 §6): activating it co-activates the adapter it wraps.
- `crates/kndo-plugin-api/tests/plugin_compliance.rs` — the baseline-then-plugin test shape
  (§8), plus proof that the two ABIs reject each other's components.
- `crates/kndo/tests/global_plugin_activation.rs` — the global-tier activation test shape,
  including `KNDO_PLUGIN_DIR`.
- `crates/kndo-plugin-nextjs` / `crates/kndo-plugin-express` — the first real convention
  plugins (built-in, but the trait is identical to what a WASM guest implements): pure path
  classifiers unit-tested in isolation, spec-first design
  ([nextjs.md](nextjs.md) / [express.md](express.md)), and
  `crates/kndo/tests/builtin_convention_plugins.rs` as the baseline-then-plugin (§8) fixture
  suite proving activation gating and contributed facts together.
