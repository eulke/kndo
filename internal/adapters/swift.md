# Adapter Spec — Swift

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M5
**Grammar:** tree-sitter-swift

The sixth adapter, and the first whose manifest (`Package.swift`) is not a data format at
all — it's Swift *source code*, executed by `swift-tools-version`-selected SwiftPM at build
time. This adapter never executes it; `manifest.rs` parses it with the **same tree-sitter-swift
grammar** extraction uses, walking the `Package(...)` initializer call's labeled arguments as
data — the SPM analogue of Rust's structured `Cargo.toml` parse, not Gradle's line-scan (there's
no "compute anything" concern here the way Groovy/Kotlin DSL raises for Gradle, since only the
literal argument shapes SwiftPM itself requires are read; anything genuinely dynamic in a real
`Package.swift` — which is rare, SwiftPM manifests are conventionally declarative — is invisible,
not misparsed, same honesty as every other best-effort manifest reader in this codebase). Like
Java/Kotlin, no dogfood corpus of its own; precision rests on the conformance fixtures (§6).
Node kinds throughout this doc are pinned against real tree-sitter-swift 0.7.3 output
(`kndo-adapter-swift/src/parsing.rs`'s `#[ignore]`d ground-truth dumps), not guessed.

## 0. What's structurally different from Java/Kotlin, and why it matters here

- **The resolution unit is the SPM *target*, not the file's own declared namespace.** Swift has
  no `package`/namespace statement at all — every file in a target implicitly shares that
  target's single flat namespace (any file can reference any other file's `internal`+
  declarations in the same target without an import). `FileFacts::unit` is therefore **not**
  declared in source the way Java/Kotlin's is; it's derived from the SwiftPM Standard Directory
  Layout: the path segment immediately following `Sources/` or `Tests/` (`Sources/MyLib/Deep/
  File.swift` → unit `MyLib`). A file outside both conventions (rare — a loose top-level script,
  a nonstandard layout) gets `unit: None`, same safe-direction fallback every other adapter uses
  for "can't place this file's identity." RFC 0012 §8 already records this convention.
