//! The unified extension surface — ONE species. An extension declares everything
//! it does in an [`ExtensionSpec`] and implements the hooks for the capabilities
//! it declared; the engine routes by the spec and never invokes an undeclared
//! hook. Claims gate the extraction cluster; activation gates conduct and
//! ingestion — gathering evidence is ungated fact-collection, emitting judgment
//! is gated. Phase discipline lives in the signatures: an extraction hook cannot
//! name the graph because no parameter provides one.

use crate::adapter::{Resolution, ResolveContext, SourceFile};
use crate::evidence::{
    CoverageRecords, Declaration, DeclarationId, EvidenceSink, EvidenceStreams, ImportShape,
    Marker, RelationKind, RootKind, SymbolKind,
};
use crate::finding::Severity;
use crate::manifest::{ManifestSink, UnitKind};
use crate::vocab::{Confidence, ProjectPath};
use serde::Serialize;
use smol_str::SmolStr;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// How this language SHAPES its namespace nodes — what the engine builds the
/// scope forest's middle layer from.
///
/// Only the two forms with a consumer are here. The design also names `Flat`,
/// `ByDirectory` and `Mounted`; each is already achieved from the other side —
/// the adapter emits the clause its language writes (`package com.a`,
/// `package http`) or the mounts it declares (`mod x;`) — so an engine-side
/// variant nothing reads would be vocabulary without a caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub enum Nesting {
    /// The file IS its namespace: what it declares is visible to nothing
    /// without an import, and a clause it emits names the node it stands
    /// alone in. The default, and Swift's and js-ts's answer.
    #[default]
    PerFile,
    /// The CLAUSE is the whole key: every file writing `package com.google.io`
    /// stands in one namespace, wherever in the tree it sits. Java's answer,
    /// and why a package that does not match its directory is not a defect.
    Flat,
    /// The clause is keyed by the DIRECTORY that holds it: two directories
    /// writing `package foo` are two namespaces, because the directory is
    /// what the compiler compiles together. Go's answer.
    ByDirectory,
    /// The namespace is the file's PATH under the unit's source roots, dotted:
    /// `src/app/views.py` in a unit rooted at `src` is `app.views`, and
    /// `src/app/__init__.py` is `app`. The engine derives it, because the
    /// source root is the manifest's to say and extraction never sees one.
    /// `roots` are source roots the LANGUAGE knows without a manifest —
    /// empty (the usual answer) leaves the unit's own roots to say it.
    ByPath { roots: Vec<SmolStr> },
    /// The namespaces NEST, and the nesting is spelled by the files
    /// themselves: `mod x;` mounts one namespace inside another, so the
    /// forest is read off the mount edges rather than off any path. Rust's
    /// answer.
    Mounted,
}

/// One machine-checkable activation predicate — cheap, evaluated against what the
/// run already discovered, never by running extension code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ActivationRule {
    /// At least one discovered file matches this glob (e.g. `next.config.*`).
    FileExists(SmolStr),
    /// Some discovered manifest declares a dependency whose name matches this
    /// [`pattern`](matches_pattern), in any section — as the claiming
    /// extensions report it through `extract_manifest`, the one manifest door.
    /// A pattern because an ecosystem spells one framework many ways:
    /// `org.springframework*` is boot, context and web alike.
    ManifestDependency(SmolStr),
    /// Some discovered file's SOURCE names an import specifier matching this
    /// pattern (`org.junit.*`, `XCTest`) — the gate for a framework no
    /// manifest mentions: a Swift tree without `Package.swift`, a JVM tree
    /// whose build file the run never sees.
    ///
    /// COARSE by construction, and never a verdict. Activation is decided
    /// before any file is parsed (see `kndo_core::conduct::activate`), so the
    /// engine matches the pattern's literal stem against file text: a
    /// specifier named in a comment or a string opens the gate too. What the
    /// pack then DOES is decided by its triggers, which read extracted
    /// evidence qualified through the file's own bindings — the exact half.
    FileImports(SmolStr),
}

/// When an extension's CONDUCT and INGESTION run (extraction is gated by claims
/// alone). `Always` is speakable on purpose: "always on and cheap" (a coverage
/// ingester) is a real posture, not an exemption with a paragraph of
/// justification. An empty rule list is the OTHER deliberate extreme — an
/// extension that can never self-activate, reachable only through another
/// extension's `dependencies` (a company framework whose users never depend on
/// it directly).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Activation {
    Always,
    AnyRule(Vec<ActivationRule>),
}

/// One rule an extension may emit findings under; the suffix of the namespaced
/// advisory category. A finding under an undeclared rule is dropped and recorded
/// — declaration is the contract, not decoration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleDescriptor {
    pub name: SmolStr,
    pub description: SmolStr,
}

/// Whether this extension contributes to graph assembly through
/// [`Extension::contribute_roots`]. Load-bearing, not a hint: any ACTIVE
/// graph-mutating extension bypasses the persisted graph cache for the run — the
/// surgical patch never re-invokes conduct hooks, so it can never safely reuse a
/// graph one influenced. An enum rather than a bool so the decision reads at the
/// call site, and an argument of [`ExtensionSpecBuilder::conduct`] rather than a
/// defaulted field so declaring conduct without deciding it does not compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutatesGraph {
    Yes,
    No,
}

/// Whether an import cycle among this language's files is a defect worth a
/// finding — a LANGUAGE fact, declared as spec data so core never names a
/// language. `Hazard`: module-initialization order makes cycles bite (ESM/CJS
/// TDZ and partially-initialized modules; Python's circular ImportError).
/// `Tolerated` — the default — covers every silent reason at once: the
/// compiler forbids them (Go), resolves them routinely (JVM multi-pass), or
/// the module system makes them idiomatic (Rust modules within a crate);
/// information is not dressed up as a defect, and silence needs no
/// sub-classification to behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CycleTolerance {
    #[default]
    Tolerated,
    Hazard,
}

impl MutatesGraph {
    pub fn as_bool(self) -> bool {
        matches!(self, MutatesGraph::Yes)
    }
}

/// `kndo:` is the built-in namespace: an external component carrying it is
/// rejected at load, which is what makes `dependencies: ["kndo:express"]`
/// unambiguous from any source.
pub fn is_reserved_coordinate(coordinate: &str) -> bool {
    coordinate.starts_with("kndo:")
}

/// What a unit of this ecosystem publishes — the surface an outside consumer
/// can name, which no narrowing advice may touch. `Exports` (the default):
/// every exported declaration is published — a jar, a crate, a Go package, a
/// Python distribution hand out all of them, so `internal-only` never advises
/// an exported declaration here until the unit's own publication says nobody
/// outside consumes it. `Entries`: only what the unit's entries export — an
/// npm package resolves through `main`/`exports`, so an `export` in a file no
/// entry reaches is internal however it is spelled, and the analysis may say
/// so. A language fact, because it is the ecosystem's resolution rule; the
/// unit's own publication refines it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum PublishedSurface {
    #[default]
    Exports,
    Entries,
}

/// How the ecosystem's manifests state a dependency's usage scope. `Scoped`
/// manifests have sections (npm `dependencies`/`devDependencies`, Cargo
/// `[dev-dependencies]`), so a declaration with no scope is one the source could
/// not classify and no usage judgment reads it. `Unscoped` ecosystems have no
/// sections at all — every direct requirement in `go.mod` is a build
/// requirement — so an unscoped declaration IS the usage claim, and "only tests
/// import it" has no section to move it to. `unused` and `test-only` are the
/// consumers, on their dependency subjects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DependencyScoping {
    #[default]
    Scoped,
    Unscoped,
}

/// How an import specifier names a declared dependency — the ecosystem's
/// spelling, as data: the engine matches every package-shaped import of an
/// adapter's files against its manifests' declarations without calling back
/// into the adapter, and an adapter cannot claim to derive identity without
/// saying how. `unused` and `test-only` read it on their dependency subjects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DependencyIdentity {
    /// Specifiers name nothing this vocabulary can match with a declaration (a
    /// JVM artifact id names no package, a Swift package name is not its module
    /// name, a Python distribution is not its import name): every usage
    /// judgment abstains, and says so.
    #[default]
    Underivable,
    /// npm's spelling: the specifier is a package name — `@scope/name` or
    /// `name` — or a `/`-path under it (`lodash/fp` names `lodash`); a
    /// `@types/` declaration names the package it types (`@types/babel__core`
    /// names `@babel/core`).
    PackageName,
    /// Go's spelling: the specifier is an import path and a declared module
    /// path prefixes it (`github.com/x/y/sub` names `github.com/x/y`). Where
    /// the module boundary falls is the declaration's to say, so a path no
    /// declaration prefixes is reported whole.
    ModulePath,
    /// Cargo's spelling: the specifier's leading `::` segment is the declared
    /// name with `-` spelled `_` (`serde_json::Value` names `serde-json`).
    CrateRoot,
}

/// A loader's query or fragment (`normalize.css?inline`, `x?raw`) is an
/// instruction about the import, never part of the name it imports.
fn without_loader_suffix(specifier: &str) -> &str {
    specifier
        .find(['?', '#'])
        .map_or(specifier, |i| &specifier[..i])
}

impl DependencyIdentity {
    /// Does `specifier` name `dependency` under this spelling? Never asked under
    /// `Underivable` — the engine abstains first — and `false` there.
    pub fn names(self, specifier: &str, dependency: &str) -> bool {
        fn under(specifier: &str, dependency: &str, separator: &str) -> bool {
            specifier == dependency
                || specifier
                    .strip_prefix(dependency)
                    .is_some_and(|rest| rest.starts_with(separator))
        }
        match self {
            DependencyIdentity::Underivable => false,
            DependencyIdentity::PackageName => {
                let specifier = without_loader_suffix(specifier);
                under(specifier, dependency, "/")
                    || dependency
                        .strip_prefix("@types/")
                        .is_some_and(|typed| under(specifier, &typed_package(typed), "/"))
            }
            DependencyIdentity::ModulePath => under(specifier, dependency, "/"),
            DependencyIdentity::CrateRoot => under(specifier, &dependency.replace('-', "_"), "::"),
        }
    }

