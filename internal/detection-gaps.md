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

## 1. WASM-boundary reachability (host imports) (RESUELTO)

The functions `crates/kndo-plugin-api/src/plugin_host.rs` hands to the component runtime are
called only *through* the WASM boundary: the caller is generated bridge code inside wasmtime,
not any source file the graph walks, so no reference edge can exist and the functions read as
`untested` — while `plugin_compliance.rs` builds and runs a real guest against exactly these
functions every CI run. The file carried a `kndo:allow-file untested` pragma. **Ya no.**

The producer the entry asked for is `kndo:wasmtime` (plugins/wasmtime.md), and what made it
writable is that the fact it needs now has a name. The Rust adapter knew which trait's `impl`
declares each member — `handle_impl` reads it to decide `implicitly_invoked` — and threw the
name away; `Declaration::implements` keeps it, and `AnnotationSink::mark_machinery_impls`
matches a plugin's curated table against it. The boundary is recognized *structurally*, by the
shape `wasmtime::component::bindgen!` gives its generated traits, which is what the original
direction note asked for and could not express.

**Sigue siendo un límite, no un gap, para un ecosistema sin plugin.** Nothing here generalizes
to "any code called through generated glue": each tool's convention is that tool's, which is
why the answer is one plugin per tool rather than a mechanism in the core.

## 2. Proc-macro derive dispatch (rkyv) (RESUELTO)

`crates/kndo-core/src/rkyv_support.rs`'s wrapper impls (`SmolStrAsString`, `TypeExprAsFlat`)
are invoked by code a derive macro generates (`#[rkyv(with = …)]`): the call sites exist only
in the macro expansion, which extraction never sees. The attr-ident scan kept the *type* alive
(an identifier inside an attribute is a read), but the impl *members* the generated code calls
had no incoming edges. The file carried a `kndo:allow-file untested` pragma. **Ya no.**

Same root cause and same answer as §1 — `kndo:rkyv` (plugins/rkyv.md) is the curated ecosystem
knowledge the entry's direction note described, and it is a table of six rows because the
mechanism it plugs into belongs to the core and the grammar it reads belongs to the adapter.
The direction note guessed the knowledge would be *adapter*-curated; that was wrong in a way
worth recording: teaching the Rust adapter about rkyv is exactly the coupling the
adapter/plugin split exists to prevent. What belongs to the adapter is the fact
(`Declaration::implements`), not the interpretation.

**Field test, both entries:** deleting the two file pragmas leaves kndo's own findings
unchanged (50 → 50 at the time of the change, inline suppressions 30 → 7). Without the two
plugins compiled in, the same tree reports 23 `untested` findings across those two files — so
the absence is load-bearing and the plugins are what produce it.

## 3. Field-access recall on inferred-typed locals (RESUELTO)

A struct whose fields are only ever read through a local of *inferred* type never showed field
usage: in `let entry = parse_entry(..); entry.path`, the receiver's type comes from the callee's
return type, which the extraction-side `TypeEnv` didn't chase — so the field reference landed in
the duck fallback (or nowhere), and `internal-only` saw "no use requires this visibility". Four
live cases in this repo, each acknowledged with a declaration pragma. **Los cuatro pragmas ya no
existen.**

**La primera mitad** (RFC 0012 §3-ter): `RawMemberType::owner` pasó a `Option`, donde `None`
significa *función libre* — "llamar a esto evalúa a T", el mismo enunciado sobre el tipo de un
valor que `Some(owner)` hace sobre un miembro, así que el core lo camina con la misma maquinaria
de cadena, aplicada a la BASE del puntero. Con eso, más que la base de un puntero pueda ser un
**qualifier** y no un símbolo (Rust llega a funciones libres por su módulo constantemente), y más
`#[derive(Default)]` como hecho declarado y la variable de un `for` proyectando el elemento del
iterable, cerraron `rollup::DirRollup`, `ContributedRoot` y `ContributedEdge`.

