# Experiments — the measure-first backlog

Every candidate feature starts here with a question and how to measure it; the number
decides. Killed entries stay, with the number that killed them, so nobody rediscovers
and rebuilds them. Prior data comes from v1's recorded measurements in this repository.

## Killed by measurement (do not rebuild without new data)

### kndo:vite / kndo:rollup plugins
Question: do bundler configs anchor enough roots to justify plugins?
Measurement (v1): only 11 of 61 vite configs in the wild declare an entry;
`kndo:rollup` had no case at all. The HTML adapter replaced the intent.
Reopen only if: config-declared entries become the norm in the ecosystem.

### Markdown/docs adapter (path references in prose)
Question: can prose path references be claimed without noise?
Measurement (v1): 15 of 74 repository Markdown files are legitimately linked from
nothing; claiming `**/*.md` breaks the dogfood gate; exempting needs a `FileRole::Docs`
that does not exist. Two real broken links were caught instead by the `doc_links` test —
links only, prose paths carry too many false positives.
Reopen only with: a `FileRole::Docs` design plus a measured false-positive rate near zero.

### kndo:guava-testlib plugin
Measurement (v1): three measurements said there is nothing to build (recorded to prevent
rediscovery).

### HTML adapter shipping its headline rule alone
Measurement (v1): first cut was net-negative on vite (−237 unused, +418 additions, 145
of them `untested` on the .html files themselves — a pre-existing gap multiplied ~35×).
Fixed only by adding `declares_units_of_testing`. Lesson generalized: a new adapter's
first corpus run gates its ship, not its fixtures.

### An unbounded `MarkerTarget::Unit` (2026-09-09)
Question: should a file-top `#![…]` claiming the unit be honored from any file of it,
rather than only from the file the build enters the unit through?
Measurement: refuted before the corpus, by a fixture already in the tree.
`attribute-dispatch` carries `#![allow(dead_code)]` in `src/scratch.rs` — a module
file, not the crate root — and expects `src/ffi.rs#truly_dead` DEAD. rustc scopes a
lint attribute lexically to the module that carries it, so honoring the claim from
any file would silence a true accusation for a numeric win. The bounded reading ships
(`dispatch::UnitVoice`) and that fixture's report is byte-identical; ripgrep 143 → 141
from the crate-root case alone.
Reopen only with: a language whose grammar says "the artifact" rather than "this
module", and a spelling an adapter can recognize without the manifest.

## Deferred with data

### Incremental analysis (recolor on dirty regions)
Measurement (v1): full recolor at 50k files costs 13 ms — incrementality is a luxury,
not a lifeline. Revisit only if an LSP latency budget demands sub-parse response, as its
own experiment.

### deep-import external-provider half
Blocked (v1): evaluating it requires the provider's own manifest, which lives outside
the discovered tree (`node_modules/` is not walked). Needs a design for external
manifest visibility before any measurement.

### See-through namespace imports and glob pooled by name — measured 2026-09-06
Measurement (instrument over the corpus, tree at `ad81711`): declarations in reachable
files kept by a whole-surface importer ALONE — no root, no reference in the pool, no
binding, no witness, no published surface, no exemption — number 63 of 101,240. vite 42
(39 behind side-effect imports of playground fixtures, 3 behind `export *`), vapor 16
behind Swift module imports (a module import has no qualifier to see through), Exposed 4
behind Kotlin star imports, ripgrep 1; flask (648 namespace imports), gin (518), lodash,
Alamofire and guava: 0. A side-effect import is an opaque importer by design and 39 of
the 63 stand behind one, so seeing through namespace and glob imports can retire at most
20 (vapor 16, Exposed 4), and seeing through a re-export chain at most 4 more (vite 3,
ripgrep 1 — that row reads `bindings+reexport-all` because rust emits a `Bindings` and a
`Namespace` import over one `use` span, the recorded import-shape debt, and only the
namespace twin is a surface importer). Proposed disposition, the owner's to take: the
keeper that reads `Reference::on` against a namespace import's local, and the glob pool
by name, land in M8.c beside the first adapter emitting qualifiers and are measured
there again. The prize sits in vapor and Exposed, so Swift and Kotlin are that first
adapter: rust emitting qualifiers was measured separately at zero (2026-09-06,
`internal-only` on ripgrep: one finding, none on a member) and waits for this keeper.

### Path aliases — measured 2026-09-06
Measurement (vite, the only corpus repository declaring any): 57 import sites of
`#types/*` and `#dep-types/*` (package.json `imports`, subpath patterns onto
`./types/*.d.ts` and `./src/types/*.d.ts`), one tsconfig `paths` entry in the main
package (`vite/module-runner`), one in the playground (`~utils`), three import maps in
playground pages. The ts resolver cuts a specifier at `#`, so the 57 edges vanish
silently today; the findings that could move sit on the files they point at — at most
4 (one `unused` under `packages/vite/types/`, three under `src/module-runner/`). vite's
12 `undeclared` are `resolve.alias` entries of `vite.config` files: a program, not a
manifest, and no alias evidence would read it. Proposed disposition, the owner's to
take: `ManifestSink::alias` with `Project.aliases` and the specifier rewrite before
`resolve` land in the js-ts slice of M8.c, where package.json and tsconfig become
manifest evidence; go.mod `replace` and Sass load paths join when their adapters
migrate.

## Open candidates (never decided in v1)

