# kndo

kndo judges a project's code for waste — dead, test-only, untested, duplicated,
cyclic, undeclared — from the evidence its language extensions report, never from a
convention the engine assumes. This file is the context's language: what the words
mean, and which words we avoid. Decisions live in `DECISIONS.md`; the law of the
repository in `CLAUDE.md`.

## Language

### Evidence

**Evidence**:
What an extension reports about one file or one manifest, weighted by confidence,
before any judgment. Weak evidence is a natural concept; a "possible fact" is not.
_Avoid_: facts, observations, index, claims

**File evidence**:
The evidence of one source file: declarations, references, imports, roots, markers,
relations, comments, metrics, embedded regions, its namespace clause and attachment.

**Manifest evidence**:
The evidence of one manifest: units, packages, dependencies, mentions, path aliases,
launcher roots, ignores.
_Avoid_: project facts, package facts

**Declaration**:
A named thing a file declares, with a symbol kind, a span, a reach and an optional
owner (the declaration it is a member of).

**Reach**:
How far a declaration's name may legally be used, as an address in the scope forest:
its owner, its file, its namespace or an ancestor of it, its unit or the unit's group,
a directory, a named namespace, the owner's reach (inherited), or exported.
_Avoid_: visibility rung, region, scope token

**Effective reach**:
A member's reach after the engine caps it by its owner's: never wider than the owner.

**Reference**:
A use of a name in a file, with a kind (call, read, write, extend, implement, type
use). A **qualified reference** also names what it went through: an import binding
(`pkg.Name`) or a namespace path (`super::x::f`).

**Import**:
One statement that reaches another file or package: a target (relative, package or
pattern), a shape (bindings, namespace, glob, side effect, re-export, mount, include,
mention) and a timing.

**Timing**:
When an import runs: at load, lazily (inside a function or a condition), or erased
(types only). Cycles are judged over load-time imports only.

**Mount**:
An import that makes its target a child namespace of the importing file's namespace
(`mod x;`). An **include** makes the target's content part of the importing file.

**Marker**:
An attribute, annotation, decorator, modifier or directive on a declaration or a
file, with its path and arguments. What a marker MEANS is a dispatch rule's business.
_Avoid_: attribute table, root attribute

**Relation**:
A typed link from a declaration to a named type — it extends a base, or it implements
an interface, conforms to a protocol, satisfies a trait bound (one word: implements).
An override is not reported: it is what the engine derives when a member's name sits
on a type its owner relates to.

**Witness**:
A member whose name satisfies a requirement of a type its owner relates to (an
interface method, a protocol requirement, an override). Alive while its owner is.

**Namespace clause**:
What a file says its namespace is (`package com.a`, `package x`).

**Attachment**:
Whether a file belongs to its namespace in every build (regular) or only in test
builds (test-only, as a `_test.go` file does).

**Embedded region**:
A span of a file written in another language (`<script>`, `<style>`), with the
language and its mode (module or classic script). The engine extracts it with that
language's extension.

### Structure

**Unit**:
What the build system compiles as one artifact: a Cargo target, a SwiftPM target, a
Maven or Gradle source set of one module, an npm workspace package, a Python
distribution, a Go module. It has a kind (library, executable, test, bench, example,
tooling), roots, excludes, entries, dependencies and a publication.
_Avoid_: package (that word names the identity a bare specifier resolves to), module

**Friend**:
A unit allowed to use another unit's unit-reaching names: a Kotlin test source set
over its main, a Swift test target with `@testable import`. A Rust integration test
crate is not a friend of the library it tests.

**Published**:
Whether a library unit's exported API is consumed outside the project. Declared
(`publish = false`, `private: true`) or inferred when the manifest is silent.

**Namespace**:
The language's name-space node a file attaches to: a Java or Kotlin package by name,
a Go package by directory and clause, a Rust module by its mount chain, a Python
module by source root and path, a Swift module (the unit), an ES module (the file).
How namespaces nest is the language's declaration (flat, by directory, mounted, by
path, per file).

**Scope forest**:
The engine's structure of project, units, namespaces, files and owners, built from
evidence and manifest evidence. Pools, effective reach and the narrowest expressible
rung are read from it.
_Avoid_: sees, seen_from, region

**Pool**:
The files, or the span within a file, from which an unqualified reference to a name
counts as a use of a declaration: the subtree of the node its effective reach names,
plus friend units when the node is a unit.

**Ladder**:
The reaches a language can spell with a keyword, narrowest first. The rung is what
`internal-only` advises.
_Avoid_: narrowable scopes, export narrowing

**Path alias**:
A specifier prefix the build system rewrites to directories (`tsconfig` paths, Sass
load paths, `go.mod` replace, an import map).

**Ignore**:
A path the language's own tool never compiles (`testdata/`, `vendor/`, `_dir/`), or
a manifest excludes. Discovered, never claimed.

### Judgment

**Root**:
What anchors liveness: a unit's entry, a test, a launched file, a declaration a
dispatch rule names. Roots carry a color: production, test or tooling.

**Color**:
Which roots reach a file: production, test, tooling. Verdicts read colors.

**Dispatch rule**:
Data: a trigger (a marker path, a relation, a name pattern in a unit kind, a member
of a matching owner, an external base's required members) and an effect (a root of
a color, a witness, an exemption from `unused`, a generated file), with a confidence.
The language's own rules ride its spec; a framework's ride a rule pack.

**Rule pack**:
A conduct extension that declares dispatch rules and an activation and nothing else.
No code, no content access.

**Keeper**:
The one piece of evidence that keeps a declaration alive: a root, a reference in its
pool, a qualified reference, a binding import, a glob importer, an opaque importer,
the published surface, a witness, an exemption. `unused` asks for one; `used-by`
lists them.

**Expectation**:
A fixture's claim as data (`expectations.toml`): a subject that must be reported, a
subject that must stay unreported, or a known gap the tree cannot close yet. A
comment cannot contradict a pin because the claim is the pin.
_Avoid_: claims (that word names file claiming by suffix)

### Extensions

**Extension**:
One species: anything that declares a spec and implements the hooks its spec gates.
A language **adapter** claims files and extracts evidence; **conduct** contributes
roots, findings or ingested coverage under activation.

**Coordinate**:
An extension's identity (`kndo:rust`, `kndo:junit`). The `kndo:` namespace is
reserved for built-ins.
