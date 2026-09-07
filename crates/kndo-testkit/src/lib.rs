//! Test machinery: the mock language (`.kmock`) every engine gate runs against, and a
//! temp-project builder. Contract-only by design — the same crate a third-party
//! adapter author can use, with no path to the engine's internals.
//!
//! The kmock DSL, one construct per line:
//!
//! ```text
//! fn name              file-reaching function declaration
//! fn name(int, T)      the same, with the signature that tells two `name`s apart
//! pub fn name          exported function declaration
//! type Name            file-reaching type declaration
//! pub type Name        exported type declaration
//! member Owner.name    owner-reaching member of `Owner` (`Owner.name(int)` with a
//!                      signature; a later line naming a bare `name` means the last
//!                      one declared)
//! pub member Owner.name   exported member of `Owner`
//! REACH fn name        any declaration under a reach word: `owner`, `file`, `ns`
//!                      (its namespace), `unit`, `group` (the unit's aggregate),
//!                      `ns(N)` (N namespaces up), `dir(N)` (N directories up),
//!                      `named(a.b)` (a namespace by
//!                      name), `heirs` (its owner and the owner's subtypes),
//!                      `heirs+ns` (those and its namespace), `inherited` (its
//!                      owner's), `pub`
//! package a.b          the namespace this file declares itself into
//! test-only            this file belongs to the namespace it declared in test
//!                      builds alone — the file's own statement, where the
//!                      language has no separate test compilation
//! include ./a          `./a`'s content becomes part of this file: its names are
//!                      readable here whatever reach they carry
//! mount a ./a          `./a` becomes the child namespace `a` of this file's,
//!                      fenced by the mount's own reach (`pub mount a ./a`,
//!                      `unit mount a ./a`; unstated, this file's namespace)
//! extends Sub Base     `Sub` extends the type named `Base`
//! implements Impl Face `Impl` implements the type named `Face`
//! call name            a Call reference to a bare `name`
//! call x.name          a Call reference to `name` read from `x`
//! import ./x           side-effect import of x.kmock in the same directory
//! import ./x { a, b }  binding import
//! root name            production root anchored on the declaration `name`
//! root-file            whole-file production root
//! test-file            whole-file test root: this file IS a test
//! mark name path a,b   a marker `path(a, b)` on the declaration `name`
//! mark-file path a,b   a marker on the whole file
//! # text               a comment (the Comments stream, declared)
//! ```
//!
//! Markers mean nothing until a spec says so: [`MockExtension::dispatching`]
//! speaks the same language under the dispatch rules a test hands it.
//!
//! A `kmock.pkg` manifest states the project's structure, one unit per line:
//!
//! ```text
//! unit name kind roots=a,b excludes=c entries=x.kmock,y.kmock needs=other friends=other publish=no
//! member sub/kmock.pkg              a manifest this one aggregates
//! run path.kmock                    a file this manifest runs (tooling)
//! ```
//!
//! `kind` is one of library, executable, test, bench, example, tooling — the
//! color a unit's entries anchor follows from it. `needs=` names units this
//! one compiles against and `friends=` the units whose unit-reaching names it
//! may use; the engine resolves each name among the manifests this one's
//! aggregator lists, which is how two units of one name stay apart.
//! `publish=yes|no` states the unit's publication; unstated, a library is
//! published and nothing else is. The kmock ecosystem publishes through its
//! entries, like npm; [`MockExtension::with`] speaks the variant that
//! publishes every export, like a jar.
//!
//! A `.kdoc` document ([`MockExtension::hosting`]) is a page holding kmock in
//! fences — what a test of embedded regions speaks:
//!
//! ```text
//! prose the document keeps to itself
//! <<kmock module          a region of kmock, run as a module (or `script`)
//! import ./lib { f }
//! call f
//! >>
//! ```

pub mod expectations;

