# Adapter Spec — CSS

**Status:** Draft · **Depends on:** RFC 0002 §3, §7 · docs/rfcs/0012-reference-semantics-and-visibility.md (member_of §3, within §4, RefKind §5, visibility ladder §6, generated-origin §7, unit-key §8) · ADR 0002 (tree-sitter)

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

Selector-level declarations and the class-name cross-language linkage are **explicitly
deferred**, not silently dropped: §7 records exactly what needs to exist first (a plugin or a
JS-TS extraction change) before `SymbolKind::CssRule` extraction is safe to turn on. That
argument is language-independent — it applies exactly as much to an SCSS `.card` selector as to
a plain-CSS one — so bringing SCSS into scope (below) doesn't reopen it.

**Grammar & scope: CSS and SCSS, LESS deferred.** `tree-sitter-css` for `.css`,
`tree-sitter-scss` for `.scss` — both real, available grammars (unlike JSON's ADR 0002 escape
hatch: there's a genuine syntax tree worth walking here). The two grammars share almost their
entire node vocabulary verbatim — `declaration`/`property_name`/`import_statement`/
`call_expression`/`function_name`/`class_selector`/`id_selector` all parse identically in both
— because `tree-sitter-scss` is a strict grammar superset, not a fork with renamed nodes. What
SCSS adds on top, all handled here (§2): `$variable` declarations/references (a dedicated
`variable` leaf node, distinct from CSS custom properties' `--name: value;`/`var(--name)`
call-expression shape, but the same `SymbolKind::CssVariable` — RFC 0002 §3 groups "variables"
as one symbol kind regardless of sigil), `@mixin`/`@include` (a mixin is a named, invokable,
parameterized block — `SymbolKind::Other("mixin")`, the same "adapter-specific facet" escape
hatch Go's `type`/Java's `record`/Rust's `macro` already use), `@function`/`@return` (invoked
like any ordinary function call — no special extraction needed beyond the generic
call-expression reference already extracted for `var()`/`url()`, §2), and `@use`/`@forward`
(SCSS's module system, alongside plain `@import` which SCSS also accepts — §3's own resolution
algorithm). Selector nesting and the `&` parent selector don't matter to this scope at all:
they're purely a *selector*-level concern, and selector extraction stays deferred (above) for
CSS and SCSS alike — nesting never changes what counts as a variable, mixin, or import.

**LESS stays out of v1** — no grammar was pulled in, no shapes were verified, and LESS's own
variable sigil (`@foo`, colliding syntactically with CSS at-rules) and mixin-via-selector-call
convention (`.mixin();`) are different enough from SCSS's that "just add the third grammar"
would not be the same small increment SCSS turned out to be. A real fast-follow (§7), not
folded in here.

## 1. Claiming & classification

**Globs:** `**/*.css` and `**/*.scss` (§0) — the extension picks which grammar `parsing.rs`
hands the content to; every other part of `claim()`/classification is identical between the
two, so `.scss` is not a second adapter, just a second glob entry and a dispatch on extension
inside `extract()`.

**No cross-adapter exclusions needed** (unlike JSON's `package.json`/`tsconfig.json`
carve-out): no other launch-set adapter declares a `.css`- or `.scss`-shaped `manifest_glob`,
so there is no well-known manifest either glob could accidentally swallow.

**Role**: always `Production` — same stance, same reasoning, as docs/adapters/json.md §0/§1: no
`*.test.css`/`*.test.scss` ecosystem convention exists to encode, and inventing one without a
real signal would be speculative. Revisit only with real dogfooding evidence.

**Origin**: real generated-origin detection, unlike JSON — both CSS and SCSS *have* comment
syntax (`/* ... */`), so the same `kndo_adapter_toolkit::classify::ContentMarkers` mechanism
every other adapter uses applies unmodified: `comment_openers: &["/*"]`, matching against the
ecosystem's real banners (Sass/Less compiler output, Tailwind's compiled CSS, PostCSS pipelines
routinely emit `/*! Generated by ... */`-shaped headers). No structural (AST-derived) signal
exists the way Java's `@Generated` annotation does — this is the line-scan mechanism, same tier
as Go's/JS's/Rust's.

