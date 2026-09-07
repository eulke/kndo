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