use kndo_contract::adapter::{Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{
    Attachment, CoverageRecords, DeclarationId, DiagnosticLevel, EvidenceSink, EvidenceStream,
    EvidenceStreams, ImportBinding, ImportShape, ImportTarget, MarkerTarget, Reach, RefKind,
    RegionMode, RelationKind, RootKind, RootTarget, SymbolKind, Timing,
};
use kndo_contract::extension::{
    ConductSink, ContentAccess, DispatchRule, Extension, ExtensionSpec, ExtensionSpecBuilder,
    GraphAccess, PublishedSurface, Step,
};
use kndo_contract::manifest::{ManifestSink, Publication, Unit, UnitKind};
use kndo_contract::vocab::{Confidence, ProjectPath, Span};
use std::collections::BTreeMap;
use std::path::Path;

type ConductHook = dyn Fn(&dyn GraphAccess, &dyn ContentAccess, &mut ConductSink) + Send + Sync;
type IngestHook = dyn Fn(&str, &[u8]) -> Option<CoverageRecords> + Send + Sync;

/// The one mock for every cluster. [`MockExtension::new`] speaks the kmock
/// language (extraction + resolution); [`MockExtension::scripted`] carries any
/// spec and runs the closures a test hangs on its conduct and ingestion hooks —
/// including behavior a correct extension never has, because drops and refusals
/// are exactly what containment tests script.
pub struct MockExtension {
    spec: ExtensionSpec,
    speaks: Speaks,
    on_contribute: Option<Box<ConductHook>>,
    on_report: Option<Box<ConductHook>>,
    on_ingest: Option<Box<IngestHook>>,
}

/// The kmock-speaking mock under its historical name.
pub type MockAdapter = MockExtension;

/// What a mock speaks: kmock (extraction, resolution, manifests), kdoc (a
/// document embedding kmock regions), or nothing (a conduct or ingestion
/// mock, whose spec and hooks are all it has).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Speaks {
    Kmock,
    Kdoc,
    Nothing,
}

/// A kdoc document: a whole-file production root, and one embedded region
/// per fence, spanning the lines between `<<LANG mode` and `>>`.
fn host_extract(file: &SourceFile<'_>, out: &mut EvidenceSink) {
    out.root(
        RootTarget::WholeFile,
        RootKind::Production,
        Confidence::Certain,
    );
    let text = String::from_utf8_lossy(file.content);
    let mut open: Option<(String, RegionMode, u32)> = None;
    let mut offset = 0u32;
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        let next = offset + raw.len() as u32;
        if let Some(rest) = line.strip_prefix("<<") {
            let mut words = rest.split_whitespace();
            let language = words.next().unwrap_or("kmock").to_string();
            let mode = if words.next() == Some("script") {
                RegionMode::Script
            } else {
                RegionMode::Module
            };
            open = Some((language, mode, next));
        } else if line == ">>"
            && let Some((language, mode, start)) = open.take()
        {
            out.region(Span::new(start, offset), language, mode);
        }
        offset = next;
    }
}

/// The kmock language's spec, before the capability a test adds to it.
fn kmock_spec() -> ExtensionSpecBuilder {
    ExtensionSpec::builder("kmock", 1)
        .suffixes(&["kmock"])
        .emits(EvidenceStreams::of(&[
            EvidenceStream::Comments,
            EvidenceStream::Markers,
            EvidenceStream::Relations,
            EvidenceStream::Qualifiers,
        ]))
        .manifests(&["**/kmock.pkg"])
        // Entries publish, like npm: an export no entry reaches is internal.
        .published_surface(PublishedSurface::Entries)
}

impl MockExtension {
    pub fn new() -> Self {
        MockExtension::speaking(kmock_spec().build())
    }

    /// The kmock language declaring import cycles a hazard — what a test of the
    /// `cyclic` analysis speaks, since the plain mock tolerates them.
    pub fn hazardous() -> Self {
        MockExtension::speaking(
            kmock_spec()
                .import_cycles(kndo_contract::extension::CycleTolerance::Hazard)
                .build(),
        )
    }

    /// The kmock language whose namespaces span the compilation — what a test
    /// of unit friendship speaks, since the plain mock keeps each namespace
    /// inside the unit that compiles it.
    pub fn spanning() -> Self {
        MockExtension::speaking(
            kmock_spec()
                .namespace_span(kndo_contract::extension::NamespaceSpan::Compilation)
                .build(),
        )
    }

    /// The kmock language under dispatch rules — what a test of the engine's
    /// dispatch speaks: `mark` lines become markers, and these rules say what
    /// they mean.
    pub fn dispatching(rules: Vec<DispatchRule>) -> Self {
        MockExtension::speaking(kmock_spec().dispatch(rules).build())
    }

