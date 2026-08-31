# v2 vs the v1 oracle — every difference explained

The M2 bar: on the territory the `js-ts` adapter claims, v2 is same-or-better than
the v1 oracle (`oracle/`), and every difference has a reason. Numbers from the run
recorded in this directory (`SUMMARY.md`); the oracle is pinned to the commits in
`corpus/corpus.toml`. v2 runs one analysis (`unused`) at M2 — v1 categories with no
v2 counterpart yet (`undeclared`, `untested`, `duplicate`, `internal-only`, `cyclic`,
…) are out of scope here and arrive with their analyses.

## vite — v2: 702 · v1 unused: 844

v1's 844 split by subject: 490 files, 115 symbols, 180 dependencies, 59 directories.
v2's 702: ~680 files, 20 symbols, 6 members. Dependency and directory subjects are
M3+ analyses (v2 has no dependency-hygiene analysis yet); the comparable slices:

**Files.** 274 of v1's 490 file accusations are reproduced exactly. The rest:

- *v2-only, `playground/` (~430)*: vite's e2e fixture forest — per-app projects wired
  through `index.html` script tags and vitest configs. v1 also accused 168 playground
  files (through different edges: its HTML and CSS adapters saw `<script src>` and
  `@import`, which v2 does not claim until their adapters land). Both tools drown in
  this directory by design — it is deliberately-disconnected fixture code; the
  per-file disagreement is exactly the missing HTML/CSS edge set.
- *v1-only (~216)*: `.css`, `.json`, `tsconfig.*` subjects — file types v1's css/json
  adapters claimed and v2 does not claim yet, plus playground subsets reachable only
  through HTML edges v1 could see.
- *v2-only, `packages/create-vite` templates (~26)*: scaffolding apps whose
  `index.html` is the only thing referencing `src/main.*`. HTML edges again; v1
  accused 31 template files through its own different blind spots.
- *v2-only, `docs/_data` (3)*: vitepress data loaders, loaded by filename convention
  (`*.data.ts`) — a framework convention no adapter encodes yet.
- *Shared verdicts worth noting*: `packages/vite/src/node/cli.ts`,
  `src/client/client.ts`, `bin/openChrome.js`, `src/*/__tests_dts__/*` are accused by
  BOTH v1 and v2 — the dist-indirection (`bin/vite.js` requires `../dist/node/cli`)
  is invisible to both, and both treat it the same way.

**Symbols.** v1: 115; v2: 20 (14 of them member-level, which v1 did not report for
JS). All five of v2's free-symbol accusations on the public API surface
(`parseAst`, `parseAstAsync`, `esbuildVersion`, `esbuildPlugin`, `ImportMeta`) are
verbatim v1 findings — shared verdicts, not v2 noise. v1's remaining ~95 symbol
findings live mostly in files v2 keeps alive through wider roots (test/config
conventions root more files in v2, and a rooted file's exported surface is kept by
design); a keep is the safe direction, and symbol-level parity inside kept files is
M3 precision work, not a regression.

**What the manifest capability fixed in v2's own first draft** (measured across the
three runs recorded in git): 771 → 739 → 702 on vite, 50 → 18 on lodash. The fixes,
each now a conformance test: `imports`-field (`#alias`) targets root their files
(`misc/true.js`), wildcard exports expand (`./types/*` — killed 7 `.d.ts` false
positives), `.d.ts` companions anchor beside their JS entry, npm-`scripts` source
paths root as Tooling, `/test/`+`/tests/` directory conventions root as Test, and
`require()`/dynamic `import()` count as imports.

## lodash — v2: 18 · v1 unused: 27

v1's 27: 13 files + 14 dependency subjects (no dependency analysis in v2 yet).
Of v1's 13 files, v2 reproduces 6; every remaining difference:

- *`dist/*` (7, v2-only)*: committed build artifacts nothing in the tree references.
  v1 kept them via edges v2 does not see (its HTML adapter and doc pages); v2's
  accusation is defensible — they are generated outputs checked into git.