- **A five-rung visibility ladder that applies uniformly to top-level AND member declarations —
  no restricted top-level subset, unlike Java/Kotlin.** Swift permits `private`/`fileprivate`/
  `internal`/`public`/`open` on *any* declaration, top-level or member (Java disallows top-level
  `private`/`protected`; Kotlin disallows top-level `protected`). One ladder, no per-position
  carve-out: `[File "private", File "fileprivate", Package "internal", Public "public", Public
  "open"]` (RFC 0012 §6, already recorded). `private` is real-Swift narrower than kndo's `File`
  scope (it's scoped to the enclosing declaration/extension, not the whole file) — no scope in
  `{File, Unit, Package, Public}` represents that, so it widens to `File`, same conservative-
  widening reasoning as every other adapter's tightest-unavailable-scope case. `fileprivate` is
  an **exact** match to `File`. `internal` — Swift's **default when no modifier is written at
  all** — maps to `Package` (kndo's "same manifest" granularity, here an SPM target): a real,
  structural difference from Java (default = `Unit`-equivalent) and Kotlin (default = `Public`)
  — the third distinct default-visibility convention among this session's three adapters, each
  requiring its own careful mapping rather than a shared reflex. `public`/`open` both widen to
  `Public` (the difference — `open` alone permits cross-module subclassing/overriding — has no
  representation in kndo's four-scope model, same "can't distinguish, so don't try" stance
  Java's `protected`-widens-to-`Public` already establishes).
- **`member_of` covers `extension` too — the one member-owner shape no other adapter has.** RFC
  0012 §3 already records this: "members of struct/class/enum/protocol/**extension** (owner =
  extended type's name)". An `extension Widget { func extra() {} }` block adds `extra` as a
  genuine member of `Widget` even though it's declared in a completely different file (possibly
  a different target, extending a type from an imported module) — extraction attributes
  `member_of` to the extended type's bare name exactly the same way a same-file member would,
  no special-casing needed once the grammar's `class_declaration{declaration_kind: extension}`
  shape is recognized (§1). `struct`/`class`/`enum`/`extension` share **one** tree-sitter node
  kind (`class_declaration`, distinguished by its `declaration_kind` field) — protocols alone
  get a distinct `protocol_declaration` node.
- **No reliable import→dependency-coordinate mapping — a different root cause from Java's, same
  outcome.** Unlike Java (whose import namespace has *zero* structural relationship to a Maven
  coordinate), a Swift `import Alamofire` names a **module**, and SwiftPM module names usually
  *do* match their declaring target/product name — but `Package.swift` only declares the
  dependency's **repository URL** (`.package(url: "https://github.com/Alamofire/Alamofire.git",
  from: "5.0.0")`), never the module/product name(s) that repository exports; those live in
  *that* repository's own `Package.swift`, which kndo — a static source analyzer with no
  network access — structurally never reads. Same consequence as Java (§0 there):
  `resolve()` never returns `Resolution::Dependency` for an external import, and
  `AdapterDescriptor.resolves_dependency_usage: false` makes `dependency_hygiene` skip Swift's
  `unused`/`test-only` dependency verdicts (one diagnostic, not a flood). **Local target-to-
  target imports are a different, fully-solved question** — `import MyLibCore` from a sibling
  target in the *same* `Package.swift` resolves exactly like Java's same-`unit` fallback, since
  SwiftPM module names are declared LOCALLY as target names right there in the manifest being
  read (§3). `version-skew` is unaffected (pure manifest comparison, no usage edge needed).
- **Root promotion has two independent triggers, neither manifest-`private`-gated the way
  Java/Kotlin's is.** `@main`-attributed types (the modern SE-0281 program-entry-point
  attribute) root unconditionally wherever they appear. A file named exactly `main.swift`
  (SwiftPM's older, still-supported convention: the **one** file in an executable target
  permitted to hold unwrapped top-level statements) roots as a **whole file** — its top-level
  code runs unconditionally at process start, the same "load-time, no separate declaration to
  hang a root on" shape Rust's `fn main` XOR JS's entry-file promotion each handle differently;
  here it's neither a single function nor an arbitrary manifest-declared entry, it's a filename
  convention SwiftPM itself enforces (at most one `main.swift` per executable target). Library-
  target root promotion (every `.swift` file under a **publicly exported** target's `Sources/`
  tree) is manifest-driven, same mechanism as Java/Kotlin (§4) — but scoped **per target**, not
  per whole package, since one `Package.swift` can declare several targets with different
  public/internal-only status via which ones appear in a `.library(...)` product's `targets:`
  list.
- **`override` dispatch rooting, same shape as Kotlin's.** A member marked `override` — the
  Swift keyword, structurally identical to Kotlin's `member_modifier` wrapper shape — implements
  a superclass/protocol-required method that framework/OS code (UIKit/SwiftUI lifecycle hooks,
  protocol-witness dispatch) may invoke without any named call site in this codebase. Every
  `override`-modified member roots `Production`/`Probable`, unconditionally, same blanket stance
  as Java's `@Override` and Kotlin's `override`.
- **No inline test regions — `Tests/<Target>/**` is the authoritative signal**, same Standard-
  Directory-Layout stance as every JVM-family adapter this session (RFC 0002 §7 already records
  this). XCTest's own naming convention (`*Tests.swift`, mirroring `class FooTests: XCTestCase`)
  is a belt-and-suspenders filename fallback for non-standard layouts, same shape as Java's
  Surefire fallback.

## 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `swift` | `**/*.swift` |
| Manifests | `**/Package.swift` only — SwiftPM has no secondary/workspace-topology manifest file the way `settings.gradle` is to `build.gradle` (a package's `targets:` array already *is* its full local topology, §4) |
| Role `test` | `Tests/**` (SwiftPM Standard Directory Layout) OR a bare filename matching XCTest's own convention (`*Tests.swift`) — an OR, not additive, same reasoning as every other adapter's dual-signal role check |
| Role `tooling` | not detected — no ecosystem-wide config-file convention for Swift source files exists (same stance as every prior adapter) |
| Origin `generated` | `// Generated by` / `// This file was generated` banner-style `Contains` markers (sourcery, swiftgen, and similar codegen tools all wrap the marker in an ordinary `//` comment) — `comment_openers: ["//", "/*", "*"]`, the same C-family set every prior `Contains`-based adapter declares (contracts §7, and the toolkit-level self-reference fix this session's Kotlin work landed) |
| Origin `vendored` | `vendor/**`, `third_party/**` (toolkit universal list) — inert but consistent, same as every JVM-family adapter; SwiftPM's own dependency cache (`.build/checkouts/**`) is excluded from discovery already (a project's own `.gitignore` universally excludes `.build/`, same "OUT_DIR" stance every adapter's doc states for its own build directory) |

**`VisibilityLevel`**: `0` (private, widened to `File`), `1` (fileprivate, exact `File`), `2`
(internal — the default when no modifier is present at all — `Package`), `3` (public, `Public`),
`4` (open, `Public`). §0 has the full ladder derivation; unlike Java/Kotlin, every level applies
at every declaration position (top-level and member alike), so there is no restricted-subset
note to make here.

## 2. Extraction

**Declarations**: `class_declaration` is the single unified node for struct/class/enum/
extension — `SymbolKind` derives from the `declaration_kind` field (`struct`/`class` →
`SymbolKind::Struct`/`Class` respectively; `enum` → `SymbolKind::Enum`; `extension` contributes
**no declaration of its own** — same "no name to hang a finding on" stance as Java's anonymous
classes and Kotlin's companion objects, since an extension block's own identity isn't a
meaningful liveness question, only its *members'* are). `protocol_declaration` →
`SymbolKind::Interface` (Swift's protocol is the interface-shaped construct in every other
adapter's vocabulary; kndo has no distinct "protocol" facet, and `Interface`'s semantics — a
contract type, never instantiated directly — line up exactly). `enum_entry` (inside
`enum_class_body`) → `SymbolKind::EnumMember`. `function_declaration` → `SymbolKind::Method`
when lexically nested in a `class_body`/`protocol_body`, else `SymbolKind::Function`. A
protocol's method requirement is its own distinct node kind, `protocol_function_declaration`
(no `function_body` field at all — a requirement has a signature, never an implementation) —
dispatched through the same handler, which already treats the body as optional.
`init_declaration` → `SymbolKind::Method` named `<init>` (constructor overloads collapse to one
liveness unit, same coarse-graining as Java/Kotlin's multi-constructor stance).
`property_declaration` → `SymbolKind::Field` (member) or `SymbolKind::Variable` (top-level —
matching Go's top-level-var stance); a `let a, b: Int` multi-binding pattern is walked per
`pattern` child, one `Declaration` each (matching Go's grouped-`var` and Java's grouped-field
stance: one physical binding, one declaration). `typealias_declaration` →
`SymbolKind::TypeAlias`. Computed properties (`var x: Int { get { … } set { … } }`) and property
observers (`willSet`/`didSet`) are not independently declared — their `computed_getter`/
`computed_setter`/`willset_didset_block` bodies are walked as part of the OWNING property's own
declaration span, matching the "one physical declaration, one liveness unit" principle (a
computed property's getter/setter aren't separately callable by name from user source; treating
them as sub-bodies of the property avoids inventing declarations nothing can reference by name).

**References**: `call_expression` with a `simple_identifier` callee → bare `Call`; a
`navigation_expression` callee (`a.b.c()`) → `Call` on the terminal `navigation_suffix`'s name
with `scope_context` set to the qualifier when it's a plain `simple_identifier`/`self_expression`
(`self` → literal `"self"`, mirroring Java's `this`), else the qualifier chain is walked
recursively for its own references — the same qualifier-vs-complex-receiver split Kotlin's
`emit_navigation_ref` already establishes, reused near-verbatim here since the grammar shapes
line up closely (`navigation_expression{target, suffix: navigation_suffix{suffix}}` vs Kotlin's
flatter `navigation_expression` child list — the field names do the work here instead of
positional last-two-children logic). `inheritance_specifier`'s `inherits_from: user_type` →
`Extend` (superclass and protocol conformance share one syntax list in Swift, so no
extends-vs-implements split is needed the way Java's grammar forces). `user_type`/
`optional_type` positions (parameter/return/property types, `is`/`as` checks, generic
constraints) → `TypeUse`. Closures (`lambda_literal`) are walked like any other expression body
— their own parameter list shadows outer bindings the same safe-direction way every adapter's
closure handling already works.

**Top-level code**: `source_file`'s children aren't only declarations — any statement can sit
directly at file scope (SwiftPM's "top-level code" file shape, §0), so a bare `helper.live()`
next to a top-level `let` is walked as body content exactly like a function body, or it would
leave no reference behind at all. **`within` for top-level globals** (RFC 0012 §4): a `let`/`var`
declared at file scope is *always lazily-initialized* by the Swift language itself (its
initializer, computed getter, and observers only run on first access/each access, never at
"module load") — so `within` names the global's own symbol, not `None`. `main.swift` is the one
exception: its top-level code is a script, executing procedurally at process start, so references
there keep the ordinary `None` ("runs when the file loads") attribution.

**Roots (`RawRoot`)**: `@main`-attributed declarations → `RootKind::Production` at `Certain`
(structural, not a heuristic the way `main()`-by-name is for every other adapter — the attribute
is SwiftPM/the Swift compiler's own authoritative entry-point marker, so this is the one adapter
whose entry-point root reaches `Certain` rather than `Probable`). A file named exactly
`main.swift` → `RootKind::Production` at `Certain`, `RawRootTarget::WholeFile` (§0's "top-level
code, not a single function" shape). `override`-modified members → `Production`/`Probable`
(§0's dispatch rule).

**Suppressions**: `// kndo:allow …` / `/* kndo:allow … */` — identical syntax to every other
adapter (RFC 0005 §12 is language-neutral).

**Metrics**: cyclomatic complexity +1 per `if_statement`, `guard_statement` (an implicit early-
return conditional — counted, same reasoning `if` is), `switch_entry` (one per case group past
the implicit base, matching every other adapter's n-way-match rule — `default` is itself one
more ordinary `switch_entry`, no special-casing), `for_statement`, `while_statement`,
`catch_block` (one per `catch` clause, **not** `do_statement` itself — matching Java's own
try/catch convention exactly: entering a `do` block always happens, only a `catch` clause is a
genuine alternate path), `&&`/`||` (leaf tokens, same shape as Java/Kotlin's). Nil-coalescing
(`??`) and force-unwrap (`!`) are **not** branches — value-producing fallback/assertion
operators, not control-flow forks, same non-branch stance as Kotlin's elvis/not-null operators.
Each `lambda_literal` clearing the clone floor becomes its own callable **shape**
(`MetricsSyntax::nested_callable_kinds` — its branches and tokens leave the enclosing shape's
stream, which keeps one `FN` in their place, and `crap`/`duplicate` report it in its own
right). A smaller one stays an expression inside its owner: promoting it would leave both
halves under the floor and cost real clone findings — measured, that was 83 clone participants
on the field corpus. The split's semantics are uniform across adapters; only the node kinds
that trigger it are per-language.

`MetricsSyntax::construction_kinds` is deliberately **empty** here: constructing a value in
Swift is an ordinary `call_expression`, indistinguishable from any other call, so this adapter
has nothing true to report and `duplicate`'s construction exemption simply never fires for
Swift — today's behaviour, unchanged. Guessing (an uppercase callee, say) would be the adapter
inventing a verdict, in the accusation direction RFC 0012 §2 forbids.

**Properties**: a property with an accessor BODY is a callable, not a value — `Method` when it
has an owner, `Function` at top level; a stored one stays `Field`/`Variable`. Swift compiles such
a property to a getter, so this is the truthful kind, and the distinguishing node is `computed_property`.
A bodyless accessor (a `private set`, an annotated bare `get`, `willSet`/`didSet` observers)
leaves the property stored: it changes the accessor, not what the property IS.

Being a callable, it also gets a **shape**: `FileFacts::functions` carries one entry per accessor
body, so `crap` and `duplicate` can see a getter the way they see a method. One symbol, one
numbering — the first accessor in source order is `shape_ordinal` 0 and the rest continue, which
is what keeps `get` and `set` from colliding on a nested shape's identity. Verified against
tree-sitter-swift 0.7.3's `node-types.json`: a `computed_property` holds either a bare
`statements` (the implicit-getter shorthand `var x: Int { 1 + 2 }`) or one
`computed_getter`/`computed_setter`/`computed_modify` each with its own. `willSet`/`didSet` get
no shape: they run *around* a store, so the property is still stored and there is no getter to
measure.


Naming both `Field` made the kind unable to separate a constant from real logic, which is what
let `untested` accuse header-name constants and `MAX_VARCHAR_LENGTH` of not being tested.

**Grammar ground truth**: pinned in `kndo-adapter-swift/src/parsing.rs`'s `#[ignore]`d probe
tests — re-run with `--ignored --nocapture` before any tree-sitter-swift version bump.

## 3. Imports & resolution

Swift's `import Foundation` / `import struct Foundation.Date` (a scoped submodule-member import,
rare in practice) both emit specifier = the leading module name (`Foundation`), `ImportKind::
Package`, `Confidence::Certain` (an import that doesn't resolve is either genuinely external or
a compile error, never a maybe).