    /// The package a specifier names, spelled as a declaration would — what an
    /// `undeclared` finding reports. `None` for a specifier that names no
    /// package under this spelling: an empty one, a scheme-qualified one, a
    /// subpath import or alias (`#x`, `~x`, `@/x`), a scope without a name.
    pub fn package_of(self, specifier: &str) -> Option<&str> {
        if specifier.is_empty() || specifier.starts_with('#') {
            return None;
        }
        let specifier = without_loader_suffix(specifier);
        if specifier.is_empty() {
            return None;
        }
        // `node:fs`, `data:…`, `virtual:x`: a scheme names the platform's own
        // resolution, never a package — under a `/`-separated spelling.
        let scheme_qualified = specifier
            .split('/')
            .next()
            .is_some_and(|first| first.contains(':'));
        match self {
            DependencyIdentity::Underivable => None,
            DependencyIdentity::PackageName => {
                if scheme_qualified || specifier.starts_with(['#', '~', '.', '/']) {
                    return None;
                }
                let mut parts = specifier.splitn(3, '/');
                let first = parts.next()?;
                if let Some(scope) = first.strip_prefix('@') {
                    let name = parts.next().filter(|n| !n.is_empty())?;
                    if scope.is_empty() {
                        return None;
                    }
                    Some(&specifier[..first.len() + 1 + name.len()])
                } else {
                    Some(first)
                }
            }
            DependencyIdentity::ModulePath => (!scheme_qualified).then_some(specifier),
            DependencyIdentity::CrateRoot => {
                let first = specifier.split("::").next()?;
                let identifier = !first.is_empty()
                    && first.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                identifier.then_some(first)
            }
        }
    }
}

/// DefinitelyTyped's encoding of the package a `@types/` declaration types:
/// `babel__core` is `@babel/core`, anything else is itself.
fn typed_package(typed: &str) -> String {
    match typed.split_once("__") {
        Some((scope, name)) => format!("@{scope}/{name}"),
        None => typed.to_string(),
    }
}

/// Which specifiers name the platform's own modules rather than a dependency —
/// never declared, never undeclared. Data, the way [`DependencyIdentity`] is,
/// so an adapter cannot claim a builtin set without spelling it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DependencyBuiltins {
    /// Nothing is the platform's: every package-shaped specifier is a dependency.
    #[default]
    None,
    /// A named list, each entry matched under the ecosystem's
    /// [`DependencyIdentity`] spelling (`fs/promises` is `fs`); an entry ending
    /// in `:` is a scheme prefix (`node:` covers `node:fs`).
    Named(Vec<SmolStr>),
    /// Go's rule: an import path whose first segment carries no `.` is the
    /// standard library — module paths are domain-qualified.
    UndottedFirstSegment,
}

impl DependencyBuiltins {
    pub fn covers(&self, specifier: &str, identity: DependencyIdentity) -> bool {
        match self {
            DependencyBuiltins::None => false,
            DependencyBuiltins::Named(names) => names.iter().any(|name| {
                if name.ends_with(':') {
                    specifier.starts_with(name.as_str())
                } else {
                    identity.names(specifier, name)
                }
            }),
            DependencyBuiltins::UndottedFirstSegment => specifier
                .split('/')
                .next()
                .is_some_and(|first| !first.contains('.')),
        }
    }
}

/// One reach a language can spell with a keyword, narrowest first — the
/// LADDER. `internal-only` reads it to ask whether a declaration could stand
/// on a narrower rung than the one it declares: Java spells
/// `[Owner, Namespace, Exported]` (private, package-private, public), so a
/// package-private name used only in its own file has somewhere to go, while
/// Go spells `[Namespace, Exported]` and the same evidence is noise. Grows as
/// languages teach us rungs; a rung this build does not know sorts above every
/// one it does, so an unknown rung never makes a narrower one appear.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Rung {
    /// Nameable only inside the declaration that owns it (Java's `private`).
    Owner,
    /// Nameable only inside its own file.
    File,
    /// Nameable inside its namespace (a Java package, a Go package).
    Namespace,
    /// Nameable inside a directory subtree (Go's `internal` fence).
    Directory,
    /// Nameable inside its owner and every subtype of it (`protected`). Sits
    /// above the namespace for the ladder's order, but is another axis: no
    /// use pattern short of "its subtypes alone" fills it.
    Heirs,
    /// Nameable inside its build unit (Kotlin's `internal`, Rust's
    /// `pub(crate)`).
    Unit,
    /// Nameable inside the group of units one manifest aggregates (Swift's
    /// `package`).
    Group,
    Exported,
}

/// How far one namespace reaches across the project's units — the fact that
/// decides who can name a namespace-scoped declaration.
///
/// Java's package is a NAME units contribute to: `guava-tests` compiles
/// `com.google.common.io` classes against `guava`'s on one classpath, and each
/// sees the other's package-private members. Go's package and Rust's module
/// tree are the opposite: the unit owns the namespace, and two units spelling
/// the same name hold two unrelated ones. Core cannot tell which without being
/// told, so it is told.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum NamespaceSpan {
    /// The unit owns its namespaces; two units spelling one name hold two.
    /// The default, and the narrower answer: a declaration stays accused where
    /// a language has not said otherwise.
    #[default]
    Unit,
    /// A namespace is a name units contribute to, so a unit and every unit
    /// compiled against it share one.
    Compilation,
}

/// What a unit-wide reach is bounded by where NO manifest named the unit —
/// the answer core cannot derive, because it turns on whether the language
/// spells anything between a namespace and a build unit.
///
/// Swift spells nothing: a module IS a namespace IS a SwiftPM target, so a
/// source tree outside SwiftPM's reach still bounds its `internal` names —
/// the namespace clause each file declares says which module it is. Kotlin
/// spells both: a package sits inside a Gradle module and many packages share
/// one, so a package can never bound an `internal` name, and one whose unit no
/// manifest named has no bound this project can enumerate.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum UnnamedUnit {
    /// The default and the wider answer: a manifest is the only thing that
    /// names a unit, so a unit-wide reach with none is unbounded — published
    /// surface, keep-alive, never a narrower guess that would accuse.
    #[default]
    Unbounded,
    /// The namespace the file declared IS its unit where no manifest named
    /// one.
    Namespace,
}

/// What a file IS by the convention of its language's own tooling, where no
/// manifest says otherwise: the go runner's `*_test.go`, pytest's `test_*.py`,
/// a page that is its own entry. A glob over the project path, the colour a
/// matching file anchors, and the confidence the convention deserves — a rule
/// the toolchain itself enforces is `Certain`, a community habit is
/// `Probable`.
///
/// A convention, never a claim over a manifest: a unit that declares its files'
/// role (a Cargo test target, a Maven test source set) has said so already, and
/// these are read only where none did. That is what keeps a language from
/// asserting what a build system knows better.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileRole {
    pub glob: SmolStr,
    pub kind: RootKind,
    pub confidence: Confidence,
}

impl FileRole {
    /// A rule the language's own toolchain enforces — `go test` compiles
    /// exactly the `_test.go` files, and runs nothing else.
    pub fn certain(glob: &'static str, kind: RootKind) -> FileRole {
        FileRole {
            glob: SmolStr::new_static(glob),
            kind,
            confidence: Confidence::Certain,
        }
    }

    /// A habit the ecosystem keeps but nothing enforces.
    pub fn probable(glob: &'static str, kind: RootKind) -> FileRole {
        FileRole {
            glob: SmolStr::new_static(glob),
            kind,
            confidence: Confidence::Probable,
        }
    }
}

/// Which declarations can stand on a step. Kotlin's `private` is file-wide on
/// a top-level declaration and class-wide on a member — two rungs under one
/// keyword — and Java's `private` exists for members alone. The ladder says
/// so, and the advice never names a keyword the declaration cannot take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Bearer {
    #[default]
    Any,
    /// A declaration with no owner.
    Free,
    /// A declaration that is a member of another.
    Member,
}

impl Bearer {
    /// Can a declaration with (`has_owner`) stand on a step for this bearer?
    pub fn admits(self, has_owner: bool) -> bool {
        match self {
            Bearer::Any => true,
            Bearer::Free => !has_owner,
            Bearer::Member => has_owner,
        }
    }
}

/// One rung as one language spells it. Every judgment and every ordering reads
/// `rung`; `word` is what a report says out loud, because a Java developer
/// narrows a `package-private` member, not a `namespace`-scoped one. One type,
/// so the rung and the word for it can never name different things.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Step {
    pub rung: Rung,
    pub word: SmolStr,
    /// Which declarations the keyword exists for; `Any` is omitted on the wire.
    #[serde(default, skip_serializing_if = "is_any")]
    pub bearer: Bearer,
}

fn is_any(bearer: &Bearer) -> bool {
    *bearer == Bearer::Any
}

impl Step {
    /// A rung under the language's own word for it, for any declaration.
    pub fn new(rung: Rung, word: &'static str) -> Step {
        Step {
            rung,
            word: SmolStr::new_static(word),
            bearer: Bearer::Any,
        }
    }

    /// A step only a declaration with no owner can stand on (TypeScript's
    /// unexported top level; Kotlin's file-wide `private`).
    pub fn for_free(rung: Rung, word: &'static str) -> Step {
        Step {
            bearer: Bearer::Free,
            ..Step::new(rung, word)
        }
    }

    /// A step only a member can stand on (Java's and Kotlin's class-wide
    /// `private`).
    pub fn for_members(rung: Rung, word: &'static str) -> Step {
        Step {
            bearer: Bearer::Member,
            ..Step::new(rung, word)
        }
    }
}