### Dependency-hygiene family (undeclared / unresolved / version-skew) — measured 2026-08-31
Full demand was 543 dependency-subject oracle findings (the 410 above plus 198
deps-unused and 14 deps-test-only hiding inside `unused`/`test-only`). The corpus
experiment decomposed every one (COMPARISON has the per-finding record):
**`unresolved` and `version-skew` shipped** (vite 10 + 3, ripgrep 10, everything
else 0 — zero measured noise), carried by `DependencyDeclaration` evidence
(js-ts and rust rich; JVM/go/python/swift name-only until their version models
exist) and three resolver/claim fixes worth more than the analyses.
**`undeclared` deferred with its number, then SHIPPED 2026-09-01** on the
shared floor: the instrument's 905 ripgrep candidates were all Rust
type/primitive/tool/`use`-bound path heads (an adapter precision fix, not a
rule), the 124-finding fixture cliff and the 195 ancestor-declared are the
floor's eligibility and manifest chain, ambient modules and aliases are
mentions. Corpus: one finding (vite's committed-`node_modules` fixture, true by
definition), zero false; the accusation path proven by ablation (lodash's
`@playwright/test` sits behind the `.html` importer doubt — M7.d lifts it) and
by four harvested fixtures. Residue with its number: tsconfig `paths` aliases
never surfaced (0 after the mention and invalid-name rules); Rust heads bound by
a parent module's glob import (0 on ripgrep).
**deps-unused/test-only deferred, REOPENED, then SHIPPED 2026-09-01** (the
owner's challenge; DECISIONS has the reasoning): the tooling-invisibility
argument holds for DEV scope only, so the second pass judges PRODUCTION-scope
declarations — the same `unused`/`test-only` categories on a `dependency`
subject, no new rule. Corpus: one finding (ripgrep `crates/index`: `fst`), zero
false, 119 declarations judged across ripgrep/vite/gin, every skip a typed
`manifests`-scoped abstention (COMPARISON has the table). Residue, each with
its number: 43 vite manifests abstain on unclaimed `.vue`/`.astro`/`.html`/
`.css` importers (M7.d's demand); JVM, Swift and Python abstain as
`specifier-identity-underivable` until a declared spelling exists (Python's
distribution → module mapping is the deptry-shaped experiment); `test-only`
never fires under an `Unscoped` ecosystem (go.mod has no section to move to);
a binary spelled unlike its package (`tsc` for `typescript`) is not a manifest
mention — dev-scope today, so nothing is accused. Also recorded: discovery
does not follow symlinks (three vite findings live on that boundary —
following them is its own experiment if a corpus repo ever hinges on it).

### cyclic (import cycles) — SHIPPED 2026-08-31
The definition question resolved as a language capability:
`ExtensionSpec::import_cycles` (default Tolerated = silence; js-ts and python
declare Hazard). Corpus: vite 31 (incl. the real 88-file node tangle), flask 3
(the package's own 20-file knot — never measured by v1), everything else 0;
v1's JVM file-cycle findings retired as vice (multi-pass compilation makes
them routine legal structure). Package-level cycles shipped with the aggregation epic
(2026-08-31): both oracle findings retired as vices against the real pins —
guava's pair is one-directional (v1 manufactured the loop) and Exposed's
closes only through a test-scoped project dependency, which never blocks a
publish. Corpus zero, mechanism pinned by an engine test (prod mutual fires,
dev pair is silence).

### Python ancestor-package import edges
`import a.b.c` executes `a/__init__.py` and `a/b/__init__.py` on the way down —
the language's rule, but kndo:python emits no edges to the ancestors, only to
`a/b/c.py` itself (plus the submodule probes M6.b.4 added for from-import
bindings). No flask finding hinged on it — every ancestor `__init__.py` there is
reached by direct imports anyway — so the edges are not built. Build only when a
corpus repo shows a finding this changes; the emission point is one loop in
`imports()` next to the binding probes.

### deep-import — DEFERRED with its number 2026-08-31
The last oracle category: 6 findings, and every one is the monorepo's own
test or tooling package deep-importing vite's internals (`@vitejs/unit-ssr`
at 99 sites IS vite's unit-test suite doing its job; `create-vite`'s tsdown
config reaching for a type is tooling). v1's design was already careful
(contract-gated on a declared surface) and still produced only this. A
consumer-role gate — test/tooling consumers exempt, which any honest v2 build
would need — makes the corpus demand exactly zero, and unlike `unresolved`
(whose zero floor guards future renames everywhere) this rule's regression
value exists only in monorepos with declared surfaces and production-role
cross-package consumers, which the corpus lacks. Cost it would need anyway:
`declares_surface` package knowledge, package-pair subjects, consumer-role
gating. Build only when a corpus repo shows a production-role deep-import.

### crap (complexity × uncoverage) — REOPENED 2026-09-01, SHIPPED 2026-09-02
Oracle demand 0 — corpus runs carry no coverage, so v1's own analysis never
fired there: an instrument gap, not a value verdict (DECISIONS 2026-09-01). The
experiment captured coverage from the real producers (flask via pytest-cov,
vite via vitest) and counted: flask 305 scored functions, 3 at the metric's
line of 30, one of them partially covered; vite 1,004 scored, 129 at 30, 70
partially covered (COMPARISON has the table). Shipped as `crap`, `Info`,
`Probable`, partially-covered functions only (cov = 0 is `untested`'s), whole-run
abstention without a report, per-file abstention where the report is silent;
`[analysis.crap] threshold` overrides 30. Residue: branch coverage would
sharpen the fraction where producers emit `BRDA` (the parser reads lines only);
a function record says whether, never how much, so a one-line function is
never scored.

### JVM package (directory) cycles
File-level JVM cycles were retired as vice (multi-pass compilation makes them
routine legal structure), but cycles between PACKAGES — directories, the unit
jdepend and ArchUnit judge — are a different claim, never measured. Candidate
(2026-09-01): a directory-level SCC over resolved imports for adapters whose
compilation unit is the package; zero-FP definition first (parent/child package
references are routine in Java too), guava count second.

### hollow-test rule
A test that asserts nothing / covers nothing real. No decision recorded; needs a
definition that can reach zero FP and a corpus count.

### speculative-abstraction rule
Abstractions with a single implementer and no external consumers. Same bar: zero-FP
definition first, corpus count second.

### `test-only` default severity
v1 left it at info with the raise "available but with no decision behind it". Measure:
how many test-only findings on the corpus are actionable?

### Churn × complexity (git history plugin)
Roadmap idea. Needs: determinism story for history-derived data (history changes run to
run — likely advisory-only), and a corpus measurement of signal quality.

### Java qualified-name segments in the reference stream (keep-alive inflation)
Discovered 2026-08-31 by the Scoped-comparison instrument: kndo:java emits a reference
for every `type_identifier` node, and tree-sitter-java types the package segments of a
qualified type that way — `java.util.function.Function` also emits `function`. Pooled
keep-alive matching then keeps any same-named declaration alive from unrelated
qualified mentions: guava `Tables.java`/`Collections2.java` carry package-private
fields named `function`, and seven instrument rows resolved against them. Direction:
`unused` false NEGATIVES — silent, bounded by lowercase-segment name collisions.
Related grammar fact: extends-position references classify as `Extend` only for raw
supertypes; a generic supertype's identifier reaches the stream as `TypeUse` (the
`generic_type` node owns it), so `RefKind::Extend` consumers see a grammar-shaped
subset. Fixed 2026-09-01 (adapter version 3): lowercase qualifier segments of
`scoped_type_identifier` left the reference stream — an uppercase qualifier (`Map`
in `Map.Entry`, an outer class the grammar cannot tell from a package) stays, the
JLS-case rule whose failure mode only ever KEEPS a reference — and supertype names
classify as `Extend` through `generic_type`/qualified wrappers, never crossing
`type_arguments`. Measured: 266 segment references dropped across guava (890,419 →
890,153), finding delta ZERO at the pin — every collision-named declaration also
carries real expression uses, so the inflation was latent, not active. Shipped for
the direction (a dead declaration silently kept alive by an unrelated spelling is
the one leak a dead-code tool must not have); two extraction tests failed before
the fix and pin both behaviors; conformance fixtures stayed byte-identical
(reports carry findings, not reference kinds).

### Visibility-ladder shape (module-and-descendants scope)
v1's linear rung ladder could not express Rust's module-and-descendants privacy — a
recorded incident (the adapter "had to stop lying about private"; equal rungs anchored
in different files compared as equal regions). Candidate: rungs gain a scope-shape
dimension.

Measured at M4.d, when the Rust and Go adapters landed (the deferred-until moment).
Demand: 8,026 oracle findings across the corpus depend on ladder knowledge —
internal-only 7,983 (guava 7,014, vapor 383, Alamofire 216, Exposed 163, vite 147,
ripgrep 59, gin 1) plus private-type-leak 43 (SHIPPED 2026-08-31: vite 30 real, the
other 13 retired as vices — rung folding, test-support, Scoped comparisons) — the largest unbuilt category, bigger
than everything v2 reports today combined. Supply: three languages shipped on binary
`Reach` alone, and NONE of M4's false-positive fixes wanted a rung between private and
exported — every one wanted scope SHAPE: Go's package scope landed as what is now
`sees` (née `sees` — a capability, not a rung; it retired the interim `ReferenceScope`), and
Rust's module-tree privacy landed as bindings-keep-whatever-the-reach. That is the hypothesis confirmed early and
partially absorbed: the linear part of the ladder is what remains, its consumer is the
internal-only analysis, and it does not exist yet — so the ladder waits for it
(consumer rule; DECISIONS 2026-08-30 M4.d has the verdict).

The same design absorbs `Declaration.exported_as` (2026-08-30, M2): the export alias
belongs inside the exported side of the visibility type — today it rides beside
`Reach` as a parallel `Option` whose `Private`+`Some` combination is representable but
inert, a recorded shape debt by the repo's own "Reach for the type" bar. Folding it in
is a deliberate contract change: fingerprint moves, the pinned conformance reports
diff, DECISIONS gets the entry.

**Worked design (2026-08-31; SHIPPED as M6.c first half — see DECISIONS):** the
region, not the rung. New demand since the entry above: kotlin measured 19 unused vs
the oracle's 277 (public-by-default meets library-mode roots — only `private` is
individually judgeable), rust folds `pub(crate)` to Exported, and swift (M6.b.3) has
`internal` as its DEFAULT. A linear rung cannot compare "module" (kotlin) with
"package" (go) with "crate" (rust) without core learning what those words mean — the
coupling the ignorance rule exists to prevent. A REGION can: visibility for judgment
is not an ordering but the SET of files that could legally name the declaration.

