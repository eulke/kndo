# Adapter Spec — Kotlin

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M5
**Grammar:** tree-sitter-kotlin-ng

The fifth adapter, and the first that shares its manifest infrastructure wholesale with a
sibling adapter (`kndo-adapter-toolkit::jvm_manifest`, extracted from the Java adapter for this
purpose — ROADMAP "Java → Kotlin share infra"): a `pom.xml`/`build.gradle` describes dependency
coordinates identically whether the module's source is `.java` or `.kt`. What's genuinely new
here is the *source* language — Kotlin's visibility, member, and reachability shapes diverge
from Java's in ways that matter, not superficial syntax differences. Like Java, Kotlin has no
dogfood corpus of its own (kndo is written in Rust); precision rests on the conformance
fixtures (§6). Node kinds throughout this doc are pinned against real tree-sitter-kotlin-ng
1.1.0 output (`kndo-adapter-kotlin/src/parsing.rs`'s `#[ignore]`d ground-truth dumps), not
guessed.

## 0. What's structurally different from Java, and why it matters here

- **`package` carries zero visibility meaning — the mirror image of Java's bug-prone case.**
  Kotlin's default visibility (no modifier at all) is **`public`**, not package-private —
  unlike Java, where the *absence* of a modifier is itself the package-scoped default. A
  Kotlin `package` declaration exists purely for namespacing (avoiding name collisions,
  organizing wildcard imports) and plays no role in what code can see what. Consequently
  `FileFacts::unit` (still the declared dotted package name, same representation as Java's for
  infra-sharing — RFC 0012 §8) is used here **only for resolution** (same-package unqualified
  reference, wildcard-import target enumeration), **never for visibility bucketing** — there is
  no `Unit`-scoped rung anywhere in Kotlin's ladder (contrast Java, where package-private maps
  exactly to `Unit`, §0 there). Getting this backwards — reusing Java's Unit-for-package-scope
  reflex here — would be the *same class* of bug the Java adapter's own fixture caught, just
  inverted: it would fabricate an `internal-only` narrowing suggestion onto public declarations
  that Kotlin's compiler would flatly reject narrowing to (public *is* the declared level).
- **`internal` genuinely is `VisibilityScope::Package`, no widening needed.** Kotlin's
  `internal` means "visible within the same compilation module" (a Gradle module / Maven
  module in practice) — exactly kndo's `Package` scope (RFC 0011's `PackageId`, "same
  manifest"). This is the one rung Java has no equivalent of at all (Java's four rungs are
  File/Unit/Public/Public; Kotlin's are File/Package/Public/Public) — a real, structural
  difference between the two languages' visibility models, not a naming coincidence.
