# Languages

Each adapter claims files by suffix, reads the ecosystem's manifests for roots
and dependencies, and extracts evidence — declarations, imports, references,
comments, per-function metrics — into one vocabulary the engine judges without
knowing which language it came from. The engine never names a language:
everything a language needs is declared on the adapter's spec, with a named
consumer in the engine.

| Adapter | Suffixes | Manifests and launchers | Roots |
|---|---|---|---|
| `kndo:js-ts` | `ts` `tsx` `js` `jsx` `mjs` `cjs` `mts` `cts` | `package.json`, `tsconfig.json` (and its `tsconfig.*.json` variants); `.github/workflows/*.yml`, `action.yml` | a `package.json` is one unit — entered through its entry fields (`main`, `module`, `browser`, `bin`, `exports`, `imports`), published unless it says `"private": true` — plus files handed to a runtime by npm scripts and by workflow or action steps (`node`, `tsx`, `ts-node`, `bun`, `deno`), test files, config files, shebangs. A `tsconfig.json` states no unit: its `compilerOptions.paths` aliases are names resolving to files, read as packages so `~utils` and `@/thing` link like any other name |
| `kndo:rust` | `rs` | `Cargo.toml` | every cargo target as a unit entered through its own file (lib, bins, tests, benches, examples, build script), with `publish = false` read as "no consumer outside"; by dispatch rule: `#[test]`, `#[bench]`, `#[cfg(test)]`, `#[no_mangle]` and the other linkage attributes, `#[tokio::main]`-style entries, a top-level `fn main` in the color of the cargo target holding it; `#[allow(dead_code)]` and kin exempt |
| `kndo:go` | `go` | `go.mod` | `package main` + `func main`; by dispatch rule: every `init` (in the color of the binary its file compiles into) and, in the test compilation, the runner's `TestXxx`/`BenchmarkXxx`/`ExampleXxx`/`FuzzXxx`; `_test.go` is a declared file role AND states its own attachment. The module `go.mod` names is one published library unit, and a package — the directory plus its package clause — is a namespace inside it, so nothing keeps a package's files alive but its exported surface and its importers |
| `kndo:java` | `java` | `pom.xml`, `build.gradle`, `build.gradle.kts`, `settings.gradle`, `settings.gradle.kts`, `gradle/libs.versions.toml` | `main` methods, test sources, framework annotations through extensions; by dispatch rule: `@Override` and the members the JDK's own bases require (`Comparable.compareTo`, `Iterable.iterator`, `AutoCloseable.close`, the four `Serializable` hooks) are WITNESSES — kept while their owner is, of no color; a `pom.xml` module is two units, its main set and its test set, the test set compiling against main and a friend of it; the test set is what the pom states (its own `<testSourceDirectory>`, else the nearest parent's along `<parent>`, plus the build helper's added test sources), and every file in it is a test root by the unit's kind. A Gradle build says the same in its own words: `settings.gradle(.kts)` names the modules it includes, each `build.gradle(.kts)` states its main and test source sets and what they compile against, and a dependency named through `gradle/libs.versions.toml` is read from the catalog beside the settings file |
| `kndo:kotlin` | `kt` | the same JVM manifests | `main` functions, and test sources by the unit's kind. No whole-library root: what a published module offers is its unit's published surface, read from the build files, so `internal` reaches that unit and its friends — a module's own test source set among them — and a module that publishes nothing roots nothing |
| `kndo:python` | `py` | `pyproject.toml`, `requirements.txt`, `requirements-*.txt` | scripts, `__main__`, test files, entry points |
| `kndo:swift` | `swift` | `Package.swift` | executable targets, `@main`, test targets |
| `kndo:html` | `html` `htm` | — | a document is its own root; `<script src>` and `<link href>` are its references; an inline `<script>` or `<style>` is a region of JavaScript or CSS, read by that adapter |
| `kndo:css` | `css` `scss` | — | `@import`, `@use`, `@forward`, resolved with Sass partial and index conventions |

Two more extensions ship for Apple projects: `kndo:interface-builder` roots
the classes storyboards and xibs instantiate at run time, and `kndo:info-plist`
roots the principal class and app delegate an `Info.plist` names. Four
[coverage ingesters](health.md) read lcov, Cobertura, JaCoCo and Go
coverprofile reports.

## What each language declares

Beyond suffixes and manifests, an adapter declares the facts the engine's
judgments depend on — with a default that keeps the judgment silent where the
adapter says nothing:

- **Reach**: how far each declaration's name legally reaches, as the
  language spells it — its owner (a `private` member), its file (a top-level
  `private`, an ES declaration without `export`), its namespace or an ancestor
  of it (Java's package-private, a Rust item without `pub`, `pub(super)`), its
  unit (`internal`, `pub(crate)`) or the group of units one manifest aggregates (Swift's
  `package`), a directory (an exported Go name under `internal/`, fenced at
  that directory's parent), a namespace by name (Rust's `pub(in crate::a)`),
  its owner and the owner's subtypes (`protected`; Java's adds the package),
  its owner's exactly (a Rust trait item), or exported. The engine pools by
  the *effective* reach — the declared one after every owner above caps it,
  so a public member of a file-private class reaches the file and is never
  handed out by a published surface — and `describe` shows both.
- **The visibility ladder** for `internal-only`: the reaches a language can
  spell, narrowest first, each under that language's own word for it and,
  where a keyword exists for members or for top-level declarations alone,
  saying so — Java declares `private` (members), `package-private`,
  `protected` (members), `public`; Kotlin `private` twice (the class on a
  member, the file on a top-level declaration), `protected` (members),
  `internal`, `public`; Rust `private`, `pub(crate)`, `pub`;
  TypeScript `unexported` (top level) and `export`. The engine judges by the
  rung and reports by the word: a package-private member used only inside its
  class reads "`private` would suffice", a `pub(crate)` function used only in
  its file "`private` would suffice", and a language that spells nothing
  between the declared rung and the one the uses need — Go below its package,
  Python anywhere — gets no advice at all.
- **The published surface** for that analysis's Exported rung: whether a unit
  publishes every export (a jar, a crate, a Go module, a Python distribution
  — the default, under which an exported declaration is never advised to
  narrow) or only what its entries export (npm, where `main`/`exports` decide,
  so an `export` in a file no entry reaches is advised `unexported` when
  nothing else in the tree names it). Whether a unit publishes at all is the
  manifest's: a library does unless it says otherwise, an executable or a test
  set never. `used-by` shows an export kept by its unit's publication as
  `published`.
- **Ignores**: the paths the language's own tool never compiles — a Go file
  whose name begins with `_` is in no package, Go's `vendor` holds copies of
  other modules, npm never builds `node_modules`, and the interpreter's
  `site-packages` holds installed packages, never the project's. A file
  under one is discovered, so an import pointing at it is not broken, and
  never claimed: no evidence, no unit, no verdict — and a manifest under one
  (a dependency's `package.json`, a vendored module's `go.mod`) declares
  nothing about the project. The rule is the tool's own, measured: a Go
  `testdata` or `_`-prefixed directory is not one, since `./...` skips it
  but an import compiles it (gin imports its `testdata/protoexample` from
  three test files); an output directory a JVM or Cargo build writes is not
  one, since a package may be named `target` or `build`; and what a manifest
  excludes is that unit's membership, not the tree's.
- **Includes**: where a language pastes one file's content into another
  (Rust's `include!`), the import says so and the engine reads it as sight: the
  including file sees every name the included one declares, whatever reach it
  carries, and nothing is handed out — what the includer never names is still
  dead.
- **Mounts**: where a language names its namespaces by ATTACHING files to
  each other rather than by a clause each file writes, the import that does it
  says so — Rust's `mod x;` makes `x.rs` the child namespace `x` of the
  mounting module's. The engine reads the tree that results: a name private to
  a module is readable in every file mounted under it, an address that climbs
  (`pub(super)`, `pub(in crate::a)`) names the node it climbs to, and the
  mount's own reach fences everything below it, so a `pub` item of a privately
  mounted module is nameable in the mounting namespace and nowhere else — off
  the unit's published surface, and judged like any other bounded name.
- **Embedded regions**: the spans of a file written in another language.
  HTML reports each inline `<script>` — a module, or a classic script — and
  each inline `<style>` as a region of JavaScript or CSS, and the engine hands
  it to that adapter, which reads it as it reads a file, in the page's
  coordinates: a function the inline module never calls is the page's
  `unused` finding, its imports resolve as JavaScript's (an extensionless
  `./src/util` finds `util.ts`, a bare name is a package), and an `@import`
  in the inline style is the stylesheet's edge. A classic script's top-level
  declarations are the page's globals — reachable from every other script and
  handler attribute on it — and stay alive; a script whose type is data (an
  import map, JSON, a template) is no region at all.
- **Cycle tolerance** for `cyclic`: a hazard in JavaScript, TypeScript and
  Python (initialization order bites at run time), tolerated in Rust, Go, Java,
  Kotlin and Swift (the compiler or the package model makes cycles benign).
