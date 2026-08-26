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

## 3. Field-access recall on inferred-typed locals (3 de 4 CERRADOS)

A struct whose fields are only ever read through a local of *inferred* type never showed field
usage: in `let entry = parse_entry(..); entry.path`, the receiver's type comes from the callee's
return type, which the extraction-side `TypeEnv` didn't chase — so the field reference landed in
the duck fallback (or nowhere), and `internal-only` saw "no use requires this visibility". Four
live cases in this repo, each acknowledged with a declaration pragma.

**Lo que cerró, y el mecanismo de cada uno.** `RawMemberType::owner` pasó a `Option`: `None`
significa *función libre* — "llamar a esto evalúa a T", el mismo enunciado sobre el tipo de un
valor que `Some(owner)` hace sobre un miembro, así que el core lo camina con la misma maquinaria
de cadena, aplicada a la BASE del puntero un paso antes de donde `chain_hop` actúa sobre un
segmento (RFC 0012 §3-ter). Sobre eso:

- **`rollup::DirRollup`** — pedía que la base de un puntero pudiera ser un **qualifier** y no un
  símbolo: `use …::rollup;` liga el módulo, nunca `directory_rollups`, así que el puntero moría en
  su primer segmento. `pointer_base` resuelve la base contra la tabla de qualifiers y consume el
  segmento siguiente como el símbolo dentro de ese archivo.
- **`ContributedRoot` / `ContributedEdge`** — pedían dos cosas: que `#[derive(Default)]` sea un
  hecho declarado (`RootSink::default()` apuntaba a un miembro que ningún impl declara) y que la
  variable de un `for` proyecte el elemento del iterable.
- **`gitutil::TreeEntry`** — sigue abierto, y es el único. Se lee por
  `ls_tree(..).map_err(..)?` y después se itera: el tipo del elemento es un parámetro de un
  parámetro (`Result<Vec<TreeEntry>, GitError>`) y `yields_params` guarda **un solo nivel**, así
  que el `TreeEntry` no está en los hechos — se perdió al aplanar. Cerrarlo pide que la cadena
  lleve una *expresión de tipo* en vez de un nombre, y que el adapter pueda declarar qué hacen los
  genéricos de la stdlib. Es un cambio de contrato propio con su propia medición; el pragma en
  `crates/kndo-core/src/gitutil.rs` dice exactamente eso.

**Medición.** kndo sobre sí mismo queda en 37 findings / health 96.6 — idéntico al baseline — con
**tres pragmas menos**. Que el número no se mueva es el punto: los tres tipos dejaron de necesitar
supresión porque el grafo ahora ve a sus consumidores, no porque se haya silenciado nada.

## 4. Recall asymmetry — the regression corpus

The mirror image of #3, kept as a regression corpus for whoever implements it:
`gitutil::BlobFetcher::spawn` has the same consumed-through-inferred-locals shape and
draws **no** finding today (its usage happens to resolve through a path the fallback
catches). Any change to reference recall should check both directions on these sites — the
goal is symmetric behavior, not moving the false positives around.

**Medido para §3.** `BlobFetcher::spawn` sigue sin producir finding, y los otros seis targets del
audit (serde 348, alacritty 994, axios 72, Exposed 1128, kotlinx.coroutines 2904, vapor 764) no se
movieron en ninguna dirección: ni un finding nuevo ni uno perdido. La simetría es el resultado, no
una intención.

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

Direction: a module-subtree scope in the ladder (`VisibilityScope::Module`), anchored on the
declaring file's `unit` — plus the anchor itself, which Rust does **not** have today. RFC 0012
§8's table claimed Rust keyed units on the module path; it does not (`FileFacts::unit` is
`None`, `internal/adapters/rust.md` §2, and the RFC row is now corrected). So this direction is
one step longer than recorded: the Rust adapter must first key units on the crate-root-relative
module path, and `FileFacts::unit_parent` must make those keys a tree the core can walk without
knowing any separator. Then adapters emit `pub(super)`/`pub(in path)` as that rung instead of
collapsing it upward.
`scope_contains_site` gains one arm; the containment comparison this analysis already performs
then decides the case exactly, with no gate needed.

## 8. A brace-imported submodule is not a usable qualifier (RESUELTO)

