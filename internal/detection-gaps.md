# Known detection gaps

Gaps in kndo's own detection. §§1–9 come from a full triage of the self-check corpus (every
finding on kndo's own repository classified against the source as genuine or false), and are
therefore all Rust; §§10 onward come from a field audit of 24 open-source repositories across
six languages, 345 findings hand-verified against their sources. Each entry records the root
cause, where to see it, and the direction a fix would take — so the next person hitting one of
these recognizes it as a *known* limit with a design sketch, not fresh noise. Genuine-verdict
policy questions (what the analyses *should* claim) belong in RFC 0005; this file is strictly
about recall and precision mechanics.

The inline `kndo:allow` pragmas and `kndo.toml` `[[rule]]` entries in this repository that
cite this file are the acknowledged instances of these gaps.

**What is a gap and what is a limit.** A gap is something kndo could see and doesn't. A limit
is something no static analysis can see, because the fact lives outside the source: a framework
instantiating a class it found by scanning the classpath, a runner calling a method it found by
annotation, a processor in a different repository. For those, `[[externally-invoked]]` in
`kndo.toml` (see `docs/src/configuration.md`) lets the project state the fact — the declaration
becomes a real entry point, so its whole reachable tree comes alive and every analysis keeps
judging it, unlike a `[[rule]] skip` which would silence the genuine findings alongside the
false ones. Entries below say which kind they are, and the ones the mechanism covers say so.

## 1. WASM-boundary reachability (host imports)

The functions `crates/kndo-core/src/plugin_host.rs` hands to the component runtime are
called only *through* the WASM boundary: the caller is generated bridge code inside
wasmtime, not any source file the graph walks, so no reference edge can exist and the
functions read as `untested`/`unused` — while `plugin_compliance.rs` builds and runs a real
guest against exactly these functions every CI run. Root cause: reachability's evidence
universe is source references; a host-side ABI surface is reached from outside it.
Direction: an adapter-declared (or plugin-declared) "externally invoked" fact for
host-import surfaces, the same shape `annotate_symbols`' externally-consumed marking
already has — the mechanism exists, what's missing is a producer that recognizes the
boundary. Acknowledged with a file pragma in `plugin_host.rs`.

**Update:** the general form of that fact now exists as `kndo.toml`'s `[[externally-invoked]]`
(§§10–13), matching `Declaration::markers` — so any entry point that CARRIES a marker is
covered with no producer at all. This case still isn't: the host-import functions carry no
attribute distinguishing them from ordinary ones, so there is nothing to match on. The pragma
stays until either the boundary grows a marker or a producer recognizes it structurally.

## 2. Proc-macro derive dispatch (rkyv)

`crates/kndo-core/src/rkyv_support.rs`'s wrapper impls (`SmolStrAsString`) are invoked by
code a derive macro generates (`#[rkyv(with = …)]`): the call sites exist only in the
macro expansion, which extraction never sees. The attr-ident scan keeps the *type* alive
(an identifier inside an attribute is a read), but the impl *members* the generated code
calls have no incoming edges. Root cause: dispatch through generated code is invisible to
source-level extraction by construction. Direction: adapter-curated knowledge per derive
ecosystem — "a type named in `#[rkyv(with = …)]` has its trait-impl members
machinery-invoked," the same curated-list pattern as `implicitly_invoked` for operator
overloads/formatting hooks. Acknowledged with a file pragma in `rkyv_support.rs`.

## 3. Field-access recall on inferred-typed locals

A struct whose fields are only ever read through a local of *inferred* type never shows
field usage: in `let entry = parse_entry(..); entry.path`, the receiver's type comes from
the callee's return type, which the extraction-side `TypeEnv` doesn't chase — so the field
reference lands in the duck fallback (or nowhere), and `internal-only` sees "no use
requires this visibility". Live examples in this repo: `gitutil::TreeEntry`
(consumed in `discovery.rs` through locals), `rollup::DirRollup`, and the plugin sink item
types (`ContributedRoot`/`ContributedEdge`, read via `root_sink.items` in `graph.rs`).
Direction: propagate declared types through let-bindings whose initializer has a known
shape (a call to a function with a declared return type, a struct literal) — bounded type
propagation, not inference. Acknowledged with declaration pragmas at the affected types.