impl From<Rung> for Step {
    /// A rung with no language word — the engine's own, which is what a report
    /// falls back to for a rung the language spelled in its evidence but left
    /// off its ladder.
    fn from(rung: Rung) -> Step {
        let word = match rung {
            Rung::Owner => "owner",
            Rung::File => "file",
            Rung::Namespace => "namespace",
            Rung::Directory => "directory",
            Rung::Heirs => "subtypes",
            Rung::Unit => "unit",
            Rung::Group => "group",
            Rung::Exported => "exported",
        };
        Step::new(rung, word)
    }
}

/// The reaches a language can spell with a keyword, narrowest first — the one
/// fact `internal-only` reads. Two questions, answered here so no analysis
/// re-derives them: which step a declaration could fall to, and what a rung
/// is called out loud.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(transparent)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Ladder(Vec<Step>);

impl Ladder {
    pub fn new(steps: Vec<Step>) -> Ladder {
        Ladder(steps)
    }

    pub fn steps(&self) -> &[Step] {
        &self.0
    }

    /// A language that states no ladder: `internal-only` stays silent for it.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The narrowest step a declaration standing on `declared` could fall to
    /// while still covering `extent` — the rung its uses actually need — and
    /// which its shape (owned or not) can take. `None` when the language
    /// spells nothing between the two: the advice would name a keyword that
    /// does not exist, so there is no advice.
    pub fn step_down(&self, declared: Rung, extent: Rung, has_owner: bool) -> Option<&Step> {
        self.0
            .iter()
            .filter(|s| s.rung >= extent && s.rung < declared && s.bearer.admits(has_owner))
            // Heirs is another axis, not a wider file or package: a member
            // used in its file alone cannot fall to `protected`, only a
            // member used from its subtypes alone can.
            .filter(|s| s.rung != Rung::Heirs || extent == Rung::Heirs)
            .min_by_key(|s| s.rung)
    }

    /// What this language calls `rung`: the ladder's word where it has one,
    /// the engine's own where the language spelled the rung in its evidence
    /// but left it off its ladder — never an empty word.
    pub fn word(&self, rung: Rung) -> SmolStr {
        self.0
            .iter()
            .find(|s| s.rung == rung)
            .map(|s| s.word.clone())
            .unwrap_or_else(|| Step::from(rung).word)
    }
}

/// What a [`DispatchRule`] watches for. Grows as the evidence grows — a
/// relation, a name pattern, a witness — each variant arriving with the
/// consumer that reads it; an extension can only trigger on evidence its own
/// files report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trigger {
    /// A marker whose path matches `path` and — when `arg` is given — carrying
    /// at least one argument matching it, on a declaration of `target` when
    /// one is named. `@Override` means something only on a method; a rule
    /// says so instead of trusting every grammar to put the annotation
    /// nowhere else.
    Marker {
        path: SmolStr,
        arg: Option<SmolStr>,
        target: Option<SymbolKind>,
    },
    /// A DECLARATION whose own name matches `pattern` — a runner's convention
    /// (`TestXxx`, `test_*`), a runtime's (`main`, `init`). `kind` narrows it
    /// to one kind of symbol; `in_files` to the files where the convention
    /// holds — the KIND of compilation the file lands in, which is its unit's
    /// kind, or [`crate::manifest::UnitKind::Test`] where the file attached
    /// itself to its namespace for test builds alone (see
    /// [`crate::evidence::Attachment`]). Only a declaration nothing owns
    /// matches: a member dispatched by name is its owner's business, and
    /// [`Trigger::MemberOf`] is how a rule says so.
    Name {
        pattern: SmolStr,
        kind: Option<SymbolKind>,
        in_unit: Option<UnitKind>,
    },
    /// A TYPE that declares a relation of `kind` to a base matching `to` —
    /// `extends XCTestCase`, `implements Serializable`, `: View`. The relation
    /// THIS file reported: a rule reads what the source said here, and the
    /// name is qualified through the file's bindings before it is compared.
    Relation { kind: RelationKind, to: SmolStr },
    /// A MEMBER whose own name matches `name` and whose OWNER matches
    /// `owner` — `test*` of an `XCTestCase`, `compareTo` of a `Comparable`.
    /// The owner is named by a trigger, which is what lets one rule state a
    /// base and its requirement together.
    MemberOf { owner: Box<Trigger>, name: SmolStr },
    /// A MEMBER of a type that reaches a base matching `base`, named by one of
    /// `members` — `compareTo` of a `Comparable`, `readObject` of a
    /// `Serializable`, `body` of a `View`. The shorthand for the requirements
    /// of a base the GRAPH CANNOT SEE: where the base is in the project, its
    /// own members are the requirements and no rule is needed.
    ///
    /// What [`Trigger::MemberOf`] over a [`Trigger::Relation`] does not do:
    /// the base is matched through the WHOLE supertype chain the project
    /// declares, of either kind. `Absent extends Optional` and `Optional
    /// implements Serializable` makes `Absent`'s `readResolve` a witness,
    /// because the serialization runtime does not care which link named the
    /// base either.
    ExternalWitness {
        base: SmolStr,
        members: Vec<SmolStr>,
    },
}

impl Trigger {
    pub fn marker(path: &'static str) -> Trigger {
        Trigger::Marker {
            path: SmolStr::new_static(path),
            arg: None,
            target: None,
        }
    }

    pub fn marker_with(path: &'static str, arg: &'static str) -> Trigger {
        Trigger::Marker {
            path: SmolStr::new_static(path),
            arg: Some(SmolStr::new_static(arg)),
            target: None,
        }
    }

    /// The same marker, narrowed to declarations of one kind.
    pub fn marker_on(path: &'static str, target: SymbolKind) -> Trigger {
        Trigger::Marker {
            path: SmolStr::new_static(path),
            arg: None,
            target: Some(target),
        }
    }

    pub fn relation(kind: RelationKind, to: &'static str) -> Trigger {
        Trigger::Relation {
            kind,
            to: SmolStr::new_static(to),
        }
    }

    /// A member of a type matching `owner`, by name.
    pub fn member_of(owner: Trigger, name: &'static str) -> Trigger {
        Trigger::MemberOf {
            owner: Box::new(owner),
            name: SmolStr::new_static(name),
        }
    }

    pub fn name(pattern: &'static str, kind: SymbolKind, in_unit: UnitKind) -> Trigger {
        Trigger::Name {
            pattern: SmolStr::new_static(pattern),
            kind: Some(kind),
            in_unit: Some(in_unit),
        }
    }

    /// The members a base outside the project requires of whatever declares a
    /// relation to it.
    pub fn required_by(base: &'static str, members: &[&'static str]) -> Trigger {
        Trigger::ExternalWitness {
            base: SmolStr::new_static(base),
            members: members.iter().map(|m| SmolStr::new_static(m)).collect(),
        }
    }

    /// Does this trigger fire on `marker`, in a file whose bindings are
    /// `cx`'s? Dispatch reads each trigger in the phase that holds its
    /// evidence, so a trigger watching something else never fires here.
    /// Every name this trigger compares THROUGH THE FILE'S BINDINGS — a
    /// marker's path, a relation's target, an external witness's base. These
    /// are names of things the project does not declare, so a rule that spells
    /// one in full matches only where the file imported it from there; a bare
    /// spelling matches whatever the language leaves unqualified, in any
    /// ecosystem. A `Name` pattern is absent on purpose: it ranges over the
    /// project's OWN declarations, which no binding qualifies.
    pub fn qualified_names(&self) -> Vec<&SmolStr> {
        match self {
            Trigger::Marker { path, .. } => vec![path],
            Trigger::Relation { to, .. } => vec![to],
            Trigger::ExternalWitness { base, .. } => vec![base],
            Trigger::MemberOf { owner, .. } => owner.qualified_names(),
            Trigger::Name { .. } => Vec::new(),
        }
    }

    pub fn matches(&self, cx: &DeclarationCx<'_>, marker: &Marker) -> bool {
        match self {
            Trigger::Marker { path, arg, target } => {
                cx.spells(path, &marker.path)
                    && arg
                        .as_ref()
                        .is_none_or(|a| marker.args.iter().any(|x| pattern_matches(a, x)))
                    && target.as_ref().is_none_or(|k| match &marker.on {
                        crate::evidence::MarkerTarget::Declaration(id) => {
                            cx.evidence.declarations[id.index()].kind == *k
                        }
                        _ => false,
                    })
            }
            _ => false,
        }
    }

    /// Does this trigger fire on the declaration `id` of `cx`'s file? As with
    /// [`Trigger::matches`], a trigger watching other evidence never fires.
    pub fn matches_declaration(&self, cx: &DeclarationCx<'_>, id: DeclarationId) -> bool {
        let d = &cx.evidence.declarations[id.index()];
        match self {
            Trigger::Marker { .. } => false,
            Trigger::Name {
                pattern,
                kind,
                in_unit,
            } => {
                // A member is its owner's business — `MemberOf` is the trigger
                // that reaches one.
                d.owner.is_none()
                    && pattern_matches(pattern, &d.name)
                    && kind.as_ref().is_none_or(|k| *k == d.kind)
                    && in_unit.is_none_or(|k| Some(k) == cx.compiled_into)
            }
            Trigger::Relation { kind, to } => cx.evidence.relations.iter().any(|r| {
                r.from == id
                        && r.kind == *kind
                        // Both spellings answer: the bare name the language
                        // wrote, and the qualified one its own bindings make
                        // of it — the same two the marker path gets.
                        && (cx.spells(to, &r.to.name) || cx.spells(to, &r.to.to_string()))
            }),
            Trigger::MemberOf { owner, name } => {
                pattern_matches(name, &d.name)
                    && d.owner.is_some_and(|o| owner.matches_declaration(cx, o))
            }
            Trigger::ExternalWitness { base, members } => {
                members.contains(&d.name)
                    && d.owner.is_some_and(|o| {
                        let owner = &cx.evidence.declarations[o.index()].name;
                        cx.reaches_base(owner, base)
                    })
            }
        }
    }
}

