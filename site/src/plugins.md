# Plugins

Adapters own what a **language specification** defines; plugins own everything a language is
merely *adjacent to* — framework conventions, coverage-report formats, organization rules.
Both are WebAssembly components: sandboxed (no filesystem, no network — content arrives
host-mediated and budgeted), portable, and installable per-project or globally.

## Using plugins

```console
$ kndo plugin install github.com/acme/kndo-conventions@v1.2.0
$ kndo plugin list
$ kndo plugin remove github.com/acme/kndo-conventions
```

Installs verify checksums and identity binding (the component must declare the coordinate it
was installed from), resolve dependency closures (a company wrapper plugin can co-install the
framework plugins it builds on), and record everything in `plugins.lock`. Project-local
components live in `.kndo/plugins/`; `kndo doctor` shows what is active and why.

Two ecosystem plugins ship as gated built-ins: `kndo:nextjs` (App/Pages router file
conventions become entry-point roots) and `kndo:express` (route and template registrations
become edges).

## What plugins can do

- **Shape the graph**: contribute roots ("files matching `pages/**` are entry points"), edges
  ("`app.get('/x', handler)` references `handler`"), classify files, mark symbols externally
  consumed (serialization, FFI, DI).
- **Contribute findings**: rules declared up front, one severity per rule, emitted under the
  namespaced category `plugin:<coordinate>/<rule>` in group `convention`. Advisory by default
  — rendered, baselineable, suppressible like any finding, but never moving the exit code
  until you opt in via `[plugins.gate]`. The health score structurally cannot see them, and a
  noisy plugin is capped (500 findings per rule, truncation reported loudly).

## Writing one

```console
$ kndo plugin new my-conventions          # scaffold (add --adapter for a language adapter)
$ kndo plugin build                       # cargo build + componentize
$ kndo plugin verify target/…/my_conventions.wasm --project ./fixture
```

The scaffold targets the current WIT world; `kndo plugin wit` prints the ABI. A CI
compatibility matrix in the kndo repo runs pinned v1 components against every new host build —
a component you ship keeps working.