- **Dependency identity**: how an import specifier names a declared dependency
  — the package name (`lodash/fp` is `lodash`), the crate root, the module
  path — and the platform's own modules that are never a dependency to
  declare (Node's built-ins; Go's standard library by the rule that its import
  paths have no dot in their first segment).
- **Dependency scoping**: whether the ecosystem's manifests separate
  production from development declarations (npm, Cargo) or not (Go).
- **Importers outside the claim**: file families that carry the ecosystem's
  imports without being its source — `.vue`, `.svelte`, `.astro`, `.mdx`,
  `.html`, `.css`, … for JavaScript — so a dependency judgment abstains when
  such files exist and no adapter reads them, rather than accuse.
- **Dispatch rules**: what the language's markers — attributes, annotations,
  decorators, pragmas — mean. An adapter reports every marker as evidence,
  path and arguments as written; its spec's rules say which ones root an entry
  of which color and which exempt a declaration from `unused`. Rust's rules
  make `#[test]`, `#[tokio::test]`, `#[bench]` and a `cfg(test)` gate Test
  roots, `#[no_mangle]`, `#[global_allocator]` and their kin Production roots,
  and honor `#[allow(dead_code)]`, `#[expect(dead_code)]` and the `unused` and
  `warnings` groups as exemptions — lexically scoped, and a file-level
  `#![allow(dead_code)]` is reported as a diagnostic so the silence is visible.
  `used-by` shows a dispatched root as `dispatch:<color>` and an exemption as
  `exempt`.
- **Attachment**: whether a file belongs to the namespace it declared in every
  build, or in test builds alone. A file states it where the language's own
  tooling compiles it into the test build and nothing else — go's `_test.go`,
  the JVM's `src/test/{java,kotlin}` source sets, SwiftPM's `Tests/`, what
  pytest collects, the web's `*.test.*`, `*.spec.*` and `__tests__/`. It is
  what stops production colour from flowing through a test file, what makes a
  dependency only tests import `test-only`, and what a dispatch rule's
  `in_unit` reads where no manifest declares a test unit. A test-shaped NAME
  outside those trees is not it: a `LoadTest.java` on the main source path is
  compiled into the library like anything beside it, and stays importable
  surface.
- **Relations**: the types a declaration promises to be (`extends`,
  `implements`, a protocol conformance). The engine reads one stream both ways:
  a member whose owner promised a type declaring the same name is a WITNESS and
  is kept — no call site can be required to exist, because every caller holds
  the supertype — and a member some subtype declares is OVERRIDDEN, so
  `internal-only` never advises narrowing it. `used-by` shows the first as
  `witness`.
- **Signatures**: what the language reads beyond the identifier to tell
  same-named declarations apart, as it spells it — Java's parameter types
  `(int, List<String>)`. It is part of a declaration's address, so an overload
  has its own finding identity and its own query address; a language that
  states none (Kotlin and Swift, until their adapters do) gets `#2` on the
  second of two the engine cannot otherwise split.
- **Evidence streams**: comments (for `kndo:allow`), per-function metrics
  (for `duplicate` and `crap`), markers (for dispatch), relations (for
  witnesses) and qualifiers — whether each reference was written ON something,
  which is how `internal-only` tells `queue.head` from somebody else's local
  named `head`. Each is declared, so its absence is typed and an analysis
  abstains or keeps its pre-stream answer instead of guessing.

## Generated files

Every language adapter recognises its ecosystem's generated banner
(`@generated`, `// Code generated … DO NOT EDIT.`) in a file's header and
REPORTS it; what it means is one rule for all of them. A generator's output is
not this project's to answer for, so nothing the file DECLARES is accused —
`unused`, `duplicate`, `crap`, `internal-only` and `private-type-leak` stand
down for its declarations, and the report says so in a line naming the banner
it matched. Coverage is not a judgment about a name, so `untested` still speaks.

Being generated is not a reason to keep the file: what it IMPORTS and NAMES is
evidence like any other file's, and a generated file nothing imports is
reported `unused` at file level, exactly like an orphan somebody typed. The fix
is to stop generating it.

## Cross-language reach

One graph: an HTML page reached from a manifest reaches the script it loads,
which reaches the stylesheet it imports; a Swift file reached through a
storyboard is production-reachable; a Go package's files see each other with no
import between them, so reaching one reaches the rest. Packages
are owned by the nearest manifest, and health can be split per package.

Files no adapter claims (images, data, unknown suffixes) are discovered but
never judged; a claimed file that fails to parse degrades to a diagnostic.
