# Languages

Every language lands through the same adapter contract — the core never contains
`if language == X`. An adapter claims files, extracts declarations/references/imports/roots,
resolves imports, reads manifests, and declares its language's *facts*: the visibility ladder
(so `internal-only` speaks the language's own words), the cycle policy (so `cyclic` judges
cycles the way the ecosystem does), and whether dependency usage is statically resolvable.
Mixed repositories are the point: one graph, cross-language edges, one report.

| Language | Files | Manifests |
|---|---|---|
| JavaScript / TypeScript | `.ts .tsx .js .jsx .mjs .cjs .mts .cts` (incl. `.d.ts`) | `package.json` |
| Go | `.go` | `go.mod`, `go.work` |
| Rust | `.rs` | `Cargo.toml` |
| Java | `.java` | `pom.xml`, `build.gradle`, `build.gradle.kts` |
| Kotlin | `.kt` | shared Gradle/Maven manifests |
| Swift | `.swift` | `Package.swift` |
| JSON | `.json` | — |
| CSS / SCSS | `.css .scss` | — |

Adapters are held to a shared **conformance harness** — fixture projects with
expected-finding JSON every adapter must reproduce exactly — so "the same verdict means the
same thing" is tested, not aspirational. New languages can also arrive as
[WebAssembly adapter components](plugins/authoring.md#writing-an-adapter) without rebuilding
kndo.

## JavaScript / TypeScript

**Modeled:** ES modules and CommonJS; `package.json` `exports`/`main`/`module` entry-point
roots; workspaces and per-package ownership; barrel re-export chains (resolved to a fixpoint,
so a symbol reached only through three `index.ts` hops is still reached); test conventions
(`*.test.*`, `*.spec.*`, `test/`, `tests/`, `__tests__/`); script-invoked CLI dependencies
(`"test": "xo && ava"` keeps `xo` and `ava` out of `unused`); dependency scopes
(`dependencies`, `devDependencies`, `peerDependencies`, `optionalDependencies`) with the
[scope rules](rules.md#unused) applied literally; declared `exports` surfaces gating
[`deep-import`](rules.md#deep-import); a hazardous cycle policy (import cycles are
warning-level — initialization-order bugs are real).

**Limits:** dynamic `import(expr)` and `require(variable)` with computed specifiers can't be
resolved to a file — such references keep candidates *live-possible* (lowered confidence),
never dead. Code invoked only via string lookups in configuration kndo doesn't model needs a
[plugin](plugins/authoring.md) root (the built-in [Next.js and Express
plugins](plugins.md#built-in-plugins) cover those frameworks' conventions).

## Go

**Modeled:** package-per-directory units; capitalization-as-visibility (an exported
identifier is part of the package surface); `go.mod`/`go.work` for module identity and
declared dependencies (`// indirect` entries excluded); `_test.go` files (including the
separate `_test` package form) as test roles; `func main`/`init` and test functions as
roots; the compiler-enforced `internal/` boundary.

**Limits:** kndo intentionally *doesn't* re-police what the Go compiler already forbids —
import cycles are impossible in Go, so `cyclic` never accuses (a cycle in the graph could
only be a resolution artifact), and `internal/` needs no `deep-import` finding. Build tags
are not evaluated: files excluded by a tag combination are still parsed and analyzed, so a
symbol used only under another platform's tag counts as used (safe direction), and files
only *reachable* under exotic tag sets may be reported through the default lens. Reflection
(`reflect`) with computed names is invisible; well-known interface machinery (`String()`,
`Error()`, marshal hooks) is modeled as implicitly invoked.

## Rust

**Modeled:** the module tree *is* the file graph — `mod foo;` is a certain file edge, and a
file no `mod` chain reaches is dead to the compiler, a verdict reachability reproduces for
free; the full visibility ladder (`pub`, `pub(crate)`, `pub(super)`, private) for
[`internal-only`](rules.md#internal-only) remediation in Rust's own words; `#[cfg(test)]`
regions as in-file test scopes (a dependency imported only there is
[`test-only`](rules.md#test-only)); `Cargo.toml` workspace topology,
`dependencies`/`dev-dependencies`/`build-dependencies` scopes; macro token-tree scanning, so
a symbol named inside a macro invocation still counts as referenced; `main.rs`/`lib.rs`/
binary targets as roots; library crates treat their public surface as consumed by
definition.

**Limits:** macro-*generated* items (declarations that only exist after expansion) are not
materialized — kndo reads source, it doesn't expand macros; token-tree scanning keeps
macro-referenced symbols alive, but code generated wholesale by proc-macros is invisible.
Trait impls dispatched only through external machinery (serialization being the classic
case) are covered by the built-in [`kndo:serde`, `kndo:rkyv` and `kndo:wasmtime`
plugins](plugins.md#built-in-plugins) — kndo records which trait's `impl` declares each
member, and each plugin matches its own ecosystem's traits against that. Another such
framework needs its own plugin, which is that table and nothing more.

## Java

**Modeled:** declared package identity plus the compiler-checked directory convention;
`src/main/java` vs `src/test/java` role promotion; `pom.xml` and Gradle build files as
manifests; dispatch roots for the reflective contracts the platform guarantees
(`@Override` dispatch, `Serializable` hooks); implicitly-public interface members;
constructors as first-class members; the visibility ladder mapped conservatively —
`protected` and `public` share a scope no static evidence can distinguish, so they never
accuse each other.

**Limits:** **dependency usage is not resolvable** — an `import com.foo.bar.Baz` has no
reliable static mapping to a Maven/Gradle coordinate without resolving the classpath, which
kndo never does; dependency `unused`/`test-only` findings are skipped for Java (one
diagnostic, not a false-positive flood), while [`version-skew`](rules.md#version-skew) stays
fully precise (it's manifest-to-manifest). Reflection with computed class names, classpath
scanning, and annotation processors that generate code are invisible; DI frameworks that
instantiate beans reflectively need a plugin to contribute the roots.

## Kotlin

**Modeled:** the `internal` ladder rung (module-scoped visibility) alongside
`public`/`protected`/`private`; primary and secondary constructors; `.kt` files under
`src/main/java` trees too (mixed Java/Kotlin source sets are normal); shared Gradle/Maven
manifest handling with Java.

**Limits:** the same JVM limits as Java — dependency usage unresolvable (skipped, not
guessed), reflection and codegen invisible. Kotlin-specific compiler plugins (serialization
et al.) that synthesize members are not expanded.

## Swift

**Modeled:** `Package.swift` parsed as the manifest it is (Swift source), including targets
with custom `path:`; XCTest targets and files as test roles, with test methods as roots;
the `open`/`public`/`internal`/`fileprivate`/`private` ladder; protocol-witness rooting — a
method implementing a protocol requirement is invoked through the protocol, not by name;
`init`/`deinit` as members.

**Limits:** Objective-C bridging (`@objc` selectors invoked from the runtime) and
storyboard/Interface Builder references are not modeled — symbols reachable only that way
need an annotation or plugin. As with every language: dynamic dispatch kndo can see keeps
things alive; dynamism it can't see never makes things dead.

## JSON

A non-source language, deliberately narrow: claims `.json` files so they exist in the graph
at all — letting other languages' imports resolve *to* them, `unused` see orphaned config
files, and byte-identical [`duplicate`](rules.md#duplicate) detection cover them. Extracts
no symbols and declares no ladder, so symbol-level analyses skip it by construction.

## CSS / SCSS

Also deliberately narrow: the `@import`/`@use`/`@forward` file graph, plus custom
properties, SCSS variables, mixins, and functions as symbols — so an unused `$variable` or
`--custom-property` falls out of ordinary reachability. Selectors, classes and ids are *not*
extracted as symbols: nothing in a static graph can prove a selector unused (any HTML,
template, or runtime class name could match it), and kndo prefers silence to a wrong
"unused selector" claim.

## Files no adapter claims

Unknown extensions still participate where honesty allows: they are discovered, can be
referenced (kept-alive) by claimed files, and the byte-identical half of
[`duplicate`](rules.md#duplicate) covers them — but they are never *accused* of anything on
their own.