/// What a declaration-shaped [`Trigger`] reads: the file's own evidence, the
/// colors the project rooted the file with (sorted, deduplicated), and the
/// project's supertype edges by name.
pub struct DeclarationCx<'a> {
    pub evidence: &'a crate::evidence::FileEvidence,
    /// The KIND of compilation this file lands in — its unit's kind, or
    /// `Test` where its attachment says the namespace holds it in test builds
    /// alone. `None` where no manifest claimed the file.
    pub compiled_into: Option<UnitKind>,
    /// Direct supertype NAMES by type name, over the whole project. Names,
    /// not declarations, because the base a rule cares about is precisely the
    /// one the project does not declare — `Serializable` appears here as the
    /// target of an edge and never as a key.
    pub supertypes: &'a BTreeMap<SmolStr, Vec<SmolStr>>,
}

impl DeclarationCx<'_> {
    /// Does `pattern` match `spelled` — the path of a marker, the name of a
    /// relation — as this file writes it, or as the file's own bindings
    /// QUALIFY it? A rule written with the full name (`org.junit.jupiter.api.Test`)
    /// matches an `@Test` the file imported from JUnit and not one from
    /// another package; a rule written bare (`Override`, `test`) matches the
    /// spelling, which is what a language's implicit scope leaves behind.
    fn spells(&self, pattern: &str, spelled: &str) -> bool {
        if pattern_matches(pattern, spelled) {
            return true;
        }
        let (head, rest) = match spelled.split_once('.') {
            Some((head, rest)) => (head, Some(rest)),
            None => (spelled, None),
        };
        self.evidence.imports.iter().any(|i| {
            let ImportShape::Bindings(bindings) = &i.shape else {
                return false;
            };
            let crate::evidence::ImportTarget::Package(path) = &i.target else {
                return false;
            };
            bindings.iter().any(|b| b.local == head)
                && pattern_matches(
                    pattern,
                    &match rest {
                        Some(rest) => format!("{path}.{rest}"),
                        None => path.to_string(),
                    },
                )
        })
    }

    /// Does `owner` reach a supertype matching `base`, directly or through
    /// another type the project declares? Cycle-safe: a type graph should be
    /// acyclic, and defective evidence must not hang the run.
    fn reaches_base(&self, owner: &SmolStr, base: &str) -> bool {
        let mut seen: BTreeSet<&SmolStr> = BTreeSet::new();
        let mut queue: Vec<&SmolStr> = vec![owner];
        while let Some(name) = queue.pop() {
            if !seen.insert(name) {
                continue;
            }
            let Some(supers) = self.supertypes.get(name) else {
                continue;
            };
            for s in supers {
                if self.spells(base, s) {
                    return true;
                }
                queue.push(s);
            }
        }
        false
    }
}

/// The trigger patterns' one grammar: literal text with `*` matching any run
/// of characters, empty included.
/// The contract's one `Pattern` semantics: a glob over a qualified name, where
/// `*` spans any run of bytes. Every place a rule NAMES something it does not
/// own — a marker path, a relation's target, an activation predicate — compares
/// through here, so a pattern means the same thing wherever it is written.
pub fn matches_pattern(pattern: &str, text: &str) -> bool {
    pattern_matches(pattern, text)
}

fn pattern_matches(pattern: &str, text: &str) -> bool {
    let (p, t) = (pattern.as_bytes(), text.as_bytes());
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// What a matched [`DispatchRule`] derives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    /// The marked declaration — or the whole file, for a file marker — is an
    /// entry of this color: something outside the graph's sight runs it.
    Root(RootKind),
    /// The marked declaration is never accused of being unused: the source
    /// itself asked the dead-code judgment to stand down (`allow(dead_code)`,
    /// `@SuppressWarnings("unused")`). Lexically scoped, as such markers are:
    /// the declaration and every declaration within its extent; on a file
    /// marker, every declaration in the file — and the run says so.
    Exempt,
    /// A generator wrote this FILE: what it declares is not this project's to
    /// judge — no unused declaration, no duplicate, no complexity verdict —
    /// while its imports and references stay evidence about the code that IS.
    /// The file itself is judged like any other: a generated file nothing
    /// imports is dead weight, and the honest finding is to stop generating
    /// it. Only a file marker carries it — a generator owns files, not
    /// declarations.
    Generated,
    /// The declaration satisfies a surface its owner promised — an override,
    /// an interface method, a protocol requirement, a runtime hook a base
    /// declares. Alive while its OWNER is, and of no color: nothing outside
    /// the graph is ENTERED here, the caller simply holds the supertype and
    /// dispatches through it. A root would say something stronger and paint
    /// the file a color the source never claimed.
    Witness,
}

/// One rule of a language or a framework, as data: when this evidence
/// appears, derive that. The engine matches every file's markers against the
/// claiming extension's rules and derives roots and exemptions from the
/// matches — the one place a marker acquires meaning, for every language
/// alike. `confidence` is the derived root's: an attribute is the code's own
/// statement (`Certain`); a convention is weaker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DispatchRule {
    pub when: Trigger,
    pub then: Effect,
    pub confidence: Confidence,
}

/// What an extension IS, as data — the one manifest for every capability. Fields
/// come in three clusters with one gate each: extraction (gated by `claims`),
/// conduct (gated by `activation` + `mutates_graph`), ingestion (gated by
/// `activation` + `reads_reports`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtensionSpec {
    coordinate: SmolStr,
    version: u32,
    // -- extraction --
    suffixes: Vec<SmolStr>,
    published_surface: PublishedSurface,
    dependency_scoping: DependencyScoping,
    dependency_identity: DependencyIdentity,
    dependency_importers: Vec<SmolStr>,
    dependency_builtins: DependencyBuiltins,
    import_cycles: CycleTolerance,
    ladder: Ladder,
    namespace_span: NamespaceSpan,
    unnamed_unit: UnnamedUnit,
    nesting: Nesting,
    file_roles: Vec<FileRole>,
    dispatch: Vec<DispatchRule>,
    claims: Vec<SmolStr>,
    emits: EvidenceStreams,
    manifests: Vec<SmolStr>,
    launchers: Vec<SmolStr>,
    ignores: Vec<SmolStr>,
    ecosystem: Option<SmolStr>,
    hidden_opt_in: Vec<SmolStr>,
    // -- conduct --
    /// Whether this spec went through the conduct stage at all. Data, not
    /// inference: an empty-but-conducting spec (an always-on ingester before its
    /// paths are declared) still owes the report a contribution row, and an
    /// extraction-only spec never appears in the round — a distinction field
    /// values alone cannot draw.
    conducts: bool,
    activation: Activation,
    mutates_graph: bool,
    dependencies: Vec<SmolStr>,
    requested_file_access: Vec<SmolStr>,
    rules: Vec<RuleDescriptor>,
    // -- ingestion --
    reads_reports: Vec<SmolStr>,
}

impl ExtensionSpec {
    /// Stage one of the two-stage builder: identity and the extraction cluster.
    /// The conduct and ingestion methods do not exist here — they live on the
    /// builder [`ExtensionSpecBuilder::conduct`] returns, which demands the two
    /// gates as arguments. Forgetting a gate is not a panic; it does not compile.
    pub fn builder(coordinate: &'static str, version: u32) -> ExtensionSpecBuilder {
        ExtensionSpecBuilder {
            spec: ExtensionSpec {
                coordinate: SmolStr::new_static(coordinate),
                version,
                suffixes: Vec::new(),
                published_surface: PublishedSurface::Exports,
                dependency_scoping: DependencyScoping::Scoped,
                dependency_identity: DependencyIdentity::Underivable,
                dependency_importers: Vec::new(),
                dependency_builtins: DependencyBuiltins::None,
                import_cycles: CycleTolerance::Tolerated,
                ladder: Ladder::default(),
                namespace_span: NamespaceSpan::Unit,
                unnamed_unit: UnnamedUnit::Unbounded,
                nesting: Nesting::PerFile,
                file_roles: Vec::new(),
                dispatch: Vec::new(),
                claims: Vec::new(),
                emits: EvidenceStreams::none(),
                manifests: Vec::new(),
                launchers: Vec::new(),
                ignores: Vec::new(),
                ecosystem: None,
                hidden_opt_in: Vec::new(),
                // Inert neutrals for an extraction-only extension: activation
                // gates only conduct and ingestion, and with no conduct declared
                // there is nothing for these to gate.
                conducts: false,
                activation: Activation::Always,
                mutates_graph: false,
                dependencies: Vec::new(),
                requested_file_access: Vec::new(),
                rules: Vec::new(),
                reads_reports: Vec::new(),
            },
        }
    }