**Resolution algorithm** (the adapter's `resolve`):

1. **Same-target (no import needed).** Handled entirely by the core's `unit`-based fallback —
   any file under the same `Sources/<Target>/**` tree resolves siblings without an import,
   never reaching this adapter's `resolve()` at all, identical shape to Java's same-package
   fallback.
2. **A locally-declared target/module name.** Look up against the known-units index (built from
   every claimed file's `unit` — §0's Sources/Tests-derived target name) — a hit resolves
   `Resolution::File` at the target's first file in path order, same algorithm as Java's
   package-to-file resolution.
3. **`Foundation`, `Swift`, `Combine`, `Dispatch`, and the rest of the platform SDK.**
   `Resolution::Stdlib` — these ship with the toolchain/OS, not a package dependency; matched
   against a small fixed prefix table (§7 — this list is inherently non-exhaustive across
   Apple's full SDK surface, a documented, narrow gap, same shape as any stdlib-list adapter's
   list-maintenance burden).
4. **Anything else (an external SPM dependency's module name).** `Resolution::Unresolved` —
   never `Resolution::Dependency` (§0's last bullet: the local `Package.swift` doesn't state
   what module name(s) a `.package(url:)` entry exports).

## 4. Manifests & packages (RFC 0011)

**`Package.swift` is parsed as Swift source**, not a data format — `manifest.rs` calls the same
`crate::parsing::parse` extraction uses, then walks the `let package = Package(…)`
`call_expression`'s `value_arguments`, matched by each `value_argument`'s `name` field
(`value_argument_label`) rather than positionally (SwiftPM itself requires every `Package(…)`
argument to be labeled, so this is exact, not a heuristic):

| Argument | Extracted as |
|---|---|
| `name:` (string literal) | `ManifestFacts::package_name` |
| `products:` (array of `.library(name:, targets:)` / `.executable(…)` calls) | `private = true` iff no `.library(…)` product entry exists at all (an executable-only or product-less package exports nothing importable, same "packaging says nothing is published" reasoning as Java's war/pom packaging check) |
| `dependencies:` (array of `.package(url:, from:/exact:/branch:/revision:/…)` calls) | one `ManifestDependency` per entry — `name` = the URL's last path segment with a trailing `.git` stripped (`https://github.com/Alamofire/Alamofire.git` → `Alamofire`; a best-effort identity, since the repo name and the module(s) it exports aren't guaranteed identical — §0's last bullet), `version_req` = whichever of `from:`/`exact:`/`branch:`/`revision:` is present (first match, string value as-is — no semver comparison attempted beyond what `version-skew` already does generically), `scope` = `Prod` always (SwiftPM's dependency model has no Maven-style compile/test/provided split at the declaration site — a documented v1 simplification, §7) |
| `targets:` (array of `.target(name:, dependencies:)` / `.testTarget(…)` / `.executableTarget(…)` / others) | `ManifestFacts::workspace_members`, one entry per target `name:` — RFC 0011 §3's local topology; a target's own `dependencies:` array (naming other local targets or external product names) is **not** cross-referenced against the top-level `dependencies:` list in v1 (§7) |

**Conformance-witness roots (M6, Alamofire corpus)**: a non-private method of a type (or
extension) that declares any inheritance/conformance entry roots `Production`/`Possible` — it
may witness a protocol requirement invoked by machinery outside the repo (a custom
`KeyedEncodingContainerProtocol`'s methods are called by the stdlib's Codable synthesis; no
source call site can exist). External protocols' requirements aren't statically enumerable, so
witness-vs-dead is undecidable — degrade toward silence at the `Possible` dynamic tier, the
same reasoning as `override` rooting but one confidence rung lower. `deinit` extracts as a
`Constructor`-kind `<deinit>` member: runtime-invoked, liveness follows the type, body walked.

**Root promotion**: for every target named in a `.library(…)` product's `targets:` list (i.e.
publicly exported, not merely locally declared) and not itself a test target, one
`ManifestRoot{Production, Certain}` per non-test `.swift` file under the target's source tree —
its explicit `path:` argument when declared (Alamofire's `.target(name: "Alamofire", path:
"Source")`, M6 FP hunt), `Sources/<TargetName>/**` under the Standard Directory Layout otherwise
— reusing `ResolveCtx::files_under` exactly like Java/Kotlin's mechanism (docs/adapters/
java.md §4), just parameterized per-target instead of once per manifest (a `Package.swift` can
declare several independently-public-or-not targets, unlike a Maven/Gradle module's single
private/public flag). Non-exported targets (declared but never listed in any `.library(…)`
product's `targets:`) get **no** root promotion — their liveness depends entirely on whether an
exported target's code actually imports and uses them, exactly the "no manifest-declared
surface, ordinary reachability applies" default every adapter falls back to.

## 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| Extension member liveness (`extension Widget { func extra() {} }`) | `extra`'s `member_of` is `Widget`'s bare name; the extension block itself has no declaration of its own (§2) — same "no name to hang a finding on" stance as Kotlin's companion objects |
| Computed properties / property observers | getter/setter/willSet/didSet bodies walked as part of the owning property's declaration span, not independently declared (§2) — a call to the property (`widget.x`) is an ordinary member reference; the get/set dispatch itself is invisible, same class of gap as Java/Kotlin's compiler-synthesized-accessor stance |
| `open` vs `public` | both widen to `VisibilityScope::Public` — kndo's model can't represent "public but not subclassable outside the module," same conservative-collapse Java's `protected`-to-`Public` already establishes |
| Local target-to-target dependency graph vs the manifest's flat `dependencies:` list | not cross-referenced in v1 (§4, §7) — a target's own SPM-declared local dependencies don't currently narrow which external packages *it specifically* needs; every declared external dependency is treated as available project-wide for resolution purposes (same as how every prior adapter's dependency list is project-wide, not per-file-scoped) |
| `@available`/platform-conditional code (`#if os(iOS)`) | not modeled — a documented non-goal (§7), same class as every adapter's stance on preprocessor-style conditional compilation |
| Result builders (`@ViewBuilder`, SwiftUI's declarative body syntax) | walked as ordinary expression bodies — no special understanding of builder-transformed control flow, same "don't try to out-think a DSL" stance macro-heavy Rust code already takes |
| Property wrappers (`@State`, `@Published`, …) | the wrapped property is an ordinary `Field`/`Variable` declaration; the wrapper attribute itself contributes no extraction-visible behavior change |

## 6. Conformance fixtures (shared harness, RFC 0002 §8)

Four fixtures, each a real SwiftPM package tree run through the real `Engine` (no mock),
mirroring the Java/Kotlin fixture sets so precision is comparable apples-to-apples:

- **`dead-code-same-target`** — a `Package.swift` with no `.library(…)` product (private, no
  root promotion): `main.swift` calls `Helper().live()` with no import (same-target `unit`
  resolution) while `Helper.dead()` is never called — `dead` reads `unused`, `live` doesn't.
  (The top-level `let helper = Helper()` itself also reads `internal-only`: a default-`internal`
  global referenced only from its own file — §2's lazy-global `within` note doesn't change this,
  since the reference sits in `main.swift`'s own script code either way.)
- **`dispatch-and-extension`** — `Impl` **subclasses** `Base` and overrides `go()`; `main.swift`
  calls `impl1.go()` by name. Because `Base` and `Impl` both declare a member named `go`, the
  call is ambiguous by name alone (extraction carries no receiver types) and lands on RFC 0012
  §3's duck-typed member fallback: `Base.go` picks up a `Possible`-confidence candidate edge
  too, which is *not* strong enough to justify any visibility rung — it reads `internal-only`
  rather than `unused`, correctly conservative given a same-named override genuinely exists. A
  separate `extension` block adds a `public` member to a type declared in another file, proving
  cross-file `member_of` attribution for extensions works — the one member-owner shape unique to
  this adapter (§0's third bullet) — and that its visibility is checked like any other member's.
- **`visibility-ladder-and-internal-default`** — one target, all five ladder rungs exercised on
  a class's members (`private`/`fileprivate`/`internal`/`public`/`open`) plus one member with
  **no modifier at all** (`useOwn`), proving the no-modifier-means-internal default (§0) computes
  the same `Package`-scope tightest-sufficient check as an explicit `internal` would — neither
  reads `internal-only` when used only from within the same target, while the `public`/`open`
  members do (declared wider than their same-target-only usage requires).
- **`package-swift-dependency-skip`** — a `Package.swift` declaring one `.package(url:, from:)`
  external dependency never imported anywhere: the `unused`-dependency verdict that would
  otherwise fire is skipped instead (`resolves_dependency_usage: false`, one diagnostic —
  §0/§4), proving a declared-but-unimported SPM dependency is never misreported as dead.

## 7. Open questions

1. `Foundation`/`Swift`/platform-SDK stdlib prefix list (§3 point 3) — inherently non-exhaustive
   against Apple's full SDK surface; parked as a small fixed table, widened only if real
   dogfooding on a third-party Swift corpus surfaces a false `undeclared`.
2. Target-to-target local dependency graph cross-referencing against the top-level
   `dependencies:` list (§4, §5) — every declared external dependency is currently treated as
   available project-wide rather than scoped to the targets that actually list it; a contained
   follow-up, not attempted in v1.
3. Per-dependency scope fidelity (`Prod` always, §4) — SwiftPM's manifest has no compile/test/
   provided split at the `.package()` declaration site the way Maven/Gradle do; a `testTarget`-
   only dependency currently reads identically to a library-wide one for scope purposes.
4. Swift Package Manager plugin targets (`.plugin(…)`), macro targets (`.macro(…)`), and binary
   targets (`.binaryTarget(…)`) — parsed as ordinary `targets:` entries contributing to
   `workspace_members`, but not given any special root-promotion or dependency-resolution
   treatment; real but likely rare in application code, parked pending signal.
5. `@available`/`#if`-gated conditional compilation (§5) — entirely unmodeled, same non-goal
   class as every adapter's preprocessor stance.
