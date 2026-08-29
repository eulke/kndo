# Adapters

Language adapter specs — how each `LanguageAdapter` implementation claims files,
extracts declarations/references/roots, resolves imports, and reads its ecosystem's
manifests. Each section below was originally its own document under `internal/adapters/`;
they are merged here per the consolidation recorded in
`.wayfinder/tickets/33-consolidation-decision.md`. Section numbers inside each language's
spec (§0, §1, …) are that language's own and are not comparable across sections.

## CSS

**Status:** Draft · **Depends on:** RFC 0002 §3, §7 · docs/rfcs/0012-reference-semantics-and-visibility.md (member_of §3, within §4, RefKind §5, visibility ladder §6, generated-origin §7, unit-key §8) · ADR 0002 (tree-sitter)

### 0. What's structurally different from every prior adapter, and why it matters here

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

### 1. Claiming & classification

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

### 2. Extraction

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

### 3. Imports & resolution

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

### 4. Manifests & packages (RFC 0011)

Not applicable — neither CSS nor SCSS has a manifest format of its own (`manifest_globs:
vec![]`, `claim_manifest()` always `false`), same as JSON (docs/adapters/json.md §4). A real
Sass package ecosystem exists (published `@use`-able packages), but it has no single dominant
manifest convention the way `package.json`/`Cargo.toml` do — out of scope, §3/§7.

### 5. Known hard cases & stances

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

### 6. Conformance fixtures (shared harness, RFC 0002 §8)

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

### 7. Open questions

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

## Go

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M3
**Grammar:** tree-sitter-go

The second adapter, and the one that exists specifically to test the contract (RFC 0002 §1: "adding
a language is adding one crate that implements one trait" — if that claim doesn't survive a second,
structurally different language, it isn't a real claim). Go is deliberately not JS-with-different-
syntax: no relative imports, visibility is a naming convention rather than a keyword, and a
"package" is a directory of files with no import needed between them — that last one required a
core contract change (`FileFacts::unit`, contracts §2) before this adapter could even be started;
see §1.1.

### 0. What's structurally different from JS/TS, and why it matters here

- **Package-scoped, not file-scoped, visibility.** A Go *package* is its containing directory —
  every `.go` file directly inside one directory belongs to the same package (the compiler
  enforces this: a directory literally cannot mix packages, with one narrow exception, §5), and
  package members see each other with **no import statement at all**, exported or not. This is
  the ordinary shape of real Go code (splitting one package across multiple files by concern is
  idiomatic), not an edge case — which is why it needed a core change rather than an adapter-side
  workaround (contracts §2, `FileFacts::unit`): this adapter sets `unit` to the file's own
  directory, and the core's phase-3b reference resolution falls back to "same unit" after
  same-file and import-bound lookups fail.
- **No relative imports.** Every import is a fully-qualified path (`"encoding/json"`,
  `"github.com/foo/bar/baz"`) resolved against the current module's declared path (`go.mod`'s
  `module` directive) plus a directory-listing step — never against the importing file's own
  location. `ImportKind::Relative` is simply never emitted by this adapter.
- **Visibility is capitalization, not a keyword.** An identifier is exported iff its first
  rune is uppercase — computed, not declared. There is no `export`, no `pub`, no manifest-level
  surface (no `exports` map equivalent) — see §1's `VisibilityLevel` note and §4.
- **`internal/` is a compiler-enforced boundary**, not a convention kndo has to police itself:
  packages under an `internal/` path segment are uncompilable outside the module subtree rooted
  at `internal/`'s parent. Two consequences: (a) it is Go's structural equivalent of npm's
  `private: true` for *root promotion purposes* (§4) — an `internal/` package's exported surface
  is not "externally consumed by definition" the way a normal package's is, so it isn't
  auto-promoted to a production root; (b) `deep-import` (RFC 0011 §4) explicitly skips boundaries
  "whose enforcement is unconditional at build time" — `internal/` is the RFC's own example — so
  this adapter records no deep-import-relevant surface data for it at all; the compiler already
  did the job.
- **No dynamic import surface worth modeling.** No `require(expr)`, no `import(expr)`, no `eval`.
  Every import is a `certain`-confidence static fact. `reflect`/`plugin`-based indirection exists
  but is rare, advanced, and not attempted here (§5) — the wildcard-edge machinery (RFC 0005 §1)
  stays available for it later if it turns out to matter in practice.
- **Root promotion is symbol-level, not file-level, far more often than in JS.** JS's manifest
  roots (`main`/`module`/`exports`) always name a *file*; the individual exports of that file get
  promoted too (RFC 0011 §5), but the file itself is always independently a root as well. Go has
  no manifest-level entry-file concept at all (§4) — `func main`, `func init`, and every promoted
  exported declaration (§2) are *symbol*-targeted roots with no accompanying file-level root. This
  exposed a real, previously-latent reachability gap: reference edges are file-granular by design
  (`graph::assemble`'s own doc — extraction tracks *which file* references something, not which
  enclosing symbol), so a file's outgoing references only ever get traversed once the BFS has
  visited that file *as a node*; a symbol reached only via its own direct `Root` edge never causes
  that visit. `main.go` and everything it (file-attributedly) referenced read as fully
  unreachable despite `main` genuinely being a root. Fixed at the core (`analysis/reachability.rs`
  — reaching a symbol now also reaches its owning file, at the same confidence), not worked around
  per-adapter, since the underlying bug — file-granular references never propagating past a
  symbol-only root — was always latent for JS too (RFC 0011 §5's own barrel-reexport promotion
  produces the identical shape); JS's existing test suite just never happened to exercise a case
  where the promoted file had *no other* path to reachability. Regression coverage:
  `reachability.rs`'s own unit test plus an end-to-end one in `kndo-adapter-go/tests/assembly.rs`
  (unit tests alone would have missed the file-granular-attribution interaction).