## 2. Extraction

**Declarations — custom properties and SCSS variables** (§0): a `declaration` node whose
`property_name` text starts with `--` (there is no distinct "custom property" grammar node —
both grammars reuse `property_name` for `color: red` and `--brand: red` alike; the `--` prefix
is a plain text check, not a node-kind check) becomes one `Declaration { kind:
SymbolKind::CssVariable, member_of: None, ... }`. An SCSS top-level `$name: value;` — same
`declaration`/`property_name` shape, `$` prefix instead of `--` — gets the *same* treatment,
same `SymbolKind::CssVariable`: RFC 0002 §3 groups "variables" as one symbol kind regardless of
which language's variable syntax declared it.

**Declarations — SCSS mixins and functions.** `@mixin name(...) { ... }` → `mixin_statement`
has a real named `name: (identifier)` field (unlike every selector/property node in this
grammar, which are field-less) — one `Declaration { kind: SymbolKind::Other("mixin"),
member_of: None, ... }` per mixin, named after that field's text. `@function name(...) {
... }` → `function_statement`, same `name` field shape — one `Declaration { kind:
SymbolKind::Function, ... }` (an SCSS function *is* an ordinary function: pure, returns a value
via `@return`, invoked with call syntax — the closest existing `SymbolKind`, no `Other` needed).

**One Declaration per unique name per file — first occurrence wins, not one per occurrence.**
Verified, not assumed: `kndo-core/src/graph.rs`'s `symbol_by_name_per_file` is a
`HashMap<SmolStr, SymbolId>` — one slot per name, per file. CSS custom properties (and, just as
commonly, SCSS variables) are routinely redeclared across multiple rule blocks in the *same*
file for real, idiomatic reasons (a `:root { --accent: blue; }` base value overridden per-theme
in `.dark { --accent: purple; }`). Emitting a second `Declaration` for the same name in the
same file wouldn't error, but it would silently become unreachable *by name lookup* (the second
insert wins or loses arbitrarily, depending on iteration order — either way, one of the two is
never resolvable), which is a subtler and worse failure than not tracking it at all. Extraction
keeps only the *first* textual occurrence of each distinct name per file (across variables,
mixins, and functions alike — a mixin/function redeclaration is a real SCSS error case anyway,
not an idiomatic pattern the way variable overriding is, so this is purely a safety net there);
later same-name occurrences contribute no additional `Declaration`, but references to that name
still resolve normally against the first one (§0's under-detection is the deliberately safe
direction: a genuinely unused custom property that appears in two rule blocks might, in
principle, only get flagged via its first occurrence — never the reverse).

**References — `var(--name)`, bare `$name`, `@include name(...)`, and any other call, all
same-file.** A `call_expression` whose `function_name` text is exactly `"var"` (case-sensitive;
CSS function names are, per spec, ASCII-case-insensitive in real engines, but neither grammar
normalizes case and neither does this extraction — a documented simplification, §7) with a
first argument whose text starts with `--` emits `RawReference { name: "--name", kind: Read,
within: None, .. }`. A bare `variable` leaf node (SCSS's `$name` used directly as a value —
grammatically distinct from CSS's `var(--name)` call-expression wrapping, no wrapping needed
since `$name` interpolates directly) emits the same shape, `name: "$name"`. `@include
name(...)` (`include_statement`, no named field — the mixin name is its first `identifier`
child) emits `RawReference { name: "name", kind: Call, .. }`. Every *other* `call_expression`
whose `function_name` isn't `"var"` (i.e. not the two builtins this extraction special-cases,
`var`/`url`) also emits a plain `Call` reference to that function name — this is what makes
`@function` invocations resolve *for free*: a call to a user-defined SCSS function is
syntactically indistinguishable from a call to `url()`, so the core's ordinary same-file
name-based symbol resolution (phase 3b, no `member_of` needed since these are never members)
finds the matching `@function`/`@mixin` declaration the same way it resolves any other bare-name
call in any other language — no SCSS-specific resolution logic required beyond "don't
special-case `var`/`url` away." `within` is always `None` for every one of these (RFC 0012 §4's
own stated fallback, "any miss falls back to `NodeRef::File` — the safe direction") — v1 tracks
no *rule*-level symbol a reference could meaningfully belong to (selector extraction stays
deferred, §0), but mixin/function *bodies* are real callable symbols with their own qualified
name available for `within` attribution when a reference sits inside one — extraction sets
`within` to the enclosing mixin/function's name for references inside `mixin_statement`/
`function_statement` bodies specifically (the one place in this adapter something other than
`None` applies), and `None` everywhere else (plain rule bodies, top-level). Resolution is
same-file only everywhere (RFC 0012 §4: "`None` [`unit`] — every adapter before Go — keeps
today's exact behavior, same-file-only, unless an import binds the name"); reaching into a
*different*, imported file's declaration is not modeled in v1 for any of these reference kinds
(§7) — real future work, not a quick addition.

**Imports — `@import`, `@use`, `@forward`.** `import_statement` (both grammars) accepts either
a bare string (`@import "base.css";`) or a `url(...)`-wrapped one (`@import url("theme.css");`)
— parsed to different shapes (`(import_statement (string_value ...))` vs. `(import_statement
(call_expression (function_name) (arguments (string_value ...))))`), so extraction searches an
`import_statement`'s subtree for the first `string_value` node either way rather than
hand-rolling both shapes twice. SCSS's `use_statement`/`forward_statement` (`@use "path";`/
`@forward "path";`, no `url()` form in real Sass syntax) get the same string-value extraction,
tagged with their own resolution algorithm (§3) — a `@use`/`@forward` specifier is a *module*
name, not a literal file path, and Sass's partial-file convention (`_foo.scss` satisfying
`@use "foo"`) is genuinely different resolution from `@import`'s literal-path CSS semantics.

**Not modeled in v1** (§0, §5): selector/class/id declarations (`SymbolKind::CssRule` stays
unused), `composes`/`@extend` (nothing to resolve against without selector declarations —
`@extend` doubly so since `tree-sitter-scss` 1.0.0 has its own upstream parse bug on it, §5),
`url(...)` asset references (images/fonts — no "asset" vocabulary exists to resolve them
against), cross-file variable/mixin/function resolution, `@media`/`@supports`/`@keyframes`/
`@font-face`/`@if`/`@each`/`@for`/`@while`/other at-rules beyond being walked *through* (their
nested `declaration`s still contribute variable declarations/references normally — only their
own at-rule-specific syntax, e.g. a `@keyframes` name or an `@each` loop variable, is untouched).

**Suppressions**: `/* kndo:allow ... */` — identical convention to every comment-capable
adapter (RFC 0005 §12 is language-neutral).

**Metrics**: not extracted — SCSS `@mixin`/`@function` bodies do have real control flow
(`@if`/`@each`/`@for`/`@while`), so cyclomatic complexity is *not* structurally meaningless the
way it is for plain CSS, but wiring `kndo-adapter-toolkit::metrics` for it is a deliberately
separate increment (§7), not bundled into "add declarations for them" here — mixins/functions
are typically small and few per file in real SCSS, so the value is real but not urgent enough
to justify the extra `MetricsSyntax`/branch-kind-mapping surface in this pass.

**Roots**: none (RFC 0002 §7's own table already says so for CSS) — no entry-point concept
exists in the language, and §0 already ruled out symbol-level rooting as a usage-blindness
workaround.

## 3. Imports & resolution

**`@import` (both CSS and SCSS)**: relative specifiers only (`./foo.css`, `../foo.scss`) —
literal path, joined against the importing file's directory, matched exactly against
`ResolveCtx`'s known-files index (no extension-implicit resolution the way JS's `candidates()`
tries `.ts` before `.js`: real browsers require the literal `.css` path in a plain-CSS
`@import`, and inventing extension-omission tolerance for a bundler/preprocessor convention
rather than the CSS language itself would blur the same "language spec, not ecosystem fashion"
line RFC 0002 §2 draws). A bare specifier (`@import "normalize.css";` meaning "resolve me
against some lookup path," a preprocessor/bundler convention, not core CSS) has no manifest to
resolve against — CSS/SCSS declare none (§4) — so it resolves to `Resolution::Unresolved`, the
same honest-incompleteness stance JSON takes on `tsconfig.json` paths (docs/adapters/json.md
§0) rather than guessing.

**`@use`/`@forward` (SCSS only)**: a *module* specifier, not a literal path. Unlike JS, Sass
doesn't require a leading `./` to mean "resolve relative to this file" — `@use "variables"`
(no dot) and `@use "./variables"` mean the same thing. A `sass:`-prefixed specifier (`@use
"sass:math"`) names a *built-in* Sass module — never a file, resolves to
`Resolution::Unresolved` unconditionally, checked first. Everything else is joined against the
importing file's directory the same way a relative specifier is, then tried as several
candidates because Sass's own "partial" convention lets a file opt into module-only status by
prefixing its name with `_` (a leading-underscore file is excluded from being compiled to its
own CSS output, existing only to be `@use`d/`@forward`ed): the literal path as given
(`variables`), then `{path}.scss`, then `_{basename}.scss` in the same directory (the
partial-file form), then `{path}/_index.scss` (a directory-as-module convention Sass also
supports). First match against `ResolveCtx`'s known-files index wins — a best-effort, syntactic
approximation of Sass's real module resolution (ADR 0002's own "tree-sitter is syntactic...
approximated" stance extends naturally to this), not a byte-exact reimplementation of the Dart
Sass compiler's file-system probing order. A genuinely bare package-style specifier (a real
Sass/npm package published for `@use`, not a local file) still resolves to `Resolution::
Unresolved` when no candidate matches — honestly incomplete, no Sass "manifest"/package
convention is modeled (§4), same stance as JS-TS's own bare-specifier handling minus the
dependency-manifest half.

**`Missing` vs `Unresolved` (contracts §2.1).** Only an *explicitly* relative specifier
(`./x`, `../x`) resolves to `Resolution::Missing` on a miss: nothing else it could have been,
so the answer is complete and the `unresolved` analysis reports a broken path. A **bare**
specifier stays `Unresolved` however thoroughly the candidate ladder missed — Sass resolves
those through load paths and `node_modules` too, which this adapter does not read, and
`@import "bootstrap"` in a real Spring PetClinic stylesheet is exactly that shape. A `sass:`
module and an `http(s)://`/`//` URL are not paths into the project at all and are checked
first. The rule cost nothing in recall and removed 27 false accusations from the corpus the
first measurement produced.

