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
(774 before the audit round; 987 after M6.c — the +31 `internal-only` and
+20 `unused` are tracked below; the deltas are called out below) against the
oracle's 1,037 (of which 163 are `internal-only`, unbuilt until M6.c, and 8
sit in unbuilt categories — cyclic 4, version-skew 4; the oracle's 14
test-only are the library-roots zero decomposed at guava).

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

## M6.d — the close-out: every cell of the table, decomposed

The census is complete: seven languages, nine repos, and this section is the
whole table read at once — current numbers (this directory's reports) against
the full oracle, with every cell either decomposed above or decomposed here.
The reread itself was run as a measurement, and it caught one more vice.

**The reread's catch: two of ripgrep's three `internal-only` were false, and
the harvested fixture had the falsehood pinned.** `set_errored` and
`ignore_messages` (`crates/core/messages.rs`) are referenced ONLY inside
`macro_rules!` bodies in their own file — but a macro template's names
resolve at every EXPANSION site, so "the narrower rung would suffice" is
exactly wrong: narrowing them breaks every `err_message!` call in the crate.
The `macro-use-mod` fixture — harvested from the same v1 hunt — had that
false `internal-only` finding pinned in its expectation since M6.c built the
analysis. The fix is a posture, not a patch: free declarations named inside a
`macro_rules!` body root `Possible` (dispatch the source never names — the
expansion site does), the fixture now pins the contrast both ways
(macro-named `set_flag` silent, ordinary own-file-only `local_only` still
fires), and ripgrep reads 144: `internal-only` 1, the survivor being
`RegexCaptures` in `matcher/tests/util.rs`, grep-verified — every use in its
own file, `private` compiles.

**The current table** (v2 12,928 across nine repos; oracle 24,459 across
eight — flask has no oracle row):

| repo | v2 | oracle | decomposed at |
|---|---|---|---|
| vite | 895 | 1,878 | M2 + M3 + M4.e, and below |
| lodash | 21 | 37 | M2 + M3, and below |
| gin | 108 | 20 | M4.c |
| ripgrep | 144 | 149 | M4, and above |
| guava | 9,925 | 20,244 | M6.b.1 |
| Exposed | 987 | 1,037 | M6.b.2 |
| vapor | 216 | 593 | M6.b.3 |
| Alamofire | 609 | 501 | M6.b.3 |
| flask | 23 | — | M6.b.4 (fresh baseline) |

**Cells written nowhere above, closed here:**

- **vite `internal-only` 0 vs 147 — structural, not missing.** The analysis
  judges `Scoped` declarations against enumerated regions, and TypeScript has
  no `Scoped` rung: its ladder is unexported (Private, file-enforced) /
  `export`ed (everywhere). v1's 147 are the OTHER judgment — `export`ed
  symbols every use of which sits in their own file, where removing the
  `export` is a language-checked narrowing. That is real advice a future
  analysis can give, but it is export-narrowing over `Exported`
  declarations, not a rung demotion — recorded in EXPERIMENTS with this
  demand, beside the ladder entry it extends.
- **`test-only` 0 on gin, guava, Exposed, vapor, Alamofire (oracle 570 + 14
  + 5 + 1 + 2) — one mechanism, named at guava, standing here for all
  five:** under library-mode roots every importable file carries its own
  Production root, so no file is ever reached ONLY by tests at file
  granularity; v1's counts are symbol-granularity. The same
  file-vs-symbol gap that untested carries everywhere coverage is not
  ingested — lcov upgrades both wherever reports exist.
- **lodash `untested` 0 vs 4, `test-only` 2 vs 0:** lodash's four oracle
  untested are inside `dist/`+`vendor/` trees v2 keeps via different edges
  (the M2 decomposition); its two v2-only test-only are `vendor/*/test/`
  fixtures only the test convention roots — mechanically true, the same
  tautology class as ripgrep's `tests/index/*`.
- **ripgrep `unused` 3 vs 11:** the three are grep-verified true; v1's
  extra eight ride its per-symbol reach inside files v2 keeps whole
  (`rally`-class precision, recorded at M4) plus its fuzzier pooling. The
  keep-alive direction, unchanged since M4.

**The unbuilt categories — the standing ledger, 497 oracle findings, each
with a disposition:**

| category | oracle demand | disposition |
|---|---|---|
| undeclared | 293 (vite 289, lodash 4) | dependency-hygiene family: EXPERIMENTS open candidate |
| unresolved | 79 (vite) | same family, same entry |
| version-skew | 38 (vite 24, ripgrep 10, Exposed 4) | same family, same entry |
| private-type-leak | 43 (vite 30, guava 9, ripgrep 4) | consumer-rule censused: `RefKind::TypeUse` is its input (EXPERIMENTS) |
| cyclic | 38 (vite 31, Exposed 4, guava 3) | EXPERIMENTS open candidate, zero-FP definition first |
| deep-import | 6 (vite) | half killed by v1's own measurement; external-provider half deferred (EXPERIMENTS) |

Every unbuilt category is a decision with a written home, not an omission:
the dependency family waits on per-language dependency models measured
against this demand, and nothing arrives without its corpus experiment
first.

**The law, applied to the whole document:** no cell above is justified by
matching v1 — the close matches (vite duplicate 174 vs 179, ripgrep totals
144 vs 149) are coincidences of composition, and the wide ones carry the
name of a v1 defect (name-fuzzy resolution, untiered confidence,
reflection-blind enum accusations, symbol-vs-file granularity without
coverage) or a v2 decision (never-declare postures, library-mode roots,
keep-alive direction, categories not yet earned through measurement) — in
both directions, including the four repos where v2 reports MORE within a
category (gin/ripgrep/Exposed duplicates, Alamofire unused).

## Health — the first measurement of the derived ratio (2026-08-31)

The model: `implicated / subjects`, distinct symbols-or-files carrying a first-party,
warning-or-worse finding, over every declaration plus every claimed file. No weights,
no bands — the score is the ratio. Across the nine repos it differentiates, and every
position is explainable from the findings themselves:

| repo | score | implicated / subjects | counting categories |
|---|---|---|---|
| vite | 85.2 | 747 / 5,052 | unused 703 · cyclic 31 · private-type-leak 30 |
| lodash | 90.4 | 18 / 187 | unused 18 |
| Alamofire | 95.4 | 154 / 3,380 | unused 154 |
| vapor | 97.6 | 77 / 3,149 | unused 77 |
| guava | 98.6 | 1,017 / 70,175 | unused 1,017 |
| flask | 99.5 | 6 / 1,187 | unused 3 · cyclic 3 |
| Exposed | 99.7 | 38 / 12,482 | unused 38 |
| ripgrep | 99.9 | 3 / 2,557 | unused 3 |
| gin | 100.0 | 0 / 1,304 | — |

(The table is refreshed as warning-tier categories land: cyclic and
private-type-leak joined on 2026-08-31 — vite absorbed both, which is the
table working as intended; version-skew is info-tier and unresolved's ten
vite findings are error-tier on Import subjects, which health's
symbol-and-file universe deliberately does not count.)

Two readings the table forces, both intended:

- **Size cannot hide damage, and damage cannot hide size.** guava carries the largest
  implicated count of the corpus (1,017) and still scores 98.6 — its universe is
  70,175 subjects. vite's 699 in a 5,044-subject universe is 86.1. A penalty score
  (v1's −25-per-bucket shape) would have ranked them by accident of caps.
- **Only the warning tier implicates, and on this corpus that is `unused` alone** —
  duplicate, internal-only and untested all report at Info, the advisory tier (gin:
  108 duplicate findings, health 100.0). This is inherited from the severity
  contract, deliberately: health has no severity opinion of its own, so the one
  place a category's tier is decided (its analysis) is also the one place its
  health-weight is decided. Promote a category to warning and health counts it,
  with no health-side change.

The scores live in each `<repo>.report.json` as the two integers plus the
per-category tally; the 100× ratio is computed at render, one decimal, in
`Health::score_text` — the envelope stays float-free.

## The dependency family (2026-08-31)

The oracle carried 543 dependency-subject findings across the corpus (undeclared
293, deps-unused 198, version-skew 38, deps-test-only 14). The corpus experiment
decomposed every one before anything shipped; two members ship, two defer, and
the oracle's totals turn out to be mostly its own vices.

**Shipped — `unresolved` (v2: vite 10, everything else 0).** The oracle's 79
were dominated by vite's `playground/` (deliberately-broken resolution
fixtures), query-suffixed specifiers (`./worker?worker&url` — 42 real edges the
resolver now strips suffixes to keep), `.d.ts` stubs answering `.js` specifiers
(the swap table now includes `.d.ts`, `.mts`, `.cts` — and the adapter now
claims `mts`/`cts` sources at all), assets that exist outside the analyzed
world (`./app.css`, `../../package.json` — the graph now carries the discovered
set so "exists but unclaimed" never reads as "missing"), and build outputs
(`../dist/…` — the missing target's parent directory holds no discovered file,
so the project is deliberately reaching outside the tree). The surviving ten
are all vite's own: three fixtures broken on purpose (`missing-file/`,
`has-invalid-import.js`, `./foo`), four resolution features the resolver does
not model (browser-field remaps, directory-`main`, exports-map deep imports),
and three symlinked files (`ssr-wasm/src/*` → `../../wasm/` — discovery does
not follow symlinks; a recorded boundary, not a bug).

**Shipped — `version-skew` (v2: ripgrep 10, vite 3, all `info`).** Peer
requirements are exempt — a wide peer range beside a narrow dev pin is CORRECT
practice the oracle flagged as skew (its `esbuild`/`sass-embedded` findings on
vite's best-maintained manifest). Non-comparable requirements (`workspace:*`,
`file:`, path/git specs) arrive as `None` from the adapter and never compare.
ripgrep's ten are real textual divergence, all semver-compatible (the
`workspace.dependencies` nudge); vite's three are real cross-manifest drift
(react pin-vs-caret, vue, tailwind). Exposed's oracle 4 were Writerside DOC
SNIPPETS read as project manifests — the JVM adapters ship names without
requirements until BOM/catalog modeling exists, so v2 reports zero there by
construction.

**Deferred — `undeclared` (oracle 289 + 4).** Deduplicated per (package, name)
and decomposed: vite = 195 ancestor-declared (workspace hoisting the per-leaf
model refuses to see), 124 fixture-tree packages, ~20 parser noise (imports
inside template literals of scaffolder/transform code), 3 self-imports (legal
via `exports` self-reference), plus ambient runtime modules (`pnpapi`,
`@vite/env`; lodash's phantomjs `system`/`webpage`). Honest true positives on
this corpus: **2–3** (lodash's `@playwright/test`, `marky-markdown`), against a
124-finding fixture cliff. The rule's real-world value is not in question — the
corpus is simply a pathological instrument for it (vite is a repository OF
resolution edge cases). Deferred until the model carries ancestor-declaration,
ambient-module and self-import knowledge, with a corpus addition that looks
like an ordinary application.

**Deferred — dependency `unused`/`test-only` (oracle 198 + 14).** After
fixture, `@types/*` and script-invocation classes, vite's remainder (24) is
dominated by config-driven tooling (`typescript`, `lint-staged`, `execa`,
`playwright-chromium` — invoked by configs and CI, invisible to import
evidence); lodash's (13) by HTML-runner test assets. Zero-FP is not reachable
with structural evidence alone — the tools that do this well carry per-tool
plugin knowledge. The oracle's numbers were noise at roughly the same ratio.
gin's two `test-only` dependency findings name the deeper vice: go.mod has no
dev section, so "move it to devDependencies" is advice Go cannot take —
unactionable by construction.

## Cyclic (2026-08-31)

The oracle's 38 decompose as: vite 31 file-level (real — including the 88-file
SCC in `packages/vite/src/node`, the repository's known tangle — beside its
deliberate cycle fixtures, which are code like any other), Exposed 3 + guava 2
file-level in JVM languages (the compiler resolves reference cycles in
multiple passes — routine legal structure, a v1 vice to flag), and 2
package-level (guava's maven test-dependency loop, Exposed's gradle one) that
wait for the package graph the package-aggregation epic will build.

v2 reports 34: vite 31 and flask 3 — the flask package's own famous 20-file
circular-import knot (`__init__.py → app.py → __init__.py` as the shortest
loop) plus two example apps; Python was never measured by v1. Cycle hazard is
the language's own declaration (`ExtensionSpec::import_cycles`): js-ts and
python say `Hazard` (initialization-order bites at run time), go (compiler
forbids), rust (intra-crate modules are idiomatic), the JVM pair (multi-pass)
and swift (single compilation unit) stay `Tolerated` — silence, not findings.
Self-import edges (Python's `from . import x` inside `__init__.py`) are not
cycles between modules and neither seed an SCC nor shadow a real loop's
rendering.

**Timing (2026-09-05).** An import now carries WHEN it runs (`Timing`: load,
lazy, erased), and `cyclic` walks load-time edges only — an initialization
hazard needs initialization, and `import type`, a dynamic `import()`, a
function-scoped `require` or a `TYPE_CHECKING` block runs after linking or
never. vite 31 → 28: the 8-file `module-runner` loop was closed by type-only
hops and is gone; the 88-file `node/` tangle anchored at `baseEnvironment.ts`
shrinks to the 48-file value-import loop anchored at `build.ts`, and the
`packages.ts ↔ …` pair it had swallowed surfaces as its own 2-file loop; three
loops that existed only through dynamic imports — the `entry-cyclic` and
`hmr-evaluated-import-race` runtime test fixtures and the 25-file
`playground/multiple-entrypoints` fan — are no longer hazards. The three that
remain in vite's own cycle fixtures are value imports, which is what those
fixtures test. flask stays at 3 with every loop reshaped by the same rule: the
20-file knot is 9 files of load-time imports (the other 11 were joined by
`TYPE_CHECKING` and function-scoped imports), `json/__init__.py ↔ provider.py`
surfaces as its own load-time pair, the celery example's loop ran through a
function-scoped import and is gone, and the `__init__.py → app.py` loop is now
`Certain` because its shortest loop no longer rides a `Possible` submodule
probe. Nothing else in the corpus moved: reachability keeps every timing.


## private-type-leak (2026-08-31)

The oracle's 43 were vite 30 + ripgrep 4 + guava 9. v2 reports **vite 30,
everything else 0** — and the difference is the analysis's own precision
floor, not lost coverage. ripgrep's four accused `pub(crate)` methods of
leaking `pub(crate)` types — v1 folded both to "exported", and crate-internal
callers can name both just fine; v2's Exported-vs-Private-only rule (any
`Scoped` reach on either side is silence) retires them by construction.
guava's nine were constructors in `guava-tests`/`guava-testlib` support code
referencing package-private types: package-private is `Scoped` in v2 (silence
again), the files carry test roots (a test makes no public promise), and the
Java adapter does not yet delimit signature spans at all — three independent
reasons, any one sufficient. vite's thirty are the real shape — exported free
functions whose parameter or return annotations name file-local types
(`Needle`, `IdResolver`, `AssetUrlFormat`, …) that consumers can call but
never name.
## Package-level cyclic (2026-08-31)

The oracle's two package cycles dissolve against their own pins. guava:
`guava-tests → guava-testlib` exists; the reverse declaration does not —
guava-testlib's pom (both trees) declares guava, junit, truth, never
guava-tests. v1 manufactured the loop. Exposed: the cycle closes only through
`exposed-kotlin-datetime`'s `testImplementation(project(":exposed-tests"))` —
a test-scoped edge that gradle never publishes, so "breaks publish ordering"
was false on its face. v2's edge rule (declared sibling dependency at any
scope but Dev, with gradle configuration words and maven `<scope>test</scope>`
read as declarations) reports zero on the corpus, and the mechanism is pinned
by an engine test: a prod mutual pair fires, the same shape dev-side is
silence.