`use crate::internals::{attr, check, Ctxt, Derive};` followed by `check::check(cx, …)` bound
nothing: `check` read as dead, and so did everything only it reached. In serde this killed
`internals::check` and the whole family of `check_*` helpers it calls.

Extraction was correct and was verified — the Rust adapter emits the import with
`specifier: "crate::internals"` and bindings `[attr, check, Ctxt, Derive]`, plus the reference
`check` with `scope_context: Some("check")`. The gap was in resolution: assembly registered a
qualifier for the specifier's own last segment (`internals` → `internals/mod.rs`) but not for
each brace member, so the qualifier `check` matched nothing and the qualified reference fell
to the duck fallback, which finds no member of that name. The one hop that closes it was
always in the data: `internals/mod.rs` itself imports `check.rs` under the name `check` (its
`mod check;`), so the chain is *a binding on the target's own import table*.

**How it was closed.** Not by extending the specifier and re-resolving `crate::internals::check`
— that shortcut requires the core to know that `::` joins path segments, exactly the language
knowledge the ignorance rule forbids it. The landed fix needs no separator at all: phase 3b's
single per-file pass became three (`resolve_imports` → `link_module_bindings` →
`resolve_references`, all three shared by `graph::assemble` and `graph::patch`). The first pass
records, per file, every name an import puts in scope that points at an in-repo FILE
(`graph::ModuleBinding`) — both the brace members that resolved to no symbol (the consumer side)
and the `local_alias` of a file-linking `mod x;`, which carries its name there and in no
bindings at all (the producer side; omitting it was why the hop first shipped inert). The middle
pass then follows one hop: a name bound to file F that F's own table binds again resolves to
where F sends it. One hop, never a fixpoint; non-settling, so a miss falls through to the
in-scope/duck ladder rather than binding the access to the wrong file; and an existing qualifier
always wins, because a direct alias is stronger provenance than a derived hop. RFC 0012 §9.

The patch path needed no signature change: `surface_signature` already hashes a file's complete
import list, so any change to F's imports changes F's surface and forces a full rebuild — a
stale hop is unrepresentable. `patch_equivalence` remains the harness, and `FilePatchMeta`
carries each file's `module_bindings` so an unchanged file contributes its table without being
re-extracted.

**Medido en campo.** serde 363 → 348 findings (`unused` 66 → 52), health 82.8 → 83.7 —
`internals::check` and the whole `check_*` family. alacritty 993 → 994 (`unused` 155 → 150):
six *new* findings, all on code that only became reachable — exactly the shape of a recall fix.
axios, vapor, Exposed and coroutines unchanged; the shape is Rust's. Regression:
`a_brace_member_naming_a_submodule_hops_through_the_module_file` and
`a_direct_qualifier_outranks_the_module_hop` in `graph::tests`, each verified failing without
its half of the fix.

