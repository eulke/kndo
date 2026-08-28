# Writing a plugin

This is the one guide to writing a kndo extension. It is the *practical* companion to the
contracts it names; where they disagree, `internal/contracts/wasm-abi.md` (the ABI),
`internal/contracts/core-traits.md` §3 (the `Plugin` trait), RFC 0003 (the plugin system) and
RFC 0015 (identity, dependencies, installation) win.

Everything here is exercised by real code in the kndo repository — the reference guests under
`examples/` and the compliance suites that build and run them on every test run. When in
doubt, read those: they are this guide's executable form.

The idea-to-working-component loop is four commands, none of which require reading kndo's
source:

```console
$ kndo plugin new my-conventions       # scaffold a component crate, ABI vendored
$ cd my-conventions
$ kndo plugin build                    # cargo build + componentize → my-conventions.wasm
$ kndo plugin verify my-conventions.wasm
$ cp my-conventions.wasm someproject/.kndo/plugins/   # try it for real; kndo doctor shows it
```

You write ordinary Rust compiled to `wasm32-unknown-unknown`
(`rustup target add wasm32-unknown-unknown`); `kndo plugin build` handles the
componentization so you never touch wasm tooling directly. No cargo-component, no wasm-tools,
no WASI SDK.

## Plugin or adapter?

One `.wasm` file implements exactly one of kndo's two extension points.

| | **Language adapter** (`kndo:adapter@0.1.0`) | **Plugin** (`kndo:plugin@0.1.0`) |
|---|---|---|
| Answers | "what does this *language* mean?" | "what does this *ecosystem/framework* mean?" |
| Claims files | yes (`claim` by path, then `extract`s facts from content) | never — reads the already-built graph |
| Direction | one-way: kndo calls you | bidirectional: you also query kndo (`list-files`, `symbols-in`) |
| Typical use | a language kndo doesn't ship (a DSL, a config format) | framework conventions: entry points nothing imports, wiring invisible to the language, symbols consumed from outside |
| Reference guest | `examples/kndo-plugin-demo` | `examples/kndo-plugin-hooks-demo` |

Rule of thumb: if your knowledge is about *files with an extension kndo doesn't understand*,
write an adapter. If it's about *code kndo already parses whose liveness or meaning the
language alone cannot see* — routes, DI registrations, scheduled jobs, templates named by
string, framework-consumed exports — write a plugin. Adapters share the toolchain below and
get their own section near the end.

## The scaffold

`kndo plugin new <dir>` writes a compilable crate (add `--adapter` for a language adapter):

```text
my-conventions/
├─ Cargo.toml          # cdylib, wit-bindgen, size-optimized release profile
├─ wit/plugin.wit      # the ABI, vendored from the exact kndo you ran — do not edit
├─ src/lib.rs          # a documented, compiling skeleton of every hook
├─ README.md
└─ .gitignore
```

The WIT contract is vendored from your kndo binary, so guest and host always agree — the
printed WIT is byte-identical to what that binary's host was compiled against, never a
possibly-mismatched git checkout. To retarget a crate at a newer kndo later:
`kndo plugin wit plugin > wit/plugin.wit` and rebuild. (`kndo plugin wit adapter` prints the
adapter world; `kndo plugin wit` alone lists them.)

What the scaffold contains, for reference — or for setting a crate up by hand:

```toml
# Cargo.toml
[package]
name = "my-framework-plugin"
version = "0.1.0"         # your version — see "Versioning" below
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
    // The ABI contract, vendored by `kndo plugin new` (or `kndo plugin wit`). Do not edit it.
    path: "wit/plugin.wit",
    world: "plugin",
});

use crate::kndo::plugin::types::*;

struct MyPlugin;

impl Guest for MyPlugin {
    fn descriptor() -> PluginDescriptor { /* see below */ }

    fn classify_file(_path: String, _current: FileClass) -> Option<FileClass> {
        None // hooks you don't need: return the neutral value, cost ~zero
    }

    fn contribute_roots() -> Vec<ContributedRoot> {
        let mut roots = Vec::new();
        for file in list_files() {
            // your convention here — e.g. files under routes/ are entry points
            if file.path.starts_with("routes/") {
                roots.push(ContributedRoot {
                    target: PluginTarget { path: file.path.clone(), symbol: None },
                    kind: RootKind::Production,
                    confidence: Confidence::Certain, // only when it IS certain
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

`kndo plugin build` runs the cargo build and componentizes the result in one step — the same
`wit_component::ComponentEncoder` call kndo's own compliance suites make. Doing it by hand
(CI without a kndo binary, or just preference) is two steps:

```bash
cargo build --release --target wasm32-unknown-unknown
```

```rust
// wit-component as a library (or `cargo component build`, same artifact):
let core = std::fs::read("target/wasm32-unknown-unknown/release/my_framework_plugin.wasm")?;
let component = wit_component::ComponentEncoder::default().module(&core)?.encode()?;
std::fs::write("my-framework-plugin.wasm", component)?;
```

## The descriptor

Everything kndo decides about your plugin *before running it* comes from `descriptor()`:

```rust
PluginDescriptor {
    // Your id IS your coordinate: the GitHub repo the component is fetched from.
    // A plain name works for hand-dropped .kndo/plugins/ files but can never be
    // installed or depended on. The kndo: namespace is reserved and will not load.
    id: "github.com/you/my-conventions".to_string(),
    version: "0.1.0".to_string(),
    // Prose for a gate `activation` below cannot express — an always-on plugin naming
    // the files it looks for. (Shown by kndo doctor.) If your gate IS an activation rule,
    // leave this empty: doctor already shows the rule, and prose beside it is the same
    // fact twice, free to drift.
    detection: Vec::new(),
    // Files outside the language graph you need to read — globs served through the
    // host's content channel. Empty = no reads.
    requested_file_access: vec!["**/my-framework.config.*".to_string()],
    // When a globally installed copy turns on. An empty list NEVER self-activates
    // globally — declare a real rule.
    activation: vec![ActivationRule::ManifestDependency("my-framework".to_string())],
    // Coordinates of plugins whose conventions are part of yours: installing you
    // installs them; activating you activates them — even when their own rules cannot
    // fire. A framework that uses Express internally is not `express` in its users'
    // manifests, so `kndo:express` never self-activates for them; naming it here is what
    // reaches it. Transitive, so wrapper chains compose.
    dependencies: Vec::new(),
}
```

### Identity: your id IS your coordinate (RFC 0015 §2)

- **External plugins**: `id` must be the source coordinate the plugin can be fetched from —
  `github.com/<owner>/<repo>`. Identity = location: nothing to squat, no name collisions, and
  it is what other plugins' `dependencies` entries and the installer resolve against. When the
  installer fetches your coordinate it verifies your descriptor declares exactly that id — an
  impersonating component is rejected.
- **`kndo:` is reserved** for built-ins (`kndo:coverage-lcov`, `kndo:nextjs`, …). The host
  rejects any external component claiming it; your plugin simply fails to load.
- A plain name (`"my-thing"`) is legal only for hand-dropped `.kndo/plugins/` files, and can
  never be the target of a `dependencies` edge. Use a real coordinate from day one.

### Activation: when does your plugin run?

Three tiers (RFC 0003 §4 + RFC 0015 §3):

1. **Project-local** (`.kndo/plugins/your.wasm` in a repo): always active. Presence is the
   opt-in; `activation` is ignored.
2. **Globally installed** (`~/.local/share/kndo/plugins` on Linux, platform equivalents
   elsewhere, `KNDO_PLUGIN_DIR` override): active only if one of your `activation` rules
   matches the project, or an active plugin depends on you. An **empty** `activation` list
   never self-activates globally — declare real rules.
3. **Implied**: an active plugin listing your coordinate in `dependencies` activates you,
   transitively.

Two rule forms, both cheap filesystem checks evaluated before your code ever runs:

- `FileExists(glob)` — some file under the project root matches (`"next.config.*"`).
- `ManifestDependency(name)` — any `package.json`/`Cargo.toml`/`pom.xml`/… in the project
  (monorepo packages included, `node_modules` excluded) declares that dependency.

Write the tightest rule that is *always* true for projects your conventions apply to.
`kndo doctor` shows every candidate with the exact rule that fired or didn't — your users'
first debugging stop, and yours.

**Activation is not the same question as contribution.** A plugin can be correctly active on a
project and correctly contribute nothing there: `kndo:libsass-maven-plugin` activates on any
project containing Sass and contributes only where a pom declares the compilation. Keep the two
separate in your own tests, or a passing proof will be measuring the wrong thing.

### Dependencies between plugins (RFC 0015 §3)

If your plugin wraps another ecosystem — your company framework re-exports Next.js — declare
it:

```rust
dependencies: vec!["kndo:nextjs".to_string()],
```

Semantics, exactly two and nothing more: installing you installs them, and *your* activation
activates them, transitively. There are **no version constraints and no ordering** between
plugins — plugins cannot read each other's contributions, so there is no inter-plugin ABI to
be compatible about. List only plugins whose conventions genuinely surface to *your* users
(the wrapper relationship): every listed dependency activates on every project you activate
on. A dependency that isn't installed is never an error — your plugin still runs, and
`kndo doctor` names the missing coordinate.

## The hooks

A plugin exports up to six hooks. Return empty collections for the ones you don't need.

| Hook | Contributes | Notes |
|---|---|---|
| `classify_file(path, current)` | a corrected role/origin for one file | `None` = no opinion (the common case); sees one file at a time, no graph queries |
| `contribute_roots()` | entry points: `(target, root-kind, confidence)` | a root keeps code alive forever — use `certain` only when the framework *guarantees* invocation |
| `contribute_edges()` | references the language can't see | route-string → handler, template → class. Every contributed edge is a `references` edge; plugins cannot mint new edge kinds |
| `annotate_symbols()` | symbols consumed from outside the graph | exempts them from `internal-only`/`private-type-leak`, never from `unused` |
| `rules()` | the findings you may emit: name, description, one severity each | undeclared rules are dropped; declared rules show in `kndo doctor` before you ever run |
| `contribute_findings()` | findings under your declared rules | emitted as `plugin:<coordinate>/<rule>`, advisory unless the user gates them |

Targets are always **named** — `{ path, symbol: Option<String> }`, where `symbol` is a bare
name or `Owner.name` — never internal ids. A target that doesn't resolve is **dropped
silently**, the same miss behavior adapters have, and counted in `kndo doctor`'s
dropped-contribution record and in `kndo plugin verify`'s report.

**Edges can target whole files.** A `contributed-edge` whose `to` has no `symbol` becomes a
file-liveness edge ("if `from` is alive, that file is in use" — the template/asset shape).
Liveness is its entire meaning: it can rescue a file from `unused`, and it is ignored by every
analysis that would create a finding from an edge's existence, so a wrong edge can never
produce a false positive — only hide a true one, which is still a reason to be sure.

### Reading the graph

Hooks read, they never own: the host serves a snapshot of adapter-derived facts through query
imports —

- `list-files` (path, role, origin) and `file-details` (language, unit, package root);
- `symbols-in(path)` (name, kind, exported, member-of) and `symbol-details` (visibility rung,
  span);
- `packages` / `package-of`;
- `imports-of` / `importers-of` / `references-to`;
- `call-sites-in(path)` — including calls with string-literal arguments (`res.render("index")`,
  `flags.isEnabled("checkout-v2")`), the raw material for route conventions and the fact to
  build on instead of ever re-parsing source;
- `attr-strings-in(path)` — its attribute sibling: `(attribute head, key, literal, decorated
  declaration)` for every string written inside an attribute or annotation
  (`#[serde(skip_serializing_if = "is_zero")]`, `@JsonDeserialize(using = "Foo")`). What the
  string *means* is your plugin's knowledge, deliberately: `skip_serializing_if = "f"` names a
  function and `rename = "f"` names a wire label, and no adapter can tell them apart without
  knowing the framework — which is the whole reason the fact stops at the key;