    pub fn coordinate(&self) -> &str {
        &self.coordinate
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// File suffixes this extension speaks (no leading dot), in
    /// resolution-candidate priority order. Analyses and resolution read this
    /// one list; claims derive from it at build time. Named for what it holds —
    /// "extension" already means the species.
    pub fn suffixes(&self) -> &[SmolStr] {
        &self.suffixes
    }

    /// See [`PublishedSurface`]; `internal-only`'s Exported rung is the
    /// consumer. `Exports` (the default) keeps it silent for this language.
    pub fn published_surface(&self) -> PublishedSurface {
        self.published_surface
    }

    /// See [`DependencyScoping`]; the dependency subjects of `unused` and
    /// `test-only` are the consumers.
    pub fn dependency_scoping(&self) -> DependencyScoping {
        self.dependency_scoping
    }

    /// See [`DependencyIdentity`]; the engine's dependency-usage judgment is
    /// the consumer, and `Underivable` is its abstention.
    pub fn dependency_identity(&self) -> DependencyIdentity {
        self.dependency_identity
    }

    /// Suffixes of files that carry this ecosystem's imports without being
    /// claimed by this extension — a `.vue` component, an `.html` page, a `.css`
    /// sheet for npm. Two consequences, decided by whether another extension
    /// claims such a file: claimed, its package-shaped specifiers are read by
    /// this ecosystem's dependency-usage judgment (a stylesheet's `@import
    /// "tailwindcss"` is npm's package); unclaimed, the judgment abstains on a
    /// manifest whose package holds it — an import of the dependency may sit
    /// where nothing can see it. Empty ⇒ only this extension's own files
    /// import, and nothing unclaimed casts doubt.
    pub fn dependency_importers(&self) -> &[SmolStr] {
        &self.dependency_importers
    }

    /// See [`DependencyBuiltins`]; `undeclared` is the consumer.
    pub fn dependency_builtins(&self) -> &DependencyBuiltins {
        &self.dependency_builtins
    }

    /// See [`CycleTolerance`]; the `cyclic` analysis is the consumer.
    pub fn import_cycles(&self) -> CycleTolerance {
        self.import_cycles
    }

    /// See [`DispatchRule`]; the engine's dispatch is the consumer, and an
    /// empty list (the default) derives nothing — markers stay evidence.
    /// See [`Nesting`]. Omitted ⇒ `PerFile` — the default-compatibility rule.
    pub fn nesting(&self) -> &Nesting {
        &self.nesting
    }

    pub fn dispatch_rules(&self) -> &[DispatchRule] {
        &self.dispatch
    }

    /// See [`Ladder`]; `internal-only` is the consumer — for the judgment and
    /// for the word its message uses. Empty (the default) means the language
    /// states no ladder and the analysis stays silent for its files.
    pub fn ladder(&self) -> &Ladder {
        &self.ladder
    }

    /// See [`NamespaceSpan`]; `Scopes` is the consumer. `Unit` (the default)
    /// keeps every namespace inside the unit that compiles it.
    pub fn namespace_span(&self) -> NamespaceSpan {
        self.namespace_span
    }

    /// What bounds a unit-wide reach where no manifest named the unit — see
    /// [`UnnamedUnit`]; `Scopes` is the consumer. `Unbounded` (the default)
    /// keeps such a declaration on the published surface.
    pub fn unnamed_unit(&self) -> UnnamedUnit {
        self.unnamed_unit
    }

    pub fn file_roles(&self) -> &[FileRole] {
        &self.file_roles
    }

    pub fn claims(&self) -> &[SmolStr] {
        &self.claims
    }

    pub fn emits(&self) -> &EvidenceStreams {
        &self.emits
    }

    pub fn manifests(&self) -> &[SmolStr] {
        &self.manifests
    }

    /// Files read for the roots they declare and nothing else — see
    /// [`ExtensionSpecBuilder::launchers`].
    pub fn launchers(&self) -> &[SmolStr] {
        &self.launchers
    }

    /// Paths this language's own tool never compiles — see
    /// [`ExtensionSpecBuilder::ignores`]. The claim pass and the manifest pass
    /// are the consumers: a file under one is discovered and never claimed by
    /// this extension, and a manifest under one is never read.
    pub fn ignores(&self) -> &[SmolStr] {
        &self.ignores
    }

    /// Whose dependencies this language's bare specifiers name — see
    /// [`ExtensionSpecBuilder::ecosystem`]. `None` (the default) means its
    /// own: a bare specifier is judged against the manifests this extension
    /// itself claims.
    pub fn ecosystem(&self) -> Option<&SmolStr> {
        self.ecosystem.as_ref()
    }

    /// Dot-named directories this language's tooling lives in — see
    /// [`ExtensionSpecBuilder::hidden_opt_in`]. Discovery is the consumer,
    /// and it already admits the dot-named segments of every declared
    /// manifest and launcher glob; this is what a language knows BESIDE
    /// those.
    pub fn hidden_opt_in(&self) -> &[SmolStr] {
        &self.hidden_opt_in
    }

    /// Whether this spec declares conduct or ingestion at all — the engine's
    /// round runs over exactly the extensions for which this is true, and only
    /// those appear as contributions in the report.
    pub fn declares_conduct(&self) -> bool {
        self.conducts
    }

    pub fn activation(&self) -> &Activation {
        &self.activation
    }

    pub fn mutates_graph(&self) -> bool {
        self.mutates_graph
    }

    pub fn dependencies(&self) -> &[SmolStr] {
        &self.dependencies
    }

    pub fn requested_file_access(&self) -> &[SmolStr] {
        &self.requested_file_access
    }

    pub fn rules(&self) -> &[RuleDescriptor] {
        &self.rules
    }

    /// Root-relative report paths the engine reads through its well-known channel
    /// (run output is gitignored — discovery never sees it) and pushes to
    /// [`Extension::ingest`], in declaration order.
    pub fn reads_reports(&self) -> &[SmolStr] {
        &self.reads_reports
    }
}

/// The owned-parts constructor for the wire boundary: a LOADED component's spec
/// arrives as data, not statics, so the builder's `&'static str` economy cannot
/// apply — and a parts struct rather than a parameter list, so no two same-typed
/// fields can swap silently. `conducts` is the loader's statement of which side
/// of the door the component's world sits on until the worlds unify.
#[derive(Debug, Default)]
pub struct ExtensionSpecParts {
    pub coordinate: SmolStr,
    pub version: u32,
    pub suffixes: Vec<SmolStr>,
    /// `Exports` (silence for the Exported rung) unless the component says
    /// its ecosystem publishes through entries.
    pub published_surface: PublishedSurface,
    pub dependency_scoping: DependencyScoping,
    pub dependency_identity: DependencyIdentity,
    pub dependency_importers: Vec<SmolStr>,
    pub dependency_builtins: DependencyBuiltins,
    pub import_cycles: CycleTolerance,
    /// Empty unless the component states its ladder, under which
    /// `internal-only` stays silent — the same absence every other undeclared
    /// capability degrades to.
    pub ladder: Ladder,
    pub namespace_span: NamespaceSpan,
    pub unnamed_unit: UnnamedUnit,
    pub nesting: Nesting,
    pub file_roles: Vec<FileRole>,
    pub dispatch: Vec<DispatchRule>,
    pub claims: Vec<SmolStr>,
    pub emits: EvidenceStreams,
    pub manifests: Vec<SmolStr>,
    pub launchers: Vec<SmolStr>,
    pub ignores: Vec<SmolStr>,
    pub ecosystem: Option<SmolStr>,
    pub hidden_opt_in: Vec<SmolStr>,
    pub conducts: bool,
    pub activation: Activation,
    pub mutates_graph: bool,
    pub dependencies: Vec<SmolStr>,
    pub requested_file_access: Vec<SmolStr>,
    pub rules: Vec<RuleDescriptor>,
    pub reads_reports: Vec<SmolStr>,
}

impl Default for Activation {
    /// The inert neutral (see [`ExtensionSpec::builder`]); a conducting spec
    /// assembled from wire parts carries the activation its component declared.
    fn default() -> Self {
        Activation::Always
    }
}

impl Default for EvidenceStreams {
    fn default() -> Self {
        EvidenceStreams::none()
    }
}

impl From<ExtensionSpecParts> for ExtensionSpec {
    fn from(parts: ExtensionSpecParts) -> ExtensionSpec {
        ExtensionSpec {
            coordinate: parts.coordinate,
            version: parts.version,
            suffixes: parts.suffixes,
            published_surface: parts.published_surface,
            dependency_scoping: parts.dependency_scoping,
            dependency_identity: parts.dependency_identity,
            dependency_importers: parts.dependency_importers,
            dependency_builtins: parts.dependency_builtins,
            import_cycles: parts.import_cycles,
            ladder: parts.ladder,
            namespace_span: parts.namespace_span,
            unnamed_unit: parts.unnamed_unit,
            nesting: parts.nesting,
            file_roles: parts.file_roles,
            dispatch: parts.dispatch,
            claims: parts.claims,
            emits: parts.emits,
            manifests: parts.manifests,
            launchers: parts.launchers,
            ecosystem: parts.ecosystem,
            hidden_opt_in: parts.hidden_opt_in,
            ignores: parts.ignores,
            conducts: parts.conducts,
            activation: parts.activation,
            mutates_graph: parts.mutates_graph,
            dependencies: parts.dependencies,
            requested_file_access: parts.requested_file_access,
            rules: parts.rules,
            reads_reports: parts.reads_reports,
        }
    }
}

/// Declaring suffixes IS claiming them: each declared suffix derives its
/// `**/*.<ext>` claim glob, in declaration order. The one spelling of the rule,
/// called by every spec builder that speaks extensions.
pub(crate) fn declare_suffixes(
    suffixes: &mut Vec<SmolStr>,
    claims: &mut Vec<SmolStr>,
    declared: &[&'static str],
) {
    for ext in declared {
        suffixes.push(SmolStr::new_static(ext));
        claims.push(SmolStr::from(format!("**/*.{ext}")));
    }
}

/// Identity + extraction. See [`ExtensionSpec::builder`].
pub struct ExtensionSpecBuilder {
    spec: ExtensionSpec,
}

impl ExtensionSpecBuilder {
    /// Declare the file suffixes this extension speaks (no leading dot), in
    /// resolution-candidate priority order — see [`declare_suffixes`]; `claims`
    /// stays for patterns that are not extension-shaped.
    pub fn suffixes(mut self, suffixes: &[&'static str]) -> Self {
        declare_suffixes(&mut self.spec.suffixes, &mut self.spec.claims, suffixes);
        self
    }

    /// Declare what a unit of this ecosystem publishes (see
    /// [`PublishedSurface`]). Omitted ⇒ `Exports` — every exported declaration
    /// is published, and `internal-only` never advises narrowing one.
    pub fn published_surface(mut self, surface: PublishedSurface) -> Self {
        self.spec.published_surface = surface;
        self
    }

    /// Declare how this ecosystem's manifests scope dependencies (see
    /// [`DependencyScoping`]). Omitted ⇒ `Scoped`: unscoped declarations are
    /// never read as usage claims.
    pub fn dependency_scoping(mut self, scoping: DependencyScoping) -> Self {
        self.spec.dependency_scoping = scoping;
        self
    }

    /// Declare how this ecosystem's import specifiers name a declared
    /// dependency (see [`DependencyIdentity`]). Omitted ⇒ `Underivable`: the
    /// dependency-usage judgment abstains for every manifest this adapter reads.
    pub fn dependency_identity(mut self, identity: DependencyIdentity) -> Self {
        self.spec.dependency_identity = identity;
        self
    }

    /// Declare the suffixes of files outside this extension's claims that carry
    /// its ecosystem's imports (see [`ExtensionSpec::dependency_importers`]).
    /// Omitted ⇒ none: only the files this extension claims import, and an
    /// unclaimed file never makes the dependency-usage judgment abstain.
    pub fn dependency_importers(mut self, suffixes: &'static [&'static str]) -> Self {
        self.spec.dependency_importers = suffixes.iter().map(|s| SmolStr::new_static(s)).collect();
        self
    }

    /// Declare which specifiers name the platform's own modules (see
    /// [`DependencyBuiltins`]). Omitted ⇒ none: every package-shaped specifier
    /// is a dependency.
    pub fn dependency_builtins(mut self, builtins: DependencyBuiltins) -> Self {
        self.spec.dependency_builtins = builtins;
        self
    }

    /// Declare the language's cycle tolerance (see [`CycleTolerance`]).
    /// Omitted ⇒ `Tolerated` — the default-compatibility rule: `cyclic` stays
    /// silent for this adapter's files.
    pub fn import_cycles(mut self, tolerance: CycleTolerance) -> Self {
        self.spec.import_cycles = tolerance;
        self
    }

    /// Declare the reaches this language can spell (see [`Ladder`]), narrowest
    /// first, each under the word this language uses for it. Omitted ⇒ none:
    /// `internal-only` never advises for its files.
    pub fn ladder(mut self, steps: &[Step]) -> Self {
        self.spec.ladder = Ladder::new(steps.to_vec());
        self
    }

    /// Declare how far a namespace reaches across units (see
    /// [`NamespaceSpan`]). Omitted ⇒ `Unit`: a namespace stops at the unit
    /// that compiles it, which keeps every advisory a wider span would silence.
    /// What this language's own tooling makes of a file by its path — see
    /// [`FileRole`]. The consumer is root anchoring, which reads them only for
    /// a file whose unit declared no role of its own.
    pub fn file_roles(mut self, roles: &[FileRole]) -> Self {
        self.spec.file_roles = roles.to_vec();
        self
    }

    pub fn namespace_span(mut self, span: NamespaceSpan) -> Self {
        self.spec.namespace_span = span;
        self
    }

    /// Declare what bounds a unit-wide reach where no manifest named the unit
    /// (see [`UnnamedUnit`]). Omitted ⇒ `Unbounded`: such a declaration is
    /// judged as published surface, which is keep-alive for every language
    /// whose namespaces are smaller than its units.
    pub fn unnamed_unit(mut self, unit: UnnamedUnit) -> Self {
        self.spec.unnamed_unit = unit;
        self
    }

    /// Declare what the language's markers mean (see [`DispatchRule`]), in
    /// the order the engine tries them. Omitted ⇒ none — the
    /// default-compatibility rule: markers are carried as evidence and derive
    /// nothing. Appends, so rules a shared builder already declared stand
    /// alongside the language's own.
    /// Declare how this language shapes its namespace nodes (see [`Nesting`]).
    /// Omitted ⇒ `PerFile`: the file is its own namespace, which is what an
    /// adapter that emits its own clause wants left alone.
    pub fn nesting(mut self, nesting: Nesting) -> Self {
        self.spec.nesting = nesting;
        self
    }

    pub fn dispatch(mut self, rules: Vec<DispatchRule>) -> Self {
        self.spec.dispatch.extend(rules);
        self
    }

    pub fn claims(mut self, globs: &[&'static str]) -> Self {
        self.spec
            .claims
            .extend(globs.iter().map(|g| SmolStr::new_static(g)));
        self
    }

    /// Omitted ⇒ `EvidenceStreams::none()` — the default-compatibility rule.
    /// Unions, so a stream a shared builder already declared survives an
    /// adapter that lists only the ones its own grammar adds.
    pub fn emits(mut self, streams: EvidenceStreams) -> Self {
        self.spec.emits.declare_all(&streams);
        self
    }

    /// Omitted ⇒ no manifests consulted and [`Extension::roots`] never called —
    /// the default-compatibility rule.
    pub fn manifests(mut self, globs: &[&'static str]) -> Self {
        self.spec.manifests = globs.iter().map(|g| SmolStr::new_static(g)).collect();
        self
    }

    /// Files that RUN the project's files without being package manifests — a
    /// CI workflow, an action definition, a task runner's file. Each is handed
    /// to [`Extension::roots`] like a manifest, and to nothing else: a launcher
    /// declares no package, no dependencies and no mentions, and owns no files,
    /// so a step's directory never becomes a package whose imports are judged
    /// against empty declarations. A glob naming a dot-directory (`.github`)
    /// is also what lets discovery enter it. Omitted ⇒ none.
    pub fn launchers(mut self, globs: &[&'static str]) -> Self {
        self.spec.launchers = globs.iter().map(|g| SmolStr::new_static(g)).collect();
        self
    }

    /// Declare the paths this language's own tool never compiles, as globs —
    /// Go's `_`-prefixed files and its `vendor` copies, npm's `node_modules`,
    /// the interpreter's `site-packages`. A file under one is
    /// discovered — an import pointing at it is not broken — but never claimed
    /// by this extension, and a manifest under one declares nothing: no
    /// evidence, no unit, no judgment. The rule is the tool's, never a guess
    /// about output directories a package might be named after. Omitted ⇒ none.
    pub fn ignores(mut self, globs: &[&'static str]) -> Self {
        self.spec.ignores = globs.iter().map(|g| SmolStr::new_static(g)).collect();
        self
    }

    /// Declare that this language's BARE specifiers name another ecosystem's
    /// dependencies. A `.css` sheet's `@import "tailwindcss"` and a page's
    /// `<script src="lodash">` name npm packages, not packages of some
    /// stylesheet or markup registry — so css and html point at `kndo:js-ts`
    /// and the dependency judgment reads their specifiers as that ecosystem's
    /// manifests' users. Omitted ⇒ its own: a bare specifier is judged against
    /// the manifests this extension itself claims.
    ///
    /// This is the other half of `dependency_importers`, from the other side:
    /// that one names SUFFIXES a manifest's ecosystem may be imported from and
    /// casts doubt when nothing claims them; this one is the claiming
    /// extension saying out loud which ecosystem it is speaking.
    pub fn ecosystem(mut self, coordinate: &'static str) -> Self {
        self.spec.ecosystem = Some(SmolStr::new_static(coordinate));
        self
    }

    /// Declare dot-named directories discovery must ENTER for this language —
    /// js-ts's `.storybook` and `.vitepress`, which hold real source and are
    /// hidden only by convention. Discovery already admits the dot-named
    /// segments of every declared manifest and launcher glob (`.github`), so
    /// this is for the directories no glob names. Omitted ⇒ none beyond those.
    pub fn hidden_opt_in(mut self, names: &[&'static str]) -> Self {
        self.spec.hidden_opt_in = names.iter().map(|n| SmolStr::new_static(n)).collect();
        self
    }

    /// The key to stage two. Declaring any conduct or ingestion — rules,
    /// dependencies, content access, report paths — requires deciding its two
    /// gates first, as arguments, not as defaults: a framework extension that
    /// fell into `Always` would fire roots on every project on the planet, and a
    /// forgotten `MutatesGraph` answer either breaks incremental analysis for
    /// everyone or corrupts it. The dependency-only posture is written by hand:
    /// `Activation::AnyRule(vec![])`.
    pub fn conduct(
        mut self,
        activation: Activation,
        mutates_graph: MutatesGraph,
    ) -> ConductBuilder {
        self.spec.conducts = true;
        self.spec.activation = activation;
        self.spec.mutates_graph = mutates_graph.as_bool();
        ConductBuilder { spec: self.spec }
    }

    pub fn build(self) -> ExtensionSpec {
        self.spec
    }
}

/// Stage two: conduct and ingestion, reachable only through
/// [`ExtensionSpecBuilder::conduct`].
pub struct ConductBuilder {
    spec: ExtensionSpec,
}

impl ConductBuilder {
    pub fn rule(mut self, name: &'static str, description: &'static str) -> Self {
        assert!(
            !name.contains('/'),
            "rule names must not contain '/': coordinates legally do, so a slash \
             here would let two (coordinate, rule) pairs spell one category"
        );
        self.spec.rules.push(RuleDescriptor {
            name: SmolStr::new_static(name),
            description: SmolStr::new_static(description),
        });
        self
    }

    /// Coordinates of extensions this one needs running beside it. An active
    /// extension activates its dependencies' conduct and ingestion, transitively
    /// — the only path for a component whose framework is an INDIRECT dependency.
    pub fn dependencies(mut self, coordinates: &[&'static str]) -> Self {
        self.spec.dependencies = coordinates.iter().map(|c| SmolStr::new_static(c)).collect();
        self
    }

    /// Globs over discovered files this extension may read through
    /// [`ContentView`]; a path outside them reads as absent.
    pub fn requested_file_access(mut self, globs: &[&'static str]) -> Self {
        self.spec.requested_file_access = globs.iter().map(|g| SmolStr::new_static(g)).collect();
        self
    }

    /// Root-relative report paths for [`Extension::ingest`], tried in order.
    pub fn reads_reports(mut self, paths: &[&'static str]) -> Self {
        self.spec.reads_reports = paths.iter().map(|p| SmolStr::new_static(p)).collect();
        self
    }

    pub fn build(self) -> ExtensionSpec {
        self.spec
    }
}

/// An extension's own severity vocabulary — deliberately not [`Severity`]: the
/// engine maps it into the advisory channel, so an extension can never construct
/// a gate-eligible finding directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConductSeverity {
    Error,
    Warning,
    Info,
}

impl ConductSeverity {
    /// The advisory mapping the engine applies; findings so mapped ride the
    /// namespaced categories the gate never counts.
    pub fn advisory(self) -> Severity {
        match self {
            ConductSeverity::Error => Severity::Error,
            ConductSeverity::Warning => Severity::Warning,
            ConductSeverity::Info => Severity::Info,
        }
    }
}

/// What a conduct contribution points at. A target that resolves to nothing is a
/// silent no-op in the graph and a described line in the contribution — the
/// author debugging "contributed 0 roots" needs the why; the run never crashes
/// on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConductTarget {
    File(ProjectPath),
    Symbol { path: ProjectPath, name: SmolStr },
}

/// The graph as conduct hooks may see it: paths, membership and the declared
/// names — no internals; the surface the WASM boundary carries. The engine
/// implements it; extensions only consume it.
pub trait GraphAccess {
    /// Every path in the assembled graph, in path order.
    fn paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_>;

    /// Membership without the full list.
    fn contains(&self, path: &ProjectPath) -> bool;

    /// Every declaration in the graph — file by file in path order, in
    /// declaration order within a file. A name an extension reads outside the
    /// code (a storyboard's class, a plist's principal class) becomes a
    /// [`ConductTarget::Symbol`] only through here: the graph alone knows
    /// where, and whether, the name is declared.
    fn declarations(&self) -> Box<dyn Iterator<Item = DeclaredSymbol<'_>> + '_>;
}

/// One declaration as conduct sees it: where it is, what it is called, what it
/// is, and the declaration it is a member of — by name, so an extension that
/// read `Owner.member` from an artifact matches it without a second lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclaredSymbol<'a> {
    pub path: &'a ProjectPath,
    pub name: &'a str,
    pub kind: &'a SymbolKind,
    pub owner: Option<&'a str>,
}

impl<'a> DeclaredSymbol<'a> {
    /// One file's declarations with owners named rather than numbered — how a
    /// graph view answers [`GraphAccess::declarations`].
    pub fn of_file(
        path: &'a ProjectPath,
        declarations: &'a [Declaration],
    ) -> impl Iterator<Item = DeclaredSymbol<'a>> + 'a {
        declarations.iter().map(move |d| DeclaredSymbol {
            path,
            name: d.name.as_str(),
            kind: &d.kind,
            owner: d.owner.map(|o| declarations[o.index()].name.as_str()),
        })
    }
}

/// The write side of one extension's conduct round.
/// One liveness anchor a conduct round contributed — the wire record's native
/// twin, so consumers name fields instead of destructuring positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributedRoot {
    pub target: ConductTarget,
    pub kind: RootKind,
    pub confidence: Confidence,
}

/// One advisory finding a conduct round contributed. `confidence` is the
/// extension's own claim — a fact parsed from a lockfile is `Certain`, a
/// heuristic is `Possible`; the engine carries it into the finding verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributedFinding {
    pub rule: SmolStr,
    pub severity: ConductSeverity,
    pub target: ConductTarget,
    pub confidence: Confidence,
    pub message: String,
}

#[derive(Default)]
pub struct ConductSink {
    roots: Vec<ContributedRoot>,
    findings: Vec<ContributedFinding>,
    notes: Vec<String>,
}

impl ConductSink {
    /// Anchor liveness the language cannot see: a route file, a DI-registered
    /// symbol. Applied to the graph only from extensions whose spec declares
    /// `mutates_graph` — the declaration is self-enforcing, because the hook
    /// that fills this is only invoked on those; anything smuggled through the
    /// shared sink drops with a described line.
    pub fn root(&mut self, target: ConductTarget, kind: RootKind, confidence: Confidence) {
        self.roots.push(ContributedRoot {
            target,
            kind,
            confidence,
        });
    }

    /// An advisory finding under one of the spec's declared rules; undeclared
    /// rules drop with a described line on the contribution.
    pub fn finding(
        &mut self,
        rule: &str,
        severity: ConductSeverity,
        target: ConductTarget,
        confidence: Confidence,
        message: impl Into<String>,
    ) {
        self.findings.push(ContributedFinding {
            rule: SmolStr::new(rule),
            severity,
            target,
            confidence,
            message: message.into(),
        });
    }

    /// A bridge-level honesty line: something went wrong OUTSIDE the guest's
    /// declared surface (a trap mid-conduct, a violated phase gate) and the
    /// contribution must say so — a vanished call and a clean empty round must
    /// never look alike. Lands in the contribution's dropped list.
    pub fn note(&mut self, line: impl Into<String>) {
        self.notes.push(line.into());
    }

    /// Everything the round wrote, for the engine to judge: roots, findings,
    /// then bridge notes, each in emission order.
    pub fn into_parts(self) -> (Vec<ContributedRoot>, Vec<ContributedFinding>, Vec<String>) {
        (self.roots, self.findings, self.notes)
    }
}

/// Scoped content reads as conduct hooks see them — the same surface natively
/// (backed by [`ContentView`]) and across the WASM boundary (backed by the
/// host's prefetched snapshot), so an extension's conduct code is written once
/// against one shape. Every miss is `None`; the budget lives behind the
/// implementation, and its cut is reported on the contribution.
pub trait ContentAccess {
    /// The discovered paths the scope admits, in path order — names only,
    /// nothing charged.
    fn readable_paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_>;

    fn read(&self, path: &ProjectPath) -> Option<&[u8]>;
}

pub const CONTENT_MAX_FILES: usize = 200;
pub const CONTENT_MAX_BYTES: usize = 8 * 1024 * 1024;

/// Budgeted, glob-scoped reads over the run's already-read file contents — no
/// second disk walk. Every miss (no glob match, unknown path, budget cut) is the
/// same `None`; the cut itself is reported on the contribution, never silent.
pub struct ContentView<'a> {
    contents: &'a BTreeMap<ProjectPath, &'a [u8]>,
    globs: Vec<globset::GlobMatcher>,
    budget: RefCell<ContentBudget>,
}

#[derive(Default)]
struct ContentBudget {
    files: usize,
    bytes: usize,
    seen: BTreeSet<ProjectPath>,
    cut_off: bool,
}

impl<'a> ContentView<'a> {
    /// A view scoped to `access` (an extension spec's `requested_file_access`).
    /// A malformed glob is simply never satisfied — declared input degrades, the
    /// run never aborts on it.
    pub fn new(contents: &'a BTreeMap<ProjectPath, &'a [u8]>, access: &[SmolStr]) -> Self {
        let globs = access
            .iter()
            .filter_map(|g| globset::Glob::new(g).ok())
            .map(|g| g.compile_matcher())
            .collect();
        ContentView {
            contents,
            globs,
            budget: RefCell::new(ContentBudget::default()),
        }
    }

    /// The discovered paths this view's globs admit, in path order — names only,
    /// nothing charged. What a prefetching consumer (the WASM bridge's
    /// before-instantiation snapshot) walks so it never re-implements the glob
    /// scope; reading each is still [`ContentView::read`], budget and all.
    pub fn readable_paths(&self) -> impl Iterator<Item = &'a ProjectPath> + '_ {
        self.contents
            .keys()
            .filter(|p| self.globs.iter().any(|g| g.is_match(p.as_str())))
    }

    pub fn read(&self, path: &ProjectPath) -> Option<&'a [u8]> {
        if !self.globs.iter().any(|g| g.is_match(path.as_str())) {
            return None;
        }
        let bytes = *self.contents.get(path)?;
        let mut budget = self.budget.borrow_mut();
        if budget.seen.contains(path) {
            // A charged path re-reads for free, cutoff or not: the budget is
            // about new reads, not about punishing a second look.
            return Some(bytes);
        }
        if budget.cut_off {
            return None;
        }
        budget.files += 1;
        budget.bytes += bytes.len();
        budget.seen.insert(path.clone());
        if budget.files > CONTENT_MAX_FILES || budget.bytes > CONTENT_MAX_BYTES {
            budget.cut_off = true;
            return None;
        }
        Some(bytes)
    }

    /// Whether the budget cut a read this round — reported on the contribution.
    pub fn budget_cut(&self) -> bool {
        self.budget.borrow().cut_off
    }
}

impl ContentAccess for ContentView<'_> {
    fn readable_paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_> {
        Box::new(ContentView::readable_paths(self))
    }

    fn read(&self, path: &ProjectPath) -> Option<&[u8]> {
        ContentView::read(self, path)
    }
}

/// The one door. Every hook has an abstaining default; [`Extension::spec`] is the
/// only obligation. The engine invokes a hook only when the spec declares its
/// capability: extraction hooks for claimed files, manifest hooks for declared
/// manifest globs, conduct hooks under activation (+ `mutates_graph` for roots),
/// ingestion under activation for declared report paths.
pub trait Extension: Send + Sync {
    fn spec(&self) -> &ExtensionSpec;

    // ---- extraction: per file, pure in (bytes, spec version); gated by claims ----

    /// Extract one claimed file's evidence — or one embedded region of a file
    /// another extension claimed, when `file.region` says so: the engine hands
    /// a reported region to the extension claiming its language's suffix, and
    /// what it reads lands in the host's evidence at the host's offsets. Never
    /// fails: extraction degrades through the sink's diagnostics.
    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        let _ = (file, out);
    }

    /// Resolve an import specifier written in `from` against the project.
    /// `Unresolved` is the keep-alive default for anything the extension cannot
    /// place.
    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        let _ = (from, specifier, cx);
        Resolution::Unresolved
    }