## 4. Manifests & packages (RFC 0011)

Not applicable — neither CSS nor SCSS has a manifest format of its own (`manifest_globs:
vec![]`, `claim_manifest()` always `false`), same as JSON (docs/adapters/json.md §4). A real
Sass package ecosystem exists (published `@use`-able packages), but it has no single dominant
manifest convention the way `package.json`/`Cargo.toml` do — out of scope, §3/§7.

## 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| Per-selector "unused CSS rule" (class-name usage from JS/TS/HTML) | Deliberately deferred, not attempted — §0's two-part argument (real usage is invisible today; rooting to compensate breaks file-level `unused`) is the authoritative record of why, not an oversight to revisit casually. Applies to SCSS selectors identically. |
| `composes` (CSS Modules) / `@extend` | Deferred alongside selector extraction (§0) — nothing to resolve `composes: btn from "./other.css"`/`@extend %placeholder` against without a selector/placeholder declaration to reference. `@extend` doubly so: `tree-sitter-scss` 1.0.0 has its own upstream parse bug on `@extend %name;` (verified directly — `has_error()` is `true`, an `ERROR` node wraps the `placeholder` node), a third-party grammar issue in the same category as the Kotlin adapter's documented tree-sitter-kotlin-ng edge cases — not something to work around here. |
| `url(...)` asset references (images, fonts, `url(data:...)`) | Out of scope — no "asset" vocabulary exists in kndo to resolve a binary target against; RFC 0002 §3 doesn't mention it for CSS either |
| Cross-file variable/mixin/function resolution through `@import`/`@use`/`@forward` | Not modeled (§2) — real future work, not a quick fix (§7) |
| `@use "path" as alias;` (the namespacing form) | `tree-sitter-scss` 1.0.0 has an upstream parse bug here too — verified directly: `@use "variables";` parses clean, `@use "variables" as vars;` produces an `ERROR (UNEXPECTED 's')` node. Extraction still recovers the specifier correctly (the `string_value` child is present and unaffected despite the trailing error, same partial-tree-survives-an-error shape every tree-sitter-backed adapter already handles via `root.has_error()`), but the file gets a "parse errors — extraction is partial" diagnostic it arguably doesn't deserve. Documented, not worked around, same posture as the `@extend` bug above. |
| LESS (`@variables`, `.mixin();` calls, `&`) | Out of scope for v1 (§0) — a real fast-follow with its own grammar (`tree-sitter-less`, available) and its own verified declaration/reference mapping; LESS's variable sigil collides syntactically with CSS at-rules in a way SCSS's `$` doesn't, so it isn't the same small increment SCSS turned out to be |
| `var()`/function-name case sensitivity | Not normalized — neither grammar lowercases, extraction doesn't either; a `VAR(--x)` (valid per spec, vanishingly rare in practice) would not be recognized |
| Nested `@media`/`@supports`/`@keyframes`/`@if`/`@each`/`@for`/`@while`/etc. | Walked *through* uniformly for their nested variable declarations/references (§2) — their own at-rule-specific syntax (a `@keyframes` name, an `@each` loop variable) is otherwise untouched |
| CSS Modules' generated hashed class names | N/A in v1 — no selector extraction exists yet for a hash to interact with |
| SCSS `@mixin`/`@function` cyclomatic complexity | Not extracted (§2) — real control flow exists inside these bodies, deliberately deferred as its own increment, not bundled into declaration extraction |