**La segunda mitad** (RFC 0012 §3-quater) cerró `gitutil::TreeEntry`, que necesitaba un cambio de
contrato porque **el hecho no era representable**: se lee por `ls_tree(..).map_err(..)?` y después
se itera, y el tipo del elemento es un parámetro de un parámetro
(`Result<Vec<TreeEntry>, GitError>`). `yields` guardaba un nombre base más una lista de un nivel,
así que el `TreeEntry` no estaba en los hechos — se había perdido al aplanar, y ninguna proyección
puede recuperar lo que nunca se guardó. Aplanar no era un detalle de representación: era la causa.

Ahora `yields` es un `TypeExpr` (`Named { name, args }` | `Param(N)` | `Unknown`) y el estado de
la cadena es un tipo, no un símbolo — porque `Vec<TreeEntry>` es un tipo que este proyecto no
declara y la cadena igual tiene que caminar *a través* de él. Sobre eso, dos piezas más: los
hechos sobre los genéricos que **provee el lenguaje** viven en el descriptor
(`builtin_member_types`, el segundo tier — el único que puede aplicar cuando la cabeza no resuelve
a ninguna declaración), y `Param(N)` deja que un hecho enuncie una *relación* ("sigue siendo un
`Result` sobre el mismo argumento 0") que los argumentos del receptor vuelven concreta. Iterar no
necesitó sintaxis nueva: es un segmento de miembro común cuyo nombre elige el adapter en los dos
lados (`@element`), declarado por contenedor — un mapa no lo declara y su variable de loop
simplemente no tipa, en vez de tipar mal.

**Medición.** kndo sobre sí mismo queda en 37 findings / health 96.6 — idéntico al baseline — con
**cero pragmas de §3**. Que el número no se mueva es el punto: los cuatro tipos dejaron de
necesitar supresión porque el grafo ahora ve a sus consumidores, no porque se haya silenciado
nada. La única supresión que quedó en pie es de otra especie y lo dice: `FlatAtom`
(`rkyv_support.rs`) es `pub(crate)` porque Rust lo exige para el tipo asociado de un impl
`pub(crate)`, y el angostamiento que el finding aconseja no es expresable.

**Lo que sigue fuera de alcance, dicho explícitamente.** Los genéricos declarados por el usuario
(`struct Wrapper<T> { inner: T }`) necesitarían `Declaration::type_params` para sustituir `T`
desde el receptor. El diseño es forward-compatible — la sustitución ya existe, sólo falta de dónde
sacar los nombres — pero nada en el corpus lo pide, y agregarlo ahora sería adivinar. Y un local
anotado `Vec<T>` no tipa su variable de loop: el binding del `TypeEnv` guarda sólo el nombre base
(es lo que termina siendo un `scope_context`), así que el argumento ya se perdió antes.

## 4. Recall asymmetry — the regression corpus

The mirror image of #3, kept as a regression corpus for whoever implements it:
`gitutil::BlobFetcher::spawn` has the same consumed-through-inferred-locals shape and
draws **no** finding today (its usage happens to resolve through a path the fallback
catches). Any change to reference recall should check both directions on these sites — the
goal is symmetric behavior, not moving the false positives around.

**Medido para §3, en sus dos mitades.** `BlobFetcher::spawn` sigue sin producir finding, y los
otros seis targets del audit (serde 348, alacritty 994, axios 72, Exposed 1128,
kotlinx.coroutines 2904, vapor 764) no se movieron en ninguna dirección — ni un finding nuevo ni
uno perdido — ni cuando aterrizó `call_yield` ni cuando la cadena pasó a llevar un tipo. La
simetría es el resultado, no una intención.

Vale registrar lo que atrapó la segunda medición: la propia herramienta reportó `type_param_names`
y `generic_param_names` como `unused` apenas `type_expr` los reemplazó. Eran código muerto que yo
había dejado, y kndo lo encontró antes que la revisión — el dogfood haciendo su trabajo.

## 5. Duplicate-group label ambiguity (RESUELTO)

A structural-clone group's instance labels were `path#Owner.name` selectors. Two identical
methods with the same name on the same owner type but in *different impl blocks* (the
generated-vs-hand-written split of `HostViewData` members in `kndo-plugin-api`) produced two
indistinguishable labels — the finding was correct, but the reader could not tell which impl
block each instance lived in.

Fixed where the ambiguity actually was. The finding's `related` entries carried the two
distinct spans all along, so the DATA was never ambiguous; only the rendered message was. The
message now names the trait whose implementation declares each member — `Declaration::implements`,
the fact §1/§2's plugins introduced — and falls back to the declaration's start line where
there is no trait to name or where both instances share one (two `#[cfg]` alternates).

The distinguisher deliberately does NOT enter the selector, which is the finding's identity
(`finding_id`'s discriminator): a line number in an id would churn the baseline every time
anything above the clone moved. Identity stable, prose readable — which is the same split
§5-bis asks for.

## 6. The kndo-core module cycle (legal structure, silent by policy)

`kndo-core`'s module graph contains one large file-level import cycle — dozens of files
(engine ↔ graph ↔ analysis ↔ suppression all reference each other's types). Under Rust's
`Idiomatic` cycle tolerance this emits **nothing**: an idiomatic cycle is true information
about legal structure, and kndo does not dress information up as a defect (tolerated
cycles don't feed health's cycles axis either). Listed here so the *absence* of a cyclic
finding on this repo isn't triaged as a detector gap — the structure is real and remains
visible through the graph itself (`kndo query`/doctor), just never as a finding.

## 7. Intra-package visibility leaks (RESUELTO)

`private-type-leak` accused only items whose visibility rung was `surface_transitive` — items
that genuinely cross the package boundary. An item visible to a *sibling module* naming a type
private to its own module is a real leak by the letter of the language's rules, and was silent:
Rust's `pub(super) fn unset_waker` in tokio's `task::state`, returning a `state.rs`-private
alias that its sibling `task::harness` caller cannot spell.

The gate was standing in for a comparison the model could not make. Two things had to be true
before it could go, and the second was not in the original diagnosis:

1. **A rung for the region.** `VisibilityScope::Module` — a unit and its subtree — with the
   anchor coming from the declaration (`Declaration::visible_in_unit`) and the tree from
   `FileFacts::unit_parent`. `pub(super)` stopped being widened into `pub(crate)`.
2. **The adapter had to stop lying about `private`.** This is what the catalogue missed. Rust
   privacy is module-**and-descendants**, and `internal/adapters/rust.md` said so while mapping
   it to `File` scope anyway, because the ladder had nowhere else to put it. With `private`
   modelled as a file, ripgrep's `flags::parse::lookup` still read as leaking `flags/mod.rs`'s
   private `Flag` — a type every module under `flags` can name perfectly well. The Module rung
   alone would have swapped one false positive for another.

**And the comparison itself was wrong in a way the gate had been hiding.** A scope means nothing
without the thing it is relative to: two `File` scopes in different files, or two subtrees
anchored at different depths, are disjoint regions an enum comparison reads as equal. The
analysis now asks one question — *does the region the TYPE is visible in reach everywhere the
item promises itself to?* — as `graph::region_covers`, region against region. That single test
replaced the gate and the three-way scope comparison both.

**Medición**, and it is the whole argument:

| repo | antes | ahora | |
|---|---|---|---|
| tokio | 0 | **6** | `unset_waker` and `set_join_waker` among them — the case this entry exists for |
| ripgrep | 0 | 1 | `flags::parse::lookup` is **not** one: `WalkParallel::run` naming the private `FnVisitor` is, and it is real |
| serde | 0 | **0** | with the rung but before the private-is-a-subtree fix it was 16, every one of them noise |
| alacritty | 0 | 2 | both real (`Window::new` returns a module-private `Result` alias) |
| axios, Exposed, vapor | — | unchanged | no language but Rust declares a Module rung |

`internal-only` gained a finding class from the same correction: a `pub(crate)` item used only
within its own module subtree really can be plain `private` in Rust, which the `File`
approximation could never say. On kndo itself that is +17 true findings, four of them about
constants this very commit introduced.

**Lo que queda fuera.** `pub(in path)` still widens to `pub(crate)` — the adapter does not
resolve the path to a unit key (7 occurrences in all of tokio). And `internal_only` compares
rung scopes, so it can no longer suggest narrowing `pub(super)` to `private`: both are `Module`
and differ only by anchor, which that comparison does not see. A silence, not an accusation, and
the fix is to make its rung walk region-aware the way this analysis now is.
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

## 9. Path references in prose (MEASURED — the proposed direction is wrong; the narrow half shipped as a gate)

The original entry: when `internal/perf-baseline.json` replaced `docs/perf-baseline.json`,
four Markdown documents kept pointing at the dead path and nothing flagged them. The proposed
direction was a lightweight docs adapter extracting path-shaped tokens from Markdown as
`Possible`-confidence references.

**Measured before building, and the measurement killed the adapter.** Three findings, on this
repository:

1. **The motivating case no longer reproduces.** The four documents were fixed. The only two
   remaining mentions of `docs/perf-baseline.json` are the two documents *describing this gap*
   — not stale pointers.
2. **Prose path tokens are noise.** 195 backticked path-shaped tokens; **63 resolve to
   nothing, and essentially none is a defect**: examples from other repositories
   (`crates/searcher/src/sink.rs` is ripgrep's), invented illustrations
   (`com/foo/bar/Widget.java`), paths that exist in a *user's* project
   (`coverage/lcov.info`, `.kndo/plugins/your.wasm`), module-relative shorthand
   (`analysis/reachability.rs`). Firing on those is 63 findings and no fix — the same
   collision arithmetic that decided §14-bis, at a worse ratio.
3. **Claiming `**/*.md` at all breaks the dogfood gate.** It makes every document eligible for
   `unused`, and 15 of this repository's 74 Markdown files are linked from nothing:
   `CLAUDE.md`, `CONTRIBUTING.md`, `SECURITY.md`, every adapter spec, several RFCs. All
   legitimate. Exempting them needs a `FileRole::Docs` that does not exist — a vocabulary
   change to make viable a component with no measured case.

**What shipped instead.** A Markdown **link** is different in kind from a prose path: it is the
author asserting that the path resolves, the closest thing prose has to an import. 189 of them
exist here, and two were broken — both introduced by the commit that moved the plugin specs
into the book, and caught by nothing. `crates/kndo/tests/doc_links.rs` is now a named gate that
checks every one of them, and it would have failed on that commit. The scanner blanks code
spans and fenced blocks first: `internal/adapters/go.md` writes Go generics as
`` `func F[T any](x T)` ``, and the naive scan read `](x T)` as a link to `x` — a path inside
backticks is quoted, not claimed, which is the same rule the whole entry turns on.

Prose is deliberately not checked. That is the measurement above, not an omission. If a real
stale-prose-path case ever appears in the field, it reopens with its own evidence.

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

## 14-bis. A name written inside an ATTRIBUTE, not a config file (RESUELTO)

§14's siblings all live in a file the language graph never opens. This one lives in the source
itself, and was invisible anyway: `#[serde(skip_serializing_if = "usize_is_zero")]` names a
function whose only caller is code serde's derive macro generates. The attr-ident scan (§2)
does not reach it — a `string_literal` is not an `identifier` token, and `emit_attr_path`
requires an uppercase initial for a lone segment besides.

kndo reported it **about its own tree**, and the tree was changed to avoid the finding: an
envelope field became `Option<usize>` purely so no named predicate would be needed. A tool
whose own source is shaped around what it cannot see is the worst version of this gap, so it
is the one that got fixed.

**Three layers, and the middle one is the point.** The adapter records
`FileFacts::string_attr_args` — attribute head, key, literal, decorated declaration — and
emits no reference. The core carries the fact to `FileNode`, `GraphView::attr_strings_in` and
the WASM import `attr-strings-in`. `kndo:serde` holds the key table and contributes the edge.

**Why the adapter cannot do it alone, measured.** Over the attributes the Rust adapter already
scans: serde writes 482 identifier-shaped `key = "literal"` pairs, 248 of whose values collide
with a real declaration in the crate, and only 176 sit under a key serde resolves as a path —
leaving **72 pairs** where `tag = "type"` or `rename = "b"` merely happens to match something
declared elsewhere. tokio adds 2, alacritty 4, ripgrep 0. An adapter treating a collision as a
reference would contribute 78 keep-alive edges across the corpus to close one real case, and
every keep-alive edge silences a true finding. RFC 0012 §2 says degrade toward keeping alive,
but keeping alive blindly at that ratio is how a tool stops finding things. Telling
`skip_serializing_if = "f"` from `rename = "f"` requires knowing what serde is — the coupling
§0.2 of the fix plan exists to prevent.

**Field test:** +0/−0 across all thirteen corpus repositories (the 0-FP prediction, confirmed),
and `dogfood` green with the `Option<usize>` workaround reverted — red the moment
`kndo:serde`'s key table is emptied, so the mechanism is what carries it.

Generalizes: Java and Kotlin annotations take string arguments and Swift has attributes; the
fact is optional per adapter, like `string_call_args`.

## 15. A published library's public API with no in-repo consumer (POLICY — Rust instance no longer reproduces)

ripgrep's `grep-searcher` exports `Bytes` and `Lossy` sinks that nothing inside the repository
uses; koin's `binds()` and its Compose-Navigation3 module are the same shape. Library mode
already roots a publishable package's public surface, so these fire only where the promotion
does not reach — a workspace member whose manifest says nothing about being published, a module
whose entry point is not declared.

Recorded here because the *verdict* is a policy question, not a mechanic: for a crate that is
published, "no consumer in this repository" is not evidence of anything. Where library-mode
promotion covers it, this never fires; where it doesn't, the honest fix is at the promotion
rule, not at the analysis.

**Mechanic verified, and the Rust instance is gone.** For Cargo, `publish = false` is the
explicit opt-out and its absence means publishable — `manifest.rs` reads it in exactly that
direction (`publish = ["registry"]` restrictions stay publishable), and a non-private package
roots its `[lib] path` (default `src/lib.rs`) at `Certain`. Re-measured against the clone:
`Bytes` and `Lossy` are still declared in `crates/searcher/src/sink.rs`, `grep-searcher`'s
manifest still carries no `publish` key, and kndo now reports **zero** `unused`/`internal-only`
findings anywhere in that crate — the promotion reaches declarations across the crate's module
tree, not just the root file. What remains under this entry is the policy statement itself, for
the shapes where promotion genuinely has nothing to key off.

## 15-bis. A publishable Maven module whose layout it declares (FIXED — own pom, and inherited)

§15 says a published library's public surface is rooted by library-mode promotion, and records
the Rust instance as gone. The JVM half had a hole underneath it: promotion walked a hardcoded
`src/main/java`, and Maven lets a module *declare* where its code lives. A module that declares
a different one promoted **nothing**, so every public class in it read as `unused` — the
promotion did not fail loudly, it simply found no files.

**Measured.** Two Maven modules, identical code and identical publishability, differing only in
that one declares `<sourceDirectory>src</sourceDirectory>`: the declaring one reported `unused`
on its public API, the conventional one reported nothing. On guava — which declares exactly
that, with tests in a sibling `test` — kndo reports 9,576 `unused` and 14,877 `internal-only`,
including 1,174 `testXxx` methods in `guava-testlib`'s testers.

**Fixed for a pom's own declaration**: `<build><sourceDirectory>` now replaces the convention,
with `${basedir}` and the pom's `<properties>` interpolated and any build-time placeholder
falling back rather than guessing (adapters/java.md §4).

**Not fixed, with its own evidence: inheritance.** guava declares it once in `guava-parent` and
every module inherits, so this fix does not move guava at all. The adapter is handed one
manifest's text at a time and `ResolveCtx` exposes paths, not contents — resolving an inherited
declaration needs the assembly-side pattern `ManifestDependency::inherited` already uses, which
means a `ManifestFacts` field and moving root promotion off the adapter. That is a contract
change and is written up as adapters/java.md §7.2 rather than approximated here.

**Note for whoever picks that up**: the plan's `kndo:guava-testlib` plugin was scoped against
guava's `unused` numbers. Those numbers are this entry's, not a plugin's. Re-measure after the
inheritance fix before writing the plugin — the mechanism it would encode is JUnit 3's
reflective `TestSuite(Class)` (guava-testlib's testers descend from `TestCase`), not anything
guava-testlib knows, and it may have nothing left to do.

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

## 5-bis. An `internal-only` message that asserts more than its evidence (RESUELTO)

Surfaced by §16's fixture. `internal-only` deliberately lets a weak (`Possible`) reference from
wider than the strong requirement demote the *verdict's confidence* rather than widen the
requirement — the right call, and RFC 0012 §2's direction. But the message it printed was
unchanged: "declared package-private but **only used within its own file** — private would
suffice". On retrofit's multi-release variants there IS a cross-file reference; it is a
`possible`-tier duck-fallback hit, which is why the verdict is `possible` too. The tier was
honest and the sentence was not, and a reader who acted on the sentence broke the build.

The two evidence states now have two sentences. With `weak_wider` the finding says that every
confidently resolved use is within the scope, that weaker matches point outside it, and that
the narrower rung would suffice *only if those are not real uses* — which is exactly what the
analysis knows. Pinned by
`only_possible_confidence_cross_file_reference_demotes_not_exempts`, which now asserts the
message never borrows the certain tier's words.

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

## 18. A file two roots reach, coloured by the one that loses (FIXED)

**The claim `test_only.rs` made and did not implement.** Its own module doc read "fires on nodes
coloured `TestOnly` — reachable, just never from a production **or tooling** root". Reading the
colour is not that test. Colours resolve by first-match precedence
(`Production > TestOnly > ToolingOnly`, `reachability.rs`), so a node reached from a test root
*and* a tooling root wins `TestOnly` — and was accused of being code "production never came" for,
while an xtask, a build script or a `module-info.java` was using it the whole time.

`ReachabilityMap` already keeps `reached_possible` per root kind for exactly this — its own doc
says the winning colour "is exactly wrong" for a query that needs the kind that lost, and
`untested` already consults `reachable_from` that way. `test_only` now does too, in both the file
and the symbol loop.

**How it surfaced.** kndo's own `dogfood` gate, the moment `xtask` grew a `src/lib.rs` and became
the workspace's first lib+bin package: 11 findings, all of them `xtask/src/package.rs` and its
symbols, reached from `xtask/src/main.rs` (tooling) *and* `xtask/tests/release_channels.rs`
(test). Import resolution was never at fault — `resolve_bare`'s `same_package` path resolved
`use xtask::package` correctly, at `Certain`. The graph was right; the analysis read it wrong.

**Measured**, one binary per cause, diffed by `(category, path, symbol)` over the five corpus
repos holding 98% of its baseline `test-only` findings (760 total: tokio 350,
kotlinx.coroutines 296, RxSwift 82, alacritty 12, Exposed 12):

| repo | before | after | removed | added |
|---|---|---|---|---|
| tokio | 2946 | 2946 | 0 | 0 |
| kotlinx.coroutines | 2577 | 2576 | **1** | 0 |
| RxSwift | 2175 | 2175 | 0 | 0 |
| alacritty | 906 | 906 | 0 | 0 |
| Exposed | 783 | 783 | 0 | 0 |

**−1 / +0.** The one removal is `reactive/kotlinx-coroutines-reactor/src/Convert.kt`, whose only
consumer is `module-info.java` — `by_color: {production: 0, test-only: 0, tooling-only: 1}`. A
file with zero test reach was being reported as test-only; that is the defect, not a suppression
of a true positive. Narrow in the field and one-directional by construction (the fix is an added
`continue`, so it can only ever remove), which is why the eleven findings on kndo itself are the
bulk of what it closes.

Pinned by `a_file_a_tool_and_a_test_both_use_is_not_test_only` and
`a_symbol_a_tool_and_a_test_both_use_is_not_test_only`. The symbol fixture keeps its file
`Production`-coloured on purpose: with a `TestOnly` file the rollup skip ("the file-level finding
already covers every symbol in it") exempts the symbol before the colour is read, and the test
would pass without the guard it exists to pin — verified by removing each guard in turn.


## 15-ter. …and the declaration was in the PARENT pom (FIXED)

§15-bis closed the case where a module's own pom declares `<sourceDirectory>`. The other half —
a declaration the module **inherits** — is the one guava actually has, and it was the larger of
the two by an order of magnitude: `guava-parent` declares `<sourceDirectory>src</sourceDirectory>`
once and all ten modules inherit it, so against the hardcoded `src/main/java` kndo found **zero
production roots in guava** and 88% of its findings were downstream of that.

`internal/adapters/java.md` §7.3 has the mechanism, the Maven fidelity rules (empty
`<relativePath/>`, parent-coordinate matching, per-module joining) and the full measurement.
The short version: **guava −11,444 / +2,435** unique findings, **0/0 on retrofit,
spring-petclinic, Exposed and kotlinx.coroutines**, and 99.3% of the additions are files that
used to be reported `unused` now being judged instead (2,344 of them `untested`).

The part worth carrying forward is why §7.2 mis-sized it. It was recorded as a contract change —
a `ManifestFacts` field, an assembly-side pool, root promotion moved off the adapter — to protect
a caching invariant that was never at risk: **manifest extraction is not cached**, and the
incremental patch **refuses on any changed manifest**. Both facts were already in the code and
neither was checked before the design was written down. The actual change is one optional
capability on the manifest-extraction `ResolveCtx` (`read_manifest`), with every Maven rule
staying in the adapter. Measuring the constraint before designing around it would have saved the
whole detour.

## 19. `kndo:guava-testlib` — the plugin measurement closed without building

W6/E5 planned a `kndo:guava-testlib` plugin for "`MapTestSuiteBuilder` discovers methods at
runtime", blocked on the Java adapter emitting `string_call_args`. Java emits them now (commit
`48f0728`), so the block is gone — and with it gone, three measurements say there is nothing to
build. Recorded here rather than left as a to-do, because an unbuilt item with no reason attached
is one somebody rediscovers and builds.

**1. The stated mechanism was not the mechanism.** guava's suite builders register testers with
**class literals** (`MapTestSuiteBuilder.using(...).named(...)` taking `Class<?>`), not strings;
the method discovery underneath is **JUnit 3**'s `testXxx` convention, not anything
guava-testlib invents. `string_call_args` — the fact E5 was waiting on — is irrelevant to it.

**2. The volume was the Maven gap.** The 1,174 findings that motivated E5 were part of the 9,576
`unused` produced by root promotion ignoring an inherited `<sourceDirectory>` (§15-ter). Fixing
that removed **11,444** findings from guava; `unused` inside the testlib modules fell from that
class to **247**.

**3. What is left is one class's private detail, and config already covers it.** 213 of those 247
are `FreshValueGenerator.generate*` methods, invoked through
`getDeclaredMethods()` + `isAnnotationPresent(Generates.class)`. `Generates` is declared
`private @interface Generates {}` — a private nested annotation, usable by exactly one class,
appearing in exactly two files in the repository (the same file, duplicated for the android
flavor). §0.2 puts "knowledge specific to one concrete **tool**" in that tool's plugin; a private
member of one class is not that, and a shipped plugin for it would serve one repository on Earth.

Measured directly: three lines of `kndo.toml` —

```toml
[[externally-invoked]]
markers = ["Generates", "Empty"]
```

— take testlib's `unused` from **247 to 34**, and the `FreshValueGenerator` methods from **213 to
0**. The project-declared-marker mechanism is exactly the right layer for a reflection registry
whose marker only its own project knows, and it already exists and already works.

**So: not built, and not pending.** The residual 34 are unrelated to the pattern and are ordinary
findings on their own merits.

## 20. `kndo:vite` and `kndo:rollup` — measured, not built, and what they were pointing at

W6/E3 and E4 planned two plugins: `kndo:vite` reading `build.lib.entry` and
`build.rollupOptions.input` from `vite.config.*`, `kndo:rollup` reading `input`/`output` from
`rollup.config.*`. Measured against the two repositories, neither survives — and the measurement
names the real gap, which is neither of them.

**`kndo:rollup` has no case at all.** rollup's own repository: 2,269 findings, of which 219 are
`unused` — 140 of those inside `rust/` (its Rust-based parser) and 33 on `package.json` files.
Nothing points at `rollup.config`. There is no measured defect for this plugin to close.

**`kndo:vite` was aimed at the wrong declaration.** Of vite's 61 `playground/*` config files,
**11** declare an `input`/`entry`/`lib` at all, and those name **`.html` files** through
`path.resolve(dirname, './index.html')` — computed expressions, not literals, the same "the
config is a program" problem Gradle has.

**What actually costs vite 831 findings** is one directory up from the config. 36 of 76 playground
apps have an `index.html` carrying `<script type="module" src="./main.js">` — vite's real entry,
and the web's, used identically by webpack, parcel, esbuild and a plain static site. **83 module
scripts are named that way and 65 of them are reported `unused`**, each the root of a subtree
that reads as dead behind it. Nothing claims `.html`: not the JS adapter, not CSS, not any other.

By §0.2 that is an **adapter** question and not a plugin one — `<script src>` is HTML's own
mechanism for naming a module, no more vite's property than `import` is webpack's. A `kndo:vite`
plugin reading `vite.config.js` would close a small minority of the cases while leaving the
mechanism that produces them unmodelled.

**Built as `kndo-adapter-html` (§20-bis).** The shape that fits is `claim`, not
`claim_manifest`: an HTML document is an **entry point**, not a module — nothing imports a page,
a browser loads it — so the adapter claims it, roots it, and emits its `<script src>`/
`<link href>` as imports.

`claim_manifest` was measured and rejected first: **assembly creates one `PackageNode` per
claimed manifest** (`assemble.rs`, ownership by nearest-manifest-ancestor), so every directory
holding an HTML file would have become its own package and taken package-scoped unit keys,
dependency ownership and surface with it.

## 20-bis. The HTML adapter, and the `untested` flood it exposed

The adapter is ~250 lines: claim `*.html`/`*.htm`, root the document, tag-scan `<script src>`,
`<link href>`, `<img src>`, `<source src>`, `<iframe src>` for local paths. No grammar — every
reference lives in one attribute of one tag, and HTML's error recovery means a "malformed"
document is still one a browser renders. References that leave the project (a CDN URL, `data:`,
`#anchor`, a root-relative `/assets/app.js` whose meaning depends on the server's document root,
a `${...}`/`{{...}}` placeholder) are skipped rather than reported unresolved.

**First measurement said do not ship it.** On vite: −237 `unused` (the recall it exists for) but
**+418** additions, net **+181** findings. 145 of the additions were `untested` **on the `.html`
files themselves** — a page is a production entry by nature, so every document in every web
project would be reported as a test blind spot, forever.

**The flood was a pre-existing gap the adapter would have multiplied ~35×.** `untested` already
reported 1 `.json`, 10 `.css` and 3 `.scss` files across the corpus for the same reason. Its
existing exemption (`files_declaring_only_values`) cannot reach these: it *requires* a file to
declare symbols, deliberately — concluding "nothing to test" from an absence of extracted facts
would silence files for a reason nobody could see, and an adapter that simply failed looks
identical from here.

So the adapter states it positively: `AdapterDescriptor::declares_units_of_testing`, carried onto
`ProjectGraph::testable_languages` like the visibility ladders. `untested` consults it for
exactly one case — **a file that declares nothing, in a language that says nothing is
declarable**. Both halves are load-bearing, and the conjunction is what keeps the existing rule
intact: a `.scss` with a `@function` declares something, so the values-only rule still decides it
and no blanket "stylesheets aren't testable" can silence it. A language the graph never recorded
answers `true` — silence is never an exemption.

**Measured with the exemption**, release binaries, `--no-cache`, by `(category, path, symbol)`:

| repo | before | after | removed | added |
|---|---|---|---|---|
| vite | 1886 | **1848** | 248 | 210 |
| spring-petclinic | 44 | **41** | 3 | 0 |
| axios | 68 | **68** | 0 | 0 |

vite's removals are 237 `unused` (the unrooted entry modules) plus 11 `untested`. Its 210
additions carry **no HTML at all**: 133 `untested` and 60 `unused` on `.js`/`.ts`, 97 of them in
files that used to be reported `unused` — the same dead-to-judged category shift the Maven fix
produced on guava. spring-petclinic's −3 are declaration-less `.scss` files: the pre-existing
class, fixed in a Java project that has no HTML entry at all. axios is untouched.

`GRAPH_SCHEMA_VERSION` 39 → 40 (a new persisted field on the snapshot), not an adapter's
`facts_schema_version`: the graph's own shape changed.