- `read-file(path)` — the **content channel**: raw bytes of files matching your declared
  `requested_file_access` globs, from the discovered (gitignore-filtered) tree only, under a
  per-run byte budget. Out-of-glob and over-budget reads answer `none`.

You only ever see *adapter-derived* data — never another plugin's contributions — so plugin
behavior is independent of what else is installed. `classify_file` gets none of these.

**`read-file` is scoped, not general.** It is for files the language graph doesn't already
claim and parse: configs, manifests, templates. Reading a source file the adapter itself
claims, to second-guess it — parsing a `.tsx` yourself instead of trusting `symbols-in` — is
out of contract even though nothing stops you mechanically. kndo's own `kndo:express` (reads
`package.json`'s `main`/`scripts`) and `kndo:nextjs` (reads `next.config.*` for a literal
`pageExtensions` array, with no JS evaluation) are the reference examples, and both fall back
to their pre-channel behavior on anything they can't read or parse — never a guess. Going over
the read budget cuts your plugin off from further reads for the rest of that run, not the run
itself.

**That is the entire visible universe.** If your convention needs something the view doesn't
carry, that is a `GraphView` extension to propose upstream — not something to work around. The
mechanism grows by adding an import, and it has: `call-sites-in` exists because route
conventions needed string-literal call arguments, and it arrived as its own import rather than
a field on an existing record, because growing a record is a breaking change in the component
model.

