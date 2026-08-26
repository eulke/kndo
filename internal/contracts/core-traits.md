# Contract — Core Traits & Graph Vocabulary

**Status:** Accepted · Normative for RFC 0001/0002/0003. Code must match this document; changing
either requires updating both in the same PR. Sketches are simplified Rust (lifetimes, error
types and non-essential fields elided) — shape is normative, exact signatures may be refined
during M1 with a PR to this file.

## 1. Graph vocabulary

```rust
pub struct FileId(u32);      // interned; stable within a snapshot
pub struct SymbolId(u32);
pub struct DependencyId(u32); // an external dependency declared in a manifest
pub struct PackageId(u32);    // a workspace unit: one manifest + the files it governs (RFC 0011)

// A file's classification is two orthogonal axes, never one enum: a generated test file and a
// vendored production file are both expressible. `Role` values mirror `RootKind` on purpose.
pub enum FileRole   { Production, Test, Tooling }
pub enum FileOrigin { Authored, Generated, Vendored }
pub struct FileClass { pub role: FileRole, pub origin: FileOrigin }

pub enum SymbolKind {
    Function, Method, Constructor, Macro, Class, Interface, Struct, Enum, EnumMember,
    TypeAlias, Const, Static, Variable, Field, Module, CssRule, CssVariable, Other(SmolStr),
}
// kebab-case names are `subject_kind` facet values (alongside file | directory | dependency |
// import | suppression) used in output and `category:subject` targeting (RFC 0005).
// Two kinds carry analysis semantics beyond the label:
// - Constructor (Java/Kotlin `<init>`, Swift `init`/`<deinit>`): instantiation references the
//   *type*, never the constructor symbol — liveness follows the container (a synthetic
//   container→constructor edge), and visibility analyses skip the kind.
// - Macro (Rust `macro_rules!`, a C preprocessor macro): an *expansion* symbol — invoked
//   textually outside the module-visibility model, and its body executes at expansion sites.
//   Visibility-scope analyses skip the kind as a subject and treat references attributed to a
//   Macro as originating from its (unknowable) expansion sites, i.e. requiring the widest scope.

pub enum RootKind { Production, Test, Tooling }
// A declared Production root's kind is capped by its file's role at assembly (RFC 0005 §2):
// an entry point in a Tooling/Test-role file becomes a Tooling/Test root. Plugin-contributed
// roots and library-surface promotions are exempt (their evidence outranks the convention).

pub enum DependencyScope { Prod, Dev, Build, Peer, Optional }
// analysis semantics per scope: RFC 0005 §5

pub enum RefKind { Call, Read, Write, Extend, Implement, Override, TypeUse }

pub enum Confidence { Certain, Probable, Possible }

pub enum EdgeKind {
    ImportsFile      { from: FileId, to: FileId },         // may cross Package boundaries (RFC 0011 §4)
    ImportsDependency{ from: FileId, to: DependencyId },
    References       { from: NodeRef, to: SymbolId, kind: RefKind },  // from: File | Symbol — see below
    Declares         { file: FileId, symbol: SymbolId },
    Root             { kind: RootKind, target: NodeRef },  // NodeRef = File | Symbol
    Wildcard         { from: FileId },                     // dynamic construct; resolved against a
                                                            // plausible target set, not a fixed target
}
// Every edge (Root and Wildcard included) carries its own Confidence and contributes to
// reachability only from that strength onward; how per-edge confidence combines into a node's
// (color, confidence) — including Wildcard's plausible-target-set expansion — is the tiered
// algorithm normatively defined in RFC 0005 §1, not left to each analysis to reinvent.
// `References.from` is `NodeRef` rather than always `SymbolId`: an adapter that hasn't tracked
// which declaration encloses a reference (only which file) emits `NodeRef::File` — sufficient
// for reachability (a reachable file referencing a symbol makes that symbol reachable
// regardless of which of the file's own functions did the referencing) though not for
// finer-grained "which caller" evidence. `NodeRef::Symbol` once an adapter tracks enclosing
// scope precisely enough to say more.
// Every File is owned by exactly one Package (nearest-manifest rule, RFC 0011 §3);
// Package depends-on Package edges are derived by the core, never emitted by adapters.
// RefKind matters to analyses: Implement/Override edges drive dispatch-aware member liveness
// (RFC 0005 §2); Extend/TypeUse distinguish type-level from value-level consumption.
// every edge: { kind: EdgeKind, confidence: Confidence, source: Provenance }
// Provenance = Adapter(AdapterId) | Plugin(PluginId)  — for attribution in output
```

## 2. `LanguageAdapter`

One implementation per language, registered at startup. Adapters are pure with respect to the
filesystem: all content arrives via parameters (determinism, sandboxing, testing — RFC 0002 §6).

