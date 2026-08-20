# Adapter Spec — Go

**Status:** Draft · **Implements:** `LanguageAdapter` (contracts §2) · **Milestone:** M3
**Grammar:** tree-sitter-go

The second adapter, and the one that exists specifically to test the contract (RFC 0002 §1: "adding
a language is adding one crate that implements one trait" — if that claim doesn't survive a second,
structurally different language, it isn't a real claim). Go is deliberately not JS-with-different-
syntax: no relative imports, visibility is a naming convention rather than a keyword, and a
"package" is a directory of files with no import needed between them — that last one required a
core contract change (`FileFacts::unit`, contracts §2) before this adapter could even be started;
see §1.1.

## 0. What's structurally different from JS/TS, and why it matters here

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

## 1. Claiming & classification

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
descriptor declares the matching ladder (RFC 0012 §6): `[Unit "unexported", Public "exported"]` —
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

## 2. Extraction

**Declarations**: top-level `func` (plain functions), methods (`func (t T) Name(...)` / `func (t
*T) Name(...)` — symbol name `T.Name`, so a value-receiver and pointer-receiver method pair on the
same type visibly share a namespace the way Go's own method-set rules do), `type` (struct,
interface, alias `type X = Y`, defined type `type X Y`), top-level `const` and `var` (including
grouped `const ( ... )` / `var ( ... )` blocks — one declaration per identifier, not one per
block), and `init` (Go's special no-args, unexported-by-construction, called-implicitly-by-the-
runtime function — always a root, §4, regardless of the capitalization rule, and there can be
more than one per file).

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

**Metrics** (`FunctionMetrics` — cyclomatic complexity, token fingerprints): **not populated**,
matching JS/TS's actual current state exactly (verified before writing this doc: `extraction.rs`'s
own header lists these as "deferred to later commits," and nothing in the codebase populates
`FunctionMetrics` for any language yet). Not a Go-specific gap — CRAP/duplicate-detection support
lands with M4 regardless of language, so there is no toolkit-shared complexity walker to call into
yet either.

**Suppressions**: `// kndo:allow …` on its own line or trailing a declaration — same syntax and
scope rules as JS/TS (RFC 0005 §12 is language-neutral; only comment *syntax* is adapter-owned,
and `//` line comments are identical between the two languages).

**Dynamic constructs → `DynamicUse`**: none emitted in this slice. See §0's last bullet and §5.

## 3. Imports & resolution

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

## 4. Manifests & packages (RFC 0011)

`go.mod` is a small line-oriented grammar (`module`, `go`, `require`/`replace`/`exclude` blocks) —
hand-parsed here rather than pulling in a dependency, the same "no more machinery than the format
needs" stance `package.json`'s `serde_json` parse takes for a format that *does* warrant a real
parser.

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

## 5. Known hard cases & stances

| Case | Stance |
|------|--------|
| External test package (`package foo_test` in a `_test.go` file) | treated as the same `unit` as `package foo` in the same directory (§1.1) — a documented, safe-direction imprecision, not a silent gap |
| Generic type parameters (`func F[T any](x T)`, `type Container[T any] struct{...}`) | the type-parameter list's constraint identifiers are ordinary references (e.g. `any`, a stdlib/local interface name); no special generics handling attempted beyond that — a constraint referencing a not-yet-declared local type still resolves correctly since phase 3a builds the whole file's symbol table before phase 3b resolves any reference, same ordering JS's forward-reference case already relies on |
| Method sets / interface satisfaction (does type `T` implement interface `I`?) | **not modeled** — Go's implicit (structural) interface satisfaction has no explicit `implements` syntax to hook a reference onto, unlike TS's `implements` clause. A type satisfying an interface produces no edge; this is a real expressiveness gap relative to TS, not an oversight — modeling it needs whole-program method-set computation, out of scope for extraction (a per-file, non-typechecking pass) |
| Struct/interface embedding | recorded as a plain reference to the embedded type's name (§2) — not a `RefKind::Extend`, matching the codebase-wide state that no adapter differentiates `RefKind` yet (§2, §7 open question) |
| `go:generate` directive comments | not parsed — the directive names a command line to run, not a file reference kndo could statically resolve without executing it |
| `reflect`/`plugin`-based dynamic dispatch | not modeled as a `DynamicUse` wildcard in this slice (§0) — genuinely rare in application code; revisit if dogfooding surfaces false `unused` positives traceable to it |
| Build-tag-gated files (`//go:build linux`, `_linux.go` suffix files) | claimed and extracted like any other `.go` file, unconditionally — kndo analyzes the union of all build configurations, the same "any-feature-is-live" stance RFC 0002 §7's table already states for Rust's `#[cfg]` features; a symbol used only under one build tag is still "used," not dead |
| Multi-module workspace (`go.work`) | claimed and parsed (RFC 0012 §10, fixtures `go-work-multi-module/` + `go-work-phantom-dep/`): `use` directives → `workspace_members`, sibling-module imports resolve as `WorkspaceMember` (reachability + the `require` contract, which go.work does not waive). A path-renaming `replace` directive remains the one recorded divergence — not modeled |

## 6. Conformance fixtures (shared harness, RFC 0002 §8)

Minimum corpus, each a mini-module with expected findings: multi-file package with a
same-package, no-import cross-file call plus one genuinely dead sibling function (the `unit`
mechanism's own reason for existing, and the exact shape that caught the reachability.rs
propagation gap, §0) · `internal/` package whose exports are correctly *not* promoted to roots,
alongside a sibling non-internal library file whose exports *are* (both directions of §4's
promotion rule in one fixture).

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

## 7. Open questions

Most of this section graduated into **RFC 0012 (Precise Reference Semantics & Visibility)**,
which owns the cross-language design for each — this list now just points there:

1. ~~`go.work` multi-module workspace support~~ — **fixed** (RFC 0012 §10, landed: go.work
   claimed and parsed, sibling-module imports resolve as `WorkspaceMember` with the full
   dependency contract; the path-renaming `replace` divergence stands recorded, not modeled).
2. `RefKind` differentiation (`TypeUse`/`Extend`) → RFC 0012 §5. Go's mapping is nearly free
   (`type_identifier` *is* the type-position signal; embeddings → `Extend`).
3. Package-level `internal-only` boundary awareness → RFC 0012 §6 (the visibility ladder as
   data; Go declares `[Unit "unexported", Public "exported"]`).
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