### What a plugin can never do

By construction: touch the filesystem or network *directly* (no WASI is linked — a component
importing WASI **fails to instantiate**; every byte you see through `read-file` was matched
against your own declared globs and fetched host-side, not a live syscall you make), see
another plugin's contributions, create new finding categories, or crash the run. Each hook
call has a fuel budget (50M units); a trap or exhaustion degrades to "this plugin contributed
nothing this round", never a failed `kndo check`.

### Cost model, and lifecycle

Your plugin costs its hooks' own runtime, never a cache penalty. Both of kndo's fast paths
work with plugins registered: the snapshot cache folds your identity — id, version, and your
component's own content hash — into its key, and the incremental patch strips your previous
contributions and re-runs your hooks against the patched graph (RFC 0017 §3). An unchanged
re-run costs a project the same whether your plugin is installed or not.

Your component is instantiated **once per graph-mutation round** — `contribute_roots` first,
then `contribute_edges` and `annotate_symbols` against the same instance, which is dropped
when `annotate_symbols` returns. You may cache work in statics across those three hooks;
state **never** survives into the next round or run, and the drop is unconditional — don't
try. `classify_file` runs on separate, view-less instances at an earlier pipeline phase; share
nothing with it. Adapter hooks (`claim`/`extract`) must be **pure functions of their
arguments** — they are called in parallel across a pool of instances, and results are
content-cached across runs.

## Emitting findings

```rust
fn rules() -> Vec<RuleDescriptor> {
    vec![RuleDescriptor {
        name: "deprecated-v1-api".to_string(),
        description: "calls to the v1 API are deprecated".to_string(),
        severity: FindingSeverity::Warning,
    }]
}

fn contribute_findings() -> Vec<ContributedFinding> {
    // walk call-sites-in / references-to, return findings for your declared rules
}
```

- **Your category is `plugin:<your-coordinate>/<rule-name>`**, assembled by the host from your
  registered id. You cannot emit a bare category, another plugin's, or an undeclared rule —
  undeclared emissions are dropped with a diagnostic, and rule names are lower-kebab.
- **Advisory by default.** A finding's severity is its rule's declared severity, but without
  the user's explicit [`[plugins.gate]`](../configuration.md#pluginsgate) opt-in it is shown,
  attributed, baselineable and suppressible (`kndo:allow plugin:<coordinate>/<rule>`) — and
  never breaks a build:

  ```toml
  [plugins.gate]
  "github.com/you/my-framework-plugin" = "warning"          # gate, capped at warning
  "github.com/you/my-framework-plugin/noisy-rule" = "off"   # per-rule override
  ```

  Config can lower your declared severity, never raise it. This is what makes installing your
  plugin safe by default — earn the opt-in with precision.
- **A noise ceiling**: 500 findings per rule per run; past it findings are dropped and the
  truncation is reported loudly. kndo's zero-false-positive statement covers only its own bare
  categories (RFC 0005 §9) — but its *spirit* is your best distribution strategy: silence over
  a guess.

## Writing an adapter

`kndo plugin new my-lang --adapter` scaffolds the adapter world instead: `descriptor()`
(id, file globs, grammar version, activation, adapter dependencies), `claim(path)` (is this
file mine, and what role/origin does it have?), and `extract(path, content)` (declarations,
references, roots, diagnostics for one file — called once per changed file, content-cached
by the host).

Scope, honestly: an external adapter's files get real `unused`/`test-only`/`untested`
analysis (declarations + references + roots is exactly what reachability consumes). It does
not yet extract manifests or resolve cross-file imports through the host, so `cyclic`,
`deep-import`, and dependency findings see nothing for it; it declares no visibility ladder,
so `internal-only`/`private-type-leak` skip its files (skipped, never guessed); and content
arrives as UTF-8 text. A wrapper adapter (a superset language like a single-file-component
format) can declare its base language's adapter in `dependencies` to co-activate it.

## Writing a coverage ingester

