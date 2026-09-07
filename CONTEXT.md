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
a directory, a named namespace, the owner and its subtypes (heirs, with or without
the namespace), the owner's reach (inherited), or exported.
_Avoid_: visibility rung, region, scope token, private (a keyword; the reach it
spells is the owner's or the file's)

**Effective reach**:
A declaration's reach after the engine caps it: by every owner above it, and by the
mounts above its file. Never wider than either.

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
(`mod x;`), carrying the reach it is attached with. An **include** makes the target's
content part of the importing file.

**Mount cap**:
The narrowest reach the mounts above a file impose on it, read from that file: what a
privately mounted module puts around everything under it. Folded into the effective
reach, so nothing under it is published surface.

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
A member that satisfies a surface its OWNER promised — an override, an interface
method, a runtime hook a base declares. Alive while its owner is, and of no color:
nothing outside the graph is entered there, the caller simply holds the supertype and
dispatches through it. Two sources, one verdict: the supertype the graph RESOLVED
(its members are the requirements), and — for a base the project does not contain — the
requirements a rule NAMES, matched through the whole declared supertype chain. Every
judgment that stands down for one stands down for the other, `internal-only` included:
narrowing a witness is a compile error, not advice.
_Avoid_: conformer, override root (a witness is never a root)

**Namespace clause**:
What a file says its namespace is (`package com.a`, `package x`).

**Attachment**:
Whether a file belongs to its namespace in every build (regular) or only in test
builds (test-only, as a `_test.go` file does). The file's own statement, and the only
one a language makes where no manifest declares a test unit.
_Avoid_: file role (that names the entry, not the membership)

**Compilation**:
The kind of build a file lands in — its unit's kind, or the test build where the
file's attachment is test-only. Derived once on the graph; what a `Trigger::Name`'s
`in_unit` narrows to, and what fences the colour flood.

**Embedded region**:
A span of a file written in another language (`<script>`, `<style>`), with the
language — named by the file suffix its extension claims — and its mode (module or
classic script). The host reports it; the engine extracts it with that language's
extension into the host's evidence, at the host's offsets, and that extension
resolves what it imports.

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
crate is not a friend of the library it tests. The manifest states it, by name, and the
engine resolves the name the way it resolves a dependency; a unit's pool is its files plus
its friends'.

**Published**:
Whether a library unit's exported API is consumed outside the project. Declared
(`publish = false`, `private: true`) or inferred when the manifest is silent: a library is,
nothing else is. Where the ecosystem publishes every export, an exported declaration of a
published unit is kept by the outside world (the `published` keeper) and never advised to
narrow; an unpublished unit's exports are its own.

**Published surface**:
Which exported declarations a unit hands to the outside: every export (a jar, a crate, a
Go module, a Python distribution) or only what its entries export (npm). The ecosystem's
resolution rule, so the language declares it; `internal-only` never advises narrowing
what is on it. A file that is a test AS A WHOLE is on no unit's surface whatever it
exports — no importer can name it — which matters where a unit holds production and test
files together, as a Go module does.

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
_Avoid_: sees (retired), seen_from, region

**Co-visibility**:
The files a language compiles TOGETHER, read off the scope forest where the language
says its namespace compiles as one (Go's package). Reachability's input, not a pool:
a pool asks who may NAME a declaration, this asks what the compiler builds, and only
the second makes a file exporting nothing alive because its package is. Where a
namespace spans the compilation (Java), it also holds the same-named files of every
unit this one compiles AGAINST — the inverse of the pool's direction, since a test
build holds the library while the library's build holds no test of it. A file that
is a test as a whole is the other asymmetry — the production build never compiles
it, so no production colour crosses through it.
_Avoid_: sees (retired), unit mates

**Pool**:
The files, or the span within a file, from which an unqualified reference to a name
counts as a use of a declaration: the subtree of the node its effective reach names,
plus friend units when the node is a unit.

**Ladder**:
The reaches a language can spell with a keyword, narrowest first, each step saying which
declarations can take it: any, a free declaration alone, a member alone. The step is what
`internal-only` advises: the narrowest one at or above the rung the uses need and below
the declared one; no such step, no advice.
_Avoid_: narrowable scopes, export narrowing

**Path alias**:
A specifier prefix the build system rewrites to directories (`tsconfig` paths, Sass
load paths, `go.mod` replace, an import map).

**Ignore**:
A path the language's own tool never compiles (`vendor/`, `_file.go`, `node_modules/`,
`site-packages/`), declared by the language as globs. Discovered, never claimed, and a
manifest under it declares nothing. What a manifest excludes is its unit's membership,
not an ignore.