```rust
pub trait LanguageAdapter: Send + Sync {
    fn descriptor(&self) -> AdapterDescriptor;
    // { id: "js-ts", facts_schema_version: u32, file_globs, manifest_globs, grammar_version,
    //   visibility_ladder: Vec<VisibilityRung> }
    // visibility_ladder (RFC 0012 §6): what VisibilityLevel indexes into — each rung a
    // { scope: File|Unit|Package|Public, label, surface_transitive } triple; the scope is what the core can check
    // (same file / same FileFacts::unit / same PackageId / anywhere — nested, narrowest to
    // widest), the label is the language's own word, used verbatim in remediation text.
    // Empty ladder = no visibility semantics (CSS, JSON): visibility analyses skip the
    // language. surface_transitive (M6): whether a re-export chain can carry this rung
    // outside its package — relative rungs (Rust pub, JS export, Java public/protected)
    // are true; capped rungs (pub(crate), package-private, Swift internal, Go exports under
    // internal/) are false. Library-mode symbol promotion and the surface-member closure
    // (RFC 0011 §5) promote only transitive rungs. Conservative-mapping rule: a language
    // level with no exact scope maps to the
    // nearest WIDER one (Java protected → Public) — over-approximating who may see a symbol
    // can only suppress a finding, never fabricate one. Assembly copies the ladder onto
    // ProjectGraph::visibility_ladders keyed by claim language; analyses are pure graph
    // functions and never touch adapters. Consumers: internal-only's tightest-sufficient
    // rung, private-type-leak's scope comparison, the member fallback's candidate scoping.
    // cycle_policy (RFC 0005 §8): { file_cycles, package_cycles }, each a CycleTolerance —
    // Hazard (cycles are ecosystem hazards → warning); Idiomatic (routine, legal structure)
    // and Impossible (the compiler forbids them) both emit nothing. Same
    // data-on-the-descriptor pattern as the ladder, carried onto
    // ProjectGraph::cycle_policies; a mixed-language cycle is reported iff any
    // participant's language declares Hazard.
    // resolves_dependency_usage (added M5, Java): whether this adapter's resolve() can ever
    // produce an ImportsDependency edge for a manifest-declared dependency of this language —
    // true for every language whose import specifier structurally identifies the declared
    // package (npm's flat name, Go's module-path prefix, Cargo's crate name, Java's `java.`/
    // `javax.` stdlib prefix). false when the import namespace has no reliable mapping to
    // manifest coordinates without resolving the classpath (Java's third-party imports:
    // docs/adapters/java.md §0). Carried onto PackageNode::resolves_dependency_usage per the
    // claiming adapter (assembly phase 2a); dependency_hygiene skips unused/test-only
    // verdicts for a package where it's false (one diagnostic, not a false-positive flood —
    // "zero usage evidence" isn't a meaningful unused claim when usage evidence can never
    // exist). version-skew is unaffected — it compares declared versions across manifests
    // directly, no usage edge needed.
    //
    // Beside it on the PackageNode, and asked of the adapters rather than derived from the
    // claim: manifest_claim_languages — EVERY registered adapter's claim language whose
    // claim_manifest accepts this manifest, not just the one that won the claim (Java and
    // Kotlin both claim pom.xml, so a .kt file's Maven declarations must keep counting no
    // matter which got there first). File→package ownership is nearest-ancestor by DIRECTORY,
    // which is right for everything it feeds except one question — whose dependency
    // declarations does this file answer to? A Jazzy-generated .js under docs/ in a Swift
    // repo, or a web/app.js beside a go.mod, owes nothing to Package.swift or go.mod, and
    // charged its bare imports against them anyway (every Swift repo's phantom `jquery`, and
    // the whole of hugo's undeclared column). PackageNode::governs_dependencies_of is the test
    // both undeclared and dependency_hygiene apply, symmetrically: such a file is evidence
    // neither that a declaration is missing nor that one is used. The implicit no-manifest
    // package declares nothing, so nothing can contradict it and every file answers to it.
    // package_test_dirs: directory names that mark files test-role only when the directory
    // is an immediate child of the owning package's manifest directory (Cargo's tests/,
    // benches/, examples/ — conventions bound to the manifest beside them, unlike
    // anywhere-in-the-path markers such as __tests__/, which stay in claim-time patterns).
    // The names are adapter data; the matching runs in core assembly (phase 2b), the only
    // layer that knows which manifest owns which file. Files in the implicit no-manifest
    // package anchor at the project root. Promotion only — a file already test- or
    // tooling-role is never demoted, so a nested package whose sources live under an
    // ancestor's tests/ tree keeps its own classification.

    /// Claim & classify a path (fast; name-based, content peeking only when unavoidable).
    fn claim(&self, path: &ProjectPath) -> Option<FileClaim>;   // { language, class: FileClass }

    /// Is this path one of this adapter's manifest files? Manifests are claimed separately from
    /// source — they never get a `FileClaim`/language of their own (docs/adapters/js-ts.md §1).
    fn claim_manifest(&self, path: &ProjectPath) -> bool;

    /// Parse one file and extract every language-defined fact. Must not fail on broken code:
    /// return partial facts + diagnostics.
    fn extract(&self, file: &SourceFile) -> FileFacts;

    /// Parse a manifest into declared dependencies, package identity/topology (RFC 0011 §3),
    /// and roots. `ctx` lets the adapter resolve entry-point specifiers (main/module/exports/
    /// bin) against the known-files index itself — a manifest root always names a *different*
    /// file than the one being extracted, so (unlike `FileFacts::roots`) it must already be a
    /// concrete `ProjectPath` by the time the core sees it; the core has no language-specific
    /// resolution rules to guess one with.
    fn extract_manifest(&self, file: &SourceFile, ctx: &ResolveCtx) -> ManifestFacts;

    /// Resolve an import specifier to a concrete target, given an index of claimable paths.
    /// Called by the core's resolution driver — including for specifiers emitted by *other*
    /// adapters (cross-language edges, RFC 0002 §4). Internal-package specifiers
    /// (workspace:*, path deps, alias paths) resolve as WorkspaceMember — the concrete
    /// sibling file plus the package name, from which assembly derives BOTH edge kinds
    /// (ImportsFile for reachability, ImportsDependency for the declaration contract —
    /// RFC 0011 §4 validates it both ways) — unless same_package is set: the specifier
    /// resolved within the importing file's OWN package (a package's tests or binaries
    /// naming its library by package name), where the file edge and bindings are real but
    /// no self-declaration contract exists to validate, so assembly derives ImportsFile
    /// only (no phantom `undeclared`, no dependency-usage credit).
    /// ResolveCtx carries the workspace-member index
    /// (name → { dir, resolved entry }) the core builds from every named manifest's facts,
    /// and the unit reverse-index in two forms. `unit_files(unit)` is repo-global;
    /// `unit_files_from(unit, &spec.from)` prefers candidates in the IMPORTER's own package and
    /// falls back to the global set only when it has none. Prefer the latter: a unit key is
    /// unique only within a package (§8 of RFC 0012 keys Java/Kotlin units on the declared
    /// package name and Swift's on the target name), so sibling modules that share a package
    /// name share a key, and a resolver picking `.first()` by path order could bind an import
    /// to an unrelated module — a phantom edge `cyclic` reports as a package cycle neither
    /// module's source supports. Reachability is insulated from the choice (same-unit fallback
    /// keeps every file in the unit reachable); the literal edge is not, because it is
    /// evidence. The fallback is what keeps genuine cross-module imports resolving.
    /// When no concrete in-repo file matches (a source checkout whose published entries are
    /// build artifacts), the specifier falls through to the external ladder as a plain
    /// Dependency — the package is still consumed, and dropping to Unresolved would silently
    /// un-count a genuinely used dependency; only the file edge is unknowable.
    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx) -> Resolution;
    // Resolution = File(ProjectPath, Confidence) | Dependency(DependencyName, Confidence)
    //            | WorkspaceMember { name, target: ProjectPath, confidence, same_package }
    //            | Stdlib | Unresolved
}
```