## Export-narrowing: internal-only's Exported rung (2026-09-01)

vite gains 51 `internal-only` findings (974 → 1,025; health 85.2 unchanged —
Info does not implicate): exported symbols whose name exists nowhere outside
their own file, in files that are not entries, not whole-surface-imported, not
rooted. The engine's 51 equal the pre-build instrument's 51 exactly, and all
51 sit inside the oracle's 147 — a strict subset, zero v2-only.

The 96 the oracle reports and v2 refuses, by mechanism:

- **18 in `playground/`** — deliberately-disconnected e2e fixture apps
  (`hmr-full-bundle-mode/dead-accept.js#value` is dead on purpose); v2's
  rooting and reachability universe keep fixture apparatus out of advice.
- **The `packages/vite` bulk: type-only re-exports v1 could not see.**
  `BuildOptions`, `ModulePreloadOptions`, `ImportMetaEnv`, … sit in
  `index.ts`'s `export type {...}` lists — documented public API v1 accused;
  v2's binding set includes `TypeOnly` imports, so they are silent.
- **Name-pool conservatism** — a same-named reference in ANY claimed file
  disqualifies, unreachable files included: three of the instrument's own
  first-draft candidates fell to `__tests_dts__` type-tests (claimed, rooted
  by nothing, still spelling the names), and that rule is now the floor.

lodash: 0 candidates (a built single-file library). Every other repo:
byte-identical — only js-ts declares `ExportNarrowing::Expressible`.

## The dependency family, second pass: `unused` and `test-only` on declarations (2026-09-01)

Production-scope dependency declarations are subjects of `unused` and
`test-only`, judged per manifest over one shared floor: the claiming adapter
derives package identity from specifiers (`DependencyIdentity`), no unclaimed
file the adapter says could import sits inside the package
(`dependency_importers`), at least one owned file is reached, and not every
owned file is a test. Each failed condition is a `manifests`-scoped abstention
in the envelope, never a silent skip. A declaration is in use when any file the
adapter claims — inside the package or across it — imports it, when a string
literal mentions it (`ImportShape::Mention`), or when the manifest names it
outside its declaration (`scripts`, `browser`, a tool config).

| repo | judged declarations | `unused` | `test-only` | unjudged manifests (reason) |
|---|---|---|---|---|
| ripgrep | 61 | 1 — `crates/index/Cargo.toml`: `fst` | 0 | 0 |
| vite | 43 | 0 | 0 | 43 unclaimed importers (`.astro .css .html .scss .vue`), 3 all-test packages, 4 unreached packages |
| gin | 15 | 0 | 0 — unscoped: nowhere to move | 0 |
| lodash | 0 | 0 | 0 | 0 — dev sections only |
| guava · Exposed · vapor · flask | 0 | 0 | 0 | 12 · 49 · 2 · 5 — specifier identity underivable |
| Alamofire | 0 | 0 | 0 | 0 — no declarations |

One finding on the corpus: `fst`, declared in ripgrep's `crates/index/Cargo.toml`,
never `use`d, never spelled in a qualified path, never named by the manifest.
The pre-build instrument reached the same floor — one true positive, zero
false — after naming the vice classes, each now a rule rather than a filter:

- **Cross-package use (11 in vite).** A dependency the root declares and only
  a workspace member imports: hoisting keeps it in use, so `users` is tree-wide.
- **Mentions (25 in vite).** `core-js/modules/…` injected at run time is spelled
  in a string, never imported: `ImportShape::Mention` keeps the declaration and
  draws no edge. The confidence-based rule it replaces had also silenced
  Python's `Possible` absolute imports — flask lost the `js_example`
  `__init__` ↔ `views` cycle for one measurement, restored here.
- **Manifest naming (1 in vite).** A `browser` map aliasing
  `@vitejs/test-resolve-browser-field-bare-import-success`: `used_by_manifest`
  reads every field but the dependency sections and prose.
- **Unclaimed importers (39 instrument rows, 43 manifests).** `.vue`, `.astro`,
  `.html`, `.css` files nothing claims — the adapter declares which suffixes
  cast doubt, the abstention names them, and M7.d's web adapters are the
  answer.
- **Attribute-only crates (2 in kndo itself).** `thiserror` named only in
  `#[derive(thiserror::Error)]`: the dogfood gate caught it, and crate paths
  inside attributes are imports (rust adapter 4).

The oracle's 198 `deps-unused` / 14 `deps-test-only` are v1's number over every
scope under name-fuzzy resolution; v2 judges production scope only, under a
declared identity, and abstains where it cannot see. Health's universe grows by
the judged declarations — ripgrep 2,557 → 2,618, vite 5,052 → 5,095, gin
1,304 → 1,319 — and every other cell of the table is byte-identical.

## `undeclared` on the shared floor (2026-09-01)

