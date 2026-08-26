# RFC 0012 — Precise Reference Semantics & Visibility

**Status:** Accepted · **Depends on:** RFC 0002, 0005, 0011 · **Changes:** contracts §2 (staged — each
stage updates `contracts/core-traits.md` in the same commit as its implementation, per repo rule)

## 1. Problem

The second language (Go, M3) stress-tested the adapter contract and surfaced a *class* of gaps
with one shared root cause: several facts the analyses need are currently either **encoded
inside strings** (a method's owner hidden in `"T.Method"`), **absent from the contract**
(which declaration a reference executes inside; whether a reference is a type-position use;
what scope each visibility level actually grants), or **fixed too early** (a file's origin
decided from its path alone, before content is ever seen). Each gap was individually worked
around or documented as an imprecision. This RFC eliminates them as a group, with one design
rule: **kndo must be precise, and the precision must live in the core** — adapters supply
declared facts, the core owns every mechanism. No proposal below may encode knowledge of a
specific language in `kndo-core`; every new contract field must map sensibly onto at least
three of the eight launch languages (RFC 0002 §7: JS/TS, Go, Java, Kotlin, Swift, Rust, JSON,
CSS) or it doesn't belong in the contract.

Concrete imprecisions this RFC closes, worst first:

1. A reference to `Method` never resolves to a declaration named `T.Method` — unexported Go
   methods used only inside their package read as `unused:method` (an active false positive,
   the one failure mode the product promises to never have).
2. A live file keeps alive **everything** it references, even references inside its own dead
   functions — transitively dead code is invisible today (under-reporting; the "AI slop"
   kndo exists to catch is exactly this shape).
3. Every `References` edge is hardcoded `RefKind::Read` — `private-type-leak` (RFC 0005 §7)
   is unimplementable, and `Extend`/`Implement`/`TypeUse` evidence doesn't exist.
4. `VisibilityLevel` is a bare index with no declared meaning — `internal-only` can only check
   the file boundary, which under-reports for every package-visibility language (Go, Java,
   Rust, Kotlin).
5. Generated files can't be detected (their marker is content, but origin is fixed at
   claim time from the path alone) — findings fire on code nobody authored.
6. An unaliased import's local name is guessed from the specifier's last path segment —
   wrong whenever the target's declared name differs (`gopkg.in/yaml.v3` binds as `yaml`).

## 2. Design principles (normative for every section below)

- **Data from the adapter, mechanism in the core.** Same pattern as `classify.rs`'s
  `PathPatterns` and `FileFacts::unit`: the adapter declares facts/tables; the core owns the
  single implementation of what they mean. A core `if language == X` still reverts the PR
  (ROADMAP standing rule).
- **Degrade toward keep-alive, never toward accusation.** Every fallback, unresolvable name,
  or missing fact must reproduce today's over-approximation (more code considered alive) —
  never a new way to call live code dead. "Never falsely accuse" outranks precision.
- **Additive and opt-in.** Every new field has a `None`/default that reproduces current
  behavior byte-for-byte. Adapters adopt independently; there is no flag day. A field's
  adoption bumps that adapter's `facts_schema_version` (RFC 0004 §3) — the designed
  invalidation mechanism; no cache migration is ever written (ADR 0004).
- **Confidence is the honesty channel.** Where type information doesn't exist, resolution
  uses RFC 0002 §5's ladder (`certain`/`probable`/`possible`) instead of guessing or refusing
  — the tiers already exist for exactly this.

## 3. Member declarations: `member_of` + visibility-scoped member-call resolution

**Contract:** `Declaration` gains `member_of: Option<SmolStr>` — the declared name of the
owning type, when this declaration is a member of one. The symbol's own `name` becomes the
bare member name (`Method`, not `"T.Method"`); display/symbol-path rendering joins them
(`T.Method`). `SymbolNode` carries the field through to the graph.

**Core mechanism — member-call fallback.** Reference resolution (assembly phase 3b) gains a
final tier after import-bound / same-file / same-unit exact-name lookups miss: the reference
matches member declarations (`member_of.is_some()`) with the same bare name. This is RFC 0002
§5's duck-typing rule ("duck-typed method with one candidate → probable"), finally
implemented, with its scope defined by §6's visibility ladder:

> A member declaration is a candidate iff its declared visibility scope **contains the
> reference site** — an unexported Go method (scope `Unit`) is only a candidate for
> references in its own unit; a public JS class method (scope `Public`) is a candidate
> project-wide.

Confidence: exactly one candidate in scope → `Probable`; several → `Possible` each (all get
edges — conservative keep-alive; `possible` sits below the default report floor, RFC 0006).
Dead-is-certain survives intact: a member with *no* same-named call anywhere is still
`unused` at `certain`. Until §6's ladder lands, the interim scope is same-file + same-unit —
for Go this is *complete by the language's own rules* (unexported members are only legally
callable in-package; exported members are roots, §4 of docs/adapters/go.md), so the interim
is exact for the one language that has members today, not an approximation.

Statically-typed adapters may later do better than the fallback (receiver-typed resolution
via `scope_context`, §9) — the fallback is the floor the core guarantees, not a ceiling.

