# Contracts

## Core traits

**Status:** Accepted · Normative for RFC 0001/0002/0003. Code must match this document; changing
either requires updating both in the same PR. Sketches are simplified Rust (lifetimes, error
types and non-essential fields elided) — shape is normative, exact signatures may be refined
during M1 with a PR to this file.

### 1. Graph vocabulary

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

### 2. `LanguageAdapter`

One implementation per language, registered at startup. Adapters are pure with respect to the
filesystem: all content arrives via parameters (determinism, sandboxing, testing — RFC 0002 §6).

```rust
pub trait LanguageAdapter: Send + Sync {
    fn descriptor(&self) -> AdapterDescriptor;
    // { id: "js-ts", facts_schema_version: u32, file_globs, manifest_globs, grammar_version,
    //   ... } — `facts_schema_version` is for a change in what THIS adapter emits. A shape
    //   change in the facts contract ITSELF (`FileFacts` and everything reachable from it —
    //   `Declaration`, `FunctionMetrics`, …) is `cache::ENTRY_FORMAT_VERSION`: ONE constant,
    //   folded into both the facts entries and the graph key. Spelling that as a bump in every
    //   adapter is the same fact six-plus times, and silently under-invalidates when someone
    //   bumps five of six.
    //   visibility_ladder: Vec<VisibilityRung> }
    // visibility_ladder (RFC 0012 §6): what VisibilityLevel indexes into — each rung a
    // { scope: File|Unit|Module|Package|Public, label, surface_transitive } triple; the scope is
    // what the core can check (same file / same FileFacts::unit / a unit AND ITS SUBTREE /
    // same PackageId / anywhere — nested, narrowest to widest), the label is the language's own
    // word, used verbatim in remediation text. Module is anchored per DECLARATION
    // (Declaration::visible_in_unit) and walks FileFacts::unit_parent: it is the rung a
    // four-bucket ladder had nowhere to put, so adapters widened Rust's pub(super) into
    // pub(crate) and mapped its `private` — which is really module-and-descendants — down to a
    // file, and private-type-leak had to stay gated behind surface_transitive as a result
    // (internal/detection-gaps.md §7). Two rungs may share the Module scope and differ only by
    // anchor; comparing them is a REGION question (graph::region_covers), not an enum one.
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
    // declares_units_of_testing: whether a file of this language can hold a unit of testing at
    // all. true for every language whose files can carry a function; false for a document
    // language — HTML, JSON, CSS — where "is this tested" has no answer. Same
    // data-on-the-descriptor pattern as the ladder, carried onto
    // ProjectGraph::testable_languages, and consulted by `untested` for ONE case: a file that
    // declares nothing. A file that declares symbols is judged on them
    // (`files_declaring_only_values`), so a `.scss` carrying a `@function` stays in scope and no
    // blanket "stylesheets aren't testable" rule can silence it. The conjunction is the point:
    // an absence of symbols alone cannot tell "nothing here is testable" from "extraction
    // failed", so the adapter has to say which — and a language the graph never recorded
    // answers true, because silence must never be an exemption. No default (like
    // Plugin::mutates_graph): wrong in either direction is a real defect — false silences a
    // language's blind spots, true on a document language buries the report under one finding
    // per page.
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
    // builtin_member_types: member-type facts about the types the LANGUAGE provides, which no
    // file in the project declares — the same (owner, member, yields) shape as
    // FileFacts::member_types, declared once here because there is no home file to hang them
    // on. Result<T,E>::map_err still yields a Result over the same T; a Vec<T> iterates to its
    // T. Same data-on-the-descriptor pattern as the ladder and the cycle policy: the knowledge
    // is the adapter's (it is ITS language's standard library), the core gets a second lookup
    // tier keyed by claim language, consulted after the owner's home file and the only one
    // that can apply when the owner name resolves to no declaration at all. Two conventions
    // the core is blind to: TypeExpr::Param says "the same argument the receiver had", and an
    // adapter may name an operation or an anonymous type with a string of its own choosing
    // (Rust's @element for iteration, @slice for &[T]) as long as it emits that same string on
    // the reference side — a member is a member, the core never interprets the name. Keep the
    // table small and evidence-driven, like the machinery-trait list: a fact earns its place
    // by closing a measured case, not by completing an API surface.

    /// Claim & classify a path (fast; name-based, content peeking only when unavoidable).
    fn claim(&self, path: &ProjectPath) -> Option<FileClaim>;   // { language, class: FileClass }

    /// Is this path one of this adapter's manifest files? Manifests are claimed separately from
    /// source — they never get a `FileClaim`/language of their own (docs/adapters/js-ts.md §1).
    fn claim_manifest(&self, path: &ProjectPath) -> bool;

    /// Parse one file and extract every language-defined fact. Must not fail on broken code:
    /// return partial facts + diagnostics.
    ///
    /// The core *enforces* that obligation rather than trusting it: extraction runs inside
    /// `catch_unwind` (`graph::assemble::claim_and_extract`), so an adapter that panics on one
    /// pathological file costs that file's facts and a `Warn` diagnostic naming the adapter,
    /// not the whole run. Extraction is the one place adapter code meets arbitrary bytes and it
    /// runs across a rayon pool, where an unwinding worker takes every other file's answer with
    /// it. This is containment, not permission: a panic here is still an adapter defect, and
    /// the diagnostic says so. The facts of a panicking file are deliberately **not** cached —
    /// caching an absence would make the defect survive the next run.
    fn extract(&self, file: &SourceFile) -> FileFacts;

    /// Parse a manifest into declared dependencies, package identity/topology (RFC 0011 §3),
    /// and roots. `ctx` lets the adapter resolve entry-point specifiers (main/module/exports/
    /// bin) against the known-files index itself — a manifest root always names a *different*
    /// file than the one being extracted, so (unlike `FileFacts::roots`) it must already be a
    /// concrete `ProjectPath` by the time the core sees it; the core has no language-specific
    /// resolution rules to guess one with.
    ///
    /// This `ctx` — and only this one — also carries `read_manifest`, the single point in the
    /// adapter contract where one file's facts may depend on another file's *contents*. Some
    /// manifest formats let one manifest declare a value another one uses (Maven's `<parent>`),
    /// and an adapter handed one manifest's text at a time cannot follow that on its own. The
    /// core stays ignorant of what any of it means: it offers "you may read a manifest", never
    /// "poms have parents".
    ///
    /// Sound here and nowhere else, for two reasons that both have to hold: manifest extraction
    /// is **not cached** (assembly re-runs it every time, reading each manifest fresh, so no
    /// entry can go stale behind an ancestor's edit), and the incremental patch **refuses**
    /// outright on any changed manifest. `resolve`'s ctx has no such channel, and an adapter
    /// must read the resulting `None` as "I cannot see it", never as "there is none".
    fn extract_manifest(&self, file: &SourceFile, ctx: &ResolveCtx) -> ManifestFacts;

    /// Does this manifest declare a dependency that an `ActivationRule::ManifestDependency`
    /// naming `query` means? How a coordinate is SPELLED is ecosystem knowledge the core has
    /// no way to hold: npm's name is the literal key an author would write, Cargo treats `-`
    /// and `_` as interchangeable, and a Maven/Gradle coordinate is `groupId:artifactId` while
    /// an author writes the artifact id alone. Defaults to exact equality over `dependencies`
    /// plus `workspace_dependencies` (a virtual workspace root declares only the latter) — so
    /// an adapter whose spelling needs no translation overrides nothing, the same
    /// dynamic-not-obligation posture as `visibility_ladder` and `claim_manifest`.
    fn declares_dependency(&self, facts: &ManifestFacts, query: &str) -> bool;

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
    //            | Stdlib | Missing | Unresolved
    //
    // Missing vs Unresolved is a normative distinction and only the adapter can make it.
    // MISSING = "I understood this specifier as a path into the project, tried every candidate
    // my language's rules allow, and no file is there" — a complete answer, and the fact the
    // `unresolved` analysis reports at severity error. UNRESOLVED = "no answer": the shape is
    // one I do not model (a self-reference imports map, an inline module's `super::`, a Sass
    // load-path name, a URL), or the information lives somewhere I do not read. The core sees
    // only that no edge came back and cannot tell them apart, so a resolver that says Missing
    // where it means Unresolved makes kndo accuse working code of being broken.
    //
    // Adoption is per-adapter and optional, like the visibility ladder: an adapter that cannot
    // yet separate the two keeps returning Unresolved and simply reports nothing. Degrading
    // toward silence is always available; degrading toward accusation never is.
}
```