- *`vendor/*` non-test (5, v2-only)*: backbone/underscore/firebug/json2, loaded
  exclusively by `<script>` tags in `test/*.html` and `perf/*.html`. The HTML edge
  again — explained, and resolved the day an HTML adapter lands.
- *`perf/*` (2, v2-only)*: same HTML-only consumers.
- *`lib/*` build scripts (3, v2-only)*: `build-doc.js`/`build-site.js` are invoked by
  documentation tooling outside `scripts` (Makefile-era wiring); v2 roots only what
  `package.json` scripts name. The other 8 `lib/*` scripts ARE rooted through
  `scripts` and correctly kept.
- *`fp/_convertBrowser.js` (1, v2-only)*: consumed only by the browser build of the
  dist pipeline.
- *v1-only (7)*: files v1 accused that v2 now keeps — all rooted by v2's wider test
  directory convention (`test/`, `vendor/*/test/`), where v1 used its own test
  detection. Keep-alive direction.

## The abstainers — Alamofire, Exposed, guava (JS slices)

Each claims a handful of docs-tooling JS files (10 / 7 / 2) that no `package.json`
roots and no convention matches, so `unused` abstains whole-run (`NoRootsAnywhere`)
rather than accusing every file. v1 reported zero `unused` JS findings on these repos
too — same verdict, spelled as typed abstention instead of silence. Their real
findings are Swift/Kotlin/Java territory: M3+ adapters.

## gin, ripgrep, vapor

Zero claimed files (Go/Rust/Swift only) — nothing to compare until their adapters
exist. v1's findings there stand as the oracle for those milestones.

## M3 — the analysis categories arrive

v2 grew test-only, untested, and duplicate (plus suppression/stale and the baseline,
which the corpus repos do not exercise — no `kndo:` pragmas, no baselines). The
comparable slices on the run recorded here:

**duplicate — v2: vite 174 · lodash 1 · Alamofire 5; v1: vite 179 · lodash 2.**
The closest match of any category. v2's winnowing (structural, kind-normalized
leaves, corpus-measured 60-token floor — DECISIONS has the experiment) finds the
create-vite template clones and byte-identical files; the residue against v1's 179
is css/json subjects v2 does not claim. Alamofire's 5 are its vendored docs-theme
JS — v1 found them under `duplicate` too, inside its larger Swift-dominated count.

**test-only — v2: vite 122 · lodash 2; v1: vite 27.** 121 of vite's 122 sit in
`packages/*`: the production color is starved by the dist indirection (every
package entry points at built output, so `src/` is production-dark and the test
color is often the only one that reaches it). The accusations are mechanically
true of the graph v2 can see and Info/Probable by design, but the VOLUME is the
dist blindness again — the same root cause behind the shared cli.ts/client.ts
verdicts. The fix is a measurable capability (mapping built entries back to their
sources), an M4 experiment, not a tweak.

**untested — v2: vite 25; v1: 222.** The inverse shape: v2 only judges
production-REACHABLE files, and the same dist starvation shrinks that set, so v2
under-accuses where v1 (whose reachability differed) judged more. All 25 are
playground app files with no tests — true positives. Keep-alive direction;
the count grows as production reachability does. On the abstainers
(guava/Exposed/Alamofire JS slices) untested and test-only abstain for want of
test evidence, and duplicate now judges everywhere Metrics are declared.

## M4 — Rust arrives, and the dogfood turns on

**ripgrep — v2: 143 (unused 3 · duplicate 135 · test-only 3 · untested 2); v1: 149
(internal-only 59 · untested 49 · duplicate 16 · unused 11 · version-skew 10 ·
private-type-leak 4).** The totals are near, the compositions are not, and both
facts are informative:

**unused — v2: 3, all verified true.** `Index::read_write`/`read_write_mut`
(declared, never called at the pinned revision) and `SHERLOCK_CRLF` (a test
constant nothing references). Zero false positives across 110 files — the number
that matters most for trust. Getting here forced four real mechanisms, each now a
conformance case: `#[path]`-redirected mods (and `use` paths riding their alias),
the crate root found by ancestor scan (ripgrep's `[[bin]] path =
"crates/core/main.rs"` in a lib-less root package broke every `crate::` under it),
private items kept by name-bindings (Rust's privacy unit is the module tree —
`super::ENCODINGS` from a child is legal), and `pub mod` as a reexport-all (a
lib's pub-mod tree IS its published surface; `published-lib-surface` pins it).

**duplicate — v2: 135; v1: 16.** Dominated by one file: `flags/defs.rs`, whose
per-flag `test_*` functions are genuine Type-2 clones of the same
parse-then-assert scaffold at the corpus-measured 60-token floor. The findings are
true and Info; whether a frontend rolls a 100-clone battery into one line is a
presentation question, recorded here rather than solved by raising the floor.

**untested — v2: 2; v1: 49.** M4 changed the graph heuristic to v1's own
semantics: where coverage is silent, a file counts as exercised when ANY test
reaches it through imports, however indirectly (the name-reference heuristic
over-accused transitively-tested code, against its own "under-accuse" charter),
and the finding lands at file granularity because that is the evidence's
granularity. Manifest-anchored Production entries are wiring the heuristic skips —
a binary's main can never be imported by a test — while ingested coverage still
judges every function, `Certain`, wiring included. On vite this took untested from
25 symbol findings to 6 file findings (same playground truths, coarser subjects);
on ripgrep the 2 are the `index/` implementation files only integration tests
(which spawn the binary — invisible statically) exercise. The gap to v1's 49 is
mostly ripgrep's core being tested end-to-end through the binary: static graphs
cannot see that, and lcov ingestion is the honest answer, not a looser heuristic.

**test-only — v2: 3** — `tests/index/*.rs`, modules of the integration-test crate
itself: mechanically true, tautologically unhelpful. v1's symbol-level test-only
(`build_fixture` in `test-only-and-cycle`) needs per-symbol reach propagation v2
does not have yet; the fixture records the gap. One more deliberate over-keep is
pinned there too: `use crate::pong;` keeps pong's whole exported surface (the
namespace record is what survives alias hops the resolver cannot follow), so
`rally` — which v1's per-symbol precision accused — stays unaccused until that
precision exists. Both belong to the same future work as the visibility ladder.

**The dogfood flipped from exclusion to subject.** The root `.ignore` now quarries
v1 (and the frozen spikes) and v2 analyzes itself with every analysis judging —
the gate's ACCEPTED abstention list emptied. First contact reported 33 findings on
our own code and every one earned a fix or a lesson: a real duplicated
`line_starts` in core (deduped), the two adapters' cloned test harnesses (promoted
to `kndo-testkit`, the second-copy rule applied to ourselves), `RunOutcome::
exit_code` unreachable because the member rule ignored re-export chains (the rule
grew `owner_bound`, which `published-lib-surface` also demanded), and pragma prose
in doc comments parsing as pragmas (a pragma now STARTS its comment). Zero stands,
measured.

## M4.c — Go arrives, package-shaped

**gin — v2: 108 (duplicate 108); v1: 20 (untested 13 · test-only 5 ·
internal-only 1 · duplicate 1).** The comparable slice is `unused`, and both
report ZERO — reached differently. Go bent the contract twice, and both bends are
now capabilities with fixtures:

**The package is the unit.** A directory of `.go` files shares one namespace with
no imports between siblings, so the adapter answers `unit_mates` — the files a
path can see with no import naming them (every non-test sibling; a test file
sees the whole package). The engine draws one reachability edge per mate and
pools references over that visibility, `_test.go` stays out of the production
color, and imports resolve through the longest go.mod module-path prefix to
`Resolution::Files` — M4.a's directory-unit variant doing exactly what it was
grown for. (`unit_mates` is the retirement of an earlier `ReferenceScope`
capability plus a synthetic sibling edge — one mechanism where two were.)

**What the grammar cannot prove, it never accuses.** Three Go facts, each pinned
by a fixture: `internal/` is the language's own visibility fence, so a bin-less
module's non-internal packages are importable published surface (whole-file
Production roots, `Probable`; `internal-package` reports exactly v1's one
finding). A `// Code generated … DO NOT EDIT.` file declares nothing accusable
while its imports and references stay live evidence (`generated-file`, zero
findings, matching v1). And methods are never declared at all: Go's interfaces
are structural, so ANY method may satisfy one and run without its name appearing
— first contact with gin accused `IsEmpty` and `MarshalYAML`, both alive through
external interface dispatch, and the fix was to stop declaring the class, not to
allowlist the cases (v1's gin oracle also reports zero unused).

**duplicate 108 vs 1** is the ripgrep story again: gin's per-endpoint test
scaffolding really is Type-2-identical at the 60-token floor (35 clones in
`context_test.go` alone). True, Info, and a presentation question already
recorded. **untested/test-only 0 vs 13/5**: gin's packages all carry `_test.go`
files, so at file granularity every production file is test-reachable — v1's
symbol-level precision is the recorded gap shared with Rust's `rally`.

## M4.e — the dist experiment pays

The M3 sections above name one cause three times: production color starved by dist
indirection (every vite package entry points at built output absent from the
tree). The recorded experiment ran: when a manifest entry resolves to nothing, the
same path with its first segment under `src/` — through the same candidate
machinery, compiled-extension swap included — is the entry's source, taken only
when it exists. Measured on the corpus:

**vite test-only: 122 → 16** (oracle: 27). The 106 that left were the
`packages/*/src` bodies the test color alone could reach; with entries mapped to
their sources, production reaches them first, and the residual 16 are the real
test-only helpers. **vite unused: 702 → 699**, three formerly-orphaned files now
production-linked. Everything else held: vite duplicate 174 and untested 6,
lodash byte-identical (its `lib/` entries exist literally — the guard means an
existing built tree always wins untouched), every fixture corpus unchanged. One
deliberate, existence-gated mapping closed the milestone's biggest recorded
distortion; the shared cli.ts/client.ts verdicts confirmed against v1 in M2
remain, as they should — v1 sees the same graph truth.

## M6.b.1 — guava speaks: the first Java measurement

`kndo:java` lands on the unified door and guava claims 3,277 of 3,352 files.
v2 reports 9,925 findings (5,621 before the 2026-08-31 audit round unified the
metrics rule; 6,369 before M6.c's scope regions; 6,620 before its second half
built `internal-only`); the oracle reports 20,244. Category by category:

**internal-only 3,305 vs 7,014 — v2's number is the ruled one; the oracle's is
inflated by v1's defects, each named.** v2's rule stands on its own evidence: a
`Scoped` declaration (here: package-private) with real uses in its own file
and no use beyond it anywhere in its ENUMERATED region — the only files that
can legally resolve the name — with a binding importer or an in-region
reference disqualifying. Because the region is enumerated, absence there is a
strong fact: `Probable`/`Info`, below `Certain` only for reflection (out of
static scope everywhere in kndo) and name-pool collisions. The oracle's extra
~3,700 decompose into v1's vices, not missed truths: it fired on declarations
with ZERO resolved uses (v2 tiers those as `unused` at `Certain` — dead is not
demotable; the 1,017 below hold that slice), it could not enumerate regions so
every finding rode name-fuzzy resolution hedged at `Possible`, and its
field/enum-member subjects multiplied that fuzz. Only adapters that DECLARE a
narrower rung exists (`narrowable_scopes`) fire at all: gin reports zero by
design — Go has nothing below "package", so the same evidence would be advice
nobody can take. Category by category:

**unused 766 vs 6,525 — and 5,485 of the oracle's are false positives its own
subject proves.** 84% of v1's guava `unused` findings are `enum-member` rows
(5,194 from `EnumsBenchmark.java` alone), and that file opens with guava's own
`@SuppressWarnings("unused") // Nested enums used reflectively in setUp.` — the
constants are reached by `Class.forName(...).getEnumConstants()`, no source
line naming any of them. v2 never declares enum constants at all (`values()`,
`valueOf`, `EnumSet.allOf` and plain reflection reach every constant
namelessly — the grammar cannot prove one dead), the same
never-accuse-the-unprovable posture as constructors here and methods in Go.
The residual non-enum oracle unused is 1,040 (703 methods, 163 fields, 46
classes, 41 files, 37 enums, 49 annotations) against v2's 766 — v2's pooled
name matching and its library-mode file roots keep more alive by design, in
the keep-alive direction.

**duplicate 5,456 vs 3,564 — the surplus is real and v1 could not see it.**
1,326 of v2's are FILE-level: guava vendors byte-identical parallel trees
(`android/guava/**` mirrors, the `futures/listenablefuture1` copy), and v1 had
no file-granularity duplicate at all. The symbol-level remainder (4,130) sits
above v1's 3,564 at the same 60-token floor since the audit round unified the
metrics rule across all five adapters (the WHOLE declaration node fingerprints,
signature included, and arrow `switch_rule` arms count): parallel overrides and
delegation overloads whose bodies alone sat under the floor now clear it —
sampled, they are the android/-mirror and overload-boilerplate families, the
same real duplication at a finer floor. `unused` (766 after
the audit round) and `untested` (147) were BYTE-IDENTICAL through the metrics
change. M6.c's scope regions then moved unused 766→1,017: package-private is
`Scoped("package")` now, and a member with a bounded region is no longer part
of the surface an entry hands out — the +251 are package-private members
nothing in their package names, concentrated in reflection-driven test
scaffolding (caliper benchmark bodies, NullPointerTester fixtures). They are
true by static reach and reported on that ground alone; the frameworks that
invoke them reflectively are framework knowledge — conduct-plugin territory,
recorded in EXPERIMENTS — and until such a plugin contributes those roots,
kndo reports what the code alone can prove.
The migration itself was a measured no-op (fixtures byte-identical) — the
member-surface rule is the only judgment that moved.

**untested 147 (file) vs 2,559 (symbol), test-only 0 vs 570** — the two known
granularity gaps, recorded since rally and gin: without coverage v2's untested
judges files, and under library-mode roots (every importable file carries its
own Production root, exactly Go's stance) nothing is ever reached ONLY by
tests at file scope. Both sharpen in M6.c when the linear ladder gives
"reached only by tests" a surface-level meaning, and wherever lcov exists the
ingestion path already upgrades untested to Certain symbols.

**Layout notes measured, not guessed:** guava's tests live at
`guava-tests/test/**` — not `src/test/java/**` — so the Surefire filename
convention carries their Test roots; multi-release variants
(`src/main/java16/**`) are unit mates of their base package symmetrically (the
jar tool merges them), which is what keeps `DefaultMethodSupport` alive in the
harvested fixture.

## M6.b.2 — Exposed speaks: the first Kotlin measurement

`kndo:kotlin` lands and Exposed claims 809 files, measuring 936 findings
(774 before the audit round; the deltas are called out below) against the
oracle's 1,037 (of which 163 are `internal-only`, unbuilt until
M6.c, and 22 more sit in unbuilt categories — test-only 14, cyclic 4,
version-skew 4).

**duplicate 812 vs 91 — the surplus is real, maintained-in-parallel code.**
(646 before the audit round's unified metrics rule; the +166 are the same two
families at the finer whole-declaration floor — overload/override delegation
pairs like `ModOp.invoke`'s and the TRUE/FALSE mirror overrides — plus the
grammar-true classifier finally normalizing Kotlin number literals.)
Exposed keeps TWO test suites in lockstep: `exposed-tests` (JDBC) and
`exposed-r2dbc-tests` (R2DBC) hold structurally identical test methods
duplicated pair-by-pair (`SelectTests.testCompoundOp`,
`UpdateTests`, `UnsignedColumnTypeTests`… — 641 symbol clones at the same
60-token floor every language uses). guava's android/ mirror at method
granularity; the recorded presentation question (grouping clone families)
applies, and v1's far smaller count is v1's weaker Kotlin metrics, not a
v2 false-positive flood.

**internal-only 31 vs 163** — the same decomposition: `internal` declarations
used only inside their own file (`private` would suffice), `Probable`/`Info`
over the enumerated module region; v1's zero-use slice sits in `unused` at
`Certain` instead, and its name-fuzz slice does not survive enumeration.

**unused 38 vs 277 — Kotlin's public-by-default meets library-mode roots**
(18 before M6.c: `internal` is `Scoped("module")` now — a bounded region the
resolver enumerates from the source-set layout — and the +20 are internal
declarations nothing in their module names. The remaining gap to 277 is public
surface, M6.c's `internal-only` half.)**
(19 before the audit round: the corrected resolve order — exact package dir
before the peeled parent — and the joint-compilation main-set mirror keep one
more declaration alive; keep-alive is the direction these fixes are allowed to
move things). Untested moved 109→106 the same way.**
The engine's whole-file root hands a file's EXPORTED surface to its
consumers, and in Kotlin everything unmodified is public — so only `private`
declarations are individually judgeable today (the kotlin fixtures pin
exactly this: v1's `unused` rows there were public members). Java dodges
this because its default is package-private → Private. This is the
library-granularity gap hitting its worst case, and M6.c is its named fix:
internal-only + the ladder reason about ACTUAL surface consumption, which is
also where the oracle's 163 internal-only wait.

**untested 109 (file) vs 484 (symbol)** — the known granularity gap, same as
every language without coverage ingestion; lcov upgrades it wherever reports
exist.

Layout notes: resolution rides the package/directory convention Kotlin
recommends but does not enforce (JetBrains' own tree follows it), with two
fallbacks Java does not need — the `.java` extension (mixed source sets) and
the package directory (file names are free in Kotlin, so a top-level
function import may live in any file of its package).

## M6.b.3 — Swift speaks: vapor and Alamofire measured, the census closes

`kndo:swift` is the sixth built-in and the first language whose DEFAULT rung is
the region mechanism's home case: no modifier means `internal`, so the ordinary
Swift declaration is `Scoped("module")` (`private`/`fileprivate` are both
file facts → Private; `public`/`open` → Exported; `narrowable(["module"])`).
The unit is the SwiftPM target and it is FLAT — subdirectories are
organizational, and a file directly under `Sources/`/`Tests/` belongs to a
path-override layout whose directory is itself the target (Alamofire's
`Source/**`). Tests are a DIFFERENT module: `sees` has no test mirror in
either direction, because crossing modules always takes an explicit import —
the `@testable import` IS the edge. The toolchain's own runners dispatch on
declarations no source line names — XCTest by `test*` name, swift-testing by
`@Test` — so those root `Certain` in test targets, the same tier as the layout
root; `override` roots Probable; the non-private methods of conforming types
root `Possible` (external protocols' requirements are not statically
enumerable — Codable synthesis, delegates). `Package.swift` and its
`Package@swift-*.swift` variants are the manifest: Tooling, nothing accusable,
dependency names read by SwiftPM's own labeled arguments.

**vapor: 216 findings vs the oracle's 593.**
- unused 77 vs 38: 12 sit in `Sources/Development` (a manually-run example
  target) and most of the rest are dead test scaffolding; sampled accusations
  reproduce as zero-use by grep.
- internal-only 85 vs 383: the tiering — v1's zero-use slice reports here as
  `unused`, and enumerated module regions replace name-fuzzy resolution.
- duplicate 51 vs 6: real delegation families (`Client.put`/`delete`/… are
  structural twins) that v1's weaker Swift metrics never fingerprinted.
- untested 3 vs 165: the recorded file-vs-symbol granularity gap, unchanged.

**Alamofire: 609 findings vs the oracle's 501.**
- unused 154 vs 20 — v2 reports MORE, and each sampled accusation grounds:
  `Tests/AFError+AlamofireTests.swift` is a sheet of per-case helper
  properties of which ~10 per family are never exercised (accused ⇔ zero
  grep uses; the used siblings — `isRequestAdaptationError`, 25 uses — stay
  kept). v1's fuzzier pooling kept dead helpers alive; that defect, not a v2
  vice, is the delta. The `Example/` app types (storyboard-instantiated
  UIViewControllers) are statically unreferenced and reported on that ground;
  storyboard/UIKit reflection is framework knowledge — the same conduct-plugin
  territory EXPERIMENTS records for caliper.
- internal-only 268 vs 216: same basis, enumerated regions.
- duplicate 145 vs 199: v2 fingerprints the WHOLE declaration under the
  unified rule; v1's different token stream drew different borderline pairs.
- untested 42 vs 64: granularity, as everywhere.

## M6.b.4 — flask speaks: the first fresh-baseline measurement

`kndo:python` is the seventh built-in and the first language v1 never spoke:
no oracle row, no harvested fixtures, no quarry. The acceptance bar is
therefore different in kind — not "every difference explained" but **every
finding explained against the tree's own ground truth**, which for 83 claimed
files and 23 findings meant reviewing all of them, not a sample.

The measurement earned its keep twice before the numbers settled — the first
run said 29, and six of those were false, each falling to a language fact the
first extractor modeled wrong:

1. **Imports are legal anywhere.** The extractor collected import statements
   only at module top level, but function-scoped lazy imports are a
   first-class idiom — flask reaches `debughelpers.py` through four imports
   and every one is inside a function body (three in src for circular-import
   avoidance, one inside a test in `test_basic.py`). `if TYPE_CHECKING:` and
   `try/except ImportError` blocks hide imports the same way. One whole-tree
   walk replaced the top-level loop; three false `untested` findings died
   (`debughelpers.py`, both `blueprintapp` blueprints — the latter reached
   only via `from blueprintapp import app` inside test functions).
2. **In `from X import a`, `a` may be the submodule `X/a.py`.** The tutorial's
   factory does `from . import auth`, which is not an attribute read — it
   imports `flaskr/auth.py`, per importlib's own lookup order. Each
   from-import binding now emits a namespace probe of its dotted path; a probe
   with no matching file resolves nowhere and is inert, one with a file IS the
   language's semantics. Three more false `untested` findings died
   (`flaskr/auth.py`, `flaskr/blog.py`, `js_example/views.py`).

What stands, 23 findings, all verified:

- **unused 3 — all true positives** under the accused ⇔ zero-grep-uses
  protocol. `src/flask/app.py:_make_timedelta` is the sharpest: the only use
  anywhere is `sansio/app.py:224`, which names its OWN sibling copy at
  `sansio/app.py:52` — the `app.py` copy is a refactoring leftover. A
  name-pooled resolution (v1's defect class) could never report this, because
  the living namesake would keep the dead one; per-file Private judgment
  separates them, and kndo accuses only the dead copy.
  `cli.py:_path_is_ancestor` and `test_json.py:_has_encoding` are plain
  zero-use leftovers.
- **duplicate 11 — flask's real shapes.** The five-way
  `template_filter`/`template_test`/`template_global` family across
  `App` and `Blueprint` is genuinely the same body modulo the registered
  dict; two test-app `__init__.py` files are byte-identical (`cmp` agrees);
  four test-body pairs clone under the token-class winnowing rule (same
  statement stream, different literals — the metric's stated definition).
- **untested 9 — all true statements** of "production-reachable, no
  test-colored path": `docs/conf.py` (sphinx config, imported by nothing),
  the celery example (ships no tests at all — where the javascript example
  ships `tests/test_js_example.py` and correctly went quiet),
  `cliapp/factory.py` and `helloworld/hello.py` (loaded only through CLI
  strings like `--app "cliapp.factory"` — a string is not an import, and
  kndo does no name-divination), and the three `tests/type_check/*` files
  (mypy fixtures driven by `pyproject.toml` config, imported by nothing).
  Whether `examples/` and `docs/` belong in a run at all is the user's
  scoping choice, not the adapter's to make.

`internal-only` is silent by construction: Python has no enforceable rung
between underscore-private and importable, so `narrowable` is empty — "add an
underscore" would be advice about a convention, not a boundary the language
checks.