```rust
pub struct FileFacts {
    pub member_types: Vec<RawMemberType>,   // { owner, member, yields, yields_params } — what accessing
                                            // owner.member evaluates to (field types, method returns);
                                            // yields_params: the annotation's type arguments in order,
                                            // projected by `?N` pointer markers; RFC 0012 §3-bis
    pub invoked_executables: Vec<SmolStr>,  // workspace executable targets this file runs as a
                                            // subprocess (Rust: env!("CARGO_BIN_EXE_<name>")) —
                                            // resolved against ManifestFacts::executables into
                                            // InvokesFile edges (RFC 0005 §1's invoked-program rule)
    pub declarations: Vec<Declaration>,     // { name, kind: SymbolKind, span, exported: bool,
                                             //   visibility, member_of: Option<Name>,
                                             //   signature_span: Option<Span>,
                                             //   implicitly_invoked: bool — the language's own
                                             //   machinery invokes this member through its owner,
                                             //   never by name at the call site (`{}` → fmt,
                                             //   `==` → eq); reachability derives the implicit
                                             //   owner → member edge (RFC 0005 §1's
                                             //   machinery-dispatch rule),
                                             //   nested_scope: bool — declared inside a scope
                                             //   unit nested within the file (an inline module):
                                             //   the tightest declarable level there means "this
                                             //   scope", strictly narrower than the ladder's
                                             //   file rung, so file-local evidence can't certify
                                             //   that rung and internal-only advances past it,
                                             //   visibility_inherited: bool — no declarable
                                             //   visibility of its own (enum variants, trait
                                             //   items): the level belongs to the container,
                                             //   which is measured separately; visibility
                                             //   analyses skip the member,
                                             //   markers: Vec<Name> — the language-visible
                                             //   annotations/attributes/decorators written on
                                             //   this declaration, verbatim and in source
                                             //   order. FACTS, never verdicts: the adapter
                                             //   never interprets them and the core never
                                             //   learns what any of them mean. Their consumer
                                             //   is kndo.toml's [[externally-invoked]], which
                                             //   matches its own marker list against these to
                                             //   seed production roots — the one question
                                             //   source alone cannot answer ("is this called
                                             //   from outside the analyzed source?"), answered
                                             //   by the project instead of guessed }
                                             // signature_span (RFC 0012 §5): the declaration's
                                             // *promise* — everything before the body block
                                             // (name, parameters, return/result types).
                                             // Callables only in v1; None on type/value
                                             // declarations (a struct's whole body is not a
                                             // signature — firing on it would falsely accuse
                                             // exported-struct/unexported-field shapes). Feeds
                                             // the `private-type-leak` analysis: a TypeUse
                                             // reference attributed to an exported callable
                                             // whose site lies inside this span, naming a
                                             // lower-visibility type, is a lying public API.
                                             // member_of (RFC 0012 §3): the owning type's
                                             // declared name when this is a member (a Go
                                             // method's receiver type, a class method's class);
                                             // name is then the BARE member name — ownership is
                                             // a structured fact, never string encoding. Members
                                             // resolve on their own track: an unqualified
                                             // reference never certain-resolves to a member; it
                                             // reaches members only through the duck-typed
                                             // fallback (assembly phase 3b — candidates are
                                             // same-language members whose declared visibility
                                             // scope contains the site, per the RFC 0012 §6
                                             // ladder; one candidate ⇒ Probable, several ⇒
                                             // Possible each, per RFC 0002 §5), and a
                                             // RawRoot targeting a member names it in qualified
                                             // `Owner.name` form. Display/selectors use the
                                             // qualified form; ids fold it in, so same-named
                                             // members of different owners stay distinct.
    pub references:   Vec<RawReference>,    // { name, scope-context, span, within, kind } —
                                             // resolved
                                             // core-side (graph::assemble) against same-file
                                             // declarations and this file's own import bindings
                                             // (below); no adapter resolution hook.
                                             // within (RFC 0012 §4): the declared symbol this
                                             // reference executes INSIDE — "the symbol whose
                                             // use triggers this code": callable bodies → the
                                             // callable (members in qualified Owner.name form);
                                             // load-time code → None; on-instantiation code →
                                             // the type. Assembly attributes the References
                                             // edge to that symbol, making transitive death
                                             // visible (a dead function's calls no longer keep
                                             // its callees alive). An unresolvable within falls
                                             // back to file attribution — the safe direction —
                                             // and None (adapters not emitting it) reproduces
                                             // prior behavior exactly.
                                             // kind (RFC 0012 §5): RefKind — what the use *is*
                                             // (Read | TypeUse | Extend | …), carried verbatim
                                             // onto the References edge. Untagged/default is
                                             // Read, byte-identical to pre-§5 behavior; tree-
                                             // sitter grammars make TypeUse nearly free (Go/TS:
                                             // `type_identifier` IS the type-position signal).
                                             // Reachability ignores kind entirely — every kind
                                             // keeps its target alive; only kind-selective
                                             // analyses (private-type-leak) filter by it.
                                             // scope_context (RFC 0012 §9): the qualifier text
                                             // of a qualified access (`json.Marshal` → { name:
                                             // "Marshal", scope_context: Some("json") }).
                                             // Assembly matches it against the file's imports —
                                             // explicit local_alias, or the resolved target's
                                             // unit_name — and resolves the name INSIDE that
                                             // target at Certain (hit or miss, a matched
                                             // qualifier settles resolution; the local tables
                                             // are never candidates). An unmatched qualifier is
                                             // a receiver expression: member access by
                                             // construction — skips free-name tables, goes
                                             // straight to the §3 member fallback.
    pub imports:      Vec<RawImport>,       // { specifier, kind: Relative|Package, span,
                                             //   side_effect_only, type_only, confidence,
                                             //   bindings: Vec<ImportBinding>, reexported,
                                             //   opaque_namespace_use,
                                             //   local_alias: Option<Name> }
                                             // local_alias (RFC 0012 §9): the EXPLICIT alias a
                                             // namespace import binds its target under (Go's
                                             // `import j "enc/json"` → Some("j")); None for
                                             // unaliased imports — assembly then derives the
                                             // qualifier from the resolved target's own
                                             // unit_name, fixing dir≠package specifiers
                                             // (gopkg.in/yaml.v3 binds as `yaml`). JS/TS
                                             // (name-binding imports) always None.
                                             // kind is syntactic shape only — Stdlib is a
                                             // resolve()-time fact, never claimed here.
                                             // ImportBinding { local, imported: Option<Name> } —
                                             // None imported = default import (binds to the
                                             // target's synthetic "default" export); lets a
                                             // same-name RawReference resolve to the *target
                                             // file's* symbol instead of (incorrectly) a
                                             // same-file one. Statically-tracked namespace
                                             // member accesses (`ns.foo`) become bindings with
                                             // a dotted local ("ns.foo") plus a same-named
                                             // RawReference. opaque_namespace_use: the imported
                                             // namespace is consumed in ways static tracking
                                             // can't follow (computed member `ns[key]`, or ns
                                             // escaping into a call/assignment) — assembly then
                                             // wildcards over the resolved target's symbols.
    pub roots:        Vec<RawRoot>,         // language-defined only (main, pub API…), target is
                                             // *within this file* — WholeFile | Declaration(name)
    pub functions:    Vec<FunctionMetrics>, // { symbol, span, cyclomatic: u32, loc,
                                             // fingerprints } (RFC 0005 §6): one entry per
                                             // callable, computed over its BODY. `span` is the
                                             // paired Declaration's OWN span, and is what
                                             // assembly resolves to a SymbolId onto
                                             // ProjectGraph::function_metrics — NOT `symbol`,
                                             // which stays the roots/within naming convention
                                             // (bare, or qualified Owner.name for members) for
                                             // display only. Name resolution was wrong here: a
                                             // file may declare one name twice (cfg-alternated
                                             // impls, platform-gated overloads) and the
                                             // per-file name tables are single-slot, so both
                                             // entries collapsed onto one symbol — read back as
                                             // a structural clone of itself, with its tokens
                                             // double-counted into health. There is no default:
                                             // an adapter emitting metrics must say which
                                             // declaration they belong to.
                                             // fingerprints = winnowing over the normalized
                                             // token stream (toolkit metrics module: IDs/
                                             // literals canonicalized, comments skipped), empty
                                             // under the 50-token granularity gate — cyclomatic
                                             // and loc always real (crap's inputs, M4).
    pub dynamics:     Vec<DynamicUse>,      // constructs forcing Wildcard edges (span + reason +
                                            // optional narrowed_to: a *project-relative* dir the
                                            // adapter already resolved — the core only prefix-
                                            // matches it, staying language-blind)
    pub suppressions: Vec<RawSuppression>,  // kndo:allow pragmas found in comments (§2.1)
    pub diagnostics:  Vec<Diagnostic>,
    pub unit:         Option<SmolStr>,      // reference-resolution scope beyond "this file" —
                                             // see below; `None` for file-scoped languages
    pub unit_name:    Option<SmolStr>,      // the name IMPORTERS bind this unit by (RFC 0012
                                             // §9): Go's `package` clause name. Distinct from
                                             // `unit` (the opaque grouping key, dir#package):
                                             // unit_name is the visible qualifier assembly
                                             // resolves unaliased qualified references
                                             // against. None where imports don't bind a
                                             // namespace by the target's declared name (JS).
    pub detected_origin: Option<FileOrigin>, // content-derived correction of the claim-time
                                             // origin axis (RFC 0012 §7): a generated banner
                                             // (`// Code generated … DO NOT EDIT.`,
                                             // `@generated`) is a fact about the CONTENT,
                                             // which claim (path-only, fast by design) can't
                                             // see. Assembly applies it to the FileNode
                                             // before role roots and every analysis, so all
                                             // Generated/Vendored exemptions see the
                                             // corrected value. Role is never content-
                                             // corrected. None = claim-time origin stands.
                                             // Toolkit `ContentMarkers` scans a bounded
                                             // first-N-lines window for comment-marker
                                             // languages; adapters with structured signals
                                             // (Java @Generated) set it from their own AST.
    pub test_spans:   Vec<Span>,             // sub-file TEST REGIONS (added M5, surfaced by
                                             // Rust — the first language whose tests live
                                             // inside production files): span extents whose
                                             // contents are test-role, attributes included
                                             // (`#[cfg(test)]` items, `#[test]`/`#[bench]`
                                             // fns, `#![cfg(test)]` = whole file). Outermost
                                             // extents only; regions never overlap. Empty
                                             // for per-file test detection (JS/TS, Go).
                                             // Consumers — see below.
}
```

**`test_spans` (added M5, surfaced by the Rust adapter):** the file's *role* stays a per-path,
claim-time axis; this field is the extraction-side truth that a *region* of a production file
is test infrastructure — and it is the **single producer-side declaration** of that truth:
adapters emit the spans and nothing else (no per-declaration Test roots — `RawRoot` is for
roots a span cannot express, like FFI exports and `fn main`). Everything downstream is
derived core-side, so the representations cannot drift:

1. **Assembly derives the in-source Test roots**: every declaration whose span lies inside a
   region gets a `Certain` `Root{Test, Symbol}` edge, emitted by the shared declaration
   emitter (full build and incremental patch alike). Reachability colors from these seeds,
   and the core's test-root exemption keeps the seeds themselves out of
   `test-only`/`untested` (a test reachable only from tests is a test).
2. **`crap`** and **health's symbol tallies** skip span-contained symbols — the same exemption
   test files get, at span granularity (a `#[test]` fn is not untested production code).