## 6. Conformance fixtures (shared harness, RFC 0002 §8)

Four fixtures — a deliberately narrow set matching §0's deliberately narrow scope, one of them
carrying the whole SCSS increment:

- **`orphaned-css-file-is-unused`** — mirrors docs/adapters/json.md §6's
  `orphaned-config-is-unused` on the same JS-TS-imports-a-target shape: a JS-TS project imports
  `main.css` (making it reachable), while a sibling `unused.css` is never imported by anything
  — `unused.css` reads `unused`/`file`, `main.css` doesn't. Run with `CssAdapter` *and*
  `JsTsAdapter` together (the same cross-language-necessity reasoning as JSON's own mixed
  fixture, docs/adapters/json.md §6).
- **`import-graph-and-unused-variable`** — `main.css` (rooted via a JS-TS `import`, since CSS
  has no root of its own, §2) `@import`s `tokens.css` (proving the same-language `@import`
  graph resolves, §3); `tokens.css` declares `--used` (read via `var(--used)` from its *own*
  `.tinted` rule — same-file, matching §2's same-file-only resolution scope) and `--dead`
  (declared, never read anywhere) — `--dead` reads `unused`, `--used` doesn't, `tokens.css`
  itself reads not-unused (reached via the `@import` edge even though nothing reads *it*
  directly by file-level import from outside CSS).
- **`redeclared-variable-and-generated-origin`** — one file (rooted via a JS-TS `import`, as
  above) redeclares `--accent` in two rule blocks (`:root` then `.dark`, §2's "first occurrence
  wins" case) and reads it via `var(--accent)` from a third rule — proving the *first*
  declaration (kndo tracks existence/name, not the real CSS cascade's runtime-winning value, so
  "which literal value wins at `.dark`" is out of scope by construction) still resolves the
  reference correctly despite the redeclaration. The same file also declares `--truly-dead`,
  never referenced anywhere — proving ordinary `unused` detection still fires correctly
  alongside the redeclaration handling, not just "nothing breaks." A second file carries a
  `/* Generated by tool X. DO NOT EDIT. */` banner and is otherwise an ordinary orphaned file
  like `unused.css` above — proving `detected_origin: Generated` exempts it from the
  `unused`/`file` finding the plain orphaned file gets.
- **`scss-variables-mixins-and-use`** — `_tokens.scss` (a real Sass partial file, leading
  underscore) declares `$brand`, a `@mixin flex-center`, an `@function double($n)`, and a
  `$unused-token` never read anywhere; a `.demo` rule in the *same* file reads `$brand`,
  `@include`s `flex-center`, and calls `double(4px)` via ordinary call syntax — same-file, since
  cross-file variable/mixin/function resolution isn't modeled (§2/§7). `main.scss` (rooted via a
  JS-TS `import`, the same cross-language-necessity shape as every fixture here) only `@use`s
  `"tokens"`, proving the partial-file resolution candidate list on its own (§3): reaching
  `_tokens.scss` at all is what keeps `$brand`/`flex-center`/`double` in scope for their
  same-file uses to matter. `$unused-token` — declared, never referenced by anything, in *or*
  out of the file — reads `unused`; the other three don't. `double`'s resolution falling out of
  the generic `call_expression` handling with no SCSS-specific resolution code is §2's central
  claim about `@function`, verified here rather than only unit-tested.

## 7. Open questions

1. **Selector-level extraction (`SymbolKind::CssRule`) and the class-name cross-language
   linkage.** §0's central deferred item — needs either an RFC 0003 plugin or a JS-TS
   extraction change (JSX `className`, CSS-Modules import-then-property-access) before it's
   safe to turn on without a false-positive flood or the file-reachability regression §0
   documents. Applies identically to SCSS selectors. Tracked here as the adapter's own open
   item; the actual mechanism belongs to whichever RFC ends up owning it.
2. **Cross-file variable/mixin/function resolution.** §2/§5 — real value, real design work
   (does an `@import`/`@use` edge widen a file's resolution scope the way a Go package
   directory does? RFC 0012 §4's `unit` mechanism might be the right shape, might not — needs
   its own investigation before committing to an approach; `@use`'s real module semantics — a
   used module's names are namespaced by default, unlike `@import`'s flat concatenation — make
   this a genuinely different problem for SCSS than for plain CSS, not the same fix twice).
3. **LESS.** §0's scope boundary — `tree-sitter-less` exists and is available, but LESS's
   variable/mixin conventions differ enough from SCSS's (§5) that this is a real fast-follow
   with its own verification pass, not a mechanical repeat of the SCSS work.
4. **`url(...)` asset resolution.** §5 — would need an "asset" concept kndo doesn't have yet
   (a claimed-but-symbol-less file class, similar in shape to how JSON participates today);
   whether that's worth inventing depends on real demand, not spec-writing speculation.
5. **Bare `@import`/bare `@use` package-style specifiers.** §3 — currently `Unresolved`,
   honestly, for both; revisit only if kndo ever grows a CSS-bundler-config-reading mechanism
   (postcss.config.js, etc.) or a real Sass-package-registry convention worth modeling — either
   is a framework/ecosystem-convention concern RFC 0002 §2 would put in plugin territory, not
   here.
6. **SCSS `@mixin`/`@function` cyclomatic complexity and duplicate-detection token streams.**
   §2/§5 — real control flow and real function bodies exist now; wiring
   `kndo-adapter-toolkit::metrics` for them is a small, well-scoped follow-up once the
   declaration/reference work here has a conformance-verified baseline to build on.
7. **`@use "path" as alias;` / `@extend %placeholder;` upstream grammar bugs.** §5 — both
   verified directly against `tree-sitter-scss` 1.0.0; worth re-checking on any future grammar
   version bump (the same "re-verify against a grammar bump" discipline `parsing.rs`'s
   `#[ignore]`d probe tests already establish for other adapters) rather than assuming they're
   permanent.
