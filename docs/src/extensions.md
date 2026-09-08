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
| `manifests` | globs of the manifests it reads. ONE door reads them, native and WASM alike — `extract_manifest` writes a `ManifestSink`: the units the build compiles and what enters them, the packages a bare specifier reaches, the dependencies declared, the names spelled elsewhere, the manifests aggregated, and the files the manifest says are RUN without entering any unit |
| `launchers` | globs of files it reads for roots alone — a CI workflow, a task runner's file — which declare no package and own no files. Same door, and the engine keeps only the roots |
| `ignores` | globs of the paths the language's own tool never compiles — Go's `_`-prefixed files and `vendor` copies, npm's `node_modules`, the interpreter's `site-packages`; a file under one is discovered, so an import into it is not broken, and never claimed, and a manifest under one declares nothing — the rule is the tool's, never a guess about an output directory a package might be named after |
| `ladder`, `published_surface`, `import_cycles`, `dependency_scoping`, `dependency_identity`, `dependency_builtins`, `dependency_importers` | the language facts the engine's judgments consume (see [Languages](languages.md)); `ladder` pairs each reach the language can spell with the word it spells it with, narrowest first, and says which declarations can take it; `published_surface` says whether a unit publishes every export or only what its entries export |
| `dispatch` | what the language's own statements mean: rules pairing a TRIGGER with an EFFECT and a confidence, so an attribute or a convention is a line of data and never a branch in an adapter. A trigger is a marker (path pattern, optionally an argument pattern, optionally the symbol kind it may sit on), a declaration NAME pattern, optionally narrowed to the kind of compilation the file lands in, a RELATION of a given kind to a base, a MEMBER of an owner another trigger matches, or the members an EXTERNAL base requires of whatever reaches it. An effect is a root of a color, a witness (kept while its owner is, of no color), an exemption from `unused`, or a generator's ownership of the file. Every pattern is compared against the name as the file writes it AND against the name the file's own binding imports qualify, so a rule written `org.junit.jupiter.api.Test` reaches an `@Test` imported from JUnit and not a same-named one from another package |
| `conduct(activation, mutates_graph)` | the two gates of a plugin: when it runs (`Always`, or any of a set of rules — a manifest dependency by name, a file glob existing), and whether its contributions change reachability (`Yes` turns the persisted graph cache off for projects it activates on; `No` is a promise the engine holds you to) |
| `rule(name, description)` | a finding category this extension may report, published as `ext:<coordinate>/<name>` |
| `dependencies` | other extensions whose activation implies this one — the path to a plugin whose framework is an indirect dependency |
| `requested_file_access` | globs of files the conduct hooks may read, name-scoped and budgeted |
| `reads_reports` | report paths an ingester turns into coverage records |

A plugin's findings are advisory: `info` at most, never counted in health. Its
roots are not — they change what is reachable, which is why `mutates_graph`
has no default.

What a declaration reaches is the adapter's statement in the contract's
vocabulary: its owner, its file, its namespace (or an ancestor, by `up`),
its unit (or its unit's group, `up: 1`), a directory (`up` levels above the
file), a namespace by name, its owner and the owner's subtypes (`Heirs`,
with or without the namespace — `protected`), its owner's exactly
(`Inherited`), or exported.
The engine resolves each to a pool of files from the scope forest and caps a
member's reach by its owner's; an adapter states the declared reach and
never computes the effective one.

The same rule holds for what a file can see without importing it: an adapter
DECLARES its namespace — `sink.namespace(["com", "foo"])` for a package clause,
an `ImportShape::Mount` for a language whose namespaces nest by declaration —
and the engine reads the co-visible set off that node. There is no hook for
handing the engine a list of files you walked, and that is deliberate: the
vocabulary is finite so that the tenth adapter's author has one way to say
each thing, not a choice between two. A language whose namespace is its unit
says so through its manifest instead (`extract_manifest`), and one whose file
IS its scope says nothing at all.

A file is not always one language. An adapter that finds a span of another
language in its file — a page's inline `<script>`, its `<style>` — reports it
as an embedded region (`sink.region(span, "js", RegionMode::Module)`): the
language as the file suffix its extension claims, and how the span runs (a
module, or a classic script whose top-level declarations are the page's
globals). The engine hands each region to the extension claiming that
suffix, which reads it exactly as it reads a file — `SourceFile::region`
says it is one, and which — with spans relative to the region's bytes; the
sink puts them in the host file's coordinates, marks the region's imports so
that extension resolves them, and drops what the host never declared, since
the host's streams bound its file's evidence. What a region declares and
imports is then judged, resolved and addressed as the host file's own, and
the host's cache entry remembers which extensions read its regions, so a
change in one of them re-extracts the file.

## Rule packs: a framework as data

Most framework knowledge needs no code. A JUnit `@Test`, a Spring
`@RestController`, an `XCTestCase` subclass — the language adapter already
reported the marker or the relation, and all that is missing is what it MEANS.
A **rule pack** is a conduct extension that declares an activation and a list
of `DispatchRule`s and nothing else: no claims, no manifests, no content
access, no hooks. Its rules reach the engine as data, so an active pack does
not invalidate the graph cache — the rules and the active set are part of its
key.

```rust
ExtensionSpec::builder("kndo:spring", 1)
    .dispatch(vec![DispatchRule {
        // The FULL path. The engine qualifies the marker a file carries
        // through that file's own import bindings before comparing, so this
        // matches `@Controller` imported from Spring — and not the
        // `@Controller` of a web framework in another language.
        when: Trigger::marker_on(
            "org.springframework.stereotype.Controller",
            SymbolKind::Type,
        ),
        then: Effect::Root(RootKind::Production),
        confidence: Confidence::Probable,
    }])
    .conduct(
        Activation::AnyRule(vec![ActivationRule::ManifestDependency(
            "org.springframework*".into(),
        )]),
        MutatesGraph::No,
    )
    .build()
```

Both gates are load-bearing, and neither substitutes for the other.
**Activation** decides whether the project uses this framework at all —
`ManifestDependency` for a framework its manifests declare, `FileImports` for
one they never mention (Swift without a `Package.swift`, a JVM tree whose build
file the run cannot see). It is coarse on purpose: it runs before anything is
parsed. **Qualification** decides whether a particular marker is this
framework's, and only a rule that spells the whole path gets it — a bare name
is the same few letters in every ecosystem. A pack that skips either one
silences code it never meant to.

The `rule_packs_are_data` gate holds the shape, and `builtin_conduct_proofs`
demands the same baseline-then-pack proof every other conducting coordinate
carries.

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