### 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `go` | `.go` (excludes `.go` files that fail to parse as Go — extraction degrades per §2's broken-code rule, never un-claims) |
| Manifests | `go.mod` and `go.work` (RFC 0012 §10). `go.sum`/`go.work.sum` are lockfiles (content hashes, not structure) — **not** claimed, same stance as JS's `package-lock.json` |
| Role `test` | `*_test.go` (Go's sole, compiler-recognized convention — no glob guessing needed) |
| Role `tooling` | not detected in this slice (§7) — Go has no ecosystem-wide config-file convention comparable to `webpack.config.js`; inventing pattern-matching for something with no real convention would be guessing, not claiming |
| Origin `generated` | detected from content via `FileFacts::detected_origin` (RFC 0012 §7 — the trait extension this row's earlier text called for): extraction scans the first 64 lines for Go's single authoritative marker (`// Code generated … DO NOT EDIT.`, `go help generate`) with the toolkit's `ContentMarkers` scanner, column-anchored so a generator mentioning the marker in a string literal doesn't classify as its own output. Assembly applies the override to the `FileNode` before any analysis, so every Generated exemption sees it (conformance fixture `generated-file/`: a dead `.pb.go` stays silent; the same file minus the banner is an `unused` finding). |
| Origin `vendored` | `vendor/**` — Go's actual `go mod vendor` output directory, already in the toolkit's `UNIVERSAL_VENDORED_DIRS` (kndo-adapter-toolkit `classify.rs`) — zero adapter-side work |

**`VisibilityLevel`**: `0` (unexported — lowercase first rune) or `1` (exported — uppercase first
rune), computed per declaration, never read from syntax the way JS reads an `export` keyword. The
descriptor declares the matching ladder (RFC 0012 §6): `[Unit "unexported", Package "exported
(internal)", Public "exported"]` — the middle rung (M6) is an export under an `internal/` path
element: compiler-walled from external modules, so capped (`surface_transitive: false`) and
Package-scoped, assigned by path at extraction time —
level 0 is **package**-grained (`Unit` = the `dir#package` key, §1.1), not file-grained, because an
unexported symbol is visible to every file in its package. That data closed this doc's own
previously-documented under-reporting: `internal-only` now computes the tightest-sufficient *rung*,
so an exported symbol used only by same-package sibling files is correctly flagged "unexported
would suffice" (conformance fixture `internal-only-unit/`), and the duck-typed member fallback
(RFC 0012 §3) scopes candidates by the same rungs — an unexported method is only a candidate
in-unit, Go's own legality rule. `internal/` is deliberately **not** a rung: it caps root
*promotion* (§0), a separate mechanism from who can name a symbol.

**1.1 Why `unit` had to exist first**: see §0. Set to `dir#declared-package-name` (RFC 0012
§8) for every claimed `.go` file — the directory *plus* the `package` clause's name, because
Go's real resolution unit is the package and one directory legally holds two: `package foo` and
its external test package `package foo_test` (the import-cycle-avoidance convention). Folding
the declared name into the opaque key splits them with zero core changes: a `foo_test` file
resolves none of `foo`'s unexported symbols by proximity — exactly Go's own rule (it imports
`foo` like any other consumer) — while an ordinary in-package `_test.go` (`package foo`) shares
the unit and sees them, also exactly Go's rule. A file whose package clause doesn't parse keys
on the directory alone (degenerate, groups with nothing wrongly).

### 2. Extraction

**Declarations**: top-level `func` (plain functions), methods (`func (t T) Name(...)` / `func (t
*T) Name(...)` — symbol name `T.Name`, so a value-receiver and pointer-receiver method pair on the
same type visibly share a namespace the way Go's own method-set rules do), `type` (struct,
interface, alias `type X = Y`, defined type `type X Y`), top-level `const` and `var` (including
grouped `const ( ... )` / `var ( ... )` blocks — one declaration per identifier, not one per
block), and `init` (Go's special no-args, unexported-by-construction, called-implicitly-by-the-
runtime function — always a root, §4, regardless of the capitalization rule, and there can be
more than one per file).

The **blank identifier declares nothing**: `var _ T = …` cannot be named by any source, so
extracting it as a symbol is a guaranteed false `unused`. It is not silence, though — the
statement exists to make a compile-time interface assertion, and that assertion is emitted as an
`Implement` reference from the concrete type to the interface. Both spellings are read: the
conversion form `var _ StructValidator = (*defaultValidator)(nil)`, which is by far the more
common (gin, hugo and go-redis each carry several), and the composite-literal form
`var API Core = jsonApi{}`.

A file that declares nothing at all — `doc.go`, a package doc comment plus `package gin` — is
handled in the core rather than here, and on a language-blind rule: a declarationless file whose
compilation **unit** is alive is never independently dead. Go's own rules compile it as part of
the package, so there is nothing in it to delete. The rule keys on the unit, not on emptiness:
an orphan that declares nothing and belongs to no live unit is still real waste and still
reported.

**References**: identifier uses (calls, reads, writes), each tagged with its `RefKind`
(RFC 0012 §5): `type_identifier` positions are `TypeUse` (the grammar itself is the
type-position signal), embedded struct/interface fields (a `field_declaration` with no name —
the type name doubling as the implicit member name) are `Extend`, everything else `Read`.
Qualified accesses are *structured facts*, not string synthesis (RFC 0012 §9): `json.Marshal`
extracts as `{ name: "Marshal", scope_context: Some("json") }` (and `pkg.Type` in type
positions the same, via the grammar's distinct `qualified_type` node, tagged `TypeUse`);
whether the qualifier names an import — by explicit alias or by the resolved target's declared
package name — or is a receiver variable is decided in assembly, which alone holds both sides.
This replaced the earlier dotted-binding synthesis and its documented last-segment alias guess.

**Roots (`RawRoot`, language-defined — RFC 0002 §2 point 3)**: `func main()` inside a file
declaring `package main` is always a `RootKind::Production` root, unconditionally (Go's literal
equivalent of npm's `bin` target — a binary entry point). `func init()` is always a
`RootKind::Production` root too (called by the runtime before `main`, in every package that has
one, not just `package main` — skipping it would read every side-effect-only `init` as dead code).
Every other exported (capitalized) top-level declaration in a **non-internal, non-test** file
becomes a root too — see §4 for why this is the right per-package analogue of JS's manifest-gated
`private` check, computed here in extraction rather than there because Go's root-worthiness is a
**per-file, path-derived fact** (is this file under `internal/`?), not a manifest-level one.

**Metrics** (`FunctionMetrics` — cyclomatic complexity, token fingerprints): populated for every
function and method, via the shared toolkit walker (`kndo_adapter_toolkit::metrics`), feeding
`crap` and structural `duplicate`. Each entry carries the **declaration's own span**, which is what
assembly resolves to a `SymbolId`; the entry's `symbol` name is display only. Passing the body's
span instead of the declaration's silently drops the metrics for that callable — assembly matches
spans exactly and does not fall back to a name lookup, deliberately: name lookup is what let
build-tag-alternated files declaring one name collapse onto a single symbol.

Each `func_literal` clearing the clone floor becomes its own callable **shape**
(`MetricsSyntax::nested_callable_kinds`): its branches and tokens leave the enclosing shape's
stream, which keeps one `FN` in their place, and `crap`/`duplicate` report it in its own right.
A smaller one stays an expression inside its owner — promoting it would leave both halves under
the floor and cost real clone findings (measured: 83 clone participants on the field corpus).
The split's semantics are uniform across adapters; only the kinds that trigger it are
per-language.

A body that is **only** a value construction (`composite_literal`) is not clone-eligible
(`MetricsSyntax::construction_kinds`): normalization erases the field values — the whole
authored content — and keeps the field list the type declaration dictates, so two constructions
of one type match by definition of the type rather than by evidence of copying.

**Suppressions**: `// kndo:allow …` on its own line or trailing a declaration — same syntax and
scope rules as JS/TS (RFC 0005 §12 is language-neutral; only comment *syntax* is adapter-owned,
and `//` line comments are identical between the two languages).

**Dynamic constructs → `DynamicUse`**: none emitted in this slice. See §0's last bullet and §5.

### 3. Imports & resolution

Every `RawImport` has `kind: ImportKind::Package` (never `Relative` — see §0) and
`confidence: Confidence::Certain` (see §0's last bullet). An `import _ "pkg"` (blank import, used
purely for side effects — `init()` registration) sets `side_effect_only: true`, matching JS's
`import "./polyfill"` shape exactly. An `import . "pkg"` (dot import — every exported name becomes
ambiently referenceable with no qualifying prefix, rare and mostly confined to test helper
packages) sets `opaque_namespace_use: true`: static per-name binding tracking would require
resolving every unqualified reference in the file against the dot-imported package's whole export
set, which needs full same-file name-shadowing awareness this extraction slice doesn't have —
following the same "wildcard over the resolved target's symbols" shape `graph::assemble` already
implements for JS's opaque namespace imports (contracts §2), no new core mechanism needed. An
aliased import (`import j "encoding/json"`) carries `local_alias: Some("j")` (RFC 0012 §9);
an unaliased one carries `None` — assembly derives its qualifier from the *target's* declared
package name (`FileFacts::unit_name`), the correct-by-construction fix for dir≠package
specifiers (`gopkg.in/yaml.v3` binds as `yaml`).

**Resolution algorithm** (the adapter's `resolve`):

1. **Same-module internal package.** If the specifier equals, or has as a `/`-segment prefix, the
   current module's own path (`go.mod`'s `module` directive) — looked up against the core's
   workspace-member index, which registers *every* named manifest in the graph, including this
   project's own single `go.mod` ("the monorepo model with n = 1," RFC 0011 §3), so a same-module
   subpackage import and a `go.work` sibling-module import share one lookup — the *variant*
   differs (RFC 0012 §10): the importer's own module resolves as plain `File` (a module can't
   require itself), a sibling module as `WorkspaceMember`, from which assembly derives both
   `ImportsFile` (reachability) and `ImportsDependency` — because go.work does **not** waive
   `require`: each module's `go.mod` must still declare its siblings for standalone builds, so
   an undeclared sibling import is a genuine phantom dependency (fixture
   `go-work-phantom-dep/`) and a declared-but-unimported one genuinely unused. A Go import
   names a **package** (a directory of files), and `Resolution::File` (contracts §2) names *one*
   file, so this adapter picks the alphabetically-first non-test `.go` file in the target
   directory as the nominal target (so `ImportsFile` reachability exists at all — an unimported
   directory correctly reads as unreachable) and relies on `FileFacts::unit` to make every file in
   that directory *individually* reachable for symbol resolution regardless of which one was
   nominally picked — both for unqualified same-unit references (contracts §2's `unit` field) and
   for import-binding lookups that miss on the nominal file (contracts §2's matching fallback on
   import-binding resolution). This is a real, documented approximation: file-level reachability
   (`unused`, `test-only`) ends up accurate at *directory* granularity for an externally-imported
   package (the whole package is reachable, not just the nominal file) — which is Go's actual
   truth anyway, since importing a package makes the *package* reachable, not one of its files
   more than another (§5).
   
   **Deliberately `Resolution::File`, not `Resolution::WorkspaceMember`** — the first draft used
   `WorkspaceMember` (matching JS's workspace-sibling shape exactly), and dogfooding caught why
   that's wrong for Go: assembly derives *both* an `ImportsFile` edge *and* an `ImportsDependency`
   edge from `WorkspaceMember` (contracts §2 — correct for JS, where every workspace member is a
   separate package that must be *declared* to be imported, RFC 0011 §4's phantom-dependency
   check). Go has no such contract: a module can't `require` itself, and a module importing its
   own subpackage read as `undeclared` — "phantom dependency on itself" — until this switched to
   `Resolution::File`, which still gets full `ImportsFile` reachability without inventing a
   dependency declaration Go doesn't have. Kept as a graph-assembly regression test
   (`kndo-adapter-go/tests/assembly.rs`), not just a resolver unit test, since the bug only shows
   up once assembly derives edges from the `Resolution` value.
2. **Stdlib.** No structural prefix exists in Go the way `node:` does (§0) — the whole precedence
   collapses to "is this exact import path in the generated stdlib list" (`kndo-stdlib v1`,
   `cargo xtask gen-stdlib go`, sourced from `go list std`). Checked *after* same-module internal
   resolution (a module path can never collide with a stdlib path in a real build, so order
   between these two never actually matters, but internal-first mirrors JS's "workspace before
   external ladder" precedence for consistency) and before the external-dependency check, so a
   module that (implausibly) required a package shadowing a stdlib import path would still resolve
   as the stdlib package — matching Go's own compiler behavior (there is no shadowing mechanism;
   `go.mod` cannot override what an import path structurally means).
3. **External dependency.** Longest-declared-prefix match against `go.mod`'s `require` entries:
   `require golang.org/x/net v0.10.0` + import `"golang.org/x/net/html"` → `Dependency
   ("golang.org/x/net", Certain)`. This is the one genuinely new resolution primitive Go needs
   that JS's flat `@scope/name`-is-always-two-segments convention doesn't (module paths have no
   fixed segment count) — implemented as a straightforward longest-prefix search over the
   declared require set, not a heuristic.
4. Anything matching none of the above (a typo'd or unresolvable import) → `Resolution::Unresolved`
   — same `unresolved` analysis territory as JS (RFC 0005 §5), not a parse error.

### 4. Manifests & packages (RFC 0011)

`go.mod` is a small line-oriented grammar (`module`, `go`, `require`/`replace`/`exclude` blocks) —
hand-parsed here rather than pulling in a dependency, the same "no more machinery than the format
needs" stance `package.json`'s `serde_json` parse takes for a format that *does* warrant a real
parser. `// indirect` require entries are **dropped** (M6, gin corpus): they are `go mod tidy`'s
bookkeeping of transitive requirements, not author declarations — nothing in the module imports
them by design, so declaring them would fabricate one false `unused` dependency each.

| `package.json` concept | `go.mod` equivalent | notes |
|---|---|---|
| `name` | `module` directive | the module path IS the package identity |
| `private: true` | *(no manifest equivalent — see below)* |
| `workspaces` | `go.work`'s `use` directives | parsed (RFC 0012 §10): single-line and block `use` forms → `ManifestFacts::workspace_members`; `go`/`toolchain`/`replace` directives contribute nothing (member modules self-register by declared module path; a path-*renaming* `replace` is the recorded divergence, not modeled) |
| dependency scopes (prod/dev/peer/optional) | `require (...)` | Go has exactly one scope — every entry is `DependencyScope::Prod`. `// indirect` comments mark transitively-pulled requires (Go's own `go mod tidy` bookkeeping) — not surfaced as a different scope, since kndo's scope taxonomy has no "transitive" concept and treating it as anything other than `Prod` would misrepresent it as unused/optional when it's exactly as required as a direct one |
| `main`/`module`/`exports` entry points, `declares_surface` | *(no equivalent)* | Go has no importable "default entry" and no explicit-surface declaration — every package directory is independently, uniformly importable by its full path. `ManifestFacts.declares_surface` is always `false` for Go: there is no `exports`-map-equivalent contract to gate `deep-import` on, and `internal/` (the one real boundary Go has) is skipped by RFC 0011 §4's own rule anyway (§0) |
| `bin` | `package main` + `func main()` | a **source-file** fact, not a manifest fact — see §2's Roots paragraph; `ManifestFacts.roots` is always empty for Go |
| `scripts` → tooling roots / invoked names | *(no equivalent)* | no script-runner convention in `go.mod`; `go:generate` directives are a stretch target, not attempted (§7) |

**`private` and library-mode root promotion, resolved without the manifest**: RFC 0011 §5's rule
("published/library: public API is a production root; private/app: exports need a real edge")
needs a *per-package* signal, and Go's real per-package privacy signal is **not** manifest-level at
all — it's the `internal/` path convention (§0), which is per-file/per-path, not per-module. So
`ManifestFacts.private` is always `false` for Go (there is no publish flag to read — this is a
statement of fact, not a default guess), and library-mode promotion happens entirely in extraction
(§2's Roots paragraph: every exported top-level declaration in a non-`internal/`, non-test file
roots itself) rather than through the `library_root_files`/manifest-root mechanism JS's promotion
rides (`graph::assemble` phase 3a, RFC 0011 §5's existing wiring) — that mechanism stays
byte-for-byte unchanged; Go simply doesn't use it, supplying `RawRoot`s directly instead. The net
effect is the RFC's intent either way: a package's genuinely-public surface is "externally
consumed by definition," an `internal/` package's is not.

### 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| External test package (`package foo_test` in a `_test.go` file) | treated as the same `unit` as `package foo` in the same directory (§1.1) — a documented, safe-direction imprecision, not a silent gap |
| Generic type parameters (`func F[T any](x T)`, `type Container[T any] struct{...}`) | the type-parameter list's constraint identifiers are ordinary references (e.g. `any`, a stdlib/local interface name); no special generics handling attempted beyond that — a constraint referencing a not-yet-declared local type still resolves correctly since phase 3a builds the whole file's symbol table before phase 3b resolves any reference, same ordering JS's forward-reference case already relies on |
| Method sets / interface satisfaction (does type `T` implement interface `I`?) | **not modeled** — Go's implicit (structural) interface satisfaction has no explicit `implements` syntax to hook a reference onto, unlike TS's `implements` clause. A type satisfying an interface produces no edge; this is a real expressiveness gap relative to TS, not an oversight — modeling it needs whole-program method-set computation, out of scope for extraction (a per-file, non-typechecking pass) |
| Struct/interface embedding | recorded as `RefKind::Extend` (§2) — the type name doubling as the implicit member name (a nameless `field_declaration`) is the signal that distinguishes it from an ordinary `TypeUse` |
| `go:generate` directive comments | not parsed — the directive names a command line to run, not a file reference kndo could statically resolve without executing it |
| `reflect`/`plugin`-based dynamic dispatch | not modeled as a `DynamicUse` wildcard in this slice (§0) — genuinely rare in application code; revisit if dogfooding surfaces false `unused` positives traceable to it |
| Build-tag-gated files (`//go:build linux`, `_linux.go` suffix files) | claimed and extracted like any other `.go` file, unconditionally — kndo analyzes the union of all build configurations, the same "any-feature-is-live" stance RFC 0002 §7's table already states for Rust's `#[cfg]` features; a symbol used only under one build tag is still "used," not dead. Two mutually exclusive files declaring **one name** in one package (gin's `binding.go` under `!nomsgpack` and `binding_nomsgpack.go` under `nomsgpack`, both `func validate`) are not a collision to break either: under the union policy both declarations are live, so core keeps the displaced ones as same-unit *twins* and every reference to that name edges to all of them (`symbol_twins_per_unit` in `graph::assemble`; the single-slot table alone gave one twin all 16 of gin's references and the other a false `unused`). Same treatment covers `#[cfg]` alternatives in Rust |
| Multi-module workspace (`go.work`) | claimed and parsed (RFC 0012 §10, fixtures `go-work-multi-module/` + `go-work-phantom-dep/`): `use` directives → `workspace_members`, sibling-module imports resolve as `WorkspaceMember` (reachability + the `require` contract, which go.work does not waive). A path-renaming `replace` directive remains the one recorded divergence — not modeled |

### 6. Conformance fixtures (shared harness, RFC 0002 §8)

Minimum corpus, each a mini-module with expected findings: multi-file package with a
same-package, no-import cross-file call plus one genuinely dead sibling function (the `unit`
mechanism's own reason for existing, and the exact shape that caught the reachability.rs
propagation gap, §0) · `internal/` package whose exports are correctly *not* promoted to roots,
alongside a sibling non-internal library file whose exports *are* (both directions of §4's
promotion rule in one fixture) · an exported function whose parameter and return type are both
an unexported same-package type — the classic Go unexported-type-in-exported-signature leak,
and the body's own use of that type is deliberately *not* a second finding since only the
signature is a promise (fixture `private-type-leak/`, pinning the category for this adapter).

**No `undeclared`-dependency fixture, deliberately.** Unlike JS/npm (where flat `node_modules`
hoisting lets code import a package that compiles fine but isn't declared — the actual phantom-
dependency problem RFC 0011 §4's check exists for), Go's module system has no equivalent: an
import that doesn't trace to a `require` line simply doesn't build, full stop — `go build`/`go mod
tidy` refuse before kndo would ever see the code. `resolve()` reflects this honestly (§3): an
external specifier with no declared-prefix match resolves `Unresolved`, not a `Dependency` edge
naming an undeclared package the way JS's `classify_bare_specifier` deliberately does — so
`undeclared` (subject `dependency`) has no realistic Go scenario to fire on. Blank imports
(`import _`) counting as dependency usage without a binding, `_test.go` exemption from
`test-only`, and vendored-directory exemption are covered by extraction/manifest unit tests
rather than duplicated here as conformance fixtures — the harness's value is exercising the real
`Engine` end to end, which the two fixtures above already do across both dependency-hygiene and
reachability findings.

### 7. Open questions

Most of this section graduated into **RFC 0012 (Precise Reference Semantics & Visibility)**,
which owns the cross-language design for each — this list now just points there:

1. ~~`go.work` multi-module workspace support~~ — **fixed** (RFC 0012 §10, landed: go.work
   claimed and parsed, sibling-module imports resolve as `WorkspaceMember` with the full
   dependency contract; the path-renaming `replace` divergence stands recorded, not modeled).
2. ~~`RefKind` differentiation (`TypeUse`/`Extend`)~~ — **fixed** (RFC 0012 §5, landed:
   `type_identifier` positions are `TypeUse`, embedded struct/interface fields are `Extend` —
   §2, §5).
3. Package-level `internal-only` boundary awareness → RFC 0012 §6 (the visibility ladder as
   data; Go declares `[Unit "unexported", Package "exported (internal)", Public "exported"]`,
   §1).
4. Content-based origin classification (generated headers) → RFC 0012 §7
   (`FileFacts::detected_origin`).
5. ~~Method-call resolution (`T.Method` declarations vs bare `Method` references)~~ —
   **fixed** (RFC 0012 §3, `member_of` + the visibility-scoped duck-typed fallback; §9's
   `scope_context` further keeps a receiver access from ever capturing a same-named free
   function).
6. ~~External test packages sharing their directory's unit~~ — **fixed** (RFC 0012 §8,
   landed with the `dir#declared-package-name` unit key; §1.1 documents the current rule).
7. ~~Unaliased-import alias guessed from the last path segment~~ — **fixed** (RFC 0012 §9,
   landed: qualified references are structured `scope_context` facts, and the unaliased
   qualifier comes from the resolved target's declared package name — conformance fixture
   `qualified-package-name/` pins the `gopkg.in/yaml.v3`-shaped case).

Still genuinely open, unowned by any RFC: tooling-role detection (§1) — Go has no
ecosystem-wide config-file convention worth pattern-matching; revisit only if dogfooding
surfaces one.

## Java

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M5
**Grammar:** tree-sitter-java

The fourth adapter, and the first with a genuinely two-ecosystem manifest story (Maven and
Gradle) and no dogfood corpus of its own — kndo is written in Rust, so unlike the Rust adapter
(validated by running kndo on itself), Java's precision rests entirely on the conformance
fixtures (§6). Node kinds throughout this doc are pinned against real tree-sitter-java 0.23.5
output (`kndo-adapter-java/src/parsing.rs`'s two `#[ignore]`d ground-truth dumps), not guessed.

### 0. What's structurally different from JS/Go/Rust, and why it matters here

- **Package identity is declared AND path-enforced — a hybrid of Rust's and Go's models.**
  Every file opens with a `package com.foo.bar;` statement (Rust's declared-tree shape), but
  javac also requires the file to live at a matching directory suffix under some source root
  (`com/foo/bar/Widget.java`, closer to Go's directory-is-the-unit convention) — except the
  source root itself (`src/main/java`, `src/test/java`, or something custom) is a **build-tool
  convention, not a language rule**, so a file's directory alone doesn't say what its package
  is without first knowing where the source root starts. The adapter never tries to detect
  source roots: `FileFacts::unit` is set to the **declared** package name (the dotted string
  from the `package` statement) — this sidesteps source-root detection entirely and is exactly
  as precise, since package membership for resolution purposes is what the compiler actually
  uses. A file with no `package` statement (the unnamed/default package) gets `unit: None`
  — its own top-level types resolve same-file only, matching every other adapter's `None`
  behavior; this is also real Java behavior (the default package cannot be referenced by name
  from a *named* package at all, so no cross-file resolution is even possible there).
- **No relative imports; two structurally different import shapes.** `import com.foo.Bar;`
  names one type by its fully-qualified path. `import com.foo.*;` (wildcard) imports **every
  top-level type physically declared by files in that exact package** — not sub-packages, not
  members — which, because a package IS its `unit` in this adapter's model, is *statically
  enumerable* the same way an unqualified same-unit reference already resolves; no
  approximation needed (§3). `import static com.foo.Bar.CONST;` (and its own wildcard form,
  `import static com.foo.Bar.*;`) bring a *member* (field, method, or nested type) into scope
  unqualified — the one import shape that targets a member rather than a type.
- **Four-rung visibility, but only two apply to top-level types — and package-private maps to
  `Unit`, not kndo's `Package`.** A **top-level** class/interface/enum/record can only be
  `public` or package-private (no modifier) — `private` and `protected` are illegal there, so
  the file/type-level ladder is exactly Go's shape: two rungs. **Members** (fields, methods,
  constructors, nested types) get the full four: `private` (class-only — no `File`-narrower
  scope exists in kndo's model, so it widens to `File`, the nearest available bucket: over-
  approximating "any code elsewhere in the file might reach it" is the safe direction, same
  reasoning as Rust's `pub(super)` widening), package-private (**`Unit`**, exact — Java's own
  "default access" scope is the `com.foo`-style declared package, which is exactly what
  `FileFacts::unit` already carries; kndo's `VisibilityScope::Package` is a *different*
  granularity — "same manifest/workspace-member" (RFC 0011's `PackageId`, JS's one-`package.json`
  scope) — and a single Maven/Gradle module routinely holds many Java packages, so mapping
  package-private to `Package` would silently collapse every Java package in one module into
  one bucket; `required_scope`'s own algorithm checks `unit` *before* `PackageId`, exactly
  mirroring Go's choice), `protected` (package **plus subclasses in any other package** — no
  scope in `{File, Unit, Package, Public}` represents "package ∪ my-subclasses-anywhere", so it
  widens to `Public`, the nearest wider bucket that safely covers the cross-package case; this
  trades recall for precision the same direction as every other conservative widening in the
  codebase), `public` (`Public`, exact). One ladder covers both: `[File "private", Unit
  "package-private", Public "protected", Public "public"]` — two rungs sharing a scope is legal
  (RFC 0012 §6's `pub(super)`
  precedent), and `VisibilityLevel` still distinguishes them for the ladder's own label text.
- **No inline test regions.** Unlike Rust's `#[cfg(test)]`, Java test code is always a
  **separate file** — the Maven/Gradle Standard Directory Layout (`src/test/java/**`) is the
  authoritative, universal convention (every build tool, every IDE, every CI config assumes
  it). `FileFacts::test_spans` (contracts §2) is therefore always empty for this adapter —
  worth stating explicitly since it's the mechanism the *previous* adapter (Rust) needed and
  this one structurally doesn't.
- **Dispatch rooting, for the same underlying reason as Rust's trait-impl methods.** JDK-
  invoked contract methods — `equals`/`hashCode`/`toString`/`compareTo` overrides, functional-
  interface implementations passed as method values — are called by collections, string
  concatenation, `Comparator`-consuming APIs, and the like, **never by a named call site in
  user source**. An ordinary interface-implementation method (`r.run()` where `r`'s static
  type is `Runnable`) *is* usually visible to the duck-typed member fallback (RFC 0012 §3) as
  a bare `run` call — but not always (the call may come from framework/JDK code entirely
  outside the graph), and distinguishing "this override's dispatcher is visible in-source" from
  "it isn't" per-method isn't something extraction can determine without a typechecker. So,
  matching Rust's stance exactly: **every `@Override`-annotated method roots
  `Production`/`Probable`**, unconditionally — blanket, safe-direction, no attempt to narrow to
  just the JDK-contract subset.
- **No reliable import→dependency-coordinate mapping — the one real scope cut.** npm's package
  name, Go's module path, and Cargo's crate name all appear *structurally* in the import
  specifier itself. A Java import names a **package** (`com.google.common.collect.*`), and
  nothing about that string says which Maven coordinate declared it (`com.google.guava:guava`
  — groupId, artifactId, and Java package frequently share **none** of the same text). The
  only sound way to resolve this is to actually resolve the classpath (run Maven/Gradle, or
  read the local repository), which kndo — a static source analyzer — structurally never
  does. Consequence (§3, §4): `resolve()` never returns `Resolution::Dependency` for any
  external Java import (so `undeclared` never fires — no realistic scenario, same *outcome* as
  Go's stance for a different root cause), and `PackageNode::resolves_dependency_usage: false`
  makes `dependency_hygiene` skip Java's `unused`/`test-only` dependency verdicts entirely
  (one diagnostic, not a false-positive flood — contracts §2). **`version-skew` is unaffected**
  — it compares declared versions across manifests directly, no usage edge needed, so it's
  fully precise for Java from day one.
- **JPMS (`module-info.java`, Java 9+ module system) is out of scope for v1** — parked, same
  posture as JS's un-followed tsconfig project references. A `module-info.java` file is still
  claimed (language `java`, ordinary `.java` glob) but its root node is `module_declaration`,
  none of the class/interface/enum/record shapes extraction walks for, so it naturally yields
  zero declarations — no special-casing needed in `claim()`. `package-info.java` (package-level
  Javadoc/annotations, no type declarations) behaves the same way, for the same reason. Both
  additionally classify as **Tooling role** (M6): their consumer is javac/javadoc, so their
  "reachability" is healthy by definition rather than an `unused` accusation.

### 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `java` | `**/*.java` (`module-info.java`/`package-info.java` included — see §0's last bullet; both yield zero declarations and classify as Tooling role) |
| Manifests | `**/pom.xml` (Maven), `**/build.gradle` + `**/build.gradle.kts` (Gradle), `**/settings.gradle` + `**/settings.gradle.kts` (Gradle multi-project topology only — §4) |
| Role `test` | `src/test/java/**` (Maven/Gradle Standard Directory Layout — the authoritative signal) OR a bare filename matching Maven Surefire's own default include patterns (`Test*.java`, `*Test.java`, `*Tests.java`, `*TestCase.java`) — a belt-and-suspenders fallback for non-standard layouts (flat scripts, Bazel-built Java) that still follow Surefire's naming convention. An OR, not additive: role is single-valued, and a `src/test/java` file is typically *also* Surefire-named, so the two signals agree far more than they diverge |
| Role `tooling` | not detected in this slice — same stance as Go (§0 there): no ecosystem-wide config-file convention comparable to `webpack.config.js` exists for Java source files (the manifests themselves — `pom.xml`/`build.gradle` — are pure manifest facts, never role-classified as source) |
| Origin `generated` | `@Generated` — the real, standard annotation (`javax.annotation.Generated` pre-JDK9, `javax.annotation.processing.Generated` JDK9+) that annotation processors (Lombok, MapStruct, protobuf, Dagger) actually emit — detected structurally by extraction (any top-level type carrying it), reported via `FileFacts::detected_origin` (RFC 0012 §7). The toolkit's text-marker scan (`@generated`, `Code generated`, `DO NOT EDIT`) still runs as a second, independent signal for generators that skip the annotation |
| Origin `vendored` | `vendor/**`, `third_party/**` (toolkit universal list) — not a real Java convention (dependencies live in `~/.m2`/Gradle's cache, never checked into the tree), so this is essentially inert for Java, included only for consistency |

Build-output directories (`target/**` for Maven, `build/**`/`.gradle/**` for Gradle) need no
adapter-side exclusion at all: discovery already walks only what the project's own
`.gitignore`/`.ignore` admits (same "OUT_DIR" stance Go's doc states for `go build` artifacts),
and every real Java project's default `.gitignore` excludes them.

**`VisibilityLevel`**: `0` (private, widened to `File`), `1` (package-private, `Package`), `2`
(protected, widened to `Public`), `3` (public, `Public`) — computed from the `modifiers` node's
child tokens (`private`/`protected`/`public`; absence of all three = package-private), never
from Java's `default`-keyword-that-doesn't-exist (there is no `default` visibility keyword —
the absence of a modifier IS the level, matching how Go reads capitalization rather than a
keyword). §0 has the full ladder derivation and reasoning.

### 2. Extraction

**Idioms with structural exemptions (M6 FP hunt, junit4 corpus):**
- Constructors declare as `SymbolKind::Constructor` named `<init>`: `new Foo()` references the
  *type*, never the constructor symbol, so the core ties a constructor's liveness to its
  container (a Certain class→constructor References edge in assembly) and `unused` never
  accuses the kind directly — a private utility-class constructor exists precisely to never be
  called, and deleting it would change behavior.
- `serialVersionUID` fields are not declared at all: the JVM reads them reflectively, so a
  declaration would guarantee a false `unused` on every `Serializable` class.
- Interface/annotation members with no modifier are implicitly `public` (JLS §9.4) — extraction
  applies the interface-body default, and interface constants (`constant_declaration`) extract
  like fields.
- `protected` members are **exported** (external subclasses of a published library override
  them) on the Public-scope rung — RFC 0012 §6's ladder table.

**Declarations** — top-level and nested (`member_of`-owned, RFC 0012 §3) alike:
`class_declaration`, `interface_declaration`, `enum_declaration` (+ `enum_constant` as
`EnumMember`, and — unlike Rust/Go — an enum can also declare ordinary methods in an
`enum_body_declarations` block, extracted exactly like a class body), `record_declaration`
(Java 16+; `SymbolKind::Other("record")` — record components (`x`, `y` in `record Point(int x,
int y)`) are **not** extracted as separate declarations, matching the "declarations must be
textual" principle: their accessor methods (`x()`, `y()`) are compiler-synthesized, never
appear as a `method_declaration` node, and calls to them (`point.x()`) simply produce no
reference edge — a documented, safe-direction gap, same class as JS's property-assignment-
callable limitation), `annotation_type_declaration` (`@interface Foo { … }` —
`SymbolKind::Other("annotation")`), `method_declaration` + `constructor_declaration` (member,
name = `Owner.methodName`), `field_declaration` (one `Declaration` per
`variable_declarator` — `int a, b;` is two declarations, matching Go's grouped-`var` stance).
Nested types (`class`/`interface`/`enum`/`record` declared inside a `class_body`/
`interface_body`/`enum_body`) are members (`member_of` = the enclosing type's bare name),
`Outer.Inner` qualified naming, recursing to arbitrary nesting depth. **Not extracted**:
anonymous classes (`new Runnable() { … }` — the `object_creation_expression`'s trailing
`class_body`, when present, is walked for its *references* like any other body, but its
methods contribute no declarations — there is no name to hang a finding on) and local classes
(a class declared inside a method body — rare, same non-declaration stance).

**References**: `method_invocation` (`object`/`name`/`arguments` fields — `object` present +
`identifier` → member/qualified access, `object` absent → bare call), `field_access`
(`object`/`field`), `identifier` reads/writes outside those shapes, `method_reference`
(`Type::method`, `instance::method`, `this::method`, `Type::new` — extracted as a `Call`-kind
reference to the method-name segment, with `scope_context` = the qualifier when it's an
`identifier`/`this`, mirroring the plain-call qualifier shape in §3). `type_identifier` and
`scoped_type_identifier` positions (`extends`/`implements`/`throws`/field & parameter types/
generic bounds/`new` targets) → `TypeUse`; `superclass`'s type and each entry of
`super_interfaces`'/`extends_interfaces`' `type_list` → `Extend`.

A `scoped_type_identifier` emits the LAST segment as the reference name, plus `scope_context`
= the qualifier **when the qualifier names a type** (`Outer` in `Outer.Inner`) and nothing when
it is a package path (`java.util.List`). Both discriminators are needed. Structural:
tree-sitter nests multi-segment paths, so a package path's qualifier is itself a
`scoped_type_identifier` while a nested type's is a bare `type_identifier`. Lexical: a
single-segment qualifier is still ambiguous between a one-word package (`p.Foo`) and an
enclosing type, and only the capitalization convention separates them — guessing wrong on
`p.Foo` sends a name the free-name tables resolve today into the member-only fallback, which
top-level types never reach, and loses the edge.

Dropping the qualifier is not merely lossy, it **mis-binds**. Resolution consults the file's
import bindings before anything else, so a bare `Query` extracted from
`new ParameterHandler.Query<>(…)` in a file that also does `import retrofit2.http.Query` binds
to the annotation: the nested type reads as dead and an unrelated type collects a reference it
never received. Pinned by the `nested-type-qualifier` fixture. Lambda bodies
(`lambda_expression`) are walked like any other expression — their parameter names shadow
outer bindings for extraction's purposes exactly the same safe-direction way locals already do
everywhere else (over-approximating ALIVE, never under).

**Roots (`RawRoot`)**: `public static void main(String[] args)` in **any** class (Java has no
Go-style "must be package main" restriction — any class can be an entry point, and a real
project may have several for different tools) → `RootKind::Production` at `Probable`
(unconditional, mirroring Go/Rust's blanket `main` stance — only the actually-invoked one
truly runs, but which one is a packaging decision `MANIFEST.MF`'s `Main-Class` records, not
something kndo parses in v1). `static_initializer` blocks and instance initializer blocks run
implicitly at class-load/instantiation time — not independently rooted (they're not named
declarations at all; their bodies are walked as part of the *enclosing type's* liveness, which
already requires the type itself to be reachable — a static initializer in an unreachable class
never runs anyway, so no separate root is needed for correctness). **`@Override`-annotated
methods** root `Production`/`Probable` (§0's dispatch rule).

**Suppressions**: `// kndo:allow …` / `/* kndo:allow … */` — same syntax and scope rules as
every other adapter (RFC 0005 §12 is language-neutral; Java's line/block comment syntax is
identical to JS/Go/Rust's).

**Metrics**: cyclomatic complexity +1 per `if_statement`, `for_statement`/`enhanced_for_
statement`, `while_statement`/`do_statement`, `catch_clause`, `case`-arm (switch — one per
label past the first, matching JS's n-way-match rule), `? :` (ternary), `&&`/`||`, and each
Each `lambda_expression` clearing the clone floor becomes its own callable **shape**
(`MetricsSyntax::nested_callable_kinds` — its branches and tokens leave the enclosing shape's
stream, which keeps one `FN` in their place, and `crap`/`duplicate` report it in its own
right). A smaller one stays an expression inside its owner: promoting it would leave both
halves under the floor and cost real clone findings — measured, that was 83 clone participants
on the field corpus. The split's semantics are uniform across adapters; only the node kinds
that trigger it are per-language.

A body that is **only** a value construction (`object_creation_expression`) is not clone-eligible
(`MetricsSyntax::construction_kinds`): normalization erases the field values — the whole
authored content — and keeps the field list the type declaration dictates, so two constructions
of one type match by definition of the type rather than by evidence of copying.

Fingerprints: normalized token stream per shape, same `$n`-renaming scheme as every other
adapter.

**String call arguments → `FileFacts::string_call_args`** (plugin fuel, ecosystem-blind).
Every `method_invocation` whose callee is a plain dotted path — `t`, `res.render`, `a.b.c`,
`this.log` — and whose arguments include a string literal records `(callee as written, first
string literal, span)`. Text blocks count; `""` does not (it names nothing). A receiver with no
written name is skipped rather than given an invented spelling: `build().render("x")`,
`items[0].render("x")` and `(cond ? a : b).render("x")` are real receivers a convention cannot
match on. No resolution and no callee filtering — `a.b` is recorded whether it is a package
qualifier, a static field or a local, because which one it is depends on the classpath and the
syntactic form is what a convention matches anyway; a name-based exclusion list would be the
ecosystem knowledge this layer must not carry. No analysis consumes these: plugins read them
through `GraphView::string_call_sites_in` or the ABI's `call-sites-in`, which is what lets a
framework plugin build on a fact this adapter already parsed instead of re-parsing the grammar.
Same contract the JS/TS adapter implements, so a plugin sees one shape regardless of grammar.

**Dynamic constructs → `DynamicUse`**: none emitted in this slice. `Class.forName(String)`
reflection exists but is rare in application code and — like Go's `reflect`/`plugin` stance
(§5 there) — not modeled; the wildcard-edge machinery stays available if dogfooding on a real
Java corpus later surfaces false `unused` positives traceable to it.

### 3. Imports & resolution

Emitted import kinds:

| Form | Emission |
|------|----------|
| `import com.foo.Bar;` | specifier `com.foo`, binding `[Bar]` — this is the ONE shape whose specifier is the *package*, not the full dotted path (unlike Go/Rust, whose two-step tail rule needs the ambiguity; Java's grammar already hands the package/type split via the `scoped_identifier`'s own nesting, so no guessing is needed) |
| annotations on a declaration | `Declaration::markers`, the names as written and in source order — `@Controller`, `@RequestMapping("/x")` and `@Advice.OnMethodEnter` all contribute. A qualified spelling contributes its last segment too (`OnMethodEnter` beside `Advice.OnMethodEnter`), because either is a legitimate `kndo.toml` entry and the adapter cannot know which the project will pick. FACTS, never verdicts: this adapter has no idea which annotations a framework acts on, and emits every one. Their consumer is `[[externally-invoked]]` — Spring's component scan, JUnit's lifecycle and ByteBuddy's `@Advice` are all invocations no source reference can ever record |
| `import com.foo.*;` | specifier `com.foo`, no bindings, and **two** facts: `opaque_namespace_use: true` — the resolved target file's own declared symbols stay `Possible`-reachable via the same `Wildcard` keep-alive mechanism Go's dot-import and JS's `export *` already use (contracts §2), at the SAME single-representative-file granularity Go's own package resolution already accepts (§3 point 2) — and `module_names_visible: true`, the JLS 7.5.2 type-import-on-demand rule itself: every type in that package is legal here *unqualified*, so the core's bare-name fallback consults that unit's table at Certain. Only top-level declarations live in a unit's name table, so this brings in exactly what the JLS says it does — types, not static members |
| `import static com.foo.Bar.CONST;` | specifier `com.foo::Bar` (§3.1), binding `[CONST]`, `type_only: false` |
| `import static com.foo.Bar.*;` | specifier `com.foo::Bar`, `opaque_namespace_use: true` — every static member of `Bar` in scope unqualified |
| `import com.foo.Bar;` used only in `extends`/`implements`/type positions | same as row 1 — Java has no `import type` keyword; whether a binding is type-only isn't visible at the import site, only at each reference site (`RefKind::TypeUse` there already carries that distinction) |

**§3.1 — static imports target a member, not a file.** `Resolution` (contracts §2) only ever
names a file or a dependency, never a member directly — so a static import's specifier encodes
*both* the class's package (for file resolution) and the class's own bare name (for the
member lookup that happens after), joined by a sentinel (`::`) the resolver splits back apart:
`com.foo::Bar` resolves the `com.foo` half exactly like row 1 (to the file declaring `Bar`),
and the importing file's binding table then looks up `CONST` as a **member** of `Bar` in that
file's declaration table — the same "resolve the file, then look up the member inside it"
two-step shape Go's package-qualified access already uses, just triggered by an import instead
of a body reference.

All Java imports are `ImportKind::Package` (there is no relative-path import shape — §0) and
`Confidence::Certain` (no bundler/build-tool ambiguity — an import that doesn't resolve is
either genuinely external or a compile error, never a maybe).

**Resolution algorithm** (the adapter's `resolve`):

1. **Same-package (no import needed).** Handled entirely by the core's existing `unit`-based
   fallback (contracts §2) — never touches this adapter's `resolve()` at all, exactly like Go's
   same-package resolution.
2. **`com.foo` / `com.foo::Bar`.** Split on `::` first if present (static-import shape, §3.1).
   Look up the package-half against the known-units index (every claimed file's declared
   `unit`, the same index the core already builds for `FileFacts::unit` fallback resolution) —
   a hit resolves `Resolution::File` at the package's **first file in path order** (a Java
   import names a package, and — same reasoning as Go's directory pick — any one file with
   that `unit` makes every file in it reachable through the same-unit resolution fallback, so
   which one is nominal doesn't affect correctness, only which file's `ImportsFile` edge is
   literal).
3. **`java.` / `javax.` prefix.** `Resolution::Stdlib`, unconditionally — a real, structural,
   version-stable namespace reservation (no third-party artifact may declare a `java.*`/
   `javax.*` package; the JDK itself enforces this), so no generated list is needed the way
   Go's arbitrary-string module paths require one.
4. **Anything else.** `Resolution::Unresolved` — deliberately **not** `Resolution::Dependency`
   (§0's last bullet: no reliable import→coordinate mapping exists, so guessing here would
   flood `undeclared` with false positives for the overwhelming majority of third-party code).
   This is the one point where this adapter's `resolve()` shape genuinely diverges from every
   other adapter's "undeclared fallback" pattern — documented here, not silently absent.

### 4. Manifests & packages (RFC 0011)

Two build ecosystems, handled at different fidelities — **Maven fully structured** (via
`roxmltree`, an XML tree parser — the `pom.xml` equivalent of `toml`'s role for `Cargo.toml`),
**Gradle best-effort** (line-oriented scanning, the same "no more machinery than can be done
honestly" stance `go.mod`'s hand-rolled parser takes, but Gradle's actual grammar — Groovy or
Kotlin — is a real programming language with arbitrary expressions, so this is a **narrower**
best-effort than go.mod's: only the common, literal-string forms are recognized, anything
computed is silently invisible, not misparsed).

**Maven (`pom.xml`)**:

| `package.json` concept | Maven equivalent | notes |
|---|---|---|
| `name` | `groupId:artifactId` (from `<groupId>`/`<artifactId>`, `<groupId>` falling back to `<parent><groupId>` when omitted — the common parent-inherits pattern) | the two-part coordinate IS the module's cross-module identity; `<version>` similarly falls back to `<parent><version>` |
| a dependency's `<version>` | `ManifestDependency::version_req`, with `<properties>` substituted | **absent is `None`**, not `"*"` — a `<dependency>` with no `<version>` is the BOM-managed shape, and `dependencyManagement` in a parent POM is out of reach by construction (kndo never resolves the classpath) |
| `private: true` | `<packaging>` ≠ `jar` (default) | Maven has no `private` flag at all — every `jar`-packaging module is nominally publishable by omission, matching npm's own asymmetric default; `pom` (aggregator, no code) and `war` (deployable app, not an importable dependency) are the two packagings this adapter treats as `private: true` |
| `workspaces` | `<modules>`/`<module>` (aggregator POM) | `ManifestFacts::workspace_members`, one entry per `<module>` text — RFC 0011 §3 |
| dependency scopes | `<dependency><scope>` | `compile`/omitted → `Prod`; `test` → `Dev`; `provided` → `Peer` (supplied by the runtime environment, not bundled — same "contract with the consumer" semantics as npm peerDependencies); `runtime` → `Prod` (a documented approximation — genuinely used at runtime, just not compile-visible; kndo's taxonomy has no runtime-only scope); `system` → `Prod` (rare, deprecated); entries inside `<dependencyManagement>` are version pins for *children*, not real dependencies of *this* module — never collected |
| `main`/`exports` | *(no equivalent)* | see §2's blanket `main()`-method stance; no manifest-level entry-class extraction attempted (a `<mainClass>` plugin config exists but resolving a dotted class name to a file needs a known-files-by-package lookup `ResolveCtx` doesn't cheaply expose today — parked, §7) |
| `scripts` | *(no equivalent)* | no script-runner convention in Maven itself |

**Gradle (`build.gradle`/`.kts`)**: line-scanned for the `dependencies { … }` block's literal-
string entries only (version catalogs' `libs.foo` references are invisible — not misparsed,
simply not seen, same honesty as go.mod's `exclude`-globs-not-expanded stance).

A coordinate is split **by segment count, never by the last colon**. Three segments
(`com.foo:bar:1.0`) is `name = com.foo:bar`, `version = 1.0`. **Two segments
(`org.springframework.boot:spring-boot-starter-actuator`) is a complete coordinate whose
version an imported BOM supplies — the name is the whole thing and there is no version.**
Splitting on the last colon read that as version `spring-boot-starter-actuator` of a
dependency named `org.springframework.boot`, which is why `version-skew` reported *artifact
ids* as diverging versions on every JVM repository the field audit covered
(`internal/detection-gaps.md` §17).

`$var` / `${var}` version placeholders resolve against the manifest's own pool — Gradle's
`val`/`def`/`var x = "1.2.3"`, Maven's `<properties>`. What the file itself cannot answer
(`gradle.properties`, a version catalog, a parent POM's properties) stays **unknown**, never
the literal: koin declares `val jmhVersion = "1.36"` two lines above its use, and
kotlinx.coroutines spells one `gradle.properties` key `$junit5Version` in one module and
`$junit5_version` in another — comparing those strings was pure noise. Configuration → scope: `implementation`/`api`/`compile` (legacy) → `Prod`;
`testImplementation`/`testCompile`(legacy)/`testRuntimeOnly` → `Dev`; `compileOnly` → `Peer`
(provided-equivalent); `runtimeOnly`/`runtime`(legacy) → `Prod`; `annotationProcessor`/
`testAnnotationProcessor` → `Build` (a build-time-only tool, Cargo's build-dependencies
analogue — Lombok, MapStruct). `group`/`project name` (from `settings.gradle`'s own `rootProject.
name`/`include(...)`, defaulting to the containing directory's name per Gradle's own
convention when absent) give the module identity. The `application` plugin block
(`apply plugin: 'application'` or the `plugins { application }` DSL form) marks `private:
true`; its absence defaults to `private: false` (library mode), same asymmetric-default
reasoning as Maven's packaging check. **`settings.gradle`/`.kts`** is claimed purely for
`include(':sub-a')`/`include 'sub-a'` line-scanning → `workspace_members` (colon-to-slash
path convention; nested `:a:b` → `a/b`), the Gradle analogue of Maven's `<modules>`.

**Root promotion (RFC 0011 §5), the mechanism that diverges from both Go and Rust.** Go
blanket-emits `RawRoot`s per exported declaration directly in extraction, because its per-file
promotion signal (`internal/`) is itself extraction-visible. Rust rides the existing
`library_root_files` mechanism from a *single* manifest-declared entry file (`lib.rs`) plus the
library-surface fixpoint (phase 2.7) for its `pub mod` re-export chains. **Java has neither**:
"is this module publishable" is a manifest-level fact extraction can't see on its own (unlike
Go), and a Java library has no single entry file the way Rust's `lib.rs` does — every public
class in the module is independently part of the API. The adapter therefore emits one
`ManifestRoot{Production, target: <file>, Certain}` **per non-test `.java` file under the
module's source root** for every **publishable**
module, reusing `graph::assemble`'s existing per-file declaration-promotion path
(`library_root_files`) with **zero new core mechanism**: each of those files already being a
manifest-declared production root makes every `public` declaration in it promote automatically,
exactly as JS's `main`-file promotion already works — just applied to every source file instead
of one. **Which directory that is comes from the pom when the pom says so.** Maven's
`<build><sourceDirectory>` wins over the Standard Directory Layout default, replacing it rather
than adding to it: a module that declares where its code lives is not also keeping `src/main/java`.
`${basedir}` and the pom's own `<properties>` are interpolated; anything else unresolved
(`${project.build.directory}`), an absolute path, or one escaping the module falls back to the
convention rather than guessing at a path that depends on a build kndo never runs. This is not
cosmetic — guava declares `<sourceDirectory>src</sourceDirectory>` with tests in a sibling
`test`, and against the hardcoded default its modules promoted *nothing*, so every public class
in a publishable library read as `unused`. Two remaining shapes are §7.2: a declaration
**inherited from a parent pom** (guava's own case — the adapter sees one manifest's text at a
time and `ResolveCtx` exposes paths, not contents), and Gradle's `sourceSets`.

`ManifestFacts.declares_surface` stays `false` (no `exports`-map equivalent — same
`deep-import`-stays-closed reasoning as Go).

### 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| Records (`record Point(int x, int y) {}`) | the type is a declaration; components/synthesized accessors are not (§2) — calls to `point.x()` produce no reference edge, a documented recall gap |
| Anonymous classes (`new Runnable() { … }`) | body walked for references (whatever it calls stays live); contributes no declaration of its own — no finding can ever target it |
| Method references (`Type::method`, `this::method`) | extracted as a `Call` reference with `scope_context` when the qualifier is a plain identifier/`this` — `Type::new` (constructor reference) treated the same, referencing `Type` itself |
| Static nested/inner classes, local classes | nested types are members (`member_of`); local classes (declared inside a method body) are not extracted at all, same non-declaration stance as anonymous classes |
| `equals`/`hashCode`/`toString`/`compareTo` overrides | covered by the blanket `@Override` dispatch root (§0, §2) — no special-casing beyond the annotation check |
| Generic bounds (`<T extends Comparable<T>>`) | the bound's type names are ordinary `TypeUse` references, ordering is irrelevant since phase 3a builds the whole file's table before phase 3b resolves anything (same forward-reference safety as every other adapter) |
| Checked/unchecked exceptions in `throws` | each named type is a `TypeUse` reference |
| Annotation processors / Lombok-generated members (`@Data`, `@Getter`, …) | not modeled — a generated `getFoo()` method has no textual declaration for extraction to see (same class of gap as record accessors); a *use* of it (`obj.getFoo()`) is a normal member-fallback reference that simply never resolves, harmlessly |
| Text blocks (`"""…"""`, Java 15+), switch expressions (`yield`), pattern matching (`instanceof Foo f`) | parsed by tree-sitter-java's grammar as ordinary expression/statement shapes; no adapter-specific handling needed — their contained references/type positions fall through the same generic walkers as everything else |
| `module-info.java` (JPMS) | claimed, yields zero declarations (§0's last bullet) — the `exports`/`requires`/`opens` module directives are not parsed; a real, parked gap (§7) |
| Multi-Release JAR source-set variants (`src/main/java`, `src/main/javaNN`) | not directory-special-cased — a `javaNN` source root is claimed like any other `.java` file (§1), so two variants declaring the same package and the same class/method name become same-unit *symbol twins*, the identical mechanism Go's `//go:build` alternates and Kotlin's `expect`/`actual` already use (RFC 0012 §8): a same-package caller's reference edges to every twin, so neither variant reads `unused`. The call resolves through the duck-typed member fallback at `Possible` confidence rather than a direct type reference, so both variants surface `internal-only` at that tier instead of a clean pass — pinned by the `multi-release-variants` fixture (§6) |
| Non-standard source roots (no `src/main/java` — flat layouts, Bazel) | `unit` (declared package) still resolves correctly regardless of directory shape (§0); role-by-path (`src/test/java`) degrades to the Surefire-filename fallback (§1); manifest root-promotion (§4) reads a pom's own `<sourceDirectory>` and otherwise assumes the Standard Directory Layout, undercounting on a layout that is neither declared here nor conventional (inherited declarations and Gradle `sourceSets`, §7.2) — documented, not silently wrong (fewer roots promoted, never phantom ones) |

### 6. Conformance fixtures (shared harness, RFC 0002 §8)

Six fixtures, each a real Maven/Gradle module tree run through the real `Engine` (no mock):

- **`dead-code-same-package`** — a private (`packaging=war`) module: `Main.main` calls
  `Helper.live()` with no import (same-package `unit` resolution) while `Helper.dead()` is
  never called — `dead` reads `unused`, `live` doesn't. (`Main` itself also reads `unused`: its
  class *name* is never referenced by anything, only invoked by the JVM — the same "a symbol's
  name and its callable body are different liveness questions" shape §2's `main()`-rooting
  note already flags; not a bug, a real fact about the code.)
- **`dispatch-and-cross-package`** — a private module: `Impl implements Greeter`, `Impl.go()`
  is `@Override`-annotated and never named-called (only reached via the interface-typed
  variable's *type*, `new Impl()`) — stays alive only through the dispatch-rooting rule (§0);
  `Greeter.go` (the interface's own abstract declaration) legitimately reads `unused` — nothing
  ever calls `g.go()` through the interface type in this fixture, only through dispatch on the
  concrete `Impl`. `Runner` imports `com.util.*` (wildcard) and calls `Helper.assist()`
  qualified — proves cross-*Java*-package visibility computation is correct (this fixture is
  what caught the `Unit`-vs-`Package` ladder bug below).
- **`visibility-ladder-and-nested-members`** — one package, all four ladder rungs exercised on
  a single class's methods plus a nested class's members (`member_of` two levels deep,
  `Inner.go`/`Inner.helper`): `private`/already-tightest `package-private` methods produce no
  finding, `protected`/`public` methods used only within their own `unit` both correctly
  downgrade-recommend to `package-private`, and the outer class itself downgrades too (never
  referenced from outside its own package in this fixture) — six `internal-only` findings in
  one file, each pinned to a distinct rung interaction.
- **`nested-type-qualifier`** — the two-bug shape a field audit on retrofit surfaced, in the
  smallest form that reproduces both, and it expects **zero findings**. `Handler.Query` is a
  nested type (a member, so absent from the file's bare-name table) whose constructor calls a
  private static on the enclosing class; `Main` constructs it as `new Handler.Query(…)` while
  also doing `import com.foo.http.Query`, binding that bare name to an unrelated annotation.
  Before the fixes both `Handler.Query` and `Handler.checkArgument` read `unused`: the
  constructor found no container to inherit liveness from, and the reference that should have
  named the nested type was captured by the import binding instead. Neither of the two existing
  nested-type fixtures caught it — `visibility-ladder-and-nested-members` has no constructor
  and no name collision — which is why it exists as its own case.
- **`maven-gradle-dependency-skip`** — a Maven module (`pom.xml`, dependency `com.other:lib`
  `1.0`) beside a Gradle module (`build.gradle`, same coordinate at `2.0`): `version-skew`
  fires (pure manifest-fact comparison, unaffected by `resolves_dependency_usage`); neither
  module's genuinely-unused `com.other:lib` declaration produces an `unused`/`test-only`
  dependency finding — verified end-to-end through the real engine (plus the diagnostic
  message), not just the unit-level `find_dependency_hygiene` test in `dependency_hygiene.rs`.
- **`multi-release-variants`** — a Multi-Release JAR layout: `retro.DefaultMethodSupport.invoke`
  is declared twice, once under `src/main/java` and once under `src/main/java16`, both in the
  same package. `Reflection.call`'s same-package call reaches both declarations as same-unit
  symbol twins rather than reading either one `unused`; both correctly surface `internal-only`
  instead, at `Possible` confidence — the mechanism and stance are §5's Multi-Release JAR row.

**A gap the fixtures surfaced, and how it closed.** A *wildcard* type import
(`import com.foo.*;`) used not to bind the target package's type names the way a plain
`import com.foo.Bar;` binds `Bar`, so a **bare, unqualified** reference to a wildcard-imported
type (`Helper` as a bare type, not through `Helper.member()`) did not resolve at all; the
`dispatch-and-cross-package` fixture passed only because its usage is the qualified-access
shape, which the duck-typed member fallback covers regardless of whether the type name itself
resolved. The core-side, post-extraction enumeration this was thought to need already existed:
`symbol_by_name_per_unit` is exactly "every top-level declaration of a unit, by name", and
`unit` for Java is the declared package. All the adapter was missing was
`module_names_visible` — the contract field Swift's `import SomeKit` already used to say the
same thing. Kotlin's `import p.*` had the identical gap and closed with the identical one-line
fact.

**No `undeclared`-dependency fixture, deliberately** — same shape as Go's own stance (§6 there),
for the different reason §0/§3 document: Java's `resolve()` never emits `Resolution::Dependency`
for an external import, so there is no code path that could produce one.

### 7. Open questions

1. `<mainClass>`/`exec.mainClass` manifest-declared entry points, resolved to a concrete file —
   parked; needs a known-files-by-declared-package-and-class lookup `ResolveCtx` doesn't cheaply
   expose today (§4).
2. ~~Custom source roots inherited from a parent pom~~ — **resolved** (§7.3). What remains of
   this item is Gradle's `sourceSets.main.java.srcDirs`, and it is the harder half by a wide
   margin: Gradle's build script is a *program*, not a declaration, and the line-scan cannot
   execute it.
3. JPMS (`module-info.java`'s `exports`/`requires`/`opens`) — real Java 9+ module boundaries
   with their own visibility semantics, entirely unmodeled (§0, §5).
4. Gradle version catalogs (`libs.versions.toml` + `libs.foo` references in `build.gradle.kts`)
   — the coordinate lives in a *different* file than the `dependencies {}` block that uses it;
   the line-scan (§4) doesn't cross that boundary. A real, common pattern in modern Gradle
   projects worth a dedicated pass if Gradle precision turns out to matter more than the
   line-scan delivers.
5. Any package-prefix→Maven-coordinate mapping for dependency-usage resolution (§0's last
   bullet) — deliberately not attempted (a hand-maintained or generated database, `kndo-stdlib`-
   style, is the only sound path here; parked pending real signal that the skip's precision
   cost is worth the ongoing-maintenance cost of such a database).
6. ~~Wildcard type import name resolution~~ — **resolved.** `import com.foo.*;` now declares
   `module_names_visible`, and the core's bare-name fallback consults the target unit's table
   (§3, §5). The post-extraction enumeration this question assumed was missing turned out to be
   `symbol_by_name_per_unit`, which already existed.


### 7.3 Inherited source directories — resolved

**The gap.** A pom's own `<sourceDirectory>` was read; one declared by an **ancestor** was not.
Maven's inheritance makes that the common shape rather than a corner: guava declares
`<sourceDirectory>src</sourceDirectory>` exactly once, in `guava-parent`, and all ten modules
inherit it. Against the hardcoded `src/main/java` — a directory guava does not have — kndo found
**zero production roots in the entire repository** and read 88% of its findings off that.

**Why it looked like a contract change and was not.** §7.2 recorded this as blocked: the adapter
is handed one manifest's text at a time and `ResolveCtx` exposed paths, not contents, so
resolving it seemed to need the assembly-side `ManifestDependency::inherited` pattern — a
`ManifestFacts` field, a shared pool, and moving root promotion off the adapter. That plan
existed to protect a caching invariant, and the invariant turned out not to be at risk:

- **Manifest extraction is not cached.** `graph::assemble` re-runs it on every assembly, reading
  each manifest fresh (`discovered.read`), so no entry can go stale behind an ancestor's edit —
  which is precisely the hazard a content channel on `extract` *would* create.
- **The incremental patch refuses on any changed manifest** (`graph::patch`: "manifests feed
  global inputs — full rebuild"), so the patch path cannot observe a partially-updated chain.

So the whole thing is one optional capability on the manifest-extraction `ResolveCtx`:
`read_manifest`. The core offers "you may read a manifest"; every Maven rule below stays in the
adapter, and root promotion never moves.

**What the adapter does with it.** From a pom that declares no `<sourceDirectory>`, walk
`<parent>` upward:

- `<relativePath>` decides where to look, defaulting to `../pom.xml`; a value naming a directory
  means that directory's `pom.xml`.
- An **empty** `<relativePath/>` is Maven's explicit "resolve from the repository", and must not
  fall back to the default. (`xml_child_text` cannot see the difference — an empty element has
  no text node — so presence and text are read separately.)
- The pom found there is accepted only if its `groupId:artifactId` is what the child declared.
  Maven checks this and falls back to the repository otherwise; so does kndo, which never
  fetches. Not hypothetical: guava's `futures/*` modules name `guava-parent` with no
  `<relativePath>`, no `futures/pom.xml` beside them, and versions (`26.0-android`) the in-repo
  parent has not carried for years. `<version>` is deliberately *not* compared — kndo is
  locating a source directory, not building, and a version-skewed but coordinate-matching parent
  on disk is still the file the author edits.
- Each hop interpolates against **that** pom's own `<properties>`, which is Maven's rule: the
  declaration and the properties it names live in one document.
- The value is joined onto the **child's** directory, also Maven's rule — and the reason one
  declaration in a parent serves ten modules with ten different source trees.
- Bounded at 16 hops with cycle detection: a `<relativePath>` loop is malformed input, not a
  shape to follow.

**Measured** — release binaries before/after, `--no-cache`, diffed by `(category, path, symbol)`:

| repo | before | after | removed | added |
|---|---|---|---|---|
| **guava** | 28802 | 19793 | **11444** | 2435 |
| retrofit | 304 | 304 | 0 | 0 |
| spring-petclinic | 44 | 44 | 0 | 0 |
| Exposed | 783 | 783 | 0 | 0 |
| kotlinx.coroutines | 2576 | 2576 | 0 | 0 |

Net **−9,009** unique findings on guava, and **no change anywhere else** — the other four either
follow the convention or declare their own. The 2,435 additions are a category shift, not new
noise: 2,419 of them (99.3%) land in files that were previously reported `unused`, and 2,344 are
`untested` — a file that becomes production-reachable stops being "dead, nothing more to say"
and starts being a testable subject. The removals are 6,410 `internal-only`, 3,078 `unused` and
1,956 `test-only`.

`facts_schema_version` for the Java adapter, not `ENTRY_FORMAT_VERSION` and not
`GRAPH_SCHEMA_VERSION`: one adapter changed what it emits (new roots), which is the case
CLAUDE.md names for that knob literally.

## JavaScript / TypeScript

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M1
**Grammar:** tree-sitter-typescript (TS + TSX variants)

The first adapter, and deliberately the hardest: JS/TS has two module systems, three ways to
export the same thing, dynamic everything, and the largest AI-generated-code surface. If the
contracts survive this document unchanged, they are probably right.

### 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `js-ts` | `.ts .tsx .js .jsx .mjs .cjs .mts .cts .d.ts` |
| Manifests | `package.json`, `pnpm-workspace.yaml` (topology only), lockfiles are **not** claimed |
| Role `test` | `*.test.*`, `*.spec.*`, `__tests__/**`, `__mocks__/**`, `test/**`, `tests/**` — the last two are mocha's and node:test's default lookup directories, where express-style repos keep plain-named specs (M6) |
| Role `tooling` | `*.config.{js,ts,mjs,cjs}` (webpack/vite/jest/eslint/…), `.storybook/**`, `scripts/**` when referenced from package.json `scripts` |
| Origin `generated` | first-comment markers, implemented via `FileFacts::detected_origin` (RFC 0012 §7): extraction scans the first 64 lines for `@generated`, `AUTO-GENERATED`, and `Code generated by` banners (GraphQL/protobuf/relay emitters) with the toolkit's `ContentMarkers` scanner. The `*.d.ts`-sibling-of-a-same-name-`.ts` rule remains a documented gap — it needs cross-file knowledge neither claim (one path) nor extract (one content) has (RFC 0012 §7's stated non-goal) |
| Origin `vendored` | `vendor/**`, `third_party/**` |

`.d.ts` files contribute *declarations only* (no runtime edges); an ambient `declare global` or
`declare module "x"` block marks its symbols externally-consumed (they exist to be seen by the
outside).

### 2. Extraction

**Declarations** — functions, classes (+ methods, fields, getters/setters as members),
interfaces, type aliases, enums (+ members), top-level `const`/`let`/`var`, namespaces.
Anonymous default exports (`export default () => {}`) declare a synthetic symbol named
`default` (symbol path: `file#default`). A **named** default export
(`export default function mergeConfig() {}`) keeps its own name and additionally records
`FileFacts::default_export_alias` — a consumer writes `import mergeConfig from './x.js'`,
whose binding asks the target for `default`, so without the alias that lookup finds nothing
and the function reads `unused` however many files call it. The CJS half of this contract
(`module.exports = local`) always recorded it; ESM's named default did not, which cost axios
four findings from one miss (`mergeConfig`, `bind`, `shouldBypassProxy`, and the file-local
const only `shouldBypassProxy` read). Pinned by the `named-default-export` fixture, and
`export function other() {}` must NOT record one — the `default` keyword token is the
discriminator. Property-assignment callables
(`obj.method = function () {}`, the pre-class prototype-extension idiom) are **not**
declarations and are not extracted — a documented scope limit, not an oversight: it bounds
recall for symbol-level analyses (`crap`, structural `duplicate`, symbol reachability) on
prototype-style codebases (the M4 express audit's `res.send` is the canonical miss), and
lifting it means designing the symbol identity (`res.send` is a member of what?) — RFC 0012
§3's member model is the frame if this scope ever widens.

**Export surface** — `export` named/default, `export { a as b }`, re-exports
(`export * from`, `export { x } from`), CJS (`module.exports = …`, `exports.foo = …`).
`module.exports = { a, b }` with identifier shorthand exports those symbols `certain`;
computed/spread members demote the file's export surface to `probable`.

**References** — identifier uses with scope context, member accesses, `extends`/`implements`
(→ `RefKind::Extend/Implement`), type positions (→ `TypeUse`), **JSX element names** —
`<Button/>` is a `certain` reference to `Button`, which is what keeps React components alive
without any framework plugin (framework *roots* remain plugin territory, RFC 0003).

**String call arguments → `FileFacts::string_call_args`** (plugin fuel, ecosystem-blind).
Every call whose callee is a plain identifier or member chain — `t`, `res.render`, `a.b.c` —
and whose arguments include a string literal records `(callee as written, first string literal,
span)`. Direct literals only, never computed strings (determinism over coverage); a callee that
is not a written path (a call result, a subscript, an IIFE) is skipped rather than given an
invented spelling. No callee filtering: an exclusion list by name would be exactly the
ecosystem knowledge this layer must not carry. No analysis consumes these — plugins read them
through `GraphView::string_call_sites_in` or the ABI's `call-sites-in`, which is how a route
convention is built on a fact the adapter already parsed instead of re-parsing source through
the content channel. The Java adapter implements the same contract.

**Dynamic constructs → `DynamicUse`** (wildcard edges, RFC 0005 §1 expansion):

| Construct | Effect |
|-----------|--------|
| `import(expr)` / `require(expr)`, non-literal | wildcard, scope narrowed by any static string prefix (`./locales/${x}` → that directory) |
| `eval`, `new Function` | wildcard, file-wide |
| computed member access on module namespace (`ns[key]`) | wildcard over that namespace's exports |
| string-keyed registries (`obj["handler"]` patterns) | plain `possible` reference when the literal resolves; wildcard otherwise |

**Metrics** — cyclomatic complexity: +1 per `if / for / while / do / case / catch / && / || /
?? / ?: / optional-chain-call`. Fingerprints: normalized token stream per function body
(consistent `$n` renaming per RFC 0005 §6; template-literal text canonicalized, string/number
literals bucketed).

Each `arrow_function`, `function_expression` or `generator_function` clearing the clone floor
becomes its own callable **shape** (`MetricsSyntax::nested_callable_kinds`): its branches and
tokens leave the enclosing shape's stream, which keeps one `FN` in their place, and
`crap`/`duplicate` report it in its own right. A smaller one stays an expression inside its
owner — promoting it would leave both halves under the floor and cost real clone findings
(measured: 83 clone participants on the field corpus). A bare `function` is deliberately NOT in
that list: in tree-sitter-typescript that name belongs to the unnamed keyword token, and only
`function_expression` is the node. The split's semantics are uniform across adapters; only the
kinds that trigger it are per-language.

A body that is **only** a value construction (`object` and `new_expression`) is not clone-eligible
(`MetricsSyntax::construction_kinds`): normalization erases the field values — the whole
authored content — and keeps the field list the type declaration dictates, so two constructions
of one type match by definition of the type rather than by evidence of copying.

**Suppressions** — `// kndo:allow …`, `/* kndo:allow … */`, JSX `{/* kndo:allow … */}`.

### 3. Imports & resolution

**`Missing` vs `Unresolved` (contracts §2.1).** A relative specifier (`./x`, `/x`) that
survives the whole candidate ladder — the explicit path, the extension appends, `.d.ts`, the
directory `index.*` — resolves to `Resolution::Missing`: every spelling the language allows was
tried, so the path names no file and the `unresolved` analysis reports it at severity `error`.
A `#`-prefixed self-reference stays `Unresolved`: it needs `package.json`'s `imports` map,
which this adapter does not parse, so kndo has no answer rather than an accusation.

Emitted import kinds and their confidence:

| Form | Edge | Confidence |
|------|------|-----------|
| ESM static (`import x from "spec"`) | file/dep | certain |
| `import type` / type-only re-export | file/dep, edges are `TypeUse` | certain; counts as usage for `@types/*`-mapped deps, and for the runtime dep only via its types |
| CJS `require("literal")` | file/dep | certain |
| `import("literal")` | file/dep | probable (bundler code-split semantics) |
| side-effect (`import "./polyfill"`) | file, `side_effect_only` | certain |
| `require.resolve`, `new URL("./x", import.meta.url)` | file | probable |

**Resolution algorithm** (the adapter's `resolve`): Node's algorithm, both module systems —
relative/absolute paths with extension resolution order (explicit > `.ts .tsx .mts .cts` >
`.js .jsx .mjs .cjs` > `.d.ts` > directory `index.*`), self-referencing package `imports`
(`#internal/*`), package `exports` maps (conditions: `types`, `import`, `require`, `default` —
evaluated in that order), `main`/`module`/`types` fallbacks, workspace packages (RFC 0011:
`workspace:*` and name matches against sibling manifests resolve to internal files through the
sibling's own `exports`), pnpm symlink layouts resolved to real paths before ownership checks.
**`tsconfig` `baseUrl`/`paths` are not read.** `tsconfig.json` is not one of the adapter's
`manifest_globs`, and there is no `compilerOptions`/`baseUrl`/`paths` parsing anywhere in
`src/`. A non-relative specifier that only resolves through such a path alias gets the same
bare-specifier treatment as any other unrecognized package name — it most often ends up
`undeclared` rather than resolved to the intended in-repo file.
Specifiers that resolve into `node_modules` yield `Dependency` targets via the subpath→package
mapping (`lodash/fp` → `lodash`, `@scope/pkg/sub` → `@scope/pkg`). **Builtins** → `Stdlib` via
the shared `kndo-stdlib v1` mechanism (RFC 0002 §6; toolkit owns format, loader, and the
four-step precedence): the `node:` prefix is the *structural* signal (covers every post-v18
builtin forever — Node's own policy makes new builtins prefix-only), and the frozen legacy
bare-name set is generated data (`cargo xtask gen-stdlib js-ts`, sourced from
`module.builtinModules` — regenerated, never hand-edited). A manifest-declared dependency
shadowing a builtin name
(userland `punycode`) resolves as the dependency, not the builtin — precedence rule 2. Asset specifiers (`.css .svg .png .json …`) resolve as cross-language
file edges when the file exists (RFC 0002 §4) — the CSS/JSON adapters claim the targets.

### 4. Manifests & packages (RFC 0011)

From `package.json`: name, `private`, `workspaces` globs (+ `pnpm-workspace.yaml` packages),
dependency scopes mapped `dependencies→prod`, `devDependencies→dev`, `peerDependencies→peer`,
`optionalDependencies→optional`; entry points (`main`, `module`, `exports`, `browser`, `bin`,
`types`) both as resolution inputs and as **roots**: `bin` targets and the export surface of
non-`private` packages are production roots (library mode); `scripts` file references become
tooling roots.

**`browser`, in both of its spellings.** As a string it is an alternate `main`
(`"./dist/browser.js"`), read at `certain`. As an OBJECT it is an alias map a bundler applies —
axios ships `{"./lib/platform/node/index.js": "./lib/platform/browser/index.js"}` — and its
*values* are the files substituted in. Those values have no incoming import anywhere: nothing
in the source names them, the bundler rewrites the specifier, so axios's entire
`lib/platform/browser/` tree read `unused` while shipping in every browser build. The values
become entries and (library mode) roots at `probable`, the same tier `exports` leaves get for
the same reason — a conditional build alternate is not unconditionally "the" entry. Keys are
skipped: they are the node-side files, already reachable through ordinary imports. A `false`
value ("stub this module out") names no file.

**Node's implicit `index`.** A package declaring neither `main` nor `exports` resolves to
`index.js` in its own directory — express declares neither, and without the fallback its
`index.js` plus the whole of `lib/` read `unused`/`test-only`. Synthesized only when nothing
else was declared (`browser` counts): a package that has stated a surface has stated it, and a
stray `index.js` beside it is not silently part of that promise. `.d.ts` is excluded
explicitly — the candidate ladder offers `index.d.ts` and `is_source_entry` does not catch it
(its final extension is `ts`), but a declaration file carries no runtime edge and Node never
resolves an entry to one.

**Visibility ladder** (for `internal-only`/`private-type-leak`, RFC 0005 §7):
module-local < exported < **package-surface** (reachable through the package's `exports` map).
An exported symbol not reachable through `exports` is *exported but package-internal* — the
ladder makes "exported yet not part of the public surface" expressible, which is exactly where
monorepo over-exposure hides. Declared as descriptor data (RFC 0012 §6):
`[File "module-local", Package "exported", Public "package surface"]` — note `exported` maps to
`Package` scope, not `Public`: an ESM export is importable anywhere *within its own package's
world*, and the wider promise is the exports-map surface. Extraction emits levels 0/1 today;
level 2 is an assembly-time promotion that lands with surface-awareness — the rung is declared
now so the ladder is complete the day a symbol carries it.

### 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| Barrel files (`index.ts` re-export fans) | resolved through, transparently; re-exported symbols alive only if some consumer imports them through *any* path — implemented for named re-exports (`export {a, b as c} from './x'`, `export type {…} from`) and CJS barrels (`module.exports = require('./x')` — aliases the target's `default`; recognized at any nesting depth, so a conditional browser/node split keeps both branches alive, over-approximating in the keep-alive direction), and, when the barrel is itself a manifest-declared production root, promotes each re-exported symbol to the package's public surface (RFC 0011 §5). Bare-star re-exports (`export * from './x'`) fix the target file's reachability, and — when the re-exporting file is part of a published package's surface — extend that surface into the target transitively (assembly's library-surface fixpoint, shared with Rust's `pub mod`). Named re-export **chains resolve to a fixpoint** (RFC 0013 §3b): a barrel re-exporting from another barrel resolves at any depth, independent of discovery order; a re-export cycle resolves to nothing. Collision rule: a name already present in a file's table — its own declaration, or an earlier alias — wins over a later alias. |
| Declaration merging (`interface X` twice, namespace+function) | one logical symbol, multiple declaration spans |
| Decorators | reference edges to the decorator expression; `emitDecoratorMetadata` adds `TypeUse` edges on decorated signatures; DI semantics stay in plugins |
| `declare module`/ambient/global augmentation | symbols marked externally-consumed (never `unused`/`internal-only`) |
| Triple-slash `/// <reference path>` | file edge, certain |
| UMD wrappers | detected by shape, treated as generated-style opaque exports at `probable` |
| Re-export of a whole dep (`export * from "lib"`) | keeps the dep used; contributes a `probable` wildcard export surface |
| `tsconfig` project references, `baseUrl`/`paths` aliases | not followed — the adapter does not parse `tsconfig.json` at all (parking lot) |
| CLI-only dependency (`"lint": "xo"`, no `import`) | counts as used via `ManifestFacts.script_invoked_names` — the leading token of each `scripts` shell clause (`&&`/`||`/`;`/`\|`-split), cross-referenced against declared dependency names by `dependency_hygiene` (RFC 0005 §5) as a synthetic tooling-role importer. Never makes a dependency `test-only` (a CLI invocation isn't test-role) — only ever rules out `unused`. |

### 6. Conformance fixtures (shared harness, RFC 0002 §8)

Minimum corpus, each a mini-project with expected `FileFacts` + findings: ESM app with dead
symbol/file/dep · CJS interop pair · dual-mode package (`exports` conditions) · npm workspace
monorepo with cross-package dep + phantom internal import (`undeclared`) + `version-skew` · JSX
component tree (component kept alive only by JSX usage) · dynamic-import directory scan
(wildcard narrows, nothing false-positive) · test-only module + its tests (`test-only` +
deletion set) · barrel with one consumed and one orphaned re-export · `.d.ts` ambient globals ·
duplicate function pair surviving rename+reformat (Type-2) · CRAP fixture with lcov ·
suppression matrix incl. one stale pragma.

### 7. Open questions

1. `export =` (TS legacy CJS export) — support at `certain` or demote to `probable`?
2. Should `scripts` in package.json parse shell to find file refs (`node scripts/build.mjs`),
   or is "token that resolves to a claimed file" enough for 1.0? Current draft: the latter.
3. `import type` keeping a *runtime* dependency alive: current draft says types-only usage keeps
   `@types/*` alive but does **not** count as runtime usage of the implementation package —
   confirm against real-world `devDependencies` conventions during dogfooding.

## JSON

**Status:** Draft · **Depends on:** RFC 0002 §3, §7 · docs/rfcs/0012-reference-semantics-and-visibility.md (unit-key §8, visibility ladder §6, generated-origin §7)

### 0. What's structurally different from every prior adapter, and why it matters here

JSON is RFC 0002 §3's "non-source language": **files claimed, no symbols extracted.** Every
prior adapter (Rust/Java/Kotlin/Swift/Go/JS-TS) exists to turn code into declarations,
references, and roots. JSON has none of those — a `.json` file has no functions to call, no
types to reference, no entry point. What it has is *existence*: a file on disk that other code
reads. RFC 0002 §3, verbatim: "JSON participates as import *targets* so file-level `unused`
findings cover config/data files."

That one sentence is the whole adapter. A concrete illustration of why it's needed at all,
traced through the actual resolution code (`kndo-core/src/graph/assemble.rs::resolve_imports`): when JS-TS's
own `resolve()` resolves `import data from './data.json'`, it checks `ctx.contains(path)` —
membership in the **discovered** file set, not the **claimed** one. That check already succeeds
today, with *zero* JSON adapter in existence, because `ResolveCtx`'s known-files index is built
from every file discovery finds, claimed or not (`graph/assemble.rs`'s `known_files` construction).
So a JSON import already resolves to a real graph edge without this adapter. What's missing is
the other half: every analysis in `kndo-core/src/analysis/*.rs` opens with `let Some(class) =
file.class else { continue; }` — a `FileNode` with no `FileClass` (nobody claimed it) is
*structurally invisible* to `unused`, `internal-only`, `crap`, `duplicate`, everything. An
orphaned `config/legacy-flags.json` nobody imports anymore currently cannot be flagged dead,
not because reachability says it's live, but because no adapter ever gave it a `FileClass` to
be judged by. Claiming is the entire fix — resolution and reachability already work once a
`FileClass` exists.

Consequences that follow directly:

- **No tree-sitter grammar** (ADR 0002's explicit escape hatch: "the contract does not mandate
  tree-sitter... an adapter only owes `FileFacts`"). There is nothing to walk — no complexity
  metric, no token stream, no query layer earns its keep over a flat value tree. `serde_json`
  (already a workspace dependency) parses-and-validates in one call; `grammar_version` names it
  instead of a tree-sitter grammar string, so cache invalidation (RFC 0002 §6) still works the
  same way on a `serde_json` version bump.
- **`extract()` is close to a no-op.** `FileFacts::default()` plus, at most, one parse-failure
  `Diagnostic` when the content isn't valid JSON (RFC 0002 §2's "surviving broken code" still
  applies — a malformed `.json` file committed by accident is real, reportable signal). No
  declarations, no imports, no references, no roots, no functions, no `unit`.
- **`resolve()` is unreachable in normal operation, not merely trivial.** `resolve_imports` (`graph/assemble.rs`, phase 3b's first pass)
  calls `adapter.resolve()` only for the adapter that *claimed the importing file*, once per
  entry in that file's own `facts.imports` — never "ask every adapter." Since JSON's `extract()`
  never populates `imports` (JSON has no import syntax), JSON's `resolve()` is never invoked by
  the engine; it exists only to satisfy the trait, and returns `Resolution::Unresolved`
  unconditionally. This is the mechanical confirmation of why RFC 0002 §4's "core's resolution
  driver asks each adapter" reads as *the importing adapter's own resolver handles cross-language
  targets by itself* (JS-TS's `resolve_relative` finds a `.json` target via `ctx.contains`
  directly) rather than a fan-out — worth recording here since this is the first adapter whose
  own `resolve()` is structurally dead code.
- **No visibility ladder** (`[]` — RFC 0012 §6 already names CSS/JSON here), **no `member_of`**
  (RFC 0012 §3: `None` always), **`unit: None`** (RFC 0012 §8: file-scoped, alongside JS/TS and
  CSS), **no manifest of its own** (`manifest_globs: vec![]`, `claim_manifest()` always `false`
  — the dependency-manifest role for JSON-*shaped* files belongs to whichever adapter's language
  the manifest format serves, never to this adapter; see §1).
- **No generated-origin signal** (absent from RFC 0012 §7's table on purpose, not an oversight):
  strict JSON has no comment syntax, so the "`// Generated by` banner" mechanism every other
  adapter's `ContentMarkers` uses has nothing to scan. `detected_origin` stays `None` always —
  a documented non-goal, not a gap.
- **No role convention.** No `*.test.json` naming convention exists in the ecosystem the way
  `*.test.ts`/`_test.go` do, and a `__fixtures__`/`testdata` directory convention is an ecosystem
  fashion (RFC 0002 §2's "language specs are stable; ecosystems are fashion" boundary), not
  something the bare JSON format defines. Every claimed `.json` file is `FileRole::Production`
  in v1 — plain, deliberate, revisited only if a real false-positive surfaces in dogfooding
  (the same "degrade toward silence over guessing" posture RFC 0002 §5 asks of resolution
  applies here to role classification too).

### 1. Claiming & classification

**Glob:** `**/*.json`.

**Explicit exclusions — the one piece of real cross-adapter awareness this adapter carries.**
RFC 0002 §3, verbatim: "Well-known manifests (`package.json`, `tsconfig.json`) are claimed by
the *owning* adapter instead." Every other manifest format in the launch set (`go.mod`,
`Cargo.toml`, `pom.xml`/`build.gradle[.kts]`, `Package.swift`) has its own distinct syntax, so
no other adapter's `claim()` can accidentally match it — Swift's `Package.swift` needed a
self-exclusion (docs/adapters/swift.md §0/§4) because it alone is *also* valid source in its
own adapter's language, a same-adapter collision. JSON's situation is the opposite shape: a
**cross-adapter** collision, because `package.json` is genuinely, unambiguously JSON syntax,
and nothing about `**/*.json` would otherwise skip it. Confirmed structurally, not just by
the RFC prose: `kndo-core/src/graph.rs`'s phase 1 (`claim`) and phase 1b (`claim_manifest`) run
independently over every discovered file with zero cross-filtering between them — nothing stops
one adapter's `claim()` from also matching a path another adapter's `claim_manifest()` claims,
unless that adapter excludes it itself. Grepping every registered adapter's `manifest_globs`
confirms `package.json` (JS-TS) is the *only* currently-implemented JSON-shaped manifest in the
launch set — `go.mod`/`Cargo.toml`/`pom.xml`/`build.gradle*`/`Package.swift` are all
distinctly-named, non-`.json` formats. `tsconfig.json` is *not* currently a JS-TS
`manifest_glob` (tsconfig parsing is an open item there — `kndo-adapter-js/src/resolution.rs`'s
own doc comment lists it unimplemented), so excluding it here is pre-emptive: RFC 0002 §3 names
it as JS-TS's eventual manifest, and excluding it now means JS-TS adding real `tsconfig.json`
support later needs no coordinated change here.

```
fn claim(path) -> Option<FileClaim> {
    if !path.ends_with(".json") { return None }
    let basename = path.rsplit('/').next();
    if basename == Some("package.json") || basename == Some("tsconfig.json") {
        return None  // owned by JS-TS (§0) — not this adapter's, claimed or not
    }
    Some(FileClaim { language: "json", class: { role: Production, origin: Authored } })
}
```

No path-pattern-driven role/origin variation (§0) — `classify()`'s usual
`PathPatterns`-into-`kndo_adapter_toolkit::classify::classify` call isn't used; role and origin
are both constants.

**`claim_manifest()`**: always `false` — this adapter has no manifest format of its own (§0).

### 2. Extraction

`FileFacts::default()`, plus:

- **Parse validation**: `serde_json::from_slice::<serde_json::Value>(content)`. On `Err`, push
  one `Diagnostic { level: Warn, message: "invalid JSON: <serde_json's error>" }` — the same
  "surviving broken code, but saying so" contract every tree-sitter-backed adapter honors via
  `root.has_error()`. On `Ok`, the parsed `Value` is discarded immediately — nothing downstream
  needs it (§0).
- Everything else (`declarations`, `imports`, `references`, `roots`, `functions`, `unit`,
  `detected_origin`, `suppressions`) stays at its `Default` value.

**Suppressions**: not applicable — `// kndo:allow …` needs a comment syntax JSON doesn't have
(§0). `kndo.toml`'s `[[rule]]` path-based mechanism (RFC 0006 §7) is the
language-neutral alternative for a file with no inline-suppression syntax — wired since the
config subsystem landed (`kndo-core/src/config.rs`), and exactly what this repo's own
`kndo.toml` uses for the `.json` files reachable only through a Rust
`PathBuf::join("...")`-style runtime read (`xtask/perf-baseline.json`, `schemas/*.json`),
which static analysis cannot see by construction (`internal/detection-gaps.md`).

**Metrics**: not applicable — no functions exist to measure (§0).

### 3. Imports & resolution

`extract()` never populates `imports` — JSON has no import/reference syntax of its own. `resolve()`
is implemented but structurally unreachable (§0):

```
fn resolve(_spec, _ctx) -> Resolution { Resolution::Unresolved }
```

A `.json` file becomes reachable exclusively through *other* languages' resolvers finding it —
JS-TS's `resolve_relative` (or a future adapter's own resolver) matching a relative specifier
against `ResolveCtx`'s known-files index, which this adapter's *claiming* is what makes the
resulting `FileNode` visible to every downstream analysis (§0's core argument).

### 4. Manifests & packages (RFC 0011)

Not applicable. `manifest_globs: vec![]`, `claim_manifest()` always `false` (§0, §1). This
adapter contributes no `ManifestDependency` nodes, no workspace topology, no root promotion —
every one of those concerns belongs to whichever adapter's *language* a given `.json` manifest
serves (`package.json` → JS-TS), never to "JSON" as a format.

### 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| JSON Schema `$ref` cross-file references | Out of scope (§0's "language spec, not ecosystem convention" boundary — JSON Schema is a convention layered on JSON, not something the bare format defines; RFC 0002 §3 doesn't mention it, and RFC 0002 §2's framing explicitly excludes "framework conventions" from adapter scope) |
| JSONC / JSON5 (comments, trailing commas) | Not modeled — `serde_json::from_slice` rejects them as parse errors, same as any other malformed JSON (§2). Revisit only if real dogfooding surfaces false parse-failure diagnostics on legitimately-JSONC-shaped config (`.vscode/settings.json`, `tsconfig.json` itself, which is excluded anyway per §1) |
| `tsconfig.*.json` variants (`tsconfig.build.json`, `tsconfig.base.json`, …) referenced via `-p` flags in `package.json` scripts | Currently claimed as ordinary JSON (only the exact literal `tsconfig.json` is excluded, §1) — revisit together if/when JS-TS implements real tsconfig manifest parsing |
| Malformed JSON | One file-level `Diagnostic`, not a finding — parse failure isn't itself a code-quality verdict, same posture every adapter takes on unparseable source (§2) |
| Empty file / bare `{}` / `[]` | Valid JSON, claimed normally, contributes nothing beyond existing as a `FileNode` — `unused` is the only finding that could ever fire on it |

### 6. Conformance fixtures (shared harness, RFC 0002 §8)

Two fixtures — deliberately fewer than every code-extracting adapter's four, since there is no
declaration/visibility/dispatch surface to exercise (§0):

- **`orphaned-config-is-unused`** — a small JS-TS project where `main.ts` imports `data.json`
  (which becomes a live `FileNode`) while a sibling `unused-config.json` is never imported by
  anything: `unused-config.json` reads `unused`/`file`, `data.json` doesn't. This is the direct
  proof of §0's whole argument — before this adapter exists, `unused-config.json` is invisible
  to `unused` entirely (unclaimed), not merely reachable.
- **`package-json-is-not-claimed-as-plain-json`** — a `package.json` sitting next to an ordinary
  `data.json` in the same directory: `package.json` is claimed as JS-TS's manifest (contributes
  `ManifestFacts`, never a `FileClaim`), never double-counted as plain JSON source: the same
  "manifests are not claimed" assertion Swift's `claims_swift_files_and_rejects_others` unit
  test made for `Package.swift`, applied here to the cross-adapter case (§1).

### 7. Open questions

1. **Role convention for JSON.** §0 commits to "always `Production`" for v1. If real dogfooding
   surfaces a common false-positive class (e.g., `__fixtures__/*.json` test data flagged
   `unused` in a way that's noisy rather than useful), a `PathPatterns`-driven test-role
   convention is a small, additive change — no reason to speculate one into existence now.
2. **`tsconfig.json` real manifest support.** Tracked in JS-TS's own open questions
   (`kndo-adapter-js/src/resolution.rs`), not here — this adapter's only obligation is to keep
   excluding the literal filename (§1) so the day JS-TS implements it, nothing here needs to
   change.
3. **JSON5/JSONC tolerance.** §5's stance (reject, same as any malformed JSON) is a v1
   simplification, not a permanent one — revisit if dogfooding on kndo's own `.vscode/`/editor
   config surfaces real noise.

## Kotlin

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M5
**Grammar:** tree-sitter-kotlin-ng

The fifth adapter, and the first that shares its manifest infrastructure wholesale with a
sibling adapter (`kndo-adapter-toolkit::jvm_manifest`, extracted from the Java adapter for this
purpose — ROADMAP "Java → Kotlin share infra"): a `pom.xml`/`build.gradle` describes dependency
coordinates identically whether the module's source is `.java` or `.kt`. What's genuinely new
here is the *source* language — Kotlin's visibility, member, and reachability shapes diverge
from Java's in ways that matter, not superficial syntax differences. Like Java, Kotlin has no
dogfood corpus of its own (kndo is written in Rust); precision rests on the conformance
fixtures (§6). Node kinds throughout this doc are pinned against real tree-sitter-kotlin-ng
1.1.0 output (`kndo-adapter-kotlin/src/parsing.rs`'s `#[ignore]`d ground-truth dumps), not
guessed.

### 0. What's structurally different from Java, and why it matters here

- **`package` carries zero visibility meaning — the mirror image of Java's bug-prone case.**
  Kotlin's default visibility (no modifier at all) is **`public`**, not package-private —
  unlike Java, where the *absence* of a modifier is itself the package-scoped default. A
  Kotlin `package` declaration exists purely for namespacing (avoiding name collisions,
  organizing wildcard imports) and plays no role in what code can see what. Consequently
  `FileFacts::unit` (still the declared dotted package name, same representation as Java's for
  infra-sharing — RFC 0012 §8) is used here **only for resolution** (same-package unqualified
  reference, wildcard-import target enumeration), **never for visibility bucketing** — there is
  no `Unit`-scoped rung anywhere in Kotlin's ladder (contrast Java, where package-private maps
  exactly to `Unit`, §0 there). Getting this backwards — reusing Java's Unit-for-package-scope
  reflex here — would be the *same class* of bug the Java adapter's own fixture caught, just
  inverted: it would fabricate an `internal-only` narrowing suggestion onto public declarations
  that Kotlin's compiler would flatly reject narrowing to (public *is* the declared level).
- **`internal` genuinely is `VisibilityScope::Package`, no widening needed.** Kotlin's
  `internal` means "visible within the same compilation module" (a Gradle module / Maven
  module in practice) — exactly kndo's `Package` scope (RFC 0011's `PackageId`, "same
  manifest"). This is the one rung Java has no equivalent of at all (Java's four rungs are
  File/Unit/Public/Public; Kotlin's are File/Package/Public/Public) — a real, structural
  difference between the two languages' visibility models, not a naming coincidence.
- **Four-rung ladder, `protected` widens to `Public` for the same reason as Java's.** Members
  (not top-level declarations — Kotlin's `protected` is illegal at top level, same restriction
  as Java) may be `protected`: visible to the declaring class **plus subclasses in any other
  module** — no scope in `{File, Package, Public}` represents "module ∪ subclasses-anywhere",
  so it widens to `Public`, trading recall for precision in the conservative direction (RFC
  0012 §6's normative rule). One ladder covers both top-level and member declarations: `[File
  "private", Package "internal", Public "protected", Public "public"]` — two rungs (2 and 3)
  sharing the `Public` scope is the same legal pattern Java's ladder already establishes.
  `private` on a top-level declaration is genuinely file-scoped (**exact** match to `File` —
  unlike Java, which disallows top-level `private` entirely); `private` on a **member** is
  class-scoped, narrower than anything kndo models, so it widens to `File` (over-approximating
  "reachable from elsewhere in the file" — same reasoning as Java's member-`private` widening).
- **`member_of` follows the RFC's stated rule literally: lexical nesting, not receiver syntax.**
  RFC 0012 §3's language-fit table says "Kotlin: class members; top-level functions have
  `None`" — this adapter applies that by lexical position alone. An **extension function**
  (`fun String.extFn() { … }`) is syntactically top-level (or a member, if nested inside a
  class) regardless of its receiver type; its `member_of` is `None` when declared at top level,
  matching every other top-level function, **not** `Some("String")`. This means a call site
  shaped like `someVar.extFn()` (identifier receiver) won't find `extFn` through the duck-typed
  member fallback (RFC 0012 §3) if nothing else names it directly — a known, documented
  precision gap (§5, §7), not a silent one; call sites shaped `"literal".extFn()` or any
  non-identifier receiver already fall through to whole-codebase by-name matching regardless of
  `member_of`, so the gap is narrower than it first appears.
- **Companion object members attribute to the *enclosing class*, not the companion itself.**
  `companion object Named { fun factory() }` inside `class Widget` is overwhelmingly called as
  `Widget.factory()` in real code — explicit `Widget.Named.factory()` addressing is rare even
  when the companion is named. Setting `member_of = "Widget"` (the outer class) rather than
  `"Named"` (or the compiler-synthesized default `"Companion"`) maximizes real-world duck-typed
  match rate — a pragmatic call, not a spec requirement, documented here so it doesn't read as
  an oversight. Regular `object Singleton { … }` (not a companion) uses its own name, exactly
  like a class would.
- **No inline test regions, same as Java.** Kotlin test code (JUnit or `kotlin.test`) is always
  a **separate file** under `src/test/kotlin/**` — the Kotlin Gradle plugin's own Standard
  Directory Layout, universally assumed by every build tool and IDE. `FileFacts::test_spans` is
  therefore always empty here, same stance as Java §0.
- **Dispatch rooting via the `override` *modifier*, not an annotation.** Kotlin requires
  `override` on every member that implements/overrides a supertype member (a compile error
  without it, unlike Java's optional `@Override`) — structurally the same "JDK/framework-
  invoked, never a named call site in user source" situation Java's `@Override` rooting and
  Rust's trait-impl-method rooting both address, just spelled as a `member_modifier` keyword
  (`(modifiers (member_modifier (override)))`) instead of an annotation node. Every `override`-
  modified member roots `Production`/`Probable`, unconditionally — same blanket, safe-direction
  stance as Java.
- **No reliable import→dependency-coordinate mapping — identical root cause to Java's.** Kotlin
  rides Maven/Gradle coordinates exactly like Java (it has no package manager of its own); a
  Kotlin `import com.foo.Bar` carries the same zero structural relationship to a Maven
  `groupId:artifactId` that a Java import does. Same consequence as Java §0:
  `resolve()` never returns `Resolution::Dependency` for an external import, and
  `AdapterDescriptor.resolves_dependency_usage: false` makes `dependency_hygiene` skip Kotlin's
  `unused`/`test-only` dependency verdicts (one diagnostic, not a flood). `version-skew` is
  unaffected (pure manifest comparison).
- **Kotlin Multiplatform (KMP) source sets are out of scope for v1** — same posture as RFC 0002
  §7's own table entry ("multiplatform source sets (post-1.0)"). This adapter targets the
  single-platform JVM layout (`src/main/kotlin`, `src/test/kotlin`) only; `commonMain`/
  `jvmMain`/`iosMain`-style source-set trees are not claimed specially — files under them are
  still claimed as ordinary `.kt` source (language detection doesn't care about the directory),
  but role classification (§1) and root promotion (§4) assume the single-platform layout and
  will undercount on a real KMP project. Parked, §7.
- **A narrow, verified upstream grammar bug: meta-annotated, parameterless `annotation class`
  declarations mis-parse.** `@Retention(...) annotation class Marker` (no primary constructor
  parens) parses under tree-sitter-kotlin-ng 1.1.0 as a bogus `infix_expression` chaining
  "annotation", "class", "Marker" as three identifiers — verified via a dedicated probe (§2);
  the moment the annotation class has ANY primary constructor (`(val x: Int)`) or the leading
  annotation is absent, it parses correctly. Rare in practice (most real custom annotations
  either take no meta-annotations or declare at least one parameter), and out of kndo's
  control — a third-party grammar issue, not an extraction bug. Falls through to "no
  declaration extracted for this specific shape," matching the "don't fabricate" degradation
  principle every other documented gap in this codebase follows.
- **A second, independently-verified grammar edge case: single-line bodies with content
  mis-parse.** `class Inner { fun m() {} }` written entirely on one line fails to parse as a
  `class_declaration`/`class_body` pair (verified via a dedicated probe, §2); the identical
  source reformatted across multiple lines parses cleanly. Real Kotlin style overwhelmingly
  uses multi-line bodies for anything but an empty declaration, so this is a narrow formatting
  artifact rather than a practical blocker — every fixture and hand-written extraction test in
  this adapter deliberately uses multi-line bodies because of it.

### 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `kotlin` | `**/*.kt` (`.kts` standalone scripts are **not** claimed — a real but rare pattern; Gradle's own `build.gradle.kts`/`settings.gradle.kts` are manifests, already claimed by both this adapter and Java's for that purpose, orthogonal to which language wrote the project's *source*) |
| Manifests | same as Java (`kndo-adapter-toolkit::jvm_manifest` — shared verbatim): `**/pom.xml`, `**/build.gradle` + `**/build.gradle.kts`, `**/settings.gradle` + `**/settings.gradle.kts` |
| Role `test` | `src/test/kotlin/**` (Kotlin Gradle plugin's Standard Directory Layout) OR a bare filename matching the same Surefire-style convention Java's fallback uses, extension-adjusted (`Test.kt`, `Tests.kt`, `TestCase.kt`) |
| Role `tooling` | not detected — same stance as Java/Go: no ecosystem-wide config-file convention for Kotlin source files exists |
| Origin `generated` | `@Generated`/`javax.annotation.Generated` annotations (same JVM-ecosystem convention Java's adapter detects — Kotlin code calling into the same annotation-processor tooling, KAPT/KSP, emits the identical marker), detected structurally by extraction, reported via `FileFacts::detected_origin` (RFC 0012 §7). The toolkit's text-marker scan runs as a second, independent signal |
| Origin `vendored` | `vendor/**`, `third_party/**` (toolkit universal list) — same inert-but-consistent inclusion as Java |

Build-output directories (`build/**`, `.gradle/**`) need no adapter-side exclusion — discovery
already respects the project's own `.gitignore`, same as every other adapter.

**`VisibilityLevel`**: `0` (private, widened to `File` at member scope / exact `File` at top
level), `1` (internal, `Package`), `2` (protected, widened to `Public`), `3` (public, `Public`
— also the default when no modifier is present at all). §0 has the full ladder derivation.

### 2. Extraction

**Declarations**: `class_declaration` is Kotlin's single unified node for class/interface/enum
class/data class/sealed class/annotation class/inner class — `SymbolKind` is derived from the
keyword leaf (`class` vs `interface`) plus any `class_modifier` (`enum` → `SymbolKind::Enum`,
`annotation` → `SymbolKind::Other("annotation")`; `data`/`sealed`/`inner`/`abstract` are
attributes of an ordinary `SymbolKind::Class`/`Interface`, not distinct kinds — matching how
Rust's own struct/enum attributes don't spawn new `SymbolKind` variants either). `enum_entry`
(inside `enum_class_body`) → `SymbolKind::EnumMember`, `member_of` the enum. `object_declaration`
(singleton) → `SymbolKind::Other("object")`, its own name used for `member_of` on its members.
`companion_object` is **not** independently declared as a symbol in v1 (its members attribute
to the enclosing class per §0 — the companion object node itself contributes no declaration of
its own, matching the "no name to hang a finding on" stance the Java adapter takes for
anonymous classes; a companion's own liveness is inseparable from its enclosing class's). `class_
parameter` marked `val`/`var` inside a `primary_constructor` is a real field (constructor-
promoted property) — extracted as `SymbolKind::Field`, `member_of` the class, same visibility
handling as any other member (a bare `class_parameter` with neither `val` nor `var` is a plain
constructor argument, not a declaration). `function_declaration` → `SymbolKind::Method` when
lexically nested in a `class_body`/`object_declaration`/`companion_object` body, else
`SymbolKind::Function` (§0's `member_of` rule). `property_declaration` → `SymbolKind::Field`
(member) or `SymbolKind::Variable` (top-level `val`/`var` — matching Go's own top-level-var
stance) — `const val` additionally carries no distinct kind (Kotlin's `const` restricts to
compile-time-constant primitives/`String`; not modeled as `SymbolKind::Const` to avoid a
member/top-level split that gains nothing analyses read). `secondary_constructor` →
`SymbolKind::Method` named `<init>` (colliding intentionally with the primary constructor's own
`<init>` name when both exist — a class's constructors are one liveness unit for kndo's
purposes, same coarse-graining Java's own multi-constructor handling already accepts).
`type_alias` → `SymbolKind::TypeAlias` (a construct Java has no equivalent of — a clean fit,
unlike Java's need for `Other("record")`). `anonymous_initializer` (`init { … }` blocks) is not
independently declared — its body is walked as part of the enclosing class's construction-time
liveness, matching RFC 0012 §4's stated rule ("init blocks/constructors → the class").

**References**: `call_expression` with an `identifier` callee → bare `Call`; a
`navigation_expression` callee (`a.b.c()`) → `Call` on the last segment with `scope_context` set
to the **second-to-last** segment's text when that segment is a plain `identifier` (mirroring
Java's `object`/`field` qualifier extraction), else `scope_context: Some("<expr>")` and the
receiver chain is walked recursively for its own references — this is what makes
`HasCompanion.Named.factory()` resolve `factory` with `scope_context: Some("Named")` while
`Singleton.x` resolves `x` with `scope_context: Some("Singleton")`. `user_type` positions
(parameter/return/property types, `is`/`as` checks, generic bounds) → `TypeUse`, using the
type's bare `identifier` (or the last segment when the reference names a qualified type).
`delegation_specifier` entries (`class C : Base(), Interface1` — both the superclass
constructor-invocation shape and a bare interface name) → `Extend`, matching Java's
superclass+implements handling. The superclass invocation's **argument list** is walked as an
ordinary expression on top of that `Extend`: `class MyMeta : Base(MyProvider)` references
`MyProvider`, and dropping it left Exposed's `PostgreSQLTypeProvider` — passed to its
superclass on the very next declaration in the same file — with no incoming reference at all.
A primary constructor parameter's **default value** is walked for the same reason
(`class Hasher(val cost: Int = DEFAULT_COST)` references `DEFAULT_COST`); only the parameter's
type used to be. Both attribute `within` to the owning class, per RFC 0012 §4's rule that code
running on instantiation belongs to the type. Pinned by the `ctor-arg-and-default-value`
fixture, whose declarations are deliberately `internal`/`private` — public ones are library
roots and stay alive without any reference, so a public version of the fixture passes even
with the extraction gap reintroduced. Lambda bodies (`lambda_literal`) and `when_expression`/
`when_entry` bodies are walked like any other expression — same safe-direction over-
approximation as every other adapter's closure handling.

**Roots (`RawRoot`)**: a top-level `fun main()` (0 or 1 parameter — the canonical Kotlin JVM
entry point; the `@JvmStatic fun main()`-inside-`object` variant used for some build-tool
interop is not special-cased, a documented non-goal §7) → `RootKind::Production` at `Probable`,
unconditional, mirroring Java/Go/Rust's blanket `main`-rooting stance. `override`-modified
members root `Production`/`Probable` (§0's dispatch rule).

**Suppressions**: `// kndo:allow …` / `/* kndo:allow … */` — identical syntax to every other
adapter (RFC 0005 §12 is language-neutral).

**Metrics**: cyclomatic complexity +1 per `if_expression`, `when_entry` (one per arm past the
implicit base, matching JS/Java's n-way-match rule), `for_statement`, `while_statement`,
`try_expression` (one per try, not per `catch_block` — matching Java's stance of not over-
counting nested clauses), `&&`/`||` (leaf tokens inside `binary_expression`, exactly like
Java's). The elvis operator (`?:`) and the not-null assertion (`!!`) are **not** counted as
branches — `?:` is a value-producing fallback expression, not a control-flow fork the way
`if`/`when` are (same reasoning JS's optional-chaining `?.` isn't counted either); this keeps
the metric consistent across adapters rather than inventing a Kotlin-specific bump. Each `lambda_literal` or `anonymous_function` clearing the clone floor becomes its own callable **shape**
(`MetricsSyntax::nested_callable_kinds` — its branches and tokens leave the enclosing shape's
stream, which keeps one `FN` in their place, and `crap`/`duplicate` report it in its own
right). A smaller one stays an expression inside its owner: promoting it would leave both
halves under the floor and cost real clone findings — measured, that was 83 clone participants
on the field corpus. The split's semantics are uniform across adapters; only the node kinds
that trigger it are per-language.

`MetricsSyntax::construction_kinds` is deliberately **empty** here: constructing a value in
Kotlin is an ordinary `call_expression`, indistinguishable from any other call, so this adapter
has nothing true to report and `duplicate`'s construction exemption simply never fires for
Kotlin — today's behaviour, unchanged. Guessing (an uppercase callee, say) would be the adapter
inventing a verdict, in the accusation direction RFC 0012 §2 forbids.

**Properties**: a property with an accessor BODY is a callable, not a value — `Method` when it
has an owner, `Function` at top level; a stored one stays `Field`/`Variable`. Kotlin compiles such
a property to a getter, so this is the truthful kind, and the distinguishing node is a `getter`/`setter` with a `function_body`.
A bodyless accessor (a `private set`, an annotated bare `get`)
leaves the property stored: it changes the accessor, not what the property IS.

Being a callable, it also gets a **shape**: `FileFacts::functions` carries one entry per accessor
body, so `crap` and `duplicate` can see a getter the way they see a function. One symbol, one
numbering — `get` is `shape_ordinal` 0 (reading the property runs it) and `set` continues from
there, which is what keeps two accessors of one property from colliding on a nested shape's
identity. `by lazy { … }` is an initializer, not an accessor: its lambda is walked for
references and belongs to the property's own liveness, not to a shape of its own.

Every child of a `property_declaration` that carries code is walked — the `= expr` initializer,
the `by expr` delegate, and each accessor body — enumerated **by kind**, never by position. The
positional "last child" rule this replaced returned the *getter* for `val x = compute()` followed
by a `get()`, and `compute()`'s reference vanished with it.


Naming both `Field` made the kind unable to separate a constant from real logic, which is what
let `untested` accuse header-name constants and `MAX_VARCHAR_LENGTH` of not being tested.

**Grammar ground truth**: pinned in `kndo-adapter-kotlin/src/parsing.rs`'s `#[ignore]`d probe
tests, covering declarations/modifiers/visibility, imports, `when`/`if`/`for`/`while`/`try`,
lambdas/string templates/elvis/not-null, companion objects/secondary constructors/inner
classes, and the annotation-class grammar edge case (§0's last bullet) — re-run with
`--ignored --nocapture` before any tree-sitter-kotlin-ng version bump.

### 3. Imports & resolution

Emitted import kinds:

| Form | Emission |
|------|----------|
| `import com.foo.Bar` | specifier `com.foo`, binding `[Bar]` — same package/type split as Java, free from the grammar's own `qualified_identifier` nesting |
| `import com.foo.Bar as Alias` | specifier `com.foo`, binding `[{local: Alias, imported: Bar}]` — Kotlin's own import-aliasing syntax (Java has none); the binding's `local`/`imported` split already exists in the contract for exactly this shape |
| annotations on a declaration | `Declaration::markers`, as written and in source order — `@Repository` (`annotation > user_type`) and `@Named("x")` (`annotation > constructor_invocation > user_type`) alike, plus the last segment of a qualified spelling. Same facts-not-verdicts contract as Java's, same consumer (`[[externally-invoked]]`) |
| `import com.foo.*` | specifier `com.foo`, no bindings, **both** `opaque_namespace_use: true` (the wildcard over the target's exports — keeps it alive without naming what it took) and `module_names_visible: true` (the language's scoping rule: every top-level name of that package is legal here *unqualified*, so the core's bare-name fallback consults that unit's table). Emitting only the first meant a bare call to a wildcard-imported top-level function resolved to nothing at all — kotlinx.coroutines calls `recoverStackTrace(…)` that way from dozens of files in other packages, and every declaration of it read `unused`; landing the second took the repo from 3454 findings to 2909 and 69.8 C to 79.3 C |
| `import com.foo::Bar` sentinel shape | **not applicable** — Kotlin has no `import static`; a top-level `const val`/function is imported the same way a class is (`import com.foo.CONST`), row 1 already covers it |

`ImportKind::Package` throughout (no relative-path import shape), `Confidence::Certain` (an
import that doesn't resolve is either genuinely external or a compile error, never a maybe —
same reasoning as Java).

**Resolution algorithm** (the adapter's `resolve`):

1. **Same-package (no import needed).** Handled entirely by the core's `unit`-based fallback —
   never reaches this adapter's `resolve()`, identical to Java.
2. **`kotlin.`/`kotlin/` prefix, and `java.`/`javax.` prefix.** `Resolution::Stdlib` — Kotlin's
   own standard library (`kotlin.collections.*`, `kotlin.io.*`, …) is as structurally reserved
   as `java.*`/`javax.*` (no third-party artifact may declare a `kotlin.*` package), and every
   Kotlin/JVM project transitively depends on the full Java standard library too, so both
   prefixes resolve the same way.
3. **`com.foo` (any other package).** Look up against the known-units index — a hit resolves
   `Resolution::File` at the package's first file in path order, identical algorithm to Java's.
4. **Anything else.** `Resolution::Unresolved` — never `Resolution::Dependency`, same §0
   reasoning as Java's last bullet.

### 4. Manifests & packages (RFC 0011)

Fully delegated to `kndo_adapter_toolkit::jvm_manifest` (extracted from the Java adapter for
this purpose) with a Kotlin-specific `JvmSourceLayout`:

| Layout field | Java | Kotlin |
|---|---|---|
| `source_root` | `src/main/java` | `src/main/kotlin` |
| `source_ext` | `.java` | `.kt` |
| `skip_file_names` | `module-info.java`, `package-info.java` | `[]` (Kotlin has neither convention) |

Every Maven/Gradle fidelity detail (roxmltree-structured `pom.xml`, line-scanned
`build.gradle`/`.kts`, dependency-scope tables, `<dependencyManagement>` exclusion, the
`application`-plugin private-mode heuristic, `settings.gradle` topology) is byte-for-byte the
same code path Java uses — see docs/adapters/java.md §4 for the full description; nothing here
diverges except the two layout fields above. The shared `GRADLE_CONFIG_SCOPES` table also
recognizes `kapt`/`kaptTest` (Kotlin's KAPT annotation-processing configurations, → `Build`
scope, the Kotlin analogue of `annotationProcessor`) — inert dead data for the Java adapter,
which never encounters a `kapt` line in its own projects, but exercised for real here.

**Root promotion**: identical mechanism to Java's (§4 there) — one `ManifestRoot{Production,
Certain}` per non-test `.kt` file under `src/main/kotlin/**`, for every publishable module,
reusing the existing per-file declaration-promotion path with zero new core mechanism.
Projects that place Kotlin source under `src/main/java` (a non-standard but technically
supported Kotlin Gradle plugin configuration, since the plugin accepts both roots
simultaneously) are **not** promoted from that second root in v1 — a narrow, documented non-
goal (§7); such files still parse, extract, and resolve correctly, they simply don't get the
manifest-driven root-promotion boost a `src/main/kotlin`-housed file gets.

### 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| Extension functions called via an identifier receiver (`someVar.extFn()`) | not resolved through the duck-typed member fallback — `member_of` is `None` for top-level declarations regardless of extension receiver syntax (§0); a documented recall gap, narrower in practice than it sounds since non-identifier receivers already fall through to whole-codebase by-name matching |
| Companion object addressing | members attribute to the enclosing class (§0) — `Widget.Named.factory()`'s fully-qualified form still resolves the CALL correctly (last-segment-name matching doesn't care which qualifier text preceded it), it's the *declaration's* `member_of` choice that's pragmatic, not exact |
| Data class synthesized members (`copy()`, `component1()`, `equals`/`hashCode`/`toString`) | not modeled — same class of gap as Java's record-accessor stance; a call to `point.copy()` produces a reference that simply never resolves, harmlessly |
| Delegated properties (`val x by lazy { … }`) | the delegate expression (`lazy { … }`) is walked for its own references like any other property initializer; the `by`-delegation protocol itself (`getValue`/`setValue` dispatch) is not modeled — same non-goal class as Java's reflection stance |
| Smart-cast / `is`/`as` type checks | the checked type is an ordinary `TypeUse` reference; the compiler-level flow-sensitive narrowing itself has no representation in kndo's model (nor does any other adapter's) |
| Meta-annotated, parameterless `annotation class` | not extracted — a verified upstream grammar limitation, not an extraction bug (§0's last bullet) |
| Kotlin Multiplatform source sets | files still claimed/extracted as ordinary `.kt` source; role classification and root promotion assume single-platform JVM layout and undercount on a real KMP tree (§0, §7) |
| `expect` / `actual` declarations | ONE logical declaration with several bodies, and the unit key (the declared package name) is the same for all of them — so they are exactly the core's same-unit *twins*, and a reference to the name edges to every one of them. Nothing Kotlin-specific in the core: the same machinery covers Go's mutually exclusive build-tag files and Rust's `#[cfg]` alternates (RFC 0012 §8). Before it, the `expect` took every call and its `actual`s read `unused`, or the reverse |
| `@JvmStatic`/`@JvmName`/`@JvmOverloads` JVM-interop annotations | not modeled specially — these affect bytecode-level dispatch shape (extra overloads, static vs. instance methods) that has no bearing on kndo's textual liveness model; a call site written in Kotlin always looks like an ordinary Kotlin call regardless of what the annotation generates for Java callers |

### 6. Conformance fixtures (shared harness, RFC 0002 §8)

Five fixtures, each a real Maven/Gradle module tree run through the real `Engine` (no mock),
directly mirroring the Java adapter's fixture set so the two adapters' precision is comparable
apples-to-apples:

- **`dead-code-same-package`** — a private (`packaging=war`) module: `main()` calls
  `Helper.live()` with no import (same-package `unit` resolution) while `Helper.dead()` is never
  called — `dead` reads `unused`, `live` doesn't.
- **`dispatch-and-cross-package`** — a private module: `Impl` implements `Greeter` via
  `override fun go()`, never named-called directly (only reached through the interface-typed
  variable's type) — stays alive only through dispatch rooting (§0); `Greeter.go` (the
  interface's own abstract declaration) legitimately reads `unused`. `Runner` imports
  `com.util.*` (wildcard) and calls `Helper.assist()` qualified — proves cross-package
  visibility computation is correct, the same shape that caught Java's `Unit`-vs-`Package` bug,
  here verified against Kotlin's *different* (correct-by-construction, §0) ladder mapping.
- **`visibility-ladder-and-internal`** — one package, all four ladder rungs exercised
  (`private`/`internal`/`protected`/`public`) on a class's members plus a companion object's
  members (attributed to the outer class per §0) and a nested class's members: `internal`
  methods used only within the module correctly stay at their declared level (no finding —
  already tightest), `protected`/`public` methods used only within the same module downgrade-
  recommend to `internal`, proving `Package`-scope (not `Unit`) is what `internal`'s tightest-
  sufficient check actually computes against.
- **`maven-gradle-dependency-skip`** — a Maven module beside a Gradle module (`kapt`
  configuration included, proving the shared scope table's Kotlin-specific entries work) at
  skewed versions of the same coordinate: `version-skew` fires, neither module's genuinely-
  unused dependency produces an `unused`/`test-only` finding.
- **`ctor-arg-and-default-value`** — `MyMeta`'s superclass constructor invocation
  (`Base(MyProvider)`) references the `internal object MyProvider` only as an argument, and
  `Hasher`'s primary-constructor default value (`= DEFAULT_COST`) references a `private`
  companion constant only as a default — both stay alive solely because the superclass-
  invocation argument list and the parameter default value are walked as ordinary references
  (§2); dropping either walk would read the referenced declaration `unused`. Both declarations
  are deliberately `internal`/`private` rather than public, so a library-root fallback can't
  mask the gap, and both correctly fire `internal-only` (used only within the file, narrower
  than declared).

**Gaps the fixtures do NOT re-verify** (already covered by Java's fixtures against the shared
`jvm_manifest` module, no value in duplicating): Maven `<dependencyManagement>` exclusion,
`<modules>`/`settings.gradle` workspace topology, the `application`-plugin private-mode
heuristic. Kotlin's fixtures focus on what's genuinely different — the ladder, dispatch
rooting via `override`, and companion-object `member_of` attribution.

### 7. Open questions

1. Extension-function call resolution via an identifier receiver (§0, §5) — would need
   `member_of` to somehow carry the receiver type without breaking the RFC's "top-level
   functions have `None`" rule, or a distinct resolution path recognizing extension-function
   declarations as a special member-like category. Not attempted in v1; parked pending real
   signal this recall gap matters in practice.
2. Kotlin Multiplatform source-set-aware role classification and root promotion (§0, §5) —
   RFC 0002 §7 already scopes this to post-1.0; unchanged here.
3. `src/main/java`-housed Kotlin-adjacent source roots (mixed Java+Kotlin modules using the
   non-default layout) — root promotion assumes `src/main/kotlin` only (§4); a contained
   follow-up, not attempted in v1.
4. `@JvmStatic fun main()` inside a top-level `object` (an alternate, less common Kotlin JVM
   entry-point shape) — not rooted; the plain top-level `fun main()` shape (§2) is the
   overwhelming convention.
5. Companion object's own liveness as a distinct symbol (§2) — currently inseparable from its
   enclosing class; if a companion object contains dead code alongside live top-level-class
   members, per-member `unused` findings still fire correctly (since members are individually
   tracked), but there's no way to ask "is this whole companion object dead" as its own
   question. Low priority — the enclosing class almost always shares the companion's fate.
6. Gradle version catalogs (`libs.versions.toml`) — identical gap to Java's §7 point 4, inherited
   unchanged via the shared `jvm_manifest` module.

## Rust

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M5
**Grammar:** tree-sitter-rust

The third language, and the first where kndo analyzes *itself* — the dogfood ceiling. Rust
stresses the contract in ways JS/TS and Go did not: the file graph is built by `mod`
declarations rather than imports, visibility is a four-step keyword ladder rather than a
binary, a package (crate) has *two* kinds of roots (lib API and bins), and macros expand
arbitrary code the static graph cannot see. Every stance below is written against those.

### 0. The one Rust-shaped idea

**The module tree IS the file graph.** In JS, any file may import any file; in Go, a package
is a directory. In Rust, files participate only when a `mod` chain from the crate root
declares them — `src/lib.rs` says `mod a;`, `a.rs` says `mod b;`, and only then does
`src/a/b.rs` exist as far as the compiler cares. Two consequences the adapter is built on:

1. `mod foo;` is an **import** in kndo's model — the parent file's certain `ImportsFile`
   edge to the child (`foo.rs` or `foo/mod.rs`). A file no `mod` chain reaches is dead to
   the compiler, and kndo's reachability reproduces that verdict for free through these
   edges — no special "orphan module" machinery.
2. `use` paths resolve **through the same tree**: `crate::` starts at the crate root
   (`src/lib.rs`, else `src/main.rs`), `self::` at the current file's module directory,
   `super::` one module up. All of it is path arithmetic over the discovered file set —
   no compiler queries, per RFC 0002.

### 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `rust` | `**/*.rs` |
| Manifests | `Cargo.toml` (workspace and package alike). `Cargo.lock` is **not** claimed |
| Role `test` | `tests/**` (integration tests), `benches/**`, `examples/**` — **package-relative**: declared as `package_test_dirs` on the descriptor and matched by core assembly against the dir of the owning `Cargo.toml`, not path-globally |
| Role `tooling` | `build.rs`, `.cargo/**`, `xtask/**` (the de-facto task-runner convention) |
| Origin `generated` | first-64-lines markers via the toolkit's `ContentMarkers` (`@generated`, `Code generated`, `Automatically generated`) — covers bindgen/prost/tonic banners. Build-script output lives in `OUT_DIR`, outside the tree — no stance needed |
| Origin `vendored` | `vendor/**`, `third_party/**` (toolkit universal list) |

`examples/**` is `test` deliberately: an example consumes the public API from outside like a
test does, and a symbol alive *only* through its own demo is exactly the `test-only` verdict
— dead API kept warm by its own showcase.

Package-relative matters because these are Cargo *target* conventions: they bind to the
manifest beside them. A workspace-excluded crate whose sources happen to live under some
ancestor's `examples/` or `tests/` tree is owned by its **own** `Cargo.toml`, and its
`src/` is ordinary production code — matching the segment anywhere in the path misroled
every such nested crate wholesale.

**Sub-file test regions.** Rust tests are whole files (the table above) *or* blocks inside
production files — so role-by-path alone cannot separate production from test code, and the
adapter must know the **spans** that belong to test constructs. Extraction records them as
`FileFacts::test_spans` (contracts §2): the extent — gating attributes included — of every
outermost `#[cfg(test)]` item (typically `mod tests { … }`), every `#[test]`/`#[bench]`
function, and the whole file under a `#![cfg(test)]` inner attribute. Items *inside* a
recorded region add nothing (the outermost extent covers them). The file's claimed role does
not flip; consumers act at span granularity:

- declarations inside a region become in-source **test roots, derived by assembly** from
  span containment (contracts §2 — the spans are the *single* producer-side declaration;
  extraction emits no per-declaration Test roots, so the two representations cannot drift).
  Reachability colors from those derived seeds, and the core exempts test-rooted symbols
  from `test-only`/`untested` (a test reachable only from tests is a test);
- `crap` and health's symbol tallies **skip** region-contained symbols — the exact exemption
  test files get (a gnarly test helper is not untested production code); `duplicate`
  deliberately does *not* skip them — it fingerprints test files too, so inline test clones
  remain findings;
- `dependency_hygiene` treats an import **sited** inside a region as a test-role usage: a
  `[dependencies]` crate consumed only under `#[cfg(test)]` is a `test-only` dependency that
  belongs in `[dev-dependencies]`;
- the **out-of-line gated module** — `#[cfg(test)] mod tests;` pointing at `src/tests.rs`, a
  whole-file test the path claim cannot see — is handled at assembly (contracts §2, phase
  2.55): the `mod` import originates inside a test region, and a file reached *only* by
  test-gated module links is demoted to test role (any production-sited `mod` link vetoes).

### 2. Extraction

**Declarations** — `fn`, `struct` (+ named fields? no — fields are not independent liveness
units in v1), `enum` + variants (variants as `EnumMember` members of the enum, RFC 0012 §3),
`union`, `trait` (+ its method signatures as members of the trait), `impl` methods and
associated consts/types (members of the **self type**'s name; `impl Trait for T` methods are
members of `T` and additionally emit an `Implement` reference to `Trait`), `const`,
`static`, `type` aliases, `macro_rules!` (kind `Macro` — the contract's expansion-symbol
kind: visibility-scope analyses skip it as a subject and treat references attributed to it
as expansion-site-wide, core-traits.md §1). Inline modules
(`mod x { … }`) **flatten**: their items extract at file level, undecorated — file ≈ module
is this adapter's standing approximation, stated once here and leaned on everywhere.
`mod foo;` (the file-declaring form) is an import, not a declaration (§0).

A member declared inside a trait `impl` records **`Declaration::implements`** — the trait's
base name — for *every* trait impl, machinery or not. `Serialize` means nothing to this
adapter and everything to `kndo:serde`; the fact is what lets that plugin be a curated table
rather than a second Rust parser, and reducing it to `implicitly_invoked` alone (this
adapter's own verdict about the language's stdlib machinery traits, §2's `is_machinery_trait`
list) is what forced the plugin to re-read source before.

Both positions of an `impl` header reduce to a **base name**: the type or trait being named,
never one of its arguments. `impl Index<usize> for Table` implements `Index` and owns `index`
under `Table`; `impl<E> Deserializer<'de> for StringDeserializer<E>` owns its members under
`StringDeserializer`. Reading the last identifier in the header's subtree instead — which is
what this adapter did until the reduction got its own function — answers with the *argument*:
every generic type's members were filed under a phantom owner (often the impl's own type
parameter, a name no receiver can ever unify with), the `Implement` reference pointed at an
argument, and every generic machinery trait (`Add`, `Index`, `PartialEq`) silently lost its
members' `implicitly_invoked` marks while the non-generic ones worked by accident. serde's
`#E.into_deserializer` findings were the visible symptom; correcting it removed 123 findings
from serde, 334 from tokio and 87 from ripgrep, and left the five non-Rust field targets
byte-identical.

**Visibility ladder** (RFC 0012 §6), declared on the descriptor:

```
[ Module@own "private", Module@parent "pub(super)", Package "pub(crate)", Public "pub" ]
```

- no `pub` → level 0, on the **`Module` rung anchored at the file's own unit**. Rust privacy is
  module-and-descendants, and that is now sayable: it used to be approximated as `File` scope
  because the ladder had no rung for a subtree, and the approximation is what made
  `private-type-leak` accuse `flags::parse::lookup` for naming `flags/mod.rs`'s private `Flag`
  — a type every module under `flags` can spell perfectly well.
- `pub(crate)` → level 2 (`Package` scope — crate = kndo package, exactly).
- `pub(self)` → level 0 — it IS `private`, spelled long.
- `pub(super)` on an item **inside an inline mod** → level 0, not exported: `super` of an
  inline mod is a module within this same file, so under file ≈ module the item never
  leaves the file (widening it to the crate rung fabricated `internal-only` on ripgrep's
  `mod convert { pub(super) fn … }` — M6 residuals).
- Top-level `pub(super)` → level 1, its own rung: the **`Module` rung anchored at the PARENT
  unit**. It used to be widened into `pub(crate)` because nothing sat between one file and one
  package, and that widening was what kept `private-type-leak` gated — the model could not tell
  tokio's `task::state::unset_waker` (whose sibling caller genuinely cannot name the
  `state.rs`-private `UpdateResult`) from the harmless inverse. Both rungs are `Module` and
  differ only by ANCHOR, which is the honest shape: in Rust every visibility but `pub` is a
  module subtree, and what distinguishes them is how high the subtree is rooted.
- `pub(in path)` → level 2, still widened to `pub(crate)`: this adapter does not resolve the
  path to a unit key yet. Widening only ever silences an `internal-only`, never accuses (7
  occurrences across the whole of tokio, for scale).
- `pub` → level 2. `exported` = any `pub*` form except `pub(self)` and
  inline-mod `pub(super)`.

**Unit:** the file's own MODULE, keyed by its path with the conventional file names folded
into the directory they stand for — `src/graph/mod.rs` and `src/graph/assemble.rs` are
`…/src/graph` and `…/src/graph/assemble`, and `src/lib.rs` is `…/src`. Rust's resolution unit
IS the module, and under file ≈ module (§0) a module is a file, so **every key names exactly
one file**.

That degeneracy is why this is not the Go machinery in disguise. Rust files still never resolve
each other's names implicitly — a one-file unit table is the file's own table, which the ladder
already consults first, so no name resolves anywhere it did not before. What the key buys is
the one thing a per-file table cannot express: **twins**. `#[cfg(target_os = "macos")] fn
socket_dir` beside `#[cfg(not(…))] fn socket_dir` is two declarations of one name, and the
single-slot bare table kept one and silently dropped the other, leaving it with no incoming
edge and a false `unused` on code every other build compiles. Twins are tracked per unit
(RFC 0012 §8), so a language with no unit key had none. `#[path]` and inline `mod x {}` break
the path convention, the same standing approximation the rest of the adapter makes.
`unit_name`: also `None` (qualified references resolve through import aliases instead).

**References** — identifiers with `within` (enclosing fn/method, `Owner.name` form for
members, RFC 0012 §4); qualified paths emit `scope_context` = the immediate qualifier
segment (`helpers::run()` → `{name: run, scope_context: helpers}`), which the core matches
against import bindings/aliases (RFC 0012 §9); method calls (`x.foo()`) and field accesses
emit member references that land in the duck-typed member fallback (RFC 0012 §3) — Rust's
`Deref`-based method resolution is exactly the case that fallback exists for. Type positions
→ `TypeUse`; `impl Trait for T` → `Implement`. Attribute derives (`#[derive(Serialize)]`)
emit `TypeUse` references to each derive name at `probable` — that is what keeps a
derive-only dependency honest in dependency hygiene. **Attribute arguments** more generally
are token soup that may name real items: `a::b::C` runs are reconstructed and emitted like
body paths (`#[rkyv(with = crate::rkyv_support::SmolStrAsString)]` imports the module and
`TypeUse`-references the item; `#[derive(serde::Serialize)]` keeps `serde` used), at item
level and field/variant level alike — a struct alive only through a field attribute stays
alive. Lint-control and non-item attributes (`allow`/`warn`/`deny`/`forbid`/`expect`,
`doc`, `cfg`/`cfg_attr`) are excluded: their arguments are lint paths and config keys, and
`#[allow(clippy::x)]` must never invent a `clippy` dependency.

**Attribute STRINGS are a different fact, and stop short of being references.** Alongside the
ident scan, every attribute outside that exclusion list records its `key = "literal"` pairs
into `FileFacts::string_attr_args` — attribute head, key, literal, and the declaration it
decorates (a field or variant attribute is attributed to its enclosing type, which is where
the generated impl lives). The adapter emits no reference for these, and the measurement is
why: over the same attributes, serde alone writes 482 identifier-shaped pairs, 248 of whose
values collide with a real declaration in the crate, and only 176 sit under a key that names
an item. Treating a collision as a reference would contribute 72 keep-alive edges in one crate
to close one real case, and a keep-alive edge silences a true finding. `skip_serializing_if =
"f"` names a function and `rename = "f"` names a wire label; telling them apart is knowing what
serde is, so the interpretation lives in `kndo:serde` and the fact stops at the key. Bump the
adapter's `facts_schema_version` when what it emits changes; the shape of the fact itself is
`cache::ENTRY_FORMAT_VERSION`'s.

**Trait items inherit the
trait's visibility** (the same language rule as enum variants): a `pub trait`'s methods sit
on the public rung, which is what lets the member fallback see their cross-file call sites
and what places them on a published crate's API surface.

**Imports** — `use` declarations, mapped to the contract like this:

| Form | Emission |
|------|----------|
| `use p::{X, Y as Z}` | specifier `p`, bindings `[X, {local Z, imported Y}]` |
| `use p::X` (single, brace-free) | specifier `p::X`, binding `[X]` — the resolver's two-step rule (§3) sorts out whether `X` was a module or an item; extraction never guesses by capitalization |
| `use p::*` | specifier `p`, no bindings, `opaque_namespace_use: true` → the core's `Wildcard` over the target's exports, which is the truth of a glob |
| `use p as q` | specifier `p`, `local_alias: q` (qualifier alias for `q::…` references) |
| `pub use …` | same as above + `reexported: true` — Rust re-export facades ride the same barrel machinery as JS, fixpoint-resolved (RFC 0013 §3b), multi-hop included |
| `mod foo;` | specifier `self::foo`, `side_effect_only: true`, `local_alias: foo` — the file-linking edge (§0); `#[path = "…"]` on the `mod` overrides the conventional location with the literal path. The alias is not decoration: it is how this file states "the name `foo` binds to that file", which is the producer side of the core's module hop (below) |
| `extern crate name;` | specifier `name` (resolves as a dependency/stdlib like any bare first segment) |

`ImportKind::Relative` for `crate::`/`self::`/`super::` paths, `Package` for bare-first-
segment paths. All `use` edges are `certain` — Rust has no bundler ambiguity.

**A brace member may be a submodule, and the core hops for it.** `use crate::internals::{attr,
check, Ctxt};` emits bindings for all three, but `check` names a *module*, not an item — nothing
in `internals/mod.rs` declares it, so the binding resolves to no symbol and `check::check(cx, …)`
used to bind nothing (`internal/detection-gaps.md` §8: this killed serde's whole `check_*`
family). The adapter emits nothing special for it; the resolution is core-side and
language-blind (RFC 0012 §9-bis): a name bound to a file that the *target's own* import table
binds again follows that one hop, and `internals/mod.rs`'s `mod check;` — with its
`local_alias` — is exactly that second binding. This is why the alias on `mod foo;` is
load-bearing, and why extending the specifier to `crate::internals::check` was the wrong fix: it
would teach the core that `::` joins path segments.

**Which imports are reconstructed.** Every import the adapter synthesizes from a use site —
attribute paths, body paths (both the rooted and bare-rooted branches), the root probes, and paths
inside macro token trees — carries `reconstructed: true`. A `use`, a `mod foo;` and an
`extern crate` do not: the file contains those. The flag is what stops a synthesized binding from
shadowing the file's own declaration and what keeps its qualifier from settling a miss
(RFC 0012 §9-quater); confidence cannot stand in for it, since a `crate`/`self`/`super`-rooted
synthetic import is `Certain` about where it resolves.

**And the adapter names every qualifier the core used to guess.** A body path with no `use`
behind it (`helpers::run()`, `kndo_core::discovery::find_files_named(..)`) reconstructs as a
synthetic module import, and that import carries `local_alias` — the segment the use site
actually qualifies by, which only something that knows what `::` joins can identify. The core
previously recovered it by splitting the specifier, its single piece of hardcoded language
syntax; that fallback is gone (RFC 0012 §9-ter). These imports are reconstructions, so they
carry `probable`/`possible` confidence, and the core reads that as "does not close the
namespace": a miss under such a qualifier keeps falling through to the in-scope/duck ladder,
where a real `use` statement's alias would settle. The braced form without `self`
(`use a::b::{X, Y}`) deliberately sets no alias — Rust does not bring `b` into scope, and the
extraction table above has always said so.

**Dynamic constructs** — macros, with a deliberately bounded stance:

| Construct | Effect |
|-----------|--------|
| `name!(…)` invocation | a `certain` reference to `name` (keeps `macro_rules!` alive), plus a scan of the token tree: `a::b` token runs are reconstructed and routed through the body-path rule — `print!("{}", render::render_query(x))` binds `render_query` through the `use`-established qualifier exactly as it would outside the macro — and lone identifier tokens stay plain reads (`format!("{}", user)` keeps `user`'s referents alive). String literals inside token trees are scanned for Rust 2021 **inline format captures**: `format!("v{VERSION}")` reads `VERSION` (`{{` escapes and positional `{}`/`{0}` contribute nothing). **No wildcard per macro** — that would drown every Rust file in `possible` edges |
| `include!("lit")` / `include_str!` / `include_bytes!` with a literal | `probable` file edge |
| `env!("CARGO_BIN_EXE_<name>")` / `option_env!` | `invoked_executables` fact: Cargo's own handshake for "this test executes the workspace binary `<name>`" — the core resolves the name through the manifest's named bins (§4) and emits an `InvokesFile` edge, RFC 0005 §1's invoked-program rule. Only the documented prefix, only a literal |
| `#[no_mangle]` / `#[export_name]` / `pub extern "C" fn` | in-source `Production` root at `probable` — an FFI consumer exists outside the graph |
| `#[test]` / `#[bench]` | a recorded test region (§1) — assembly derives the `certain` `Test` root from span containment; extraction emits no root of its own |
| `impl MachineryTrait for T` members | `implicitly_invoked` (RFC 0005 §1's machinery-dispatch rule) — the curated criterion is "the call site never writes the method's name": the fmt hooks (`Display`/`Debug`/numeric formats), `Drop`, `Default`, comparison + `Hash`, the operator-overload traits, `Index`/`Deref` sugar, `Iterator`/`IntoIterator`, `Future`, `FromStr`, `Error`. Composes with the trait-impl `Probable` Production root (which keeps hooks alive with zero owner usage): the flag lets them *inherit the owner's colors*, so a `Display` impl on a test-covered type stops reading as untested. Name-called trait methods (`.clone()`, `.into()`) and third-party traits (serde et al.) deliberately stay out — the duck fallback covers the former; a curated fact table beyond the stdlib is the recorded future source for the latter |
| `#[proc_macro_derive(Name, …)]` | **two declarations**, because there are two: `Name` (kind `Macro`, exported, spanned at the identifier inside the attribute) is what a `#[derive(Name)]` site references and the only thing a proc-macro crate can export, and the `fn` keeps its own declaration for its span, metrics and callees. A `Call` reference from the function, `within` the macro, carries the contract's rule "`within` = the symbol whose use triggers this code": using `Name` runs the function. Reachability then flows from every derive site — the test suite's included — into the implementation. Without it a proc-macro crate reads as production code no test reaches, however thoroughly its derive sites are tested. `#[proc_macro]` and `#[proc_macro_attribute]` need nothing: there the invocation name IS the function's |
| `#[cfg(…)]` | **both branches kept**, always: kndo analyzes the source, not one compilation; over-approximating alive is the safe direction. Two cfg-gated same-name items collapse to last-wins in the symbol table (documented artifact, harmless for liveness). A cfg satisfiable **only under a harness** (`test`, `loom`, `fuzzing`, `miri`, `kani` — never `feature = "…"`, which is published surface) marks a test region instead; conjunctions need one harness branch, disjunctions need all, and a negation or an unrecognized predicate is an ordinary build |

**Metrics** (`MetricsSyntax` as data): branches `if_expression`, `match_arm` (n-way match ≈
n−1 branches plus the base — counted per arm past the tree shape), `while_expression`,
`for_expression`, `try_expression` (`?` is an early-return branch), `&&`, `||`;
identifiers → `identifier`, `field_identifier`, `type_identifier`,
`shorthand_field_identifier`; literals → string/raw-string/char/byte/integer/float
literals; skipped → `line_comment`, `block_comment`. Winnowing parameters shared (toolkit).

Each `closure_expression` clearing the clone floor becomes its own callable **shape**
(`MetricsSyntax::nested_callable_kinds`): its branches and tokens leave the enclosing shape's
stream, which keeps one `FN` in their place, and `crap`/`duplicate` report it in its own right.
A smaller one stays an expression inside its owner — promoting it would leave both halves under
the floor and cost real clone findings (measured: 83 clone participants on the field corpus).
The split's semantics are uniform across adapters; only the kinds that trigger it are
per-language.

A body that is **only** a value construction (`struct_expression`) is not clone-eligible
(`MetricsSyntax::construction_kinds`): normalization erases the field values — the whole
authored content — and keeps the field list the type declaration dictates, so two constructions
of one type match by definition of the type rather than by evidence of copying.

**Suppressions** — `// kndo:allow …` via the toolkit scanner, like every language.

**Extraction refinements from the M6 FP hunt (ripgrep corpus):**
- A body-scoped `use` (inside a function) routes through the same `collect_use` extraction as
  item-level uses — walking it as expressions fabricated phantom package references from its
  intermediate segments (`use std::{fs::File, os::{fd::AsFd, …}}` → "fs"/"os"/"fd" as
  packages).
- Scoped items inside use lists register their bound tails as local qualifiers
  (`use crate::flags::{doc::version}` binds `version`, so `version::generate()` is that
  import's alias, never a phantom root import).
- A `use` inside a macro *invocation's* token tree keeps only its root as a side-effect import
  and skips through its `;` — inner segments are import structure, not expression paths.
- `macro_rules!` expansion templates (each rule's right-hand token tree) are scanned with the
  same reconstruction macro invocations get, attributed `within` the macro — ripgrep's
  `err_message!` calls `crate::messages::set_errored()` from a template, which was otherwise
  invisible and false-positived the callee as `unused`.
- `#[macro_use] mod x;` sets `opaque_namespace_use: true` on the mod import: it globs the
  child's macro namespace into crate scope — invocations anywhere reach its `macro_rules!`
  with no import for a binding to express, which is precisely RFC 0005 §1's wildcard truth.
  The per-macro no-wildcard stance (§2's table) is untouched; this is one edge per
  `#[macro_use]`, not one per invocation.

**When a bare path root is EVIDENCE of a crate (the field-audit pass, serde/alacritty
corpus).** A bare path (`mem::size_of`, `fmt::Write::write_fmt`) emits a root import so that
an unknown crate becomes the `Dependency` edge `undeclared` judges. That claim is only as good
as the file it came from, and five shapes make it worthless — in each the import is
**downgraded to `Possible`, never dropped**: `undeclared` ignores that tier so the accusation
disappears, while the edge survives so a *declared* crate reached only through such a path
still reads as used. (Suppressing them outright instead turned alacritty's `dirs` and `home`
into false `unused` dependencies — the downgrade is load-bearing, not cosmetic.)

| Shape | Why the root proves nothing | Field case |
|-------|----------------------------|------------|
| A glob import anywhere in the file (`use crate::lib::*;`) | a glob binds an open set of names extraction cannot enumerate without resolving the target | serde re-exports `mem`, `cmp`, `fmt`, `iter`, `net`, `slice` this way — all six accused of being phantom deps of `serde_core` |
| Inside a macro token tree (`quote!` body, `macro_rules!` right-hand side) | a template describes code that does not exist yet, under names the expansion invents | serde_derive's `_serde`, `__S`, `__D`, `__E`, `__A`, `__Field`, `__private`, `private2`, and `clippy::…` lint paths |
| The root names a **type** declared in this file | a type in scope shadows an extern-prelude crate; raw identifiers (`enum r#type`) defeat the "lowercase root ⇒ crate-shaped" test outright | serde's test suite, `r#type::r#struct` |
| The root is a name this file's own `use` statements bind to something else | `use serde::de::{self, …};` then `use de::Error;` re-qualifies that binding — an import binding shadows a crate of the same name | alacritty `alacritty/src/config/bindings.rs` |
| An inline `mod` declared in a **function body**, then `use`d below it | the flatten model puts inline-mod contents in this same file; nothing outside it is named | serde `test_annotations.rs`'s `mod desugared` |

The boundary for the macro rule is the token TREE, not the invocation: `serde_json::json!(…)`'s
own path is ordinary code at the call site and keeps probing, because it is a sibling of the
tree rather than inside it. Both emitters carry the gate — the single-qualifier root and the
deep-path root (`fmt::Write::write_fmt` is three segments, so only the deep one ever fires for
it; hardcoding `Probable` there kept serde's `fmt` accused after the shallow branch was fixed).

- Member chains inside macro token trees get the same receiver typing as body code:
  `write!(col2, "{}", flag.doc_short())` arrives as token soup, but the receiver's type is
  a declared fact — the reference carries qualifier `Flag` exactly as outside the macro
  (untyped receivers keep the raw name, the duck route; argument token trees are still
  scanned). Before this, every member call inside `write!`/`format!` degraded to a bare
  read that resolved to nothing — ripgrep's whole help generator was invisible.
- A path through a same-file **inline mod** (`convert::usize(…)` with `mod convert { … }`
  right there) emits a *bare* reference: extraction flattens inline-mod bodies, so the
  target is a same-file symbol — routing it through qualifier resolution had nothing to
  bind to and false-positived the target as `unused`.
- A qualified path whose qualifier is a **type** (`logger::Logger::init()`) additionally
  references the type itself (`Logger`, resolved through the segment before it): the
  traversal uses the type, and without the reference it read as file-local and
  `internal-only` advised narrowing it.
- A `crate::`/`self::`/`super::` path that crosses into type space
  (`crate::logger::Logger::init()`) splits at the first uppercase segment: the module
  prefix (`crate::logger`) becomes the import specifier binding the type (`Logger`), the
  type is referenced, and the trailing item resolves through the member table — an unsplit
  specifier resolved to no file and the whole chain read as dead.
- Every member of an `impl Trait for T` — methods AND associated types/consts (`type
  Output = …` in an `Add` impl) — roots at `Probable`: trait-impl items are consumed
  through the trait's dispatch machinery (`dyn`, generic bounds, operators, `for` loops),
  never by name, so the name-based fallback cannot see their consumption (the RFC 0012 §3
  dispatch rule made concrete; same stance as Swift's conformance witnesses). Twin impls
  (`impl Add for Stats` twice with different RHS) each root — the core lands a
  declaration-targeted root on every declaration sharing the selector.
- `#[global_allocator]` / `#[panic_handler]` / `#[alloc_error_handler]` root the item: the
  runtime is the consumer, same externally-invoked semantics as `#[no_mangle]`.

**Receiver typing (RFC 0012 §3-bis)** — a per-function `TypeEnv` built from language FACTS
visible in the file pins receiver types, so `args.matcher()` emits its TYPE as the
qualifier (`HiArgs`) and the core resolves the member in the type's home file at Certain,
exactly like the explicit `HiArgs::matcher` path. The sources, in order of certainty:

- `self` / `Self` → the enclosing `impl`'s self type (including `Self::assoc()` paths and
  `let x = Self::new()` initializers).
- Typed parameters — fn and closure alike — and annotated `let`s. The declared type is
  reduced to its dispatch base: `&`/`&mut` stripped, `Box/Rc/Arc<T>` auto-deref to `T`,
  `impl Trait`/`dyn Trait` to the trait, `Vec<T>` stays `Vec`.
- Struct-literal initializers (`let t = Widget { .. }` — certain) and `T::assoc(…)`
  initializers (`T::new()`, builders — the constructor convention), with the type flowing
  through call chains (`SearchWorkerBuilder::new().opt(x).build()` types every link).
- A name bound to CONFLICTING types anywhere in the function is dropped outright:
  ambiguity degrades to the duck-typed fallback (RFC 0012 §3), never to a guess.

Reliability bound, stated once: a wrong inference can only MISS (→ duck fallback, exactly
today's behavior — the core never settles on a receiver-typed qualifier) or hit a member
the named type genuinely declares — both degrade toward silence, never toward accusation.
**Member-type facts (the cross-file tier, `FileFacts::member_types`)** — every struct
field (named and tuple-positional), impl method return type, and associated const emits an
`(owner, member, yields)` fact, dispatch-reduced, `Self` resolved to the owner. Three more
producers, all of them *declared* facts rather than inference (RFC 0012 §3-ter):

- **Every top-level `fn`'s return type**, as an OWNER-LESS fact — "calling this evaluates to
  that". `let entry = parse_entry(..); entry.path` has no receiver to read a type off anything
  else, and without the fact the type `parse_entry` returns looks used only where it is declared.
  `Self` is left alone there: a free function has no impl around it, so the name simply resolves
  to nothing.
- **`#[derive(Default)]`** → `(T, default) → T`. The derive states that the impl exists and the
  trait's signature states what it returns — the same curated-stdlib knowledge
  `is_machinery_trait` carries, and no derive means no fact.
- **A `for` variable** binds `{iterable}?0` — the element is parameter 0 of the collection's
  declared type, which the projection marker below already expresses. Only a CHAIN qualifies:
  a local's own annotation kept just its base name, and `Vec` alone has no parameters to project.

A CALL initializer types its binding by naming the FUNCTION, never by guessing its return: a bare
callee binds its own name, and a module-qualified one binds the pointer that matches what the
path's reconstructed import puts in scope — the trailing name for a `crate`/`self`/`super`-rooted
path (the import binds it), `module.function` for a bare-rooted one (the import binds the module,
and the core resolves that base through the qualifier table). Where the path crosses into type
space first (`crate::plugin::RootSink::default`), the type is what ends up in scope, so the
pointer is `RootSink.default` — the same split `emit_path` makes for the import itself. Member
accesses whose base is typed but whose own type is not locally knowable emit a one-hop
dotted POINTER qualifier — `low.context_separator.into_bytes()` → `LowArgs.context_separator`,
`Builder::new().opt(x)` → `Builder.new` — which the core resolves hop by hop through the
facts (and also credits the *yielded type* with a Read from the site: consuming a value
through a field IS a use of its type). Fact-first ordering: pointer, then the TypeEnv's
direct type, then the opaque receiver (duck fallback).

**The language's own generics (`builtin_member_types`)** — `Result<T, E>::map_err` still yields a
`Result` over the same `T`; a `Vec<T>` iterates to its `T`. Those are facts about types whose
declaration lives in the standard library, so no file here can emit them and they are declared on
the descriptor instead. Two names in that table are this adapter's own choice and mean nothing to
the core, which sees them as ordinary members: `@element` is the hop an iteration takes (a
container that declares none simply does not type its loop variable — a map iterates to a tuple,
which the model cannot name, and silence beats a confident wrong element), and `@slice` names the
anonymous slice/array type so it can carry an `@element` like any other container. The table is
deliberately small and grows only on measured evidence, the same discipline as the machinery-trait
list — it is a curated set of facts, not a model of the standard library.

**Payload unwrapping (`yields`'s arguments)**: an annotation is recorded as a TREE, not a base
name plus a flat list — `Result<Vec<TreeEntry>, GitError>` keeps the `TreeEntry` two levels down,
which is what a one-level list threw away and could never give back (`Box`/`Rc`/`Arc` are still
looked through, and a lifetime or fn type holds its POSITION as `Unknown` so `?N` keeps indexing
the arguments as written). An unwrapping operation marks its pointer hop
with `?N` — the index of the argument it extracts, landing on a whole subtree. The try operator extracts the success
payload, parameter 0 (`?` is shorthand for `?0`): `let chir = self.config.build_many(x)?`
binds `chir` to `Config.build_many?`, resolved by the core through that parameter instead
of the wrapper. WHICH index an operation projects is this adapter's knowledge — the core
only follows the marker. The `?` is the language's own operator (a fact);
`.unwrap()`/`.expect()` are plain method names and stay unmodeled (a curated
stdlib-semantics fact table is the future step).
Initializer bindings are pointer-first too (`let b = Builder::new()` binds `Builder.new` —
the declared return is the fact, the constructor-name convention only backs mid-chain
bases), `self.field` types through the file's own field facts, and pointer depth caps at
four segments (deeper chains fall to the duck fallback).

Untracked (deliberately, each a future fact-source, not a patch): `.unwrap()`/`.expect()`
(stdlib method semantics), `match`/`if let` bindings (the scrutinee's payload type is not
local), untyped closure params, and multi-bound generics.

### 3. Resolution

The specifier grammar is Rust paths; every step is arithmetic over `ResolveCtx`'s known
file set:

1. **Anchor.** `crate::` → the owning package's crate root: the nearest ancestor directory
   with a `Cargo.toml` whose `src/lib.rs` exists (else `src/main.rs`; both existing prefers
   `lib.rs` — bins re-import through the lib in every workspace kndo has to care about).
   When the convention misses entirely, the manifest's **declared targets** decide
   (`WorkspaceMember::targets`, fed by `[[bin]] path` / `[lib] path`): among the owning
   member's target files whose directory contains the importing file, the deepest wins and
   its directory is the anchor — ripgrep's root manifest declares
   `[[bin]] path = "crates/core/main.rs"`, and without this every `crate::` path in that
   tree was unresolvable and the whole subtree read as dead.
   `self::` → the importing file's own module directory (a *directory-owner* file — `mod.rs`,
   `lib.rs`, `main.rs` — owns its directory; a named file `a.rs` owns child directory `a/`).
   `super::` → one module step up, iterated.
2. **Walk.** Each path segment `s` descends: `<dir>/s.rs`, else `<dir>/s/mod.rs`, then the
   next segment against `<dir>/s/`. All hits `certain`.
3. **Two-step tail.** If the full path resolves to a file, done (module import). Else retry
   with the last segment dropped — the tail was an item; the binding resolves it inside the
   target file. One rule, no name-shape heuristics.
4. **Bare first segment** (no `crate`/`self`/`super`): `std`/`core`/`alloc`/`proc_macro`/
   `test` → `Stdlib`; a workspace member name → `WorkspaceMember` (entry, or a subpath
   walked into its `src/` — deep imports recorded); a declared dependency → `Dependency`.
   Name normalization applies **at lookup, in one direction**: the crate ident's `_` may
   stand for a declared name's `-` (`serde_json` ↔ `serde-json`); the *declared* form is
   what gets returned, so dependency hygiene cross-references correctly.
5. Anything else → `Unresolved`.

**Stdlib** is a hand-maintained five-line list (`std`, `core`, `alloc`, `proc_macro`,
`test`) rather than an `xtask gen-stdlib` pipeline, with the reason recorded: this is the
extern-prelude set, a language-stability guarantee — not toolchain data that drifts with
versions the way `go list std` or `builtinModules` do. If Rust ever adds a sysroot crate,
it is a one-line, reviewed change.

### 4. Manifests & packages (Cargo.toml, RFC 0011)

- Identity: `package.name`; `publish = false` → private (app mode). A pure
  `[workspace]` manifest (no `[package]`) contributes topology only.
- Workspace: `workspace.members` globs (`workspace.exclude` honored).
- Dependencies: `[dependencies]` → `Prod`, `[dev-dependencies]` → `Dev`,
  `[build-dependencies]` → `Build`, `target.'cfg(…)'.dependencies` → flattened into the
  same three. Version req from the string or table form; path deps without `version` get
  `"*"`. Path/workspace deps still register by *name* — sibling resolution is by name
  through the workspace index, same as npm workspaces.
- Roots: `src/main.rs` and `src/bin/*.rs` (autobins) plus `[[bin]] path` entries →
  `Production`, certain, unconditionally (a binary is an entry point, period). The lib
  entry (`src/lib.rs` or `[lib] path`) → `Production` root **only when the package is
  publishable** (library mode, RFC 0011 §5); an unpublished crate's `pub` API must earn its
  keep through actual imports. `build.rs` → `Tooling` root.
- Executables (`ManifestFacts::executables`, RFC 0005 §1's invoked-program rule): every bin
  root also registers under its cargo-assigned **name** — the package name for
  `src/main.rs`, the file stem for autobins, the declared `name` for `[[bin]]` entries.
  This is the identity `env!("CARGO_BIN_EXE_<name>")` invokes it by (see §2), letting a
  test that *executes* the binary — never imports it — reach the binary's whole call tree.
- `resolved_entries`: the lib entry — what a sibling's `use this_crate::…` resolves
  through.
- `declares_surface: false`, always: Cargo has no `exports` map; a Rust crate's surface IS
  its `pub` items, and reaching into `crate_x::internal::y` is legitimate when it's `pub`.
  The `deep-import` gate stays closed **by design**, not by omission.
- `script_invoked_names`: none (no scripts mechanism; `cargo alias`/xtask are out of scope
  for hygiene evidence in v1).

### 5. Cycle policy — where Rust genuinely differs from Go

```
file_cycles:    Idiomatic   (emits nothing)
package_cycles: Idiomatic   (emits nothing)
```

Module-level cycles inside a crate (`a` uses `b`, `b` uses `a`) are legal, common, and
compile fine — RFC 0005 §8's own example of Idiomatic. Package cycles are **not
`Impossible`**, and this is deliberate: cargo forbids cyclic `[dependencies]`, but
`[dev-dependencies]` cycles are legal and idiomatic (a crate dev-depending on a sibling
that depends on it, for integration tests), and kndo's package edges are derived from file
imports — which include test files. `Impossible` would misstate the language (the cycle
is real and legal, not an artifact); `Idiomatic` states the honest stance — and per RFC
0005 §8, a tolerated cycle emits nothing: it is information, not a defect.

### 6. Known hard cases & stances

| Case | Stance |
|------|--------|
| Macros defining items (`macro_rules!` expanding to `pub fn`) | invisible to the static graph; the *macro* stays alive via invocation references, its expansion products do not exist as symbols. Documented recall bound, same class as JS's property-assignment limit |
| Proc-macro crates | ordinary lib crates; `#[derive(X)]` references keep them used |
| Doc-tests (``` blocks in ///) | not parsed in v1 — they exercise the public API, which library mode already roots for publishable crates; for private crates a doc-test-only symbol reads `unused`, accepted |
| Trait-object / generic dispatch (`dyn Trait`, `T: Trait`) | `Implement` references from impls + the member fallback keep implementations alive when the trait is used — the RFC 0012 §3 dispatch rule, unchanged |
| `Deref` coercion method calls | member fallback (`probable`/`possible`), by construction |
| Re-exports of foreign crates (`pub use serde::…`) | dependency stays used; no file alias (nothing in-repo to alias) |
| `#[path]` on `mod` | honored, literal — `#[cfg_attr(cond, path = "…")]` included, and **every** alternate on one declaration is recorded (serde_derive_internals relocates its whole module with two `cfg_attr`s; reading only the bare spelling left the mod resolving to a nonexistent `internals.rs`, which took the module dark and made the `pub use internals::*;` beside it read as an undeclared crate) |
| Bare `use <mod>::…` beside a `#[path]`-relocated `mod <mod>;` | the 2015-edition spelling of `self::<mod>`, expanded to the same declared locations — a local module shadows any extern crate sharing its name, so the relocation is the only reading |
| Same name, `mod x;` and `fn x` | legal Rust; the two-step resolver prefers the module file — accepted approximation |
| Editions | 2018+ path semantics assumed; edition 2015's bare crate-relative paths would read as dependencies → `Unresolved`, degrading to silence, never to a wrong accusation |
| `no_std` crates | nothing special — `core`/`alloc` are in the stdlib list |

### 7. Open questions

1. Should `xtask/**` tooling-role classification instead key off workspace metadata
   (`[workspace.metadata]`) rather than the directory convention? Convention chosen for v1;
   revisit if a real repo uses `xtask` as a lib name.
2. Field-level liveness (`struct` fields as members) — deferred; needs `Field` symbol
   emission plus read/write reference discrimination to be useful, and the noise risk is
   high until then.
3. `cargo metadata`-grade target discovery (`[lib] name`, `autotests = false`, …) — v1
   reads the manifest textually and honors the common keys; the exotic ones degrade to
   convention defaults, recorded here rather than silently.

## Swift

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M5
**Grammar:** tree-sitter-swift

The sixth adapter, and the first whose manifest (`Package.swift`) is not a data format at
all — it's Swift *source code*, executed by `swift-tools-version`-selected SwiftPM at build
time. This adapter never executes it; `manifest.rs` parses it with the **same tree-sitter-swift
grammar** extraction uses, walking the `Package(...)` initializer call's labeled arguments as
data — the SPM analogue of Rust's structured `Cargo.toml` parse, not Gradle's line-scan (there's
no "compute anything" concern here the way Groovy/Kotlin DSL raises for Gradle, since only the
literal argument shapes SwiftPM itself requires are read; anything genuinely dynamic in a real
`Package.swift` — which is rare, SwiftPM manifests are conventionally declarative — is invisible,
not misparsed, same honesty as every other best-effort manifest reader in this codebase). Like
Java/Kotlin, no dogfood corpus of its own; precision rests on the conformance fixtures (§6).
Node kinds throughout this doc are pinned against real tree-sitter-swift 0.7.3 output
(`kndo-adapter-swift/src/parsing.rs`'s `#[ignore]`d ground-truth dumps), not guessed.

### 0. What's structurally different from Java/Kotlin, and why it matters here

- **The resolution unit is the SPM *target*, not the file's own declared namespace.** Swift has
  no `package`/namespace statement at all — every file in a target implicitly shares that
  target's single flat namespace (any file can reference any other file's `internal`+
  declarations in the same target without an import). `FileFacts::unit` is therefore **not**
  declared in source the way Java/Kotlin's is; it's derived from the SwiftPM Standard Directory
  Layout: the path segment immediately following `Sources/` or `Tests/` (`Sources/MyLib/Deep/
  File.swift` → unit `MyLib`). A file outside both conventions (rare — a loose top-level script,
  a nonstandard layout) gets `unit: None`, same safe-direction fallback every other adapter uses
  for "can't place this file's identity." RFC 0012 §8 already records this convention.
- **A five-rung visibility ladder that applies uniformly to top-level AND member declarations —
  no restricted top-level subset, unlike Java/Kotlin.** Swift permits `private`/`fileprivate`/
  `internal`/`public`/`open` on *any* declaration, top-level or member (Java disallows top-level
  `private`/`protected`; Kotlin disallows top-level `protected`). One ladder, no per-position
  carve-out: `[File "private", File "fileprivate", Package "internal", Public "public", Public
  "open"]` (RFC 0012 §6, already recorded). `private` is real-Swift narrower than kndo's `File`
  scope (it's scoped to the enclosing declaration/extension, not the whole file) — no scope in
  `{File, Unit, Package, Public}` represents that, so it widens to `File`, same conservative-
  widening reasoning as every other adapter's tightest-unavailable-scope case. `fileprivate` is
  an **exact** match to `File`. `internal` — Swift's **default when no modifier is written at
  all** — maps to `Package` (kndo's "same manifest" granularity, here an SPM target): a real,
  structural difference from Java (default = `Unit`-equivalent) and Kotlin (default = `Public`)
  — the third distinct default-visibility convention among this session's three adapters, each
  requiring its own careful mapping rather than a shared reflex. `public`/`open` both widen to
  `Public` (the difference — `open` alone permits cross-module subclassing/overriding — has no
  representation in kndo's four-scope model, same "can't distinguish, so don't try" stance
  Java's `protected`-widens-to-`Public` already establishes).
- **`member_of` covers `extension` too — the one member-owner shape no other adapter has.** RFC
  0012 §3 already records this: "members of struct/class/enum/protocol/**extension** (owner =
  extended type's name)". An `extension Widget { func extra() {} }` block adds `extra` as a
  genuine member of `Widget` even though it's declared in a completely different file (possibly
  a different target, extending a type from an imported module) — extraction attributes
  `member_of` to the extended type's bare name exactly the same way a same-file member would,
  no special-casing needed once the grammar's `class_declaration{declaration_kind: extension}`
  shape is recognized (§1). `struct`/`class`/`enum`/`extension` share **one** tree-sitter node
  kind (`class_declaration`, distinguished by its `declaration_kind` field) — protocols alone
  get a distinct `protocol_declaration` node.
- **No reliable import→dependency-coordinate mapping — a different root cause from Java's, same
  outcome.** Unlike Java (whose import namespace has *zero* structural relationship to a Maven
  coordinate), a Swift `import Alamofire` names a **module**, and SwiftPM module names usually
  *do* match their declaring target/product name — but `Package.swift` only declares the
  dependency's **repository URL** (`.package(url: "https://github.com/Alamofire/Alamofire.git",
  from: "5.0.0")`), never the module/product name(s) that repository exports; those live in
  *that* repository's own `Package.swift`, which kndo — a static source analyzer with no
  network access — structurally never reads. Same consequence as Java (§0 there):
  `resolve()` never returns `Resolution::Dependency` for an external import, and
  `AdapterDescriptor.resolves_dependency_usage: false` makes `dependency_hygiene` skip Swift's
  `unused`/`test-only` dependency verdicts (one diagnostic, not a flood). **Local target-to-
  target imports are a different, fully-solved question** — `import MyLibCore` from a sibling
  target in the *same* `Package.swift` resolves exactly like Java's same-`unit` fallback, since
  SwiftPM module names are declared LOCALLY as target names right there in the manifest being
  read (§3). `version-skew` is unaffected (pure manifest comparison, no usage edge needed).
- **Root promotion has two independent triggers, neither manifest-`private`-gated the way
  Java/Kotlin's is.** `@main`-attributed types (the modern SE-0281 program-entry-point
  attribute) root unconditionally wherever they appear. A file named exactly `main.swift`
  (SwiftPM's older, still-supported convention: the **one** file in an executable target
  permitted to hold unwrapped top-level statements) roots as a **whole file** — its top-level
  code runs unconditionally at process start, the same "load-time, no separate declaration to
  hang a root on" shape Rust's `fn main` XOR JS's entry-file promotion each handle differently;
  here it's neither a single function nor an arbitrary manifest-declared entry, it's a filename
  convention SwiftPM itself enforces (at most one `main.swift` per executable target). Library-
  target root promotion (every `.swift` file under a **publicly exported** target's `Sources/`
  tree) is manifest-driven, same mechanism as Java/Kotlin (§4) — but scoped **per target**, not
  per whole package, since one `Package.swift` can declare several targets with different
  public/internal-only status via which ones appear in a `.library(...)` product's `targets:`
  list.
- **`override` dispatch rooting, same shape as Kotlin's.** A member marked `override` — the
  Swift keyword, structurally identical to Kotlin's `member_modifier` wrapper shape — implements
  a superclass/protocol-required method that framework/OS code (UIKit/SwiftUI lifecycle hooks,
  protocol-witness dispatch) may invoke without any named call site in this codebase. Every
  `override`-modified member roots `Production`/`Probable`, unconditionally, same blanket stance
  as Java's `@Override` and Kotlin's `override`.
- **No inline test regions — `Tests/<Target>/**` is the authoritative signal**, same Standard-
  Directory-Layout stance as every JVM-family adapter this session (RFC 0002 §7 already records
  this). XCTest's own naming convention (`*Tests.swift`, mirroring `class FooTests: XCTestCase`)
  is a belt-and-suspenders filename fallback for non-standard layouts, same shape as Java's
  Surefire fallback.

### 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `swift` | `**/*.swift` |
| Manifests | `**/Package.swift` only — SwiftPM has no secondary/workspace-topology manifest file the way `settings.gradle` is to `build.gradle` (a package's `targets:` array already *is* its full local topology, §4) |
| Role `test` | `Tests/**` (SwiftPM Standard Directory Layout) OR a bare filename matching XCTest's own convention (`*Tests.swift`) — an OR, not additive, same reasoning as every other adapter's dual-signal role check |
| Role `tooling` | not detected — no ecosystem-wide config-file convention for Swift source files exists (same stance as every prior adapter) |
| Origin `generated` | `// Generated by` / `// This file was generated` banner-style `Contains` markers (sourcery, swiftgen, and similar codegen tools all wrap the marker in an ordinary `//` comment) — `comment_openers: ["//", "/*", "*"]`, the same C-family set every prior `Contains`-based adapter declares (contracts §7, and the toolkit-level self-reference fix this session's Kotlin work landed) |
| Origin `vendored` | `vendor/**`, `third_party/**` (toolkit universal list) — inert but consistent, same as every JVM-family adapter; SwiftPM's own dependency cache (`.build/checkouts/**`) is excluded from discovery already (a project's own `.gitignore` universally excludes `.build/`, same "OUT_DIR" stance every adapter's doc states for its own build directory) |

**`VisibilityLevel`**: `0` (private, widened to `File`), `1` (fileprivate, exact `File`), `2`
(internal — the default when no modifier is present at all — `Package`), `3` (public, `Public`),
`4` (open, `Public`). §0 has the full ladder derivation; unlike Java/Kotlin, every level applies
at every declaration position (top-level and member alike), so there is no restricted-subset
note to make here.

### 2. Extraction

**Declarations**: `class_declaration` is the single unified node for struct/class/enum/
extension — `SymbolKind` derives from the `declaration_kind` field (`struct`/`class` →
`SymbolKind::Struct`/`Class` respectively; `enum` → `SymbolKind::Enum`; `extension` contributes
**no declaration of its own** — same "no name to hang a finding on" stance as Java's anonymous
classes and Kotlin's companion objects, since an extension block's own identity isn't a
meaningful liveness question, only its *members'* are). `protocol_declaration` →
`SymbolKind::Interface` (Swift's protocol is the interface-shaped construct in every other
adapter's vocabulary; kndo has no distinct "protocol" facet, and `Interface`'s semantics — a
contract type, never instantiated directly — line up exactly). `enum_entry` (inside
`enum_class_body`) → `SymbolKind::EnumMember`. `function_declaration` → `SymbolKind::Method`
when lexically nested in a `class_body`/`protocol_body`, else `SymbolKind::Function`. A
protocol's method requirement is its own distinct node kind, `protocol_function_declaration`
(no `function_body` field at all — a requirement has a signature, never an implementation) —
dispatched through the same handler, which already treats the body as optional.
`init_declaration` → `SymbolKind::Method` named `<init>` (constructor overloads collapse to one
liveness unit, same coarse-graining as Java/Kotlin's multi-constructor stance).
`property_declaration` → `SymbolKind::Field` (member) or `SymbolKind::Variable` (top-level —
matching Go's top-level-var stance); a `let a, b: Int` multi-binding pattern is walked per
`pattern` child, one `Declaration` each (matching Go's grouped-`var` and Java's grouped-field
stance: one physical binding, one declaration). `typealias_declaration` →
`SymbolKind::TypeAlias`. Computed properties (`var x: Int { get { … } set { … } }`) and property
observers (`willSet`/`didSet`) are not independently declared — their `computed_getter`/
`computed_setter`/`willset_didset_block` bodies are walked as part of the OWNING property's own
declaration span, matching the "one physical declaration, one liveness unit" principle (a
computed property's getter/setter aren't separately callable by name from user source; treating
them as sub-bodies of the property avoids inventing declarations nothing can reference by name).

**References**: `call_expression` with a `simple_identifier` callee → bare `Call`; a
`navigation_expression` callee (`a.b.c()`) → `Call` on the terminal `navigation_suffix`'s name
with `scope_context` set to the qualifier when it's a plain `simple_identifier`/`self_expression`
(`self` → literal `"self"`, mirroring Java's `this`), else the qualifier chain is walked
recursively for its own references — the same qualifier-vs-complex-receiver split Kotlin's
`emit_navigation_ref` already establishes, reused near-verbatim here since the grammar shapes
line up closely (`navigation_expression{target, suffix: navigation_suffix{suffix}}` vs Kotlin's
flatter `navigation_expression` child list — the field names do the work here instead of
positional last-two-children logic). `inheritance_specifier`'s `inherits_from: user_type` →
`Extend` (superclass and protocol conformance share one syntax list in Swift, so no
extends-vs-implements split is needed the way Java's grammar forces). `user_type`/
`optional_type` positions (parameter/return/property types, `is`/`as` checks, generic
constraints) → `TypeUse`. Closures (`lambda_literal`) are walked like any other expression body
— their own parameter list shadows outer bindings the same safe-direction way every adapter's
closure handling already works.

**Top-level code**: `source_file`'s children aren't only declarations — any statement can sit
directly at file scope (SwiftPM's "top-level code" file shape, §0), so a bare `helper.live()`
next to a top-level `let` is walked as body content exactly like a function body, or it would
leave no reference behind at all. **`within` for top-level globals** (RFC 0012 §4): a `let`/`var`
declared at file scope is *always lazily-initialized* by the Swift language itself (its
initializer, computed getter, and observers only run on first access/each access, never at
"module load") — so `within` names the global's own symbol, not `None`. `main.swift` is the one
exception: its top-level code is a script, executing procedurally at process start, so references
there keep the ordinary `None` ("runs when the file loads") attribution.

**Roots (`RawRoot`)**: `@main`-attributed declarations → `RootKind::Production` at `Certain`
(structural, not a heuristic the way `main()`-by-name is for every other adapter — the attribute
is SwiftPM/the Swift compiler's own authoritative entry-point marker, so this is the one adapter
whose entry-point root reaches `Certain` rather than `Probable`). A file named exactly
`main.swift` → `RootKind::Production` at `Certain`, `RawRootTarget::WholeFile` (§0's "top-level
code, not a single function" shape). `override`-modified members → `Production`/`Probable`
(§0's dispatch rule).

**Suppressions**: `// kndo:allow …` / `/* kndo:allow … */` — identical syntax to every other
adapter (RFC 0005 §12 is language-neutral).

**Metrics**: cyclomatic complexity +1 per `if_statement`, `guard_statement` (an implicit early-
return conditional — counted, same reasoning `if` is), `switch_entry` (one per case group past
the implicit base, matching every other adapter's n-way-match rule — `default` is itself one
more ordinary `switch_entry`, no special-casing), `for_statement`, `while_statement`,
`catch_block` (one per `catch` clause, **not** `do_statement` itself — matching Java's own
try/catch convention exactly: entering a `do` block always happens, only a `catch` clause is a
genuine alternate path), `&&`/`||` (leaf tokens, same shape as Java/Kotlin's). Nil-coalescing
(`??`) and force-unwrap (`!`) are **not** branches — value-producing fallback/assertion
operators, not control-flow forks, same non-branch stance as Kotlin's elvis/not-null operators.
Each `lambda_literal` clearing the clone floor becomes its own callable **shape**
(`MetricsSyntax::nested_callable_kinds` — its branches and tokens leave the enclosing shape's
stream, which keeps one `FN` in their place, and `crap`/`duplicate` report it in its own
right). A smaller one stays an expression inside its owner: promoting it would leave both
halves under the floor and cost real clone findings — measured, that was 83 clone participants
on the field corpus. The split's semantics are uniform across adapters; only the node kinds
that trigger it are per-language.

`MetricsSyntax::construction_kinds` is deliberately **empty** here: constructing a value in
Swift is an ordinary `call_expression`, indistinguishable from any other call, so this adapter
has nothing true to report and `duplicate`'s construction exemption simply never fires for
Swift — today's behaviour, unchanged. Guessing (an uppercase callee, say) would be the adapter
inventing a verdict, in the accusation direction RFC 0012 §2 forbids.

**Properties**: a property with an accessor BODY is a callable, not a value — `Method` when it
has an owner, `Function` at top level; a stored one stays `Field`/`Variable`. Swift compiles such
a property to a getter, so this is the truthful kind, and the distinguishing node is `computed_property`.
A bodyless accessor (a `private set`, an annotated bare `get`, `willSet`/`didSet` observers)
leaves the property stored: it changes the accessor, not what the property IS.

Being a callable, it also gets a **shape**: `FileFacts::functions` carries one entry per accessor
body, so `crap` and `duplicate` can see a getter the way they see a method. One symbol, one
numbering — the first accessor in source order is `shape_ordinal` 0 and the rest continue, which
is what keeps `get` and `set` from colliding on a nested shape's identity. Verified against
tree-sitter-swift 0.7.3's `node-types.json`: a `computed_property` holds either a bare
`statements` (the implicit-getter shorthand `var x: Int { 1 + 2 }`) or one
`computed_getter`/`computed_setter`/`computed_modify` each with its own. `willSet`/`didSet` get
no shape: they run *around* a store, so the property is still stored and there is no getter to
measure.


Naming both `Field` made the kind unable to separate a constant from real logic, which is what
let `untested` accuse header-name constants and `MAX_VARCHAR_LENGTH` of not being tested.

**Grammar ground truth**: pinned in `kndo-adapter-swift/src/parsing.rs`'s `#[ignore]`d probe
tests — re-run with `--ignored --nocapture` before any tree-sitter-swift version bump.

### 3. Imports & resolution

Swift's `import Foundation` / `import struct Foundation.Date` (a scoped submodule-member import,
rare in practice) both emit specifier = the leading module name (`Foundation`), `ImportKind::
Package`, `Confidence::Certain` (an import that doesn't resolve is either genuinely external or
a compile error, never a maybe).

**Resolution algorithm** (the adapter's `resolve`):

1. **Same-target (no import needed).** Handled entirely by the core's `unit`-based fallback —
   any file under the same `Sources/<Target>/**` tree resolves siblings without an import,
   never reaching this adapter's `resolve()` at all, identical shape to Java's same-package
   fallback.
2. **A locally-declared target/module name.** Look up against the known-units index (built from
   every claimed file's `unit` — §0's Sources/Tests-derived target name) — a hit resolves
   `Resolution::File` at the target's first file in path order, same algorithm as Java's
   package-to-file resolution.
3. **`Foundation`, `Swift`, `Combine`, `Dispatch`, and the rest of the platform SDK.**
   `Resolution::Stdlib` — these ship with the toolchain/OS, not a package dependency; matched
   against a small fixed prefix table (§7 — this list is inherently non-exhaustive across
   Apple's full SDK surface, a documented, narrow gap, same shape as any stdlib-list adapter's
   list-maintenance burden).
4. **Anything else (an external SPM dependency's module name).** `Resolution::Unresolved` —
   never `Resolution::Dependency` (§0's last bullet: the local `Package.swift` doesn't state
   what module name(s) a `.package(url:)` entry exports).

### 4. Manifests & packages (RFC 0011)

**`Package.swift` is parsed as Swift source**, not a data format — `manifest.rs` calls the same
`crate::parsing::parse` extraction uses, then walks the `let package = Package(…)`
`call_expression`'s `value_arguments`, matched by each `value_argument`'s `name` field
(`value_argument_label`) rather than positionally (SwiftPM itself requires every `Package(…)`
argument to be labeled, so this is exact, not a heuristic):

| Argument | Extracted as |
|---|---|
| `name:` (string literal) | `ManifestFacts::package_name` |
| `products:` (array of `.library(name:, targets:)` / `.executable(…)` calls) | `private = true` iff no `.library(…)` product entry exists at all (an executable-only or product-less package exports nothing importable, same "packaging says nothing is published" reasoning as Java's war/pom packaging check) |
| `dependencies:` (array of `.package(url:, from:/exact:/branch:/revision:/…)` calls) | one `ManifestDependency` per entry — `name` = the URL's last path segment with a trailing `.git` stripped (`https://github.com/Alamofire/Alamofire.git` → `Alamofire`; a best-effort identity, since the repo name and the module(s) it exports aren't guaranteed identical — §0's last bullet), `version_req` = whichever of `from:`/`exact:`/`branch:`/`revision:` is present (first match, string value as-is — no semver comparison attempted beyond what `version-skew` already does generically), `scope` = `Prod` always (SwiftPM's dependency model has no Maven-style compile/test/provided split at the declaration site — a documented v1 simplification, §7) |
| `targets:` (array of `.target(name:, dependencies:)` / `.testTarget(…)` / `.executableTarget(…)` / others) | `ManifestFacts::workspace_members`, one entry per target `name:` — RFC 0011 §3's local topology; a target's own `dependencies:` array (naming other local targets or external product names) is **not** cross-referenced against the top-level `dependencies:` list in v1 (§7) |

**Conformance-witness roots (M6, Alamofire corpus)**: a non-private method of a type (or
extension) that declares any inheritance/conformance entry roots `Production`/`Possible` — it
may witness a protocol requirement invoked by machinery outside the repo (a custom
`KeyedEncodingContainerProtocol`'s methods are called by the stdlib's Codable synthesis; no
source call site can exist). External protocols' requirements aren't statically enumerable, so
witness-vs-dead is undecidable — degrade toward silence at the `Possible` dynamic tier, the
same reasoning as `override` rooting but one confidence rung lower. `deinit` extracts as a
`Constructor`-kind `<deinit>` member: runtime-invoked, liveness follows the type, body walked.

**Root promotion**: for every target named in a `.library(…)` product's `targets:` list (i.e.
publicly exported, not merely locally declared) and not itself a test target, one
`ManifestRoot{Production, Certain}` per non-test `.swift` file under the target's source tree —
its explicit `path:` argument when declared (Alamofire's `.target(name: "Alamofire", path:
"Source")`, M6 FP hunt), `Sources/<TargetName>/**` under the Standard Directory Layout otherwise
— reusing `ResolveCtx::files_under` exactly like Java/Kotlin's mechanism (docs/adapters/
java.md §4), just parameterized per-target instead of once per manifest (a `Package.swift` can
declare several independently-public-or-not targets, unlike a Maven/Gradle module's single
private/public flag). Non-exported targets (declared but never listed in any `.library(…)`
product's `targets:`) get **no** root promotion — their liveness depends entirely on whether an
exported target's code actually imports and uses them, exactly the "no manifest-declared
surface, ordinary reachability applies" default every adapter falls back to.

### 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| Extension member liveness (`extension Widget { func extra() {} }`) | `extra`'s `member_of` is `Widget`'s bare name; the extension block itself has no declaration of its own (§2) — same "no name to hang a finding on" stance as Kotlin's companion objects |
| Computed properties / property observers | getter/setter/willSet/didSet bodies walked as part of the owning property's declaration span, not independently declared (§2) — a call to the property (`widget.x`) is an ordinary member reference; the get/set dispatch itself is invisible, same class of gap as Java/Kotlin's compiler-synthesized-accessor stance |
| `open` vs `public` | both widen to `VisibilityScope::Public` — kndo's model can't represent "public but not subclassable outside the module," same conservative-collapse Java's `protected`-to-`Public` already establishes |
| Local target-to-target dependency graph vs the manifest's flat `dependencies:` list | not cross-referenced in v1 (§4, §7) — a target's own SPM-declared local dependencies don't currently narrow which external packages *it specifically* needs; every declared external dependency is treated as available project-wide for resolution purposes (same as how every prior adapter's dependency list is project-wide, not per-file-scoped) |
| `@available`/platform-conditional code (`#if os(iOS)`) | not modeled — a documented non-goal (§7), same class as every adapter's stance on preprocessor-style conditional compilation |
| Result builders (`@ViewBuilder`, SwiftUI's declarative body syntax) | walked as ordinary expression bodies — no special understanding of builder-transformed control flow, same "don't try to out-think a DSL" stance macro-heavy Rust code already takes |
| Property wrappers (`@State`, `@Published`, …) | the wrapped property is an ordinary `Field`/`Variable` declaration; the wrapper attribute itself contributes no extraction-visible behavior change |

### 6. Conformance fixtures (shared harness, RFC 0002 §8)

Four fixtures, each a real SwiftPM package tree run through the real `Engine` (no mock),
mirroring the Java/Kotlin fixture sets so precision is comparable apples-to-apples:

- **`dead-code-same-target`** — a `Package.swift` with no `.library(…)` product (private, no
  root promotion): `main.swift` calls `Helper().live()` with no import (same-target `unit`
  resolution) while `Helper.dead()` is never called — `dead` reads `unused`, `live` doesn't.
  (The top-level `let helper = Helper()` itself also reads `internal-only`: a default-`internal`
  global referenced only from its own file — §2's lazy-global `within` note doesn't change this,
  since the reference sits in `main.swift`'s own script code either way.)
- **`dispatch-and-extension`** — `Impl` **subclasses** `Base` and overrides `go()`; `main.swift`
  calls `impl1.go()` by name. Because `Base` and `Impl` both declare a member named `go`, the
  call is ambiguous by name alone (extraction carries no receiver types) and lands on RFC 0012
  §3's duck-typed member fallback: `Base.go` picks up a `Possible`-confidence candidate edge
  too, which is *not* strong enough to justify any visibility rung — it reads `internal-only`
  rather than `unused`, correctly conservative given a same-named override genuinely exists. A
  separate `extension` block adds a `public` member to a type declared in another file, proving
  cross-file `member_of` attribution for extensions works — the one member-owner shape unique to
  this adapter (§0's third bullet) — and that its visibility is checked like any other member's.
- **`visibility-ladder-and-internal-default`** — one target, all five ladder rungs exercised on
  a class's members (`private`/`fileprivate`/`internal`/`public`/`open`) plus one member with
  **no modifier at all** (`useOwn`), proving the no-modifier-means-internal default (§0) computes
  the same `Package`-scope tightest-sufficient check as an explicit `internal` would — neither
  reads `internal-only` when used only from within the same target, while the `public`/`open`
  members do (declared wider than their same-target-only usage requires).
- **`package-swift-dependency-skip`** — a `Package.swift` declaring one `.package(url:, from:)`
  external dependency never imported anywhere: the `unused`-dependency verdict that would
  otherwise fire is skipped instead (`resolves_dependency_usage: false`, one diagnostic —
  §0/§4), proving a declared-but-unimported SPM dependency is never misreported as dead.

### 7. Open questions

1. `Foundation`/`Swift`/platform-SDK stdlib prefix list (§3 point 3) — inherently non-exhaustive
   against Apple's full SDK surface; parked as a small fixed table, widened only if real
   dogfooding on a third-party Swift corpus surfaces a false `undeclared`.
2. Target-to-target local dependency graph cross-referencing against the top-level
   `dependencies:` list (§4, §5) — every declared external dependency is currently treated as
   available project-wide rather than scoped to the targets that actually list it; a contained
   follow-up, not attempted in v1.
3. Per-dependency scope fidelity (`Prod` always, §4) — SwiftPM's manifest has no compile/test/
   provided split at the `.package()` declaration site the way Maven/Gradle do; a `testTarget`-
   only dependency currently reads identically to a library-wide one for scope purposes.
4. Swift Package Manager plugin targets (`.plugin(…)`), macro targets (`.macro(…)`), and binary
   targets (`.binaryTarget(…)`) — parsed as ordinary `targets:` entries contributing to
   `workspace_members`, but not given any special root-promotion or dependency-resolution
   treatment; real but likely rare in application code, parked pending signal.
5. `@available`/`#if`-gated conditional compilation (§5) — entirely unmodeled, same non-goal
   class as every adapter's preprocessor stance.

## HTML

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M6
**Grammar:** none — a tag scan, not tree-sitter

The ninth language, and the first built from a measurement rather than a launch-set plan:
`internal/detection-gaps.md` §20 traced 831 of vite's findings to one missing fact — nothing
claimed `.html`, so nothing rooted the `<script type="module" src="./main.js">` every bundled
web app uses as its real entry point — and §20-bis is the record of building, measuring, and
fixing what that fix itself broke. `kndo-adapter-html` is ~250 lines
(`kndo-adapter-html/src/{lib,extraction,resolution}.rs`): claim `.html`/`.htm`, root the
document, tag-scan five attributes for local file references. Like JSON, it is RFC 0002 §3's
"non-source" shape — no symbols, no visibility ladder, no metrics — but unlike JSON it emits
real imports and a real root of its own; a `.json` file is purely a target other languages
point at, an `.html` file is where a project's reachability graph starts.

### 0. What's structurally different from every prior adapter, and why it matters here

**A document is an entry point, not a module.** Nothing imports a page: a browser loads it, a
server renders it, a bundler is handed it. Every other file this adapter can name is reachable
*through* it, never the other way around — so the document roots itself unconditionally, and
`<script src>`/`<link href>`/`<img src>`/`<source src>`/`<iframe src>` become the edges leading
out. That one rule is the whole adapter; everything below is either the tag scan that finds
those edges or a stance on what the scan deliberately does not attempt.

**Why this is an adapter and not a plugin (`internal/detection-gaps.md` §20).** The RFC 0003/
RFC 0002 §2 boundary is "language spec vs. ecosystem convention" — `<script src>` is HTML's own
mechanism for naming another file, no more vite's property than `import` is webpack's, and it
resolves identically whether the page is served by vite, webpack, parcel, esbuild, or nothing at
all. A `kndo:vite` plugin reading `vite.config.*` was measured first and rejected: of vite's own
`playground/*` configs, only 11 of 61 declare an entry at all, and each does it through a
computed `path.resolve(dirname, './index.html')` expression — the same "the config is a program"
problem Gradle's build scripts pose — while the actual cost (831 findings, 65 of 83 module
scripts reading `unused`) sits one directory up, in the `.html` file the config merely points
at. Fixing the plugin's target would have closed a minority of cases while leaving the
HTML-level mechanism that produces them unmodeled.

**Why `claim`, not `claim_manifest`.** `claim_manifest` was tried first and rejected on a
structural fact, not a style preference: assembly creates one `PackageNode` per claimed
manifest (`graph/assemble.rs`, ownership by nearest-manifest-ancestor), so every directory
holding an `.html` file would have become its own package — taking package-scoped unit keys,
dependency ownership, and surface promotion with it. An HTML document has none of that: it
names files, it doesn't declare a package. `claim`, the same mechanism CSS/JSON use, is the
shape that fits.

**The `untested` flood this exposed, and the descriptor field that fixed it (§20-bis).** The
first measurement of the rooting rule alone was net *negative*: −237 `unused` (the recall it
exists for) but +418 additions, 145 of them `untested` on the `.html` files themselves — a page
is a production entry point by nature, so without an exemption every document in every web
project reads as a permanent test blind spot. The existing exemption
(`files_declaring_only_values`) couldn't reach this: it requires a file to have declared
symbols, deliberately, so concluding "nothing to test" from an *absence* of extracted facts
would silence a file for a reason nobody could see — indistinguishable from an adapter that
silently failed. The adapter states its stance positively instead:
`AdapterDescriptor::declares_units_of_testing: false`, carried onto `ProjectGraph
::testable_languages` alongside the visibility ladders. `untested` consults it for exactly one
case — a file that declares nothing, in a language that says nothing is declarable — so a
`.scss` file that *does* declare a `@function` is untouched: the values-only rule still decides
it, no blanket "documents aren't testable" rule involved. Measured with the exemption in place:
vite 1886 → 1848 (248 removed, 210 added — the additions carry no HTML at all, they're the
same dead-to-judged category shift Java's guava fix produced), spring-petclinic 44 → 41 (three
declaration-less `.scss` files, the same pre-existing gap surfacing in a project with no HTML),
axios untouched.

### 1. Claiming & classification

| Claim | Files |
|-------|-------|
| Language `html` | `**/*.html`, `**/*.htm` |
| Manifests | none — `manifest_globs: vec![]`, `claim_manifest()` always `false` (§0's rejected-first-choice paragraph) |
| Role `test` | not detected — every claimed document is `FileRole::Production` unconditionally; a page is a deliverable by nature, and no `*.test.html` ecosystem convention exists to encode instead |
| Role `tooling` | not detected — no ecosystem-wide config-file convention for HTML documents exists (same stance as every prior adapter's tooling-role gap) |
| Origin `generated` | not detected — `origin: FileOrigin::Authored` unconditionally. HTML has real comment syntax (`<!-- … -->`), unlike JSON, so the toolkit's `ContentMarkers` mechanism every other comment-capable adapter uses is a plausible small addition; not built because no generator-banner convention has surfaced in measurement the way Go's `// Code generated` marker did (§7) |
| Origin `vendored` | not applicable — no `vendor/**`/`third_party/**` convention for HTML source; the toolkit's universal list is inert here the same way it is for Java's non-vendored-dependency stance |

No path-pattern-driven role/origin variation, same posture as JSON's constant-classification
stance: `claim()` returns `Production`/`Authored` unconditionally, no `PathPatterns` call
involved. **No visibility ladder** (`vec![]` — HTML has no visibility semantics; visibility
analyses skip its files entirely, the same `[]` JSON's own descriptor declares). **Cycle
policy**: `file_cycles`/`package_cycles` both `Idiomatic` — a page referencing a page (or a
component fragment referencing its parent) is a link, not a structural defect, and per RFC 0005
§8 a tolerated cycle emits nothing. **`resolves_dependency_usage: false`**: this adapter
contributes no manifest and no `PackageNode`, so `dependency_hygiene` never consults the flag
for it — recorded for completeness, not because a real ambiguity exists the way Java's
coordinate-mapping gap does. **`declares_units_of_testing: false`**: §0's central fix — the
descriptor field this adapter's own measurement required inventing.

### 2. Extraction

**The document roots itself, unconditionally, on every claimed file.** One
`RawRoot { kind: Production, target: WholeFile, confidence: Certain }`, pushed before any
tag scan runs and regardless of what (if anything) the scan finds — even a document with zero
references still roots itself (§0's whole argument: an empty page is still something a browser
loads). There is no per-file exception: unlike Go's `func main` or Rust's `fn main`, which root
one function inside a file, an HTML document has no finer-grained entry-point concept to root
instead — the *whole file* is what a browser fetches.

**Reference extraction is a tag scan, not a grammar** — deliberately no tree-sitter dependency
the way every code-extracting adapter has one (ADR 0002's escape hatch, the same one JSON/CSS's
"no grammar earns its keep" reasoning already establishes, here for a different cause: HTML's
own error-recovery model means a "malformed" document is still one a browser renders, so a real
parse tree would buy structure nothing downstream reads). Five `(tag, attribute)` pairs are
scanned, uniformly, no filtering by any other attribute on the same tag (`<link>`'s scan doesn't
key off `rel` — a stylesheet, an icon, a preload hint, and a web-app manifest are all followed
identically):

| Tag | Attribute |
|-----|-----------|
| `script` | `src` |
| `link` | `href` |
| `img` | `src` |
| `source` | `src` |
| `iframe` | `src` |

`script`/`src` is the one the adapter exists for (§0); the other four are the same fact in
other tags, costing nothing extra once the scanner exists. **Not scanned**: `srcset` (multi-value,
`<img srcset="a.jpg 1x, b.jpg 2x">`), `<object data>`, `<embed src>`, `<video>`/`<audio src>`,
`<track src>`, `<use href>` (SVG), `<form action>` — a documented recall gap (§7), not an
oversight; each would need its own value-shape handling (`srcset`'s comma-separated
candidate list, in particular, is a different grammar from a single-value attribute) rather
than a sixth uniform table row.

**A reference is emitted only when it plainly names a local file.** `local_reference` skips,
rather than reports as unresolved: a scheme (`https:`, `data:`, `mailto:`), a protocol-relative
URL (`//cdn…`), a bare fragment (`#top`), and a template placeholder (`${base}/app.js`,
`{{ url_for(...) }}`) — none of these name a file in this project, and treating them as
unresolved references would turn every page into a source of noise. A root-relative path
(`/assets/app.js`) is skipped too, for a sharper reason: what it names depends on the server's
document root, which a static read of the source tree cannot know. A query string or fragment
on an otherwise-local path is stripped before matching (`./main.js?v=2` is `./main.js` on disk).
Every surviving reference becomes one `RawImport`: `kind: Relative`, `side_effect_only: true`
(a page does not bind a name from what it loads — naming the file *is* the use, the same shape
as JS's `import "./polyfill"`), `confidence: Certain`. No suppressions, no metrics, no
declarations, no `unit` — there is nothing here for `kndo:allow` to attach to or for a
complexity metric to measure.

**The scan is written to under-report, not to guess.** Attribute values may be double-quoted,
single-quoted, or unquoted — all three are valid HTML, and a scanner that handled only the
first would silently miss real references (`<script src=./b.js>` is exactly as real as
`<script src="./b.js">`). Matching is case-insensitive on the tag and attribute name (`<SCRIPT
SRC=…>` matches) but preserves the value's original case. Both the tag-name and attribute-name
matches guard against substring collisions: `<script` must not match `<scripting-thing`, and
`src` must not match the `src` inside `data-src` — each checks the character immediately
following/preceding the match. A file that fails UTF-8 decoding yields empty `FileFacts` with
*no* diagnostic: a binary file wearing an `.html` extension is the user's business, not a
defect this adapter reports on every run (contrast JSON's own stance, which *does* emit a parse
diagnostic on malformed content — HTML has no "malformed" concept in the same sense, since a
non-UTF-8 file isn't attempting to be a document extraction can partially trust).

**Spans point at the reference, not the document.** A `LineIndex` (byte-offset-to-line/column,
built once per file) locates each attribute *value's* span precisely, so `kndo describe` points
at the actual `src="…"` text rather than the top of the file — the same precision-over-cost
tradeoff every span-carrying adapter makes, here amortized across a file that may carry many
references rather than rebuilt per reference.

### 3. Imports & resolution

**Resolution is path arithmetic, not a module algorithm — no candidate list at all.** A browser
fetches the exact path a document writes; there is no extension-resolution order to try (JS's
`.ts` before `.js`), no `package.json`/`exports` map, no bundler-alias layer. `resolve()` joins
the importing document's directory with the specifier as written and checks membership in
`ResolveCtx`'s known-files index — one lookup, no fallback ladder:

```
fn resolve(spec, ctx) -> Resolution {
    let path = join(dirname(spec.from), spec.specifier);
    if ctx.contains(&path) { File(path, Certain) } else { Unresolved }
}
```

**No extension guessing.** `./main` is not treated as a possible spelling of `./main.js` the
way JS's candidate ladder would try it — a browser wouldn't fetch the latter for the former
either, so inventing tolerance here would diverge from what the reference actually means.

**Every miss is `Unresolved`, never `Missing`.** Unlike JS-TS's or CSS's explicitly-relative-
path convention (contracts §2.1: a relative specifier that exhausts a real candidate ladder
resolves `Missing`, and `unresolved` reports it at `error` severity), this adapter has no
ladder to exhaust — a single join-and-lookup either hits or it doesn't, and a miss has three
equally plausible causes a static read of the source tree cannot distinguish: a build artifact
not yet produced (`./dist/bundle.js`, real in any bundled project before its first build), a
server-generated asset, or a genuine typo. Reporting all three at `error` severity would make
the common, harmless case (a fresh checkout that hasn't been built yet) look identical to a
real broken link, so this adapter stays at the honestly-incomplete `Unresolved` rather than
manufacturing a `Missing` verdict it cannot back up.

**Never `Dependency`, never `Stdlib`.** An HTML reference never names a package — there is no
bare-specifier, `node_modules`-style resolution step for a `<script src>` value the way there is
for a JS import — so `resolve()` has no bare-specifier branch at all; every specifier is treated
as a path. Correspondingly there is no browser-platform "stdlib" module namespace to special-
case the way `java.*`/`kotlin.*`/`Foundation` are for their languages.

### 4. Manifests & packages (RFC 0011)

Not applicable, and — unlike CSS's or JSON's "no manifest format of its own" (a fact about the
format), this one is a **rejected design**, not merely an absence: `claim_manifest` was
measured and specifically ruled out because of what claiming a manifest would cost elsewhere in
assembly (§0). `manifest_globs: vec![]`, `claim_manifest()` always `false`. This adapter
contributes no `ManifestDependency` nodes, no workspace topology, no root promotion through the
manifest mechanism other adapters use — its only root comes from extraction itself (§2), always
unconditional, never manifest-gated the way JS's/Java's/Rust's library-mode promotion is.

### 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| Root-relative reference (`/assets/app.js`) | Skipped, not `Unresolved` — depends on the server's document root, which a static read cannot know (§2) |
| CDN / absolute / protocol-relative URL (`https://…`, `//cdn…`) | Skipped — never named a project file (§2) |
| Template placeholder (`${base}/app.js`, `{{ url_for(...) }}`) | Skipped — a value the framework computes at render/build time, not a literal path (§2) |
| `data:`/`mailto:` URI | Skipped — not a file reference at all (§2) |
| Query string or fragment on an otherwise-local path (`./main.js?v=2`, `./a.css#x`) | Stripped before matching — the file on disk is `./main.js`/`./a.css` (§2) |
| Extensionless specifier (`./main` when only `main.js` exists) | `Unresolved` — no extension-guessing candidate ladder; a browser wouldn't fetch the extensionless path either (§3) |
| A build artifact not yet produced (`./dist/bundle.js`) | `Unresolved`, indistinguishable from a typo by a static read of the source tree — deliberately never `Missing` (§3) |
| Non-UTF-8 file with an `.html`/`.htm` name | Claimed, yields empty `FileFacts`, no diagnostic (§2) |
| `<!-- Generated by … -->`-style banners | Not detected — `detected_origin` is always `Authored` (§1); a real, comment-syntax-backed gap, not a structural non-goal the way JSON's is |
| `srcset`, `<object data>`, `<embed src>`, `<video>`/`<audio src>`, `<track src>`, SVG `<use href>`, `<form action>` | Not scanned — `FILE_ATTRS` covers five attributes only (§2); each of these needs its own value-shape handling, not a uniform table row |
| Inline `<script>`/`<style>` content | Never read as JS/CSS in place — an inline module's own imports or an inline stylesheet's own `@import`s are outside this adapter's tag-scan model entirely; only a *linked* file becomes a cross-language edge |
| `<base href>` | Not honored — the HTML spec lets it change the resolution root for every relative URL in the document; a page using it would have every reference resolved against the wrong directory |
| `.html` files as `untested` blind spots | Exempted via `declares_units_of_testing: false` (§0) — a document declares nothing a test could call |

### 6. Conformance fixtures (shared harness, RFC 0002 §8)

**None exist yet — a real, open gap, not an oversight to paper over.** Every other adapter in
this document pins its precision against a `tests/conformance.rs` fixture set run through the
real `Engine`; this one does not have a `tests/` directory at all. What stands in its place
today: unit tests inside `extraction.rs` (quoting-style coverage, tag/attribute substring
safety, span placement, query/fragment stripping, non-UTF-8 handling, the whole-file rooting
behavior) and `resolution.rs` (sibling and parent-relative resolution, extensionless-specifier
rejection, out-of-project misses), plus `lib.rs`'s own end-to-end delegation test — real
coverage of the scanner in isolation, but nothing yet exercises claim → extract → resolve →
reachability together the way a fixture under the shared harness does. The measured evidence
that the adapter *works* at the `Engine` level lives instead in `internal/detection-gaps.md`
§20-bis's vite/spring-petclinic/axios before-and-after tables (§0) — real, but a released-binary
measurement is not a standing regression gate the way a conformance fixture is. §7 tracks
closing this.

### 7. Open questions

1. **Conformance fixtures under the shared harness.** §6 — the adapter has no
   `tests/conformance.rs`/`tests/fixtures/` mirroring every sibling adapter's. A minimal corpus
   mirroring CSS's/JSON's `orphaned-*-is-unused` shape (an `index.html` rooting a
   `<script type="module">` entry, a sibling orphaned `.html` file that stays unreachable) is
   the natural first fixture; the `untested`-exemption behavior (§0) is a second, since nothing
   currently pins it against the real `Engine` rather than the unit-level `testable_languages`
   check.
2. **Generated-origin detection.** §1, §5 — HTML has real comment syntax
   (`<!-- ... -->`), so a toolkit `ContentMarkers` scan is structurally straightforward; not
   built because no generator-banner convention (sourcery/swiftgen-style, or a templating
   engine's own marker) has surfaced in measurement yet.
3. **`srcset` and the remaining unscanned reference-bearing attributes.** §2, §5 — `srcset`'s
   comma-separated, descriptor-suffixed value shape (`"a.jpg 1x, b.jpg 2x"`) is a different
   parse from every other `FILE_ATTRS` entry; `<object data>`/`<embed src>`/`<video src>`/
   `<audio src>`/`<track src>`/SVG `<use href>`/`<form action>` are each a real target the
   current five-row table doesn't reach. Parked pending measurement that any of them costs
   real findings the way `<script src>` did.
4. **Inline `<script type="module">`/`<style>` content.** §5 — an inline module's own `import`
   statements and an inline stylesheet's own `@import`s are invisible today; closing this
   would mean handing the inline text to the JS-TS/CSS adapters' own extraction rather than
   this adapter's tag scan, a cross-adapter question this doc doesn't resolve on its own.
5. **`<base href>`.** §5 — changes every relative URL's resolution root per the HTML spec;
   not honored, a documented, narrow gap until a real project surfaces it as a false miss.
