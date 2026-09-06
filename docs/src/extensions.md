# Extensions

Everything beyond the engine is an extension, built-in or external, on one
trait: a language adapter, a coverage ingester, and a framework plugin
implement the same `Extension` and declare what they do in one spec. The
engine invokes only what a spec declares.

## Built in

| Extension | Kind | What it does |
|---|---|---|
| `kndo:js-ts`, `kndo:rust`, `kndo:go`, `kndo:java`, `kndo:kotlin`, `kndo:python`, `kndo:swift`, `kndo:html`, `kndo:css` | adapters | claim files, read manifests and launchers, extract evidence, resolve imports |
| `kndo:coverage-lcov`, `kndo:coverage-cobertura`, `kndo:coverage-jacoco`, `kndo:coverage-go` | ingesters | turn a report at a conventional path into coverage records |
| `kndo:interface-builder` | conduct | roots the classes storyboards and xibs instantiate; active when such files exist |
| `kndo:info-plist` | conduct | roots the principal class and app delegate an `Info.plist` names |

`kndo doctor` lists them with their versions. The `kndo:` coordinate namespace
is reserved for built-ins; an external component claiming it is rejected at
load, with a diagnostic.

## External components

kndo loads WASM components from `.kndo/plugins/*.wasm`, in name order, on
every run. A component is a WebAssembly component implementing the `extension`
world — one world for every kind of extension, so an adapter for a language
kndo does not ship, an ingester for another report format, or a framework
plugin are the same artifact shape. What a component may touch is decided by
its spec and enforced by the host per phase: extraction sees one file's bytes
and nothing else (evidence is cached by content, so the file set may not
influence it); resolution sees the project's file and package enumerations;
conduct sees the assembled graph and the files its `requested-file-access`
globs admit, under a content budget; ingestion sees the report's bytes. A
component that traps, overruns its fuel or its memory contributes nothing for
that call and the run continues — never a crashed run — and the report's
`plugins` block says what each contributed and what was dropped.

## Writing one

The SDK crate, `kndo-sdk`, makes the guest half the same code a built-in is:
implement `kndo_contract::extension::Extension`, export it, build for
`wasm32-unknown-unknown`.

```toml
[package]
name = "acme-framework"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
kndo-contract = { git = "https://github.com/eulke/kondo" }
kndo-sdk = { git = "https://github.com/eulke/kondo" }
```

```rust
use kndo_contract::extension::{
    Activation, ActivationRule, ConductSink, ConductTarget, Extension, ExtensionSpec,
    GraphAccess, MutatesGraph,
};
use kndo_contract::evidence::RootKind;
use kndo_contract::vocab::Confidence;
use smol_str::SmolStr;

pub struct AcmeFramework {
    spec: ExtensionSpec,
}

impl Default for AcmeFramework {
    fn default() -> Self {
        AcmeFramework {
            spec: ExtensionSpec::builder("acme:framework", 1)
                // Both gates are arguments, never defaults: when the plugin
                // runs, and whether its roots change the graph.
                .conduct(
                    Activation::AnyRule(vec![ActivationRule::ManifestDependency(
                        SmolStr::new_static("acme-framework"),
                    )]),
                    MutatesGraph::Yes,
                )
                .rule("routes", "route modules the framework mounts by directory")
                .requested_file_access(&["routes/**"])
                .build(),
        }
    }
}

impl Extension for AcmeFramework {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn contribute(&self, graph: &dyn GraphAccess, sink: &mut ConductSink) {
        for path in graph.paths() {
            if path.as_str().starts_with("routes/") {
                sink.root(
                    ConductTarget::File(path.clone()),
                    RootKind::Production,
                    Confidence::Certain,
                );
            }
        }
    }
}

kndo_sdk::export_extension!(AcmeFramework);
```

```sh
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/acme_framework.wasm /path/to/project/.kndo/plugins/
```

The spec is the whole declaration. What a builder chain can state:

| Declaration | Meaning |
|---|---|
| `builder(coordinate, version)` | the extension's identity and the version of the evidence it emits — bump it when the same source yields different evidence, and every cache keyed on it invalidates |
| `suffixes`, `claims` | which files it claims (a suffix list, and extra globs) |
| `emits` | the optional evidence streams it produces (`Comments`, `Metrics`, `Markers`, `Relations`, `Qualifiers`); an undeclared stream is typed absence, and analyses that need it abstain on its files or keep the answer they gave before it existed |
| `manifests` | globs of the manifests it reads for roots, packages and dependency declarations |
| `launchers` | globs of files it reads for roots alone — a CI workflow, a task runner's file — which declare no package and own no files |
| `ignores` | globs of the paths the language's own tool never compiles — Go's `_`-prefixed files and `vendor` copies, npm's `node_modules`, the interpreter's `site-packages`; a file under one is discovered, so an import into it is not broken, and never claimed, and a manifest under one declares nothing — the rule is the tool's, never a guess about an output directory a package might be named after |
| `ladder`, `published_surface`, `import_cycles`, `dependency_scoping`, `dependency_identity`, `dependency_builtins`, `dependency_importers` | the language facts the engine's judgments consume (see [Languages](languages.md)); `ladder` pairs each reach the language can spell with the word it spells it with, narrowest first, and says which declarations can take it; `published_surface` says whether a unit publishes every export or only what its entries export |
| `dispatch` | what the language's markers mean: rules pairing a trigger (a marker path pattern, optionally with an argument pattern) with an effect — a root of a color, or an exemption from `unused` — and a confidence; the engine derives roots and exemptions from every file's markers, so an attribute is a line of data, never a branch in an adapter |
| `conduct(activation, mutates_graph)` | the two gates of a plugin: when it runs (`Always`, or any of a set of rules — a manifest dependency by name, a file glob existing), and whether its contributions change reachability (`Yes` turns the persisted graph cache off for projects it activates on; `No` is a promise the engine holds you to) |
| `rule(name, description)` | a finding category this extension may report, published as `ext:<coordinate>/<name>` |
| `dependencies` | other extensions whose activation implies this one — the path to a plugin whose framework is an indirect dependency |
| `requested_file_access` | globs of files the conduct hooks may read, name-scoped and budgeted |
| `reads_reports` | report paths an ingester turns into coverage records |

A plugin's findings are advisory: `info` at most, never counted in health. Its
roots are not — they change what is reachable, which is why `mutates_graph`
has no default.

## Proving one

A plugin that removes findings must be shown to remove exactly those. The
discipline every built-in follows, and the one to keep for your own: a fixture
project whose findings fire **without** the plugin; run it **with** the plugin;
exactly the intended findings disappear, unrelated dead code stays reported,
and the report's `plugins` block for the coordinate matches — roots
contributed, findings reported, nothing `dropped`. The reference components
under `abi/guests/` in the repository are complete worked examples: a toy
language adapter, a framework plugin, an ingester, and a deliberately
misbehaving guest the host must survive.