Shape: `Reach` grows `Scoped { scope: SmolStr }` — the token is the adapter's own
word ("package", "module", "crate", "in:a::b"), core never parses it (the
`SymbolKind::Other` posture). The adapter grows one capability:
`scope_of(path, scope, cx) -> Option<files>` — path- and manifest-computable, never
content-dependent (the `sees` stability class, so the persisted graph can trust
it), `None` = unboundable, treated exactly as Exported (keep-alive). Engine judgment
becomes evidence-shaped, ignorant of every language word: Private pools over
file+unit; Scoped pools over its region, is NOT part of the `entry_surface` handed-out
surface (`unused.rs`), and namespace/glob importers keep it only from inside the
region; Exported unchanged. The same region machinery is what `internal-only` (the
7,983) needs to classify uses as inside/outside — one mechanism, two named consumers,
and it absorbs the recorded `exported_as` shape debt (the alias moves into the
non-Private variants, killing the representable-but-inert `Private`+`Some`). Cost:
`Reach` is documented closed-by-design, so this is a deliberate semantic contract
change — fingerprint moves, WIT variant + pin-abi toll, adapters bump when they start
emitting it. Conformance case: kotlin `internal` fixtures (unused internal accused;
cross-package-same-module use keeps it; `protected` stays Unknown→Exported).