**Y el `::` se fue con ello.** The `rsplit("::")` qualifier fallback in `resolve_imports` — the
core's one piece of hardcoded language syntax — is gone, in its own commit and measured on its
own. Two Rust shapes leaned on it and now state the name themselves in `local_alias`: the
single-qualifier bare path (`helpers::run()`) and the deep one
(`kndo_core::discovery::find_files_named(..)`), both synthetic imports the adapter reconstructs
from a use site and qualifies by a segment only it can identify. The over-approximation the
fallback also carried — registering `b` for `use a::b::{X, Y}`, where Rust does *not* bring `b`
into scope — is simply gone. Whether a qualifier miss settles now follows the import's own
confidence (`Certain` = a statement the file makes, a closed namespace; anything less = the
adapter's reading of a path, which must keep falling through), locked in by
`a_reconstructed_imports_qualifier_does_not_settle_on_a_miss`. Field result: byte-identical
findings on all seven measured targets — the adapter-side alias reproduces the split exactly.

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

## 13-bis. Cross-language attribution (WAS a gap — fixed, and now fixtured)

Recorded because the *absence* of these findings is now load-bearing. A file whose own adapter
would never claim the nearest ancestor manifest used to be charged against it anyway: Jazzy's
`docs/js/typeahead.jquery.js` made `jquery` a phantom dependency of `Package.swift` in all four
Swift repos, and hugo's `docs/` JS imports were checked against `go.mod` — its entire
`undeclared` column. `PackageNode::manifest_claim_languages` plus
`governs_dependencies_of` closes it on both sides (`undeclared` and `dependency_hygiene`), and
`crates/kndo/tests/fixtures/` — the cross-language conformance suite, which registers every
adapter at once — pins it. Neither an adapter's own fixture suite could have: the Swift suite
has no JS adapter to claim the file, the JS suite has no `Package.swift` to misattribute to.

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

## 16. Multi-release variants of one class (WAS a gap — already covered, now fixtured)

retrofit ships `DefaultMethodSupport` three times — `main/java`, `java14`, `java16` — for the
multi-release-jar pattern; exactly one is on the classpath at runtime, and `Reflection.java`
calls it by its single name. The audit saw all three flagged unreachable.

**This entry was stale when it was written.** Java's `unit` is the declared package name alone,
never directory-derived (`kndo-adapter-java`'s `handle_top_level`), and all three variants
declare `package retrofit2;` — so they were always same-unit twins. What was missing was the
twins mechanism itself, which landed for Go's `//go:build` alternates and Kotlin's
`expect`/`actual` *after* the audit ran (RFC 0012 §8). Re-measured on retrofit: all three are
reachable, and the only finding left on them is an `internal-only` on `DefaultMethodSupport.invoke`
— a different verdict, at the `possible` tier, and one §5-bis now covers. Recorded here because
the *absence* of `unused` is load-bearing: `kndo-adapter-java`'s `multi-release-variants` fixture
pins it.

## 5-bis. An `internal-only` message that asserts more than its evidence (GAP)

Surfaced by §16's fixture. `internal-only` deliberately lets a weak (`Possible`) reference from
wider than the strong requirement demote the *verdict's confidence* rather than widen the
requirement — the right call, and RFC 0012 §2's direction. But the message it prints is
unchanged: "declared package-private but **only used within its own file** — private would
suffice". On retrofit's multi-release variants there IS a cross-file reference; it is a
`possible`-tier duck-fallback hit, which is why the verdict is `possible` too. The tier is
honest and the sentence is not, and a reader who acts on the sentence breaks the build.

Direction: when `weak_wider` holds, say what is actually true — no reference stronger than
`possible` requires more than this scope — instead of asserting exclusivity the graph
contradicts. Same family as §5: the verdict is defensible, the rendering overclaims.

## 17. Version skew read out of BOM-managed and property-declared coordinates (FIXED)

`version-skew` compares declared version strings, and three JVM shapes defeated that comparison
on every JVM repository in the audit. The root cause of the loudest one turned out to be a
parsing bug, not a comparison policy:

- **BOM-managed coordinates.** `gradle_dependency_line` split every coordinate on its LAST
  colon. A BOM-managed coordinate has **two** segments, not three, so
  `'org.springframework.boot:spring-boot-starter-actuator'` parsed as *version*
  `spring-boot-starter-actuator` of a dependency named `org.springframework.boot`. That is why
  the findings listed **artifact ids** as diverging versions — spring-petclinic, mockito,
  Exposed's 16 `kotlin-test-junit5`/`kotlin-reflect` entries, koin. Fixed by splitting on
  segment count: two segments is a complete coordinate with no version.
- **Property placeholders.** `${spring.version}` and `$kotlinVersion` were compared as literal
  strings, so two spellings of one `gradle.properties` key read as skew. Fixed by resolving
  against the manifest's own pool (Maven `<properties>`, Gradle `val`/`def`/`var`), and by
  leaving what the file cannot answer unknown.
- **The `"*"` sentinel.** "This manifest states no comparable requirement" was encoded as a
  version that then diverged from every real one. Fixed by making the fact representable:
  `ManifestDependency::version_req` and `DeclaredDependency::version_req` are `Option`. npm's
  `"*"`, which IS a declared requirement, stays a value.

`version_skew` now drops unknowns before grouping — a comparison the code knows it could not
perform cannot produce a `certain` finding — while a real disagreement among the manifests that
DID state a requirement still fires. Measured: spring-petclinic 5 → 0, kotlinx.coroutines 4 → 0,
Exposed 9 → 4, and the four that remain are genuine (`com.h2database:h2` at `2.4.240` in three
manifests against `2.3.232` in a sample).