    /// The kdoc language: a document holding kmock in fenced regions — what a
    /// test of embedded regions speaks. A `.kdoc` file roots itself like a
    /// page, and each `<<LANG module` (or `script`) … `>>` fence is a region
    /// of language `LANG`, read by the extension claiming that suffix.
    pub fn hosting() -> Self {
        MockExtension {
            spec: ExtensionSpec::builder("kdoc", 1)
                .suffixes(&["kdoc"])
                .build(),
            speaks: Speaks::Kdoc,
            on_contribute: None,
            on_report: None,
            on_ingest: None,
        }
    }

    /// The kmock language with a ladder — what a test of `internal-only`
    /// speaks, since the plain mock states none and gets no advice. Its unit
    /// is the whole project: every kmock file is one compilation.
    pub fn laddered(steps: &[Step]) -> Self {
        MockExtension::speaking(kmock_spec().ladder(steps).build())
    }

    /// The kmock language declaring more than the plain mock does — whatever
    /// `declare` adds to its spec: a published surface of every export, a
    /// ladder, both.
    pub fn with(declare: impl FnOnce(ExtensionSpecBuilder) -> ExtensionSpecBuilder) -> Self {
        MockExtension::speaking(declare(kmock_spec()).build())
    }

    fn speaking(spec: ExtensionSpec) -> Self {
        MockExtension {
            spec,
            speaks: Speaks::Kmock,
            on_contribute: None,
            on_report: None,
            on_ingest: None,
        }
    }

    /// A conduct/ingestion mock: no language, the given spec, and whatever the
    /// closures script.
    pub fn scripted(spec: ExtensionSpec) -> Self {
        MockExtension {
            spec,
            speaks: Speaks::Nothing,
            on_contribute: None,
            on_report: None,
            on_ingest: None,
        }
    }

    pub fn on_contribute(
        mut self,
        f: impl Fn(&dyn GraphAccess, &dyn ContentAccess, &mut ConductSink) + Send + Sync + 'static,
    ) -> Self {
        self.on_contribute = Some(Box::new(f));
        self
    }

    pub fn on_report(
        mut self,
        f: impl Fn(&dyn GraphAccess, &dyn ContentAccess, &mut ConductSink) + Send + Sync + 'static,
    ) -> Self {
        self.on_report = Some(Box::new(f));
        self
    }

    pub fn on_ingest(
        mut self,
        f: impl Fn(&str, &[u8]) -> Option<CoverageRecords> + Send + Sync + 'static,
    ) -> Self {
        self.on_ingest = Some(Box::new(f));
        self
    }
}

impl Default for MockExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl Extension for MockExtension {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn contribute_roots(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        if let Some(f) = &self.on_contribute {
            f(graph, content, out);
        }
    }

    fn report_findings(
        &self,
        graph: &dyn GraphAccess,
        content: &dyn ContentAccess,
        out: &mut ConductSink,
    ) {
        if let Some(f) = &self.on_report {
            f(graph, content, out);
        }
    }