    /// Everything one manifest SAYS about the project: its units with their
    /// source roots, excludes and entries; the packages it declares; its
    /// dependencies and the names it mentions; the files it runs. The mirror
    /// of [`Extension::extract`] — transcription of a project fact, which is
    /// why claims gate it and activation does not, called for every discovered
    /// file matching the spec's manifest and launcher globs. Unparseable or
    /// dangling entries degrade to absence: structure nobody stated is
    /// structure the engine does not assume.
    ///
    /// The ONE door: units, packages, dependencies, mentions, roots and
    /// members all arrive through [`ManifestSink`], native and WASM alike.
    fn extract_manifest(
        &self,
        manifest: &SourceFile<'_>,
        cx: &ResolveContext<'_>,
        out: &mut ManifestSink,
    ) {
        let _ = (manifest, cx, out);
    }

    // ---- conduct: post-graph, once; gated by activation ----

    /// Liveness the language cannot know. Invoked only when the spec declares
    /// `mutates_graph`.
    fn contribute_roots(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        let _ = (graph, content, out);
    }

    /// Advisory findings under the spec's declared rules.
    fn report_findings(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        let _ = (graph, content, out);
    }

    // ---- ingestion: pure; gated by activation + reads_reports ----

    /// Parse one report the engine located through the spec's `reads_reports`
    /// paths and read for you: what the report STATES, never a line table —
    /// mapping records onto the project is the engine's, uniformly for every
    /// ingester. `None` when the bytes are not this extension's format.
    fn ingest(&self, report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        let _ = (report_path, content);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_stage_builder_produces_the_declared_spec() {
        let extraction_only = ExtensionSpec::builder("kndo:kmini", 3)
            .suffixes(&["kmini"])
            .claims(&["**/legacy.km"])
            .manifests(&["kmini.toml"])
            .launchers(&["**/.ci/*.yml"])
            .build();
        assert_eq!(extraction_only.coordinate(), "kndo:kmini");
        assert_eq!(extraction_only.version(), 3);
        assert_eq!(extraction_only.claims(), ["**/*.kmini", "**/legacy.km"]);
        assert_eq!(extraction_only.manifests(), ["kmini.toml"]);
        assert_eq!(extraction_only.launchers(), ["**/.ci/*.yml"]);
        // The inert neutrals: nothing declared for them to gate.
        assert_eq!(extraction_only.activation(), &Activation::Always);
        assert!(!extraction_only.mutates_graph());
        assert!(extraction_only.rules().is_empty());
        assert!(extraction_only.reads_reports().is_empty());

        let conduct = ExtensionSpec::builder("acme:framework", 1)
            .conduct(
                Activation::AnyRule(vec![ActivationRule::ManifestDependency(
                    SmolStr::new_static("acme-framework"),
                )]),
                MutatesGraph::Yes,
            )
            .rule("routes", "route files the framework wires")
            .dependencies(&["kndo:express"])
            .requested_file_access(&["routes/**"])
            .build();
        assert!(conduct.mutates_graph());
        assert_eq!(conduct.dependencies(), ["kndo:express"]);
        assert_eq!(conduct.rules().len(), 1);

        let ingester = ExtensionSpec::builder("kndo:coverage-lcov", 1)
            .conduct(Activation::Always, MutatesGraph::No)
            .reads_reports(&["coverage/lcov.info", "lcov.info"])
            .build();
        assert!(!ingester.mutates_graph());
        assert_eq!(
            ingester.reads_reports(),
            ["coverage/lcov.info", "lcov.info"]
        );
    }

    #[test]
    fn trigger_patterns_are_literal_but_for_the_star() {
        use crate::evidence::MarkerTarget;
        use crate::vocab::Span;
        let marker = |path: &str, args: &[&str]| Marker {
            on: MarkerTarget::File,
            path: SmolStr::new(path),
            args: args.iter().map(SmolStr::new).collect(),
            span: Span::new(0, 1),
        };
        // A file that binds nothing: every path here is compared as written.
        let evidence = EvidenceSink::new(1000, EvidenceStreams::none()).finish();
        let supertypes = BTreeMap::new();
        let cx = DeclarationCx {
            evidence: &evidence,
            compiled_into: None,
            supertypes: &supertypes,
        };
        assert!(Trigger::marker("test").matches(&cx, &marker("test", &[])));
        assert!(!Trigger::marker("test").matches(&cx, &marker("tokio::test", &[])));
        assert!(Trigger::marker("*::test").matches(&cx, &marker("tokio::test", &[])));
        assert!(Trigger::marker("*::test").matches(&cx, &marker("a::b::test", &[])));
        assert!(!Trigger::marker("*::test").matches(&cx, &marker("test", &[])));
        assert!(!Trigger::marker("*::test").matches(&cx, &marker("tokio::tests", &[])));
        assert!(Trigger::marker("*").matches(&cx, &marker("anything", &[])));
        let cfg_test = Trigger::marker_with("cfg", "test");
        assert!(cfg_test.matches(&cx, &marker("cfg", &["test"])));
        assert!(cfg_test.matches(&cx, &marker("cfg", &["unix", "test"])));
        assert!(!cfg_test.matches(&cx, &marker("cfg", &["!test"])));
        assert!(!cfg_test.matches(&cx, &marker("cfg", &["feature = \"test\""])));
        assert!(!cfg_test.matches(&cx, &marker("cfg", &[])));
        // Qualified through the file's own bindings: a rule written with the
        // full name reaches the imported type and not the same simple name
        // from another package.
        let mut sink = EvidenceSink::new(1000, EvidenceStreams::none());
        sink.import(
            crate::evidence::ImportTarget::Package(SmolStr::new_static("com.vendor.Closer")),
            crate::evidence::ImportShape::Bindings(vec![crate::evidence::ImportBinding {
                imported: SmolStr::new_static("Closer"),
                local: SmolStr::new_static("Closer"),
            }]),
            Span::new(0, 1),
            crate::vocab::Confidence::Certain,
        );
        let evidence = sink.finish();
        let cx = DeclarationCx {
            evidence: &evidence,
            compiled_into: None,
            supertypes: &supertypes,
        };
        assert!(cx.spells("com.vendor.Closer", "Closer"));
        assert!(!cx.spells("com.other.Closer", "Closer"));
        assert!(
            cx.spells("Closer", "Closer"),
            "the written name still reaches"
        );
        assert!(
            cx.spells("com.vendor.Closer.Inner", "Closer.Inner"),
            "the path INSIDE the bound name is carried"
        );
        assert!(
            !cx.spells("com.vendor.Closer", "Opener"),
            "a name the file binds nothing for is compared as written"
        );

        assert!(pattern_matches("a*c", "abbbc"));
        assert!(pattern_matches("a*", "a"));
        assert!(pattern_matches("**", ""));
        assert!(!pattern_matches("a*c", "ab"));
    }

    #[test]
    fn a_loader_query_is_not_part_of_a_package_name() {
        let npm = DependencyIdentity::PackageName;
        assert_eq!(
            npm.package_of("normalize.css?inline"),
            Some("normalize.css")
        );
        assert_eq!(npm.package_of("@scope/pkg/sub?raw"), Some("@scope/pkg"));
        assert_eq!(npm.package_of("pkg#section"), Some("pkg"));
        assert_eq!(
            npm.package_of("#internal/x"),
            None,
            "a subpath import, as before"
        );
        assert_eq!(npm.package_of("?raw"), None);
        assert!(npm.names("normalize.css?inline", "normalize.css"));
    }

    #[test]
    fn every_hook_defaults_to_abstention() {
        struct Bare(ExtensionSpec);
        impl Extension for Bare {
            fn spec(&self) -> &ExtensionSpec {
                &self.0
            }
        }
        struct NoGraph;
        impl GraphAccess for NoGraph {
            fn paths(&self) -> Box<dyn Iterator<Item = &ProjectPath> + '_> {
                Box::new(std::iter::empty())
            }
            fn contains(&self, _: &ProjectPath) -> bool {
                false
            }
            fn declarations(&self) -> Box<dyn Iterator<Item = DeclaredSymbol<'_>> + '_> {
                Box::new(std::iter::empty())
            }
        }

        let bare = Bare(ExtensionSpec::builder("demo:bare", 1).build());
        let cx_files = BTreeSet::new();
        let cx = ResolveContext::new(&cx_files);
        let from = ProjectPath::new("a.js");
        assert_eq!(bare.resolve(&from, "./b", &cx), Resolution::Unresolved);
        let mut manifest = ManifestSink::new();
        bare.extract_manifest(
            &SourceFile {
                path: &from,
                content: b"{}",
                region: None,
            },
            &cx,
            &mut manifest,
        );
        let manifest = manifest.finish();
        assert!(manifest.units.is_empty() && manifest.packages.is_empty());
        assert_eq!(bare.ingest("coverage/lcov.info", b"TN:"), None);

        let contents = BTreeMap::new();
        let view = ContentView::new(&contents, bare.spec().requested_file_access());
        let mut sink = ConductSink::default();
        bare.contribute_roots(&NoGraph, &view, &mut sink);
        bare.report_findings(&NoGraph, &view, &mut sink);
        let (roots, findings, _) = sink.into_parts();
        assert!(roots.is_empty() && findings.is_empty() && !view.budget_cut());
    }

    #[test]
    fn content_view_scopes_reads_to_the_declared_globs() {
        let a = ProjectPath::new("routes/web.rb");
        let b = ProjectPath::new("secrets.env");
        let mut contents: BTreeMap<ProjectPath, &[u8]> = BTreeMap::new();
        contents.insert(a.clone(), b"root 'x'");
        contents.insert(b.clone(), b"KEY=1");
        let access = [SmolStr::new_static("routes/**")];
        let view = ContentView::new(&contents, &access);
        assert_eq!(view.read(&a), Some(b"root 'x'".as_slice()));
        assert_eq!(view.read(&b), None);
        assert_eq!(view.readable_paths().collect::<Vec<_>>(), [&a]);
    }
}