Coverage-report formats kndo doesn't ship built-in load through a third world,
`coverage-ingester` (print it with `kndo plugin wit`): `descriptor()` plus one export,
`ingest-coverage(path, content) -> ingested-coverage`. The world is deliberately
unidirectional — no imports at all: the **host** locates the report (the descriptor's
`requested-file-access` globs, or the user's `[plugins.<id>] report`), freshness-checks it,
and pushes the raw bytes in; the guest parses and returns line facts (`path`, `line`,
`hits`), and the host alone writes its sink, records provenance, and rebases paths (project
root and package-table rebasing apply to every ingester uniformly). Record paths as the
report states them — normalization beyond separators and `./` is the host's job. A coverage
component carries no graph hooks (`mutates_graph` is structurally `false`, so the graph
cache and incremental patch stay fully live) and cannot also be a graph-hooks plugin.
There's no scaffold variant yet — start from `examples/kndo-coverage-demo`, the reference
ingester the compliance suite builds and runs.

## Verify

```console
$ kndo plugin verify my-conventions.wasm
my-conventions.wasm: loads as a plugin component

descriptor:
  id github.com/you/my-conventions  version 0.1.0
  activation: manifest-dependency(my-framework)
  rule: deprecated-v1-api (warning): calls to the v1 API are deprecated

warnings:
  activation is empty — a globally installed copy would never run

fixture drive:
  contribute-roots: 2 roots (2 resolved)
  contribute-edges: 0 edges
  annotate-symbols: 1 annotation
```

`verify` loads the component through the exact loaders `kndo::open` discovery uses, reports
which world accepted it, lints the legal-but-probably-wrong descriptor shapes this guide calls
out (plain-name ids, empty `activation`), and drives every hook for real — your component
dropped project-local into a synthesized fixture project, a genuine full check, and the run's
audit record read back. Zero contributions on the generic fixture is a note, not a failure.
It is the fast answer to "why does my plugin do nothing": an activation rule that doesn't
match, a target that doesn't resolve, a rule you forgot to declare.

## Testing your plugin

- **First**: `kndo plugin verify your-component.wasm` — load, descriptor lint, and a generic
  fixture drive in one command, before you build any fixture of your own.
- **Against your own fixture**: `kndo plugin verify your-component.wasm --project
  path/to/fixture` runs the same drive over a copy of a project you provide (your directory is
  never touched). Build a fixture exhibiting your conventions and this becomes your test
  command.
- **When a contribution doesn't land**: a root, edge or annotation whose target doesn't
  resolve is silently dropped in production — but `verify` lists every dropped item with the
  exact target that failed ("root target `src/app.ts#foo` did not resolve"), and `kndo doctor`
  shows the per-plugin dropped count from the last run. "Contributed 0 roots" is a debuggable
  fact, not a dead end.
- **Locally, end to end**: `kndo plugin build`, drop the `.wasm` into a test project's
  `.kndo/plugins/`, run `kndo check` and `kndo doctor` there. Doctor shows whether you loaded,
  activated, and why — plus, after a run, what every plugin actually contributed. For the
  global tier, point `KNDO_PLUGIN_DIR` at a scratch directory.

### The baseline-then-plugin standard

Make a fixture project exhibiting your conventions, run kndo *without* your plugin, then
*with* it, and assert the delta. Four assertions, because "fewer findings" is not one of them:

1. **The baseline fires.** The findings your plugin exists to remove are present without it.
   Skipping this is how a proof passes while proving nothing.
2. **Exactly those disappear** — the named ones, not merely fewer.
3. **Unrelated dead code in the same fixture stays reported.** Without this, a plugin that
   keeps *everything* alive passes (1) and (2) and is worthless.
4. **The contribution record matches**: roots, edges, annotations, and `dropped` spelled out
   rather than counted. "Contributed two edges" looks identical whether the right two landed
   or the walk lost one and gained another.

**Build the fixture from a real project, reduced** — Kingfisher's storyboard,
spring-petclinic's pom and template — never from an invented example. A convention plugin
exists because a real tool wires things a real way; a fixture you designed to pass proves your
fixture. If your plugin has no field case that reduces, that is itself a finding worth writing
down rather than a reason to fabricate one.

In the kndo repository this is `crates/kndo/tests/builtin_plugin_proofs.rs`, which proves
every built-in this way and is the shape to copy;
`crates/kndo-plugin-api/tests/plugin_compliance.rs` and
`crates/kndo/tests/external_plugin.rs` are the same shape driven through the WASM ABI. For a
built-in it is not optional: `every_built_in_plugin_is_proven_here` closes that file against
`default_plugins()`, and it is one of the named CI gates.

**What "correct" means**: the zero-false-positive discipline applies to you too. A root you
contribute keeps code alive forever; if your convention has exceptions, use
`Confidence::Probable`/`Possible` instead of `Certain`, or don't contribute the fact at all —
silence over a guess.

## Versioning & compatibility

Three versions matter, and they are independent:

1. **Your `descriptor().version`** — yours entirely. Semver recommended; it is shown in
   `kndo doctor` and selected by `@vX.Y.Z` git tags on your repo. Tag releases; the tag is
   what users pin. An adapter additionally bumps its facts schema version when `extract`'s
   output changes meaning. These key kndo's caches, so a stale version ships stale analysis.
2. **The WIT package version** (`kndo:plugin@0.1.0`, the first line of the WIT file) — the
   ABI. kndo's promise: **at kndo 1.0 this freezes**, and a component built against a frozen
   package version keeps working against every compatible host indefinitely; a breaking change
   means a new package version, never a silent reinterpretation. **Before kndo 1.0, honesty
   over comfort: `0.1.0` may still evolve in place** (it has — `activation` and `dependencies`
   were added to the descriptor record after the first cut), and a record gaining a field is a
   break for already-built components. Pre-1.0 authors should expect to re-vendor the WIT and
   rebuild against new kndo releases. The compatibility promise is CI-enforced, not
   aspirational: pre-built, committed v1 components run against the HEAD host on every push
   (`crates/kndo-plugin-api/tests/compat_matrix.rs`), so a host change that would break your
   already-shipped binary breaks kndo's own build first.
3. **kndo's own binary version** — irrelevant to you beyond which WIT version it hosts.

Maintenance checklist per kndo release, until the freeze: diff your vendored `wit/plugin.wit`
against the release's; if changed, re-vendor, rebuild, re-tag. After the freeze: nothing,
until a `kndo:plugin@0.2.0` ever exists — and `0.1.0` components keep working even then.

## Distributing

Publish a GitHub release tagged `vX.Y.Z` on the repository your descriptor `id` names,
carrying exactly:

- `<crate-name>.wasm` — the component `kndo plugin build` produced;
- `checksums.txt` — `sha256sum` format.

Users install it with `kndo plugin install github.com/you/my-conventions[@vX.Y.Z]`. The
installer verifies the checksum, then identity binding: your descriptor's `id` must equal the
coordinate the user typed, or the install is refused. Private repos need only a
`GITHUB_TOKEN`/`GH_TOKEN` in the user's environment — their existing GitHub credential,
nothing plugin-specific. Until then, hand your users the `.wasm` and let them drop it in
`.kndo/plugins/` or the global directory.

## Worked references in the kndo repository

- `examples/kndo-plugin-hooks-demo` — a complete plugin: all four graph hooks, host-import
  queries, an activation rule, empty dependencies. ~110 lines.
- `examples/kndo-plugin-demo` — a complete adapter for an invented language.
- `examples/kndo-adapter-wrapper-demo` — a wrapper adapter whose whole point is
  `dependencies: ["kdemo"]`: activating it co-activates the adapter it wraps.
- `examples/kndo-coverage-demo` — the reference coverage ingester.
- `crates/kndo-plugin-api/tests/plugin_compliance.rs` — the baseline-then-plugin shape through
  the ABI, plus proof that the two worlds reject each other's components.
- `crates/kndo/tests/global_plugin_activation.rs` — the global-tier activation test shape,
  including `KNDO_PLUGIN_DIR`.
- `crates/kndo/tests/builtin_plugin_proofs.rs` — every built-in plugin's proof, the four
  assertions above in their reference form.
- The built-in plugin crates themselves (`crates/kndo-plugin-*`) — the `Plugin` trait a
  built-in implements is identical to what a WASM guest implements, and each ships a spec
  beside it in this directory.

## A note on helpers

Before writing a path or text helper in a plugin crate, check whether one already exists.
The promotion criterion, deliberately conservative: a helper moves to shared code when a
**third** plugin needs it *and* the three versions are the same function — not at the second.
Two plugins each having an `is_under` is cheaper than a shared abstraction over two cases that
turn out to differ; three is when the shape is actually known.