    fn ingest(&self, report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        self.on_ingest.as_ref()?(report_path, content)
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        match self.speaks {
            Speaks::Nothing => return,
            Speaks::Kdoc => return host_extract(file, out),
            Speaks::Kmock => {}
        }
        let text = String::from_utf8_lossy(file.content);

        // Pass 1: declarations, so roots and members can name each other by id
        // regardless of line order — types first, so a member finds its owner.
        let mut decls: BTreeMap<&str, DeclarationId> = BTreeMap::new();
        for (line, span) in lines_with_spans(&text) {
            let Some((reach, kind, rest)) = declaration_line(line) else {
                continue;
            };
            if kind == SymbolKind::Method {
                continue;
            }
            let (name, signature) = split_signature(rest.trim());
            let id = out.declaration(name, kind, span, reach);
            if let Some(signature) = signature {
                out.signature(id, signature);
            }
            decls.insert(name, id);
        }
        for (line, span) in lines_with_spans(&text) {
            let Some((reach, kind, rest)) = declaration_line(line) else {
                continue;
            };
            if kind != SymbolKind::Method {
                continue;
            }
            // `member Owner.name`: the owner is a type declared above.
            let (rest, signature) = split_signature(rest.trim());
            let (owner, name) = match rest.split_once('.') {
                Some((owner, name)) => (Some(owner), name),
                None => (None, rest),
            };
            let id = out.declaration(name, kind, span, reach);
            if let Some(signature) = signature {
                out.signature(id, signature);
            }
            decls.insert(name, id);
            match owner.map(|o| decls.get(o)) {
                Some(Some(owner)) => out.member_of(id, *owner),
                Some(None) => out.diagnostic(
                    DiagnosticLevel::Warn,
                    format!("member names undeclared owner in `{line}`"),
                    Some(span),
                ),
                None => {}
            }
        }

        for (line, _) in lines_with_spans(&text) {
            if let Some(rest) = line.strip_prefix("package ") {
                out.namespace(rest.trim().split('.').map(smol_str::SmolStr::new));
            } else if line == "test-only" {
                out.attachment(Attachment::TestOnly);
            }
        }

        // Pass 2b: supertype links, by name, after every type is declared.
        for (line, span) in lines_with_spans(&text) {
            let Some((kind, rest)) = line
                .strip_prefix("extends ")
                .map(|r| (RelationKind::Extends, r))
                .or_else(|| {
                    line.strip_prefix("implements ")
                        .map(|r| (RelationKind::Implements, r))
                })
            else {
                continue;
            };
            let Some((from, to)) = rest.trim().split_once(' ') else {
                continue;
            };
            match decls.get(from) {
                Some(id) => out.relation(*id, kind, to.trim(), span),
                None => out.diagnostic(
                    DiagnosticLevel::Warn,
                    format!("relation names undeclared `{from}`"),
                    Some(span),
                ),
            }
        }

        // Pass 2: everything that may point at a declaration.
        for (line, span) in lines_with_spans(&text) {
            if let Some(name) = line.strip_prefix("call ") {
                // `call x.name` is a member access ON `x`; `call name` is the
                // bare name a local or an unqualified call would write.
                match name.trim().rsplit_once('.') {
                    Some((on, member)) => out.reference_on(
                        member,
                        RefKind::Call,
                        Some(smol_str::SmolStr::new(on)),
                        span,
                    ),
                    None => out.reference(name.trim(), RefKind::Call, span),
                }
            } else if line == "root-file" {
                out.root(
                    RootTarget::WholeFile,
                    RootKind::Production,
                    Confidence::Certain,
                );
            } else if line == "test-file" {
                out.root(RootTarget::WholeFile, RootKind::Test, Confidence::Certain);
            } else if let Some(name) = line.strip_prefix("root ") {
                match decls.get(name.trim()) {
                    Some(id) => out.root(
                        RootTarget::Declaration(*id),
                        RootKind::Production,
                        Confidence::Certain,
                    ),
                    None => out.diagnostic(
                        DiagnosticLevel::Warn,
                        format!("root names undeclared `{}`", name.trim()),
                        Some(span),
                    ),
                }
            } else if let Some(specifier) = line.strip_prefix("include ") {
                out.import(
                    ImportTarget::Relative(specifier.trim().into()),
                    ImportShape::Include,
                    span,
                    Confidence::Certain,
                );
            } else if let Some((reach, segment, specifier)) = mount_line(line) {
                out.import(
                    ImportTarget::Relative(specifier.into()),
                    ImportShape::Mount {
                        namespace: segment,
                        reach,
                    },
                    span,
                    Confidence::Certain,
                );
            } else if let Some(rest) = line.strip_prefix("mark-file ") {
                let (path, args) = marker_parts(rest);
                out.marker(MarkerTarget::File, path, args, span);
            } else if let Some(rest) = line.strip_prefix("mark ") {
                let (name, rest) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
                let (path, args) = marker_parts(rest);
                match decls.get(name) {
                    Some(id) => out.marker(MarkerTarget::Declaration(*id), path, args, span),
                    None => out.diagnostic(
                        DiagnosticLevel::Warn,
                        format!("mark names undeclared `{name}`"),
                        Some(span),
                    ),
                }
            } else if let Some((timing, rest)) = timed_import(line) {
                let (specifier, shape) = match rest.split_once('{') {
                    Some((spec, names)) => {
                        let bindings = names
                            .trim_end_matches('}')
                            .split(',')
                            .map(str::trim)
                            .filter(|n| !n.is_empty())
                            .map(|n| ImportBinding {
                                imported: n.into(),
                                local: n.into(),
                            })
                            .collect();
                        (spec.trim(), ImportShape::Bindings(bindings))
                    }
                    None => (rest.trim(), ImportShape::SideEffect),
                };
                out.import_at(
                    timing,
                    ImportTarget::Relative(specifier.into()),
                    shape,
                    span,
                    Confidence::Certain,
                );
            } else if let Some(t) = line.strip_prefix("# ") {
                let text_start = span.start + (line.len() - t.len()) as u32;
                out.comment(span, Span::new(text_start, span.end));
            }
        }
    }

