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
no imports between siblings, so `AdapterSpec` grew `ReferenceScope::Directory`
(default `File` — the default-compatibility rule; `unused` is the named consumer;
`multi-file-package` the conformance case): analyses pool references per
directory. Every file also carries a synthetic `"."` edge to its non-test
siblings — reachability travels, `_test.go` stays out of the production color —
and imports resolve through the longest go.mod module-path prefix to
`Resolution::Files`, M4.a's directory-unit variant doing exactly what it was
grown for.

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
v2 reports 5,621 findings; the oracle (minus its 7,014 `internal-only`, which
v2 does not build until M6.c) reports 13,230. Category by category:

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

**duplicate 4,708 vs 3,564 — the surplus is real and v1 could not see it.**
1,326 of v2's are FILE-level: guava vendors byte-identical parallel trees
(`android/guava/**` mirrors, the `futures/listenablefuture1` copy), and v1 had
no file-granularity duplicate at all. The symbol-level remainder (3,382) sits
under v1's 3,564 with the same 60-token floor.

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
harvested fixture exactly as v1 did.

## M6.b.2 — Exposed speaks: the first Kotlin measurement

`kndo:kotlin` lands and Exposed claims 809 files, measuring 774 findings
against the oracle's 1,037 (of which 163 are `internal-only`, unbuilt until
M6.c, and 22 more sit in unbuilt categories — test-only 14, cyclic 4,
version-skew 4).

**duplicate 646 vs 91 — the surplus is real, maintained-in-parallel code.**
Exposed keeps TWO test suites in lockstep: `exposed-tests` (JDBC) and
`exposed-r2dbc-tests` (R2DBC) hold structurally identical test methods
duplicated pair-by-pair (`SelectTests.testCompoundOp`,
`UpdateTests`, `UnsignedColumnTypeTests`… — 641 symbol clones at the same
60-token floor every language uses). guava's android/ mirror at method
granularity; the recorded presentation question (grouping clone families)
applies, and v1's far smaller count is v1's weaker Kotlin metrics, not a
v2 false-positive flood.

**unused 19 vs 277 — Kotlin's public-by-default meets library-mode roots.**
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
