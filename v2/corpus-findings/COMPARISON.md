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