    fn extract_manifest(
        &self,
        manifest: &SourceFile<'_>,
        _cx: &ResolveContext<'_>,
        out: &mut ManifestSink,
    ) {
        if self.speaks != Speaks::Kmock {
            return;
        }
        let text = String::from_utf8_lossy(manifest.content);
        for (line, _) in lines_with_spans(&text) {
            if let Some(rest) = line.strip_prefix("run ") {
                out.root(kndo_contract::adapter::ProjectRoot {
                    file: ProjectPath::new(rest.trim()),
                    kind: RootKind::Tooling,
                    confidence: Confidence::Probable,
                });
                continue;
            }
            if let Some(rest) = line.strip_prefix("member ") {
                out.member(ProjectPath::new(rest.trim()));
                continue;
            }
            let Some(rest) = line.strip_prefix("unit ") else {
                continue;
            };
            let mut words = rest.split_whitespace();
            let (Some(name), Some(kind)) = (words.next(), words.next()) else {
                out.diagnostic(
                    DiagnosticLevel::Warn,
                    format!("unit line `{line}` names no kind"),
                );
                continue;
            };
            let Some(kind) = unit_kind(kind) else {
                out.diagnostic(DiagnosticLevel::Warn, format!("unknown unit kind `{kind}`"));
                continue;
            };
            let list = |key: &str| -> Vec<&str> {
                words
                    .clone()
                    .find_map(|w| w.strip_prefix(key))
                    .map(|v| v.split(',').filter(|x| !x.is_empty()).collect())
                    .unwrap_or_default()
            };
            let publication = match words.clone().find_map(|w| w.strip_prefix("publish=")) {
                Some("yes") => Publication::Published,
                Some("no") => Publication::Unpublished,
                _ => Publication::Unstated,
            };
            out.unit(Unit {
                name: name.into(),
                kind,
                roots: list("roots=").into_iter().map(Into::into).collect(),
                excludes: list("excludes=").into_iter().map(Into::into).collect(),
                entries: list("entries=").into_iter().map(ProjectPath::new).collect(),
                depends_on: list("needs=").into_iter().map(Into::into).collect(),
                friend_of: list("friends=").into_iter().map(Into::into).collect(),
                publication,
            });
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        if self.speaks != Speaks::Kmock {
            return Resolution::Unresolved;
        }
        let Some(name) = specifier.strip_prefix("./") else {
            return Resolution::Unresolved;
        };
        let dir = match from.as_str().rsplit_once('/') {
            Some((dir, _)) => format!("{dir}/"),
            None => String::new(),
        };
        let ext = &self.spec.suffixes()[0];
        let candidate = ProjectPath::new(format!("{}.{ext}", normalized(&format!("{dir}{name}"))));
        if cx.contains(&candidate) {
            Resolution::File(candidate)
        } else {
            Resolution::Unresolved
        }
    }
}

/// A joined `/`-separated path with `.` dropped and `..` popped — what makes
/// `./../src/lib` from `tests/api.kmock` name `src/lib.kmock`.
fn normalized(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// `f(int, String)` → the name and the language's spelling of what tells it
/// from another `f`, kept verbatim with its parentheses; a bare `f` has none.
fn split_signature(rest: &str) -> (&str, Option<&str>) {
    match rest.find('(') {
        Some(at) => (&rest[..at], Some(&rest[at..])),
        None => (rest, None),
    }
}

/// `pub fn f`, `type T`, `pub member T.m` — a declaration line's reach, kind
/// and the rest of it.
fn declaration_line(line: &str) -> Option<(Reach, SymbolKind, &str)> {
    let (reach, rest) = reach_prefix(line);
    let (kind, rest) = [
        ("fn ", SymbolKind::Function),
        ("type ", SymbolKind::Type),
        ("member ", SymbolKind::Method),
    ]
    .into_iter()
    .find_map(|(word, kind)| rest.strip_prefix(word).map(|rest| (kind, rest)))?;
    // Unstated, a free declaration reaches its file and a member its owner —
    // Kotlin's `private` on either.
    let reach = reach.unwrap_or(if kind == SymbolKind::Method {
        Reach::Owner
    } else {
        Reach::File
    });
    Some((reach, kind, rest))
}

/// The reach word a declaration line opens with, if any, and the rest of the
/// line. `unit` is nameable from every kmock file, the one unit the mock
/// language compiles; `ns` from the namespace the file declares.
fn reach_prefix(line: &str) -> (Option<Reach>, &str) {
    let words: [(&str, Reach); 9] = [
        ("pub ", Reach::Exported),
        ("owner ", Reach::Owner),
        ("file ", Reach::File),
        ("ns ", Reach::Namespace { up: 0 }),
        ("unit ", Reach::Unit { up: 0 }),
        ("group ", Reach::Unit { up: 1 }),
        ("inherited ", Reach::Inherited),
        (
            "heirs+ns ",
            Reach::Heirs {
                and_namespace: true,
            },
        ),
        (
            "heirs ",
            Reach::Heirs {
                and_namespace: false,
            },
        ),
    ];
    for (word, reach) in words {
        if let Some(rest) = line.strip_prefix(word) {
            return (Some(reach), rest);
        }
    }
    if let Some(rest) = line.strip_prefix("ns(")
        && let Some((up, rest)) = rest.split_once(") ")
        && let Ok(up) = up.parse()
    {
        return (Some(Reach::Namespace { up }), rest);
    }
    if let Some(rest) = line.strip_prefix("dir(")
        && let Some((up, rest)) = rest.split_once(") ")
        && let Ok(up) = up.parse()
    {
        return (Some(Reach::Directory { up }), rest);
    }
    if let Some(rest) = line.strip_prefix("named(")
        && let Some((path, rest)) = rest.split_once(") ")
    {
        let namespace = path.split('.').map(smol_str::SmolStr::new).collect();
        return (Some(Reach::Named { namespace }), rest);
    }
    (None, line)
}

fn unit_kind(word: &str) -> Option<UnitKind> {
    Some(match word {
        "library" => UnitKind::Library,
        "executable" => UnitKind::Executable,
        "test" => UnitKind::Test,
        "bench" => UnitKind::Bench,
        "example" => UnitKind::Example,
        "tooling" => UnitKind::Tooling,
        _ => return None,
    })
}

/// `path a,b` — a marker's path and its comma-separated arguments.
fn marker_parts(rest: &str) -> (&str, Vec<smol_str::SmolStr>) {
    let rest = rest.trim();
    let (path, args) = rest.split_once(' ').unwrap_or((rest, ""));
    let args = args
        .split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(smol_str::SmolStr::new)
        .collect();
    (path, args)
}

/// `mount a ./a`, `pub mount a ./a`, `unit mount a ./a` — the segment the
/// target is mounted as, the reach the mount carries, and the file it names.
/// Unstated, a mount is private to the mounting file's own namespace, which
/// is what `mod x;` means where the word is optional.
fn mount_line(line: &str) -> Option<(Reach, smol_str::SmolStr, &str)> {
    let (reach, rest) = reach_prefix(line);
    let rest = rest.strip_prefix("mount ")?;
    let (segment, specifier) = rest.trim().split_once(' ')?;
    Some((
        reach.unwrap_or(Reach::Namespace { up: 0 }),
        smol_str::SmolStr::new(segment),
        specifier.trim(),
    ))
}

/// `import ./x`, `lazy-import ./x`, `erased-import ./x` — the three moments an
/// import can run, spelled as kmock lines.
fn timed_import(line: &str) -> Option<(Timing, &str)> {
    line.strip_prefix("import ")
        .map(|rest| (Timing::Load, rest))
        .or_else(|| {
            line.strip_prefix("lazy-import ")
                .map(|rest| (Timing::Lazy, rest))
        })
        .or_else(|| {
            line.strip_prefix("erased-import ")
                .map(|rest| (Timing::Erased, rest))
        })
}

fn lines_with_spans(text: &str) -> Vec<(&str, Span)> {
    let mut out = Vec::new();
    let mut offset = 0u32;
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        if !line.is_empty() {
            out.push((line, Span::new(offset, offset + line.len() as u32)));
        }
        offset += raw.len() as u32;
    }
    out
}

/// A throwaway project on disk (tempfile-backed — never a hand-rolled temp path).
pub struct TempProject {
    dir: tempfile::TempDir,
}

impl TempProject {
    pub fn new() -> Self {
        TempProject {
            dir: tempfile::tempdir().expect("create temp project"),
        }
    }