**Post-ship residue (2026-08-31, M6.d):** the shipped regions serve every Scoped
rung (guava 3,305, Alamofire 268, vapor 85, Exposed 31, ripgrep 1 measured), which
leaves the demand's two-rung slice structurally unserved: vite's 147 are
`export`ed TypeScript symbols whose every use sits in their own file, where
removing the `export` is the language-checked narrowing — `internal-only` judges
`Scoped` declarations only, and TS has no Scoped rung. That is a DIFFERENT
analysis (export-narrowing over `Exported` declarations, likely also the residual
slice of rust's `pub`-vs-`pub(crate)`), with its own false-positive surface:
an Exported name is nameable from anywhere, so "all uses in own file" is a weaker
fact than region-enumerated absence. Zero-FP definition first, corpus count
second — the demand number is pinned here.

**Residue resolved (2026-09-01):** shipped as `internal-only`'s Exported rung,
gated by a new spec capability (`ExportNarrowing`; js-ts alone declares
`Expressible` — dropping `export` is the narrowing tsc itself then enforces).
The zero-FP floor: own-file use required (`unused` owns the rest), whole-file-
rooted files exempt (entries, tests, tooling are outside surface), any
whole-surface importer exempts the file, and the disqualifier is the WHOLE
COMPILATION — binding imports and same-named references pool over every claimed
file, unreachable ones included, because an unreachable file still compiles
against what it spells (three audited FPs in vite's `__tests_dts__` type-tests
bought that rule, and the engine test proves it adversarially: gating by
reachability makes it fail). Corpus: vite +51 (974 → 1,025 findings; health
85.2 unchanged — Info does not implicate), the engine's set equal to the
instrument's, all 51 inside the oracle's 147; the 96 oracle-only decompose
into 18 playground e2e fixtures, type-only re-exports v1 missed
(`BuildOptions` is index.ts public API), and name-pool conservatism.
lodash: 0. DECISIONS 2026-09-01 carries the full decomposition.

**Scoped-comparison verdict (2026-08-31):** the region-subset half of
`private-type-leak` (fire when the signature's audience strictly contains the named
type's region — Exported-vs-Scoped, Scoped-vs-narrower-Scoped) was instrumented
before being built and measured **0 true defects in a 1,190-candidate upper bound**
across the six Scoped-rung repos (guava 1,048, Alamofire 69, vapor 35, Exposed 30,
gin 7, ripgrep 1). Every signature-position hit decomposed into deliberate idiom
(guava's 88 package-private skeleton/bridge supertypes, Exposed's 15 internal
marker-interface conformances), rung folding (java `protected` → Exported), shadow
trees (GWT `src-super`), or the instrument's own name-fuzzy mis-resolution — Swift
was the control group, its compiler making the real thing unrepresentable, and all
40 of its "hits" were noise, catching same-directory name matching red-handed.
DECISIONS 2026-08-31 has the full decomposition and the instrument's shape for a
re-run. Parked until a consumer arrives with a real case; the toolchains own the
valuable slices already (kotlinc/swiftc reject signature exposure, rustc lints
`private_interfaces`), and javac's silence measured zero on the one Java repo.

### Import-shape debt: the `use`-leaf pair (recorded 2026-08-30, M4)
A plain Rust `use` leaf emits TWO import records over the same target: a
`Bindings` (per-item precision — what keeps a private a child module legally
imports) and a `Namespace` (the whole-surface keep that survives alias hops the
resolver cannot follow). Two records to express one statement is the "two fields
that must agree" smell by this repo's own bar — the honest shape is one variant
carrying both meanings (a named binding that also keeps the surface). Folding it
is a deliberate contract change (fingerprint moves, adapters and the pinned
conformance reports diff), so it rides the next planned import-shape change
rather than moving the contract twice in one milestone. Two smaller ledger notes
from the same audit, each already stated at its code site: Go declares no methods
(structural interfaces), so `duplicate` cannot see Go method-body clones — an
under-report to measure if a corpus ever suggests it matters; and the Rust
adapter's unimportable-crate-root and single-file-crate checks are path-segment
conventions (`src/bin`, `tests/`, `examples/`), the only knowledge available at
their layer — a module literally named `tests` inside `src/` would be misread,
in the fewer-keeps direction.

### Type-3 clones (divergent copies)
Winnowing covers Type-1/2 today. Measure recall/precision of a Type-3 extension on the
corpus before designing anything.

### LESS / JPMS / KMP / Windows-target support
Each is an adapter-capability or platform experiment; Windows enters the release table
only after the CI matrix compiles it (the v1 lesson: it shipped in the target table with
nothing ever building it, and died on a grammar's build script).

### RootKind::Docs (a fourth reachability color)

Candidate: documentation is neither production, test nor tooling. A `docs` root
kind would let doc-example code contribute liveness WITHOUT hiding dead code:
today references inside doctests/KDoc/DocC examples are invisible (symmetric
with v1 — ripgrep parity holds), and if an adapter ever parsed them as plain
references, an export used only by its own examples would go silently alive.
Under a docs color it becomes a `docs-only` finding instead — test-only's
sibling. The same color is what a docs adapter would root, if one ever earns
revival (the Markdown adapter above is killed with data; that verdict stands).

Demand today: none measured — no oracle category is docs-shaped and no corpus
false positive asks for the color. Consumer rule, the ladder's own precedent:
the color waits for its analysis, and a RootKind variant nothing emits and
nothing reads is the "commented-out config key" the release law forbids.

Why the entry exists anyway — a timing constraint: `root-kind` is a WIT enum
and the ABI freezes at the first release. Adding an enum case is cheap before
the freeze and a versioned evolution after. At the freeze (M6.e) this becomes
a forced decision: either an experiment by then justifies the variant, or the
set {production, test, tooling} is declared closed and docs-liveness, if it
ever comes, arrives by another mechanism.

### See-through re-measured after the go migration — 2026-09-06
The 2026-09-06 measurement above was taken at `ad81711`, before go declared its
namespaces and units, and the go migration changed which keeper holds an
exported name (a library-mode whole-file root became a published surface). So
the number was taken again, this time as an ABLATION rather than an instrument:
the whole-surface-importer keeper made unconditionally inert, which is the
maximum a see-through keeper could ever retire, and the corpus re-run.

| repo | with the keeper | keeper ablated | upper bound |
|---|---|---|---|
| gin | 110 | 110 | **0** |
| flask | 29 | 29 | 0 |
| vapor | 199 | 215 | 16 |
| Exposed | 971 | 975 | 4 |
| vite | 711 | 753 | 42 (39 behind side-effect imports) |
| ripgrep | 151 | 155 | 4 |

Go's zero is unchanged by the migration, with 518 namespace imports in gin: an
exported Go name in an imported package is kept by its module's published
surface, and the surface import is never its only keeper. So the keeper that
reads `Reference::on` against a namespace import's local does NOT land in the
go slice — it has nothing to retire there — and go does not emit qualifiers
yet either, since the only consumer of that evidence would be the keeper. The
disposition already recorded stands and is now measured twice: the prize is
vapor's 16 and Exposed's 4, so swift and kotlin are the adapters that bring
both the qualifiers and the keeper, and the audit's G10 for go closes as a
FINDING ABOUT GO's SHAPE rather than a defect: `pkg.Name` is always spelled,
but nothing in Go's corpus is accusable behind it.

### The member name-collision keeper, sized — measured 2026-09-07

`keepers` keeps a MEMBER alive on any reachable reference to its name, anywhere
("dispatch is not lexical"). The plan says witnesses replace that keep-alive,
so the slice that landed `Effect::Witness` sized what replacing it would cost.
Ablating the keeper on guava takes 8,250 findings to **27,405** — it is holding
roughly nineteen thousand members alive by name alone. With java's
`ExternalWitness` table in place the same ablation gives 27,396: the precise
keeper covers **9** of the nineteen thousand today.

The number says the retirement is not a rule-table away. What replaces the
keeper is qualified references (`Reference::on`, which java already emits) plus
witnesses, and the honest next step is to measure how far the QUALIFIER gets
before any of the keeper comes off — the same instrument, one variable at a
time. Recorded so nobody reads "9" as the witness table failing: it is the
keeper masking it.

### `internal-only` and the wildcard import, sized — measured 2026-09-08

With Gradle read, `internal-only` speaks for Kotlin for the first time: Exposed
goes from 0 to 28. The v1 oracle reports 43 on the comparable rung (`certain`,
declared `internal`), and the 27 it has that v2 does not are dominated by ONE
posture — 20 sit in packages some other file wildcard-imports, where
`internal_only` holds that "a namespace/glob importer may use anything" and
declines to advise. `import org.jetbrains.exposed.v1.core.vendors.*` appears in
28 files; `…v1.core.*` in 139.

The engine already builds the per-file reference-name sets a sharper rule would
need: a glob importer disqualifies only the names it actually spells. The
population that rule could reach is those 20 on Exposed alone, and the risk it
takes is a name the importing file reaches through something other than a bare
reference. Recorded with its number so the slice that tries it starts from a
measurement rather than the idea — and so the 27 is not mistaken for a v2 gap
of 27 distinct causes. The other 7: 5 are v1 false positives (`TestDbDsl.kt`'s
four and `ExposedExtension.kt`'s one ARE used from other files), and 2 are
members named `getBoolean`/`getString`, names other files spell for unrelated
JDBC `ResultSet` calls.

### What a `tsconfig.json` states and kndo does not read — measured 2026-09-08

`compilerOptions.paths` earned its reading (132 import edges on vite). The rest
of the file was measured on the same corpus and read nothing, and the numbers
are here so nobody rebuilds one expecting a finding.

| stated | on the corpus | why it buys nothing yet |
|---|---|---|
| `references` | 14, in 7 of vite's 55 tsconfigs | they point at configs that declare no unit, so they disambiguate no unit name — the job `ManifestEvidence::members` exists for |
| `include` / `exclude` | every config has them | kndo claims by suffix and discovers by tree; no finding in the corpus turns on either |
| `extends` | 55 configs, chains 1–2 deep | no alias on the corpus is inherited rather than declared, and a child that declares `paths` overrides its parent's outright |
| a multi-target wildcard alias | 1 (`"@fallback/*"`) | 0 import sites; the reader takes the first target that lands, as TypeScript does |

The alias populations, for the slice that widens this: 143 import sites behind
exact aliases, 4 behind wildcards, 35 behind an alias pointing into
`node_modules/` (absent by design — a target outside the project states
nothing).

## The v1 surface ledger (owner directive, 2026-08-31)

v1's shipped surface is a floor: every capability it offers is either present in v2,
carried here with a disposition, or dead with its vice named in `DECISIONS.md`. Nothing
falls off quietly. The renders row landed first (the directive's own example); the rest
is inventory awaiting its turn — an `open` disposition is a claim of value, not a
commitment to v1's design: each item is rebuilt v2-native when it lands.

| v1 surface | v2 state | disposition |
|---|---|---|
| formats: human, json, agent, sarif + flag > `KNDO_FORMAT` > tty selection | shipped | the render toll (2026-08-31): `--format`, agent format 1, SARIF with byte regions |
| line/column in findings and output | lines shipped (2026-08-31): `Finding.lines`, `path:line` in human/agent, SARIF `startLine`/`endLine` | resolved per run from in-memory contents — no cache or graph format learned about lines, and identity never includes them; columns stay deliberately absent (SARIF counts them in UTF-16 units — a slightly-wrong column is worse than none) |
| `--staged` / `--diff <ref>` change-scoped runs | shipped (2026-08-31): two full analyses over two pinned trees, composed | the engine never learned git — the CLI materializes the base (and, for staged, the index) via `git archive`, and `Snapshot::against` rides the baseline mechanism; envelope carries `run.mode` + `base_health` (a pure function of the pinned base tree, not v1's cross-run `previous`) |
| health (score + per-category breakdown) | model + verb shipped (2026-08-31) | derived ratio, no weights (see DECISIONS); `kndo health` prints the block alone (line on a tty, JSON piped, always exit 0 — measurement, not a gate); `--by-package` shipped 2026-08-31 — the same two integers partitioned by directory-truth ownership, parallel same-named trees told apart by manifest |
| navigation verbs (`find`/`describe`/`uses`/`used-by`/`trace`/`impact`, batched `query`) | all seven verbs shipped (2026-08-31) | one contract (`kndo-query/1`, `Snapshot::query`) answered from the SAME index `unused` judged on — gate-certified: used-by empties exactly where unused accused; `trace` proves liveness (rooted file → import hops with recorded confidence → in-file keeper), `impact --if-deleted` simulates the removal as typed reachability flips, never fabricated findings; serve carries one MCP tool per verb over the same `Request`, answering in the agent grammar from a held session; the directed pair ships as `trace <from> --to <target>` (v1's positional pair would break 1:1 inputs→results; its `--all --max-paths` enumeration is dead — speculative surface, no consumer or measurement); JSONL batch stays out (serve IS the batch amortizer, and it ships) |
| `explain <id>` (evidence chain behind one finding) | shipped (2026-08-31) | finding brief + the full description of its subject — for `unused` the empty keeper preview IS the why; deepens per category as analyses grow evidence |
| `doctor` (what kndo sees: extensions, cache, config) | shipped (2026-08-31) | the real composition (WASM load failures included), config as parsed (a broken kndo.toml is doctor's diagnosis, never its crash), cache and baseline as filesystem facts; two facade doors opened for it (`Session::extensions`, `Session::composition_diagnostics`) |
| `init` + `kndo.toml` + pre-commit hook | shipped (2026-08-31) | `[check]` fail-on/format/only/skip — only keys with living consumers; a typo REFUSES the run (`deny_unknown_fields`); one merge site (flag > `KNDO_FORMAT` > file > default); `init --hook` writes a pre-commit running `kndo check --staged`; template↔struct held one-list by a test |
| `plugin` manager verb (install/list/new/build/wit/verify) | absent — `.kndo/plugins/` loads, nothing manages | open — authoring exists (`docs`), management waits for demand |
| `--only` / `--skip` / `--strict` category filters | `--only`/`--skip` shipped (2026-08-31) as judgment scope — the unselected analysis never runs, `judged` shrinks, health follows | `--strict` dead: it promoted `undeclared`, a category v2 has not built — the dependency family carries it (DECISIONS) |
| `--quiet` / `--verbose` / `--color` | shipped (2026-08-31) | human-render options only (elsewhere they warn and change nothing): quiet = the one verdict line, verbose = the phases line from beside-the-report timings, color = flag > `NO_COLOR` > tty with a semantic palette (severity, clean line, arrow DIRECTION — never an absolute score); v1's verbose reveal-of-hidden-tiers has no successor because v2 hides no tier |
| SIGPIPE default disposition (`kndo \| head` exits like a filter) | shipped (2026-08-31): SIG_DFL restored in the CLI binary, asserted-overflow test pair | v1's reasoning adopted as v2's own judgment — piped-by-design output makes the filter convention correct; serve is a protocol conversation, not a filter, and keeps clean error-propagation |
| adapters: css, html, json (v1 claims 9 territories) | `kndo:html` and `kndo:css` shipped (2026-09-02): v2 claims 9 territories, python included; json dead with its vice named | closed by measurement (2026-09-02): vite 999 → 834 findings with 336 page-reached files no longer dead, lodash's and the doc sites' scripts likewise; json measured at 22 import edges into 386 files on vite, and v1's own json findings were 15 `unused` tsconfigs and 20 unreferenced data files — a config file's consumer is a tool, never an import, so the accusation is the vice — COMPARISON has the decomposition |
| coverage ingesters beside lcov: cobertura, jacoco, go-cover | all three shipped (2026-09-02), each on a report captured from its producer: coverage.py's cobertura on flask, the jacoco Maven plugin's XML, `go test -coverprofile` on gin | closed (2026-09-02): the engine maps the report's own path spelling onto the project (import path, package and source name, name under a source root); gin with its profile gains 7 Certain `untested` functions, flask's cobertura and lcov from one run judge identically — COMPARISON has the numbers |
| built-in conduct plugins: nextjs, express, serde, info-plist, thymeleaf, libsass, uikit, rkyv, wasmtime — each with a baseline-then-plugin proof | `kndo:interface-builder` (uikit's successor: every runtime Interface Builder writes for) and `kndo:info-plist` built in, proven in `builtin_conduct_proofs` on Xcode's artifacts verbatim (2026-09-02); express is a js-ts scripts rule, not a plugin | closed by measurement (2026-09-02): serde/rkyv/wasmtime have no subjects in v2 (a trait impl's members are never declarations), nextjs has zero corpus demand, thymeleaf/libsass wait for M7.d's html and css adapters with spring-petclinic as the instrument — COMPARISON has the table |
| `docs/` mdbook site (install, getting-started, languages, configuration, health, navigation, agents, ci, faq) + `docs.yml` | shipped (2026-09-02): `docs/` — fourteen pages (introduction, install, getting-started, cli, findings, health, configuration, suppressions, ci, navigation, agents, languages, extensions, faq) written from v2's shipped surface, every example a real render; a `docs` CI job builds it on every push, deployed to Pages from `main` since the root swap (2026-09-02) | closed: the install page is a release-channel consumer read by `release_channels.rs` against the target table; every relative Markdown link in the tree is held by the `every_relative_markdown_link_resolves` gate; v1's per-plugin pages have no successor — the two Apple plugins and the four ingesters are rows on the extensions page, and a page per plugin was prose about code that a proof now pins |
| release channels: `release.yml` (git-cliff body), `install.sh`, the GitHub Action, the Homebrew template — held consistent by v1's `release_channels` test | shipped (2026-09-02): one table in `kndo_gates::release` renders the release workflow, names `xtask package`'s artifact, and `release_channels.rs` reads the installer, the Action, the Homebrew template and (with the docs site) the install page against it; the musl build, the installer over HTTP and git-cliff run in CI on every push | closed; `publish-crates` not carried (every v2 crate is `publish = false` until the ABI freezes — DECISIONS 2026-09-02); the `v*` tag trigger is live since the root swap (2026-09-02) |
| `cargo xtask bench` (machine-specific baseline in CONTRIBUTING) and `gen-stdlib` (stdlib datasets feeding v1 resolution) | bench shipped (2026-09-02): `cargo xtask bench` on the baseline recorded in `xtask/perf-baseline.json` — release binary end to end, generated 1k/5k/50k fixtures, five scenarios; gen-stdlib dead | closed (2026-09-02): bench is not a CI gate (one machine's baseline — CONTRIBUTING); gen-stdlib's job is done by rule — of the 26 entries v1's node dataset carries beyond v2's list, 12 are subpaths the prefix rule already names and 14 are underscore internals imported nowhere in the corpus, and Go's 258-package list is the undotted-first-segment rule; no corpus finding traces to a builtin misclassification (DECISIONS). First head-to-head numbers (2026-09-01, release builds, this container, cwd-identical invocations, JSON to /dev/null, 3+ runs): cold `--no-cache` — guava v1 17.8s / v2 9.1s, vite v1 1.8s / v2 0.4s (v1 claims 434 more files there: json 46 + css 233 + html 155), ripgrep 0.47 / 0.30, gin and Exposed at parity; warm over the on-disk cache — guava v1 unstable at 8.9–31.9s across runs (spikes above its own cold) vs v2 stable 1.0–1.2s; peak RSS on guava cold — v1 2,999 MiB vs v2 176 MiB (17×). flask is not comparable (v1 never had a python adapter). Machine-specific like v1's CONTRIBUTING baseline — direction, not gospel |
| Windows in the CI test matrix | back in both matrices (2026-09-02): the SCSS grammar is vendored with its one portable build flag (`vendor/README.md`) | closed pending CI's return (M0): the vendored build resolves and passes here, the MSVC run is the first thing CI will show |

## Standing experiment infrastructure

- The corpus (`corpus/corpus.toml`) is the measuring instrument; keep it pinned.
- The oracle (`oracle/`) is the baseline; every emitting milestone diffs against it.
- An experiment's result — kill, defer, or build — lands in `DECISIONS.md` with its
  number, and killed entries move up into this file's killed section.

### Honesty-channel gaps recorded by the 2026-08-31 audit round

Conduct traps now land on the contribution (`ConductSink::note` → `dropped`), but two
sibling calls still degrade silently: a trapped `ingest` returns `None`
(indistinguishable from "report unparseable"), and a trapped `manifest_dependencies`
returns no names (activation stays off — safe direction, wrong silence). Both want the
same shape: a bridge note on something report-visible. The ingest one also wants the
winning ingester's coordinate on the run. Waiting on a channel design, not on demand.

### Declared-but-unjudged evidence (consumer rule, recorded 2026-08-31)

`Reference.kind` (six variants every adapter computes), `Import.confidence` (always
`Certain`), and root confidence (read only by dedup ordering) currently have no
analysis consumer. Kept, not cut: `RefKind::TypeUse` is `private-type-leak`'s input
and `Extend` is dispatch analysis's, both censused; cutting and re-adding would churn
the fingerprint twice for nothing. The rule stands: the next analysis that wants
confidence-carried-through starts by naming one of these as its input (the
`test_only.rs` comment about carried uncertainty becomes true then, not before).

### Reflection-driven dispatch conventions (recorded 2026-08-31, M6.c)

Caliper benchmarks, NullPointerTester fixtures, JUnit-3-style `testXxx`: methods
whose only caller is a framework that finds them reflectively by naming
convention. Statically they are dead, and kndo reports exactly that (guava's
+251 package-private members after the member-surface rule). The missing fact is
FRAMEWORK knowledge — which conventions a runner dispatches on — which is
conduct-plugin territory (`contribute_roots` with `ManifestDependency`
activation), never adapter or core territory: a `kndo:caliper` or `kndo:junit`
plugin would anchor declaration roots on the convention and those findings
disappear for projects that actually depend on the framework. Same family:
UIKit storyboards instantiate view-controller TYPES by name at runtime
(Alamofire's `Example/` app) — a `kndo:uikit` conduct plugin's FileExists
activation on `*.storyboard` could root the named classes. Demand: the guava
slice above. No plugin is built until an experiment shows the roots land on real
corpus findings — and no finding is suppressed core-side in the meantime just
because v1 happened to ship the same false positives.

## The legacy ledger: every old/new pair, with its ablation number (2026-09-07)

Each row is a mechanism that coexists today with the mechanism the plan says
replaces it. The number is the corpus finding delta from ablating the OLD one
alone — how much it currently carries, which is what decides whether it can be
retired now, needs its replacement wired first, or has to be replaced with a
measured change. Every ablation restored the tree byte-for-byte, and the run
that follows every restore reproduces the pinned reports exactly.

### Duplicate: both mechanisms live, and the new one is wired

`Extension::sees` AND `Extension::seen_from` are gone from the vocabulary
entirely as of 2026-09-07 — trait, WIT, SDK, host, guests and core, along with
`sees_of`, `regions_of`, `GraphFile.regions` and `region_of`. Rows 1a–1c and
3a–3b are closed. No adapter hands the engine a list of files any more: it
declares a namespace, and a manifest declares its units. What a language cannot
yet say that way ABSTAINS — keep-alive, the typed absence — and the abstention
is recorded as a fixture `known_gap` naming the milestone that closes it.

| # | old | replaced by | ablation | disposition |
|---|---|---|---|---|
| 1a | `sees` (kndo:swift) | the scope forest | **0** | ~~retire now~~ **DONE 2026-09-07** |
| 1b | `sees` (kndo:java) | the scope forest | 2 (guava, `untested`) | ~~retire with the delta explained~~ **DONE 2026-09-07** — `covisible` spans the compilation; guava moved 8250 → 8249, the other direction from the ablation, and for a better reason |
| 1c | `sees` (kndo:kotlin) | the scope forest | 15 (Exposed, `untested`) | ~~blocked~~ **DONE 2026-09-07** — kotlin declares its `package` clause; 10 of the 15 recovered, the other 5 wait on Gradle units (M8.d) |
| 2 | member dispatch pool at rung 3, unscoped, every member | the plan's rung 3 (pool-scoped) + rung 9 (Exported only) | **+232** to conform | a judgment call for the owner, below |

### Blocked: the replacement is not wired for that adapter

| # | old | replaced by | ablation | blocked on |
|---|---|---|---|---|
| 3a | `seen_from` (kndo:kotlin) | units from `extract_manifest` | 42 (Exposed) | **DONE 2026-09-07** — the hook is deleted; kotlin's 42 abstain (keep-alive) until the Gradle parser names its units, recorded as a `known_gap` in `internal-scope` |
| 3b | `seen_from` (kndo:swift) | units from `extract_manifest` | **486** (Alamofire 346, vapor 140) | **DONE 2026-09-07** — `Package.swift` names the targets, so 482 of the 486 became real units; the last 4 abstain |
| 4a | library-mode whole-file Production root (java) | `publishes()` in core | 18 findings move (net −2) | the residual is real; needs its own slice |
| 4b | the same (kotlin) | `publishes()` | 84 move (net +50) | M8.d — no units, so nothing publishes |
| 4c | the same (swift) | `publishes()` | 39 (Alamofire; vapor 0) | M8.d |
| 4d | the same (python) | `publishes()` | 16 (flask) | M8.d |
| 5 | `roots` (kndo:js-ts) | `extract_manifest` | **332** (vite 317, lodash 15) | M8.d |
| 6 | `packages` / `manifest_dependencies` / `manifest_mentions` | `extract_manifest` | not ablated: they carry resolution wholesale | M8.d |

Row 6 is not duplication. Java is the one adapter with both, and they are
disjoint by manifest kind: `extract_manifest` reads `pom.xml`, `packages` reads
Gradle settings. The trait's own invariant ("one or the other populated, never
both") holds.

### The forest is fed by two adapters out of nine

`out.namespace` is emitted by **go and java** only. Rust declares its namespaces
structurally instead (mount chains). Kotlin, swift, python, js-ts, html and css
declare none — which is why rows 1c, 3a, 3b and 4b–4d are blocked rather than
free: their visibility still comes from `sees`, `seen_from` and path
conventions, and the forest has nothing to answer with. This is the single
fact behind most of the ledger.

### Row 2 in full: the member ladder

The plan's ladder ends "… superficie publicada solo para efectivo `Exported` en
unidades publicadas; pool de despacho de miembros solo para miembros
`Exported`". Today the member branch puts an UNSCOPED pool — any reachable
reference to the name, whatever its reach — at rung 3, and has no pool-scoped
rung at all. Four shapes, measured:

| shape | corpus delta |
|---|---|
| today (unscoped pool at rung 3, every member) | baseline |
| no member pool at all | **+21,641** (guava +19,148) |
| the plan's rung 9 alone (Exported members, last) | **+21,641** — identical |
| the plan's rung 3 (pool-scoped) + rung 9 | **+232** (guava 144, vite 66, Exposed 15, flask 4, vapor 3) |

Two things fall out. First, the plan's rung 9 is **vacuous**: it recovers
nothing over rung 3, because every Exported member it could keep is already
kept by the published surface, the entry surface or an owner binding. Second,
the real cost of conforming is 232, not 21,641 — the earlier partial
measurement (guava 27,405 vs 27,398) was measuring a ladder with no rung 3 at
all and is superseded.

The 232 are members whose only use is a same-named reference from OUTSIDE the
pool their reach names. Read, they are three families, and none of them is a
reason to leave the ladder as it is:

1. **84 (36%) sit in a test or benchmark file** — Caliper's `setUp` across
   eight guava benchmarks, JUnit inner classes, EventBus `Callback.call`,
   `DummySubscriber.handle`. Framework dispatch, which the plan already sends
   to rule packs (M8.e). Land those and this family goes to zero without the
   ladder being involved.
2. **vite's 66** are public members of exported types in a package that
   publishes through ENTRIES — `ModuleRunner.close`,
   `EvaluatedModules.getModuleByUrl`, `DevEnvironment.warmupRequest`. Under
   `PublishedSurface::Entries` the member branch has no rung that hands a
   member out when the entry surface reaches its OWNER: `Published` is gated
   on the owner's pool being `Pool::Published`, which `Entries` never is.
   That is a gap in the plan's ladder, not in this implementation of it, and
   it needs a rung before the ladder tightens.
3. **guava's 69 non-test** are the `android/` twin tree (the same package
   names as `guava/`, so today's unscoped match keeps each from the other's
   call sites), plus classic deliberate padding (`Striped64.Cell.p1`..`p4`)
   and backported public API (`LongAdder.decrement`). The only family that
   needs its own verdict.

**Order, therefore**: M8.e first, then the entry-surface member rung, then
re-measure and decide family 3. Tightening the ladder before those two would
ship 150 findings whose cause is known and addressable elsewhere.

## What the plan says disappears, verified (2026-09-07)

The plan's removal list, checked against the tree rather than remembered. Run
the commands to re-check: this section is a claim with its evidence, not a
promise.

### Gone

| mechanism | check |
|---|---|
| `Extension::sees` | `grep -rn "fn sees" crates/ abi/ wit/` — one hit, `Project::sees_into`, which is UNIT FRIENDSHIP (the plan's own vocabulary) and not the hook |
| `Extension::seen_from` | `grep -rn "seen_from\|seen-from" crates/ abi/ wit/` — none |
| `sees_of`, `regions_of`, `GraphFile.regions`, `region_of`, `Index::seen_by` | none; `GraphFile.includes` and `Index::included_by` hold what remained, the file's own `Include` imports |
| `Reach::Scoped { token }` | none; the `Scoped` hits are `DependencyScoping::Scoped`, an unrelated capability |
| `narrowable_scopes`, `export_narrowing` | none |
| the second `TypeScriptAdapter` inside html | none |
| `InFiles` | none |
| `Covisibility` | none |

### Still here, with the mechanism that replaces each

Nothing. Every mechanism this section tracked is closed; the rows are below.

### Captures owed

A parser is graded against the ecosystem's own tool where that tool runs here.
Two do not, and the rows say so rather than letting an ungraded reader pass for
a graded one.

| parser | graded against | owed |
|---|---|---|
| python PEP 508/503 | `packaging` 24.0, 14 specifiers — `tests/captured/tooling.json` | — |
| python setuptools + setup.cfg | `setuptools` 68.1.2, 3 pyproject layouts + 1 setup.cfg — same capture | — |
| jvm pom | `mvn help:effective-pom`, Maven 3.9.11, a two-pom reactor — `kndo-toolkit/tests/captured/maven.json` | — |
| jvm Gradle | a `kndoReport` task run inside Gradle 8.14.3, a two-module build with a version catalog — `kndo-toolkit/tests/captured/gradle.json` | — |
| python flit / poetry / hatch roots | their documented keys, fixture-exercised | none of the three is installed here; capture when one is |
| swift `Package.swift` | fixtures + vapor/Alamofire | `swift package dump-package` — no swift toolchain here |

### Closed since

| mechanism | closed |
|---|---|
| whole-file roots per adapter (20 sites) | `FileRole` declarations + the unit's kind. `grep -rn "RootTarget::WholeFile" crates/kndo-adapter-*/src` — 2 hits, both the FILE's own statement: python's `if __name__ == "__main__"` and js-ts's shebang |
| swift's library-mode root and its silent namespace | swift declares its namespace (the SwiftPM target, path-only), so `publishes()` and the scope forest answer instead |
| python's library-mode root, and its line-scanned manifests | `pyproject.toml` / `setup.cfg` / `requirements*.txt` parsed into units, entries, packages and dependencies; `publishes()` reads the unit. Ablations: without the parser the deletion cost flask +15, with it nothing |
| swift's `manifest_dependencies` | `Package.swift` read once, dependencies from the `Package(...)` call's own list |
| js-ts's four hooks | `package.json` states the unit npm compiles and its entries; `tsconfig.json` joins the manifests for its `paths` aliases, which travel as packages. vite 691 → 687 findings, 2342 → 2474 import edges; no conformance fixture moved, which is the equivalence for the half that only changed door |
| the four hooks on the TRAIT, and the three in the WIT | `extract-manifest` on both sides — `grep -c "fn roots(\|fn packages(\|fn manifest_dependencies(\|fn manifest_mentions(" crates/kndo-*/src/*.rs` is **0**, and the WIT exports one manifest function. The ABI grew what the old exports could not carry (`unit` with its publication and friendships, `dependency-declaration` with scope and requirement, `members`, `diagnostics`); `Phase::Manifest` retires with them, since a manifest read resolves entries and so runs in the project phase. Corpus byte-identical across all nine repositories |
| the pom and Gradle line scanners | `roxmltree` over the pom; a block scanner over a comment-blanked copy of the Gradle scripts, plus the version catalog as TOML. Both graded against the ecosystem's own tool, rows above |
| kotlin's library-mode Production root | the engine's published surface, read from the Gradle unit. Ablations from both sides: deleting it before the Gradle reader existed cost Exposed +125; with the reader it costs +38, every one decomposed in `corpus-findings/COMPARISON.md` |

Two roots stayed for reasons that are NOT a missing parser, and they are not
debt: js-ts's shebang and python's `if __name__ == "__main__"` are the FILE's
own statements, which is exactly what extraction is for. The additive-glob
worry the previous note raised dissolved with the library roots: what remains
overlaps on purpose, and a file the test build alone compiles seeds no
production flood whatever colour a root on it claims.


## The build tool grades the manifest reader (2026-09-09)

`ToolTranscript` landed with five producers; two measurements it produced are
recorded here because neither shipped.

| what | number | disposition |
|---|---|---|
| read `<build><sourceDirectory>` as the Maven main unit's root | guava 8271 → 8235: 60 retire (47 `untested`, 8 `unused`, 5 `internal-only`, all under `guava-gwt/src-super`, `guava-gwt/test-super`, `futures/failureaccess`), 24 appear (15 `unused`, 9 `internal-only`); eight other repos byte-identical | HELD. The 24 appear because those files then belong to NO unit and the graph reads a unit-less claimed file as second-class. The tag is read (one walk, generalised over both source-directory tags) and not yet rooted |
| a file no unit compiles publishes every export | guava identical either way (8235); `gradle-multi-module`'s `legacy/Scratch.kt` stops being an `unused` FILE and becomes "production-reachable, but no test reaches this file" | KILLED. The fixture already decided the opposite, and deliberately: `include("legacy")` is commented out, so Gradle compiles none of it. Two trees produce a unit-less file and want opposite answers — guava's is shipped for another compiler, Gradle's is left out of the build — and no evidence yet tells them apart |
| every fixture pom read by maven 3.9.11 offline | 8 of 14 REFUSED before the fix, 14 of 14 after | SHIPPED. A manifest fixture the real tool refuses to read proves nothing about the real tool |

The first row above changed disposition the same day it was written. Asked with
`used-by` instead of reasoned about, the 24 turned out to be cross-variant name
resolution ending, not a graph defect: guava keeps three to five copies of each
of those classes across `guava/src`, `android/guava/src` and
`guava-gwt/{src,test}-super`, and GWT compiles its copy INSTEAD of the one it
shadows. The read SHIPPED, once the build helper's `add-source` was read beside
`<sourceDirectory>` so no generated tree is dropped. guava 8271 → 8235; the
other eight repositories byte-identical.