3. **`dependency_hygiene`** treats an import whose *site* lies inside a region as a test-role
   usage: a `prod`-scoped dependency consumed only under `#[cfg(test)]` is a `test-only`
   dependency, exactly as if the imports lived in test files.
4. **Assembly (phase 2.55)** demotes a claimed-production file to test role when at least one
   module-linking import (side-effect import binding a module name — Rust's `mod tests;`)
   reaches it from inside a region and none reaches it from production code: the out-of-line
   `#[cfg(test)] mod tests;` whole-file case the path-based claim cannot see. Patch safety:
   whether each import sits inside a region is part of the surface signature (span-derived but
   reformat-stable), so a gating change declines the incremental patch.
5. **`duplicate` deliberately does not consult regions** — it already fingerprints test files,
   so inline test clones remain findings (parity, not an exemption).

**`unit` (added M3, surfaced by the Go adapter):** an unqualified [`RawReference`] resolves
against, in order, import-bound names, this file's own declarations, and — new — every other
file sharing this file's non-`None` `unit` key. Exists because "a reference resolves within its
own file, or through an explicit import" is a JS/TS-ism, not universal: Go's visibility unit is
the *package* (its containing directory), and two files in one package calling each other's
unexported functions with no `import` statement at all is the ordinary case, not an edge case a
file-scoped model can shrug off. The adapter computes the key from information it already has
(for Go: the file's own directory — no new input); the core only groups declarations by it
(`graph::assemble` phase 3a) and adds it as a third, last-resort lookup in phase 3b's reference
resolution. `None` (every adapter before Go, and JS/TS today) reproduces prior behavior exactly —
this is additive, not a breaking change to the trait's existing implementors. Import-binding
resolution gets the same fallback for the same reason, one level removed: `Resolution::File`
names one concrete file, but for a package-scoped language a single import can still name a whole
directory of files (Go: `resolve()` picks one representative file in the target package so an
`ImportsFile` edge has somewhere to point; the specific symbol an import binding names may live in
any of that directory's *other* files) — so a binding lookup that misses in the target file's own
symbol table falls back to the target file's `unit` table before giving up, mirroring the
reference-resolution fallback exactly.

```rust
pub struct ManifestFacts {
    pub package_name:       Option<SmolStr>,
    pub private:            bool,                    // publish signal: true → app mode (RFC 0011 §5)
    pub workspace_members:  Vec<SmolStr>,             // workspace globs (RFC 0011 §3)
    pub dependencies:       Vec<ManifestDependency>,  // { name, version_req: Option<Name>,
                                             //   scope: DependencyScope, inherited: bool }
                                             // version_req None = this manifest states no
                                             // COMPARABLE requirement, which is a different
                                             // fact from "any version" and has to be
                                             // representable as one: a BOM/platform-managed
                                             // JVM coordinate names no version by design, a
                                             // Cargo path/git dep constrains nothing, a
                                             // SwiftPM branch pin is not a range, an
                                             // unresolved `${property}` is not a value.
                                             // Encoding all of those as "*" collided with
                                             // npm's "*", which IS a declared requirement,
                                             // and made version-skew compare a sentinel
                                             // against a version and call it a defect.
                                             // `inherited` deps arrive None and are resolved
                                             // against workspace_dependencies at assembly.
    pub entry_points:       Vec<SmolStr>,             // main/module/exports/bin/types, raw and
                                                       // unresolved — future self-import resolution
                                                       // input; NOT the root-worthiness signal
    pub roots:              Vec<ManifestRoot>,        // { kind: RootKind, target: ProjectPath,
                                                       //   confidence } — already resolved by the
                                                       // adapter (a manifest root always names a
                                                       // *different* file, unlike FileFacts::roots)
    pub resolved_entries:   Vec<(ProjectPath, Confidence)>, // import entry points, resolved, in
                                                       // precedence order (main > module > exports
                                                       // leaves) — the WorkspaceMember.entry source.
                                                       // NOT private-gated (a sibling importing a
                                                       // private member still resolves through its
                                                       // entry; privateness only gates roots)
    pub declares_surface:   bool,                     // `exports` map or equivalent present —
                                                       // the contract gate for `deep-import` (RFC 0011 §4).
                                                       // Assembly carries it (plus resolved_entries
                                                       // as FileIds) onto PackageNode
                                                       // { declares_surface, surface }, so the
                                                       // analysis is a pure graph function: an
                                                       // ImportsFile edge from another package
                                                       // landing off-surface while the gate holds
                                                       // is a deep import.
    pub script_invoked_names: Vec<SmolStr>,           // names invoked as the leading command of a
                                                       // build/tooling script (npm `scripts`,
                                                       // Cargo `[[bin]]`-adjacent xtasks, …) — a
                                                       // CLI-only tool dependency never produces an
                                                       // Imports edge, so this is dependency
                                                       // hygiene's (RFC 0005 §5) only usage evidence
                                                       // for it; cross-referenced by name against
                                                       // declared dependencies downstream, so an
                                                       // unrelated match is simply never looked up
    pub executables: Vec<ExecutableTarget>,            // named executable targets { name, entry } —
                                                       // the identity a subprocess invocation uses
                                                       // (Cargo bin names, npm `bin` keys); resolved
                                                       // against FileFacts::invoked_executables into
                                                       // InvokesFile edges (RFC 0005 §1's
                                                       // invoked-program rule)
    pub diagnostics:        Vec<Diagnostic>,
}
```

Root defaults (RFC 0011 §5, library mode lands fully at M3 — this is the default rule, no
per-package config override yet): `bin` targets are production roots unconditionally; `main`/
`module`/`exports` targets are production roots only when `private` is false — an unpublished
app's exports are not roots on their own, something must actually import them. `types`/
`typings` are never roots — a `.d.ts` target carries no runtime edge.

### 2.1 Suppression extraction

Comment syntax is language-defined, so **adapters extract suppression pragmas**; the core only
validates and binds them (RFC 0005 §12):

```rust
pub struct RawSuppression {
    pub span: Span,                    // the pragma comment itself
    pub category: SmolStr,             // verdict name — validated by the core against the registry
    pub subject: Option<SmolStr>,      // optional :subject facet (kebab-case)
    pub reason: Option<String>,        // free text after the directive
    pub scope: SuppressionScope,       // Declaration | File
}
pub enum SuppressionScope { Declaration, File }
```

- Grammar inside any comment style of the language:
  `kndo:allow <category>[:<subject>] [reason…]` (scope `Declaration`) and
  `kndo:allow-file <category>[:<subject>] [reason…]` (scope `File`).
- Binding (core-side): a `Declaration` pragma attaches to the declaration it precedes or shares
  a line with, covering that symbol *and everything it declares* (a class-level allow covers its
  members). `File` pragmas cover the whole file.
- **Evaluation order (no-flicker guarantee):** analyses run as if no pragmas existed and compute
  the full finding set; suppression then *marks* matched findings (hidden from report and
  `--fail-on`, still counted) — it never deletes them. A pragma is `stale` only when it binds to
  nothing, names an unknown category, or matches nothing in that **pre-suppression** set. Thus
  "actively suppressing" and "stale" are mutually exclusive by construction: deleting a stale
  pragma cannot resurrect a finding (it was stale precisely because the finding no longer
  exists), and deleting an active one correctly un-hides its finding.
- `stale` findings are not inline-suppressible (`kndo:allow stale` is rejected as unknown-target
  meta-suppression); acknowledge them via baseline or config if needed.
- Adapters do **not** interpret pragmas — extraction only. Validation, binding, counting, and
  staleness are core logic, identical across languages.

Compliance: every adapter must pass the shared conformance harness with its fixture corpus
(RFC 0002 §8). `FileFacts` must be deterministic for identical content.

## 3. `Plugin`

All hooks optional; a plugin implements what it needs (RFC 0003 §2). Same trait for built-ins
(statically linked) and external WASM components — both the four graph-mutation hooks
(`kndo:plugin@0.1.0`) and `LanguageAdapter` (`kndo:adapter@0.1.0`) are bridged
(`kndo-plugin-api`, [wasm-abi.md](wasm-abi.md) §5). `ingest_coverage`/`suppress` aren't bridged
either way yet.

```rust
pub trait Plugin: Send + Sync {
    fn descriptor(&self) -> PluginDescriptor;
    // { id, version, detection: Vec<SmolStr>, requested_file_access: Vec<SmolStr>,
    //   activation: Vec<ActivationRule>, dependencies: Vec<SmolStr> } — `detection` is prose
    // for `kndo doctor`; `activation` (RFC 0003 §4) is what gates a plugin that isn't
    // unconditionally present (globally installed, or a built-in with rules); `id` is a
    // coordinate (`kndo:` reserved for built-ins, source coordinates for external — RFC 0015
    // §2) and `dependencies` names coordinates whose conventions are part of this plugin's
    // own (co-install + co-activate fixpoint, RFC 0015 §3).
    // No ordering-constraints field yet (RFC 0003 §5's open item) — plugins run sorted by `id`,
    // a real but interim determinism rule.

    fn mutates_graph(&self) -> bool; // REQUIRED, no default — see the bullet below
    fn classify_file(&self, path: &ProjectPath, current: FileClass) -> Option<FileClass> { None }
    fn contribute_roots(&self, graph: &GraphView<'_>, out: &mut RootSink) {}
    fn contribute_edges(&self, graph: &GraphView<'_>, out: &mut EdgeSink) {}
    fn annotate_symbols(&self, graph: &GraphView<'_>, out: &mut AnnotationSink) {}
    fn ingest_coverage(&self, path: &ProjectPath, content: &[u8], out: &mut CoverageSink) {}
    fn suppress(&self, finding: &Finding) -> Option<SuppressReason> { None } // not wired yet
}
```

- **Landed (M5).** `GraphView<'a>` borrows the graph's own `files`/`symbols` (built, never
  copied) and exposes `files()` plus `symbols_in(path)` — the latter backed by a one-time
  `FileId -> [symbol index]` map built when the view is constructed, so a plugin walking every
  file's symbols costs `O(files + symbols)`, not `O(files * symbols)`. `RootSink`/`EdgeSink`/
  `AnnotationSink` are **write-only and id-free**: every call takes a `PluginTarget { path:
  ProjectPath, symbol: Option<SmolStr> }` (bare or `Owner.name`), never a `FileId`/`SymbolId` —
  the core resolves it against the same bare/qualified symbol tables `RawRoot`/`RawReference`
  already resolve against, and drops an unresolvable target silently (an adapter's own facts
  already have this exact miss behavior). `contribute_roots`/`contribute_edges`/
  `annotate_symbols` all run once, together, in `graph::assemble_from_source` right after phase
  3b's reference-resolution merge and before the canonical edge sort — every contributed
  `Edge`/root/annotation is attributed `Provenance::Plugin(id)` and folds into that one sort, no
  second pass. `annotate_symbols`' marks land in `ProjectGraph::externally_consumed:
  Vec<SymbolId>` (sorted, deduplicated — `is_externally_consumed` binary-searches it), consumed
  by `internal-only`/`private-type-leak` (RFC 0005 §7's exemption). `classify_file` runs earlier,
  inline in phase 2's file-node build, right after RFC 0012 §7's content-derived origin
  correction — its answer is what every downstream role/origin exemption sees.
- Any registered plugin whose `mutates_graph()` returns `true` makes `assemble_from_source`
  skip both the graph-snapshot cache hit and the incremental patch, full-rebuilding every run:
  neither reuse path re-invokes plugin hooks, and RFC 0003 §5's plugin-identity-in-the-cache-key
  mechanism isn't built yet. A plugin that only implements `ingest_coverage`/`suppress` (like
  `LcovPlugin`) declares `mutates_graph() == false` and is invisible to both fast paths — and
  the declaration is self-enforcing, not trusted: assembly only ever *calls* the four
  graph-mutation hooks on plugins that claim `true`, so a false claim means the hooks never run
  (identically cold or cached), never a stale cached graph. `mutates_graph` has **no default** —
  every implementor states it explicitly; forgetting it is a compile error, not a silently
  disabled incremental patch. (History note: the method used to default to `true`, and before
  that this predicate checked raw-registry emptiness, which — with `LcovPlugin` unconditionally
  registered — silently disabled both fast paths on every real run; the required method with no
  default is the fix that can't regress the same way again.)
- Host-mediated file access: content for `requested_file_access` globs is provided by the core;
  no ambient fs/net (enforced natively by convention, in WASM by the sandbox).
- `ingest_coverage` (ADR 0005: coverage is *ingested, never measured*) follows the same sink
  discipline as the contribute hooks: the host locates reports via the plugin's
  `requested_file_access` paths, enforces the freshness gate (default 7 days; a stale report
  gets one diagnostic and is ignored), and hands over the bytes; the plugin parses its format
  and writes per-file line hit counts through `CoverageSink`, normalizing report paths to
  project-relative form (it alone knows the format's path conventions). The resulting
  `CoverageMap` is a per-run analysis input — never part of the graph or its snapshot, because
  report freshness varies independently of source content hashes. Built-in at launch: lcov.
- Budget: per-hook fuel/time limit; an over-budget plugin is disabled for the run + diagnostic
  (RFC 0003 §3) — landed for both WASM tiers (`kndo-plugin-api`'s `FUEL_PER_CALL`, one constant
  per bridge file); a trapped/exhausted `Plugin` hook over WASM degrades to "contributed
  nothing this round," never a crashed run. Native built-in plugins have no such budget (there
  is no untrusted call to limit for statically-linked code).

## 4. `Analysis`

Internal trait (not pluggable in 1.0 — RFC 0003 §6; §6 below also marks it "Internal — may
change any release"), listed here because its shape constrains the graph API. What's landed
(`analysis/mod.rs`) is the uniform-output seam this section originally speculated a
dirty-region incremental subsystem into, without building that subsystem before anything
needs it:

```rust
pub(crate) struct AnalysisCtx<'a> {
    graph: &'a ProjectGraph, reach: &'a ReachabilityMap,
    coverage: &'a CoverageMap, tuning: &'a AnalysisTuning,
}
#[derive(Default)]
pub(crate) struct AnalysisOutput {
    findings: Vec<Finding>, diagnostics: Vec<Diagnostic>,
    cycle_files: HashSet<FileId>,        // populated by `cyclic` only
    duplicated: Vec<(SymbolId, u32)>,    // populated by `duplicate-functions` only
}
pub(crate) trait Analysis: Send + Sync {
    fn id(&self) -> &'static str;                        // "unused", "crap", … — also the
                                                           // `--verbose` timings label
    fn run(&self, ctx: &AnalysisCtx<'_>) -> AnalysisOutput;
}
```

`AnalysisTuning` carries `[analysis.crap] threshold`, `[analysis.duplicate] min-tokens`, and
`[[externally-invoked]]` — every knob that acts strictly POST-assembly, which is why none of
them belongs in the graph or its cache key. `run_all` resolves the last of these into symbol
ids (`reachability::externally_invoked_symbols`: a declaration whose `Declaration::markers`
include one of a rule's `markers`, scoped by its `paths`) and seeds
`reachability::compute_with_roots` with them, at `Production`/`Certain` — the standing a
manifest-declared entry point has, because the project asserted the fact. `compute(graph)`
remains as the no-extra-roots form; the navigation verbs go through
`query_envelope::compute_reachability`, which takes the same rules so `kndo used-by` and
`kndo check` can never disagree about a symbol's color.

`run_all` holds a fixed-order `Vec<Box<dyn Analysis>>` registry, runs it via
`par_iter().map(...).collect()` (order-preserving regardless of completion order — no
explicit join tree to hand-maintain per analysis added), then reduces deterministically:
findings concatenate in registry order and are id-sorted after: `cycle_files`/`duplicated`
merge from whichever single analysis populates them; diagnostics keep their pre-refactor
fixed order (`crap`, `untested`, `dependencies`-hygiene — the only three that ever emit one)
rather than falling out of registry position.

**Not built, and not what this landed as:** `GraphView`/`Enrichments`/`DirtyRegion`/
`FindingsView` and a `run_incremental` entry point. RFC 0004 §5's dirty-region analysis
incrementality doesn't exist yet (§5 below); when it lands, the registry above is the seam
it plugs into — an `Analysis` impl gaining a second method, not a new dispatch mechanism.

## 5. `Engine` — the frontend boundary

`kndo-core` is a **library**; every interface to it — today's CLI, tomorrow's `kndo serve`/MCP,
an LSP, a GUI, a CI action — is a *frontend* consuming one facade. Nothing else is exported.

```rust
pub struct Engine { /* opaque: graph, cache, adapters, plugins */ }

impl Engine {
    /// `adapters` is composed by the DISTRIBUTION layer (the `kndo` crate, RFC 0001 §2) —
    /// frontends call `kndo::open(root, overrides)` and never touch this parameter; only
    /// embedders and tests pass a custom set. `open` registers NO plugins — not even the
    /// coverage ingesters: plugins are composition, not core (the ignorance rule covers
    /// report formats too). The product's built-in set arrives through `open_with_plugins`
    /// from the `kndo` crate's `default_plugins()`, exactly like adapters do.
    pub fn open(root: &Path, overrides: ConfigOverrides,
                adapters: Vec<Box<dyn LanguageAdapter>>) -> Result<Engine, EngineError>;
    /// Same as `open`, additionally taking the registered `Plugin` set explicitly (landed M5).
    pub fn open_with_plugins(root: &Path, overrides: ConfigOverrides,
                adapters: Vec<Box<dyn LanguageAdapter>>,
                plugins: Vec<Box<dyn Plugin>>) -> Result<Engine, EngineError>;
    pub fn check(&mut self, mode: RunMode) -> RunResult;    // full | staged | diff
    pub fn query(&self, req: QueryRequest) -> QueryResult;  // RFC 0007 verbs, incl. batches
    pub fn query_batch(&self, requests: Vec<QueryRequest>) -> Vec<QueryResult>;  // one shared graph load
    pub fn baseline(&mut self, op: BaselineOp) -> BaselineResult;
    pub fn doctor(&self) -> DoctorReport;
}
// Planned, not landed: `explain(id) -> Explanation` (per-finding remediation prose). Purely
// additive when it comes; the contract lists only what exists.
//
// Gate policy lives here too, as data rather than an exit code (the core-never-prints rule
// covers exit codes as much as ANSI): `RunMode::default_fail_on() -> Option<Severity>` (full
// mode: None; a diff mode: Some(Warning)) and `RunResult::fails_at(threshold:
// Option<Severity>) -> bool` (severity ranking + the advisory-finding exemption). A frontend's
// own job shrinks to parsing `--fail-on` and mapping the bool to its own exit-code convention.
// `RunResult::plugin_contributions: Vec<PluginContribution>` carries this run's own
// graph-mutation audit record — `Some` whenever `run_plugin_round` actually ran this call
// (full build, patch), falling back to the cache's sidecar record only on a pure
// snapshot-hit, where nothing ran this call but the graph (and so the record) is unchanged.

// Distribution layer (crate `kndo`) — what frontends actually call:
// pub fn kndo::open(root: &Path, overrides: ConfigOverrides) -> Result<Engine, EngineError>
// pub fn kndo::default_adapters() -> Vec<Box<dyn LanguageAdapter>>
// pub fn kndo::default_plugins() -> Vec<Box<dyn Plugin>>  // the built-in coverage ingesters
//     (lcov, Cobertura, JaCoCo, go cover — RFC 0003 §3), plus any activated convention plugin
```

- `RunResult`/`QueryResult` are the **typed forms of the output schema**
  ([output-schema.md](output-schema.md)); the JSON and SARIF serializers live core-side so every
  frontend emits byte-identical machine output. *Human* rendering lives frontend-side (RFC 0009).
- **Separation rules, enforced by dependency direction:** the core contains no terminal concerns
  (no ANSI, no TTY detection, no exit codes, no stdout) — it returns data and never prints;
  frontends contain no analysis concerns — they cannot reach the graph, cache, or adapters
  except through `Engine`. A frontend that needs a new fact is a core PR adding it to
  `RunResult`, never a core import.
- `Engine` is synchronous and single-instance-per-project (the cache lock, RFC 0004 §7); a
  serving frontend wraps it in its own concurrency model. `query`/`query_batch` take `&self`
  specifically so that model can run many read-only queries concurrently against one shared
  `Engine` without external synchronization — `check`/`baseline` stay `&mut self` (they touch
  the baseline file and the health-trend snapshot, state a concurrent query must never
  perturb). The graph-snapshot write after assembly runs on a background thread so
  serialization overlaps with analysis and rendering; the in-flight handle is joined before the
  next assembly and on `Drop` (a frontend drops `Engine` after printing, which is exactly
  "written after results are printed, before exit" — a killed process loses only cache warmth,
  never correctness). That handle now lives behind a `Mutex` rather than a plain field
  precisely so joining it — from `query`/`query_batch`'s next call, or from `Drop` — never
  needs `&mut Engine`.
- **Facade re-exports.** `kndo-core`'s crate root re-exports the frontend-facing surface
  directly (`kndo_core::Engine`, `RunResult`, `Finding`, `Severity`, `Confidence`, `Group`,
  `Category`, `Diagnostic`, `QueryRequest`, `QueryResult`, `sort_findings_for_display`, …) so
  a frontend never needs to know which internal module (`engine`, `vocab`, `query_envelope`)
  actually defines a type; the `kndo` distribution crate re-exports the same names at its own
  root in turn (`kndo::Engine`, not `kndo::engine::Engine`). Adapter/plugin authoring types
  (`LanguageAdapter`, `Plugin`, `GraphView`, …) are a different surface — component authors,
  not frontends — and stay reached through their own modules (`kndo_core::adapter`,
  `kndo_core::plugin`, …), unchanged.
- `ConfigOverrides.use_cache` (default `true`) is the `--no-cache` switch (RFC 0004 §4), gating
  two cache layers (`cache.rs`, ADR 0004): the facts layer (`.kndo/cache/facts/`) skips
  re-parsing any file whose content hash already has a current entry; the graph layer
  (`.kndo/cache/graphs/<key>.bin`, content-addressed like facts entries so full mode's working
  tree and diff modes' before/after tree states coexist instead of evicting each other) skips
  claim/extract/resolve/link *entirely* when a single digest — folding in the whole discovered
  file set, every registered adapter's id/version, and the core graph-schema version — matches a
  stored snapshot exactly. There is no partial reuse yet: a graph-snapshot miss falls through to
  full assembly (itself facts-cache-warm for whichever files didn't change). `RunResult
  .cache_enabled`/`cache_hits` are how a frontend learns whether a run was actually warm —
  `run.cache` in the JSON envelope is `"warm"` only when the cache was on *and* served at least
  one file or the whole graph; an enabled-but-empty cache (first run, or a change big enough that
  nothing hit) is honestly `"cold"`. Correctness never depends on this: `--no-cache` must produce
  byte-identical findings (verified on the fixture matrix and the 5k-file benchmark; not yet
  wired into a CI workflow — RFC 0004 §4). On a graph-snapshot key miss, the warm-run *patch*
  algorithm (`graph.rs`'s `try_patch`) reuses the previous snapshot when its guards all hold —
  same graph-schema version and plugin digest, identical file set (any add/remove/rename bails),
  no changed manifest, at most ~5% of files changed, unchanged surface signatures — otherwise it
  falls through to full assembly (itself facts-cache-warm for unchanged files). Patched ≡
  fully-rebuilt output is test-enforced (`patch_equivalence`). Still unimplemented from RFC 0004:
  the findings snapshot and dirty-region *analysis* incrementality (§5) — analyses always re-run
  over the (possibly patched) graph.

## 6. Stability tiers

| Surface | Tier |
|---------|------|
| Graph vocabulary (§1), `LanguageAdapter`, `Plugin` | **Contract** — semver'd from 1.0; WASM ABI versioned independently |
| `Engine` facade (§5) | **Contract** — semver'd from 1.0; the only surface frontends may touch |
| `Analysis`, cache layouts | Internal — may change any release (cache self-invalidates) |
| Output schema | Contract — see [output-schema.md](output-schema.md) |