    pub fn file(&self, rel: &str, content: &str) -> &Self {
        let path = self.dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(path, content).expect("write project file");
        self
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }
}

impl Default for TempProject {
    fn default() -> Self {
        Self::new()
    }
}

/// The three liveness stories every frontend test needs, as one tiny JS
/// project: a production root (`src/index.js`, the manifest's `main`), a
/// symbol it keeps (`src/used.js#used`), and a floating orphan
/// (`src/orphan.js#floats`) that yields exactly one unused finding.
pub fn js_demo_project() -> TempProject {
    let p = TempProject::new();
    p.file(
        "package.json",
        r#"{ "name": "demo", "main": "src/index.js" }"#,
    );
    p.file(
        "src/index.js",
        "export function api() { return used(); }\nimport { used } from \"./used.js\";\n",
    );
    p.file("src/used.js", "export function used() { return 1; }\n");
    p.file("src/orphan.js", "export function floats() {}\n");
    p
}

/// One extraction, harness-style: feed `source` to `adapter` as `path` through a
/// fresh sink and return the finished evidence. The shared front half of every
/// adapter's extraction tests.
pub fn extract_evidence(
    adapter: &dyn Extension,
    path: &str,
    source: &str,
) -> kndo_contract::evidence::FileEvidence {
    let path = ProjectPath::new(path);
    let mut sink = EvidenceSink::new(source.len() as u32, adapter.spec().emits().clone());
    adapter.extract(
        &SourceFile {
            path: &path,
            content: source.as_bytes(),
            region: None,
        },
        &mut sink,
    );
    sink.finish()
}

/// One manifest read, harness-style: hand `content` to `adapter` as `path`
/// through a fresh sink and return the finished evidence. `tree` is the file
/// set the reader may ask about — a source root is a question about what
/// exists, so a manifest test that pretends the tree is empty tests a
/// different manifest. The shared front half of every adapter's manifest
/// tests.
pub fn manifest_evidence(
    adapter: &dyn Extension,
    path: &str,
    content: &str,
    tree: &[&str],
) -> kndo_contract::manifest::ManifestEvidence {
    let known: std::collections::BTreeSet<ProjectPath> =
        tree.iter().map(|p| ProjectPath::new(*p)).collect();
    let cx = ResolveContext::new(&known);
    let path = ProjectPath::new(path);
    let mut sink = kndo_contract::manifest::ManifestSink::new();
    adapter.extract_manifest(
        &SourceFile {
            path: &path,
            content: content.as_bytes(),
            region: None,
        },
        &cx,
        &mut sink,
    );
    sink.finish()
}

/// The declaration named `name`, or a panic that prints every declaration — the
/// assertion failure an extraction test wants to read.
pub fn declaration_named<'e>(
    ev: &'e kndo_contract::evidence::FileEvidence,
    name: &str,
) -> &'e kndo_contract::evidence::Declaration {
    ev.declarations
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("declaration {name} missing: {:#?}", ev.declarations))
}

/// One resolution against a synthetic file set — the shared front half of every
/// adapter's resolution tests.
pub fn resolve_in(
    adapter: &dyn Extension,
    files: &[&str],
    from: &str,
    specifier: &str,
) -> Resolution {
    let known: std::collections::BTreeSet<ProjectPath> =
        files.iter().map(|p| ProjectPath::new(*p)).collect();
    let cx = ResolveContext::new(&known);
    adapter.resolve(&ProjectPath::new(from), specifier, &cx)
}

/// The first import whose specifier matches, or a panic that prints every import —
/// the assertion failure an extraction test wants to read.
pub fn import_named<'e>(
    ev: &'e kndo_contract::evidence::FileEvidence,
    specifier: &str,
) -> &'e kndo_contract::evidence::Import {
    ev.imports
        .iter()
        .find(|i| match &i.target {
            ImportTarget::Relative(s) | ImportTarget::Package(s) => s == specifier,
            _ => false,
        })
        .unwrap_or_else(|| panic!("import {specifier} missing: {:#?}", ev.imports))
}