```rust
pub struct FileFacts {
    pub member_types: Vec<RawMemberType>,   // { owner: Option<Name>, member, yields: TypeExpr }
                                            // — what accessing owner.member evaluates to (field
                                            // types, method returns). owner NONE = a free
                                            // FUNCTION: "calling this evaluates to yields", the
                                            // same statement about a value's type, walked by the
                                            // same chain machinery — it just applies where a
                                            // pointer's BASE names the function, before any member
                                            // segment (`let e = parse_entry(..); e.path`).
                                            // yields is a TREE, because a type is one:
                                            // Named { name, args } | Param(N) | Unknown.
                                            // `?N` projects argument N and lands on a SUBTREE
                                            // with its own arguments intact — Result<Vec<T>, E>
                                            // at 0 is Vec<T>, which a flat list could not say.
                                            // Param(N) states a RELATIONSHIP rather than a type
                                            // ("still a Result over the same T"), substituted
                                            // against the receiver's own arguments at the hop;
                                            // Unknown is a type the fact cannot name, explicit so
                                            // no fact has to lie about its arity to stay silent.
                                            // RFC 0012 §3-bis
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
                                             //   scope", strictly narrower than any scope the
                                             //   core can compute, since every scope up to and
                                             //   including Module is derived from file/unit
                                             //   co-location and none of it sees inside a file;
                                             //   internal-only therefore certifies only rungs
                                             //   strictly wider than Module for such a
                                             //   declaration (Package and Public are unaffected:
                                             //   nesting cannot change a package),
                                             //   visibility_inherited: bool — no declarable
                                             //   visibility of its own (enum variants, trait
                                             //   items): the level belongs to the container,
                                             //   which is measured separately; visibility
                                             //   analyses skip the member,
                                             //   implements: Option<Name> — the trait /
                                             //   protocol / interface whose IMPLEMENTATION
                                             //   declares this member (Rust's `impl Serialize
                                             //   for T`, Swift's `extension T: Codable`).
                                             //   A FACT about where the member is written,
                                             //   never a verdict about what invokes it, and
                                             //   None wherever a language declares members in
                                             //   the type body with the interface separate
                                             //   (Java, Kotlin, Go, JS) — an optional fact a
                                             //   language may simply never fill. The core
                                             //   carries it and interprets nothing; its
                                             //   consumer is a PLUGIN that legitimately holds
                                             //   one ecosystem's knowledge, matching a curated
                                             //   trait table through
                                             //   AnnotationSink::mark_machinery_impls. That is
                                             //   what lets kndo:serde / kndo:rkyv /
                                             //   kndo:wasmtime be their tables and nothing
                                             //   else, instead of re-parsing a grammar the
                                             //   adapter already parsed,
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
                                             // target at Certain. A matched qualifier settles
                                             // resolution hit or miss — the local tables are
                                             // never candidates — WHEN the import that
                                             // registered it is Certain: a statement the file
                                             // makes names a closed namespace. An import the
                                             // adapter reconstructed from a use site
                                             // (non-Certain) does not settle; its miss keeps
                                             // falling through (RFC 0012 §9-ter).
                                             // An unmatched qualifier is
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
                                             // (name-binding imports) always None. An import
                                             // RECONSTRUCTED from a use site sets it too — the
                                             // segment that site qualifies by, which only the
                                             // adapter can identify; the core does not split
                                             // specifiers on any separator (RFC 0012 §9-ter).
                                             // reconstructed: the file contains NO import
                                             // statement for this — the adapter synthesized it
                                             // from a use site so resolution has something to
                                             // bind. Two rules read it: such an import's
                                             // qualifier does not SETTLE a member miss, and its
                                             // bindings rank BELOW the file's own declarations
                                             // (no language lets a written import shadow one,
                                             // so a collision can only come from a synthetic).
                                             // Confidence cannot stand in for it — Rust's
                                             // crate/self/super-rooted synthetic imports are
                                             // Certain about where they resolve.
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
    pub functions:    Vec<FunctionMetrics>, // { symbol, span, shape_span, shape_ordinal,
                                             // cyclomatic: u32, loc, token_count,
                                             // fingerprints, body_is_construction } (RFC 0005
                                             // §6): one entry per
                                             // callable SHAPE — a declaration's own body plus
                                             // one for every callable nested inside it that is
                                             // big enough to carry clone evidence by itself
                                             // (MetricsSyntax::nested_callable_kinds names the
                                             // node kinds; below the clone floor a nested
                                             // callable stays an expression inside its owner).
                                             // A promoted shape's branches and tokens LEAVE the
                                             // enclosing stream, which keeps one `FN` in their
                                             // place, so summing over shapes counts every token
                                             // once. Every shape of one declaration repeats that
                                             // declaration's `span`; `shape_ordinal` (0 = the
                                             // declaration's own, 1..N nested in pre-order)
                                             // separates them in a finding id and `shape_span`
                                             // (== `span` for ordinal 0) is what every consumer
                                             // reports as the LOCATION. An ordinal, not a line:
                                             // a line churns a baseline whenever anything above
                                             // the closure moves. `span` is the
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
                                             // and loc always real. cyclomatic is crap's
                                             // complexity input; loc is reported by
                                             // `describe`'s metrics block (RFC 0007 §4.2) and
                                             // is NOT a term of the crap score.
                                             // body_is_construction: this shape's body is a
                                             // single value-construction expression and
                                             // nothing else (MetricsSyntax::construction_kinds
                                             // names the node kinds; empty is valid and means
                                             // "this language cannot tell"). A FACT, never a
                                             // verdict — `duplicate` is what consumes it, and
                                             // exempts such a body because normalization
                                             // INVERTS there: it erases the field values (the
                                             // whole authored content) and keeps the field
                                             // list the type declaration dictates.
    pub dynamics:     Vec<DynamicUse>,      // constructs forcing Wildcard edges (span + reason +
                                            // optional narrowed_to: a *project-relative* dir the
                                            // adapter already resolved — the core only prefix-
                                            // matches it, staying language-blind)
    pub suppressions: Vec<RawSuppression>,  // kndo:allow pragmas found in comments (§2.1)
    pub diagnostics:  Vec<Diagnostic>,
    pub unit:         Option<SmolStr>,      // reference-resolution scope beyond "this file" —
                                             // see below; `None` for file-scoped languages
    pub unit_parent:  Option<SmolStr>,      // the unit CONTAINING this file's unit — the link
                                             // that turns the flat key set into a TREE the core
                                             // walks without knowing any separator (the adapter,
                                             // which knows its language's, supplies it). None at
                                             // a root and for languages whose units do not nest,
                                             // where VisibilityScope::Module collapses to Unit.
                                             // Every file of one unit must report the same
                                             // parent: first writer wins, a disagreement is an
                                             // adapter bug rather than something to merge
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
    pub string_call_args:                    // PLUGIN FUEL, ecosystem-blind. `(callee dotted
        Vec<StringCallArg>,                  // path, first string-literal argument, span)`:
                                             // `res.render("index")`, `app.get("/users", …)`.
                                             // The adapter records that a call carried a
                                             // literal, never what a framework means by it.
                                             // First literal argument only, direct literals
                                             // only. Optional per adapter, default empty;
                                             // JS/TS first.
    pub string_attr_args:                    // Its ATTRIBUTE sibling. `(attribute head, key,
        Vec<StringAttrArg>,                  // literal, decorated declaration, span)`:
                                             // `#[serde(skip_serializing_if = "is_zero")]`.
                                             // Key empty for a bare `#[x = "v"]`; owner None
                                             // when the attribute decorates a block rather
                                             // than a declaration. Optional per adapter,
                                             // default empty; Rust first.
}
```

**`string_call_args` / `string_attr_args` — the two ecosystem-blind literal facts.** Both
exist so a plugin can build a framework convention on what the adapter already parsed instead
of re-parsing claimed source through the content channel, and both stop at the same line: the
adapter records *what was written*, never what it means. That line is not stylistic. Over the
attributes the Rust adapter scans, serde alone writes 482 `key = "literal"` pairs whose value
is identifier-shaped; 248 of those values collide with a real declaration in the crate, and
only 176 sit under a key whose value serde actually resolves as a path. An adapter that
treated a collision as a reference would contribute 72 keep-alive edges in one crate to close
one real case — and a keep-alive edge silences a true finding. Telling
`skip_serializing_if = "f"` from `rename = "f"` requires knowing what serde is, and an adapter
that knew would be the adapter/plugin coupling §0.2 exists to prevent.

Both travel the same road: `FileFacts` → `FileNode` (canonically sorted, so the graph snapshot
round-trips them) → `GraphView::string_call_sites_in` / `attr_strings_in` natively, or
`call-sites-in` / `attr-strings-in` over the WASM ABI. **No analysis consumes either.** A
change to their shape is a change to the facts contract: bump `cache::ENTRY_FORMAT_VERSION`,
not a per-adapter `facts_schema_version`. An adapter beginning to *emit* one bumps its own.

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

#### 2.1 Suppression extraction

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
  nothing, names an unknown category, or matches nothing in that **pre-suppression** set *for a
  category some analysis judged*. Thus "actively suppressing" and "stale" are mutually exclusive
  by construction: deleting a stale pragma cannot resurrect a finding (it was stale precisely
  because the finding no longer exists), and deleting an active one correctly un-hides its
  finding.
- **Abstention.** An analysis returns a `Verdict`: `Judged`, or `Abstained(Diagnostic)` when the
  input its verdict needs is absent this run (`crap` without an ingested coverage report,
  `untested` in a project with no test roots). An abstained analysis emits no findings, so its
  categories carry **no information** — and an empty finding list read as "clean" is what breaks
  the guarantee above: the user deletes the pragma on kndo's advice, drops in a coverage report,
  and the finding returns. A category is unknown only when *every* analysis that can emit it
  (`Analysis::categories`) abstained. Consequences, all from that one value:
  - the matched-nothing verdict skips pragmas naming an unknown category (the binds-to-nothing
    and unknown-category verdicts do not: both are structural errors, verifiable without
    running any analysis);
  - `health` leaves the corresponding axis unmeasured rather than scoring zero penalty against
    a re-derived skip predicate of its own;
  - the run reports `run.abstained: [{category, reason}]`, so a consumer can tell "clean" from
    "not measured" and knows what would make it measurable.
- `stale` findings are not inline-suppressible (`kndo:allow stale` is rejected as unknown-target
  meta-suppression); acknowledge them via baseline or config if needed.
- Adapters do **not** interpret pragmas — extraction only. Validation, binding, counting, and
  staleness are core logic, identical across languages.

Compliance: every adapter must pass the shared conformance harness with its fixture corpus
(RFC 0002 §8). `FileFacts` must be deterministic for identical content.

### 3. `Plugin`

All hooks optional; a plugin implements what it needs (RFC 0003 §2). Same trait for built-ins
(statically linked) and external WASM components. Four WIT worlds carry it
(`kndo-plugin-api`, [WASM ABI](#wasm-abi) §5): `adapter` (`kndo:adapter@0.1.0`) for
`LanguageAdapter`, `plugin` (`kndo:plugin@0.1.0`) for the four graph-mutation hooks,
`plugin-findings` for `rules`/`contribute_findings` (RFC 0018), and `coverage-ingester` for
`ingest_coverage`. A component declares the world it implements; the host accepts each
separately, which is what lets a coverage ingester ship without a graph-mutation surface.

```rust
pub trait Plugin: Send + Sync {
    fn descriptor(&self) -> PluginDescriptor;
    // { id, version, detection: Vec<SmolStr>, requested_file_access: Vec<SmolStr>,
    //   activation: Vec<ActivationRule>, dependencies: Vec<SmolStr> } — `activation`
    // (RFC 0003 §4) is what gates a plugin that isn't unconditionally present (globally
    // installed, or a built-in with rules). `ManifestDependency` is evaluated against EVERY
    // manifest the compiled-in adapters claim, parsed by those adapters' own
    // `extract_manifest` and matched by their own `declares_dependency` — the frontend holds
    // no manifest parser of its own. It once held two (`package.json` and `Cargo.toml`), which
    // silently made the rule unmatchable for every JVM, Go and Swift project; and `detection` is prose for `kndo doctor`
    // describing a gate `activation` CANNOT express (an always-on coverage ingester naming
    // its report paths). A gate that IS a rule leaves `detection` empty rather than restating
    // it — one concept, one source. Every descriptor is written as a full struct literal, in
    // built-ins included: the field list is documentation that cannot drift, and a
    // constructor hiding four of the six is how `dependencies` stopped being visible to the
    // one author who needed it. `id` is a
    // coordinate (`kndo:` reserved for built-ins, source coordinates for external — RFC 0015
    // §2) and `dependencies` names coordinates whose conventions are part of this plugin's
    // own (co-install + co-activate fixpoint, RFC 0015 §3). Co-activation is not a
    // convenience: it is the ONLY path to a plugin whose framework is an indirect
    // dependency. A company framework that uses Express internally is never `express` in its
    // users' manifests, so `kndo:express`'s own rule can never fire there; the framework's
    // plugin names `kndo:express` here, and being active is what activates it. Pinned by
    // `crates/kndo/tests/plugin_dependency_implication.rs` — external component, built-in
    // dependency, both halves.
    // No ordering-constraints field yet (RFC 0003 §5's open item) — plugins run sorted by `id`,
    // a real but interim determinism rule.