A package a reached file imports that no manifest from its own up to the root
declares. Same floor as the dependency subjects above (the nearest manifest must
be judged), then only the author's own unconditional statement accuses — a
`require` inside a function, branch or `||` guard is `Probable` now, the
optional-dependency idiom — and everything the project itself provides exempts:
a self-reference resolved in the tree (a sibling package must still be
declared: the phantom-internal pattern), a platform module
(`DependencyBuiltins`: Node's list and specifier schemes, Go's undotted first
segment, Rust's `std`/`core`/`alloc`), a name the file declares, a declaration
in the chain (`@types/` included), a mention in any manifest or any literal in
the tree. Rust's qualified paths stopped pretending: `Vec::new`, `u64::MAX`,
`io::Result` after `use std::io`, `#[rustfmt::skip]` name no crate and are no
longer imports (rust adapter 5) — the instrument's first pass had 905
candidates on ripgrep, every one a type, a primitive, a tool attribute or a
`use`-bound continuation.

| repo | judged | `undeclared` | ablation: the same rule with the importer doubt off |
|---|---|---|---|
| ripgrep | every manifest | 0 | 0 |
| gin | every manifest | 0 | 0 |
| vite | 184 manifests abstain (41 unreached, 117 with unclaimed importers, 26 all-test); the rest judged | 1 — `test-package-a`, imported by `playground/nested-deps/test-package-b` from a COMMITTED `node_modules` | 1 (the same) |
| lodash | 0 — `test/*.html`, `*.css` unclaimed | 0 | 1 — `@playwright/test` in `playwright.config.js`, declared nowhere |
| guava · Exposed · vapor · flask · Alamofire | 0 — underivable | 0 | — |

One finding on the corpus, true by the definition and vendored by construction:
vite's fixture ships the dependency inside a checked-in `node_modules`. The
accusation path is validated by the ablation (lodash's `@playwright/test`, the
true positive the first measurement named) and by the harvested fixtures
(go-work-phantom-dep's `example.com/a` across `go.work`, npm-workspace-monorepo's
`@demo/a` and `left-pad`, workspace-deps' `rand_chacha`). The oracle's 293 were
v1 over every scope with name-fuzzy resolution and no floor: 195
ancestor-declared, 124 fixture-tree packages, ambient modules, self-imports —
each now a rule or an abstention, never a filter.

Two side effects rode along, both keep-alive: a `scripts` token with a path
roots what it names (`node lib/main/build-site`, `tsc -p src/module-runner`), so
lodash loses three `unused` build scripts and two `test-only` files they reach
(21 → 16), and vite loses fifteen `test-only` files under `module-runner/` and
`shared/` that its typecheck script reaches (1,024 → 1,010); manifests without
declarations now hold an entry, so the JVM/Swift/Python abstentions count every
manifest (guava 12 → 16, Exposed 49 → 62) and vite's unclaimed-importer doubt
names 117 manifests with the full suffix family the js-ts adapter declares.

## `crap` with real coverage: the instrument the corpus never had (2026-09-02)

The corpus runs carry no coverage, so v1's `crap` never fired on it and the
oracle's demand read zero — an instrument gap. The experiment captured
coverage from the real producers on two corpus repositories, in scratch copies:

- flask (pinned commit) with `pytest --cov=flask --cov=src
  --cov-report=lcov:coverage/lcov.info tests` — 482 tests pass, one async view
  fails in this container; 24 files, 356 function records.
- vite (pinned commit) with the workspace installed, `packages/vite` built, and
  `vitest run --coverage.enabled=true --coverage.reporter=lcov
  --coverage.reportOnFailure=true` over the unit suite (932 tests pass; the
  IPv6 listen and `create-vite` CLI specs fail in this container and are
  excluded) — 137 files, 3,624 function records. Two producer facts worth the
  record: vitest writes no report at all when any test fails unless
  `reportOnFailure` is set, and `vite` must be built first or vitest cannot
  load the workspace package it resolves to.

`CRAP = cc² × (1 − cov)³ + cc`, `cov` the covered fraction of the function's
instrumented body lines, over every declaration carrying metrics outside test
files. "cov = 0" is `untested`'s subject already; the last column is what only
`crap` can say.

| repo | scored | CRAP ≥ 30 | of which cov = 0 (`untested`) | partially covered |
|---|---|---|---|---|
| flask | 305 | 3 | 2 | 1 — `cli.py#load_dotenv`, cc 13 at 32% |
| vite | 1,004 | 129 | 59 | 70 — `transformMiddleware` cc 45 at 4%, `importAnalysisPlugin` cc 121 at 61%, … |

vite at other lines: 82 at ≥ 50 (47 partial), 51 at ≥ 100 (33 partial); 158
functions have cc ≥ 10, 52 of them under half covered. flask, a small and
well-tested tree, puts eleven functions at cc ≥ 10 and all but one above 75%
coverage. The metric's own line — 30 — is where its authors put "crappy", and
both trees read sensibly at it: a handful on the well-tested one, a real risk
list on the large one. It ships as the default, `[analysis.crap] threshold`
overrides it per project.

Two defects the real reports surfaced before any finding could: the engine
panicked on ingestion (a one-line function inverted the coverage line range —
`BTreeMap::range` refuses it; flask has hundreds of such functions), and
coverage.py's lcov 2.x `FN:<line>,<end>,<name>` records paired with no `FNDA`,
silently dropping every function record. Both fixed, both pinned by tests; the
python fixture `crap-partial-coverage` carries a report captured from
pytest-cov itself.

With `crap` registered, every corpus report gains one whole-run abstention —
`no-coverage-ingested` — and nothing else moves: findings are identical on all
nine repositories.

## M7.c — framework conduct: nine candidates measured, two plugins and a rule (2026-09-02)

v1 shipped nine built-in plugins. Each went through the reflection-dispatch rule
before anything was built: measure the demand on the corpus, and a plugin lands
only when its roots would land on real findings.

| v1 plugin | corpus demand | disposition |
|---|---|---|
| uikit | Alamofire: `MasterViewController`, `HostingController` unused; `titleImageView` internal-only | built as `kndo:interface-builder` |
| info-plist | Alamofire: `ExtensionDelegate` unused | built as `kndo:info-plist` |
| express | vite: 11 files a package script hands to `node` reported unused | a js-ts adapter rule (the runtime's argument is an entry), no plugin |
| serde, rkyv, wasmtime | 0 — v1's roots kept trait-impl members alive; v2 never makes those subjects | dead by construction |
| nextjs | 0 — no corpus repository is a Next.js app | deferred: no instrument to measure with |
| thymeleaf, libsass | 0 — their subjects are html and css files, which no v2 adapter claims yet | deferred to M7.d, spring-petclinic as the instrument |

**Alamofire: 609 → 599.** Ten findings leave and none arrive. Three `unused`
classes — `MasterViewController` (`Main.storyboard`, `iOS.CocoaTouch`),
`HostingController` (`Interface.storyboard`, `watchKit`), `ExtensionDelegate`
(`WKExtensionDelegateClassName` in the extension's `Info.plist`). Four
`internal-only` members — `titleImageView`, the connected outlet, and
`detailViewController`, `elapsedTime`, `numberFormatter`, members of the classes
the storyboard instantiates: a rooted owner is used from outside the graph's
sight, and its members with it (core's rule since M6.c, not the plugin's). Three
file-level `untested` verdicts on the same files: a file carrying an anchored
Production root is declared wiring — the rule that already exempts a manifest's
entry point — so the graph heuristic passes on it rather than accuse what UIKit
reaches and no test can. v1's `uikit` read `iOS.CocoaTouch` documents only and
would have left `HostingController` dead; the document format is one, so v2's
plugin reads every runtime the editor writes for, and the coordinate names the
editor.

**vite: 1,011 → 1,000.** Eleven `unused` files, every one the argument of a
`node …` or `tsx …` package script (`playground/ssr/server.js` and its nine
siblings, `ssr-html/test-stacktrace.js`, `ssr-webworker/worker.js`). The old
rule rooted a script's path-shaped tokens only; a bare `server` handed to a
runtime is an entry however it is spelled.

Every other repository is byte-identical, ripgrep included — the Rust adapter's
alias fix (a `use` headed by another `use`'s local, found by the dogfood gate
on the host and SDK sources) moves nothing there. The proof fixture carries
`Main.storyboard`, `LaunchScreen.storyboard`, `Interface.storyboard` and both
`Info.plist` files verbatim from the pinned Alamofire clone beside sources in
its shape: eight findings without the plugins, two with, and every one that
leaves is a name one of the artifacts spells.

## M7.d — the web adapters: a page is an entry, a sheet is a file (2026-09-02)

The instrument before anything was built, on vite: 155 html pages (134 in
`playground/`), 205 css and 27 scss files, none claimed; 79 of the 676 files v2
reported `unused` were entries an `index.html` names in a `script[src]`; 29
pages carry their imports in inline `<script type="module">` bodies — 243
statements, 134 of them bare package names; 122 stylesheet imports in JS and 61
`@import`/`@use`/`@forward` statements resolved to nothing. v1's oracle on vite
carried 137 findings on stylesheets, every one file-level: 103 `unused`, 5
`test-only`, 28 whole-file `duplicate`, 1 `unresolved`. Its custom-property,
variable and mixin declarations produced none — so `kndo:css` extracts no
symbol, and `kndo:html` extracts no declaration at all.

| repo | before | after | claimed | what moved |
|---|---|---|---|---|
| vite | 999 | 834 | 1,558 → 1,946 | −338: 336 `unused` files a page reaches (243 js, 71 ts, 10 tsx, 8 jsx, 4 others) and 2 symbols; +173 below |
| lodash | 16 | 19 | 54 → 65 | −6 `unused` (`dist/*.fp.js`, `perf/*.js`, vendor scripts — the HTML edge this file predicted); +3 `test-only` (vendor scripts only `test/*.html` loads), +5 `unused` css (firebug-lite's skin, loaded by its own JS through strings), +1 `undeclared` (`@playwright/test`, imported by the runner and its config, declared by no manifest) |
| Alamofire | 599 | 591 | 108 → 444 | jazzy's 336 doc pages reach their scripts: −10 `unused`, +2 `untested` |
| Exposed | 987 | 986 | 809 → 5,150 | Dokka's 4,341 pages reach `docs/api/scripts/*.js`: −7 `unused` files, +4 `unused` symbols inside them, +2 `untested`; 1.25 s → 2.3 s |
| flask | 26 | 28 | 83 → 105 | +2 `unused` css: sheets Jinja names through `url_for('static', filename=…)` — framework knowledge, a `kndo:flask` plugin's to read |
| guava, vapor | — | — | +1, +3 | findings unchanged |

**vite's +173.** 68 `unused` stylesheets (47 css, 21 scss): the same class as
the playground's e2e fixture files this file already explains — reached only
through a bundler alias (`=/nested`, `@/x`), a `new URL()`, a test's string, or
a file that is itself dead; v1 accused 103. 60 `untested` files (42 js, 12 ts, 3
tsx, 3 jsx): app files a page now reaches that no test imports — the playwright
specs drive a browser, and the graph cannot see that. 13 `unused` symbols:
exports of files now reached, judged one by one where the whole file was dead
before. 21 dependency verdicts from manifests judged now that their html and
css importers are claimed: 11 `undeclared` (5 are `resolve.alias` names imported
bare in `playground/alias`, by design; `virtual-with-scheme`, a test's
`__F_ABSOLUTE_PACKAGE_PATH__` placeholder, a deliberate `missing-modules`, 2
declared only in a sibling package), 3 `unused` (`@tailwindcss/postcss` and
`autoprefixer` are postcss config keys, never imports; `@vitejs/test-package-f`),
7 `test-only` in `__tests__` packages. 7 `unresolved` in pages: 3 in
`playground/lib/index.dist.html` name built bundles the tree does not carry; the
rest are `resolve.extensions` and `resolve.alias` cases a config resolves. 4
`test-only` stylesheets only test pages reach.

**What the corpus corrected before shipping.** A `<script src>` spelled inside a
`document.write` string in lodash's test pages produced nine `unresolved`
findings: a script's body is raw text, never markup, and the scanner now skips
it. A non-ASCII page panicked the byte-walking inline scan on vite (a `str`
slice mid-character). An inline `import 'normalize.css?inline'` reached the
dependency judgment with its query and produced an `undeclared`/`unused` pair
on one manifest: a loader's query is never part of a package name, now in the
contract's `package_of` and `names`.

**Not inherited.** v1's 28 whole-file `duplicate` findings on vite's html and
css: identical playground scaffolds, and a document declares nothing to
fingerprint — `duplicate` stays a structural clone over declarations with
metrics, and the html/css files abstain under it by declared absence
(`streams-not-declared`, files scope), the one new abstention row every
web-carrying report gains.

**json, measured and declined.** 386 json files on vite, 22 imported from JS in
18 files; v1's own json findings on vite were 15 `unused` tsconfigs and 20
unreferenced data files (its 500-odd `package.json` rows are dependency
subjects, which v2 already judges). A config file's consumer is a tool, never an
import, so claiming it manufactures the accusation. The 22 edges keep alive
files nothing accuses today.

## M7.e — three more report formats, each from its producer (2026-09-02)

The corpus carries no coverage, so the instrument is the same as `crap`'s:
real producers run on scratch copies, reports captured, kndo run with and
without them.

- **go-cover on gin** (`go test -coverprofile`, 1,641 blocks keyed by import
  path): 108 → 115 findings, the seven `untested` and Certain — `ginS`'s
  `Run`, `RunTLS`, `RunFd`, `RunUnix`, `LoadHTMLFS`, `LoadHTMLFiles` and a
  sibling, the wrappers gin's own suite never executes. Nothing at `crap`'s
  line.
- **cobertura on flask** (coverage.py, 482 tests): 46 findings — 26
  `untested` (17 Certain), 1 `crap` — and the lcov report from the SAME test
  run gives the identical 46. Two formats, one judgment.
- **jacoco** (the jacoco Maven plugin, on a two-class Maven project): the
  method counters are function records — `Classify.neverRan` and
  `Dark.untouched` Certain, `grade` covered — the primary evidence a declaration
  line's own hit cannot give.

The one engine change underneath: a report's own spelling maps onto the ONE
project file it names — `github.com/gin-gonic/gin/render/json.go` onto
`render/json.go`, `demo/Classify.java` onto `src/main/java/demo/Classify.java`,
coverage.py's `classify.py` under its recorded source root onto
`src/classify.py`; an ambiguous spelling maps onto nothing rather than credit
the wrong file. Every corpus and conformance report gains three always-on
ingester rows and nothing else: 89 conformance reports regenerated, audited to
differ by those rows alone.

## M7.f — launchers: what a workflow runs is a root (2026-09-02)

vite 835 → 831: `scripts/detect-release.ts`, `scripts/extract-changelog.ts`
and `scripts/prepare-release.ts` — handed to `node` by
`.github/workflows/publish.yml` and `prepare-release.yml` — and
`scripts/releaseUtils.ts`, which two of them import. v1 accused the same four:
its walk never entered `.github/` either, and its scripts rule read
`package.json` alone. No other corpus repository launches a project file from
a workflow (python, go, cargo, swift: none), so each gains only its `.github`
files in `files_discovered` and nothing in findings. Surface v1 never had: a
composite action's own file through `$GITHUB_ACTION_PATH` — the dogfood's
`action/render.mjs`, the finding that started this — and a JavaScript action's
`main`/`pre`/`post` entries.

## M8.a — markers and dispatch: the attribute becomes evidence (2026-09-05)

ripgrep 155 → 154: `unused` 4 → 3. The const `SHERLOCK_CRLF` in
`crates/printer/src/standard.rs` carries its own `#[allow(dead_code)]`; the
rust adapter now reports the attribute as a marker and the spec's rule reads
it as an exemption, so the accusation the source had already disclaimed is
gone — `used-by` answers `exempt`. v1 accused it too (its allow handling was
`kndo:allow` pragmas only), so this is the second contract vice retired on
ripgrep: an attribute is the code's own statement, and a judgment that ignores
it is not reading the code.

Of the three `unused` that remain, `Handle.read_write` and
`Handle.read_write_mut` in `crates/index/src/index.rs` sit under the crate
root's `#![allow(warnings)]` in `crates/index/src/lib.rs`. rustc scopes a
crate root's inner attribute over the whole crate; the marker sits on the file
that carries it and dispatch exempts that file's declarations only, so the two
stay accused — the `crate-level-allow` fixture holds this as a known gap with
its fix named (M8.b units: a unit root's file-level markers dispatch over the
unit). The third, `crates/index/src/index.rs` aside, is the grep-verified true
positive of M4.

Every other repository byte-identical — the change is rust's alone, and every
root the adapter's attribute table used to state is the root the rules derive:
all 27 existing rust conformance fixtures re-pin to the same bytes.

## M8.b — the engine stops guessing generously (2026-09-06)

Three adapter under-reports and one engine over-keep, measured against each
other. The engine's surface keepers used to hand out private members: an entry
point's exported API and a namespace importer kept every member of a file,
whatever the language exported. Removing that keep is a lens — it shows which
adapters were being carried by it.

**First attempt: 47 new `unused`, 27 of them false.** Exposed 17, guava 28,
vapor 2. Reading the sources: Exposed's seventeen were all names USED in their
own file (`iterations: Int = DEFAULT_ITERATIONS`, `"DEFAULT CHARSET=$charset"`)
that the Kotlin adapter never reported; ten of guava's were declarations
carrying `@SuppressWarnings("unused")`, whose own comment reads "many methods
tested reflectively". The rule was reverted rather than shipped at 43%
precision.

**Three root fixes, each measured alone.**

| fix | what it recovers | corpus effect |
|---|---|---|
| kotlin: a parameter's default value is a use | every constant a primary constructor defaults to | Exposed +148 references, 0 findings |
| java: annotations are markers, `@SuppressWarnings("unused")` exempts | what the author already declared | guava 9,925 → 9,837 (−88 `unused`, all suppressed at the source) |
| kotlin: `$name` in a string template is a use | the properties SQL builders read | Exposed −5 `unused` |

The Kotlin fixes changed no finding on their own (the surface keep was already
hiding those declarations) and 148 references + 5 findings once measured
against it — the shape of a masked defect.

**Shipped: 19 new `unused`, zero false.** guava 17 (private test helpers whose
name appears exactly once in the file: `ForwardingCacheTest.OnlyGet`,
`Fingerprint2011Test.LENGTH_FINGERPRINTS`/`MAX_BYTES`,
`HashingTest.MAX_PERCENT_SPREAD`, `ImmutableSetTest.HASH_FLOODING_FPP`,
`FuturesTest.MapperFunction`/`constantAsyncCallable`/`TestException`, ×2 for
the android mirror), vapor 2 (`insertOrReturn`, `checkBodyStorage`, each
declared once and never called). Net across the corpus: guava −71, Exposed −5,
vapor +2. Every other repository byte-identical, and every conformance fixture
in every corpus byte-identical.

### The namespace pool, and the one that was rejected (2026-09-06)

Java's package region stopped being an adapter convention and became a scope
node the engine derives. Before shipping it, the LOOSE version was measured:
a namespace keyed by its segments alone, so every file spelling
`com.google.common.collect` shares one pool wherever it sits.

| pool | guava findings | delta |
|---|---|---|
| segments alone | 6,626 | `+0 -2994` — `internal-only` 2,961, `unused` 33 |
| `(source root, segments)` — shipped | 9,361 | identical to the previous run in every field |

The 2,961 silenced advisories, decomposed by where the sibling that silenced
each one lives:

| the disqualifying use sits in | count | is it real? |
|---|---|---|
| a different source root | 2,653 | no — guava's `android/` mirror is a separate compilation |
| the same build's test tree | 263 | yes — needs units (M8.d), not a looser scope |
| the same source root | 45 | yes — already recovered by the shipped pool |

Eighty-nine percent of what the loose pool "fixed" was guava's Android mirror
pretending to be the same program. The oracle's name-fuzzy resolution makes
exactly this mistake; buying 263 true fixes with 2,653 false silences is the
trade v2 exists to refuse. The 263 return when Maven and Gradle say which files
one compilation contains.

The report's extension row now carries the ladder — `private`/`package`/`public`
for java — in place of the single `narrowable_scopes` token, so the row still
answers "why does `internal-only` fire here", and now also says what to type.

### Units: a package spans the classpath (2026-09-06)

Maven's reactor entered the model, and guava's separate test artifact stopped
looking like a stranger to the library it exercises.

| guava | findings | internal-only |
|---|---|---|
| namespace keyed by source root | 9,620 | 3,051 |
| keyed by unit, span declared | 9,011 | 2,474 |

589 findings retired, none added, every other repository byte-identical.
Decomposed by whether the use that silenced each one can be corroborated — the
file naming the member also names its owner type:

| class | count | verdict |
|---|---|---|
| corroborated | 321 | real cross-artifact uses; the advisory was wrong |
| bare name only | 268 | the name-only reference test, not the scope model |

Three corroborated ones read by hand — `LongMath.FLOOR_SQRT_MAX_LONG`,
`BloomFilter.optimalNumOfHashFunctions`, `TreeRangeSet.rangesByLowerBound` —
are all used under exactly that spelling from `guava-tests`. Ten sampled from
the bare-name class are all coincidences: a local `CountDownLatch latch`
silencing `FinalizableReference.latch`, a parameter named `encoding` silencing
`EncodingOption.encoding`, a doc comment saying "head".

The trade is deliberate and one-directional: every change is a silence, so the
tool stops telling developers to narrow members their own test module uses, and
loses 268 advisories to a coincidence it already had. Qualified references is
the slice that takes those back, and 268 is the number it will be measured
against.

### Qualified references: the name is not the member (2026-09-06)

| guava | findings | internal-only |
|---|---|---|
| pool by unit, bare-name matching | 9,011 | 2,474 |
| a member needs an access | 9,741 | 3,166 |

730 advisories recovered (692 by distinct id, the rest overloads that share
one), none removed, every other repository byte-identical —
only `kndo:java` declares the qualifiers stream, so every other language keeps
the answer it gave before the stream existed. Fifteen of the recovered were
read by hand and all fifteen are true: what silenced them was another class's
identically-named member, a test double's own field, a local variable, a static
import of a different class's method, or prose in a comment.

Against the pre-unit baseline (9,620) the two slices net to +121: 609
accusations withdrawn because a sibling artifact really uses those members, 730
restored because a coincidence never did. Precision moved in both directions
for the same reason — the engine stopped answering a question about a SYMBOL
with evidence about a WORD.

### Identity: every declaration has its own (2026-09-06)

Finding identity hashed `Owner.name`, so overloads shared one id — and one
baseline entry, and one query address. The selector is now a key the evidence
sink makes unique by construction.

| repository | duplicated ids before | after | findings carrying a signature | positional `#k` |
|---|---|---|---|---|
| guava | 152 (277 rows) | 0 | 6,084 | 17 |
| Exposed | 18 (45 rows) | 0 | 0 | 57 |
| Alamofire | 21 | 0 | 0 | 33 |
| vapor | 2 | 0 | 0 | 3 |
| vite | 1 | 0 | 0 | 1 |
| flask, gin, lodash, ripgrep | 0 | 0 | 0 | 5, 0, 0, 1 |

Every row is unchanged as a multiset of (category, subject, lines): this slice
changed what findings are CALLED, not which exist. Java states signatures, so
its `#k` cases are only the ones no signature can split — two `ImmutableSortedMap.of(K, V, …)`
whose parameter types spell identically; Kotlin and Swift state none yet and
lean on position until their adapters migrate.

### Identity, the rest of the family (2026-09-06)

Imports written twice and allows written twice now carry their position, like
overloads do; a gate over every pinned report refuses two findings with one
identity. Corpus: zero identities moved, zero duplicated, every row unchanged.
This slice changed no verdict; it made the class of defect impossible to
reintroduce without a red gate.

### `internal-only` on the ladder (2026-09-06)

The analysis now reads one fact about a language — its ladder — and names the
keyword to type. Nine repositories, before → after:

| repo | before | after | retired | what the ladder says |
|---|---|---|---|---|
| guava | 3,204 | 3,184 | 20 | top-level package-private classes: Java spells nothing between the file and the package for a free declaration |
| Exposed | 31 | 23 | 8 | `internal` members used from elsewhere in their file: `private` would break them, and Kotlin has no file-wide keyword for a member |
| Alamofire | 264 | 264 | 0 | 24 now read `private`, 240 `fileprivate` |
| vapor | 85 | 85 | 0 | 16 `private`, 69 `fileprivate` |
| ripgrep | 1 | 1 | 0 | `pub(crate)` → `private` |
| vite | 51 | 51 | 0 | `export` → `unexported` |
| flask, gin, lodash | 0 | 0 | 0 | no ladder (Python), nothing below the package (Go), nothing exported and unnamed (lodash) |

Zero findings added anywhere; every finding of every other category is
byte-identical. Both retired classes were advice naming a keyword that does not
exist — the defect a step's bearer and the owner extent make unrepresentable.

### Friends and publication: the surface the engine now states (2026-09-06)

The corpus is byte-identical: the adapters' library-mode roots still stand, and
Maven's inherited source directories wait for M8.d. The instrument — guava with
`kndo:java`'s library-mode root deleted, on the engine's publication alone:

| guava | findings |
|---|---|
| the adapters' convention | 9,721 |
| the engine's publication | 9,716 |
| added | 9: seven GWT super-source files dead as files, `ListSizeDistribution.chooseSize` twice |
| removed | 14: the same seven files' symbol and `untested` findings |
| changed in place | 0 |

All 16 verified in the sources, every one an improvement. The published
surface is the engine's statement now, and the adapters' copies can be deleted
with a number behind each.

### File roles: the unit's kind, read where Maven puts it (2026-09-06)

| guava | before | after |
|---|---|---|
| findings | 9,721 | 9,593 |
| `untested` | 147 | 17 |
| `internal-only` | 3,184 | 3,186 |

The 130 retired `untested` are benchmark and test files the pom calls test
sources — colored production by a path convention that never saw guava's
`test` and `benchmark` directories, now test roots by their unit's kind. The
two added advisories are collision-driven both ways and true. Every other
repository byte-identical. Instrument: with java's directory conventions
deleted, every fixture with a pom is byte-identical, and guava moves only by
the seven GWT super-source files already decomposed.

### The duplicate floor for files (2026-09-06)

| repo | before | after | retired |
|---|---|---|---|
| vite | 828 | 689 | 139 fixture stubs and template configs, 0 to 362 raw bytes |
| guava | 9,593 | 9,555 | 38 `package-info.java` and empty holder classes of the mirror |
| Exposed | 973 | 971 | 2 hello-world snippets |
| flask | 28 | 26 | 2 empty `__init__.py` markers |

Zero findings added, zero changed in place, every other repository
byte-identical. Function clones are untouched: the smallest the token floor
admits are real, so the byte floor belongs to files, measured outside their
comments.

### Discovery ignores: what the tool never compiles (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| vite | 689 | 687 | 7 files under a committed `node_modules` unclaimed (5 JavaScript, 2 SCSS); 2 `unused` file findings on them retired |
| gin | 108 | 108 | byte-identical under the corrected declaration; the first declaration (`testdata` ignored) added a false `undeclared` on the module's own `testdata/protoexample`, imported by three test files |

Zero findings added, zero changed in place, every other repository
byte-identical. Two vite `test-only` dependency findings that the first run
retired came back once an unclaimed file under an ignore stopped casting
`unclaimed-importers` doubt on its manifest: they were an artifact of the
mechanism, not a judgment.

### Embedded regions: inline scripts and styles read by their language (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| vite | 687 | 711 | −2 `unused` files reached through an inline style's `@import url()` and an inline module's dynamic `import()`; +20 `untested` playground pages whose inline modules declare functions; +6 `unused` — three constants inline modules declare and never read, two named exports of `transform-visibility.js` the page never imports (the old whole-surface `Glob` kept them by not looking), and `wasm/imports.js#imported_func`, consumed by a wasm import section no adapter reads |
| flask | 26 | 29 | +3 `untested` example templates with inline functions |
| lodash | 19 | 18 | −1: `firebug.css` reached by an inline `<style>@import` |
| Exposed | 971 | 971 | byte-identical findings; 12,891 inline classic scripts of the generated docs now read (declarations 11,673 → 20,249), their `var`s the pages' globals |

vite's unresolved edges fall 165 → 155 (inline imports resolve under
JavaScript's spellings) and its diagnostics rise 19 → 20 (an inline
`@import url(./imported.scss)` parses partially). Every other repository is
byte-identical.

### Structured reach and the owner's cap (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| guava | 9,555 | 8,242 | −1,315 `internal-only` on members whose owner already fences them (1,295 inside private nested classes); +2 `unused` on `BenchmarkHelpers.chooseSize`, a public method of a nested enum of a package-private test class that nothing references |
| Alamofire | 591 | 538 | −53 `internal-only` on `internal` members of `private` and `fileprivate` types |
| vapor | 218 | 199 | −19 of the same shape, all in tests |
| gin | 108 | 109 | +1 `internal-only`: an exported helper of an `internal` package's test file used in that file alone — `unexported` would suffice |

A member reaches no farther than its owner: advising a narrower word on a
member of a private type changes nothing anyone outside the owner can
name, so v2 does not. Exposed, flask, lodash, ripgrep and vite are
byte-identical; ripgrep's trait items and `pub(super)` items change reach
without changing judgment.

### Heirs: `protected` judged by its owner, its subtypes and its package (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| guava | 8,242 | 8,254 | +12 `internal-only`, six per tree (android, main), on `protected` members used within their owner alone: `AbstractIteratorTester.MultiExceptionListIterator`, `AbstractTableTest.cellValue` and `nullableCellValue`, `AbstractClosingFutureTest.assertFinalStepThrowsIllegalStateException` (test sets), `AbstractBaseGraph.nodePairInvalidatableSet` and `LineBuffer.handleLine` (main, both of package-private owners) — `private` would suffice |

`protected` reaches the owner, its transitive subtypes and, in Java, the
package; a heirs member of a public type in a published unit is published
surface and silent, as `public` is. Alamofire, Exposed, flask, gin, lodash,
ripgrep, vapor and vite are byte-identical in their findings. v1's 28
`protected` findings on guava match none of the 12: 8 are published surface
(guava-gwt's `ForwardingSortedMultiset`, failureaccess's
`InternalFutureFailureAccess`), 8 are `SourceSinkTester` members used from
its four subtypes, 8 are `OldAbstractFuture` members overridden by the
same-file facade or kept by same-named accesses on other receivers, 2 are a
caliper `@BeforeExperiment` entry, and its 2 `unused` on
`SomeClassThatDoesNotUseNullable.protectedButDoesNotCheckNull` ride the
owner's import binding in the test that subclasses it and exercises it by
reflection.

### Rust on mounts and units: a module is its mount chain (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| ripgrep | 154 | 151 | 64 `duplicate` re-identified under the module that owns them (`tests.only_matching`, not `only_matching`); −3 `test-only` false positives on `tests/index/*.rs`, modules of an integration-test crate the unit's kind now colors as tests; one `internal-only` re-pooled over the mount tree |

`mod x;` mounts, `pub mod` no longer re-exports, an item with no `pub` reaches
its module's namespace, an inline `mod` owns what it declares, and every cargo
target is a unit entered through its own file. No `unused` moves on ripgrep:
its crates' exported surfaces are kept by publication or by real uses, and the
one crate that declares `publish = false` (`crates/index`) has no unreferenced
export. Alamofire, Exposed, flask, gin, guava, lodash, vapor and vite are
byte-identical — rust claims none of them. v1's oracle is no comparison here:
it read `pub` as exported everywhere, kept every `pub mod` surface alive, and
named a test fn inside `mod tests` without its module, so its ripgrep numbers
answer a different question in all three respects.

### Paths inside macro tokens: three edges, no finding (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| ripgrep | 151 | 151 | nothing — 3 import edges appear (309 → 312), no category moves |

The audit's largest `Certain` false-positive class for rust — a `pub` item
reached only through a macro argument — was retired by the mount model before
this change and does not reproduce. What the reconstruction adds is the edge:
`used-by` and `trace` now answer for a cross-crate path spelled only inside a
macro. Marked `Possible`, so dependency hygiene never accuses a manifest on a
path the grammar did not parse; the first, uncorrected run of this scan added
four false `undeclared` on ripgrep and one on this repository, which is what
sent the confidence down and the two run defects up. Every other corpus
repository is byte-identical, as rust claims none of them.

### `#[path]`, aliases and includes: three false positives with no corpus population (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| every repository | — | — | nothing |

`include!` appears in no rust file of the corpus, and ripgrep's only `#[path]`
sits in a `mod.rs`, the case that already worked — so the three `Certain`
false positives the audit verified (a redirected module reported `unused`, its
alias reported `undeclared`, an included file reported `unused`) have no corpus
population at all. The probes and the `path-attribute-and-include` fixture are
the measurement; the corpus number is zero by construction, and stays zero.

### Go on namespaces and units: the library-mode root retires (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| gin | 109 | 110 | +1 `test-only` on `testdata/protoexample/test.pb.go` — a generated file only gin's tests import, which the test files' own colour hid |

The package became a namespace declared from the package clause and the
directory, the module became one published library unit, and `kndo:go`'s
library-mode whole-file root — the `Probable` Production root it put on every
non-internal, non-main, non-test file — retired. That root was carrying
nothing gin needed: all seven of gin's packages declare an exported top-level
name, so every file either roots on the module's published surface or is seen
by a file that does. The single delta comes from the engine defect the
migration exposed: a `_test.go` file declaring `func TestXxx` was riding the
module's published surface as a production root, and every gin test file was
production-coloured because of it. With test files off the surface (no
importer can name what they export) the file they alone import is test-only,
correctly.

v1's oracle reported five `test-only` on gin, and none is the one v2 reports —
they are five different subjects. Two are dependencies (`testify`,
`reflect2`) whose message is "belongs in devDependencies": v2 declares go
`DependencyScoping::Unscoped`, because go.mod has no dev section and the
verdict has nowhere to move a requirement to, so naming npm's remedy for a Go
manifest is v1's vice and v2's silence is deliberate. The other three are
declarations — `test_helpers.go#waitForServerReady`, `utils.go#localhostIP`,
`utils.go#localhostIPv6` — and v2's `test-only` has no declaration subject at
all: it names files and dependencies. That is a v2 scope decision this slice
does not touch; what the slice adds is the one file subject v1 never had, and
what makes it possible is that a file is no longer painted production by its
own test siblings. Alamofire, Exposed, flask, guava, lodash, ripgrep, vapor
and vite are byte-identical: no other language's test files sit inside a
published library unit, because every other build system gives tests a unit of
their own.

### Co-visibility off the forest: the same numbers by the right mechanism (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| every repository | — | — | nothing — every report byte-identical |

`Extension::sees` asked each adapter to enumerate, from paths, the files a
given file compiles with; go's answer was its directory, with `_test.go`
handled asymmetrically. The scope forest already holds that set as a namespace
node, so go now declares `Covisibility::Namespace` and reachability floods over
the node instead of over an enumerated directory — with the test asymmetry
stated once in the engine rather than per adapter. Every corpus report is
unchanged, which is the result this kind of swap must produce: the forest holds
exactly what the hook re-derived. What proves the capability load-bearing is
the conformance case, which reports `unreachable` in place of `production` when
the declaration is removed.

### Go's grammar fields: five thousand references that were never uses (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| gin | 110 | 110 | no finding — but 1205 → 1253 declarations and 43,995 → 38,999 references |

Four field misreadings and a receiver, each verified against the grammar: a
grouped `var` hides its specs under a `var_spec_list` a one-level walk never
entered (48 names invisible in gin); a multi-name `const` labels its separating
commas with the `name` field, so an unfiltered read declares a symbol called
`,`; `is_use` excluded only the FIRST `name` child, so the second name of
`var a, b int` was a use of itself; `package_clause` carries no field, so the
clause read as a use of anything sharing the package's name; and a method's
receiver type — which Go requires to be declared in the same package — was a
use rather than part of its type's definition, making any type with a method
unaccusable. The `:=` binder positions go with them.

gin's verdict does not move, which is the result worth reading: none of the 48
newly visible grouped names is dead, no type there is kept alive only by its
own receiver, and none of the ~5,000 removed references was the last keeper of
anything. Eleven percent of the reference stream was carrying nothing. The
oracle offers nothing to compare against here — it publishes findings, not
declaration or reference counts, and on gin it reports one `duplicate` where v2
reports 108, so the two are not counting the same population.

### The generated header, and G10 measured shut (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| every repository | — | — | nothing |

Two go close-out items, neither with a corpus population. The generated-file
marker scan is now bounded by the file's HEADER rather than by 24 lines —
`cmd/go`'s own rule, promoted to the toolkit where eight adapters shared the
bound — which no corpus repository exercises, because none stamps a marker
under a licence that long. The probe is the measurement: a file with the marker
at line 26 reported its declaration `unused certain` and is silent now.

And the audit's largest go gap closes as a number rather than as code. Ablating
the whole-surface-importer keeper — the most a see-through keeper could ever
retire — leaves gin at 110 and flask at 29, against vapor 199 → 215 and
Exposed 971 → 975. An exported Go name is kept by its module's published
surface, so the surface import is never its only keeper: Go is the language
that always spells `pkg.Name`, and the language where nothing is accusable
behind it. The keeper and the first qualifier emission belong to swift and
kotlin, where the prize is.

### The co-visibility gate comes off: guava's cross-tree tests (2026-09-06)

| repo | before | after | what moved |
|---|---|---|---|
| guava | 8254 | 8251 | −3 `untested` on files whose test lives in a sibling source tree under the same declared package |

The go slice gated the co-visibility flood behind a capability the design never
had; the design reads `Scopes::covisible` unconditionally and lets the shape of
the declared namespace decide what the set holds. With the gate removed,
`guava/src/com/google/common/xml/XmlEscapers.java`, its android twin and a
`test-super` GWT mirror stop being reported untested —
`guava-tests/test/com/google/common/xml/XmlEscapersTest.java` exists, and the
namespace node reaches it where java's directory-and-mirror `sees` does not.
Every other repository is byte-identical, which is what a gate removal must
look like when the languages behind it declare namespaces of one file.

### A generator's output is judged by one law: `Effect::Generated` (2026-09-07)

| repo | before | after | what moved |
|---|---|---|---|
| gin | 110 | 109 | −1 `test-only` on `testdata/protoexample/test.pb.go` |
| guava | 8251 | 8250 | −1 `duplicate` on `guava/src/com/google/thirdparty/publicsuffix/PublicSuffixPatterns.java` |
| Alamofire, Exposed, flask, lodash, ripgrep, vapor, vite | — | — | byte-identical |

Three generated files across nine repositories: gin has one, guava has two
(the `PublicSuffixPatterns.java` pair), and nothing else in the corpus stamps a
banner in its header. A small population is the honest one — this slice's claim
is a single law where eight adapters had four different answers, not a new
finding count.

**gin.** The file reports its declarations now instead of hiding them, and one
of them is `func init()`, which the Go runtime calls on package load; a
Production root therefore reaches the file, and "only tests reach this file" was
simply not true of it. Withholding judgment from a generated file's NAMES never
meant hiding the program's shape from the engine — its imports, its references
and the roots it declares are evidence like any other file's.

**guava.** `android/guava/.../PublicSuffixPatterns.java` and
`guava/.../PublicSuffixPatterns.java` are byte-identical output of the tool that
compiles the public suffix list. `duplicate` asked this project to unify two
files it did not write; the seam now abstains and the finding is gone.

**Nothing became more accused.** The owner's decision was that being generated
withholds judgment from the file's declarations and does NOT root the file — so
a generated file nothing imports is reported `unused` like any other orphan,
and v2's old rust/ts/css `Tooling` root is retired. No generated file in this
corpus is an orphan, so that half costs nothing here; it is pinned in fixtures
instead (`kndo-adapter-ts` `generated-file` holds both halves,
`kndo-adapter-css` `generated-sheet-is-an-orphan-like-any-other` the orphan
one).

**Against the oracle.** v1 rooted generated files as tooling output — the rule
v2's rust, ts and css had inherited. v2 does not, and the reason is not a
number: a root asserts that something outside the graph USES the file, and a
`DO NOT EDIT` banner asserts nothing of the kind. It says who WROTE it.

### A name that dispatches is declared: `Trigger::Name` (2026-09-07)

| repo | before | after | what moved |
|---|---|---|---|
| every repository | — | — | byte-identical |

go's `TestXxx`/`init` roots and rust's `fn main` root left extraction for the
spec, qualified by the file's ROLE instead of by its path, and every report
agrees with the previous run to the byte. That is what a faithful mechanism
swap looks like; the interesting numbers are the two ablations behind it.

**The runner rule earns its place: gin 109 → 119 without it.** Dropping go's
`Test*`/`Benchmark*`/`Example*`/`Fuzz*` rule adds ten `unused` findings, all
under `internal/bytesconv` and `internal/fs`. Everywhere else in gin an
exported `TestFoo` rides its file's entry surface; under the `internal` fence
its reach is the fenced subtree rather than the published surface, so the
runner's own root is the only thing that keeps it. A rule that changes nothing
on the corpus would have been the one to delete — this one is load-bearing in
exactly the place Go's visibility rules make it necessary.

**rust's color now comes from cargo, not from the path.** `main_root_kind`
read `build.rs` and `examples/` out of the path; the rule reads the target kind
cargo declared for the file. The two agree on every corpus repository, so
nothing moves — but they disagree on a `src/examples/` module (library code in
a directory that shares an example's name), which the path convention would
have colored tooling. The fixture `main-in-every-target` pins the three
targets a `fn main` can sit in.

**The prefix glob's measured cost: zero.** `go test` runs a name only when the
character after `Test` is not lowercase; the pattern grammar has `*` and
nothing narrower. Of gin's 658 runner entries, none would be over-matched by
the bare prefix, and where it could happen the direction is keep-alive inside a
file already rooted Test. The grammar stays as it is until a repository pays
for the difference.

### A promise its owner made: `Effect::Witness` (2026-09-07)

| repo | before | after | what moved |
|---|---|---|---|
| every repository | — | — | byte-identical |

java's `@Override` Production root and its hardcoded `SERIALIZATION_HOOKS`
name list left extraction for two spec rules — `@Override` means WITNESS, and
the JDK bases the graph can never resolve name their own requirements — and
every report agrees with the previous run to the byte. The ablations are where
the numbers are.

**`@Override` keeps 52 members on guava**, and it now keeps them without
painting their files production: a root asserts that something outside the
graph is entered, and an override asserts nothing of the kind.

**The base table is 9 members, and invisible behind a broader keeper.**
Removing it changes nothing (8250 → 8250) because `keepers` keeps any member
alive on a reachable reference to its NAME, anywhere. Ablate that keeper and
the table's contribution appears: 27,405 findings without it, 27,396 with.
The same ablation sizes the keeper itself at roughly nineteen thousand guava
members — recorded in EXPERIMENTS as the first measurement of the plan's
"witnesses replace the keep-alive by name collision".

**The first attempt over-narrowed, and the corpus said so.** Matching only
the relation a file reports itself put 18 new `unused` on guava, all
`readResolve` on classes serializable through a base (`Absent` through
`Optional`). The runtime does not care which link named `Serializable`, so the
matcher walks the whole declared supertype chain by name.

**A defect the change exposed.** `internal-only` stood down for a witness the
graph resolved but not for one a rule states, so guava's 37 JUnit
`setUp`/`tearDown` overrides became findings the moment `@Override` stopped
being a root. Both halves read one seam now, and the 37 are gone again.

### The dispatch vocabulary, entire (2026-09-07)

| repo | before | after | what moved |
|---|---|---|---|
| every repository | — | — | byte-identical |

`Trigger::Relation { kind, to }`, `Trigger::MemberOf { owner, name }`,
`Marker.target` and the Pattern qualification landed together, and no report
moves. That is the expected shape: `@Override` narrowed to methods changes
nothing because the Java compiler already rejects it elsewhere; the
qualification changes nothing because java's own rules name JDK types the
source writes bare and no import qualifies; and the two new triggers have
their language consumers in the rule packs still ahead (M8.e).

What the fixture proves instead of a corpus number: in
`runtime-required-members`, two package-private `shut()` methods sit on two
classes implementing two different `Closer` interfaces. The rule names
`com.vendor.Closer`, and the file's own import decides — one is
`witness:com.vendor.Closer`, the other is reported. Nothing about the two
files differs except which package their import names.

### Attachment: the carrier moves, the verdicts do not (2026-09-07)

| repo | before | after | what moved |
|---|---|---|---|
| every repository | — | — | byte-identical |

`Attachment { Regular \| TestOnly }` became the file's own statement of which
build carries it, `InFiles` was deleted, and `Trigger::Name.in_unit` began
reading the compilation instead of a whole-file root. Seven adapters state
the attachment where their tooling compiles a file into the test build alone.
No report moves — a carrier change, not a judgment change.

**What the ablation sizes.** With the seven emissions removed and
`is_test_file` still reading the attachment:

| repo | with | ablated | what the attachment carries |
|---|---|---|---|
| gin | 109 | 119 | 10 `Test*`/`Benchmark*` runners in `internal/**_test.go` |
| vite | 711 | 704 | 7 `test-only` dependencies of two `__tests__/package.json` |
| guava, Exposed, vapor, Alamofire, flask | — | — | unchanged: the manifest declares test units |

The split is the point. Where a build system names a test unit (Maven source
sets, SwiftPM test targets), the unit already answered and the attachment
only agrees with it. Where none does — go's `_test.go`, the web's
`__tests__/` — the file is the only witness, and without it the runners are
accused and the test dependencies lose their colour.

### `sees` retires in java and swift (2026-09-07)

| repo | before | after | what moved |
|---|---|---|---|
| guava | 8250 | 8249 | −1 `untested` |
| every other repository | — | — | byte-identical |

Swift's `sees` measured zero before it went, so nothing could move there.
Java's carried the co-visibility edge a test needs to reach the class it
exercises, and `Scopes::covisible` now carries it instead — spanning the
namespace across the units this one COMPILES AGAINST, the inverse of the pool
direction.

The one finding is `guava-gwt/.../ForceGuavaCompilationEntryPoint.java`, which
stops being `untested`. guava's parent pom declares
`<sourceDirectory>src</sourceDirectory>`; guava-gwt's main set is therefore
`src/` and its test set `test/`, and `guava-gwt/test/com/google/common/GwtTestSuite.java`
is that file's package-mate in its own module's test set. Java's directory
mirror only knew `src/main/java` ↔ `src/test/java` and could not see it. v2
reports one finding fewer because it read the project's manifest instead of
guessing from directory names — the difference the plan was built to make.

The union of both span directions was tried first and rejected by measurement:
it made fifteen guava-gwt GWT super-source files (compiled INSTEAD of the
library's, never beside them) look exercised.

### `sees` leaves the vocabulary (2026-09-07)

| repo | before | after | what moved |
|---|---|---|---|
| Exposed | 971 | 976 | +5 `untested` |
| every other repository | — | — | byte-identical |

Kotlin now declares its `package` clause and `NamespaceSpan::Compilation`, and
`Extension::sees` is deleted everywhere — trait, WIT, SDK, host, guests, core.

The ledger sized kotlin's `sees` at 15; the clause recovers 10. The five that
remain are `exposed-migration-r2dbc/.../MigrationUtils.kt`, the springboot3
sample's three, and `MixedDatabaseTestsBase.kt` — each has a test in its own
package, in its own module's `src/test/kotlin`, but main and test are different
namespace ROOTS there and only a unit's friendship joins them. Kotlin reads no
Gradle yet, so it has no units: M8.d closes these five, and they are the cost,
named, of one vocabulary instead of two.

### Package.swift becomes a structural manifest (2026-09-07)

| repo | before | after | what moved |
|---|---|---|---|
| vapor | 199 | 218 | +19 `internal-only` |
| every other repository | — | — | byte-identical |

SwiftPM's targets are now units, with their `path`/`exclude`/`sources` roots
and their test targets as friends of what they test. The 19 are declarations
the old `module_region` could not narrow, because it bounded `internal` by the
target's files PLUS every file under any `Tests/` tree — a superset chosen when
friendship could not be read. `TestError` and `Payload` are each declared and
used in a single file, and `Performance/` is its own package.

`seen_from` for swift drops from 486 findings to 4, all in Alamofire's
`Example/` and `watchOS Example/` — Xcode projects no SwiftPM target covers.

### `seen_from` leaves the vocabulary (2026-09-07)

| repo | before | after | what moved |
|---|---|---|---|
| Alamofire | 538 | 534 | −2 `internal-only`, −2 `unused` |
| Exposed | 976 | 934 | −23 `internal-only`, −19 `unused` |
| every other repository | — | — | byte-identical |

No adapter hands the engine a list of files any more. Where a manifest names
the unit, its pool answers; where the language mounts its namespaces, the tree
answers; where neither does, the reach is unbounded and v2 abstains — the
contract's own law, and the reason both numbers go DOWN.

Alamofire's 4 are in `Example/` and `watchOS Example/`, Xcode projects no
SwiftPM target covers. Exposed's 42 are kotlin `internal` declarations that a
`/src/`-shaped path convention used to bound; kotlin reads no Gradle yet, so
nothing names its units. Both close when their parser lands — swift's already
closed 482 of its own 486.

### Twenty whole-file roots leave the adapters (2026-09-07)

| repo | before | after | what moved |
|---|---|---|---|
| Alamofire | 534 | 426 | −113 `unused` XCTest case classes, +3 `internal-only`, +2 `unused` |
| vapor | 218 | 176 | −42 `unused` XCTest case classes |
| vite | 711 | 691 | −20 `untested` HTML pages |
| flask | 29 | 26 | −3 `untested` HTML templates |
| guava | 8249 | 8247 | −2 `unused` |
| lodash | 18 | 20 | +2 `test-only` |
| Exposed, gin, ripgrep | — | — | byte-identical |

Nine adapters stopped reading a path to conclude a root; the spec declares the
convention and the engine applies it. Four judgments the old roots had been
masking come out of that, and every number above is one of them.

**The 155 XCTest classes.** `ApplicationTests`, `CacheTestCase`,
`DataRequestCombineTests` — classes holding nothing but rooted `test*` methods,
reported as dead by v2 because a root on a member did not keep its owner. XCTest
instantiates the class by reflection and runs the methods off it, so the class
cannot be dead while a method of it is an entry. Only a CERTAIN root travels
that way: swift roots a conforming type's non-private methods at `Possible`, and
letting THAT travel keeps every conforming class alive — measured at another 115
findings of silence on Alamofire, which is why the tier decides.

v1 reported none of these either, and for the opposite reason: its name-fuzzy
pooling kept nearly everything in a test target alive. v2 now agrees with v1's
answer here by knowing why, not by inheriting the vice — the ~130 genuinely dead
test-case HELPERS v2 reports and v1 missed are untouched by this change.

**The 23 HTML pages.** A page's Production root moved from adapter evidence to
an engine anchor, and `untested` has always exempted an engine-anchored
production entry: it is wiring, an entry nothing can import, so the question of
whether a test reaches it is asked of what it leads to instead. vite's
`playground/*/index.html` and flask's Jinja templates are exactly that. The
pages' inline `<script>` regions are why they carried functions to be asked
about at all.

**lodash's +2.** `fp/_baseConvert.js` and `fp/placeholder.js` are reached only
from `test/test-fp.js`. That file had been a TOOLING entry — lodash's
`package.json` names `"test:fp": "node test/test-fp"`, and an npm script's file
roots as tooling whatever the script is called — so the tooling colour flooded
from a test file onto everything it reaches. A file the test build alone
compiles now seeds no other colour, and both files are correctly `test-only`.

**Alamofire's +5.** Three `internal-only` advisories in `Example/` and `watchOS
Example/` (`Sections`, `HTTPBinResponse`, `Networking.result`), each an
`internal` name used only within its own file — reachable as advice for the
first time because those Xcode trees now have a bound at all: swift declares its
namespace, and a unit-wide reach whose unit no manifest named falls back to it.
Two `unused` on `ContentViewPreviews` and its `previews`: a SwiftUI
`PreviewProvider`, which only Xcode's canvas instantiates. That is the swiftui
rule pack's to witness (M8.e), and it is on the ledger rather than papered over.

### Python's manifests are parsed (2026-09-07)

| repo | before | after | what moved |
|---|---|---|---|
| flask | 26 | 20 | −5 `untested`, −1 `unused`, all under `tests/` |
| every other repository | — | — | byte-identical |

`pyproject.toml` now states what it always said and kndo could not read: the
distribution, its source root, its console scripts, and — through
`[tool.pytest.ini_options] testpaths` — that `tests/` is a Test unit. The six
findings that leave were the library-mode root's: with no units at all, every
non-test module was production, so `tests/test_apps/cliapp/factory.py` and
`tests/type_check/typing_route.py` were "production-reachable, but no test
reaches this file" — a question asked of files that are themselves the test
material.

Two ablations bound the change from both sides. Deleting the library-mode root
BEFORE the parser existed cost flask +15 (`src/flask/app.py` and `cli.py` among
them, unreached because nothing was left to reach them from); deleting it after
costs nothing, which is what makes it a deletion rather than a trade. Dropping
the pytest Test unit while keeping everything else costs +16, all `unused`
under `tests/` — the unit is what tells the engine those files are the runner's.

v1 is not the comparison here. It read `pyproject.toml` for dependency names
alone and had no notion of a Python unit, so every one of these six is a
question v1 never asked rather than one it answered differently.

### Gradle is read as blocks, and kotlin's library root dies (2026-09-08)

| repo | before | after | what moved |
|---|---|---|---|
| Exposed | 934 | 972 | +28 `internal-only`, +18 `unused`, −8 `untested` |
| every other repository | — | — | byte-identical |

`settings.gradle.kts` and every `build.gradle(.kts)` now state what they always
said: the modules the build includes, the two source sets Gradle's java plugin
gives each module, and that the test set is the main set's FRIEND. Three of
Gradle's own answers are why this is a block scanner over a comment-blanked copy
rather than a line reader — an `include(` spans lines, a commented-out one names
nothing, and a dependency named through `gradle/libs.versions.toml` has no
coordinate in the script at all — and all three are graded against what a
`kndoReport` task printed from inside Gradle 8.14.3
(`crates/kndo-toolkit/tests/captured/gradle.json`).

With a unit under every Kotlin file, kotlin's library-mode whole-file Production
root is deleted. The engine's published surface roots what a published unit
publishes, and nothing roots what it does not.

**+28 `internal-only`, from a category that reported ZERO here before.** Not a
threshold change: `internal` means "the whole module", and with no module the
reach was unbounded, so the rung could not be judged at all. Every one is a
declaration whose only uses share its own file — `TransactionManagersContainer`,
whose implementation sits below it in the same file while other modules import
only `TransactionManagersContainerImpl`, is the shape.

**+18 `unused`, three families.** Fourteen are the `samples/springboot3-exposed-r2dbc`
module: seven Spring beans in `src/main` (`@RestController`, `@Configuration`,
an `EnvironmentPostProcessor`) that only a component scan instantiates, and
seven in `src/test` — an `internal` `@SpringBootTest` class and its six
`@Test` methods. Both are M8.e's rule packs to witness, and they are visible
now because the whole-file root no longer keeps every `src/main` file alive and
because `internal` finally has a bound. One is true: nothing in the whole
repository calls `TransactionManagersContainer.getCurrentTransactionManager`.
Three are the pinned Kotlin grammar, and each now has a fixture holding its gap
open (M8.f):

- `Entity.kt#isPersistedIn` and `References.kt#allReferencesMatch` — `Entity.kt`
  writes four `when` GUARDS (`is CompositeID if allReferencesMatch(…) ->`), and
  the grammar ends its reading of the branch there. `used-by` on both returns
  empty against uses at lines 168, 232 and 325.
- `documentation-website/…/App.kt` — Exposed's own quick-start snippet writes
  `Tasks.insert { … } get Tasks.id`. `get` and `set` are Kotlin's accessor soft
  keywords and the grammar prefers that reading wherever they are an INFIX
  function name, so the file yields NO declarations, `main` among them, and
  nothing roots it. Measured beside it: `a foo b`, `a to b`, `a eq b` and
  `a.get(b)` all parse; `set` fails exactly as `get` does.

**−8 `untested`, none of it lost advice.** Instrumented on the run: three
(`ExposedConfig.kt`, `HelloController.kt`, `UserController.kt`) went
`prod=false test=false` — they are the Spring beans counted above, and `unused`
is the one verdict, not two. Five went `prod=true test=true`: a module's Gradle
test source set is now a stated unit sharing the main set's compilation, so the
test colour reaches `MixedDatabaseTestsBase.kt`, `MigrationUtils.kt`,
`Application.kt` and the two `hints/` files, and the question `untested` asks is
answered rather than dropped.

**The cost of judging `internal` instead of abstaining.** Exposed, release
build, warm page cache, cold graph cache: 1.0s → 1.8s. Debug: 7.5s → 26s. The
unit pools are computed for 5150 files that previously had none.

**v1 is the quarry here, and the rung does not line up.** v1 reports 163
`internal-only` on Exposed. Only 43 of them are the rung v2 judges: 84 are
`possible`, whose own message reads "weaker matches point outside it — private
would suffice … only if those are not real uses" (name-fuzzy resolution advising
a code change on a maybe), 46 are `protected`, which v2 judges on its own
`Heirs` rung over the owner and its subtypes, and 28 are `public`. Of those 43,
v2 shares 16 and adds 12 — all but one in `src/test` trees, which now have test
units to be judged in. The 27 v1 reports and v2 does not decompose into: 20 in
packages some other file wildcard-imports (`import …core.vendors.*` in
`Column.kt` and `Table.kt`, 28 such importers; `…v1.core` has 139), where the
engine holds that a glob importer may name anything in the target and so
declines to advise; 5 that are v1 false positives — `TestDbDsl.kt`'s four and
`ExposedExtension.kt`'s one are used from other files, which `grep` confirms and
v1's own resolution missed; and 2 members of `R2dbcDatabaseMetadataImpl.kt`
named `getBoolean`/`getString`, names many other files spell for unrelated JDBC
`ResultSet` calls.

### `package.json` and `tsconfig.json` through the one door (2026-09-08)

| repo | before | after | what moved |
|---|---|---|---|
| vite | 691 | 687 | −6 `unused`, +2 `test-only`; import edges 2342 → **2474** |
| every other repository | — | — | byte-identical |

js-ts was the last adapter reading manifests through the four old hooks. It now
writes to `ManifestSink` like every other: a `package.json` states the UNIT npm
compiles — entered through its entry fields, `Library` where it has one and
`Executable` where it does not, `Unpublished` where it says `"private": true`,
compiled against the dependencies it declares — and the files its `scripts` run
stay the manifest's own tooling roots, which is what they are. The entries move
from a hook's `Production, Certain` root to the unit's own, where the colour
comes from its kind; no fixture moved, which is the equivalence.

`tsconfig.json` is new: js-ts never read it. What it states that the engine can
use is `compilerOptions.paths`, and the reading is a type rather than a
mechanism — an alias is a NAME that resolves to a FILE, which is exactly what a
package entry is, so an alias travels as one. An exact alias (`"~utils":
["./src/util.ts"]`) is a package with an entry; a wildcard (`"@/*": ["./src/*"]`)
is a package whose directory the subpath resolves against, which the bare-
specifier path already did for workspace siblings. Two spellings needed sharpening
next door: the WHOLE specifier is tried against the package map first (an npm name
is a scope and a name and stops there, so `"vite/module-runner"` can only be an
alias), and `@/` is not a scope, because a scope needs a name after it.

**What the 132 new edges buy.** Six files vite could not see a use of:
`playground/test-utils.ts` (127 files spell `~utils`), `playground/vitestSetup.ts`
behind it, and four under `playground/resolve-tsconfig-paths/src/` reached by
`@/*` and `#/*` — two of them a `.css` and a `.scss`, since resolution is the
IMPORTER's adapter and the alias serves whoever imports through it. Two of the six
come back as `test-only`, which is the true verdict: only tests reach them.

**What a tsconfig states and this does not read, each with its number on this
corpus.** `references`: 14, all between configs that declare no unit, so they
resolve no ambiguity — nothing to gain. `include`/`exclude`: kndo claims by
suffix, and no finding in the corpus turns on them. `extends`: 55 tsconfigs, and
no alias here is inherited rather than declared — a child without its own `paths`
contributes none, which is what TypeScript does anyway when the child overrides.
A wildcard alias with several targets takes the first that lands; the one such
mapping on the corpus (`"@fallback/*"`) has no import site at all.

v1 read `package.json` for entry roots and dependency names and never read
`tsconfig.json`, so all six files are questions v1 never asked rather than ones
it answered differently.

### Rule packs, and the evidence a pack needs (2026-09-08)

| repo | before | after | what moved |
|---|---|---|---|
| Exposed | 972 | 966 | −6 `unused`, −4 `internal-only`, +3 `untested`, +2 `internal-only` (a defect, named below) |
| every other repository | — | — | byte-identical |

A RULE PACK is an extension that claims no files and declares nothing but
dispatch rules: what a FRAMEWORK means, which is no language's to own. Its
rules ride beside the claiming adapter's, and its TRIGGER is its gate — a
marker no file carries fires nowhere, so a project without the framework is
untouched without anything having to decide that.

**The first measurement killed the plan's own ordering.** Ten packs were
scoped; the corpus was instrumented for all ten before any was written, and the
whole addressable population is ~16 findings: 13 in one Spring Boot sample
module in Exposed, 2 SwiftUI previews in Alamofire, 1 JUnit method. `testng`,
`lombok`, `rstest`, `pytest`, `storybook` and `vitest` have **zero** — the
corpus is nine repositories chosen for language coverage, not framework
coverage, and a pack shipped against zero is a promise, not a capability.

**And the packs fired on nothing, because the evidence was not there.** Of nine
adapters, only java emits both markers and relations; rust and go emit markers
alone; kotlin, swift, python and js-ts emit NEITHER. The rule-pack machinery
landed in M8.a and has been unreachable for six languages since. So this slice
is the seam plus the evidence for one of them: `kndo:kotlin` now reports the
annotations a declaration carries, the supertypes it promises (a constructor
call is the superclass, a bare name an interface — Kotlin's own rule), and what
a member access was read from.

**−6 `unused`, the spring pack.** Three `@Configuration`/`@RestController`
files and three `@Service`/`@Repository`/`@Component` classes in
`samples/springboot3-exposed-r2dbc`, every one of them constructed by a
component scan and named by nothing in the project. Three come back as
`untested`, which is the true verdict now that they are production-reachable.

**−4 `internal-only`, the relations.** `MergeBaseTest.withMergeTestTables` and
its neighbours are `protected` members of abstract test bases whose subtypes
live in other files. Without relation evidence the heirs pool was empty and the
advice said `private`; with it the pool is real and the analysis correctly says
nothing.

**+2 `internal-only`, a defect this slice introduces and does not fix.**
`SqlTypeProvider.appendDataPrecisions` and `appendDataTypes` are `internal`
members of an `internal abstract class`, called as
`typeProvider.appendDataTypes(…)` from five other files of the same unit, and
`used-by` lists all five — yet the advisory fires. It appears only once kotlin
declares the streams and survives declaring `Qualifiers` and emitting the
receiver, so the qualifier gate is not what is missing. A minimal reproduction
of the shape — an abstract class, an internal member, a subclass in a second
file, a qualified call from a third — does NOT reproduce it, which is the
useful clue: something in the real hierarchy (`MetadataProvider` HOLDS a
`SqlTypeProvider` rather than extending it) is the difference, and the fix
belongs to whoever narrows `internal_only`'s member branch with that case in
hand.

v1 read Spring through a reflection-dispatch plugin of its own and reported
these files alive; the difference is not the verdict but where the knowledge
lives — a rule pack states what a marker MEANS and parses nothing.

### A pack is gated twice, and one of the gates was never the trigger (2026-09-08)

| repo | before | after | what moved |
|---|---|---|---|
| Exposed | 966 | 966 | same findings; `kndo:spring` now reports 77 roots as a contribution |
| every other repository | — | — | byte-identical |

The findings do not move; the mechanism does, and the measurement is about
which mechanism was load-bearing.

`kndo:spring` had been composed for every adapter with its rules written as
bare names (`Controller`, `GetMapping`), so it rooted Vapor's `@Controller
struct` and its `@GET`/`@HTTP` routes: −20 findings on vapor, Vapor's macro
under Spring's rule. Rewritten as the plan writes them — the full path
(`org.springframework.stereotype.Controller`), which the engine qualifies
against the file's own import bindings before comparing — **the bleed is gone
with the pack forced ON for every project**: vapor 176 with
`Activation::Always`, 176 with the pack absent, byte-identical. Qualification
alone closes it, measured. `ExtensionSpec::rules_for` — a pack naming the
language coordinates it speaks for — was solving a problem the contract's own
`Pattern` semantics already solved, and it is reverted.

Activation is the SECOND gate and earns its place elsewhere: it is what makes
the pack a conduct extension, which is what puts it in `builtin_conduct_proofs`'
baseline-then-pack toggle and in the run's contributions. Without it the pack
was invisible to its own proof — on and unaccounted for in both runs.

The 77 roots are the plan's shape for the rules, not the previous one: every
stereotype AND every handler is a `Production` root at `Probable`, where this
repository had shipped `Certain` stereotypes and `Witness` handlers. The
silences are identical either way on this corpus (the handlers' owners are
rooted, so a witness and a root keep the same set alive); the difference is what
the pack CLAIMS, and `Probable` is the honest claim — an annotation says a
container may construct this, and whether the container is ever started is
outside anything kndo reads.

### Swift states what a type promises (2026-09-08)

| repo | before | after | what moved |
|---|---|---|---|
| Alamofire | 426 | 419 | −7 `internal-only` |
| vapor | 176 | 175 | −1 `internal-only` |
| every other repository | — | — | byte-identical |

`kndo:swift` reports two streams it never reported: the attributes a
declaration carries (plus `override`, the one modifier a rule reads) and the
types it promises the surface of. Purely additive evidence — no rule of the
language reads a Swift marker yet, and the ablation says so: with `Relations`
withdrawn from the spec and markers alone flowing, Alamofire measures 426 and
vapor 176, byte-identical to before. Every one of the eight is the relation
stream.

All eight are `internal-only` advisories correctly WITHDRAWN. `internal_only`
asks two questions of the relation graph — does a subtype override this member,
and does a supertype declare it — and a yes to either means narrowing the member
is a compile error rather than advice. With no relations both answers were no,
so Alamofire's protocol witnesses (`AuthenticationInterceptor.refresh`,
`ResponseSerialization.emptyValue`, `SessionDelegate.didGatherMetricsForTask`
and four more) were advised down to a reach the compiler would reject.

Swift writes its superclass and its protocols in ONE list its grammar does not
separate, so one relation kind is emitted for all of them: `Implements`, the
contract's word for "promises another type's surface", true of a subclass as
much as a conformer. Nothing in the engine reads the kind — it reaches a
`Trigger::Relation` and the supertype edges, and both compare names.

**Measured and NOT taken here.** The `Possible` root swift puts on every
non-private method of a type that declares any conformance — the
conformer-methods heuristic the design replaces with witnesses — is worth
Alamofire +63 and vapor +15 if simply deleted. It stays until the witnesses
that replace it exist: relations resolved inside the project (an override, a
protocol declared here) plus the packs that state the requirements of bases
outside it (`XCTestCase`, `View`, `Codable`). Deleting it first would ship 78
accusations the design already knows how to answer. The
`protocol-requirement-reach` fixture carries that as a named `known_gap`.

### A Swift value is a use, a backtick is spelling, and a member is read FROM something (2026-09-08)

| repo | before | after | what moved |
|---|---|---|---|
| Alamofire | 419 | 468 | −4 `unused`, +53 `internal-only` |
| vapor | 175 | 180 | +5 `internal-only` |
| every other repository | — | — | byte-identical |

Three changes, priced one at a time by ablation from the 419/175 baseline.

**Backticks: −2 `unused` on Alamofire.** `` `default` `` and `default` are one
identifier; Swift demands the quotes only where the word is a keyword and
permits them anywhere. `Endpoint.default` and `TestParameters.default` were
declared with them and reached without, so both sides read as dead. The quotes
now come off on both.

**The value of a binding: −2 more `unused`.** `property_declaration` was named a
binder seat wholesale, and its `name` field IS a `pattern` — so every bound
name was already covered and the only thing the rule actually threw away was
the VALUE. `let alpha = beta` reads `beta`. This is audit finding S5, and the
fix is the deletion of one word rather than a new mechanism.

**The receiver: +58 `internal-only`, all on members.** `internal_only` abstains
on a member wherever its adapter does not declare `Qualifiers` — without a
receiver it cannot tell `holder.name` from a bare `name`, and guessing is worse
than silence. Swift now reports what `expr.member` was read from, so the whole
member branch opens for it at once. Sampled: 50 of Alamofire's 53 are ordinary
`internal` members of internal helper types named in one file
(`RequestConvertible.parameters`, `Request.MutableState.downloadProgressHandler`,
`StreamMutableState.outputStream`) — the advice this category exists for.

**Named and not taken: 7 of the 58 sit on a `Codable`/`Content` conformer**
(3 Alamofire, 4 vapor), where the stored properties are also a serialization
surface the compiler reads. Narrowing them still compiles, so the advice is not
wrong — but the design's `codable-synthesis` rule would make them witnesses,
and it needs a kind filter on `Trigger::MemberOf` (the synthesis reads stored
properties, not methods) that the contract does not have. Seven findings is not
a contract change; it is a measured entry, and the rule lands if a bigger
population appears.

### An operator is a name, and a requirement is as visible as its protocol (2026-09-08)

| repo | before | after | what moved |
|---|---|---|---|
| Alamofire | 468 | 467 | −1 `internal-only` |
| vapor | 180 | 180 | −1 `internal-only`, +1 `unused` |
| every other repository | — | — | byte-identical |

Four gaps closed, and the corpus barely moves — which is the point of measuring
them rather than assuming.

**Operators: 0 findings, 10 declarations.** `static func == (l:r:)` has no
`name` field at all — the operator is an anonymous token — so every such
declaration was dropped whole (4 in Alamofire, 6 in vapor), and `a == b` spends
no named node either, so the uses were invisible too. Silence on both sides
nets to zero findings, which is exactly why this was never visible as a bug.
Both halves land together: the declarations enter the graph, `describe`,
`used-by`, the metrics and `duplicate`, and none is accused because their uses
are now reported. The `operators` fixture pins both directions.

**A protocol's PROPERTY requirement was never declared at all.**
`protocol_property_declaration` is its own node kind and the adapter walked
past it.

**`Reach::Inherited` for what a member does not spell.** A protocol requirement
is exactly as visible as its protocol — there is nothing narrower for it to be
— and a `public extension` hands its own modifier down. Worth −2
`internal-only`: `TestCredential.requiresRefresh` and
`CustomServer.listeningAddress` were being advised down to a reach the
compiler would reject.

**And the +1 `unused` is a true positive that file-scoping unmasked.** A
`private extension`'s members are the FILE's, not the module's. Vapor declares
`static var space` twice, in `DotEnv.swift` (used there) and in
`HTTPFields+Directive.swift` (used nowhere in the repository). Under a
module-wide reach the first one's use kept the second alive; under the reach
the source actually writes, the dead one is reported.

Closing that reach also exposed a pre-existing hole and it is fixed here: `case
.space` in a switch is a PATTERN, and every identifier under a pattern was
treated as a binder — so enum-case dot-shorthand, the pervasive use form in
Swift, produced no reference. A pattern that starts with `.` binds nothing.

### Python states what a definition carries, what a class promises, and how far `_x` reaches (2026-09-08)

| repo | before | after | what moved |
|---|---|---|---|
| flask | 20 | 18 | −2 `unused` |
| every other repository | — | — | byte-identical |

Three changes; the two lost findings are both the price of ONE of them, and it
is the owner's recorded decision.

**Decorators are markers, bases are relations.** Purely additive: no rule of
the language reads a Python marker yet, and flask's numbers do not move for
them. What changes is that the blanket `Possible` root a decorated definition
carries — "whatever the decorator does with it" — now has an alternative: the
decorator's own path is evidence a rule can read, which is what
`kndo:pytest`/`kndo:django`/`kndo:flask` will do in M8.e. The root stays until
they exist.

**P3, the parameter default.** `guard=_clamp` and `typed_guard: object =
_clamp` each bind one name and READ another under one node, and the binder seat
was named by parent KIND — so every default was thrown away. This is the second
consumer of the design's `Seats` ((kind, field) pairs), so it lands in the
toolkit now rather than as a third copy. Zero corpus movement: flask defaults
to imported names, which other evidence already kept.

**`_x` reaches the distribution's root package**, the owner's decision of
2026-09-05, replacing the file. Both lost findings are here, and both are real
losses:

- `flask/app.py`'s `_make_timedelta` is dead — a leftover of the sansio split —
  and `flask/sansio/app.py` declares a function of the same name that IS used.
  A unit-scoped pool joins them by name. This is the same cost Java's
  package-private and Kotlin's `internal` already pay.
- `cli.py`'s `_path_is_ancestor` is now kept by a `surface-import`: an opaque
  namespace import keeps every name of `Unit` reach or wider, per the design's
  keeper list. Python's `Unit{0}` means "internal to the distribution", which
  is not the same thing as "part of the module's surface" — the two coincide
  for every other language on this rung and diverge here. Named rather than
  patched: one finding does not justify a keeper rule that reads a language.

The rung still accuses (`_has_encoding` stands), which is what the decision
turned on: the alternative, `Exported`, would have made no `_x` accusable
inside a published distribution at all.

### Python reads what is inside the quotes, behind the guard, and after the dot (2026-09-08)

| repo | before | after | what moved |
|---|---|---|---|
| flask | 18 | 18 | no finding moves; **+9 subjects judged** |

Four gaps, and the honest headline is that the corpus does not move — what
moves is how much of the tree is under judgment at all.

**Forward annotations.** `def resolve(target: "_Later")` names `_Later` exactly
as the unquoted form does; the quotes are there because the name is not bound
at runtime under `if TYPE_CHECKING`, which is the dominant idiom in modern
Python. An adapter reading only identifiers sees a string literal and calls the
class dead. Pinned by the `string-annotations` fixture, both directions.

**Defs behind a guard.** A `def` under `if TYPE_CHECKING:`, in a
`try/except ImportError` fallback, or behind `if sys.version_info >= …` is
module surface exactly like one at column zero — the guard decides WHICH
definition binds, never whether the name exists. Together with PEP 695's
`type X = …`, this is the +9: nine names that were invisible are now declared,
reachable, and judged.

**`Reference::on` for `obj.attr`,** from the attribute's `object` where the
source spells it as a name. Zero movement, and the reason is worth stating:
Python's ladder is deliberately EMPTY (there is no keyword between
module-private and public, so `internal-only` never speaks for it), so unlike
Swift the receiver opens no advisory branch. What it buys is attribution — a
qualified use lands on the member it names instead of on every member of that
name — and that is precision the next measurement will read, not this one.

### The `_x` rung, re-measured — and the keeper that makes the narrow one honest (2026-09-08)

| repo | before | after | what moved |
|---|---|---|---|
| flask | 18 | 20 | +2 `unused`, both true positives the wide rung was swallowing |
| every other repository | — | — | byte-identical |

The 2026-09-05 decision put Python's `_x` at `Unit{0}` — the whole distribution
— on the reasoning that `mod._x()` from a sibling module is legal and common.
That reasoning was sound in its own context and is not in this one, so it was
measured again with `Nesting::ByPath` in place.

**The candidates, on flask, counted by the underscore accusations each keeps:**

| rung | flask | `_x` accused |
|---|---|---|
| `Namespace{0}` — the module | 20 | `_make_timedelta`, `_path_is_ancestor`, `_has_encoding` — 3 |
| `Unit{0}` — the distribution (was) | 18 | `_has_encoding` — 1 |
| `Namespace{1}` — the package | 17 | 0 |

All three are true positives, hand-checked: `app.py`'s `_make_timedelta` is a
leftover of the sansio split, `cli.py`'s `_path_is_ancestor` is named nowhere in
flask, and `_has_encoding` was already reported. Wider rungs lose them to name
collision and to the surface-import keeper; none of the three is a false
accusation at the narrow rung.

**What changed the context is a keeper, and it was missing.** The design's list
carries a QUALIFIED reference — a file that imports this module under a local
name and then writes `local.name` — and the engine had no such rule: only
`internal_only` ever read `Reference::on`. Without it, `Namespace{0}` really
would accuse a `_x` that a sibling reaches as `mod._x()`, which is exactly
what the original decision feared. `Keeper::Qualified` now closes it: the
qualifier names the module out loud, so the use counts wherever it is written,
and a bounded reach bounds who may name a declaration WITHOUT a qualifier —
never who may import it and say which module they mean.

The `underscore-namespace-access` fixture is what found the gap and what pins
it: `inner._qualified()` from a sibling keeps it alive; `_alone`, which nobody
qualifies, stays accused.

### Two texts are not two requirements (2026-09-08)

`VersionReq { spelled, range }` lands, and with it the comparison a range is
for. Before, `version-skew` compared the requirement TEXTS: two manifests that
spelled the same resolvable requirement differently diverged. That was never a
statement about the build — cargo unifies compatible carets at resolve time, and
npm's ranges overlap or they do not — it was a statement about typography.

| repo | before | after | why |
|---|---|---|---|
| ripgrep | 151 | 141 | −10 `version-skew`, every one compatible under cargo's caret |
| vite | 687 | 685 | −2 `version-skew`, both overlapping npm ranges |
| every other repository | — | — | byte-identical |

Every removal is a false positive with a name. ripgrep's workspace pins
`serde_json = "1.0.23"` while `crates/globset` asks for `1.0.107`: one range,
`[1.0.23, 2.0.0)` ∩ `[1.0.107, 2.0.0)`, one build, nothing to reconcile. The
same shape ten times, across `bstr`, `termcolor`, `log`, `walkdir`, `serde`,
`anyhow`, `winapi-util`, `regex-syntax` and `regex-automata`. On vite, `vue`
at `^3.5.41` beside `^3.5.18` and `react` at `^19.2.8` beside the pin `19.2.8`
— overlapping, so silent.

**What survives is the finding the category exists for.** vite's `tailwindcss`
is declared `^4.3.3` in three playgrounds and `^3.4.19` in `playground/tailwind-v3`:
`[4.3.3, 5.0.0)` and `[3.4.19, 4.0.0)` cannot both hold, and it is still
reported. One real skew kept, twelve typographic ones dropped.

The range reader is `kndo_toolkit::semver_range`, one function for two
ecosystems: npm and cargo spell `^`, `~`, `=`, `>=` and the `x`/`*` wildcards
identically and differ on the BARE form alone (npm pins it, cargo widens it to
a caret), which is why `Bare` is its only parameter. What it cannot spell — a
comma-joined conjunction, a `||` union, a hyphen range — returns no range at
all, and a requirement with no range is never a conflict: `disjoint` answers
`None`, and the analysis falls back to the text comparison that is all the
evidence there is. Guessing a range wrong is worse than having none, because a
comparison silently made against the wrong bounds is a finding nobody can check.

### Three things a language says about itself (2026-09-08)

`ecosystem`, `hidden_opt_in` and the whole `Nesting` enum land as spec data,
and the wire carries the ENTIRE `ExtensionSpec` for the first time — a WASM
guest could not previously declare its nesting, its file roles, its namespace
span, or one word of the dependency vocabulary, and the host filled each with a
default it invented on the guest's behalf.

| repo | before | after | why |
|---|---|---|---|
| vite | 685 | 690 | +5 `unused`, all under `docs/.vitepress` — six files discovery had never entered |
| every other repository | — | — | byte-identical |

**The shapes were declared, and the numbers did not move.** go now says
`ByDirectory`, java and kotlin `Flat`, rust `Mounted`, python
`ByPath { roots }`, and every other adapter `PerFile` — where before the engine
inferred the shape from which evidence happened to be present (a mount chain
means Mounted, a `by_path` answer means ByPath, a clause means the fallback).
Byte-identical output across guava, Exposed, gin, ripgrep, Alamofire and vapor
is the result: the inference agreed with the declaration everywhere the corpus
reaches. What changed is who decides — `Scopes::build` reads a capability
instead of guessing, and `Nesting::Flat` and `ByDirectory` are now different
answers rather than one accidental one. The kmock proof is the pair: two
directories of one unit writing one clause, and a namespace-reaching
declaration in only one of them — accused under `ByDirectory`, alive under
`Flat`, same tree.

**`ecosystem` moved nothing, in the only direction it can move.** css and html
declare `kndo:js-ts`, so a bare `@import "tailwindcss"` in a stylesheet is now
a USE of the npm declaration rather than nothing. No corpus stylesheet spells a
bare specifier that matches a declared dependency, so no accusation was
withdrawn — and none could ever be added, because the rule only ever adds
users.

**`hidden_opt_in` is the one that added findings, and every one is owed to a
pack.** js-ts declares `.storybook` and `.vitepress`; vite has a VitePress
docs site, so six files entered the graph and five are accused:

- `docs/.vitepress/config.ts` and `theme/index.ts` are entered by VitePress
  itself, by exact path. Nothing in the tree imports them, and nothing should:
  the framework's convention is what roots them, which is `kndo:vitepress`'s
  rule to state (M8.e). Until it exists these are false accusations, counted
  here rather than hidden by leaving the directory undiscovered.
- `theme/styles.css` is imported by `theme/index.ts` and rides its root.
- `theme/composables/sponsor.ts` and `theme/live/useYoutubePlayer.ts` are
  imported only from `.vue` components, which no adapter claims — the same
  blind spot this file has recorded on vite since M2, now visible one
  directory deeper.

Not discovering a directory is not the same as judging it correctly. The
mechanism belongs to discovery, the roots belong to a pack, and the honest
place for the gap in between is a number with its owner named.

### Python's roots leave the extractor (2026-09-08)

Five branches in `extract.rs` concluded a root from evidence the same function
had just emitted — a decorator reported as a marker and then read again, two
lines apart, to decide what it meant. The design says `DispatchRule` replaces
the root code of every adapter; python is the first row to actually do it.

| repo | before | after |
|---|---|---|
| every repository | — | — | byte-identical |

**And every conformance fixture byte-identical too.** That is the result: the
rules reproduce the hand-written roots exactly, so the migration changed the
mechanism and not one verdict. Six rules now say what python's own runtime and
runners dispatch on — the `__main__` guard as a file marker, a decorator on a
class, a function or a method, `test*` by name in a test compilation (free
functions and members alike), and a member dunder as a WITNESS rather than a
root, which is the design's word for "alive while its owner is, and of no
colour".

The deletion the migration exposed is the part worth naming: `test_file`
threaded through five functions, and once nothing concluded from it, four of
those parameters had no reader. The extractor now states facts and stops.

Frameworks stayed out: pytest's collection of a `TestCase` subclass, Django's
URL conf, Flask's `@app.route` are their packs' rules, gated by the dependency
that proves the framework is installed. What is here is the LANGUAGE's, which
is the line the design draws.

### Python resolves against declared roots, not against a path's tail (2026-09-08)

Resolution asked the tree for a file whose path ENDS in the dotted name — the
last thing python's row still did by convention rather than by manifest. It now
asks `cx.project()`: the source roots the unit declared, its `namespace_root`
where the manifest mapped a package onto a directory that does not contain it,
and finally the project root, always, because `sys.path` holds the directory
the interpreter starts in.

| repo | before | after | delta |
|---|---|---|---|
| flask | 20 | 19 | −1 `cyclic` |
| every other repository | — | — | byte-identical |

**The withdrawn finding is a false positive v1 also shipped.** `src/flask/json/
provider.py` writes `import json`, meaning the standard library. Suffix
matching found `src/flask/json/__init__.py` — a path that genuinely ends in
`json/__init__.py` — and drew an edge from flask's JSON package to itself, then
reported the cycle it had just invented. Under declared roots the candidate is
`src/json.py`, which does not exist, so the import stays unresolved the way
every third-party import does. `stdlib-shadow` is that case as a fixture, and
restoring the suffix resolver makes it fail by name.

Import edges fell 226 → 197 on flask in the same run. Those 29 are the same
error in its harmless form: stdlib and third-party names that happened to have
a look-alike tail inside the distribution. An edge that should not exist is not
free even when nothing accuses on it — it feeds `cyclic`, `test-only` and every
reachability answer downstream.

**Nothing else moved, and that is the claim.** Eight repositories are
byte-identical, so the new resolver reproduces every correct answer the old one
gave and drops one class of wrong ones. The remaining python surface the
change touches — PEP 420 portions under two declared roots, `package-dir`
namespace roots, the `src` layout read off the tree — is pinned by
`namespace-package`, `src-layout-roots` and the manifest unit tests rather than
by a corpus number, because flask declares none of those shapes.

### `[project.entry-points]` is read, and the tables it joins (2026-09-08)

`[project.scripts]` was read and `[project.entry-points.<group>]` was not, so a
pytest plugin, a Flask command or a Django app registered through the installer
was invisible: the manifest is the only witness such a module ever has. Every
group is now read, because what registers a callable is what calls it — the
group decides who does the calling, never whether anyone does.

| repo | before | after |
|---|---|---|
| every repository | — | — | byte-identical |

No corpus distribution registers an entry-point group, so the number is zero
and the proof is `entry-points`: three modules in a distribution the classifier
marks `Private :: Do Not Upload`, two named by groups and one named by nothing.
Un-reading the table turns the first two into the third — the accusation the
fixture's control carries.

The private classifier is what makes that fixture answerable at all, and it is
worth stating plainly: an uploadable python distribution publishes its whole
export surface, so `unused` on public API is a question v2 can only ask of a
distribution that says it does not upload. `_x` remains the class of dead code
python reports everywhere else, which is what the `_x → Unit{0}` decision was
weighed against and why re-measuring it changed nothing.

### JVM resolution is the package clause, not the directory (2026-09-08)

Java and Kotlin resolved by PATH SUFFIX: `com.foo.Bar` found the file whose
path ends in `com/foo/Bar.java`, with a "nearest module" tie-break among equal
matches, plus a package-DIRECTORY listing for wildcards. Both are the mirror
javac uses to FIND sources on disk, not the rule for what a name MEANS — and
the plan's deletion column names all of it (`package_dir_files`, layout
mirrors, Kotlin's directory fallback). What a JVM import names is a package,
and a package is a clause its files declare. `cx.project().files_in_namespace`
is now the one reader of that fact; `nearest_suffix_match` is gone from the
toolkit with its last caller.

**A namespace is a name inside a COMPILATION, and guava is why.** Answering
with every file that wrote a clause merges packages that never share a
classpath: `guava-gwt/src-super/.../Platform.java` declares
`com.google.common.base` and REPLACES the real one under the GWT compiler.
Resolution now filters the namespace to the importer's unit and the units it
compiles against — the closure `Project::compiles_against` already held,
surfaced on `UnitView` rather than re-derived.

| repo | before | after | delta |
|---|---|---|---|
| guava | 8247 | 8271 | +24 (50 new, 26 withdrawn) |
| every other repository | — | — | byte-identical |

**26 withdrawn, 50 new, and the interesting half is neither.** Eight
`internal-only` findings MOVED: they sat on
`guava-gwt/src-super/.../LocalCache.java` and now sit on
`guava/src/.../LocalCache.java` — the same eight members, named on the file
that ships instead of on the GWT copy that shadows it. Nearest-suffix had
been attributing a use to whichever copy was closer to the importer, which is
a fact about directory depth and about nothing else.

**39 of the 50 new findings are `untested`, and they are the phantom edge
withdrawing.** `guava-gwt/src-super/` and `futures/failureaccess/` used to be
reached by tests whose imports suffix-matched into them. They are not:
guava's Java test suite compiles against `guava`, and the GWT super-source is
compiled by the GWT compiler instead of it. Nothing tests those files, which
is what the run now says. An invented edge does not only invent accusations —
it invents coverage, and that is the quieter half of the same defect.

Exposed is byte-identical: Kotlin's corpus repository is one module tree whose
clauses and directories agree everywhere, so the two mechanisms had nothing to
disagree about. That is the result, not the absence of one.

### The rest of the sweep: swift's copy, css's quotes, js-ts's subpaths (2026-09-08)

Three more entries from the plan's deletion columns, in one pass.

**swift's private `GENERATED_NEEDLES`** was already byte-identical to the
toolkit's — python's commit promoted `"Generated by"` into the shared list and
left swift's copy sitting beside it. Deleting it moves nothing, which is the
point: the second copy promotes, and this one had already been promoted out
from under itself.

**css's `unquote`** trimmed `"` and `'` off both ends of a value. The plan says
to read what the grammar gives instead — and the grammar gives the delimiters
as the node's own first and last children, so the content is the span between
them. `tree-sitter-scss` 1.0 has no `string_content` node, which the plan's
wording assumed; reading the quote tokens' byte offsets is the same idea
against the grammar that exists, and it is exact where character-trimming was
lucky (`url('a"')` was always going to be wrong).

**js-ts's `split_bare`** is the one that cost something.

| repo | before | after | delta |
|---|---|---|---|
| vite | 690 | 701 | +11 (14 new, 3 withdrawn) |
| every other repository | — | — | byte-identical |

`split_bare` cut a bare specifier into package name and subpath, then mirrored
the subpath onto the package's directory. That agreed with `package.json`'s
`exports` map only when the package had no map — and where a map exists it is
the whole truth: `./sub/*` may point anywhere, may be absent, and may differ by
`import` versus `require` condition. Until the manifest half reads it (M8.d), a
subpath is an external specifier: unresolved, keep-alive, never an accusation.

**All 14 new findings are in `playground/`** — vite's own resolver test tree, a
collection of deliberately-odd little packages exercising the `browser` field,
`exports` and tsconfig paths. They are false positives, and they are the
precise shape of the debt: every one is a subpath into a local package whose
map decides where it lands. The `deep-import` fixture is the other half of the
same sentence and comes out the other way — `@org/ui` publishes
`{ ".": "./index.ts" }` and nothing else, so `@org/ui/secret` is a path the
package refuses, and the file behind it is now correctly accused where the
directory mirror had been quietly resolving past the boundary the sibling drew.

One mechanism, two directions of error, and the fixture that proves the
deletion sits next to the corpus number that prices it.

### java and kotlin: what the language dispatches on, as data (2026-09-08)

Both rows leave `ROOTS_STILL_IN_THE_EXTRACTOR`. Four hand-written roots become
rules, and one of them changes an answer.

| repo | before | after | delta |
|---|---|---|---|
| Exposed | 966 | 965 | −1 (2 withdrawn, 1 new) |
| guava | 8271 | 8271 | byte-identical |
| every other repository | — | — | byte-identical |

**Kotlin's `override` and `operator` stop being roots and become WITNESSES,
which is what the plan says and what they are.** A root is an entry: something
outside enters here. An override is not that — it says "this member is alive
while its type is", which is exactly `Effect::Witness`. Rooting it made every
overriding member an entry point, and an entry point keeps its whole file
reachable.

The one file where that mattered is
`samples/springboot3-exposed-r2dbc/.../EmailEnvironmentPostProcessor.kt`. Its
class overrides Spring's `EnvironmentPostProcessor`, and that override was the
only thing anchoring the file. Before: two findings (the class `unused`, the
file `untested`). After: one, on the file — nothing in the project reaches this
class at all, which is the truer sentence. It stays a false positive until
`kndo:spring` reads `META-INF/spring.factories` (M8.e), and it is now one false
positive instead of two, filed against the file rather than the member.

**guava byte-identical is the java half's result.** `public static void
main(String[])` moved from a hand-written root to a marker plus a rule, and the
JLS shape is matched exactly as before — the adapter still recognizes
`static`, `public`, `void` and `(String[])`, because a signature is grammar
knowledge and belongs in the adapter. What moved is the VERDICT: the extractor
reports the launcher shape, the rule says it is a production root.

**One entry deleted from java's shipped rule table: `com.vendor.Closer`.** It
was a fabricated vendor type carried in the language's own witness list so one
fixture could prove that qualification distinguishes two same-simple-name
types. The contract already proves that where the trigger lives
(`cx.spells("com.vendor.Closer", "Closer")` beside
`!cx.spells("com.other.Closer", "Closer")`), so the entry was a second proof
paid for with invented data in production. The entry, its two fixture classes
and the two interfaces they implemented are gone.

**Naming a fixture the previous commit moved and did not name.**
`kndo-adapter-ts/tsconfig-path-aliases` changed in the `split_bare` commit
alongside `deep-import`, and only `deep-import` was written down. Same cause:
an alias whose target is a bare specifier's subpath no longer resolves through
the directory mirror. The gate caught the omission, which is what it is for.

### swift: the framework's dispatch leaves the language's adapter (2026-09-08)

The largest delta this milestone, and it is a debt made visible rather than a
regression discovered.

| repo | before | after | delta |
|---|---|---|---|
| Alamofire | 446 | 1448 | +1002 (0 withdrawn) |
| vapor | 178 | 729 | +551 (0 withdrawn) |
| every other repository | — | — | byte-identical |

**965 of Alamofire's 1002 and 540 of vapor's 551 are inside `Tests/` trees.**
They are XCTest methods, and the adapter used to root them by hand: `test*` in
a `Tests/` target, `Certain`. XCTest is a LIBRARY — you import it, and a
package that does not import it is not collected by it — so under the design's
own sentence ("the stdlib packs of each language are not packs: they are the
adapter's `dispatch_rules`, because they are facts about the language") its
collection rule belongs to `kndo:xctest`, and `@Test`'s to
`kndo:swift-testing`. Both are M8.e.

The remaining ~37 and ~11 are the other two deletions:

- **The conformer-methods keep.** Every non-private method of a type that
  declared ANY conformance carried a `Possible` root. It named no protocol and
  no requirement — a silence with a confidence attached rather than a rule. The
  `protocol-requirement-reach` fixture had predicted this exactly: its
  `[[known_gap]]` on `Cube.volume` closed the moment the keep went, and is
  promoted to `[[dead]]` in this commit.
- **`@main` and `override` became rules instead of roots**, changing nothing on
  their own — `@main` is still a production root (Certain), and `override` is
  now a witness, which is what an override is.

**The apple-bundles proof changed shape, and the new shape is the more honest
one.** Without the plugins, the watchOS files are now accused WHOLE rather than
by their types: nothing reaches them at all once the blanket conformer keep is
gone. With the plugins, the storyboard and plist roots land and the files
become reachable, which surfaces the members that were dead all along behind
the file-level finding. The proof's invariant is restated where it holds:
nothing appears in the `after` set whose own file was not already accused
whole.

`xctest-discovery` is the debt as a fixture — two `[[known_gap]]` entries owed
to `kndo:xctest` and one control that must stay dead once the pack lands. It
fails the day the pack closes it.

### rust and ts: the list empties (2026-09-08)

The last two rows of `ROOTS_STILL_IN_THE_EXTRACTOR`. What remains on it is go's
`func main`, which is a UNIT fact go.mod cannot state — a Go module is one
Library unit, so no `in_unit` reaches it — and stays named rather than pretended
away.

| repo | before | after | delta |
|---|---|---|---|
| ripgrep | 141 | 143 | +2 `internal-only` |
| every other repository | — | — | byte-identical |

**ts's `#!` line becomes a marker and a rule.** The file says the loader runs
it, which is content and not a path, so it stays the adapter's — reported as
the marker `#!`, rooted production Certain by one rule. Nothing moved.

**rust's macro-template root becomes references, and the +2 is the honest
price.** A name a `macro_rules!` body mentions was rooted `Possible`. The plan's
shape for names inside macro token trees is a REFERENCE, and a reference is
what the file can honestly say: the name appears here. References rose 61733 →
61751 on ripgrep — the mentions, now recorded as the uses they are — and the
declarations stay alive.

What the root was also doing, by accident, is suppress narrowing advice. The
use happens at every EXPANSION site; the reference is recorded where the
template is written; so the ladder now sees a use pooled in one file and
advises a rung the macro cannot live at. `ignore_messages` and `set_errored`
are that, and both fixtures carry it as a `[[known_gap]]` naming the missing
coordinate: **a reference that travels**. The design has none — a use recorded
in one file and performed in another is not something the vocabulary can say,
and `macro_rules!` is the case that needs it.

A root that suppresses advice by being a root is not a rule about macros; it is
a rule about entry points, borrowed. Naming the gap is worth two findings.

## The transcript tranche moves nothing, and that is the measurement (2026-09-09)

Five build tools now grade the manifest readers (`captured_transcripts_hold`),
and the first run found two real defects in `kndo:python`'s scoping: the runtime
table declared no scope at all, and an extra declared `Dev` where it is a gate
the consumer opens. Both are silent on this corpus, and the reason is stated in
the reports themselves — every python run abstains from the dependency family
with `specifier-identity-underivable`, so nothing reads a python declaration's
scope yet. All nine repositories are byte-identical against a baseline taken
before this tranche began: Alamofire 1484, Exposed 965, flask 19, gin 109,
guava 8271, lodash 20, ripgrep 143, vapor 732, vite 712.

The committed reports move anyway, for two reasons that are not this tranche's
findings. `corpus-findings/` was one commit stale: the alias tranche's vite
number (701 → 712, import edges 2447 → 2590, the eleven decomposed in its own
DECISIONS entry) was measured and recorded in prose but never written out, and
this run writes it. And every report's `extensions` block drops
`published_surface`, a field the previous commit deleted from the contract while
the reports still carried it. Findings themselves move on no repository.

Two candidate changes were measured and held back rather than shipped for a
number; both are decomposed in `DECISIONS.md` and carried in `EXPERIMENTS.md`.
The one worth naming here is guava's, because it is the largest single block of
findings this corpus still holds that a manifest could retire: reading the
`<sourceDirectory>src</sourceDirectory>` guava's own root pom declares retires
60 findings across `guava-gwt/src-super`, `guava-gwt/test-super` and
`futures/failureaccess` — trees javac does not compile — and adds 24 in the same
trees, because a file no unit compiles is read as private rather than as
unstated. The 60 are the reason to do it; the 24 are the question to answer
first.

## guava 8271 → 8235: two variants of one library stop keeping each other alive (2026-09-09)

The `<sourceDirectory>` guava's own root pom declares is now the main unit's
root, and the delta is one mechanism, not two. `guava-gwt/src-super` holds a
second copy of the classes it replaces — `ForwardingImmutableList`,
`ExtraObjectsMethodsForWeb`, `LongAddables`, `Platform`, `TestPlatform` and the
rest each exist three to five times across `guava/src`, `android/guava/src`,
`guava-gwt/src-super` and `guava-gwt/test-super`, all under the same package
clause. GWT compiles super-source INSTEAD of the file it shadows, never beside
it; while the whole module directory was one unit, both copies shared one
namespace pool and resolved into each other.

**60 retire**: 47 `untested`, 8 `unused`, 5 `internal-only`, every one under
`guava-gwt/src-super`, `guava-gwt/test-super` or `futures/failureaccess` —
trees javac does not compile from this pom.

**24 appear**, and they are the false keeps ending rather than new noise:

- **3 in `guava/src`** — `ForwardingImmutable{List,Map,Set}`, named by nothing
  in `guava/src` and previously kept by a super-source class that extends its
  own same-named copy. In the Maven build of the `guava` module they are dead.
- **21 in `guava-gwt/{src,test}-super`** — 12 `unused` and 9 `internal-only` on
  files nothing in this project reaches. True of the javac build; unknown of the
  GWT build, which nothing here reads. A `.gwt.xml` reader is what would answer
  them and is on the books in DECISIONS.

`kndo used-by` is what settled it: `kept_by` for `ForwardingImmutableList` was
one super-source reference before and is empty after. The first reading of this
delta, recorded earlier the same day, called the 24 false positives without
asking that question; it was wrong.
## A crate root's `#![allow(warnings)]` is the crate's, and ripgrep says so (2026-09-09)

| repo | before | after | what moved |
|---|---|---|---|
| ripgrep | 143 | 141 | −2 `unused` under one `#![allow(warnings)]` |

`crates/index/src/lib.rs` opens with `#![allow(warnings)]` and then
`mod index; pub mod literal;`. rustc scopes a crate root's inner attribute to
the crate, so the author silenced the lint for all three files; v2 silenced it
for the one that carried the words. `Handle::read_write` and
`Handle::read_write_mut`, both in `crates/index/src/index.rs`, were the two
accusations that survived a blanket written to cover them. Two diagnostics
appear in their place — `index.rs` (23 declarations) and `literal.rs` (71) each
report the blanket that stands their declarations down, which is the whole point
of reporting a blanket at all: the exemption is now visible where it applies
rather than only where it was typed.

The eight other repositories are byte-identical: Alamofire 1484, Exposed 965,
flask 19, gin 109, guava 8271, lodash 20, vapor 732, vite 712. Nothing but rust
emits a unit-scoped marker, and cargo's `path =` — the other half of this
tranche — disambiguates nothing here, because no repository in the corpus
declares two crates of one name.

The reach is bounded in the direction that matters, and the corpus is why it
had to be. An `#![allow(dead_code)]` on a MODULE file is that module's, not the
crate's; ripgrep carries none, but this tree does, and its
`attribute-dispatch` fixture pins the case: `src/scratch.rs`'s blanket must not
reach `src/ffi.rs#truly_dead`. It does not, and that fixture's report is
byte-identical.

`corpus-findings/flask.report.json` also moves, and it is NOT this tranche's:
measured on the pristine tree before any edit here, flask already differed from
its committed report. `docs/conf.py` reports as an unused FILE rather than as
an untested file plus an unused `setup`, and `examples/celery/make_celery.py`
joins it. The total is 19 either way, which is why the stale pin went unnoticed.
No python file emits a unit marker and no python manifest emits a `path =`, so
nothing in this tranche can reach flask.

## ripgrep 141 → 139: a template's mention stops advising (2026-09-09)

`crates/core/messages.rs` declares `pub(crate) fn set_errored` and
`pub(crate) fn ignore_messages`, and the only mentions of either are inside
`macro_rules! err_message` and `macro_rules! ignore_message` in that same file.
The macros expand from `main.rs` and `haystack.rs`, so the uses are performed
there and recorded here — and the ladder, seeing every use pooled in one file,
advised a rung the macro cannot live at. A reference performed `Elsewhere`
states the use and withholds the site, so the advice stops. Both findings
retire, nothing is added, and the eight other repositories are byte-identical.

`crates/matcher/tests/util.rs#RegexCaptures` — the repository's third
`internal-only` — is ordinary code and stays accused, which is the precision
half of the same measurement.