### Judgment

**Finding**:
One verdict of one category on one subject, with a message. Two findings with one
identity are one finding — so identity must tell apart everything a verdict can be
about.

**Subject**:
What a finding is about: a file, a symbol, a package, a dependency, a directory, an
import statement, a suppression. Rendered one way everywhere a subject is shown or
addressed; the parts are never re-spelled by a consumer. A subject a file can hold
more than once under one spelling — an overload, the same import written twice, the
same allow written twice — carries its **position** among those, so two of them are
two subjects.

**Selector**:
The address of one declaration inside its file: its owner (when it is a member), its
name, its signature (when its language has one) and, when nothing else does, its
position among the declarations that share all of those. Unique within a file by
construction — the evidence sink guarantees it, so an overload, a field and method of
one name, or two nested types of one name can never share an address.
_Avoid_: symbol path, qualified name

**Signature**:
What a language reads beyond the identifier to tell same-named declarations apart, as
that language spells it: Java's and Kotlin's parameter types `(int, String)`, Swift's
argument labels `(_:with:)`. Never the identifier (references carry identifiers
alone), never parameter names or a return type (renaming a parameter does not make a
new method). Absent where the language has no such thing.
_Avoid_: arity, overload key

**Identity**:
What makes a finding the same finding across runs: its category and its subject —
never its span, so moving code changes nothing. Unique within a run by construction:
no analysis needs a discriminator to keep two subjects apart. Baselines and
suppressions match on it, and `fixed` is what a baseline holds and a run no longer
does.

**Root**:
What anchors liveness: a unit's entry, every file of a test or tooling unit (its kind is
its role), a test, a launched file, a declaration a dispatch rule names, the published
surface. Roots carry a color: production, test or tooling.

**Color**:
Which roots reach a file: production, test, tooling. Verdicts read colors.

**Dispatch rule**:
Data: a trigger (a marker path, a name pattern in files of a given role, a relation to a
base, a member of a matching owner, the members an external base requires) and an effect
(a root of a color, a witness, an exemption from `unused`, a generated file), with a
confidence. The language's own rules ride its spec; a framework's ride a rule pack.

**Pattern**:
What a trigger compares with: literal text where `*` matches any run. Compared against
the name AS THE FILE WRITES IT and against the name its own BINDINGS qualify — so a rule
written `com.vendor.Closer` reaches an `implements Closer` in a file that imports it and
says nothing about the same simple name from another package, while a rule written bare
reaches whatever the language's implicit scope left unqualified.
_Avoid_: glob (that names a path pattern — an ignore, a file role)

**File role**:
What the project says a file IS — a test, a tooling artifact — stated by its unit's
kind or, where no unit spoke for it, by a `FileRole` glob the language declares. Both
land as a whole-file root: the role answers "is this an ENTRY", never "which build
carries it" — that is the compilation's question, and a `Trigger::Name` asks it there.
_Avoid_: attachment, test-only (that names a finding), compilation

**Rule pack**:
A conduct extension that declares dispatch rules and an activation and nothing else.
No code, no content access.

**Generated file**:
A file whose header carries its ecosystem's banner (`@generated`, `// Code generated …
DO NOT EDIT.`), reported as a marker and named by an `Effect::Generated` rule. What it
DECLARES is the generator's, so no accusation about those names is this project's to
answer for. What it IMPORTS and NAMES is evidence like any other — the code it keeps
alive is really kept alive, and a root it declares really runs. Being generated is not
a root: a generated file nothing imports is dead weight whoever wrote it, and the
whole-file verdict stands.
_Avoid_: origin, tooling root (a generated file is not rooted)

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