    fn mutates_graph(&self) -> bool; // REQUIRED, no default — see the bullet below
    fn classify_file(&self, path: &ProjectPath, current: FileClass,
                     content: &ContentView) -> Option<FileClass> { None }
    fn contribute_roots(&self, graph: &GraphView<'_>, out: &mut RootSink) {}
    fn contribute_edges(&self, graph: &GraphView<'_>, out: &mut EdgeSink) {}
    fn annotate_symbols(&self, graph: &GraphView<'_>, out: &mut AnnotationSink) {}
    fn ingest_coverage(&self, path: &ProjectPath, content: &[u8], out: &mut CoverageSink) {}
    // RFC 0018, landed: what a plugin MAY assert, declared before any hook runs, and the
    // hook that asserts it. `contribute_findings` is NOT a graph-mutation hook — it runs
    // after assembly on every path (cold, patch, warm snapshot hit) and its output lands
    // under `plugin:<coordinate>/<rule>` on the advisory channel.
    fn rules(&self) -> Vec<RuleDescriptor> { Vec::new() }
    fn contribute_findings(&self, graph: &GraphView<'_>, content: &ContentView<'_>,
                           out: &mut FindingSink) {}
}
```

- **No `suppress` hook, and this is a decision rather than a gap.** RFC 0016 §7 evaluated
  domain-specific suppression against the components actually shipping and **cut** it — not
  deferred it. It is on no trait, in no WIT world (wasm-abi.md §5.2), and this listing is the
  whole surface. A real use case reopens it as a new, additive hook; none exists.

- **`ActivationReason` (landed).** Core owns the *vocabulary* of why a component is active —
  `Registered` (presence is the opt-in) · `AlwaysOn` (ships with the product, declares no
  rules) · `RuleMatched(ActivationRule)` (this rule, not merely "a rule") · `ImpliedBy(id)` —
  because every term in it is a field core already defines. Core does NOT own the *decision*:
  the tiers, the rule evaluation and the `dependencies` fixpoint live in the distribution layer,
  which reports its verdict in this shape and passes it to `open_with_plugins` inside a
  `RegisteredPlugin`. One enum serves plugins and adapters alike (both descriptors carry
  `activation`/`dependencies`), and its `Display` is the single rendering behind both
  `kndo doctor`'s `active (…)` line and the envelope's `run.plugins[].activated_by` — two
  spellings is how those two would come to disagree about the same run.

- **Landed (M5).** `GraphView<'a>` borrows the graph's own `files`/`symbols` (built, never
  copied) and exposes `files()` plus `symbols_in(path)` (and the two literal readers above,
  `string_call_sites_in(path)` / `attr_strings_in(path)`) — `symbols_in` backed by a one-time
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
  by `internal-only`/`private-type-leak` (RFC 0005 §7's exemption). `AnnotationSink` also offers
  **`mark_machinery_impls(graph, drives)`** — the whole body of an ecosystem conventions
  plugin: it walks the graph's members and marks every one whose `implements` fact and name the
  caller's `drives(trait_name, member_name)` closure accepts. The walk lives here and the
  CURATED TABLE is what a plugin brings, which is the split the layer demands: the core has no
  list of trait names and cannot acquire one without breaking the ignorance rule, while a
  plugin has no business re-parsing a grammar its adapter already parsed (which is exactly what
  `kndo:serde` did before `Declaration::implements` existed). `classify_file` runs earlier,
  inline in phase 2's file-node build, right after RFC 0012 §7's content-derived origin
  correction — its answer is what every downstream role/origin exemption sees. It gets the same
  content channel the other hooks do, scoped to its own globs: a file is often generated for a
  reason no path convention can express — a build tool's config SAYS SO (Maven's
  `libsass-maven-plugin` naming an `outputPath`) — and reading that needs no graph, which is why
  the hook can have it despite running before the graph exists. The WASM side needed no ABI
  change: `read-file` was already an import in both plugin worlds, and the host was simply
  answering it with an empty view here.
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

### 4. `Analysis`

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

### 5. `Engine` — the frontend boundary

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
    /// Each plugin arrives as a `RegisteredPlugin { plugin, activated_by: ActivationReason }`
    /// — the component plus the CALLER's answer to why it is active, which the run reports
    /// verbatim as `run.plugins[].activated_by`. The engine never derives that answer: the
    /// tiers, the rule evaluation and the `dependencies` fixpoint all live in the distribution
    /// layer, and a second derivation in core would be one fact with two sources (the failure
    /// mode that left this field specified-but-unemitted for three milestones). A bare
    /// `Box<dyn Plugin>` — or a boxed concrete plugin — converts in, yielding
    /// `ActivationReason::Registered`, which is exactly what an embedder choosing the set by
    /// hand did.
    pub fn open_with_plugins(root: &Path, overrides: ConfigOverrides,
                adapters: Vec<Box<dyn LanguageAdapter>>,
                plugins: impl IntoIterator<Item = impl Into<RegisteredPlugin>>)
                -> Result<Engine, EngineError>;
    pub fn check(&mut self, mode: RunMode) -> RunResult;    // full | staged | diff
    pub fn query(&self, req: QueryRequest) -> QueryResult;  // RFC 0007 verbs, incl. batches
    pub fn query_batch(&self, requests: Vec<QueryRequest>) -> Vec<QueryResult>;  // one shared graph load
    pub fn baseline(&mut self, op: BaselineOp) -> BaselineResult;
    pub fn doctor(&self) -> DoctorReport;
}
// `kndo explain <finding-id>` (RFC 0006 §2) is NOT a method here: it is `Verb::Explain`,
// answered through `query`/`query_batch` like every other verb. Its "selector" is a finding
// id rather than a node selector, and that is the only thing about it that differs — the
// envelope, the `not-found` status, the exit-code mapping and `kndo query` batching all come
// from the machinery the navigation verbs already use, rather than from a second copy of it.
// What it returns is deliberately a pair, not a derivation: the finding verbatim plus
// `describe` of its subject (`ExplainResult { finding, subject: Option<DescribeResult>,
// subject_selector }`). `subject` is `None` for a subject that is not one graph node — a
// directory rollup stands in for many — and the finding still explains itself there. The
// per-finding remediation prose an earlier draft imagined stays cut for the reason
// output-schema §2 gives about `remediation`.
//
// Gate policy lives here too, as data rather than an exit code (the core-never-prints rule
// covers exit codes as much as ANSI). It has TWO halves, and a frontend must call the reader
// that composes them, never one half:
//   RunMode::default_fail_on() -> Option<Severity>   // full: None; a diff mode: Some(Warning)
//   RunResult::fails_at(Option<Severity>) -> bool    // severity + the advisory exemption
//   RunResult::budget_failed() -> bool               // the [delta] budgets (see below)
//   RunResult::gate_fails(Option<Severity>) -> bool  // fails_at OR budget_failed — CALL THIS
// RFC 0006 §5 composes them with OR; two frontends each reimplementing that composition is how
// one of them silently honors half a gate. A frontend's own job shrinks to parsing `--fail-on`
// and mapping one bool to its own exit-code convention.
//
// `RunResult::net_findings() -> i64` is `new − fixed` under the same advisory exemption — what
// `max-net-findings` judges and what every renderer prints as `net ±N`.
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
  ([Output schema (JSON)](#output-schema-json)); the JSON and SARIF serializers live core-side so every
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

- **Provenance (`graph::provenance::ProvenanceIndex`)** answers "whose facts is this resting
  on" for one graph, once. [`Provenance`](#1-graph-vocabulary) lives on **edges**; both
  consumers that need it per *node* — a `describe` envelope's `sources` and every
  `Finding.sources` ([Output schema (JSON)](#output-schema-json) §2.1) — read this index rather than
  deriving their own, so the two cannot answer differently about the same node, and a new
  `EdgeKind` teaches both at once. `Engine` fills findings in one pass after suppression and
  config filtering: a verdict knows what it decided, not who supplied the graph it decided on,
  and thirteen analyses each answering would be thirteen chances to answer differently. It is
  read off the **persisted graph**, never off the live plugin round — a snapshot-hit run runs
  no plugins, and provenance sourced from that round would silently vanish exactly when the
  cache is warm.

- **Delta budgets (`crate::delta`)** are the gate's aggregate half. `RunResult.budget:
  Option<Budget>` is `Some` exactly when the run is a diff mode *and* `kndo.toml` has a
  `[delta]` section; `Budget { verdict, rules: Vec<BudgetRule { rule, limit, measured,
  verdict, over_by }> }` serializes straight into the envelope's top-level `budget`
  ([Output schema (JSON)](#output-schema-json) §1) — a sibling of `health`, not a member of `run`.
  Three rule kinds, in this evaluation order: `max-health-drop` (a **drop**, so an improving
  change measures negative), `max-net-findings` (`new − fixed`), and one rule per
  `[delta.budget]` key sorted, each an **absolute** count of new findings whose group *or*
  category matches — `fixed` compensates only inside `max-net-findings` (RFC 0006 §5).
  Advisory findings are excluded throughout, as they are from `fails_at`. A rule passes when
  `measured <= limit`, so a measurement landing exactly on its own stated maximum is not a
  failure. **`None` is load-bearing and never means "everything passed":** no `[delta]`
  section, full mode, and a run that could not assemble either side all report no budget at
  all, which is how a consumer tells "nobody set one" from "every budget held". The section's
  presence is the entire opt-in; inside it the strict ratchet (0.0 / 0) is the default, so no
  existing project changes exit code because the subsystem exists.

### 6. Stability tiers

| Surface | Tier |
|---------|------|
| Graph vocabulary (§1), `LanguageAdapter`, `Plugin` | **Contract** — semver'd from 1.0; WASM ABI versioned independently |
| `Engine` facade (§5) | **Contract** — semver'd from 1.0; the only surface frontends may touch |
| `Analysis`, cache layouts | Internal — may change any release (cache self-invalidates) |
| Output schema | Contract — see [Output schema (JSON)](#output-schema-json) |

## Output schema (JSON)

**Status:** Accepted · Normative for `--format json`. Versioned: `schema_version` uses semver;
additive = minor, breaking = major (RFC 0006 §4). A machine-readable JSON Schema
(`schemas/kndo-output.schema.json`) is generated from the Rust types at build time and must
round-trip these examples in CI.

### 1. Envelope

```jsonc
{
  "schema_version": "1.3.0",
  "kndo_version": "0.3.1",
  "run": {
    "mode": "staged",                    // "full" | "staged" | "diff"
    "base_ref": null,                    // set for "diff"
    "started_at": "2026-08-18T12:00:00Z",
    "duration_ms": 312,
    "cache": "warm",                     // "warm" | "cold" | "disabled" (--no-cache)
    "project_root": ".",
    "adapters": [ { "id": "js-ts", "files": 1240 } ],
    "plugins":  [ { "id": "kndo:nextjs", "activated_by": "manifest-dependency: next" } ],
    "abstained": [                       // categories NO analysis judged this run
      { "category": "crap", "reason": "crap: no coverage ingested — skipped (…)" }
    ]
  },
  "health": { /* §4 */ },
  "budget": {                            // diff modes, only when [delta] rules are configured (RFC 0006 §5)
    "verdict": "fail",                   // "pass" | "fail"
    "rules": [                           // in evaluation order: the two ratchets, then [delta.budget] keys sorted
      // limit/measured/over_by are always JSON numbers, counts included — a count is a
      // measurement against the same scale as a health drop, and one type keeps a consumer
      // from having to branch on the rule name to know what it is reading.
      { "rule": "max-health-drop", "limit": 0.0, "measured": -1.7, "verdict": "pass" },
      { "rule": "max-net-findings", "limit": 0.0, "measured": 1.0, "verdict": "fail", "over_by": 1.0 },
      { "rule": "defect", "limit": 0.0, "measured": 0.0, "verdict": "pass" }
    ]
  },
  "findings": [ /* §2 — in diff modes: only new findings */ ],
  "fixed": [ /* §3 — diff modes only */ ],
  "baseline": { "acknowledged": 412, "stale": 3 },
  "suppressed": { "inline": 9, "config": 2 },
  "elided": 47,                          // `--only` narrowed these away; ABSENT when nothing was narrowed
  "diagnostics": [ { "level": "warn", "message": "coverage report older than 7d — ignored" } ]
}
```

**`run.plugins` (normative).** The plugins that actually ran, in registration order — a
plugin appears here **only if it was active**, so `activated_by` answers *why*, never
*whether*. Its value is the composition layer's own verdict, rendered once
(`ActivationReason`'s `Display`) and reported unchanged: `"manifest-dependency: next"` /
`"file-exists: next.config.*"` for a plugin whose own activation rule fired (the rule itself,
because "a rule matched" does not answer the question), `"dependency of <id>"` for one another active
plugin implied through `dependencies`, `"always-on"` for a built-in that declares no
rules, `"registered"` for one whose presence *is* the opt-in (a `.kndo/plugins/` drop-in, or an
embedder's explicit set). The engine never derives these: whoever activated a plugin says why,
which is what keeps this field and `kndo doctor` from disagreeing. New reason spellings are
additive; consumers must not exhaustively match on the string.

**`run.abstained` (normative).** A category listed here was **not judged** this run: the analysis
that owns it could not (no ingested coverage report for `crap`, no test roots for `untested`) and
emitted nothing. Consumers must read a listed category as *unknown*, never as clean — zero
findings in an abstained category is the absence of a measurement, not a passing verdict. Absent
categories were judged, so their emptiness does mean clean. Usually `[]`. The same value drives
the `stale` rule (a pragma naming an abstained category is never reported matched-nothing) and
the health axes, so the three can never disagree.

**`elided` vs `suppressed` (normative).** Both say a finding is not in `findings`, and they
are not interchangeable. `suppressed` counts findings *acknowledged* — an inline pragma or a
configured/flagged skip; a consumer may treat them as known and accepted. `elided` counts
findings the caller's `--only` lens did not ask for; they are neither acknowledged nor clean,
merely out of view, and a consumer that read a narrowed run as a clean one would be wrong.
Absent (never `0`) when nothing was narrowed. `--only` narrows every category alike, `stale`
included — the lens is one invocation's scope, not a stored policy, and this count is what
keeps it from hiding anything silently.

Diagnostic levels: `info` · `warn` (the run degraded but ran) · `error` (M6, additive) — the
run could not do what was asked (a `--diff` base that doesn't resolve): frontends exit 2 when
any error-level diagnostic is present, so an analysis that never ran can never read as a clean
pass. Consumers must treat unknown levels as at least `warn`.

### 2. Finding

```jsonc
{
  "id": "kndo-a3f81c92e5d4",            // stable content-anchored id, §5
  "category": "unused",                 // verdict; registry in §6
  "group": "waste",                     // the verdict's nature: defect | waste | risk | hygiene | convention
                                         // (fixed mapping, §6) — convention is reserved for plugin-contributed
                                         // findings (RFC 0018 §2.1); a core analysis never emits it
  "subject_kind": "function",           // what the verdict landed on: symbol kind | file | directory | dependency | import | suppression
  "severity": "warning",                // "error" | "warning" | "info"
  "confidence": "certain",              // "certain" | "probable" | "possible"
  "message": "calcLegacyTax() is unreachable from any production or test root",
  "location": { "path": "src/billing/tax.ts", "range": { "start": [41,1], "end": [78,2] },
                "symbol": "calcLegacyTax", "package": "@org/billing" },   // owning workspace package (RFC 0011)
  "rolled_up": 50,                      // rollup ladder: how many findings this one subsumes; ABSENT when it subsumes nothing
  "related": [                           // evidence chain (also what `kndo explain` renders)
    { "role": "cause", "path": "src/billing/index.ts", "range": { "start": [12,1], "end": [12,42] },
      "note": "last production reference removed by this change" }
  ],
  "sources": ["adapter:js-ts", "plugin:kndo:nextjs"],  // §2.1 — ABSENT when nothing claimed the subject
  "delta": "new",                        // diff modes: "new"; absent in full mode
  "delta_origin": "derived",             // diff modes: "introduced" (inside the change set — dead on arrival) | "derived" (flipped by it); RFC 0004 §6
  "advisory": true                       // RFC 0018 §2.2: never influences exit codes/budgets; only ever present (as true) on plugin: findings without a [plugins.gate] opt-in
}
```

**A finding whose subject spans several places** (`duplicate` over identical files,
`version-skew` over disagreeing manifests) **anchors on the lexicographically-first member and
carries every member — that one included — in `related`.** Normative: `location` makes the
finding addressable, `related` makes it complete, and only then may `message` summarize
("… and 3 more"). A consumer must never have to read the prose to learn which places a finding
covers. The anchor is presentation, not identity: `id` for those categories stays keyed on the
content hash or the coordinate, so renaming one member while the group survives is the same
finding, not a new one.

#### 2.1 `sources` (normative)

The adapters and plugins whose **facts** the finding's subject rests on, sorted, deduplicated,
spelled `adapter:<adapter-id>` · `plugin:<coordinate>` · `core:surface`. Same values and same
derivation as a `describe` envelope's `sources` — one index answers both, so the two can never
disagree about a node.

A subject contributes:

- the adapter that **claimed** its file, whether or not any edge touches it (a claimed file's
  declarations, spans and metrics are that adapter's facts);
- every adapter or plugin that contributed an **edge touching** it, in either direction — an
  incoming reference is as load-bearing as an outgoing one, which is what puts a plugin's name
  on the symbol it keeps alive;
- for a **dependency** subject, the adapters that read the manifest declaring it. A dependency
  declared and never imported has no edge anywhere, and the manifest's readers are the only
  honest answer;
- for a **directory** subject (the rollup ladder), every file underneath — a rollup stands in
  for exactly those findings, so its provenance is exactly theirs;
- every `related` path as well as the anchor, so a finding that spans places names every
  component it rests on.

**Absent means no component was involved, and is a real answer.** `duplicate` over two
identical files no adapter claims rests on nobody's facts: the core hashed the bytes. Consumers
must read absence as "no adapter or plugin contributed", never as "not recorded".

**What `sources` cannot say.** It names components whose facts are *present*. It can never name
the plugin that would have kept a symbol alive had it activated — a verdict of absence
(`unused` is the whole category) rests on the silence of every component that ran, and silence
has no provenance. `run.plugins` (§1) is the field that says who ran.

**Two fields this object deliberately does NOT have.** `evidence` — a category-specific block —
was specified before any category had one, and no analysis has since produced a fact that
`related` cannot carry; a per-category schema invented ahead of its first consumer is a shape
every consumer would have to tolerate and none could rely on. `remediation` — the advice a
finding carries travels *inside* `message`, where it is written by the analysis that knows the
subject (`deep-import` is the worked example); lifting it into its own field means committing to
computed remediation prose for every category, which is a product decision, not a serialization
one. Both stay cut rather than emitted null: a field that is always `null` teaches a consumer to
stop reading it.

### 3. Fixed finding (diff modes)

Same shape as a finding, with `"delta": "fixed"` and the *previous* location. Lets CI/agents
credit improvements and lets pre-commit output celebrate deletions.

### 4. Health

```jsonc
{
  "score": 84.1, "grade": "B",                  // one-decimal score; A≥90 B≥80 C≥65 D≥50 F
  "previous": { "score": 82.0, "grade": "B" },  // full mode: last snapshot (.kndo/health.json), if any;
                                                // diff modes: the computed "before" side
  "categories": [                               // always all computed categories, each with ratio+penalty
    { "category": "unused-symbols", "ratio": 0.031, "penalty": 6.2, "count": 47 },
    { "category": "duplication",  "ratio": 0.058, "penalty": 7.1, "tokens_duplicated": 8412 },
    { "category": "crap",         "ratio": 0.2, "penalty": 4.0, "count": 12, "crapload": 1912.4,
      "coverage": "coverage-lcov coverage/lcov.info (2d old)" }   // or "none"
  ],
  "packages": [                                 // RFC 0011 §6 breakdown; present only when ≥ 2
    { "package": "@demo/a", "score": 91.0, "grade": "A" }         // packages own claimed files
  ]
}
```

Category names in the breakdown: `unused-symbols`, `unused-dependencies`, `unused-files`,
`test-only`, `duplication`, `crap`, `cycles`, `internal-only`, `untested` (the last omitted
when the project has no test roots — RFC 0005 §11's gate). Weights, saturation constants, and
ratio definitions are normative in RFC 0005 §11.

### 5. Finding id stability

`id = "kndo-" + hash(category, subject_kind, project-relative path, symbol path (not line numbers),
category-specific discriminator)`, truncated to 12 hex chars. Line/column changes do **not** change the id; renames
and moves do (a rename is a different code object). Guarantees: an agent that fixes finding X can
re-run kndo and assert X is absent; a baseline survives reformatting.

### 6. Category registry (1.0)

Categories are pure verdicts (RFC 0005 taxonomy rule):
`unused`, `test-only`, `untested`, `undeclared`, `unresolved`, `version-skew`, `duplicate`,
`internal-only`, `private-type-leak`, `cyclic`, `deep-import`, `crap`, `stale`.
New categories are additive (minor bump); consumers must ignore unknown categories.

**Plugin-contributed findings (RFC 0018, landed).** Categories under the `plugin:` prefix —
`plugin:<coordinate>/<rule>`, host-assembled from the emitting plugin's registered identity —
are third-party verdicts, always in group `convention`, and are OUTSIDE the zero-FP statement
that covers the bare categories above (RFC 0005 §9). They carry `advisory: true` unless a
`[plugins.gate]` entry in `kndo.toml` opts the coordinate (or `<coordinate>/<rule>`) in, at
which point severity is capped at the configured level (lower than declared, never higher).
An `advisory` finding never influences exit codes or budgets, whatever its `severity`. The
prefix and group remain reserved: no core category or group may claim either.

Each category maps to exactly one `group` — `defect` (unresolved, undeclared, version-skew,
private-type-leak), `waste` (unused, test-only, duplicate, internal-only), `risk` (crap, cyclic,
untested, deep-import), `hygiene` (stale) — normative mapping in RFC 0005. The
field is redundant with `category` by design: it is included so consumers section and sort
without maintaining the mapping themselves. New groups are additive; consumers must render
unknown groups after known ones rather than dropping their findings.

What the verdict landed on travels in `subject_kind`: the `SymbolKind` names from
[Core traits](#core-traits) in kebab-case, plus `file`, `directory`, `package`,
`dependency`, `import`, `suppression`. Suppression/config targets may append the subject as
`category:subject` (e.g. `unused:enum-member`, `test-only:dependency`). Subject kinds are
additive like categories and are not a registry of their own. Human renderers compose the
two (`unused (dependency)`); JSON consumers filter on either axis independently.

### 7. SARIF mapping

`category` → `rule.id`; `severity` → SARIF `level` (error/warning/note); evidence chain →
`relatedLocations`; confidence → `properties.confidence`. One run object per kndo run.

### 8. Query envelopes (navigation verbs, RFC 0007)

All navigation verbs share one envelope; `result` is verb-specific. Listings are always capped
and carry explicit `elided` counts (RFC 0007 §2) — consumers must treat `elided > 0` as "there is
more", never as "that's all".

```jsonc
{
  "schema_version": "1.3.0",
  "query": { "verb": "used-by", "selectors": ["src/billing/tax.ts#calcLegacyTax"],
             "flags": { "depth": 1, "split_by_color": true }, "id": "q1" },   // id: query-mode echo, optional
  "run": { "cache": "warm", "duration_ms": 74 },
  "status": "ok",                        // "ok" | "not-found" | "error" (per request)
  "results": [ { /* one verb-specific result per selector, argument order */ } ],
  "diagnostics": []
}
```

Verbs accept multiple selectors; `results` always aligns 1:1 with `query.selectors` (a failed
selector yields an inline `{ "status": "not-found" | "error", … }` entry without failing its
siblings). In `kndo query` mode (RFC 0007 §4.7) this same envelope is emitted as one JSON Line
per request, in input order, `run` appearing only on the first line (shared graph snapshot).

Common building blocks:

```jsonc
// NodeRef — every node mention, everywhere:
{ "selector": "src/billing/tax.ts#TaxTable.lookup", "kind": "method",
  "color": "test-only", "span": { "path": "src/billing/tax.ts", "start": [90,3], "end": [104,4] } }

// EdgeRef — every edge mention:
{ "edge": "references", "confidence": "certain",
  "site": { "path": "src/billing/index.ts", "start": [12,10], "end": [12,23] } }
```

Verb result shapes (fields beyond these are additive/minor):

- **find**: `{ "matches": [NodeRef…], "elided": N }` — ranked.
- **describe**: `{ "node": NodeRef, "declaration": {…}, "degree": { "in": {...by edge kind}, "out": {…} },
  "reached_by_roots": [NodeRef…], "metrics": { "cyclomatic": 14, "crap": 36.2, "coverage": 0.12 },
  "findings": [finding-id…], "uses": [ {NodeRef, via: EdgeRef}… ], "used_by": [ … ],
  "elided": { "uses": N, "used_by": M } }`.
- **uses / used-by**: `{ "node": NodeRef, "entries": [ { "node": NodeRef, "via": EdgeRef,
  "depth": 1 }… ], "by_color": { "production": N, "test-only": M, "tooling": K },
  "elided": N }`.
- **trace**: `{ "from": NodeRef, "to": NodeRef, "paths": [ { "hops": [ { "node": NodeRef,
  "via": EdgeRef }… ], "weakest_confidence": "possible" }… ], "paths_elided": N }` —
  liveness traces set `"from"` to the root found.
- **impact**: `{ "node": NodeRef, "affected": [ { "node": NodeRef, "via": EdgeRef,
  "depth": N }… ], "by_color": {…}, "elided": N,
  "affected_roots": [ { "kind": "production"|"test"|"tooling", "node": NodeRef }… ],
  "affected_roots_elided": N, "if_deleted": { "newly_unreachable": [NodeRef…],
  "newly_unreachable_elided": N, "newly_test_only": [NodeRef…],
  "newly_test_only_elided": N, "freed_dependencies": [name…] } }` — `if_deleted` present
  only with the flag. `affected` reuses uses/used-by's depth-annotated entry shape (one
  grammar, not two); the simulation reports *typed reachability flips* rather than
  synthesized §2 finding objects — the flips are the graph-level fact, and fabricating
  finding ids/messages for findings that don't exist yet would put untruths in the envelope.

Query exit codes are defined in RFC 0007 §6 and are part of this contract.

#### 8.1 `explain` (normative)

`kndo explain <finding-id>` answers in the §8 query envelope with `verb: "explain"` and the
id in `selectors`. Each result is:

```jsonc
{
  "finding": { /* §2, verbatim — the same object `check` reported */ },
  "subject": { /* §8 describe result for what the finding landed on */ },
  "subject_selector": "src/billing/tax.ts#calcLegacyTax"
}
```

Normative points:

- **It is a pair, never a derivation.** The finding's own message, `related` chain, `sources`
  and `rolled_up` are the explanation the analysis already wrote; the subject block is
  `describe`'s answer about the node. A consumer comparing `kndo explain <id>` with
  `kndo describe <subject_selector>` must see the same node facts.
- **`subject` is absent when the subject is not one graph node** — a directory rollup stands
  in for many files, and a path the graph never saw has none. `subject_selector` is absent
  with it in the rollup case, and present-without-`subject` when a selector was formed but
  did not resolve. The finding is still returned: an explanation with less context beats an
  invented node.
- **An id nothing reported is `not-found`, not `error`** — the exit-code tier every other
  verb's unresolvable selector uses. A finding can be missing because it was fixed,
  suppressed, or acknowledged in a baseline since the reader saw it, and the message says so
  rather than implying a typo.

### 9. Agent format (`--format agent`)

A line-oriented plain-text rendering of the same data, optimized for LLM context windows:
maximum information per token, deterministic grammar, no decoration. Versioned independently of
the JSON schema (`agent-format 1` in the header); grammar changes bump the version and the old
version stays available for one release cycle, like JSON majors.

```
kndo 0.3.1 agent-format 1 | mode staged | cache warm | 312ms
result: 3 new, 2 fixed, net +1 | health 82.4 -> 84.1 (B) | baseline 412 acknowledged
budget: fail (2/3) | health-drop<=0 ok -1.7 | net<=0 FAIL 1 over-by 1 | defect<=0 ok 0
new:
1. [kndo-a3f81c92e5d4] unused function src/billing/tax.ts:41 calcLegacyTax
   cause: last production reference removed by src/billing/index.ts:12 (this change)
   fix: delete calcLegacyTax() and its export in src/billing/index.ts:12
2. [kndo-9c04d1b2aa7e] test-only function src/util/csv.ts:8 exportCsv (2 test roots: src/util/csv.test.ts)
   fix: delete exportCsv() together with its tests
fixed:
3. [kndo-77b0e4f2c19d] unused dependency package.json date-fns
more: none
next: kndo explain <id> | kndo used-by <selector> --format agent
```

Grammar rules (normative):

- **Header + result lines always first**, fixed field order, `|`-separated. An agent reads two
  lines and knows the outcome.
- **`budget:` line** appears only when `[delta]` rules are configured (RFC 0006 §5): overall
  verdict + one `rule<=limit ok|FAIL measured [over-by N]` segment per rule — a failing agent
  reads `over-by` and knows exactly how much work remains, without interpretation. Absence is
  itself information: it says no budget was configured, never that every budget held.
- **One finding = one numbered line**: `N. [id] <category> <subject_kind> <path:line> <name>`,
  followed by optional indented `cause:` / `fix:` / `evidence:` lines. Numbers let a model refer
  to findings cheaply ("fix 1 and 3"); ids are the durable anchors.
- **Findings appear in group order** (defect, waste, risk, hygiene) within `new:` / `fixed:` /
  `findings:` blocks — same triage order as every other renderer.
- **Elision is always explicit**: `more: 47 unused (kndo check --only unused --format agent)`
  or `more: none`. A model must never have to guess whether it saw everything.
- **`next:` closes every response** with the drill-down commands relevant to what was shown —
  affordances travel with the data, so the model needn't memorize the CLI.
- Confidence below `certain` is appended in parentheses (`(probable)`); severity is implied by
  group/category and never repeated per line.
- Navigation verbs (RFC 0007) render in the same grammar: numbered entries of
  `[selector] kind path:line` plus the verb's specifics (depth, via-edge, cycle path), same
  `more:`/`next:` discipline. `kndo query` (JSONL) is unaffected — it stays JSON by nature.
- Encoding: UTF-8, no ANSI, no glyphs, stable across `--threads` and cache states (RFC 0008 §4).

The agent format is a *rendering* of `RunResult`/`QueryResult` — it can never carry information
absent from the JSON, and anything added to it must land in the JSON schema first. Like JSON and
SARIF it renders **core-side** (machine formats, contracts §5): every frontend — CLI today,
`kndo serve`/MCP tomorrow — emits byte-identical agent text.

## WASM ABI

**Status:** Accepted, both v1s shipped · Normative for the WASM tier of ADR 0003 and RFC 0003
§§2–3. Code must match this document; changing either requires updating both in the same PR.

### 0. What this is

ADR 0003 splits extensions into two tiers: first-party adapters/plugins compiled into the
`kndo` binary, and third-party ones shipped as WASM components against a versioned ABI —
`kndo-plugin-api`. **Two independently-versioned WIT packages live under that one crate**, one
per native trait: `kndo:adapter@0.1.0` bridges `LanguageAdapter` (§§1–4 below),
`kndo:plugin@0.1.0` bridges `Plugin`'s four graph-mutation hooks (§5 below). Independent
versioning is deliberate (contracts/core-traits.md §6: "WASM ABI versioned independently") —
a breaking change to one package's shape never forces a lockstep bump of the other, and the
small vocabulary overlap between them (`file-class`, `root-kind`, `ref-kind`, `confidence`) is
duplicated rather than shared for the same reason.

Both packages cover only their v1 scope — real, working, and deliberately smaller than the
native trait's full surface; §2 and §5.2 each list their own cuts and why. `Plugin`'s
`ingest_coverage`/`suppress` hooks are not bridged by either package yet.

### 1. The adapter WIT world

`crates/kndo-plugin-api/wit/adapter.wit`, package `kndo:adapter@0.1.0`, world `adapter`:

```
export descriptor: func() -> adapter-descriptor;
export claim: func(path: string) -> option<file-claim>;
export extract: func(path: string, content: string) -> file-facts;
```

`adapter-descriptor`, `file-claim`, and `file-facts` are v1-scoped mirrors of the native
`AdapterDescriptor`/`FileClaim`/`FileFacts` (contracts/core-traits.md §2) — see the WIT file's
own doc comments for the field-by-field mapping and what each omission means. The three
functions are the whole world: **no host-import callbacks exist in v1** — a component never
calls back into the host. That is what lets the reference guest
(`examples/kndo-plugin-demo`) target plain `wasm32-unknown-unknown` with zero WASI: there is
nothing for it to import, so there is no ambient fs/net surface to sandbox *away* — the
target itself has none.

### 2. v1 scope cuts, and why

Every cut below is the same shape of decision this project makes elsewhere (CSS's deferred
selector extraction, JSON's non-source-language non-goals): ship the honestly-smaller thing
that's fully correct, rather than a bigger thing with a hidden gap.

- **No `claim_manifest`/`extract_manifest`/`resolve`.** The host bridge (`WasmAdapter`)
  answers all three itself without ever calling the guest: `claim_manifest` is always
  `false`, `extract_manifest` always returns `ManifestFacts::default()`, `resolve` always
  returns `Resolution::Unresolved` — the exact posture the JSON and CSS adapters already
  document for their own non-applicable trait methods (docs/adapters/json.md, css.md). A v1
  external adapter therefore has no manifest, no dependency graph, and no cross-file import
  resolution; `unused`/`test-only`/`untested` are real for it (declarations + references +
  roots is exactly what reachability consumes), but `cyclic`, `deep-import`, and dependency
  hygiene see nothing.
- **No `ResolveCtx` host-import callbacks.** `resolve()`'s real job needs `ResolveCtx`'s
  querying API (`contains`, `workspace_member`, `unit_files_from`, `files_in_dir`,
  `files_under` — contracts/core-traits.md §2), which only makes sense as **host-import**
  functions a component calls back into — the opposite data-flow direction from everything
  else in v1. Adding it is what a v2 needs to make `resolve()` real; deliberately deferred
  until an external adapter actually wants cross-file resolution (the same "don't build the
  mechanism before the demand" call RFC 0003 §6 makes for custom analyses).

  When that v2 lands, the unit query it exposes must be `unit_files_from(unit, from)` — the
  importer-relative one — and **not** the repo-global `unit_files`. A unit key is unique only
  within a package, so the global form hands a resolver candidates from unrelated modules that
  merely share a package or target name; picking among them by path order invents cross-module
  edges that `cyclic` reports as package cycles no source supports. Every compiled-in adapter
  that resolves by unit hit this (see RFC 0012 §8). Exposing the global form across the ABI
  would rebuild that footgun at the boundary where it is most expensive to change later.
- **No visibility ladder, no cycle policy, no `resolves_dependency_usage`.** The host fills
  in the same safe defaults CSS/JSON already use for a language with no such semantics: an
  empty visibility ladder (every declaration reports the widest level — the ladder's own
  conservative-mapping rule, contracts/core-traits.md §2), `Idiomatic` cycle tolerance at
  both levels, `resolves_dependency_usage: false`. A v1 external adapter is exempt from
  `internal-only`/`private-type-leak` rather than risk a wrong ladder guess. The mechanism is
  worth stating precisely, since it is uniformity rather than an explicit skip: the bridge
  assigns every declaration `VisibilityLevel(0)`, so the "is the referenced type narrower than
  the declaration?" comparison is always `0 < 0` and never fires. `private-type-leak`'s
  `surface_transitive` gate reaches the same answer independently — an empty ladder has no rung
  at any level, and the check treats a missing rung as surface-transitive (degrade toward
  keep-alive), so the gate passes and the comparison below it stays the deciding step.
- **UTF-8 text content, not raw bytes.** `extract`'s `content` parameter is a WIT `string`
  (valid UTF-8 by construction), not `list<u8>` — simpler for v1, at the cost of an adapter
  for a language with non-UTF-8-safe source files not being expressible yet. Every launch
  language's grammar already assumes UTF-8 source in practice, so this has cost nothing so
  far.
- **`SymbolKind::Other(name)` isn't representable.** An adapter-specific facet (Rust's
  `"macro"`, Go's `"type"`) has nowhere to go in the v1 enum; a WASM adapter needing one
  today folds it into the nearest listed kind.

None of these are silent: every one is enforced by the host bridge never calling the guest
for the corresponding native method (§3), not by a guest-side promise the host has to trust.

### 3. The host bridge (`crates/kndo-plugin-api`)

`WasmAdapter::load(path: &Path) -> Result<WasmAdapter, LoadError>` loads an **already
componentized** `.wasm` file (component-model binary — see §6 for how one gets produced) and
returns a value implementing `kndo_core::adapter::LanguageAdapter` directly. From the
`Engine`'s side this is indistinguishable from a compiled-in adapter (ADR 0003: "the WASM ABI
is a generated bridge over [the native traits]") — it goes on the very same
`Vec<Box<dyn LanguageAdapter>>` `default_adapters()` returns.

**Fuel budget (RFC 0003 §3).** Every guest call runs under a fixed fuel allowance
(`FUEL_PER_CALL` in `host.rs`); a call that exhausts it or traps is caught and converted to a
conservative empty result — `None` from `claim`, or `FileFacts::default()` plus a `Warn`
diagnostic from `extract` — never a crashed `kndo check`. One misbehaving external adapter
degrades to silence for its own files, not a broken run for every other language in the
project.

**Memory ceiling (`MAX_GUEST_MEMORY_BYTES` in `engine.rs`, 256 MiB).** Fuel bounds *work*, not
*bytes*: `memory.grow` costs a handful of fuel units and commits megabytes, so fuel alone lets
a guest exhaust the host long before it exhausts its allowance — and that failure arrives as an
OOM kill, which no `catch` converts to silence. Every store therefore installs a
`wasmtime::StoreLimits` capping guest memory; exceeding it fails the `memory.grow` inside the
guest, which reaches the host as an ordinary trap and takes the same degrade-to-silence path as
fuel exhaustion. Deliberately memory-only: table and instance counts are bounded by the
component's own type section, which the host validates at load.

Note for implementors: `StoreLimits::default()` is *unlimited*, and every store's data type in
this crate derives `Default`. A limiter that is merely a field of that data is decorative — the
value must be assigned explicitly before `Store::limiter` is installed.

**No wall-clock deadline — and this is a rejection, not a deferral.** `epoch_deadline` bounds
elapsed time, and kndo guarantees byte-identical output across thread counts and machines
(`threads_determinism`, `patch_equivalence` in the named gates). A guest cut off by elapsed
time contributes different facts on a loaded machine than on an idle one, which is precisely
the property those gates exist to forbid. Fuel is instruction-counted and therefore
deterministic; it is not a cheaper stand-in for a deadline, it is the correct instrument, and
the memory ceiling above closes the one hole fuel genuinely had.

**Sandbox.** No WASI is linked into the host's `Linker` at all — v1's world has no imports to
satisfy, so there is nothing to grant. This is stronger than a policy promise: a component
that somehow declared a WASI import would fail to *instantiate*, not silently receive
capabilities nobody meant to give it.

**Execution model: an instance pool, performance parity as a contract.** The component is
Cranelift-compiled once per load (against a process-wide shared engine whose disk compilation
cache makes a previously seen component's load skip codegen entirely), and `claim`/`extract`
run against a **pool of instances**: each concurrent call checks one out — instantiating a
fresh one from the shared compiled component when all are busy — and returns it afterward.
Graph assembly's parallel extraction phase therefore parallelizes a WASM adapter's files
exactly as it does a compiled-in adapter's; nothing serializes on a shared guest. Two
consequences are normative:

- **`claim`/`extract` must be pure functions of their arguments.** Calls may land on any
  instance in any order; instance state must not be relied on between calls. This was always
  the contract in effect — the facts cache (ADR 0004) has served any file's facts from any
  prior run since M1, so a call-order-dependent guest was already broken — the pool just makes
  it observable. (Contrast the `kndo:plugin` side, where RFC 0017 §4 *guarantees* one instance
  across a round's hooks — graph-mutation rounds are sequential by design; per-file extraction
  is parallel by design. Two execution models, each documented where it binds.)
- **A trapped instance is discarded, never re-pooled** — no later call inherits a guest that
  died mid-call.

### 4. Discovery (`kndo::open`, RFC 0003 §3)

The distribution crate (`crates/kndo/src/lib.rs`) auto-discovers `.kndo/plugins/*.wasm`
relative to the project root on every `kndo::open` call — no `kndo.toml` entry needed, the
zero-config default RFC 0003 §3 already names. This section covers that project-local
directory; both `Plugin`s and (since RFC 0016 §4) `LanguageAdapter`s also auto-discover from a
global, per-machine directory, filtered by activation rules rather than unconditional — §4.1
for adapters, §5.5 for plugins. **One directory, two loaders, no naming convention**: every
discovered `.wasm` file is tried against both
`WasmAdapter::load` and
`WasmPlugin::load`; each fails to *instantiate* (not merely "doesn't look right") against a
component built for the other package's world, since wasmtime's own component type-checking
requires every world-declared export to be present with matching types. A component that
fails to load either way is skipped, not fatal to the run (§3's/§5.3's "one bad extension
doesn't take down the rest" posture, applied at load time as well as call time).
`kndo_plugin_api::WasmAdapter`/`WasmPlugin` never guess which ABI a `.wasm` file targets by its
name, path, or a magic byte prefix — the type system already answers that, so nothing else
needs to. Both loaders are feature-gated together (`external-adapters`, on by default) so an
embedder building a minimal static binary can drop the WASM runtime entirely
(`--no-default-features --features js,go,...`, ADR 0006).

Demo components shipped **in this repository** live outside the compiled product on purpose
(`examples/kndo-plugin-demo`, `examples/kndo-plugin-hooks-demo` — both excluded from the
workspace's own `members`, same convention as `spikes/perf`): "third-party" means never
statically linked, checked by keeping it structurally incapable of being one.

#### 4.1 Identity, global installation & activation (RFC 0016 §4)

`WasmAdapter::load` rejects any component whose descriptor claims a `kndo:`-prefixed id
(`host.rs`, mirroring §5.1's identity binding for plugins) — the reserved namespace is not
claimable by an external component, full stop, independent of what any first-party adapter's
own id happens to be (none of them use the `kndo:` prefix; renaming them would only churn the
graph cache key — RFC 0016 §4's own note on why that's not worth doing).

Beyond `.kndo/plugins/`, `crates/kndo/src/lib.rs`'s `compose_adapters` also scans the same
**global** directory the plugin tier uses (§5.5 — `dirs::data_dir()/kndo/plugins`,
`KNDO_PLUGIN_DIR`-overridable): each candidate's `descriptor().activation` is evaluated against
the project root before it joins composition, reusing the exact `activation::activates`/
`ActivationRule` machinery §5.5 documents for plugins — `file-exists(glob)`/
`manifest-dependency(name)`, any single match activates, an empty list never self-activates
globally. Project-local and compiled-in adapters are unconditional either way, same as their
plugin-tier counterparts. Since RFC 0017 §6, `descriptor().dependencies` participates too:
an *active* adapter (any tier) activates every global candidate it names, transitively —
the same co-activation fixpoint the plugin tier runs, shared as one generic implementation
over kind-neutral candidate identities, and reported the same way (`missing_dependencies`
on `kndo::adapter_resolution`, reason-aware status — "active (dependency of X)" — on each
global candidate). `examples/kndo-adapter-wrapper-demo` is the reference wrapper adapter
proving the chain against real components.

**Claim priority.** With project-local, global, and compiled-in adapters all in play for the
same file extension, composition orders the final `Vec<Box<dyn LanguageAdapter>>` project-local
first, then active global candidates, then compiled-in — ties within a tier broken by
descriptor id — because `graph.rs`'s claim resolution takes the first adapter in that list
whose `claim()` returns `Some`. Auditing this while implementing it found the *actual*
pre-existing order was the reverse (compiled-in first, externals appended after): a
project-local adapter could never have won a contested extension against a built-in one. That
is corrected, not merely documented, by RFC 0016 §4.

`kndo::adapter_resolution`/`kndo::global_adapter_candidates` mirror `plugin_resolution`/
`global_plugin_candidates` (§5.5) exactly — `kndo doctor` renders both the composed set with
each adapter's `activation` rules shown, and a "global adapter candidates" section listing
every `.wasm` the global directory holds, activated or not.

`kndo plugin install <coordinate>` (RFC 0015 §4, `kndo::plugin_install`) accepts adapter
components too: `wasm_probe` tries the plugin loader, then the adapter loader, and whichever
accepts the bytes carries the descriptor identity binding checks against. No installer-side
distinction between the two kinds beyond that — checksum, identity, dependency closure, and
`plugins.lock` are all kind-agnostic.

### 5. The Plugin ABI (`kndo:plugin`)

#### 5.1 The WIT world

`crates/kndo-plugin-api/wit/plugin.wit`, package `kndo:plugin@0.1.0`, world `plugin`:

```
import list-files: func() -> list<wasm-file-info>;
import symbols-in: func(path: string) -> list<wasm-symbol-info>;
import read-file: func(path: string) -> option<list<u8>>;

export descriptor: func() -> plugin-descriptor;
export classify-file: func(path: string, current: file-class) -> option<file-class>;
export contribute-roots: func() -> list<contributed-root>;
export contribute-edges: func() -> list<contributed-edge>;
export annotate-symbols: func() -> list<plugin-target>;
```

Unlike the adapter world, this one is **bidirectional** — `contribute-roots`/`contribute-
edges`/`annotate-symbols` need to *read* the graph, not just report facts about one file. Two
narrow host-import queries (`list-files`, `symbols-in`) mirror `kndo_core::plugin::GraphView`'s
own two methods exactly, rather than serializing the whole graph into every call: a guest only
pays for what it actually queries. The three "write" hooks return a `list<...>` of their
contributions in one call, the WASM analogue of filling `RootSink`/`EdgeSink`/`AnnotationSink`
via repeated `add()` calls collapsed into a single call-boundary crossing — cheaper, and it
keeps the imperative sink shape out of the wire format entirely. `classify-file` needs no
queries of its own (it only ever sees the one file it's asked about, mirroring the native
hook's own contract) and is called against a lightweight, view-less instance.

Every target is named, never addressed by an internal id — `plugin-target { path, symbol:
option<string> }`, same as `kndo_core::plugin::PluginTarget`; resolved host-side against the
same bare/qualified lookup tables `RawRoot`/`RawReference` resolve against, and an unresolvable
target is dropped silently (the same miss behavior the adapter ABI and the native `Plugin`
trait both already have).

`plugin-descriptor` also carries `activation: list<activation-rule>` — `variant activation-rule
{ file-exists(string), manifest-dependency(string) }`, the machine-checkable counterpart to
`detection`'s human-readable prose. `descriptor()` is the *only* call the host makes before
deciding whether a globally installed plugin even joins composition (§5.5); a project-local
`.kndo/plugins/*.wasm` file never has this field consulted at all.

Two RFC 0015 fields ride the same record: `id` is the plugin's *coordinate* (its fetchable
source, `github.com/<owner>/<repo>`; the `kndo:` namespace is reserved for built-ins, and the
host **fails the load** of any external component claiming it — same skipped-not-fatal handling
as an instantiation error), and `dependencies: list<string>` names coordinates of plugins whose
conventions are part of this one's (install closure + activation implication, RFC 0015 §3 —
never versions, ordering, or data flow).

**`read-file` (RFC 0016 §5's content channel, landed).** Scoped to `requested-file-access`:
the host prefetches every discovered path matching the descriptor's declared globs, budget-
charged and byte-read through the run's `ContentView` (`kndo_core::plugin::ContentView`) exactly
as a native plugin's own `.read()` calls would be, *before* instantiating each round's guest —
the guest can't make a host round-trip of its own choosing mid-call, so `read-file` on the guest
side is a lookup into that owned snapshot, not a live filesystem call. Budget accounting is
keyed by path, not by call: a component's read scope shouldn't depend on how many hooks look at
the same file — a path already charged is served again for free within the round. (This keying
predates RFC 0017 §4's one-instance-per-round lifecycle, §5.3, which removed its original
triple-charge motivation; it stays because it is the right semantics regardless.) A path outside
the declared globs, or one the budget has cut off, comes back `none` — the same silent-miss
shape every other host-mediated lookup in this ABI already has.

#### 5.2 v1 scope cuts, and why

- **No `ingest_coverage`/`suppress`.** `ingest_coverage` isn't wired on the native `Plugin`
  trait either — nothing to bridge until it's real. `suppress` went further: RFC 0016 §7
  evaluated it against real shipped components and decided cut, not merely deferred (RFC 0003
  §2) — it stays undeclared on both the native trait and this WIT package.
- **The frozen v1 records stay frozen; the read surface grew by imports instead (RFC 0017
  §5).** `wasm-file-info` (path/role/origin) and `wasm-symbol-info`
  (name/kind/exported/member-of) never gain fields — growing a record is a breaking change in
  the component model. Everything else the graph stably holds arrives through the additive
  imports `packages`/`package-of`, `file-details`/`symbol-details`, `symbol-implements`,
  `imports-of`/`importers-of`/`references-to`, `call-sites-in`, and `attr-strings-in` (each
  with its own new record type — `wasm-package-info`, `wasm-file-details`,
  `wasm-symbol-details`, `wasm-ref-site`, `wasm-call-site`, `wasm-attr-string`, `wasm-span`;
  `symbol-implements` needs none, it answers `option<string>`). `symbol-implements` is the rule applied to itself: the trait/protocol
  whose implementation declares a member is a new fact, and it arrived as its own import
  rather than a field on `wasm-symbol-details`, which is just as frozen in practice as the v1
  records once a component is built against it. It is what lets a THIRD-PARTY conventions
  plugin be its curated table, exactly like the built-in `kndo:serde`/`kndo:rkyv`/
  `kndo:wasmtime`, instead of asking for source access and re-parsing a grammar.
  `attr-strings-in` is the same rule applied a second time, and the pair it completes says
  what the rule is *for*: `call-sites-in` carries string literals written in a call,
  `attr-strings-in` carries them written in an attribute or annotation, and neither says what
  the string means. `#[serde(skip_serializing_if = "is_zero")]` names a function and
  `#[serde(rename = "is_zero")]` names a wire label; only a plugin that knows serde can tell
  them apart, and putting that knowledge in the record — or in an adapter — is the coupling
  the split exists to prevent. All answer from the same
  pre-instantiation snapshot as `list-files`/`symbols-in`, sorted and deterministic, and from
  **adapter-derived data only** (RFC 0017 §2's rule R1): no plugin ever observes another
  plugin's contributions, which is what keeps runs identical across plugin compositions. The
  snapshot clone grows accordingly — bounded `O(files + symbols + edges + content bytes)`
  per round.
- **`SymbolKind::Other(name)`/`CssRule`/`CssVariable` aren't representable** — same cut as
  §2's adapter-side one; the host bridge folds them into `variable` rather than fabricate a
  wire value.
- **No fuel-budget layer around individual host-import calls** — the *whole* hook call
  (guest logic plus every `list-files`/`symbols-in` round trip inside it) shares one fuel
  allowance, refilled per hook. A guest that queries in a tight loop pays for it out of the
  same budget its own logic does; there is no separate per-query cap.

None of these are silent: every one is enforced by what the host bridge (`plugin_host.rs`)
does and doesn't call or expose, not by a guest-side promise the host has to trust.

#### 5.3 The host bridge

`WasmPlugin::load(path: &Path) -> Result<WasmPlugin, LoadError>` loads an already-componentized
`.wasm` file and returns a value implementing `kndo_core::plugin::Plugin` directly — same
"generated bridge" posture as `WasmAdapter` (ADR 0003), on the same `Vec<Box<dyn Plugin>>`
`default_plugins()`/`Engine::open_with_plugins` accept.

**Host state and the borrow problem.** `contribute_roots`/`contribute_edges`/`annotate_symbols`
run with a real `&GraphView<'_>` (and, since RFC 0016 §5, a real `&ContentView<'_>`) borrowed
for the duration of one `assemble_from_source` call (graph.rs, RFC 0003 §2's "landed" note);
`wasmtime::Store`'s state type must be `'static`, so a live borrow can't sit inside it directly.
`WasmPlugin` resolves this by cloning exactly what `list-files`/`symbols-in` can answer, plus
every content-channel path the descriptor's globs match (`HostViewData`, built once per
graph-mutation round, not once per query), into the store's state rather than reaching for
raw-pointer plumbing across the FFI boundary — a bounded `O(files + symbols + content bytes)`
clone, once per round, and the resulting code has no `unsafe`.

**Guest lifecycle (RFC 0017 §4): one instance per graph-mutation round.** The bridge
instantiates the component when `contribute-roots` — the round's first hook in the world's
declaration order — is invoked; `contribute-edges` and `annotate-symbols` run against that
same instance, and it is dropped when `annotate-symbols` returns. Two consequences a guest
author may rely on, and one it must never rely on: guest state (statics, lazily built caches)
*persists across the three hooks of one round* — compute something in `contribute-roots`,
reuse it in `contribute-edges`; guest state *never survives into the next round or run* — the
drop is unconditional, success or trap; and a hook invoked out of order by a non-core host
gets a defensively fresh instance rather than another round's state. Stateless
request/response guests (what `wit-bindgen` produces by default) behave identically under
either lifecycle. Proven observable by the compliance suite's `staged_`/`fresh_` scenarios
against `examples/kndo-plugin-hooks-demo`.

**Fuel budget and sandbox** are the same posture and the same constant class as §3's adapter
bridge (`FUEL_PER_CALL` in `plugin_host.rs`), re-armed before *every* hook call — the per-call
budget semantics are unchanged by the shared instance; a heavy `contribute-roots` can't starve
`annotate-symbols`. An exhausted or trapped hook degrades to "this plugin contributed nothing
this round," never a crashed `kndo check`; no WASI linked, so a component declaring one fails
to instantiate rather than silently receiving capabilities.

#### 5.4 Correctness: cache and patch bypass

Same rule as the native `Plugin`'s own graph-mutation hooks (contracts/core-traits.md §3): any
registered plugin — WASM or built-in — that declares `mutates_graph()` (a `kndo:plugin`
component always does: the world exports all four hooks, so `WasmPlugin` keeps the trait's
`true` default) participates in `assemble_from_source`'s cache-key folding (RFC 0016 §6).
Coverage-only plugins (`LcovPlugin`) declare `false` and were never part of either bypass.

**The snapshot cache is reusable, the incremental patch is not — landed asymmetrically, on
purpose.** `Plugin::content_hash()` (`WasmPlugin` overrides it to the blake3 hash of its own
component bytes, computed once at `load()`; a native plugin's default `None` relies on
`PluginDescriptor.version` as its trust boundary, same discipline `AdapterDescriptor
.facts_schema_version` already established) folds into the graph cache key alongside every
discovered file's content hash. A `ContentView` never answers a path outside that same
discovered set (§5.1), so any input a plugin's hooks — including its content-channel reads —
could react to was already part of the key. That makes the graph-snapshot fast path safe: a
snapshot written under one plugin's identity can only ever match a run with the identical
component (bytes and all, for WASM) over identical inputs. The incremental patch (RFC 0017
§3) covers the other fast path without needing the key-folding argument at all: every plugin
contribution is provenance-tagged, so the patch strips them, splices the source change, and
re-runs the full plugin round against the patched graph — byte-identical to a full rebuild by
the equivalence gate, guarded by a snapshot-stored plugin-set digest (a changed set
full-rebuilds once). Nothing a plugin contributes ever rides either fast path unrevised.

#### 5.5 Global installation & activation (RFC 0003 §4)

Beyond project-local `.kndo/plugins/`, `crates/kndo/src/lib.rs`'s `activation` module also scans
a **global** directory — `dirs::data_dir()/kndo/plugins` (XDG data dir on Linux, Application
Support on macOS, `%APPDATA%` on Windows), overridable wholesale via the `KNDO_PLUGIN_DIR`
env var. This directory is not tied to any one project, so presence there can't be the opt-in
signal `.kndo/plugins/` gets to use — each candidate's `descriptor().activation` is evaluated
against the project root *before* the plugin joins composition at all:

- `file-exists(glob)` — at least one file under the project root matches (`glob` crate
  semantics, evaluated once at `kndo::open` time, not per-analysis-run).
- `manifest-dependency(name)` — any `package.json`/`Cargo.toml` under the project root declares
  a dependency by this name in any dependency section, not just the root's own
  (`kndo_core::discovery::find_files_named` — the same gitignore-aware walker `discover` itself
  uses, so `node_modules` etc. are excluded exactly like everywhere else in the product; Cargo's
  `-`/`_` interchangeability is honored). Root-only would have made every monorepo package a
  false negative for a dependency only *it* declares — not an acceptable v1 cut, since kndo's
  monorepo awareness is a first-class feature everywhere else (RFC 0012 §8/§10).

Any single matching rule activates the plugin; an **empty** `activation` list never
self-activates from the global directory (silence over a guess, the zero-false-positive
default) — such a plugin only ever runs if placed in a project's own `.kndo/plugins/` instead.
`LanguageAdapter` shares this exact mechanism since RFC 0016 §4 — §4.1 covers the adapter-side
specifics (identity, claim priority) this section doesn't repeat.

`kndo doctor` (`crates/kndo-cli/src/main.rs`'s `doctor_cmd`) reports both sides: `report.plugins`
(from `Engine::doctor`) for the final composed set, and `kndo::global_plugin_candidates(root)`
— a separate call, since `Engine` itself never sees a candidate that didn't activate — for
*every* `.wasm` file the global directory holds, each with `activated: bool` and its
`activation` rules rendered via `ActivationRule::describe`. A globally installed plugin whose
rule doesn't match isn't invisible; it shows up as inactive with the rule that didn't fire.

`kndo plugin install <coordinate>` (RFC 0015 §4, `kndo::plugin_install`) populates the global
directory from GitHub releases — checksum-verified, identity-bound (the fetched component's
descriptor id must equal the coordinate), dependency-closed, recorded in `plugins.lock` beside
the `.wasm` files. Hand-copying a file in still works and is still the project-local tier's
only mechanism; `kndo plugin list` shows such files as hand-installed rather than hiding them.

### 6. Producing a component

(The full author-facing walkthrough — project setup, descriptor fields, testing shape,
versioning/maintenance — is [docs/src/plugins/authoring.md](../docs/src/plugins/authoring.md); this section
is only the componentization mechanics.)

A third-party author needs a real component-model `.wasm` binary, not a plain core module.
Two ways, both documented rather than assumed, for either package:

- `cargo component build` (the `cargo-component` tool) — the ecosystem-standard path.
- The `wit-component` crate directly, as a library, with **no extra tool install** —
  `wit_component::ComponentEncoder::default().module(&core_wasm_bytes)?.encode()?`. This is
  exactly what `kndo-plugin-api`'s own compliance tests do to build both
  `examples/kndo-plugin-demo` and `examples/kndo-plugin-hooks-demo` fresh on every run — it
  works with zero WASI imports to satisfy (§2/§3, §5.2/§5.3), which is true of any
  v1-conformant adapter or plugin by construction.

### 7. Compliance

The suites below build their demo component fresh from source and componentize it in-process
on every run — testing today's guest source against today's host. The one deliberate
exception is the compat matrix (last entry), whose whole point is *committed, pinned* binary
components:

- `crates/kndo-plugin-api/tests/compliance.rs` — drives a `WasmAdapter` directly against a
  hand-built `Engine`.
- `crates/kndo-plugin-api/tests/plugin_compliance.rs` — drives a `WasmPlugin` directly against
  a hand-built `Engine` and its own minimal `LanguageAdapter`, exercising all four hooks
  (including the `list-files`/`symbols-in` round trip) with a baseline run proving the
  assertions aren't vacuous; also proves the two ABIs reject each other's components
  (`each_abi_rejects_a_component_built_for_the_other`) — the mechanism §4's discovery design
  depends on.
- `crates/kndo/tests/external_adapter.rs` and `crates/kndo/tests/external_plugin.rs` — go
  through the full product composition (`kndo::open`, `.kndo/plugins/` discovery included), the
  same code path `kndo-cli` uses for every command; `external_plugin.rs` drops *both* an
  adapter and a plugin component into the same `.kndo/plugins/` directory, proving §4's
  single-directory sort actually works end to end, not just at the loader level.
- `crates/kndo/tests/global_plugin_activation.rs` — same full-product composition, but through
  `KNDO_PLUGIN_DIR` (§5.5): one `#[test]` opens two temp projects against the same globally
  installed plugin — one without, one with the file that satisfies its `file-exists` rule —
  proving activation is genuinely conditional, not just wired and always-on.
- `crates/kndo/tests/global_adapter_activation.rs` (RFC 0016 §4) — the adapter-side mirror of
  the above, plus a claim-priority assertion: with the same component placed both project-local
  and in the global tier for one project, `kndo::adapter_resolution` must list the project-local
  copy first — proving §4.1's corrected composition order, not just that both tiers activate.
- `crates/kndo/tests/plugin_install_probe.rs` (RFC 0015 §4, extended by RFC 0016 §4) — a real
  component through `kndo::plugin_install::wasm_probe`; one case per kind proves the probe's
  plugin-then-adapter fallback reaches identity binding for both, not just plugins.
- `crates/kndo/tests/adapter_dependency_implication.rs` (RFC 0017 §6) — two real components
  in the global tier; satisfying only the wrapper's activation rule must activate the adapter
  it depends on (`ImpliedBy`), all the way to that adapter's findings actually firing.
- `crates/kndo-plugin-api/tests/compat_matrix.rs` (RFC 0017 §7) — the ABI compatibility
  matrix: the two reference components **pre-built and committed** under `tests/compat/`,
  loaded and hook-driven against the HEAD host with no wasm toolchain in the loop. This is
  §8's "a v1 component keeps working indefinitely" promise as a build-breaking CI job (its
  own named job in `ci.yml`, plus the ordinary workspace test run). Pre-1.0, a WIT change
  that breaks the pinned binaries is legal (authoring.md §7) — the rebuild of `tests/compat/`
  in the same commit is the explicit, reviewable record that a break happened.

`kndo plugin verify <component.wasm>` (RFC 0017 §7) packages the public half of this for
plugin authors: the exact discovery loaders, a descriptor report with lint-grade warnings,
and a real fixture-project check reporting what the component contributed.

### 8. Versioning

Each WIT package version (`kndo:adapter@0.1.0`, `kndo:plugin@0.1.0`) and the corresponding
section of this document change together, independently of each other (§0). A breaking v2 of
either package (the adapter side's `resolve()` host-import callbacks or byte-content; the
plugin side's `ingest_coverage`, or per-query fuel) is a new package version, not a silent
reinterpretation of `0.1.0` — a component built against a v1 package must keep working
against a v1-compatible host indefinitely. (The "richer `GraphView` surface" this paragraph
once listed as a breaking-v2 example turned out not to need one: RFC 0017 §5 grew it entirely
through additive imports with new record types — §5.2 above — the same evolution shape as
`read-file`.)

**Both RFC 0016 §8 phase 0 reservations are now landed**, additively, exactly as reserved:

- **`kndo:plugin`'s `read-file` host import (RFC 0016 §5).** One added import,
  `read-file(path) → option<list<u8>>` (§5.1/§5.3 above). A component built against the
  pre-§5 world simply never calls it, and the host still answers every existing import
  identically.
- **`kndo:adapter`'s component-descriptor fields (RFC 0016 §4).** The `adapter-descriptor`
  record gained `activation: list<activation-rule>` (wired and read — §4.1) and
  `dependencies: list<string>` (initially riding the wire unevaluated; RFC 0017 §6 later
  gave it RFC 0015 §3's exact co-activation semantics in the global tier, through the same
  fixpoint plugins use — the wire shape never changed). No `version` field landed — §4.1's
  own note explains why one was never needed.
  A component built against the pre-§4 world has neither field; the host reads them as empty,
  the same value the dormant reservation always implied.

Neither changed a byte of previously shipped behavior — both are the freeze committing to an
evolution *path* it had already declared, landing on schedule.

**A second world in the same package (RFC 0018).** `kndo:plugin@0.1.0` gained `world
plugin-findings` — everything `world plugin` has plus two exports (`rules`,
`contribute-findings`) and two type additions (`rule-descriptor`, `contributed-finding`).
A world's exports are mandatory, so growing `plugin` itself would have broken every
already-built v1 component; a sibling world is the additive shape for new *exports*, exactly
as new imports were the additive shape for new host surface. The host probes
`plugin-findings` first (a findings-capable component also satisfies `plugin`, so the other
order would silently strip its findings) and falls back to `plugin` — the pinned compat
components exercise the fallback on every push. Authors choose their world in `generate!`;
the scaffold targets `plugin-findings`.

### 9. Threat model

Written down explicitly (RFC 0017 §7) because the tool is published and components come from
anywhere. What a malicious or buggy component **cannot** do, by construction:

- **Read outside its grant.** No filesystem, no environment, no clocks, no network: the WASM
  sandbox has no WASI world at all — every byte a component sees arrives through a host
  import. The content channel (RFC 0016 §5) serves only files matching the component's own
  declared `requested_file_access` globs, from the already-discovered, gitignore-filtered
  tree, under a per-round byte budget whose cutoff is surfaced as a diagnostic.
- **Write anything.** There is no write-shaped import. Hook outputs are *claims about the
  graph*, applied by the host under the sink vocabulary (§5.1) — no new node/edge kinds, no
  finding creation, no file mutation.
- **Hang or exhaust the host.** Every hook call runs under a wasmtime fuel budget, re-armed
  per call (RFC 0017 §4), *and* every store under a 256 MiB memory ceiling (§3) — fuel alone
  bounds work, not bytes, and an OOM kill is the one failure no `catch` can degrade. An
  exhausted, over-committed or trapping call is dropped like any other component error —
  skipped, never fatal to the run.
- **Impersonate.** Reserved-namespace ids fail the load (§4.1/§5.5); the installer's identity
  binding refuses a component whose descriptor id differs from the coordinate it was fetched
  from, and the lockfile pins the checksum (RFC 0015 §4).

What a malicious component **can** do — the residual risk, stated honestly: **lie about graph
facts** and, since RFC 0018, **emit noisy findings**. A false root, edge, annotation, or
`classify_file` override suppresses findings that should have fired (it cannot *create* false
core findings: plugin evidence is liveness-only, RFC 0005 §1, and file-target edges are
consumed by reachability alone). A plugin's own findings can be wrong or spammy — but they
are namespaced (`plugin:<coordinate>/<rule>`), quota-capped per rule with loud truncation,
excluded from health, and **advisory by default**: without an explicit `[plugins.gate]`
opt-in they cannot move an exit code, so the blast radius of a lying rule is a mislabeled
line in a report, not a broken build. The mitigations are visibility, not prevention:
contributions are provenance-tagged in the graph, declared rules are shown by doctor/verify
before a component ever runs, and `kndo doctor` reports the per-plugin audit record from the
last run — id, roots, edges, annotations (`plugin contributions (last recorded run)`), so
"this plugin exempted 400 symbols" is a line in a report, not an invisible bias. Installing a
component remains a trust decision at exactly that scope: the worst case is quieter output or
noisier advisory lines, never exfiltration or code execution.