- **Four-rung ladder, `protected` widens to `Public` for the same reason as Java's.** Members
  (not top-level declarations — Kotlin's `protected` is illegal at top level, same restriction
  as Java) may be `protected`: visible to the declaring class **plus subclasses in any other
  module** — no scope in `{File, Package, Public}` represents "module ∪ subclasses-anywhere",
  so it widens to `Public`, trading recall for precision in the conservative direction (RFC
  0012 §6's normative rule). One ladder covers both top-level and member declarations: `[File
  "private", Package "internal", Public "protected", Public "public"]` — two rungs (2 and 3)
  sharing the `Public` scope is the same legal pattern Java's ladder already establishes.
  `private` on a top-level declaration is genuinely file-scoped (**exact** match to `File` —
  unlike Java, which disallows top-level `private` entirely); `private` on a **member** is
  class-scoped, narrower than anything kndo models, so it widens to `File` (over-approximating
  "reachable from elsewhere in the file" — same reasoning as Java's member-`private` widening).
- **`member_of` follows the RFC's stated rule literally: lexical nesting, not receiver syntax.**
  RFC 0012 §3's language-fit table says "Kotlin: class members; top-level functions have
  `None`" — this adapter applies that by lexical position alone. An **extension function**
  (`fun String.extFn() { … }`) is syntactically top-level (or a member, if nested inside a
  class) regardless of its receiver type; its `member_of` is `None` when declared at top level,
  matching every other top-level function, **not** `Some("String")`. This means a call site
  shaped like `someVar.extFn()` (identifier receiver) won't find `extFn` through the duck-typed
  member fallback (RFC 0012 §3) if nothing else names it directly — a known, documented
  precision gap (§5, §7), not a silent one; call sites shaped `"literal".extFn()` or any
  non-identifier receiver already fall through to whole-codebase by-name matching regardless of
  `member_of`, so the gap is narrower than it first appears.
- **Companion object members attribute to the *enclosing class*, not the companion itself.**
  `companion object Named { fun factory() }` inside `class Widget` is overwhelmingly called as
  `Widget.factory()` in real code — explicit `Widget.Named.factory()` addressing is rare even
  when the companion is named. Setting `member_of = "Widget"` (the outer class) rather than
  `"Named"` (or the compiler-synthesized default `"Companion"`) maximizes real-world duck-typed
  match rate — a pragmatic call, not a spec requirement, documented here so it doesn't read as
  an oversight. Regular `object Singleton { … }` (not a companion) uses its own name, exactly
  like a class would.
- **No inline test regions, same as Java.** Kotlin test code (JUnit or `kotlin.test`) is always
  a **separate file** under `src/test/kotlin/**` — the Kotlin Gradle plugin's own Standard
  Directory Layout, universally assumed by every build tool and IDE. `FileFacts::test_spans` is
  therefore always empty here, same stance as Java §0.
- **Dispatch rooting via the `override` *modifier*, not an annotation.** Kotlin requires
  `override` on every member that implements/overrides a supertype member (a compile error
  without it, unlike Java's optional `@Override`) — structurally the same "JDK/framework-
  invoked, never a named call site in user source" situation Java's `@Override` rooting and
  Rust's trait-impl-method rooting both address, just spelled as a `member_modifier` keyword
  (`(modifiers (member_modifier (override)))`) instead of an annotation node. Every `override`-
  modified member roots `Production`/`Probable`, unconditionally — same blanket, safe-direction
  stance as Java.
- **No reliable import→dependency-coordinate mapping — identical root cause to Java's.** Kotlin
  rides Maven/Gradle coordinates exactly like Java (it has no package manager of its own); a
  Kotlin `import com.foo.Bar` carries the same zero structural relationship to a Maven
  `groupId:artifactId` that a Java import does. Same consequence as Java §0:
  `resolve()` never returns `Resolution::Dependency` for an external import, and
  `AdapterDescriptor.resolves_dependency_usage: false` makes `dependency_hygiene` skip Kotlin's
  `unused`/`test-only` dependency verdicts (one diagnostic, not a flood). `version-skew` is
  unaffected (pure manifest comparison).
- **Kotlin Multiplatform (KMP) source sets are out of scope for v1** — same posture as RFC 0002
  §7's own table entry ("multiplatform source sets (post-1.0)"). This adapter targets the
  single-platform JVM layout (`src/main/kotlin`, `src/test/kotlin`) only; `commonMain`/
  `jvmMain`/`iosMain`-style source-set trees are not claimed specially — files under them are
  still claimed as ordinary `.kt` source (language detection doesn't care about the directory),
  but role classification (§1) and root promotion (§4) assume the single-platform layout and
  will undercount on a real KMP project. Parked, §7.
- **A narrow, verified upstream grammar bug: meta-annotated, parameterless `annotation class`
  declarations mis-parse.** `@Retention(...) annotation class Marker` (no primary constructor
  parens) parses under tree-sitter-kotlin-ng 1.1.0 as a bogus `infix_expression` chaining
  "annotation", "class", "Marker" as three identifiers — verified via a dedicated probe (§2);
  the moment the annotation class has ANY primary constructor (`(val x: Int)`) or the leading
  annotation is absent, it parses correctly. Rare in practice (most real custom annotations
  either take no meta-annotations or declare at least one parameter), and out of kndo's
  control — a third-party grammar issue, not an extraction bug. Falls through to "no
  declaration extracted for this specific shape," matching the "don't fabricate" degradation
  principle every other documented gap in this codebase follows.
- **A second, independently-verified grammar edge case: single-line bodies with content
  mis-parse.** `class Inner { fun m() {} }` written entirely on one line fails to parse as a
  `class_declaration`/`class_body` pair (verified via a dedicated probe, §2); the identical
  source reformatted across multiple lines parses cleanly. Real Kotlin style overwhelmingly
  uses multi-line bodies for anything but an empty declaration, so this is a narrow formatting
  artifact rather than a practical blocker — every fixture and hand-written extraction test in
  this adapter deliberately uses multi-line bodies because of it.

## 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `kotlin` | `**/*.kt` (`.kts` standalone scripts are **not** claimed — a real but rare pattern; Gradle's own `build.gradle.kts`/`settings.gradle.kts` are manifests, already claimed by both this adapter and Java's for that purpose, orthogonal to which language wrote the project's *source*) |
| Manifests | same as Java (`kndo-adapter-toolkit::jvm_manifest` — shared verbatim): `**/pom.xml`, `**/build.gradle` + `**/build.gradle.kts`, `**/settings.gradle` + `**/settings.gradle.kts` |
| Role `test` | `src/test/kotlin/**` (Kotlin Gradle plugin's Standard Directory Layout) OR a bare filename matching the same Surefire-style convention Java's fallback uses, extension-adjusted (`Test.kt`, `Tests.kt`, `TestCase.kt`) |
| Role `tooling` | not detected — same stance as Java/Go: no ecosystem-wide config-file convention for Kotlin source files exists |
| Origin `generated` | `@Generated`/`javax.annotation.Generated` annotations (same JVM-ecosystem convention Java's adapter detects — Kotlin code calling into the same annotation-processor tooling, KAPT/KSP, emits the identical marker), detected structurally by extraction, reported via `FileFacts::detected_origin` (RFC 0012 §7). The toolkit's text-marker scan runs as a second, independent signal |
| Origin `vendored` | `vendor/**`, `third_party/**` (toolkit universal list) — same inert-but-consistent inclusion as Java |

Build-output directories (`build/**`, `.gradle/**`) need no adapter-side exclusion — discovery
already respects the project's own `.gitignore`, same as every other adapter.

**`VisibilityLevel`**: `0` (private, widened to `File` at member scope / exact `File` at top
level), `1` (internal, `Package`), `2` (protected, widened to `Public`), `3` (public, `Public`
— also the default when no modifier is present at all). §0 has the full ladder derivation.

## 2. Extraction

**Declarations**: `class_declaration` is Kotlin's single unified node for class/interface/enum
class/data class/sealed class/annotation class/inner class — `SymbolKind` is derived from the
keyword leaf (`class` vs `interface`) plus any `class_modifier` (`enum` → `SymbolKind::Enum`,
`annotation` → `SymbolKind::Other("annotation")`; `data`/`sealed`/`inner`/`abstract` are
attributes of an ordinary `SymbolKind::Class`/`Interface`, not distinct kinds — matching how
Rust's own struct/enum attributes don't spawn new `SymbolKind` variants either). `enum_entry`
(inside `enum_class_body`) → `SymbolKind::EnumMember`, `member_of` the enum. `object_declaration`
(singleton) → `SymbolKind::Other("object")`, its own name used for `member_of` on its members.
`companion_object` is **not** independently declared as a symbol in v1 (its members attribute
to the enclosing class per §0 — the companion object node itself contributes no declaration of
its own, matching the "no name to hang a finding on" stance the Java adapter takes for
anonymous classes; a companion's own liveness is inseparable from its enclosing class's). `class_
parameter` marked `val`/`var` inside a `primary_constructor` is a real field (constructor-
promoted property) — extracted as `SymbolKind::Field`, `member_of` the class, same visibility
handling as any other member (a bare `class_parameter` with neither `val` nor `var` is a plain
constructor argument, not a declaration). `function_declaration` → `SymbolKind::Method` when
lexically nested in a `class_body`/`object_declaration`/`companion_object` body, else
`SymbolKind::Function` (§0's `member_of` rule). `property_declaration` → `SymbolKind::Field`
(member) or `SymbolKind::Variable` (top-level `val`/`var` — matching Go's own top-level-var
stance) — `const val` additionally carries no distinct kind (Kotlin's `const` restricts to
compile-time-constant primitives/`String`; not modeled as `SymbolKind::Const` to avoid a
member/top-level split that gains nothing analyses read). `secondary_constructor` →
`SymbolKind::Method` named `<init>` (colliding intentionally with the primary constructor's own
`<init>` name when both exist — a class's constructors are one liveness unit for kndo's
purposes, same coarse-graining Java's own multi-constructor handling already accepts).
`type_alias` → `SymbolKind::TypeAlias` (a construct Java has no equivalent of — a clean fit,
unlike Java's need for `Other("record")`). `anonymous_initializer` (`init { … }` blocks) is not
independently declared — its body is walked as part of the enclosing class's construction-time
liveness, matching RFC 0012 §4's stated rule ("init blocks/constructors → the class").

**References**: `call_expression` with an `identifier` callee → bare `Call`; a
`navigation_expression` callee (`a.b.c()`) → `Call` on the last segment with `scope_context` set
to the **second-to-last** segment's text when that segment is a plain `identifier` (mirroring
Java's `object`/`field` qualifier extraction), else `scope_context: Some("<expr>")` and the
receiver chain is walked recursively for its own references — this is what makes
`HasCompanion.Named.factory()` resolve `factory` with `scope_context: Some("Named")` while
`Singleton.x` resolves `x` with `scope_context: Some("Singleton")`. `user_type` positions
(parameter/return/property types, `is`/`as` checks, generic bounds) → `TypeUse`, using the
type's bare `identifier` (or the last segment when the reference names a qualified type).
`delegation_specifier` entries (`class C : Base(), Interface1` — both the superclass
constructor-invocation shape and a bare interface name) → `Extend`, matching Java's
superclass+implements handling. The superclass invocation's **argument list** is walked as an
ordinary expression on top of that `Extend`: `class MyMeta : Base(MyProvider)` references
`MyProvider`, and dropping it left Exposed's `PostgreSQLTypeProvider` — passed to its
superclass on the very next declaration in the same file — with no incoming reference at all.
A primary constructor parameter's **default value** is walked for the same reason
(`class Hasher(val cost: Int = DEFAULT_COST)` references `DEFAULT_COST`); only the parameter's
type used to be. Both attribute `within` to the owning class, per RFC 0012 §4's rule that code
running on instantiation belongs to the type. Pinned by the `ctor-arg-and-default-value`
fixture, whose declarations are deliberately `internal`/`private` — public ones are library
roots and stay alive without any reference, so a public version of the fixture passes even
with the extraction gap reintroduced. Lambda bodies (`lambda_literal`) and `when_expression`/
`when_entry` bodies are walked like any other expression — same safe-direction over-
approximation as every other adapter's closure handling.

**Roots (`RawRoot`)**: a top-level `fun main()` (0 or 1 parameter — the canonical Kotlin JVM
entry point; the `@JvmStatic fun main()`-inside-`object` variant used for some build-tool
interop is not special-cased, a documented non-goal §7) → `RootKind::Production` at `Probable`,
unconditional, mirroring Java/Go/Rust's blanket `main`-rooting stance. `override`-modified
members root `Production`/`Probable` (§0's dispatch rule).

**Suppressions**: `// kndo:allow …` / `/* kndo:allow … */` — identical syntax to every other
adapter (RFC 0005 §12 is language-neutral).

**Metrics**: cyclomatic complexity +1 per `if_expression`, `when_entry` (one per arm past the
implicit base, matching JS/Java's n-way-match rule), `for_statement`, `while_statement`,
`try_expression` (one per try, not per `catch_block` — matching Java's stance of not over-
counting nested clauses), `&&`/`||` (leaf tokens inside `binary_expression`, exactly like
Java's). The elvis operator (`?:`) and the not-null assertion (`!!`) are **not** counted as
branches — `?:` is a value-producing fallback expression, not a control-flow fork the way
`if`/`when` are (same reasoning JS's optional-chaining `?.` isn't counted either); this keeps
the metric consistent across adapters rather than inventing a Kotlin-specific bump. Each `lambda_literal` or `anonymous_function` clearing the clone floor becomes its own callable **shape**
(`MetricsSyntax::nested_callable_kinds` — its branches and tokens leave the enclosing shape's
stream, which keeps one `FN` in their place, and `crap`/`duplicate` report it in its own
right). A smaller one stays an expression inside its owner: promoting it would leave both
halves under the floor and cost real clone findings — measured, that was 83 clone participants
on the field corpus. The split's semantics are uniform across adapters; only the node kinds
that trigger it are per-language.

`MetricsSyntax::construction_kinds` is deliberately **empty** here: constructing a value in
Kotlin is an ordinary `call_expression`, indistinguishable from any other call, so this adapter
has nothing true to report and `duplicate`'s construction exemption simply never fires for
Kotlin — today's behaviour, unchanged. Guessing (an uppercase callee, say) would be the adapter
inventing a verdict, in the accusation direction RFC 0012 §2 forbids.

**Properties**: a property with an accessor BODY is a callable, not a value — `Method` when it
has an owner, `Function` at top level; a stored one stays `Field`/`Variable`. Kotlin compiles such
a property to a getter, so this is the truthful kind, and the distinguishing node is a `getter`/`setter` with a `function_body`.
A bodyless accessor (a `private set`, an annotated bare `get`)
leaves the property stored: it changes the accessor, not what the property IS.

Naming both `Field` made the kind unable to separate a constant from real logic, which is what
let `untested` accuse header-name constants and `MAX_VARCHAR_LENGTH` of not being tested.

**Grammar ground truth**: pinned in `kndo-adapter-kotlin/src/parsing.rs`'s `#[ignore]`d probe
tests, covering declarations/modifiers/visibility, imports, `when`/`if`/`for`/`while`/`try`,
lambdas/string templates/elvis/not-null, companion objects/secondary constructors/inner
classes, and the annotation-class grammar edge case (§0's last bullet) — re-run with
`--ignored --nocapture` before any tree-sitter-kotlin-ng version bump.

## 3. Imports & resolution

Emitted import kinds:

| Form | Emission |
|------|----------|
| `import com.foo.Bar` | specifier `com.foo`, binding `[Bar]` — same package/type split as Java, free from the grammar's own `qualified_identifier` nesting |
| `import com.foo.Bar as Alias` | specifier `com.foo`, binding `[{local: Alias, imported: Bar}]` — Kotlin's own import-aliasing syntax (Java has none); the binding's `local`/`imported` split already exists in the contract for exactly this shape |
| annotations on a declaration | `Declaration::markers`, as written and in source order — `@Repository` (`annotation > user_type`) and `@Named("x")` (`annotation > constructor_invocation > user_type`) alike, plus the last segment of a qualified spelling. Same facts-not-verdicts contract as Java's, same consumer (`[[externally-invoked]]`) |
| `import com.foo.*` | specifier `com.foo`, no bindings, **both** `opaque_namespace_use: true` (the wildcard over the target's exports — keeps it alive without naming what it took) and `module_names_visible: true` (the language's scoping rule: every top-level name of that package is legal here *unqualified*, so the core's bare-name fallback consults that unit's table). Emitting only the first meant a bare call to a wildcard-imported top-level function resolved to nothing at all — kotlinx.coroutines calls `recoverStackTrace(…)` that way from dozens of files in other packages, and every declaration of it read `unused`; landing the second took the repo from 3454 findings to 2909 and 69.8 C to 79.3 C |
| `import com.foo::Bar` sentinel shape | **not applicable** — Kotlin has no `import static`; a top-level `const val`/function is imported the same way a class is (`import com.foo.CONST`), row 1 already covers it |

`ImportKind::Package` throughout (no relative-path import shape), `Confidence::Certain` (an
import that doesn't resolve is either genuinely external or a compile error, never a maybe —
same reasoning as Java).

**Resolution algorithm** (the adapter's `resolve`):

1. **Same-package (no import needed).** Handled entirely by the core's `unit`-based fallback —
   never reaches this adapter's `resolve()`, identical to Java.
2. **`kotlin.`/`kotlin/` prefix, and `java.`/`javax.` prefix.** `Resolution::Stdlib` — Kotlin's
   own standard library (`kotlin.collections.*`, `kotlin.io.*`, …) is as structurally reserved
   as `java.*`/`javax.*` (no third-party artifact may declare a `kotlin.*` package), and every
   Kotlin/JVM project transitively depends on the full Java standard library too, so both
   prefixes resolve the same way.
3. **`com.foo` (any other package).** Look up against the known-units index — a hit resolves
   `Resolution::File` at the package's first file in path order, identical algorithm to Java's.
4. **Anything else.** `Resolution::Unresolved` — never `Resolution::Dependency`, same §0
   reasoning as Java's last bullet.

## 4. Manifests & packages (RFC 0011)

Fully delegated to `kndo_adapter_toolkit::jvm_manifest` (extracted from the Java adapter for
this purpose) with a Kotlin-specific `JvmSourceLayout`:

| Layout field | Java | Kotlin |
|---|---|---|
| `source_root` | `src/main/java` | `src/main/kotlin` |
| `source_ext` | `.java` | `.kt` |
| `skip_file_names` | `module-info.java`, `package-info.java` | `[]` (Kotlin has neither convention) |

Every Maven/Gradle fidelity detail (roxmltree-structured `pom.xml`, line-scanned
`build.gradle`/`.kts`, dependency-scope tables, `<dependencyManagement>` exclusion, the
`application`-plugin private-mode heuristic, `settings.gradle` topology) is byte-for-byte the
same code path Java uses — see docs/adapters/java.md §4 for the full description; nothing here
diverges except the two layout fields above. The shared `GRADLE_CONFIG_SCOPES` table also
recognizes `kapt`/`kaptTest` (Kotlin's KAPT annotation-processing configurations, → `Build`
scope, the Kotlin analogue of `annotationProcessor`) — inert dead data for the Java adapter,
which never encounters a `kapt` line in its own projects, but exercised for real here.

**Root promotion**: identical mechanism to Java's (§4 there) — one `ManifestRoot{Production,
Certain}` per non-test `.kt` file under `src/main/kotlin/**`, for every publishable module,
reusing the existing per-file declaration-promotion path with zero new core mechanism.
Projects that place Kotlin source under `src/main/java` (a non-standard but technically
supported Kotlin Gradle plugin configuration, since the plugin accepts both roots
simultaneously) are **not** promoted from that second root in v1 — a narrow, documented non-
goal (§7); such files still parse, extract, and resolve correctly, they simply don't get the
manifest-driven root-promotion boost a `src/main/kotlin`-housed file gets.

## 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| Extension functions called via an identifier receiver (`someVar.extFn()`) | not resolved through the duck-typed member fallback — `member_of` is `None` for top-level declarations regardless of extension receiver syntax (§0); a documented recall gap, narrower in practice than it sounds since non-identifier receivers already fall through to whole-codebase by-name matching |
| Companion object addressing | members attribute to the enclosing class (§0) — `Widget.Named.factory()`'s fully-qualified form still resolves the CALL correctly (last-segment-name matching doesn't care which qualifier text preceded it), it's the *declaration's* `member_of` choice that's pragmatic, not exact |
| Data class synthesized members (`copy()`, `component1()`, `equals`/`hashCode`/`toString`) | not modeled — same class of gap as Java's record-accessor stance; a call to `point.copy()` produces a reference that simply never resolves, harmlessly |
| Delegated properties (`val x by lazy { … }`) | the delegate expression (`lazy { … }`) is walked for its own references like any other property initializer; the `by`-delegation protocol itself (`getValue`/`setValue` dispatch) is not modeled — same non-goal class as Java's reflection stance |
| Smart-cast / `is`/`as` type checks | the checked type is an ordinary `TypeUse` reference; the compiler-level flow-sensitive narrowing itself has no representation in kndo's model (nor does any other adapter's) |
| Meta-annotated, parameterless `annotation class` | not extracted — a verified upstream grammar limitation, not an extraction bug (§0's last bullet) |
| Kotlin Multiplatform source sets | files still claimed/extracted as ordinary `.kt` source; role classification and root promotion assume single-platform JVM layout and undercount on a real KMP tree (§0, §7) |
| `expect` / `actual` declarations | ONE logical declaration with several bodies, and the unit key (the declared package name) is the same for all of them — so they are exactly the core's same-unit *twins*, and a reference to the name edges to every one of them. Nothing Kotlin-specific in the core: the same machinery covers Go's mutually exclusive build-tag files and Rust's `#[cfg]` alternates (RFC 0012 §8). Before it, the `expect` took every call and its `actual`s read `unused`, or the reverse |
| `@JvmStatic`/`@JvmName`/`@JvmOverloads` JVM-interop annotations | not modeled specially — these affect bytecode-level dispatch shape (extra overloads, static vs. instance methods) that has no bearing on kndo's textual liveness model; a call site written in Kotlin always looks like an ordinary Kotlin call regardless of what the annotation generates for Java callers |

## 6. Conformance fixtures (shared harness, RFC 0002 §8)

Four fixtures, each a real Maven/Gradle module tree run through the real `Engine` (no mock),
directly mirroring the Java adapter's fixture set so the two adapters' precision is comparable
apples-to-apples:

- **`dead-code-same-package`** — a private (`packaging=war`) module: `main()` calls
  `Helper.live()` with no import (same-package `unit` resolution) while `Helper.dead()` is never
  called — `dead` reads `unused`, `live` doesn't.
- **`dispatch-and-cross-package`** — a private module: `Impl` implements `Greeter` via
  `override fun go()`, never named-called directly (only reached through the interface-typed
  variable's type) — stays alive only through dispatch rooting (§0); `Greeter.go` (the
  interface's own abstract declaration) legitimately reads `unused`. `Runner` imports
  `com.util.*` (wildcard) and calls `Helper.assist()` qualified — proves cross-package
  visibility computation is correct, the same shape that caught Java's `Unit`-vs-`Package` bug,
  here verified against Kotlin's *different* (correct-by-construction, §0) ladder mapping.
- **`visibility-ladder-and-internal`** — one package, all four ladder rungs exercised
  (`private`/`internal`/`protected`/`public`) on a class's members plus a companion object's
  members (attributed to the outer class per §0) and a nested class's members: `internal`
  methods used only within the module correctly stay at their declared level (no finding —
  already tightest), `protected`/`public` methods used only within the same module downgrade-
  recommend to `internal`, proving `Package`-scope (not `Unit`) is what `internal`'s tightest-
  sufficient check actually computes against.
- **`maven-gradle-dependency-skip`** — a Maven module beside a Gradle module (`kapt`
  configuration included, proving the shared scope table's Kotlin-specific entries work) at
  skewed versions of the same coordinate: `version-skew` fires, neither module's genuinely-
  unused dependency produces an `unused`/`test-only` finding.

**Gaps the fixtures do NOT re-verify** (already covered by Java's fixtures against the shared
`jvm_manifest` module, no value in duplicating): Maven `<dependencyManagement>` exclusion,
`<modules>`/`settings.gradle` workspace topology, the `application`-plugin private-mode
heuristic. Kotlin's fixtures focus on what's genuinely different — the ladder, dispatch
rooting via `override`, and companion-object `member_of` attribution.

## 7. Open questions

1. Extension-function call resolution via an identifier receiver (§0, §5) — would need
   `member_of` to somehow carry the receiver type without breaking the RFC's "top-level
   functions have `None`" rule, or a distinct resolution path recognizing extension-function
   declarations as a special member-like category. Not attempted in v1; parked pending real
   signal this recall gap matters in practice.
2. Kotlin Multiplatform source-set-aware role classification and root promotion (§0, §5) —
   RFC 0002 §7 already scopes this to post-1.0; unchanged here.
3. `src/main/java`-housed Kotlin-adjacent source roots (mixed Java+Kotlin modules using the
   non-default layout) — root promotion assumes `src/main/kotlin` only (§4); a contained
   follow-up, not attempted in v1.
4. `@JvmStatic fun main()` inside a top-level `object` (an alternate, less common Kotlin JVM
   entry-point shape) — not rooted; the plain top-level `fun main()` shape (§2) is the
   overwhelming convention.
5. Companion object's own liveness as a distinct symbol (§2) — currently inseparable from its
   enclosing class; if a companion object contains dead code alongside live top-level-class
   members, per-member `unused` findings still fire correctly (since members are individually
   tracked), but there's no way to ask "is this whole companion object dead" as its own
   question. Low priority — the enclosing class almost always shares the companion's fate.
6. Gradle version catalogs (`libs.versions.toml`) — identical gap to Java's §7 point 4, inherited
   unchanged via the shared `jvm_manifest` module.
