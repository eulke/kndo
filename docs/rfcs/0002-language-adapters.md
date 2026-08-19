# RFC 0002 — Language Adapters

**Status:** Accepted · **Depends on:** RFC 0001 · **Normative contract:** [contracts/core-traits.md](../contracts/core-traits.md)

## 1. Purpose

A **language adapter** is the only component that understands a language. It translates source
files into the core's language-neutral vocabulary. The core discovers adapters through a registry
and treats them uniformly; adding a language is adding one crate that implements one trait.

## 2. Responsibilities (exactly these, no more)

An adapter owns what the **language specification and its standard toolchain** define:

1. **Claiming files** — which extensions/filenames it handles (`.ts`, `go.mod`, `BUILD.gradle.kts`…),
   including classifying each file on two orthogonal axes — *role* (production / test / tooling:
   `_test.go`, `*.spec.ts`) and *origin* (authored / generated / vendored: headers like
   `// Code generated … DO NOT EDIT`).
2. **Parsing** — producing a syntax tree (tree-sitter grammar, ADR 0002) and surviving broken code.
3. **Extraction** — emitting `FileFacts`:
   - declared symbols (name, kind, span, visibility, exported?)
   - references (identifier uses with enough context for resolution)
   - imports (raw specifier + kind: relative, package, stdlib)
   - language-defined roots (`main` functions, `pub` API of a library crate, exported members of
     an npm package's `main`/`exports`, `@main`/top-level code in Swift…)
   - per-function cyclomatic complexity (for CRAP, RFC 0005 §10)
   - normalized token streams per function/block (for duplicate detection, RFC 0005 §6)
4. **Resolution** — mapping an import specifier or a reference to its target, given the graph
   assembly context (e.g. Node resolution algorithm incl. `tsconfig` paths; Go module paths; Java
   package/classpath conventions; Cargo module tree).
5. **Manifests** — parsing the language's dependency manifests (`package.json`, `go.mod`,
   `Cargo.toml`, Gradle version catalogs, `Package.swift`) into `ManifestDependency` nodes, and
   mapping import specifiers → package names (e.g. `lodash/fp` → `lodash`, `golang.org/x/net/html`
   → module).

**Explicitly out of adapter scope** (goes to plugins, RFC 0003): framework conventions (a React
component "used" via JSX by a router config; Spring beans; SwiftUI previews), test frameworks
beyond the standard library/dominant convention, coverage formats, org-specific entry points.

Rationale for the boundary: language specs are stable and versioned; ecosystems are fashion.
Keeping fashion out of adapters keeps them small, testable, and slow-changing.

## 3. Non-source languages (JSON, CSS)

The vocabulary must not assume "code". For data/style languages the mapping is:

- **JSON**: files claimed only when *referenced* semantics exist (e.g. imported by JS/TS, listed
  in a manifest). Symbols are not extracted; JSON participates as import *targets* so file-level
  `unused` findings cover config/data files. Well-known manifests (`package.json`, `tsconfig.json`) are
  claimed by the *owning* adapter instead.
- **CSS/SCSS/LESS**: symbols are selectors/mixins/variables; references are `@import`/`@use`,
  `composes`, and — via the cross-language edge mechanism (§4) — class-name usage from JS/TS/HTML.
  This enables "unused CSS rule" as a normal unused-symbol finding.

## 4. Cross-language edges

Real projects cross language boundaries (TS imports a CSS module; JS reads a JSON file; Kotlin and
Java in one Gradle module). Adapters never call each other. Instead, an adapter emits an import
with a raw specifier; the **core's resolution driver** asks *each* registered adapter's resolver
whether it can resolve that specifier to a file it claims. First unambiguous claim wins; ambiguity
demotes the edge to `probable`.

## 5. Confidence & dynamic constructs

Static analysis of dynamic features must degrade honestly, not guess:

- `certain` — spec-level static resolution (a Go import, a Rust `use`, a TS named import).
- `probable` — resolution relied on convention or a single plausible candidate (string literal in
  `require(x)` with a resolvable literal value; duck-typed method with one candidate).
- `possible` — dynamic construct detected but not resolvable (`import(variable)`, reflection,
  `eval`). The adapter emits a **wildcard edge** from the file to *unknown* instead of guessing
  a target.

This is per-edge strength; how many edges of differing strength combine into a node's overall
reachability color and confidence — including how wildcard edges fold in — is the tiered
algorithm in RFC 0005 §1. Findings inherit the *weakest* confidence on their evidence path and
report it (RFC 0006).

## 6. Adapter lifecycle & versioning

- Adapters implement the `LanguageAdapter` trait (normative in contracts/core-traits.md) and
  register capabilities: claimed globs, manifest patterns, grammar version, **facts schema version**.
- The facts schema version participates in the cache key (RFC 0004 §3): bumping it invalidates
  only that adapter's cached facts.
- First-party adapters are compiled into the binary; the same trait is bridged to WASM for
  third-party adapters (ADR 0003). An adapter must not perform I/O beyond the file content handed
  to it — all filesystem access goes through the core (determinism, sandboxability, testability).
- **Version-dependent language data is generated data, never hand-maintained code.** Facts that
  track a language/runtime release cadence — stdlib/builtin module lists (Node builtins, Go
  packages, Java modules), reserved words, version-gated syntax tables — ship as data files
  embedded at build time, produced by a **checked-in generator** that queries the authoritative
  source (`module.builtinModules`, `go list std`, `java --list-modules`). A new runtime version
  means regenerating a file, never editing adapter code. Two companions to the rule: prefer
  *structural* version-proof signals over lists where the language offers one (Node's `node:`
  prefix covers every post-v18 builtin by that runtime's own policy — the bare-name list is a
  frozen legacy set); and a **manifest-declared dependency always shadows the builtin list**
  (the userland `punycode` package is real) — declared intent beats shipped data. Never query
  the *ambient* installed runtime at analysis time: that would make findings depend on the
  machine, violating determinism (RFC 0008 §4) — the generator runs at kndo development time,
  not at the user's analysis time.

## 7. Per-language notes (initial scope)

| Language | Resolution highlights | Roots (language-defined) | Test-role detection |
|----------|----------------------|--------------------------|----------------------|
| JS/TS | Node ESM+CJS, `tsconfig` paths/baseUrl, package.json `exports`; JSX/TSX | package entry points (`main`, `exports`, `bin`), scripts referenced files | `*.test.*`, `*.spec.*`, `__tests__/` |
| Go | Go modules, internal/ visibility | `main.main`, exported identifiers of library modules, `init` | `_test.go` |
| Java | package + source roots (Maven/Gradle layout) | `public static void main`, public API of published modules | `src/test/` |
| Kotlin | as Java + top-level functions, multiplatform source sets (post-1.0) | `main`, public API | `src/test/`, `commonTest` |
| Swift | SPM targets, module imports | `@main`, top-level code in `main.swift`, public API of library targets | `Tests/` targets |
| Rust | module tree from crate roots, `use`/paths, features (coarse: any-feature = live) | `main`, `lib.rs` `pub` API, `#[no_mangle]`/`export` | `#[cfg(test)]`, `tests/` |
| JSON | n/a (target-only) | n/a | n/a |
| CSS | `@import`/`@use` graph, CSS Modules | none (reachability comes from consumers) | n/a |

Each adapter gets its own detailed spec document under `docs/adapters/<lang>.md` before its
implementation milestone (ROADMAP), including the tricky cases above.

## 8. Testing contract

Every adapter ships **conformance fixtures**: a miniature project + the expected `FileFacts` and
expected findings, executed by a shared test harness in the core. This doubles as the compliance
suite third-party adapters run against.