## 4. Recall asymmetry — the regression corpus

The mirror image of #3, kept as a regression corpus for whoever implements it:
`gitutil::BlobFetcher::spawn` has the same consumed-through-inferred-locals shape and
draws **no** finding today (its usage happens to resolve through a path the fallback
catches). Any change to reference recall should check both directions on these sites — the
goal is symmetric behavior, not moving the false positives around.

## 5. Duplicate-group label ambiguity

A structural-clone group's instance labels are `path#Owner.name` selectors. Two identical
methods with the same name on the same owner type but in *different impl blocks* (the
generated-vs-hand-written split of `HostViewData` members in `kndo-plugin-api`) produce
two indistinguishable labels — the finding is correct, but the reader cannot tell which
impl block each instance lives in. Direction: selectors (and `related` entries) could
carry the impl block's span or a disambiguating qualifier when `qualified_name` collides
within one file. Cosmetic; recorded so the "same label twice" report isn't mistaken for a
detector bug.

## 6. The kndo-core module cycle (legal structure, silent by policy)

`kndo-core`'s module graph contains one large file-level import cycle — dozens of files
(engine ↔ graph ↔ analysis ↔ suppression all reference each other's types). Under Rust's
`Idiomatic` cycle tolerance this emits **nothing**: an idiomatic cycle is true information
about legal structure, and kndo does not dress information up as a defect (tolerated
cycles don't feed health's cycles axis either). Listed here so the *absence* of a cyclic
finding on this repo isn't triaged as a detector gap — the structure is real and remains
visible through the graph itself (`kndo query`/doctor), just never as a finding.

## 7. Intra-package visibility leaks (narrower than the four-bucket ladder)

`private-type-leak` accuses only items whose visibility rung is `surface_transitive` — items
that genuinely cross the package boundary. An item visible to a *sibling module* that names a
type private to its own module is a real leak by the letter of the language's rules, and is
now silent: Rust's `pub(super) fn unset_waker` in tokio's `task::state`, returning a
`state.rs`-private alias that its sibling `task::harness` caller cannot spell.

Root cause: the region such an item is visible to sits strictly between `File` and `Package`,
and `VisibilityScope` has no rung there — adapters over-approximate a top-level `pub(super)`
as `pub(crate)` for exactly that reason (`kndo-adapter-rust`'s `restriction_level`). With that
approximation the model cannot tell the tokio case apart from the far more common inverse,
where every caller that can reach the item can also name the type (ripgrep's
`flags::parse::lookup` against the private `Flag` trait, serde's `de::deserialize_custom`
against `Parameters`) — a field audit found the harmless shape three times for each real one.
RFC 0012 §2 is explicit about which way to degrade when the model cannot prove the accusation,
so the gate stays until the scope exists.

Direction: a module-subtree scope in the ladder (`VisibilityScope::Module`, anchored on the
declaring file's `unit` — Rust already keys units on the module path, RFC 0012 §8), plus
adapters emitting `pub(super)`/`pub(in path)` as that rung instead of collapsing it upward.
`scope_contains_site` gains one arm; the containment comparison this analysis already performs
then decides the case exactly, with no gate needed.

## 8. A brace-imported submodule is not a usable qualifier

`use crate::internals::{attr, check, Ctxt, Derive};` followed by `check::check(cx, …)` binds
nothing: `check` reads as dead, and so does everything only it reaches. In serde this killed
`internals::check` and the whole family of `check_*` helpers it calls.

Extraction is correct and was verified — the Rust adapter emits the import with
`specifier: "crate::internals"` and bindings `[attr, check, Ctxt, Derive]`, plus the reference
`check` with `scope_context: Some("check")`. The gap is in resolution: assembly registers a
qualifier for the specifier's own last segment (`internals` → `internals/mod.rs`) but not for
each brace member, so the qualifier `check` matches nothing and the qualified reference falls
to the duck fallback, which finds no member of that name. The one-hop that would close it is
real and available in the data: `internals/mod.rs` itself imports `check.rs` under the name
`check` (its `mod check;`), so the chain is *binding on the target's own import table*.

Direction, and the reason it is not simply done: the obvious shortcut — extend the specifier
with the binding name and re-resolve `crate::internals::check` — requires the core to know
that `::` joins path segments, which is exactly the language knowledge the ignorance rule
forbids it. (One such separator is already hardcoded in `assemble.rs`'s qualifier fallback;
that is a wart to remove, not a precedent to widen.) The language-blind version needs no
separator at all: resolve every file's imports into a `file → (binding name → target file)`
map first, then let a qualifier that binds to file F and names one of F's own import bindings
follow it one hop. That splits phase 3b's single per-file pass into an import pass and a
reference pass — a restructure, not a patch, which is why it is recorded here rather than
attempted in passing.

Worth weighing before scheduling: `use path::{submodule, Type}` is a very common Rust shape,
so this likely suppresses real recall across every Rust codebase, including kndo's own.

## 9. False negative: path references in prose

When `internal/perf-baseline.json` replaced `docs/perf-baseline.json`, four Markdown
documents kept pointing at the dead path and nothing flagged them: kndo extracts no
references from prose, so a path-shaped string in documentation participates in no
resolution and can go stale silently. Direction: a lightweight docs adapter (or plugin)
extracting path-shaped tokens from Markdown as `Possible`-confidence references — enough
for a "documentation references a path that no longer exists" hygiene verdict without
pretending prose is code.

## 10. Framework dispatch: component scan, HTTP routing, DI wiring (LIMIT — covered)

Spring instantiates a `@Controller`/`@Service`/`@Repository`/`@Configuration` class by scanning
the classpath and calls its methods from a servlet dispatcher keyed on `@GetMapping`. No source
reference exists in either direction, and the class is either `unused` or — when only its own
`@WebMvcTest` calls its methods — `test-only`. spring-petclinic showed both shapes at once
(`CacheConfiguration` unused, `OwnerController` and `CrashController` test-only), and Exposed's
sample app the same for a `@Repository`-annotated `UserDaoImpl`.

Not a detection bug: the wiring genuinely is not in the source. **Covered** by
`[[externally-invoked]]` with the stereotype names — on petclinic that is `test-only` 6 → 0 and
`unused` 3 → 2, with nothing new reported. What stays behind is §14.

## 11. Reflective test and benchmark harnesses (LIMIT — covered)

JUnit calls `@BeforeEach`/`@AfterEach` from the runner; Guava's own testlib discovers `test*`
methods by name at runtime through `MapTestSuiteBuilder`; Caliper selects `@Param` enum
constants by iterating `values()`. mockito's `TestBase5.cleanUpConfigInAnyThread` and guava's
`MapGetOrDefaultTester.test` are the audit's instances.

The annotated ones (`@BeforeEach`, `@Test`, `@Param`) are **covered** by
`[[externally-invoked]]`. The name-convention ones are not: `MapTestSuiteBuilder` finds methods
by the `test` prefix, which is a *runtime* string operation with no marker to match on.
Direction for those: a plugin, which can see the builder call and the naming rule together —
the same shape `kndo-plugin-express` uses for its convention-named entries.

## 12. Bytecode instrumentation (LIMIT — covered)

A ByteBuddy `@Advice.OnMethodEnter` method body is *inlined* into instrumented target methods at
class-generation time; it is never invoked as a Java call and never will be. mockito's
`ForHashCode.enter` is the case. **Covered** by `[[externally-invoked]]` — this is exactly what
the mechanism exists for, since no amount of analysis will ever find a call site that does not
exist.

## 13. Consumers in another repository (LIMIT — partly covered)

Koin's `@Scoped` annotation and `Single.binds` parameter are read by `koin-ksp-compiler`, which
lives in a different repository; koin's `binds()` DSL function is documented for library
consumers with no in-repo caller. The annotation halves are **covered** by
`[[externally-invoked]]`. The DSL function is not, and is really §15: published public API.

## 14. Runtime-config string references (GAP)

A file named only from a config or template — never from code — is invisible:
- `WKExtensionDelegateClassName` in an `Info.plist` names Alamofire's `ExtensionDelegate` class
  as a string; the WatchKit runtime instantiates it.
- Thymeleaf's `th:href="@{/resources/css/petclinic.css}"` in `layout.html` is the only reference
  to petclinic's stylesheet — the two `unused` findings left there after §10 is configured.
- vite's `bin/vite.js` does `import('../dist/node/cli.js')`, a built artifact that maps back to
  `src/node/cli.ts` only through vite's own rollup entry config.

Direction: a plugin per ecosystem, which is what the plugin content channel exists for — it can
read exactly the non-source files (`Info.plist`, `templates/**`, `rollup.config.*`) the language
graph never sees, and contribute the root or the edge. Not `[[externally-invoked]]`: there is no
marker on the declaration to match, the name lives in the other file.

## 15. A published library's public API with no in-repo consumer (POLICY, not a gap)

ripgrep's `grep-searcher` exports `Bytes` and `Lossy` sinks that nothing inside the repository
uses; koin's `binds()` and its Compose-Navigation3 module are the same shape. Library mode
already roots a publishable package's public surface, so these fire only where the promotion
does not reach — a workspace member whose manifest says nothing about being published, a module
whose entry point is not declared.

Recorded here because the *verdict* is a policy question, not a mechanic: for a crate that is
published, "no consumer in this repository" is not evidence of anything. Where library-mode
promotion covers it, this never fires; where it doesn't, the honest fix is at the promotion
rule, not at the analysis.

## 16. Multi-release and multi-source-set variants of one class (GAP)

retrofit ships `DefaultMethodSupport` three times — `main/java`, `java14`, `java16` — for the
multi-release-jar pattern; exactly one is on the classpath at runtime, and `Reflection.java`
calls it by its single name. kndo flags all three unreachable, having resolved the call to
none of them.

This is a NEAR MISS of a mechanism that already exists: same-unit twins (RFC 0012 §8) is exactly
this shape — several declarations of one name that are alternatives, all live under the union of
configurations — and it already covers Go's `//go:build` files, Rust's `#[cfg]` alternates, and
Kotlin's `expect`/`actual`. It does not fire here because the three variants live in three
different source roots and so carry three different `unit` keys, not one. Direction: derive the
Java/Kotlin `unit` from the declared package alone (which it already is) *and* make the source
root not part of file identity for this purpose — or, more precisely, let a manifest declare
alternate source roots the way SwiftPM's `unit_overrides` already declares alternate target
paths.

## 17. Version skew read out of BOM-managed and property-declared coordinates (GAP)

`version-skew` compares declared version strings. Three JVM shapes defeat that comparison and
produced findings on every JVM repo in the audit:
- **BOM-managed dependencies** declare no version at all (`platform("io.insert-koin:koin-bom")`
  then bare artifact coordinates). The differing halves are *artifact ids*, not versions —
  spring-petclinic's `spring-boot-starter-actuator` vs `spring-boot-docker-compose`, mockito's
  `mockito-android` vs `mockito-junit-jupiter`, Exposed's 16 "diverging" `kotlin-test-junit5` /
  `kotlin-reflect` entries.
- **Property placeholders** are taken verbatim: Maven's `${spring.version}` and Gradle's
  `$kotlinVersion` compare as literal strings, so `$junit5Version` and `$junit5_version`
  resolving to the same `gradle.properties` key read as skew.
- **The `"*"` default** collides with every real version.

Direction, in order of value: resolve `<properties>` and simple Gradle `val x = "1.2.3"`
declarations from the same manifest; treat an unresolved placeholder as *unknown*, not as a
version (a comparison the code knows it could not perform must not produce a `certain`
finding); and never compare across differing artifact ids in the first place.