**Language fit:**

| Language | `member_of` maps to |
|----------|--------------------|
| JS/TS | class/interface members (methods, fields, getters — js-ts.md §2 already promises them), enum members (owner = enum) |
| Go | methods via receiver type (`func (t T) M()` → `M` member_of `T`) |
| Java | every method/field (owner = enclosing class) — Java has no non-member functions |
| Kotlin | class members; top-level functions have `None` |
| Swift | members of struct/class/enum/protocol/**extension** (owner = extended type's name) |
| Rust | `impl T` fns (owner `T`), trait fns (owner = trait name) |
| CSS/JSON | `None` always |

## 4. Symbol-granular reference attribution: `within`

**Contract:** `RawReference` gains `within: Option<SmolStr>` — the declared name of the
symbol this reference executes inside, under one language-blind rule:

> **`within` = the declared symbol whose *use* triggers this code.**
> - Body of a callable (function, method, initializer that runs on call) → that callable.
> - Code that runs when the module/file **loads** (top-level statements, package-level
>   variable initializers, JS static class blocks — anything unconditional at load) → `None`.
> - Code that runs when a type is **instantiated or first used** (constructors, instance
>   field initializers, Java static initializers — which are lazy on first class use,
>   Swift lazy globals) → that type/symbol.

The principle decides the cases, not the list — the list is illustrative. Member bodies name
their `within` exactly as the `Declaration` is named (bare name + the §3 `member_of`
convention), so `within` resolution reuses the same tables.

**Core mechanism.** Phase 3b resolves `within` against the file's own symbol table and emits
`References { from: NodeRef::Symbol(enclosing), .. }` when it resolves; **any miss falls back
to `NodeRef::File` — today's behavior, the safe direction** (this fallback is the load-bearing
safety property; it gets its own regression test).

**Twins: the name is not enough, the span is.** `within` is a NAME, and same-name overloads
are legitimate twins sharing one `Owner.name` selector — Swift's `get(at:)` beside
`get(path:)`, Java's arity overloads. The qualified table is single-slot, so a name lookup
attributed every reference in EITHER body to whichever twin was inserted last; the other had
no outgoing references at all and read as dead unless something else named it (vapor's private
`get`, called from its public sibling one line above). Resolution therefore disambiguates among
`insert_qualified`'s twin set by **span containment** — a reference lies physically inside
exactly one declaration, so the containing one is its author. Exact, language-blind, and it
falls back to the single-slot answer when no candidate contains the span, so an adapter
reporting `within` without a matching span is no worse off than before. `graph::assemble`'s
`within_owner` is the one implementation. The reachability algorithm needs *zero
changes*: adjacency is already `NodeRef`-keyed, `Symbol → Symbol` edges already traverse, and
the symbol-reaches-its-owning-file propagation (added in M3, RFC 0005 §1) is the second half
of this model — formally:

> **Module-load rule:** reaching a symbol reaches its owning file (using a symbol loads its
> module — the file's `within: None` references and its `ImportsFile` edges fire).
> **Execution rule:** a symbol-attributed reference fires only when its symbol is reached.

Together these make transitive death visible: `main → a` keeps `a` alive; dead `z → b` no
longer keeps `b` alive. They also sharpen `test-only` (a test-only function's callees color
test-only instead of inheriting the file's production color) and — with §5 — let a dead
function's signature types die with it.

`DynamicUse`/wildcard expansion stays file-granular in this RFC (coarser = safer; revisit only
with evidence). Navigation (`uses`/`used-by`/`trace`) and finding evidence gain real
"which caller" attribution for free — the file-granularity apology in `engine.rs` is deleted.

**Consequences, stated honestly:** on real repos, findings *appear* that were previously
masked (transitively dead code). That is the point, but it mandates a dogfooding pass on the
JS corpus and Go fixtures before trusting, updated conformance fixtures in the same commit,
and (pre-1.0) accepting baseline churn.

**Language fit:**

| Language | callable bodies | load-time (`None`) | on-use/instantiation |
|----------|-----------------|--------------------|---------------------|
| JS/TS | functions, methods, arrows bound to a declaration | top-level statements, static blocks/fields (class evaluation runs at load) | constructor + instance fields → the class |
| Go | func/method bodies, `init` | package-level var/const initializers | — |
| Java | method/constructor bodies | — (class loading is lazy) | field initializers, instance *and static* init blocks → the class |
| Kotlin | functions, methods | top-level property initializers | init blocks/constructors → the class; companion initializers → the class |
| Swift | funcs, methods, closures bound to a declaration | — | lazy globals → the global's own symbol; type members → the type |
| Rust | fn bodies | — (no load-time execution) | const/static initializers → the const/static (compile-time, but the *dependency* is real: a dead const's referents die with it) |
| CSS | `@mixin`/`@function` bodies (SCSS only) → that callable's own symbol | plain rule bodies (no rule-level symbol exists to attribute to — selector/class/id extraction is deliberately out of v1 scope, docs/adapters/css.md §0) | — |

## 5. Reference kinds & signature spans → `private-type-leak`

**Contract:** two fields.

- `RawReference.kind: RefKind` — the vocabulary (`Call/Read/Write/Extend/Implement/Override/
  TypeUse`) has existed since M0; the core stops hardcoding `Read` and passes the adapter's
  kind through to the edge. Adapters adopt incrementally (untagged = `Read`, exactly today).
- `Declaration.signature_span: Option<Span>` — the sub-span covering the declaration's
  *signature* (parameters + return/result types; everything before the body). The adapter
  knows where a body starts; the core must not.

**Core mechanism — `private-type-leak` (RFC 0005 §7's second half), as pure core analysis:**
for each exported declaration `D` with a `signature_span`, each `TypeUse` reference whose span
lies inside it, resolved to a symbol `T`: if `T`'s visibility scope (§6) is narrower than
`D`'s → finding (group `defect`). Zero language knowledge; evidence is the reference span.

**v1 scope, honestly bounded:** callables only. A *type's* leak surface is its exported
*fields* — which requires fields to exist as member declarations with their own visibility
(§3) and their own type spans; that lands with member extraction, not before. Firing on a
whole struct body without field-level visibility would accuse exported-struct/unexported-field
cases falsely — exactly what the degradation principle forbids.

**Language fit:** Go gets `TypeUse` nearly free (`type_identifier` node kind *is* the
type-position signal) and embeddings → `Extend`. TS: type positions → `TypeUse` (extraction
work), `extends`/`implements` → `Extend`/`Implement`, `import type` bindings → `TypeUse`.
Java/Kotlin/Swift: extends/implements/conformance clauses and signature type positions map
one-to-one. Rust: `impl Trait for T` → `Implement`, path-in-type-position → `TypeUse`.
CSS: `var(--name)`/bare SCSS `$name` → `Read`, `@include`/any other call-expression → `Call` —
no `TypeUse`/`Extend`/`Implement` analogue exists (no type system). `composes` (CSS Modules)
would also be `Read` in spirit, but stays unimplemented in v1 alongside selector/class
extraction (docs/adapters/css.md §0/§5) — nothing to resolve it against yet. JSON: none.

## 6. The visibility ladder as data

**Contract:** `AdapterDescriptor` gains the ladder the adapter's `VisibilityLevel` indices
into — each rung declaring its **scope** (a graph concept the core already owns) and its
**label** (the language's own word, for remediation text — RFC 0005 §7 requires remediation
"in the language's own terms, supplied by the adapter"):

```rust
pub struct VisibilityRung { pub scope: VisibilityScope, pub label: SmolStr, pub surface_transitive: bool }
pub enum VisibilityScope { File, Unit, Package, Public }
// AdapterDescriptor gains: pub visibility_ladder: Vec<VisibilityRung>  (index = VisibilityLevel)
```

`File` = same file · `Unit` = same `FileFacts::unit` key · `Package` = same `PackageId`
(RFC 0011) · `Public` = everywhere. Two rungs may share a scope (the ladder is the language's
own level list; the scope is what the core can check). Empty ladder = visibility analyses
skip the language entirely (CSS, JSON).

**`surface_transitive` (M6):** whether a re-export chain can carry a declaration at this rung
*outside its package* — an axis `scope` cannot express. Rust `pub` and a JS `export` are
**relative** (as visible as the module path re-exporting them → `true`); Rust `pub(crate)`,
Java package-private, Swift `internal`, and Go exports under an `internal/` path element are
**capped** (no re-export widens them → `false`). Two core mechanisms read it: library-mode
symbol promotion (RFC 0011 §5 — only transitive rungs are consumable surface) and the
surface-member closure (a surface type's transitive members are surface too — a `pub` method
of a re-exported struct is consumer-callable API even with zero in-package references).
Note Java `protected` and JS "exported" are transitive despite non-`Public`-looking
consumption paths: external subclasses override `protected`, and a JS entry file's exports
are the package surface by definition.

**Core mechanism — `internal-only` generalized:** tightest-sufficient visibility = the lowest
rung whose scope contains **every** incoming reference's origin (checked per edge against
facts the graph already has: same file / same unit / same package). Declared rung above it ⇒
finding; the remediation names the lower rung's `label`. This replaces today's file-boundary
approximation, fixes the documented Go under-reporting (an exported symbol used only by
same-unit siblings → "could be `unexported`"), and is what §3's member-fallback scoping and
§5's leak comparison read from — one ladder, three consumers.

**Language ladders (adapter-declared data, listed here as the design record):**

| Language | ladder (index → scope, label) | notes |
|----------|-------------------------------|-------|
| JS/TS | 0 `File` "module-local" · 1 `Package` "exported" · 2 `Public` "package surface" | rung 2 = reachable through the `exports` map (js-ts.md §4); adapter emits 0/1 today, 2 lands with surface-awareness |
| Go | 0 `Unit` "unexported" · 1 `Package` "exported (internal)" · 2 `Public` "exported" | rung 1 (M6): an export under an `internal/` path element — the compiler itself walls it off from external modules, so it is capped (`surface_transitive: false`) and Package-scoped, keeping it out of the library-surface machinery while `internal-only` can still advise narrowing; the adapter assigns it by path, the one visibility fact Go keeps outside the identifier |
| Java | 0 `File` "private" · 1 `Unit` "package-private" · 2 `Public` "protected" · 3 `Public` "public" | `private` ≈ enclosing file (nested classes share it); `protected` maps conservatively to `Public` — subclasses live anywhere, never suggest narrowing onto them — and is **exported** (subclass-consumable API, M6); interface/annotation members with no modifier are implicitly `public` (JLS §9.4) |
| Kotlin | 0 `File` "private" · 1 `Package` "internal" · 2 `Public` "protected" · 3 `Public` "public" | `internal` = compilation module ≈ Package (unlike Java, Kotlin's `package` carries no visibility meaning at all — the default with no modifier is `public`, not package-scoped); `protected` (members only, same "package ∪ subclasses anywhere" shape as Java's) maps conservatively to `Public`, mirroring Java's own two-rungs-share-a-scope pattern |
| Swift | 0 `File` "private" · 1 `File` "fileprivate" · 2 `Package` "internal" · 3 `Public` "public" · 4 `Public` "open" | the ladder applies uniformly at top-level and member position (no restricted subset); `internal` — the default with no modifier at all — is a *third* distinct default among the launch languages (Java ≈ `Unit`, Kotlin = `Public`); `open` (subclassable outside the module) maps conservatively to `Public` alongside `public`, mirroring Java's `protected`/`public` collapse — kndo's scope model can't distinguish the two |
| Rust | 0 `Unit` "private" · 1 `Package` "pub(crate)" · 2 `Public` "pub" | unit = module; `pub(super)`/`pub(in …)` map to the nearest **wider** rung (conservative) |
| CSS/JSON | `[]` | visibility analyses skip |

Conservative-mapping rule (normative): when a language level has no exact `VisibilityScope`,
the adapter maps it to the nearest **wider** scope — over-approximating who may see a symbol
can only suppress an `internal-only` finding, never fabricate one.

## 7. Content-derived origin: `detected_origin`

**Contract:** `FileFacts` gains `detected_origin: Option<FileOrigin>` — extraction may
*correct the origin axis* of the claim-time `FileClass` (role stays claim-time; no use case
justifies content-derived roles yet). Assembly applies the override when building `FileNode`,
before the role-derived-roots phase, so every origin exemption (`unused`, `test-only`,
`untested`, `internal-only` all exempt `Generated`) sees the corrected value.

**Why this shape:** origin-by-content is a fact *about the content*, and `FileFacts` is the
content-addressed fact bundle — the override rides the facts cache with zero extra I/O and no
claim-time slowdown. The rejected alternative (a content-peek at claim time) breaks claim's
"fast, name-based" property and discovery parallelism.

**Toolkit:** a data-driven first-N-lines scanner (`ContentMarkers { line_patterns,
scan_window_lines }`, mirroring `PathPatterns`) for comment-marker languages. The *field* is
the contract; the scanner is a convenience — an adapter with a structured signal (Java's
`@Generated` annotation, parsed from the AST it already has) sets the field from its own
parse instead.

| Language | generated signal |
|----------|------------------|
| Go | `^// Code generated .* DO NOT EDIT\.$` (the `go generate` convention — single authoritative source) |
| JS/TS | `@generated` markers, codegen banners (js-ts.md §1's existing list, finally actionable) |
| Java/Kotlin | `@Generated`/`@javax.annotation.Generated` annotations (AST-derived, not line-scanned) |
| Swift | `// Generated by` banners (sourcery et al) |
| Rust | `// @generated` / build-script banners |

Known non-goal: JS's ".d.ts sibling of a same-name .ts" rule needs *cross-file* knowledge —
neither claim (one path) nor extract (one content) can see siblings; that stays a documented
js-ts gap, unsolved by this RFC rather than half-solved.

## 8. Unit-key conventions (no contract change — a design record)

`FileFacts::unit` is an opaque key; the core only groups by it. That opacity is load-bearing:
adapters encode their language's *real* resolution unit in it without the core learning
anything. Conventions per language, recorded so adapters stay mutually consistent in spirit:

**A unit key is unique only within a package.** Java/Kotlin key on the declared package name
and Swift on the target name, so two Gradle modules declaring `package retrofit2;`, or two
Swift packages each declaring a target `Core`, share one key repo-wide. The core therefore
keeps the reverse index in two forms — repo-global, and partitioned by owning package
(`graph::assemble::build_unit_indexes`, shared with the patch path) — and
`ResolveCtx::unit_files_from` prefers the importer's own package, falling back to global only
when it has no candidate. Without the preference a resolver picking `.first()` by path order
binds intra-module imports to unrelated siblings; without the fallback genuine cross-module
imports stop resolving. Reachability never depended on which file an edge landed on (same-unit
fallback), but `cyclic` reads the literal edge as evidence, and read phantom package cycles out
of it in four of the six languages a field audit covered.

**And a NAME is not unique within a unit.** The mirror of the §4 twins case, on the incoming
side: two files of one unit may legitimately declare one name when the language makes them
mutually exclusive — Go's `//go:build` alternates (gin's `binding.go` under `!nomsgpack` and
`binding_nomsgpack.go` under `nomsgpack`, both `func validate`), Rust's `#[cfg]` alternates.
Span containment cannot disambiguate here (the twins sit in *different* files and the
reference is inside neither), and nothing should: kndo analyzes the union of build
configurations, so both declarations are live and a reference under either configuration
reaches its own. The single-slot unit table gave the last-inserted twin every reference — 16
of them in gin — and left the other at zero incoming edges, falsely `unused`. Core therefore
keeps the displaced declarations (`symbol_twins_per_unit`, built beside the name table in
both `graph::assemble` and `graph::patch`) and emits the edge to **every** twin. Twins are
consulted only when the winner came from the unit table: a name bound by an import, or
declared in the referencing file itself, is one specific symbol and not a member of a twin
set. Same keep-alive direction as everywhere else — an extra edge to a twin that some build
configuration excludes costs recall under that configuration, never a false accusation.

**Which tier resolved decides which twins apply.** The bare-name ladder is: names bound by
this file's imports, then its own declarations, then its unit's, then units a wildcard import
makes visible. Kotlin multiplatform is the case that exercises every rung — `expect` in
`common` beside one `actual` per platform, all under one declared package, hence all twins of
each other — and the callers reach them from all four positions: an explicit import (the
binding's own twin set, recorded when the binding resolved through the TARGET's unit table),
the declaring file itself (`expect inline fun yieldThread()` sits in the file that calls it),
a sibling file of the unit, and a wildcard-imported package. Each tier therefore carries the
unit's full twin set for the name, minus whichever member of it that tier returned.

**Wildcard-visible names rank BELOW members in scope.** The last rung is consulted only after
the §3 duck-typed member fallback comes up empty, which is where every language with this tier
puts it: Kotlin resolves an unqualified call against local names, then implicit receivers, and
only then imported top-level names. Ranked above the fallback instead, an unrelated top-level
`updateState` in a wildcard-imported package took both call sites of kotlinx.coroutines'
`StateFlowImpl.updateState` — a method calling its own type's member — and left it `unused`.

| Language | unit key |
|----------|----------|
| Go | `dir#declared-package-name` — splits external test packages (`foo_test`) from `foo` in the same directory, closing the documented §1.1 imprecision of docs/adapters/go.md with zero core changes |
| Java | declared package name (dotted string from the `package` statement) — never directory-derived, sidestepping source-root detection (`src/main/java` is a build-tool convention, not language-visible from a bare file path); docs/adapters/java.md §0 |
| Kotlin | same as Java (declared dotted package name) — but note this key carries *zero* visibility meaning for Kotlin (§6), only resolution meaning (same-package unqualified reference, wildcard import enumeration) |
| Rust | the file's own module, keyed by path with `mod.rs`/`lib.rs`/`main.rs` folded into their directory — so **one file per key**. Rust files still resolve nothing implicitly across files (a one-file unit table IS the file's own, already the earlier tier), which is exactly why turning the key on moves no resolution; what it buys is twin tracking, which is per-unit and which a keyless language therefore had none of. `internal/adapters/rust.md` §2 |
| Swift | target/module name |
| JS/TS, CSS, JSON | `None` — file-scoped languages |

## 9. Qualified-reference resolution in the core (accepted direction, scheduled after §§3–7)

The remaining string-guess in the system: an unaliased import's local name derived from the
specifier's last segment. Correct resolution needs the *target's declared name* — knowable
only where both sides exist: assembly. Contract (when scheduled): `RawImport.local_alias:
Option<SmolStr>` (the explicit alias, else `None`), `FileFacts.unit_name: Option<SmolStr>`
(the name importers bind this unit by — Go's `package` clause, Rust's module name), and the
already-existing-but-unused `RawReference.scope_context` carrying the receiver/qualifier
text. Phase 3b then resolves `qualifier.member`: qualifier matches the import's
`local_alias`, or — unaliased — the resolved target's `unit_name`. This deletes the
adapter-side dotted-binding synthesis (Go) and, long-term, subsumes JS's namespace-member
machinery (`ns.foo`) under the same core rule. Deferred because its current blast radius is
small (external targets have no in-graph symbols to mis-bind; in-repo dir≠package mismatches
are rare) and §§3–6 change the same code paths — land those first, then refactor once.

**As landed (M6, extended):** a matched qualifier resolves in order — the target file's bare
table, its unit siblings, then its *member table*: the alias may name a TYPE rather than a
module (`Thing::from_low_args()` through `use crate::thing::Thing`), where the member lookup
follows the qualifier symbol to its home file first (a barrel's re-export alias lands on the
original, so `SearchMode::Standard` through `use crate::flags::{SearchMode}` reaches the
declaring file's members). Hit or miss, a matched *alias* still settles. A qualifier matching
no alias but matching an **import binding** resolves `Original.member` in the bound symbol's
home file at Certain — and on a miss does NOT settle: a binding is a value/type, not a closed
namespace, so an unknown member falls through to the §3 duck-typed fallback exactly like a
receiver expression. Re-exported GLOBS (`pub use x::*`, `export * from './x'`) alias the
target's exported surface into the barrel inside the RFC 0013 §3b fixpoint (or-insert
collision rule; alternates that lose the collision stay alive through the glob's Wildcard
edge). In-source roots targeting a declaration name land on EVERY declaration sharing the
selector — twins are legitimate (two `impl Add for Stats` blocks both declare `Stats.add`).

**§3-bis, receiver typing (M6):** adapters may pin a receiver's TYPE from facts local to the
file (Rust: `self`/`Self` → impl owner, typed params/lets, struct-literal and `T::assoc(…)`
initializers with chain flow — see the adapter spec) and emit the type as `scope_context`
instead of the opaque receiver name. Core-side, the qualifier resolution above gains one
tier: a qualifier matching a name in scope — an import binding OR a same-file declaration —
resolves `Original.member` in that symbol's home file at Certain, and on a miss falls
through to the §3 duck fallback (never settles: a name in scope is a value/type, not a
closed namespace). Qualified-member hits land on EVERY declaration sharing the selector
(cfg-alternated twin impls both own `Data.from_path`; the single-slot table's displaced
twins are tracked and each gets the edge). The reliability invariant, both sides: a wrong
receiver type can only miss into the fallback or hit a member the named type genuinely
declares — silence-direction errors only.

**§3-bis cross-file tier (M6): member-type facts.** `FileFacts.member_types` carries
`(owner, member, yields)` — what accessing a member evaluates to, as declared (field types,
method returns, associated consts). A DOTTED qualifier is a chained pointer
(`LowArgs.context_separator`): the base resolves in the reference's scope, `yields` comes
from the owner's home-file facts, the yielded type name resolves first where the annotation
was written (the home's declarations + re-export aliases) then in the reference site's own
scope, and the final member resolves in the yielded type's home — twins included, every hop
a declared fact, Certain on hit, duck fallback on any miss (never a settle). The resolved
chain also credits the yielded TYPE with a Read from the site. Facts are part of the
RFC 0013 §4 surface signature (an annotation change re-resolves dependents) and persist in
`FilePatchMeta` for the patch path. `RawMemberType.yields` carries the annotation's arguments
as written (`Result<ConfiguredHIR, Error>`); a pointer segment marked `?N` projects argument N
instead of the wrapper (`?` is shorthand for `?0`). The marker is structural — WHICH argument an
operation extracts is the adapter's knowledge (Rust's try operator → 0), the core just indexes.
**As landed, `yields` is a tree and a projection lands on a subtree** (§3-quater below): the
one-level list this paragraph originally described could not hold `Result<Vec<T>, E>`'s `T`. Pointers compose to
N hops (each hop a declared fact, each resolved hop's type credited with a Read from the
site); a projection index with no parameter at that position is a miss — duck fallback,
never a settle.

**§3-ter, a value's type from the call that produced it (M6).** `let entry = parse_entry(..);
entry.path` — the receiver's type is the callee's declared RETURN, which lives in the callee's
file. `RawMemberType::owner` is therefore `Option`: `None` reads "calling `member` evaluates to
`yields`", the same statement about a value's type that `Some(owner)` makes about a member
access, so the core walks it with the same chain machinery — a `None`-owner fact simply applies
where a pointer's BASE names the function, one step before `chain_hop` acts on a member segment
(`call_yield`). The projection marker composes there too: `let cfg = build(..)?` takes parameter 0.

A pointer's base also stopped having to be a symbol. Rust reaches free functions through their
module constantly (`rollup::directory_rollups(..)`), and there the file binds only the MODULE —
so `pointer_base` resolves a base that matches no name in scope against the qualifier table,
consuming the next segment as the symbol inside that target (its bare table, then its unit
siblings, the same pair every other tier uses). Without that arm the pointer died at its first
segment and every field the caller read looked file-local.

The adapter side of the same tier is language knowledge, and stays in the adapter: a member-type
fact for every top-level function's declared return; a fact from `#[derive(Default)]` (the derive
states the impl exists, the trait's signature states what it returns — curated stdlib knowledge
like `is_machinery_trait`, not inference); a bare or module-qualified callee typing its binding by
naming the FUNCTION rather than guessing its return; and a `for` variable projecting the
iterable's parameter 0, only where the iterable is a chain whose last hop is a declared fact — a
local's own annotation kept just its base name, so `Vec` alone has no parameter to project.

Measured on this repo: three types that carried an acknowledgement pragma
(`internal/detection-gaps.md` §3 — `DirRollup`, `ContributedRoot`, `ContributedEdge`) stopped
needing one, with the finding count and health unchanged and no movement in either direction on
six other codebases (§4's symmetry requirement).

**§3-quater, the chain carries a TYPE (M6).** `RawMemberType::yields` was a base name plus a
one-level list of parameter names, and the chain walked `SymbolId → SymbolId`. Both halves of
that lose the same thing: `Result<Vec<TreeEntry>, GitError>` reduced to `Result` +
`["Vec", "GitError"]`, and the `TreeEntry` was gone from the facts entirely — no projection
could recover what was never stored. Flattening was not a representation detail; it was the
reason `internal/detection-gaps.md` §3's last case could not be fixed.

`yields` is now a `TypeExpr` — `Named { name, args }` | `Param(N)` | `Unknown` — and the chain's
state is one of those plus, when the head name resolves to something this project declares, the
symbol whose home file holds its member table. The two are separate because they genuinely are:
`Vec<TreeEntry>` is a type no file here declares, so it has no symbol, and the chain still has to
walk *through* it to reach the `TreeEntry` inside. A `?N` marker means exactly what it meant, and
now lands on a SUBTREE with its own arguments intact.

Three pieces follow from that, all language-blind:

- **`Param(N)` and substitution.** A fact may state a relationship rather than a type — "`map_err`
  still yields a `Result` over the same argument 0" — and the receiver's own arguments make it
  concrete at the hop. A parameter the receiver does not have becomes `Unknown`: silence, not the
  type that happened to sit at that position. `Unknown` is a variant rather than a short argument
  list so a fact never has to lie about its arity to admit it cannot name something.
- **`AdapterDescriptor::builtin_member_types`**, a second lookup tier keyed by claim language,
  consulted after the owner's home file. It is the only tier that can apply when the head resolves
  to no declaration at all, which is every type the language itself provides. Same
  data-on-the-descriptor path as the ladder and the cycle policy; the knowledge stays the
  adapter's, the core just gets another table.
- **The chain carries the file its type was WRITTEN in.** A builtin hop introduces no home of its
  own: `Vec<TreeEntry>`'s `TreeEntry` was written wherever the receiver's fact was, and resolving
  it against the reference site instead finds nothing — the site imports the container's owner,
  not every type inside it.

Iteration needed no new syntax and got none. A `for` variable's pointer ends in an ordinary
member segment whose name the adapter chooses on both sides (Rust uses `@element`), and the
adapter's builtin table declares, per container, which argument iterating one yields. A container
that declares none simply does not type its loop variable — a map iterates to a tuple, which this
model has no way to name, and silence beats a confident wrong element. The same trick names
anonymous types: Rust calls a slice `@slice` so it can carry an `@element` like any container. The
core never interprets either string.

Persistence stayed out of the contract's way. rkyv 0.8 cannot derive `Archive` for a type that
recurses through `Vec<Self>`, so `crate::rkyv_support::TypeExprAsFlat` archives the tree as a
preorder walk with arity — each atom carries its own, its children are the next `arity` subtrees,
and a cursor rebuilds it with no indices to dangle. That module exists for exactly this: the
archive format does not get to dictate the shape adapters write.

Measured: `gitutil::TreeEntry`, read through `ls_tree(..).map_err(..)?` and then iterated, was the
last of §3's four acknowledged types. All four pragmas are gone, kndo's own finding count and
health are unchanged, and six other codebases moved in neither direction.

**§9-quater, what a synthesized import may and may not do (M6).** An adapter may reconstruct an
import from a use site — Rust writes `crate::a::b::f()` with no `use` in sight, and resolution
needs something to bind. `RawImport::reconstructed` says so outright. It replaced a proxy that was
wrong: §9-ter derived "does a qualifier miss settle" from the import's CONFIDENCE, and Rust's
`crate`/`self`/`super`-rooted synthetic imports are `Certain` about where they resolve while being
no statement at all. Confidence answers "does this resolve as stated"; this answers "did the file
state it". Two rules follow.

**A synthesized binding ranks below the file's own declarations.** The bare-name ladder is five
tiers now — a STATED import, the file's own declarations, a RECONSTRUCTED import, the unit, the
units a wildcard makes visible — and the middle move is the fix. No language kndo supports lets a
written import shadow a same-named local declaration (Rust E0255), so a collision there can only
ever come from a synthetic one, claiming a precedence the language never grants it. tokio's
`dump.rs` declares `pub struct Trace` and merely *mentions* `super::task::trace::Trace` in a
field; the synthetic binding captured the file's own `-> &Trace` and `private-type-leak` reported
a leak that was not there. Every site that asks what a bare name refers to shares one function
(`graph::assemble::name_in_scope`) so the order cannot drift between them.

**A qualifier names every file bound to it, not the first.** Rust's platform modules are
`#[cfg(windows)] #[path = "windows/sys.rs"] mod imp;` beside a `not(windows)` twin, so
`imp::ctrl_break()` names two live functions and the union-of-configurations rule (§8) says both
are real. `qualifier_targets` holds a LIST; a hit emits to all of them. Keeping the first left
every alternate but one with no incoming edge and a false `unused` — the same shape
`symbol_twins_per_unit` fixes for declarations, one level up at the module binding. A hop (§9-bis)
still fills only a qualifier nothing else claimed: a direct alias is stronger provenance.

**§9-bis, the one hop through a module file (M6).** `use crate::internals::{attr, check, Ctxt};`
followed by `check::check(cx, …)` registered ONE qualifier — the specifier's own target — and
left `check` a mere binding that resolved to no symbol, because `internals/mod.rs` declares
nothing by that name: it only re-links the file with its own `mod check;`. The chain is *a
binding on the target's own import table*, and no single pass can follow it (the target's table
does not exist when its consumer is resolved). Phase 3b is therefore three passes, shared by
`graph::assemble` and `graph::patch` alike — one resolution semantics, two data sources:

1. `resolve_imports` — per file, no cross-file dependency. Alongside the bindings, qualifiers,
   visible units and edges it already produced, it records `module_bindings`: every name this
   file's imports put in scope that points at an in-repo FILE (`graph::ModuleBinding`). Both
   shapes count and the pair is the point — a brace member kept even though it bound no symbol
   (the consumer side), and an import's `local_alias`, which is where a file-linking `mod x;`
   carries its name when it has no bindings at all (the producer side).
2. `link_module_bindings` — per file, reading every file's table. A name bound to file F that
   F's own table binds again registers the qualifier F sends it to. **One hop, never a
   fixpoint** (chasing further needs a cycle guard for evidence nobody has produced);
   **non-settling**, like every derived qualifier, so a miss takes the in-scope/duck ladder
   rather than binding the access to the wrong file and killing a live method; and an existing
   qualifier **always wins** — a direct alias is stronger provenance than a hop.
3. `resolve_references` — per file, the reference/dynamic/diagnostic body, against its own
   (now hop-extended) import resolution.

Language-blind by construction: no separator, no path arithmetic, nothing but "this name binds
to that file, and that file binds this name to another file." The alternative — extend the
specifier and re-resolve `crate::internals::check` — would teach the core that `::` joins path
segments, which the ignorance rule forbids.

No RFC 0013 §4 signature change was needed: `surface_signature` already hashes a file's complete
import list, so any change to F's imports moves F's surface and forces a full rebuild — a stale
hop is unrepresentable. `FilePatchMeta` carries each file's `module_bindings` so an unchanged
file contributes its table without re-extraction; `patch_equivalence` is the harness.

**§9-ter, the last hardcoded separator (M6).** With §9-bis's hop in place, the core stopped
deriving a qualifier from the specifier's last `::`-separated segment — the one place it knew a
language's path syntax. Two Rust shapes depended on that guess and now state the name
themselves, in `local_alias`: the single-qualifier bare path (`helpers::run()`) and the deep
one (`kndo_core::discovery::find_files_named(..)`), both of which the adapter reconstructs as
synthetic imports and both of which it qualifies by a segment only it can identify. What the
guess also did — registering `b` for `use a::b::{X, Y}`, where Rust does **not** bring `b` into
scope — is simply gone; the adapter already declines to set `local_alias` there, and did before
this change.

The alias's *strength* is no longer uniform, and it is the import's own confidence that decides
it: a qualifier miss settles iff the import is `Certain`. A real `use`/`import` statement names
a closed namespace — the member is in there or nowhere — while a reconstructed import is the
adapter's reading of a path, and closing a namespace on a misread path would kill a live method.
That is the same asymmetry §9 already stated between an alias (settles) and a binding (does
not), now derived from a fact the contract already carried instead of from which code path
produced the qualifier. Measured across serde, alacritty, axios, Exposed, kotlinx.coroutines,
vapor and kndo itself: byte-identical findings, so the adapter-side alias reproduces the split
exactly and the dropped over-approximation cost nothing.

## 10. Multi-module topology (`go.work` et al) — adapter work, one recorded divergence

`go.work` becomes a second claimed manifest contributing `workspace_members` (the field
exists); member modules already self-register by declared module path, and local `replace`
directives resolve through the same name index when the target declares the same path. The
one true gap — a `replace` that *renames* a module path — is a recorded divergence, not
modeled. Analogues: Cargo workspaces + path deps (same shape), Gradle `settings.gradle`
includes, SPM local packages. No contract change anticipated.

## 11. Cache, determinism, migration

Every serialized-shape change (`Declaration`, `RawReference`, `FileFacts`) bumps the adopting
adapter's `facts_schema_version` — invalidating only that adapter's facts (RFC 0004 §3); the
graph key already folds adapter versions, so graph snapshots self-invalidate. No migration
code, ever (ADR 0004). Determinism (RFC 0008 §4): every new resolution tier iterates sorted
candidate sets; the member-fallback and ladder checks are pure functions of the graph.

## 12. Landing order (each stage = code + contract doc + fixtures in one commit)

1. **§3 `member_of` + member-call fallback** (interim same-file/same-unit scope) — kills the
   active false positive; fixes the naming convention §4 depends on.
2. **§8 Go unit-key refinement** — trivial, adapter-only.
3. **§4 `within`** — core + Go adapter (the simple adopter: no hoisting, no CJS).
4. **§4 `within`** — JS/TS adapter (the bulk: class taxonomy, multi-pass threading).
5. **§5 `RefKind` + `signature_span` → `private-type-leak`** (callables-only v1).
6. **§6 ladder-as-data → `internal-only` generalized** (upgrades §3's fallback scope too).
7. **§7 `detected_origin`** — independent; any time after 1.
8. **§9, §10** — when dogfooding demands them; §9 explicitly after 1–6.

Analyses affected at each stage keep their existing conformance fixtures green *or* update
them in the same commit with the diff explained — a fixture change without an explanation
line in the commit message is a red flag, not a formality.
