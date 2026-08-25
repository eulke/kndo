# Adapter Spec — Java

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M5
**Grammar:** tree-sitter-java

The fourth adapter, and the first with a genuinely two-ecosystem manifest story (Maven and
Gradle) and no dogfood corpus of its own — kndo is written in Rust, so unlike the Rust adapter
(validated by running kndo on itself), Java's precision rests entirely on the conformance
fixtures (§6). Node kinds throughout this doc are pinned against real tree-sitter-java 0.23.5
output (`kndo-adapter-java/src/parsing.rs`'s two `#[ignore]`d ground-truth dumps), not guessed.

## 0. What's structurally different from JS/Go/Rust, and why it matters here

- **Package identity is declared AND path-enforced — a hybrid of Rust's and Go's models.**
  Every file opens with a `package com.foo.bar;` statement (Rust's declared-tree shape), but
  javac also requires the file to live at a matching directory suffix under some source root
  (`com/foo/bar/Widget.java`, closer to Go's directory-is-the-unit convention) — except the
  source root itself (`src/main/java`, `src/test/java`, or something custom) is a **build-tool
  convention, not a language rule**, so a file's directory alone doesn't say what its package
  is without first knowing where the source root starts. The adapter never tries to detect
  source roots: `FileFacts::unit` is set to the **declared** package name (the dotted string
  from the `package` statement) — this sidesteps source-root detection entirely and is exactly
  as precise, since package membership for resolution purposes is what the compiler actually
  uses. A file with no `package` statement (the unnamed/default package) gets `unit: None`
  — its own top-level types resolve same-file only, matching every other adapter's `None`
  behavior; this is also real Java behavior (the default package cannot be referenced by name
  from a *named* package at all, so no cross-file resolution is even possible there).
- **No relative imports; two structurally different import shapes.** `import com.foo.Bar;`
  names one type by its fully-qualified path. `import com.foo.*;` (wildcard) imports **every
  top-level type physically declared by files in that exact package** — not sub-packages, not
  members — which, because a package IS its `unit` in this adapter's model, is *statically
  enumerable* the same way an unqualified same-unit reference already resolves; no
  approximation needed (§3). `import static com.foo.Bar.CONST;` (and its own wildcard form,
  `import static com.foo.Bar.*;`) bring a *member* (field, method, or nested type) into scope
  unqualified — the one import shape that targets a member rather than a type.
- **Four-rung visibility, but only two apply to top-level types — and package-private maps to
  `Unit`, not kndo's `Package`.** A **top-level** class/interface/enum/record can only be
  `public` or package-private (no modifier) — `private` and `protected` are illegal there, so
  the file/type-level ladder is exactly Go's shape: two rungs. **Members** (fields, methods,
  constructors, nested types) get the full four: `private` (class-only — no `File`-narrower
  scope exists in kndo's model, so it widens to `File`, the nearest available bucket: over-
  approximating "any code elsewhere in the file might reach it" is the safe direction, same
  reasoning as Rust's `pub(super)` widening), package-private (**`Unit`**, exact — Java's own
  "default access" scope is the `com.foo`-style declared package, which is exactly what
  `FileFacts::unit` already carries; kndo's `VisibilityScope::Package` is a *different*
  granularity — "same manifest/workspace-member" (RFC 0011's `PackageId`, JS's one-`package.json`
  scope) — and a single Maven/Gradle module routinely holds many Java packages, so mapping
  package-private to `Package` would silently collapse every Java package in one module into
  one bucket; `required_scope`'s own algorithm checks `unit` *before* `PackageId`, exactly
  mirroring Go's choice), `protected` (package **plus subclasses in any other package** — no
  scope in `{File, Unit, Package, Public}` represents "package ∪ my-subclasses-anywhere", so it
  widens to `Public`, the nearest wider bucket that safely covers the cross-package case; this
  trades recall for precision the same direction as every other conservative widening in the
  codebase), `public` (`Public`, exact). One ladder covers both: `[File "private", Unit
  "package-private", Public "protected", Public "public"]` — two rungs sharing a scope is legal
  (RFC 0012 §6's `pub(super)`
  precedent), and `VisibilityLevel` still distinguishes them for the ladder's own label text.
- **No inline test regions.** Unlike Rust's `#[cfg(test)]`, Java test code is always a
  **separate file** — the Maven/Gradle Standard Directory Layout (`src/test/java/**`) is the
  authoritative, universal convention (every build tool, every IDE, every CI config assumes
  it). `FileFacts::test_spans` (contracts §2) is therefore always empty for this adapter —
  worth stating explicitly since it's the mechanism the *previous* adapter (Rust) needed and
  this one structurally doesn't.
- **Dispatch rooting, for the same underlying reason as Rust's trait-impl methods.** JDK-
  invoked contract methods — `equals`/`hashCode`/`toString`/`compareTo` overrides, functional-
  interface implementations passed as method values — are called by collections, string
  concatenation, `Comparator`-consuming APIs, and the like, **never by a named call site in
  user source**. An ordinary interface-implementation method (`r.run()` where `r`'s static
  type is `Runnable`) *is* usually visible to the duck-typed member fallback (RFC 0012 §3) as
  a bare `run` call — but not always (the call may come from framework/JDK code entirely
  outside the graph), and distinguishing "this override's dispatcher is visible in-source" from
  "it isn't" per-method isn't something extraction can determine without a typechecker. So,
  matching Rust's stance exactly: **every `@Override`-annotated method roots
  `Production`/`Probable`**, unconditionally — blanket, safe-direction, no attempt to narrow to
  just the JDK-contract subset.
- **No reliable import→dependency-coordinate mapping — the one real scope cut.** npm's package
  name, Go's module path, and Cargo's crate name all appear *structurally* in the import
  specifier itself. A Java import names a **package** (`com.google.common.collect.*`), and
  nothing about that string says which Maven coordinate declared it (`com.google.guava:guava`
  — groupId, artifactId, and Java package frequently share **none** of the same text). The
  only sound way to resolve this is to actually resolve the classpath (run Maven/Gradle, or
  read the local repository), which kndo — a static source analyzer — structurally never
  does. Consequence (§3, §4): `resolve()` never returns `Resolution::Dependency` for any
  external Java import (so `undeclared` never fires — no realistic scenario, same *outcome* as
  Go's stance for a different root cause), and `PackageNode::resolves_dependency_usage: false`
  makes `dependency_hygiene` skip Java's `unused`/`test-only` dependency verdicts entirely
  (one diagnostic, not a false-positive flood — contracts §2). **`version-skew` is unaffected**
  — it compares declared versions across manifests directly, no usage edge needed, so it's
  fully precise for Java from day one.
- **JPMS (`module-info.java`, Java 9+ module system) is out of scope for v1** — parked, same
  posture as JS's un-followed tsconfig project references. A `module-info.java` file is still
  claimed (language `java`, ordinary `.java` glob) but its root node is `module_declaration`,
  none of the class/interface/enum/record shapes extraction walks for, so it naturally yields
  zero declarations — no special-casing needed in `claim()`. `package-info.java` (package-level
  Javadoc/annotations, no type declarations) behaves the same way, for the same reason. Both
  additionally classify as **Tooling role** (M6): their consumer is javac/javadoc, so their
  "reachability" is healthy by definition rather than an `unused` accusation.

## 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `java` | `**/*.java` (`module-info.java`/`package-info.java` included — see §0's last bullet; both yield zero declarations and classify as Tooling role) |
| Manifests | `**/pom.xml` (Maven), `**/build.gradle` + `**/build.gradle.kts` (Gradle), `**/settings.gradle` + `**/settings.gradle.kts` (Gradle multi-project topology only — §4) |
| Role `test` | `src/test/java/**` (Maven/Gradle Standard Directory Layout — the authoritative signal) OR a bare filename matching Maven Surefire's own default include patterns (`Test*.java`, `*Test.java`, `*Tests.java`, `*TestCase.java`) — a belt-and-suspenders fallback for non-standard layouts (flat scripts, Bazel-built Java) that still follow Surefire's naming convention. An OR, not additive: role is single-valued, and a `src/test/java` file is typically *also* Surefire-named, so the two signals agree far more than they diverge |
| Role `tooling` | not detected in this slice — same stance as Go (§0 there): no ecosystem-wide config-file convention comparable to `webpack.config.js` exists for Java source files (the manifests themselves — `pom.xml`/`build.gradle` — are pure manifest facts, never role-classified as source) |
| Origin `generated` | `@Generated` — the real, standard annotation (`javax.annotation.Generated` pre-JDK9, `javax.annotation.processing.Generated` JDK9+) that annotation processors (Lombok, MapStruct, protobuf, Dagger) actually emit — detected structurally by extraction (any top-level type carrying it), reported via `FileFacts::detected_origin` (RFC 0012 §7). The toolkit's text-marker scan (`@generated`, `Code generated`, `DO NOT EDIT`) still runs as a second, independent signal for generators that skip the annotation |
| Origin `vendored` | `vendor/**`, `third_party/**` (toolkit universal list) — not a real Java convention (dependencies live in `~/.m2`/Gradle's cache, never checked into the tree), so this is essentially inert for Java, included only for consistency |

Build-output directories (`target/**` for Maven, `build/**`/`.gradle/**` for Gradle) need no
adapter-side exclusion at all: discovery already walks only what the project's own
`.gitignore`/`.ignore` admits (same "OUT_DIR" stance Go's doc states for `go build` artifacts),
and every real Java project's default `.gitignore` excludes them.

**`VisibilityLevel`**: `0` (private, widened to `File`), `1` (package-private, `Package`), `2`
(protected, widened to `Public`), `3` (public, `Public`) — computed from the `modifiers` node's
child tokens (`private`/`protected`/`public`; absence of all three = package-private), never
from Java's `default`-keyword-that-doesn't-exist (there is no `default` visibility keyword —
the absence of a modifier IS the level, matching how Go reads capitalization rather than a
keyword). §0 has the full ladder derivation and reasoning.

## 2. Extraction

**Idioms with structural exemptions (M6 FP hunt, junit4 corpus):**
- Constructors declare as `SymbolKind::Constructor` named `<init>`: `new Foo()` references the
  *type*, never the constructor symbol, so the core ties a constructor's liveness to its
  container (a Certain class→constructor References edge in assembly) and `unused` never
  accuses the kind directly — a private utility-class constructor exists precisely to never be
  called, and deleting it would change behavior.
- `serialVersionUID` fields are not declared at all: the JVM reads them reflectively, so a
  declaration would guarantee a false `unused` on every `Serializable` class.
- Interface/annotation members with no modifier are implicitly `public` (JLS §9.4) — extraction
  applies the interface-body default, and interface constants (`constant_declaration`) extract
  like fields.
- `protected` members are **exported** (external subclasses of a published library override
  them) on the Public-scope rung — RFC 0012 §6's ladder table.

**Declarations** — top-level and nested (`member_of`-owned, RFC 0012 §3) alike:
`class_declaration`, `interface_declaration`, `enum_declaration` (+ `enum_constant` as
`EnumMember`, and — unlike Rust/Go — an enum can also declare ordinary methods in an
`enum_body_declarations` block, extracted exactly like a class body), `record_declaration`
(Java 16+; `SymbolKind::Other("record")` — record components (`x`, `y` in `record Point(int x,
int y)`) are **not** extracted as separate declarations, matching the "declarations must be
textual" principle: their accessor methods (`x()`, `y()`) are compiler-synthesized, never
appear as a `method_declaration` node, and calls to them (`point.x()`) simply produce no
reference edge — a documented, safe-direction gap, same class as JS's property-assignment-
callable limitation), `annotation_type_declaration` (`@interface Foo { … }` —
`SymbolKind::Other("annotation")`), `method_declaration` + `constructor_declaration` (member,
name = `Owner.methodName`), `field_declaration` (one `Declaration` per
`variable_declarator` — `int a, b;` is two declarations, matching Go's grouped-`var` stance).
Nested types (`class`/`interface`/`enum`/`record` declared inside a `class_body`/
`interface_body`/`enum_body`) are members (`member_of` = the enclosing type's bare name),
`Outer.Inner` qualified naming, recursing to arbitrary nesting depth. **Not extracted**:
anonymous classes (`new Runnable() { … }` — the `object_creation_expression`'s trailing
`class_body`, when present, is walked for its *references* like any other body, but its
methods contribute no declarations — there is no name to hang a finding on) and local classes
(a class declared inside a method body — rare, same non-declaration stance).

**References**: `method_invocation` (`object`/`name`/`arguments` fields — `object` present +
`identifier` → member/qualified access, `object` absent → bare call), `field_access`
(`object`/`field`), `identifier` reads/writes outside those shapes, `method_reference`
(`Type::method`, `instance::method`, `this::method`, `Type::new` — extracted as a `Call`-kind
reference to the method-name segment, with `scope_context` = the qualifier when it's an
`identifier`/`this`, mirroring the plain-call qualifier shape in §3). `type_identifier` and
`scoped_type_identifier` positions (`extends`/`implements`/`throws`/field & parameter types/
generic bounds/`new` targets) → `TypeUse`; `superclass`'s type and each entry of
`super_interfaces`'/`extends_interfaces`' `type_list` → `Extend`.

A `scoped_type_identifier` emits the LAST segment as the reference name, plus `scope_context`
= the qualifier **when the qualifier names a type** (`Outer` in `Outer.Inner`) and nothing when
it is a package path (`java.util.List`). Both discriminators are needed. Structural:
tree-sitter nests multi-segment paths, so a package path's qualifier is itself a
`scoped_type_identifier` while a nested type's is a bare `type_identifier`. Lexical: a
single-segment qualifier is still ambiguous between a one-word package (`p.Foo`) and an
enclosing type, and only the capitalization convention separates them — guessing wrong on
`p.Foo` sends a name the free-name tables resolve today into the member-only fallback, which
top-level types never reach, and loses the edge.

Dropping the qualifier is not merely lossy, it **mis-binds**. Resolution consults the file's
import bindings before anything else, so a bare `Query` extracted from
`new ParameterHandler.Query<>(…)` in a file that also does `import retrofit2.http.Query` binds
to the annotation: the nested type reads as dead and an unrelated type collects a reference it
never received. Pinned by the `nested-type-qualifier` fixture. Lambda bodies
(`lambda_expression`) are walked like any other expression — their parameter names shadow
outer bindings for extraction's purposes exactly the same safe-direction way locals already do
everywhere else (over-approximating ALIVE, never under).

**Roots (`RawRoot`)**: `public static void main(String[] args)` in **any** class (Java has no
Go-style "must be package main" restriction — any class can be an entry point, and a real
project may have several for different tools) → `RootKind::Production` at `Probable`
(unconditional, mirroring Go/Rust's blanket `main` stance — only the actually-invoked one
truly runs, but which one is a packaging decision `MANIFEST.MF`'s `Main-Class` records, not
something kndo parses in v1). `static_initializer` blocks and instance initializer blocks run
implicitly at class-load/instantiation time — not independently rooted (they're not named
declarations at all; their bodies are walked as part of the *enclosing type's* liveness, which
already requires the type itself to be reachable — a static initializer in an unreachable class
never runs anyway, so no separate root is needed for correctness). **`@Override`-annotated
methods** root `Production`/`Probable` (§0's dispatch rule).

**Suppressions**: `// kndo:allow …` / `/* kndo:allow … */` — same syntax and scope rules as
every other adapter (RFC 0005 §12 is language-neutral; Java's line/block comment syntax is
identical to JS/Go/Rust's).

**Metrics**: cyclomatic complexity +1 per `if_statement`, `for_statement`/`enhanced_for_
statement`, `while_statement`/`do_statement`, `catch_clause`, `case`-arm (switch — one per
label past the first, matching JS's n-way-match rule), `? :` (ternary), `&&`/`||`, and each
`lambda_expression` body counted as its own function-shape unit (same "each closure gets its
own metrics" stance the JS/Rust extractors already take for arrow functions/closures — a
lambda passed to `.forEach` is a callable shape in its own right). Fingerprints: normalized
token stream per method/lambda body, same `$n`-renaming scheme as every other adapter.

**Dynamic constructs → `DynamicUse`**: none emitted in this slice. `Class.forName(String)`
reflection exists but is rare in application code and — like Go's `reflect`/`plugin` stance
(§5 there) — not modeled; the wildcard-edge machinery stays available if dogfooding on a real
Java corpus later surfaces false `unused` positives traceable to it.

## 3. Imports & resolution

Emitted import kinds:

| Form | Emission |
|------|----------|
| `import com.foo.Bar;` | specifier `com.foo`, binding `[Bar]` — this is the ONE shape whose specifier is the *package*, not the full dotted path (unlike Go/Rust, whose two-step tail rule needs the ambiguity; Java's grammar already hands the package/type split via the `scoped_identifier`'s own nesting, so no guessing is needed) |
| annotations on a declaration | `Declaration::markers`, the names as written and in source order — `@Controller`, `@RequestMapping("/x")` and `@Advice.OnMethodEnter` all contribute. A qualified spelling contributes its last segment too (`OnMethodEnter` beside `Advice.OnMethodEnter`), because either is a legitimate `kndo.toml` entry and the adapter cannot know which the project will pick. FACTS, never verdicts: this adapter has no idea which annotations a framework acts on, and emits every one. Their consumer is `[[externally-invoked]]` — Spring's component scan, JUnit's lifecycle and ByteBuddy's `@Advice` are all invocations no source reference can ever record |
| `import com.foo.*;` | specifier `com.foo`, no bindings, and **two** facts: `opaque_namespace_use: true` — the resolved target file's own declared symbols stay `Possible`-reachable via the same `Wildcard` keep-alive mechanism Go's dot-import and JS's `export *` already use (contracts §2), at the SAME single-representative-file granularity Go's own package resolution already accepts (§3 point 2) — and `module_names_visible: true`, the JLS 7.5.2 type-import-on-demand rule itself: every type in that package is legal here *unqualified*, so the core's bare-name fallback consults that unit's table at Certain. Only top-level declarations live in a unit's name table, so this brings in exactly what the JLS says it does — types, not static members |
| `import static com.foo.Bar.CONST;` | specifier `com.foo::Bar` (§3.1), binding `[CONST]`, `type_only: false` |
| `import static com.foo.Bar.*;` | specifier `com.foo::Bar`, `opaque_namespace_use: true` — every static member of `Bar` in scope unqualified |
| `import com.foo.Bar;` used only in `extends`/`implements`/type positions | same as row 1 — Java has no `import type` keyword; whether a binding is type-only isn't visible at the import site, only at each reference site (`RefKind::TypeUse` there already carries that distinction) |

**§3.1 — static imports target a member, not a file.** `Resolution` (contracts §2) only ever
names a file or a dependency, never a member directly — so a static import's specifier encodes
*both* the class's package (for file resolution) and the class's own bare name (for the
member lookup that happens after), joined by a sentinel (`::`) the resolver splits back apart:
`com.foo::Bar` resolves the `com.foo` half exactly like row 1 (to the file declaring `Bar`),
and the importing file's binding table then looks up `CONST` as a **member** of `Bar` in that
file's declaration table — the same "resolve the file, then look up the member inside it"
two-step shape Go's package-qualified access already uses, just triggered by an import instead
of a body reference.

All Java imports are `ImportKind::Package` (there is no relative-path import shape — §0) and
`Confidence::Certain` (no bundler/build-tool ambiguity — an import that doesn't resolve is
either genuinely external or a compile error, never a maybe).

**Resolution algorithm** (the adapter's `resolve`):

1. **Same-package (no import needed).** Handled entirely by the core's existing `unit`-based
   fallback (contracts §2) — never touches this adapter's `resolve()` at all, exactly like Go's
   same-package resolution.
2. **`com.foo` / `com.foo::Bar`.** Split on `::` first if present (static-import shape, §3.1).
   Look up the package-half against the known-units index (every claimed file's declared
   `unit`, the same index the core already builds for `FileFacts::unit` fallback resolution) —
   a hit resolves `Resolution::File` at the package's **first file in path order** (a Java
   import names a package, and — same reasoning as Go's directory pick — any one file with
   that `unit` makes every file in it reachable through the same-unit resolution fallback, so
   which one is nominal doesn't affect correctness, only which file's `ImportsFile` edge is
   literal).
3. **`java.` / `javax.` prefix.** `Resolution::Stdlib`, unconditionally — a real, structural,
   version-stable namespace reservation (no third-party artifact may declare a `java.*`/
   `javax.*` package; the JDK itself enforces this), so no generated list is needed the way
   Go's arbitrary-string module paths require one.
4. **Anything else.** `Resolution::Unresolved` — deliberately **not** `Resolution::Dependency`
   (§0's last bullet: no reliable import→coordinate mapping exists, so guessing here would
   flood `undeclared` with false positives for the overwhelming majority of third-party code).
   This is the one point where this adapter's `resolve()` shape genuinely diverges from every
   other adapter's "undeclared fallback" pattern — documented here, not silently absent.

## 4. Manifests & packages (RFC 0011)

Two build ecosystems, handled at different fidelities — **Maven fully structured** (via
`roxmltree`, an XML tree parser — the `pom.xml` equivalent of `toml`'s role for `Cargo.toml`),
**Gradle best-effort** (line-oriented scanning, the same "no more machinery than can be done
honestly" stance `go.mod`'s hand-rolled parser takes, but Gradle's actual grammar — Groovy or
Kotlin — is a real programming language with arbitrary expressions, so this is a **narrower**
best-effort than go.mod's: only the common, literal-string forms are recognized, anything
computed is silently invisible, not misparsed).

**Maven (`pom.xml`)**:

| `package.json` concept | Maven equivalent | notes |
|---|---|---|
| `name` | `groupId:artifactId` (from `<groupId>`/`<artifactId>`, `<groupId>` falling back to `<parent><groupId>` when omitted — the common parent-inherits pattern) | the two-part coordinate IS the module's cross-module identity; `<version>` similarly falls back to `<parent><version>` |
| `private: true` | `<packaging>` ≠ `jar` (default) | Maven has no `private` flag at all — every `jar`-packaging module is nominally publishable by omission, matching npm's own asymmetric default; `pom` (aggregator, no code) and `war` (deployable app, not an importable dependency) are the two packagings this adapter treats as `private: true` |
| `workspaces` | `<modules>`/`<module>` (aggregator POM) | `ManifestFacts::workspace_members`, one entry per `<module>` text — RFC 0011 §3 |
| dependency scopes | `<dependency><scope>` | `compile`/omitted → `Prod`; `test` → `Dev`; `provided` → `Peer` (supplied by the runtime environment, not bundled — same "contract with the consumer" semantics as npm peerDependencies); `runtime` → `Prod` (a documented approximation — genuinely used at runtime, just not compile-visible; kndo's taxonomy has no runtime-only scope); `system` → `Prod` (rare, deprecated); entries inside `<dependencyManagement>` are version pins for *children*, not real dependencies of *this* module — never collected |
| `main`/`exports` | *(no equivalent)* | see §2's blanket `main()`-method stance; no manifest-level entry-class extraction attempted (a `<mainClass>` plugin config exists but resolving a dotted class name to a file needs a known-files-by-package lookup `ResolveCtx` doesn't cheaply expose today — parked, §7) |
| `scripts` | *(no equivalent)* | no script-runner convention in Maven itself |

**Gradle (`build.gradle`/`.kts`)**: line-scanned for the `dependencies { … }` block's literal-
string entries only (`implementation "com.foo:bar:1.0"`, `testImplementation("com.foo:bar:1.0")`,
version catalogs' `libs.foo` references and any computed/variable-interpolated coordinate are
invisible — not misparsed, simply not seen, same honesty as go.mod's `exclude`-globs-not-
expanded stance). Configuration → scope: `implementation`/`api`/`compile` (legacy) → `Prod`;
`testImplementation`/`testCompile`(legacy)/`testRuntimeOnly` → `Dev`; `compileOnly` → `Peer`
(provided-equivalent); `runtimeOnly`/`runtime`(legacy) → `Prod`; `annotationProcessor`/
`testAnnotationProcessor` → `Build` (a build-time-only tool, Cargo's build-dependencies
analogue — Lombok, MapStruct). `group`/`project name` (from `settings.gradle`'s own `rootProject.
name`/`include(...)`, defaulting to the containing directory's name per Gradle's own
convention when absent) give the module identity. The `application` plugin block
(`apply plugin: 'application'` or the `plugins { application }` DSL form) marks `private:
true`; its absence defaults to `private: false` (library mode), same asymmetric-default
reasoning as Maven's packaging check. **`settings.gradle`/`.kts`** is claimed purely for
`include(':sub-a')`/`include 'sub-a'` line-scanning → `workspace_members` (colon-to-slash
path convention; nested `:a:b` → `a/b`), the Gradle analogue of Maven's `<modules>`.

**Root promotion (RFC 0011 §5), the mechanism that diverges from both Go and Rust.** Go
blanket-emits `RawRoot`s per exported declaration directly in extraction, because its per-file
promotion signal (`internal/`) is itself extraction-visible. Rust rides the existing
`library_root_files` mechanism from a *single* manifest-declared entry file (`lib.rs`) plus the
library-surface fixpoint (phase 2.7) for its `pub mod` re-export chains. **Java has neither**:
"is this module publishable" is a manifest-level fact extraction can't see on its own (unlike
Go), and a Java library has no single entry file the way Rust's `lib.rs` does — every public
class in the module is independently part of the API. The adapter therefore emits one
`ManifestRoot{Production, target: <file>, Certain}` **per non-test `.java` file under the
module's source root** (`src/main/java` — the Standard Directory Layout default; a custom
`<sourceDirectory>`/`sourceSets` override is a stretch goal, §7) for every **publishable**
module, reusing `graph::assemble`'s existing per-file declaration-promotion path
(`library_root_files`) with **zero new core mechanism**: each of those files already being a
manifest-declared production root makes every `public` declaration in it promote automatically,
exactly as JS's `main`-file promotion already works — just applied to every source file instead
of one. `ManifestFacts.declares_surface` stays `false` (no `exports`-map equivalent — same
`deep-import`-stays-closed reasoning as Go).

## 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| Records (`record Point(int x, int y) {}`) | the type is a declaration; components/synthesized accessors are not (§2) — calls to `point.x()` produce no reference edge, a documented recall gap |
| Anonymous classes (`new Runnable() { … }`) | body walked for references (whatever it calls stays live); contributes no declaration of its own — no finding can ever target it |
| Method references (`Type::method`, `this::method`) | extracted as a `Call` reference with `scope_context` when the qualifier is a plain identifier/`this` — `Type::new` (constructor reference) treated the same, referencing `Type` itself |
| Static nested/inner classes, local classes | nested types are members (`member_of`); local classes (declared inside a method body) are not extracted at all, same non-declaration stance as anonymous classes |
| `equals`/`hashCode`/`toString`/`compareTo` overrides | covered by the blanket `@Override` dispatch root (§0, §2) — no special-casing beyond the annotation check |
| Generic bounds (`<T extends Comparable<T>>`) | the bound's type names are ordinary `TypeUse` references, ordering is irrelevant since phase 3a builds the whole file's table before phase 3b resolves anything (same forward-reference safety as every other adapter) |
| Checked/unchecked exceptions in `throws` | each named type is a `TypeUse` reference |
| Annotation processors / Lombok-generated members (`@Data`, `@Getter`, …) | not modeled — a generated `getFoo()` method has no textual declaration for extraction to see (same class of gap as record accessors); a *use* of it (`obj.getFoo()`) is a normal member-fallback reference that simply never resolves, harmlessly |
| Text blocks (`"""…"""`, Java 15+), switch expressions (`yield`), pattern matching (`instanceof Foo f`) | parsed by tree-sitter-java's grammar as ordinary expression/statement shapes; no adapter-specific handling needed — their contained references/type positions fall through the same generic walkers as everything else |
| `module-info.java` (JPMS) | claimed, yields zero declarations (§0's last bullet) — the `exports`/`requires`/`opens` module directives are not parsed; a real, parked gap (§7) |
| Non-standard source roots (no `src/main/java` — flat layouts, Bazel) | `unit` (declared package) still resolves correctly regardless of directory shape (§0); role-by-path (`src/test/java`) degrades to the Surefire-filename fallback (§1); manifest root-promotion (§4) specifically assumes the Standard Directory Layout and undercounts on a genuinely nonstandard one — documented, not silently wrong (fewer roots promoted, never phantom ones) |

## 6. Conformance fixtures (shared harness, RFC 0002 §8)

Four fixtures, each a real Maven/Gradle module tree run through the real `Engine` (no mock):

- **`dead-code-same-package`** — a private (`packaging=war`) module: `Main.main` calls
  `Helper.live()` with no import (same-package `unit` resolution) while `Helper.dead()` is
  never called — `dead` reads `unused`, `live` doesn't. (`Main` itself also reads `unused`: its
  class *name* is never referenced by anything, only invoked by the JVM — the same "a symbol's
  name and its callable body are different liveness questions" shape §2's `main()`-rooting
  note already flags; not a bug, a real fact about the code.)
- **`dispatch-and-cross-package`** — a private module: `Impl implements Greeter`, `Impl.go()`
  is `@Override`-annotated and never named-called (only reached via the interface-typed
  variable's *type*, `new Impl()`) — stays alive only through the dispatch-rooting rule (§0);
  `Greeter.go` (the interface's own abstract declaration) legitimately reads `unused` — nothing
  ever calls `g.go()` through the interface type in this fixture, only through dispatch on the
  concrete `Impl`. `Runner` imports `com.util.*` (wildcard) and calls `Helper.assist()`
  qualified — proves cross-*Java*-package visibility computation is correct (this fixture is
  what caught the `Unit`-vs-`Package` ladder bug below).
- **`visibility-ladder-and-nested-members`** — one package, all four ladder rungs exercised on
  a single class's methods plus a nested class's members (`member_of` two levels deep,
  `Inner.go`/`Inner.helper`): `private`/already-tightest `package-private` methods produce no
  finding, `protected`/`public` methods used only within their own `unit` both correctly
  downgrade-recommend to `package-private`, and the outer class itself downgrades too (never
  referenced from outside its own package in this fixture) — six `internal-only` findings in
  one file, each pinned to a distinct rung interaction.
- **`nested-type-qualifier`** — the two-bug shape a field audit on retrofit surfaced, in the
  smallest form that reproduces both, and it expects **zero findings**. `Handler.Query` is a
  nested type (a member, so absent from the file's bare-name table) whose constructor calls a
  private static on the enclosing class; `Main` constructs it as `new Handler.Query(…)` while
  also doing `import com.foo.http.Query`, binding that bare name to an unrelated annotation.
  Before the fixes both `Handler.Query` and `Handler.checkArgument` read `unused`: the
  constructor found no container to inherit liveness from, and the reference that should have
  named the nested type was captured by the import binding instead. Neither of the two existing
  nested-type fixtures caught it — `visibility-ladder-and-nested-members` has no constructor
  and no name collision — which is why it exists as its own case.
- **`maven-gradle-dependency-skip`** — a Maven module (`pom.xml`, dependency `com.other:lib`
  `1.0`) beside a Gradle module (`build.gradle`, same coordinate at `2.0`): `version-skew`
  fires (pure manifest-fact comparison, unaffected by `resolves_dependency_usage`); neither
  module's genuinely-unused `com.other:lib` declaration produces an `unused`/`test-only`
  dependency finding — verified end-to-end through the real engine (plus the diagnostic
  message), not just the unit-level `find_dependency_hygiene` test in `dependency_hygiene.rs`.

**A gap the fixtures surfaced, and how it closed.** A *wildcard* type import
(`import com.foo.*;`) used not to bind the target package's type names the way a plain
`import com.foo.Bar;` binds `Bar`, so a **bare, unqualified** reference to a wildcard-imported
type (`Helper` as a bare type, not through `Helper.member()`) did not resolve at all; the
`dispatch-and-cross-package` fixture passed only because its usage is the qualified-access
shape, which the duck-typed member fallback covers regardless of whether the type name itself
resolved. The core-side, post-extraction enumeration this was thought to need already existed:
`symbol_by_name_per_unit` is exactly "every top-level declaration of a unit, by name", and
`unit` for Java is the declared package. All the adapter was missing was
`module_names_visible` — the contract field Swift's `import SomeKit` already used to say the
same thing. Kotlin's `import p.*` had the identical gap and closed with the identical one-line
fact.

**No `undeclared`-dependency fixture, deliberately** — same shape as Go's own stance (§6 there),
for the different reason §0/§3 document: Java's `resolve()` never emits `Resolution::Dependency`
for an external import, so there is no code path that could produce one.

## 7. Open questions

1. `<mainClass>`/`exec.mainClass` manifest-declared entry points, resolved to a concrete file —
   parked; needs a known-files-by-declared-package-and-class lookup `ResolveCtx` doesn't cheaply
   expose today (§4).
2. Custom source roots (`<sourceDirectory>`, Gradle `sourceSets.main.java.srcDirs`) — root
   promotion (§4) assumes the Standard Directory Layout default; reading the override is a
   contained follow-up, not attempted in v1.
3. JPMS (`module-info.java`'s `exports`/`requires`/`opens`) — real Java 9+ module boundaries
   with their own visibility semantics, entirely unmodeled (§0, §5).
4. Gradle version catalogs (`libs.versions.toml` + `libs.foo` references in `build.gradle.kts`)
   — the coordinate lives in a *different* file than the `dependencies {}` block that uses it;
   the line-scan (§4) doesn't cross that boundary. A real, common pattern in modern Gradle
   projects worth a dedicated pass if Gradle precision turns out to matter more than the
   line-scan delivers.
5. Any package-prefix→Maven-coordinate mapping for dependency-usage resolution (§0's last
   bullet) — deliberately not attempted (a hand-maintained or generated database, `kndo-stdlib`-
   style, is the only sound path here; parked pending real signal that the skip's precision
   cost is worth the ongoing-maintenance cost of such a database).
6. ~~Wildcard type import name resolution~~ — **resolved.** `import com.foo.*;` now declares
   `module_names_visible`, and the core's bare-name fallback consults the target unit's table
   (§3, §5). The post-extraction enumeration this question assumed was missing turned out to be
   `symbol_by_name_per_unit`, which already existed.
