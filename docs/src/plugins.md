# Plugins

Language adapters own what a **language specification** defines; plugins own everything a
language is merely *adjacent to* — framework conventions, coverage-report formats,
organization rules. A Next.js page is "unused" to pure import analysis; the plugin knows the
router calls it. Plugins close exactly that gap: they contribute **roots** (entry points),
**edges** (references the language can't see), **file classifications**,
**externally-consumed annotations** (FFI, serialization), **coverage ingestion**, and their
own namespaced **findings**.

Third-party plugins are WebAssembly components: sandboxed (no filesystem, no network, no
clocks — every byte arrives host-mediated and budgeted), portable across platforms, and
installable per-project or per-machine. Built-in plugins ship inside the kndo binary under
the reserved `kndo:` namespace and are gated by the same activation rules.

## Built-in plugins

| Plugin | Activates when | What it does |
|---|---|---|
| `kndo:coverage-lcov` | always on (reads `coverage/lcov.info` / `lcov.info`, or `[plugins.<id>] report`) | ingests lcov line coverage for [`crap`](rules.md#crap) and [health](health.md#coverage-ingestion) |
| `kndo:coverage-cobertura` | always on (reads `coverage.xml` / `cobertura.xml` / `coverage/cobertura-coverage.xml`) | ingests Cobertura XML line coverage |
| `kndo:coverage-jacoco` | always on (reads the Gradle/Maven JaCoCo XML report paths) | ingests JaCoCo XML line coverage |
| `kndo:coverage-go` | always on (reads `coverage.out` / `cover.out`) | ingests Go coverprofile line coverage |
| `kndo:nextjs` | any manifest in the project declares `next` | roots `pages/**`/`app/**` convention files and their framework-consumed exports; reads `next.config.*` for custom page extensions; marks framework-visible exports externally consumed |
| `kndo:express` | any manifest declares `express` | roots the conventional server entry files (from `main`/`scripts` and entry-name conventions) that are *launched*, never imported |
| `kndo:serde` | any `Cargo.toml` declares `serde` | marks hand-written `Serialize`/`Deserialize` impls as implicitly invoked, so a serialized type's impl doesn't read as a test blind spot |
| `kndo:rkyv` | any `Cargo.toml` declares `rkyv` | marks hand-written `Archive`/`Serialize`/`Deserialize` impls — and the `*With` adapters `#[rkyv(with = …)]` reaches through generated code — as implicitly invoked |
| `kndo:wasmtime` | any `Cargo.toml` declares `wasmtime` | marks the members of the host traits `wasmtime::component::bindgen!` generates, whose only caller is the guest |
| `kndo:info-plist` | an `Info.plist` exists anywhere under the project root | roots the classes an Apple bundle names by string and the system instantiates — `NSPrincipalClass`, `WKExtensionDelegateClassName`, a scene manifest's delegate |

`kndo doctor` shows each with its activation state and reason.

**A plugin's rules are not the only way it turns on.** A plugin that names another in its
`dependencies` activates it by implication — transitively, and regardless of whether the named
plugin's own rules match. This is the only path to a plugin whose framework is an *indirect*
dependency: a company framework that uses Express internally is never `express` in its users'
manifests, so `kndo:express` can never self-activate there; the framework's own plugin names
`kndo:express` and that is what reaches it. `kndo doctor` renders the result as
`active (dependency of <id>)`, and a named coordinate no installed plugin carries is reported
as a missing dependency rather than silently ignored.

## Installing plugins

```console
$ kndo plugin install github.com/acme/kndo-conventions@v1.2.0
installed github.com/acme/kndo-conventions v1.2.0
$ kndo plugin list
global plugin directory: /home/you/.local/share/kndo/plugins
github.com/acme/kndo-conventions v1.2.0 (kndo-conventions.wasm)
$ kndo plugin remove github.com/acme/kndo-conventions
```

A plugin's **coordinate is its identity**: `github.com/<owner>/<repo>`, installed from that
repository's GitHub releases (`@tag` optional; latest release otherwise). The installer:

- **verifies the checksum** against the release's `checksums.txt`;
- **binds identity** — the fetched component's own declared id must equal the coordinate it
  was installed from; a component claiming to be something else is refused, as is any
  external component claiming the reserved `kndo:` namespace;
- **closes over dependencies** — a company wrapper plugin can declare the framework plugins
  it builds on, and they install (or resolve to built-ins) with it. A dependency that can't
  be satisfied is a visible gap in `kndo doctor`, never an error;
- **records everything in `plugins.lock`** beside the `.wasm` files.

Hand-copying a `.wasm` file works too; `kndo plugin list` shows such files as hand-installed
rather than hiding them.

## Where plugins live

| Tier | Location | Activation |
|---|---|---|
| project-local | `.kndo/plugins/*.wasm` | unconditional — presence is the opt-in |
| global (per-machine) | `~/.local/share/kndo/plugins` (Linux), `~/Library/Application Support/kndo/plugins` (macOS), `%APPDATA%\kndo\plugins` (Windows); override with `KNDO_PLUGIN_DIR` | gated by each plugin's own activation rules |
| built-in | inside the kndo binary | gated by activation rules (always-on only if a plugin declares none) |

The same directories and rules serve **adapter** components (new languages) — one directory,
and each `.wasm` is recognized as an adapter or a plugin by its ABI, never by its filename.
When several adapters could claim one file extension, project-local beats global beats
built-in.

## Activation: when does a plugin run?

A globally installed or built-in plugin runs only when its declared rules match your project:

- `file-exists(<glob>)` — some file under the project root matches;
- `manifest-dependency(<name>)` — any `package.json`/`Cargo.toml` **anywhere in the project**
  (gitignore-aware, so `node_modules` never counts; hyphen/underscore-insensitive for Cargo)
  declares the dependency. Monorepos activate on any member's manifest, not just the root's.

Any single matching rule activates. An empty rule list **never** self-activates from the
global tier — silence over a guess. Additionally, an active plugin activates every plugin it
declares as a dependency, transitively — a company framework plugin brings the plugins for
the frameworks it wraps. `kndo doctor` shows every candidate, active or not, with the exact
rule that fired (or didn't): `active (rule matched)`, `active (dependency of X)`, or
`inactive`.

## Plugin findings

Plugins can emit findings under declared rules. They are namespaced and fenced:

- category `plugin:<coordinate>/<rule>` (e.g. `plugin:github.com/acme/conventions/deprecated-v1-api`),
  always in group `convention` — the namespace and group are reserved; no core finding uses
  them, and no plugin can emit a bare core category.
- **Advisory by default**: rendered, baselineable, suppressible
  (`kndo:allow plugin:<coordinate>/<rule>`), but *never* moving the exit code — installing a
  plugin is safe by default, whatever severities it declares.
- **Opt into the gate** per plugin or per rule with
  [`[plugins.gate]`](configuration.md#pluginsgate); the configured level caps
  severity (lower than declared, never higher).
- **Capped**: at most 500 findings per rule per run, with loud truncation — a noisy plugin is
  a bounded annoyance.
- **Invisible to health**: the [score](health.md) structurally cannot be moved by plugin
  findings.

## What a plugin can never do

The sandbox is structural, not a policy promise:

- **No ambient capabilities.** The WebAssembly world has no filesystem, network, clock, or
  environment imports at all — a component that declared one would fail to load. File content
  arrives only through the host, only for paths matching the plugin's own declared access
  globs, only from the already-discovered (gitignore-filtered) tree, and under a per-run byte
  budget whose cutoff is a visible diagnostic.
- **No writes.** Hook outputs are claims about the graph, applied by the host under a fixed
  vocabulary — no new node kinds, no file mutation.
- **No hangs.** Every hook call runs under a fuel budget; an exhausted or trapping call means
  "this plugin contributed nothing this round", never a crashed `kndo check`.
- **No impersonation.** Reserved ids fail to load; the installer's identity binding and
  checksum pinning refuse a swapped component.

What a malicious or buggy plugin *can* do, stated honestly: lie about graph facts (a false
root suppresses findings that should have fired — it cannot create false core findings), and
emit noisy advisory findings. The mitigations are visibility: contributions are
provenance-tagged, and `kndo doctor` reports each plugin's last-run contribution record —
roots, edges, annotations, dropped targets — so "this plugin exempted 400 symbols" is a line
in a report, not an invisible bias.

Want to build one? See [Writing a plugin](plugin-authoring.md).
