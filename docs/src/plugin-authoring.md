# Writing a plugin

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
componentization so you never touch wasm tooling directly.

## Plugin or adapter?

- Write a **plugin** to teach kndo about a *framework or convention* in languages it already
  parses: entry points invisible to imports (routes, DI registrations, scheduled jobs),
  edges the language can't see (route-string → handler), file classifications
  (`*.stories.tsx` is tooling), externally-consumed symbols (FFI, serialization), or your
  organization's own findings.
- Write an **adapter** (`kndo plugin new my-lang --adapter`) to teach kndo a *language*:
  claim files by extension and extract declarations, references, and roots from their
  content.

## The scaffold

`kndo plugin new <dir>` writes a compilable crate:

```text
my-conventions/
├─ Cargo.toml          # cdylib, wit-bindgen, size-optimized release profile
├─ wit/plugin.wit      # the ABI, vendored from the exact kndo you ran — do not edit
├─ src/lib.rs          # a documented, compiling skeleton of every hook
├─ README.md
└─ .gitignore
```

The WIT contract is vendored from your kndo binary, so guest and host always agree. To
retarget a crate at a newer kndo later: `kndo plugin wit plugin > wit/plugin.wit` and
rebuild. (`kndo plugin wit adapter` prints the adapter world.)

## The descriptor

Everything kndo decides about your plugin *before running it* comes from `descriptor()`:

```rust
PluginDescriptor {
    // Your id IS your coordinate: the GitHub repo the component is fetched from.
    // A plain name works for hand-dropped .kndo/plugins/ files but can never be
    // installed or depended on. The kndo: namespace is reserved and will not load.
    id: "github.com/you/my-conventions".to_string(),
    version: "0.1.0".to_string(),
    // One line of prose: when does this plugin apply? (Shown by kndo doctor.)
    detection: vec!["a package.json depends on my-framework".to_string()],
    // Files outside the language graph you need to read — globs served through the
    // host's content channel. Empty = no reads.
    requested_file_access: vec!["**/my-framework.config.*".to_string()],
    // When a globally installed copy turns on. An empty list NEVER self-activates
    // globally — declare a real rule.
    activation: vec![ActivationRule::ManifestDependency("my-framework".to_string())],
    // Coordinates of plugins whose conventions are part of yours: installing you
    // installs them; activating you activates them.
    dependencies: Vec::new(),
}
```

## The hooks

A plugin exports up to six hooks. Return empty collections for the ones you don't need.

| Hook | Contributes | Notes |
|---|---|---|
| `classify_file(path, current)` | a corrected role/origin for one file | `None` = no opinion (the common case) |
| `contribute_roots()` | entry points: `(target, root-kind, confidence)` | a root keeps code alive forever — use `certain` only when the framework *guarantees* invocation; kndo's zero-false-positive bar applies to you |
| `contribute_edges()` | references the language can't see | e.g. route-string → handler, template → class |
| `annotate_symbols()` | symbols consumed from outside the graph | exempts them from `internal-only`/`private-type-leak`, never from `unused` |
| `rules()` | the findings you may emit: name, description, one severity each | undeclared rules are dropped; declared rules show in `kndo doctor` before you ever run |
| `contribute_findings()` | findings under your declared rules | emitted as `plugin:<coordinate>/<rule>`, advisory unless the user gates them |

Targets are always **named** — `{ path, symbol: Option<String> }` — never internal ids; an
unresolvable target is dropped silently (and counted in `kndo doctor`'s dropped-contribution
record and in `kndo plugin verify`'s report).

### Reading the graph

Hooks read, they never own: the host serves a snapshot of adapter-derived facts through
query imports —

- `list-files` (path, role, origin) and `file-details`;
- `symbols-in(path)` (name, kind, exported, member-of) and `symbol-details`;
- `packages` / `package-of`;
- `imports-of` / `importers-of` / `references-to`;
- `call-sites-in(path)` — including calls with string literal arguments, the raw material for
  route conventions;
- `read-file(path)` — the **content channel**: raw bytes of files matching your declared
  `requested_file_access` globs, from the discovered (gitignore-filtered) tree only, under a
  per-run byte budget. Out-of-glob and over-budget reads answer `none`.

You only ever see *adapter-derived* data — never another plugin's contributions — so plugin
behavior is independent of what else is installed.

### Lifecycle and budgets

- One instance serves the three graph-mutation hooks of a run (`contribute_roots` →
  `contribute_edges` → `annotate_symbols`): you may cache work in statics across them. State
  **never** survives into the next run — don't try.
- Every hook call runs under a fuel budget (re-armed per hook). Exhaustion or a panic means
  your plugin contributes nothing this round; it never breaks the run.
- Adapter hooks (`claim`/`extract`) must be **pure functions of their arguments** — they are
  called in parallel across a pool of instances, and results are content-cached across runs.

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

A finding's severity is its rule's declared severity — but without the user's explicit
[`[plugins.gate]`](configuration.md#pluginsgate) opt-in it is **advisory**:
shown, never gating. Gated severities are capped at the configured level. Emissions are
capped at 500 findings per rule per run, truncated loudly. Users can suppress your findings
inline (`kndo:allow plugin:<coordinate>/<rule>`) and baseline them like any other.

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
report states them — normalization beyond separators/`./` is the host's job. A coverage
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

`verify` runs the exact loaders kndo itself uses, lints the common descriptor mistakes, and
drives every hook against a synthesized fixture project — or your own with
`--project <dir>`. It is the fast answer to "why does my plugin do nothing": an activation
rule that doesn't match, a target that doesn't resolve, a rule you forgot to declare.

## Distributing

Publish a GitHub release tagged `vX.Y.Z` on the repository your descriptor `id` names,
carrying:

- `<crate-name>.wasm` — the component `kndo plugin build` produced;
- `checksums.txt` — `sha256sum` format.

Users install it with `kndo plugin install github.com/you/my-conventions[@vX.Y.Z]`. The
installer's identity binding means the release must really be yours: the component's declared
id must match the coordinate it was fetched from.

Compatibility: the ABI is versioned, and a component built against a given ABI version keeps
working against compatible hosts — the ABI grows by adding imports and sibling worlds, not by
reshaping what shipped. When you change your plugin's behavior, bump its `version` (an
adapter bumps its facts schema version when `extract`'s output changes meaning) — these key
kndo's caches, so a stale version ships stale analysis.
