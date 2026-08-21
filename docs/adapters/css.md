# Adapter Spec — CSS

**Status:** Draft, pre-implementation · **Depends on:** RFC 0002 §3, §7 · docs/rfcs/0012-reference-semantics-and-visibility.md (member_of §3, within §4, RefKind §5, visibility ladder §6, generated-origin §7, unit-key §8) · ADR 0002 (tree-sitter)

## 0. What's structurally different from every prior adapter, and why it matters here

RFC 0002 §3 groups CSS with JSON as a "non-source language," but CSS is a **much narrower**
kind of non-source than JSON turned out to be. JSON has *zero* internal structure worth
extracting (RFC 0002 §3: "symbols are not extracted"). CSS's own line reads differently:
"symbols are selectors/mixins/variables; references are `@import`/`@use`, `composes`, and —
via the cross-language edge mechanism (§4) — class-name usage from JS/TS/HTML. This enables
'unused CSS rule' as a normal unused-symbol finding." That's a real declaration/reference
model, not a bare file-claim. The honest v1 scope turned out to be narrower than that sentence
promises, for a reason worth stating precisely rather than discovering by trial and error in a
fixture.

**Why per-selector "unused CSS rule" isn't safe to ship in v1 — traced through the actual
reachability code, not assumed.** `kndo-core/src/analysis/reachability.rs::compute` builds one
CSR graph over files *and* symbols, and its own comment states the rule plainly: "The
module-load rule's implicit symbol → owner edge, one per symbol" — every `Symbol` node has an
unconditional edge to its *owning file*, seeded from the symbol regardless of whether the
symbol is reached by a reference or is itself a root (`RawRoot`). Two consequences follow,
neither optional:

1. **A class/id selector's real consumers are invisible to kndo today.** RFC 0002 §3's own
   phrase — "class-name usage from JS/TS/HTML, via the cross-language edge mechanism" —
   describes a mechanism that does not exist yet. Checked directly: `kndo-adapter-js` extracts
   no `className="..."`/`styles.foo` references at all (confirmed against
   `kndo-adapter-js/src/extraction.rs`); the only CSS-touching thing JS-TS does today is
   resolve a bare `import './styles.css'` as a *file*-level edge (js-ts.md §3, "Asset
   specifiers... resolve as cross-language file edges when the file exists — the CSS/JSON
   adapters claim the targets"). And RFC 0002 §2 draws the line explicitly: framework/DOM
   conventions (JSX `className`, CSS Modules' `styles.x` indirection) are plugin territory
   (RFC 0003), not adapter territory — HTML itself isn't even in the eight-language launch set.
   So "who uses this class" is real, unmodeled *design* work belonging elsewhere, not a gap
   this adapter can extraction its way out of.
2. **Marking every class/id selector a root to route around gap 1 breaks something worse.**
   The tempting fix — "we can't see real usage, so treat every selector as reachable by
   definition, same as library-mode public API" — was checked directly against the CSR
   construction above and rejected: a rooted symbol's implicit symbol→file edge would make its
   *owning file* reachable too, unconditionally. That silently disables `unused`'s file-level
   check for every `.css` file that declares so much as one class — the one piece of "unused
   CSS" this adapter *can* deliver honestly (§0 point 3 below) — for the sake of a per-rule
   check this adapter cannot deliver honestly yet. Not a trade worth making.

**v1's actual, narrower, fully-honest scope** — three axes that don't have either problem:

1. **File claiming.** A `.css` file gets a `FileClass` the same way `.json` does (RFC 0002 §3,
   docs/adapters/json.md §0's identical argument): `ResolveCtx`'s known-files index already
   lets JS-TS's own resolver find `.css` targets with zero CSS adapter in existence, but every
   analysis in `kndo-core/src/analysis/` still needs `file.class` to be `Some` to have an
   opinion at all. This alone makes "this `.css` file is never imported by anything" a real,
   safe `unused`/`file` finding — CSS's whole value proposition on the JSON model, before any
   CSS-specific parsing is even involved.
2. **The `@import` graph among CSS files themselves.** Unlike class-name consumers, one CSS
   file `@import`-ing another is *entirely visible* within CSS — no cross-language blindness,
   no plugin dependency. Real declaration/reference machinery, real value, zero honesty gap.
3. **Custom properties (`--foo`) and `var(--foo)`.** The one *symbol* kind kept in v1. Unlike a
   class selector, a custom property's consumers are — in the overwhelming common case — other
   CSS in the same project (`var(--brand-color)`), not JS/HTML. `SymbolKind::CssVariable`
   already exists in `kndo-core::vocab` for exactly this (alongside `SymbolKind::CssRule`,
   deliberately *not* used yet — reserved for whenever selector-level extraction becomes safe,
   §7). "Unused CSS custom property" is real, checkable, and honest today.

Selectors/mixins ("mixins" is SCSS-only syntax to begin with — plain CSS has none) and the
class-name cross-language linkage are **explicitly deferred**, not silently dropped: §7 records
exactly what needs to exist first (a plugin or a JS-TS extraction change) before `SymbolKind::
CssRule` extraction is safe to turn on.

**Grammar**: `tree-sitter-css` (real, available, unlike JSON's ADR 0002 escape hatch — there is
a genuine syntax tree worth walking here: nested at-rule blocks, selector combinators,
`declaration`/`import_statement` nodes). **Scope: plain `.css` only** — `.scss`/`.less` are not
claimed in v1 (a documented, bounded choice, not an oversight: RFC 0002 §7's own per-language
table names the launch-set row "CSS," singular, even though §3's prose bundles "CSS/SCSS/LESS"
together; SCSS/LESS bring real extra grammar surface — nesting, `$variables`, `@mixin`/
`@include`, the `&` parent selector — that deserves its own verified pass, not a rushed
addition here, mirroring the "one language lands, verified, at a time" discipline this session
has followed throughout M5).

## 1. Claiming & classification

**Glob:** `**/*.css` only (§0).

**No cross-adapter exclusions needed** (unlike JSON's `package.json`/`tsconfig.json`
carve-out): no other launch-set adapter declares a `.css`-shaped `manifest_glob`, so there is
no well-known manifest this glob could accidentally swallow.

**Role**: always `Production` — same stance, same reasoning, as docs/adapters/json.md §0/§1: no
`*.test.css` ecosystem convention exists to encode, and inventing one without a real signal
would be speculative. Revisit only with real dogfooding evidence.

**Origin**: real generated-origin detection, unlike JSON — CSS *has* comment syntax (`/* ... */`),
so the same `kndo_adapter_toolkit::classify::ContentMarkers` mechanism every other adapter uses
applies unmodified: `comment_openers: &["/*"]`, matching against the ecosystem's real banners
(Sass/Less compiler output, Tailwind's compiled CSS, PostCSS pipelines routinely emit `/*!
Generated by ... */`-shaped headers). No structural (AST-derived) signal exists the way Java's
`@Generated` annotation does — this is the line-scan mechanism, same tier as Go's/JS's/Rust's.

## 2. Extraction

**Declarations — custom properties only** (§0 point 3): a `declaration` node whose
`property_name` text starts with `--` (there is no distinct "custom property" grammar node —
`tree-sitter-css` reuses `property_name` for both `color: red` and `--brand: red`; the `--`
prefix is a plain text check, not a node-kind check) becomes one `Declaration { kind:
SymbolKind::CssVariable, member_of: None, ... }`.

**One Declaration per unique name per file — first occurrence wins, not one per occurrence.**
Verified, not assumed: `kndo-core/src/graph.rs`'s `symbol_by_name_per_file` is a
`HashMap<SmolStr, SymbolId>` — one slot per name, per file. CSS custom properties are routinely
redeclared across multiple rule blocks in the *same* file for real, idiomatic reasons (a
`:root { --accent: blue; }` base value overridden per-theme in `.dark { --accent: purple; }`).
Emitting a second `Declaration` for the same name in the same file wouldn't error, but it would
silently become unreachable *by name lookup* (the second insert wins or loses arbitrarily,
depending on iteration order — either way, one of the two is never resolvable), which is a
subtler and worse failure than not tracking it at all. Extraction keeps only the *first*
textual occurrence of each distinct `--name` per file; later same-name occurrences contribute
no additional `Declaration`, but their `var(--name)` reads and writes still resolve normally
against the first one (§0's under-detection is the deliberately safe direction: a genuinely
unused custom property that appears in two rule blocks might, in principle, only get flagged
via its first occurrence — never the reverse).

**References — `var(--name)` only, same-file.** A `call_expression` whose `function_name` text
is exactly `"var"` (case-sensitive; CSS function names are, per spec, ASCII-case-insensitive in
real engines, but `tree-sitter-css` doesn't normalize case and neither does this extraction —
a documented simplification, §7) with a first argument whose text starts with `--` emits a
`RawReference { name: "--name", kind: Read, within: None, .. }`. `within` is always `None`
(RFC 0012 §4's own stated fallback, "any miss falls back to `NodeRef::File` — the safe
direction") — v1 tracks no rule-level symbol a reference could meaningfully belong to, since
selector extraction is deferred (§0). Resolution is same-file only (RFC 0012 §4: "`None`
[`unit`] — every adapter before Go — keeps today's exact behavior, same-file-only, unless an
import binds the name"); a `var(--name)` reaching into a *different*, `@import`-ed file's
declaration is not modeled in v1 (§7) — CSS custom properties are lexically global across the
real cascade in a way `@import` doesn't map onto kndo's per-file resolution model cleanly, and
inventing that mapping is real, uncertain-value design work, not a quick addition.

**Imports — `@import`, both syntactic forms.** `import_statement` accepts either a bare string
(`@import "base.css";`) or a `url(...)`-wrapped one (`@import url("theme.css");`) —
`tree-sitter-css` parses them to different shapes (`(import_statement (string_value ...))` vs.
`(import_statement (call_expression (function_name) (arguments (string_value ...))))`), so
extraction searches an `import_statement`'s subtree for the first `string_value` node either
way rather than hand-rolling both shapes twice. `@use` (SCSS-only) doesn't appear in the plain-
CSS grammar at all — nothing to handle, consistent with §0's SCSS deferral.

**Not modeled in v1** (§0, §5): selector/class/id declarations (`SymbolKind::CssRule` stays
unused), `composes` (has nothing to resolve against without selector declarations), `url(...)`
asset references (images/fonts — no "asset" vocabulary exists to resolve them against), cross-
file `var()` resolution, `@media`/`@supports`/`@keyframes`/`@font-face`/other at-rules beyond
being walked *through* (their nested `declaration`s still contribute custom properties/`var()`
references normally — only their own at-rule-specific syntax, e.g. a `@keyframes` name, is
untouched).

**Suppressions**: `/* kndo:allow ... */` — identical convention to every comment-capable
adapter (RFC 0005 §12 is language-neutral).

**Metrics**: not applicable — no functions exist to measure (same as JSON §0).

**Roots**: none (RFC 0002 §7's own table already says so for CSS) — no entry-point concept
exists in the language, and §0 already ruled out symbol-level rooting as a usage-blindness
workaround.

## 3. Imports & resolution

`resolve()` handles relative specifiers only (`./foo.css`, `../foo.css`) — literal path,
joined against the importing file's directory, matched exactly against `ResolveCtx`'s
known-files index (no extension-implicit resolution the way JS's `candidates()` tries `.ts`
before `.js`: real browsers require the literal `.css` path in an `@import`, and inventing
extension-omission tolerance for a bundler/preprocessor convention rather than the CSS
language itself would blur the same "language spec, not ecosystem fashion" line RFC 0002 §2
draws). A bare specifier (`@import "normalize.css";` meaning "resolve me against some
lookup path," a preprocessor/bundler convention, not core CSS) has no manifest to resolve
against — CSS declares none (§4) — so it resolves to `Resolution::Unresolved`, the same
honest-incompleteness stance JSON takes on `tsconfig.json` paths (docs/adapters/json.md §0)
rather than guessing.

## 4. Manifests & packages (RFC 0011)

Not applicable — CSS has no manifest format of its own (`manifest_globs: vec![]`,
`claim_manifest()` always `false`), same as JSON (docs/adapters/json.md §4).

## 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| Per-selector "unused CSS rule" (class-name usage from JS/TS/HTML) | Deliberately deferred, not attempted — §0's two-part argument (real usage is invisible today; rooting to compensate breaks file-level `unused`) is the authoritative record of why, not an oversight to revisit casually |
| `composes` (CSS Modules) | Deferred alongside selector extraction (§0) — nothing to resolve `composes: btn from "./other.css"` against without a `btn` declaration to reference |
| `url(...)` asset references (images, fonts, `url(data:...)`) | Out of scope — no "asset" vocabulary exists in kndo to resolve a binary target against; RFC 0002 §3 doesn't mention it for CSS either |
| Cross-file `var()` resolution through `@import` | Not modeled (§2) — CSS custom properties are lexically global across the real cascade in a way `@import`'s file-to-file edge doesn't cleanly represent; real future work, not a quick fix |
| SCSS/LESS (`$variables`, `@mixin`/`@include`, nesting, `&`) | Out of scope for v1 (§0) — a fast-follow with its own grammar (`tree-sitter-scss`/`tree-sitter-less`, both available) and its own verified declaration/reference mapping, not a same-commit addition |
| `var()`/function-name case sensitivity | Not normalized — `tree-sitter-css` doesn't lowercase, extraction doesn't either; a `VAR(--x)` (valid per spec, vanishingly rare in practice) would not be recognized |
| Nested `@media`/`@supports`/`@keyframes`/etc. | Walked *through* uniformly for their nested custom-property declarations/`var()` references (§2) — their own at-rule-specific syntax is otherwise untouched |
| CSS Modules' generated hashed class names | N/A in v1 — no selector extraction exists yet for a hash to interact with |

## 6. Conformance fixtures (shared harness, RFC 0002 §8)

Three fixtures — a deliberately narrow set matching §0's deliberately narrow scope:

- **`orphaned-css-file-is-unused`** — mirrors docs/adapters/json.md §6's
  `orphaned-config-is-unused` on the same JS-TS-imports-a-target shape: a JS-TS project imports
  `main.css` (making it reachable), while a sibling `unused.css` is never imported by anything
  — `unused.css` reads `unused`/`file`, `main.css` doesn't. Run with `CssAdapter` *and*
  `JsTsAdapter` together (the same cross-language-necessity reasoning as JSON's own mixed
  fixture, docs/adapters/json.md §6).
- **`import-graph-and-unused-variable`** — `main.css` `@import`s `tokens.css` (proving the
  same-language `@import` graph resolves, both syntactic forms exercised); `tokens.css`
  declares `--used` (read via `var(--used)` in `main.css`) and `--dead` (declared, never read)
  — `--dead` reads `unused`, `--used` doesn't, `tokens.css` itself reads not-unused (reached via
  the `@import` edge even though nothing reads *it* directly by file-level import from outside
  CSS).
- **`redeclared-variable-and-generated-origin`** — one file redeclares `--accent` in two rule
  blocks (`:root` then `.dark`, §2's "first occurrence wins" case) with only the *second*
  occurrence's value actually read via `var()`, proving the first declaration still resolves
  the reference correctly; a second file carries a `/* Generated by tool X */` banner and is
  otherwise identical to `unused.css` above, proving `detected_origin: Generated` exempts it
  from the `unused`/`file` finding the plain orphaned file gets.

## 7. Open questions

1. **Selector-level extraction (`SymbolKind::CssRule`) and the class-name cross-language
   linkage.** §0's central deferred item — needs either an RFC 0003 plugin or a JS-TS
   extraction change (JSX `className`, CSS-Modules import-then-property-access) before it's
   safe to turn on without a false-positive flood or the file-reachability regression §0
   documents. Tracked here as the adapter's own open item; the actual mechanism belongs to
   whichever RFC ends up owning it.
2. **Cross-file `var()` resolution.** §2/§5 — real value, real design work (does an `@import`
   edge widen a file's resolution scope the way a Go package directory does? RFC 0012 §4's
   `unit` mechanism might be the right shape, might not — needs its own investigation before
   committing to an approach).
3. **SCSS/LESS.** §0's scope boundary — both grammars exist and are available
   (`tree-sitter-scss`, `tree-sitter-less`); a fast-follow, not a v1 blocker.
4. **`url(...)` asset resolution.** §5 — would need an "asset" concept kndo doesn't have yet
   (a claimed-but-symbol-less file class, similar in shape to how JSON participates today);
   whether that's worth inventing depends on real demand, not spec-writing speculation.
5. **Bare `@import` specifiers (bundler resolve-path convention).** §3 — currently
   `Unresolved`, honestly; revisit only if kndo ever grows a CSS-bundler-config-reading
   mechanism (postcss.config.js, etc.) — itself a framework-convention concern RFC 0002 §2
   would put in plugin territory, not here.
