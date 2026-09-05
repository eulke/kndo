//! The reference external adapter: a deliberately tiny invented language
//! ("kmini") hand-scanned with no dependency beyond the SDK — a tree-sitter
//! grammar would mean cross-compiling C to wasm32, a choice for native adapters,
//! not a requirement of the ABI. The author-facing surface is the point: this
//! crate implements the same [`Extension`] a built-in does, sink, resolve
//! context and all, and two lines at the bottom export it as a component.
//!
//! kmini, the whole of it — one statement per line:
//! ```text
//! entry                  the file is a production entry point
//! pub fn name            an exported function
//! fn name                a private function
//! call name              a reference to a function
//! use ./file             an import of ./file.kmini (side-effect shape)
//! use ./file name        an import binding `name` from ./file.kmini
//! use pkg                a bare import of a manifest-declared package
//! lazy use …             the same import, run when the code around it does
//! type use …             the same import, for the type checker alone
//! @path arg …            a marker on the next declaration (`@test`, `@keep`)
//! @! path arg …          a marker on the whole file
//! # text                 a comment (the Comments stream)
//! ```
//! What a marker means is the spec's dispatch rules — `@test` roots a Test
//! entry, `@keep` exempts from `unused` — matched host-side like every
//! language's; and kmini declares its import cycles a hazard, so `cyclic`
//! judges its load-time edges.
//! A `kmini.pkg` manifest declares `name <package>`, `entry <path>`, and
//! `dep <name>` lines. Files `x.kmini` and `x_part.kmini` are one compilation
//! unit — a pure function of path and file set, as the contract demands.

use kndo_contract::adapter::{PackageEntry, ProjectRoot, Resolution, ResolveContext, SourceFile};
use kndo_contract::evidence::{
    EvidenceSink, EvidenceStream, EvidenceStreams, ImportBinding, ImportShape, ImportTarget,
    MarkerTarget, Reach, RefKind, RootKind, RootTarget, SymbolKind, Timing,
};
use kndo_contract::extension::{
    CycleTolerance, DispatchRule, Effect, Extension, ExtensionSpec, Trigger,
};
use kndo_contract::vocab::{Confidence, ProjectPath, Span};
use smol_str::SmolStr;

pub struct KminiAdapter {
    spec: ExtensionSpec,
}

impl Default for KminiAdapter {
    fn default() -> Self {
        let rule = |when: Trigger, then: Effect| DispatchRule {
            when,
            then,
            confidence: Confidence::Certain,
        };
        KminiAdapter {
            // 2: markers, dispatch rules, import timing and cycle tolerance.
            spec: ExtensionSpec::builder("kmini", 2)
                .suffixes(&["kmini"])
                .emits(EvidenceStreams::of(&[
                    EvidenceStream::Comments,
                    EvidenceStream::Markers,
                ]))
                .manifests(&["**/kmini.pkg"])
                .import_cycles(CycleTolerance::Hazard)
                .dispatch(vec![
                    rule(Trigger::marker("test"), Effect::Root(RootKind::Test)),
                    rule(Trigger::marker("keep"), Effect::Exempt),
                ])
                .build(),
        }
    }
}

/// `@path arg arg` → the marker's path and arguments.
fn marker_parts(text: &str) -> (&str, Vec<SmolStr>) {
    let mut words = text.split_whitespace();
    let path = words.next().unwrap_or("");
    (path, words.map(SmolStr::new).collect())
}

fn line_spans(content: &[u8]) -> impl Iterator<Item = (u32, &str)> {
    let text = std::str::from_utf8(content).unwrap_or("");
    let mut offset = 0u32;
    text.split_inclusive('\n').map(move |raw| {
        let start = offset;
        offset += raw.len() as u32;
        (start, raw.trim_end_matches('\n'))
    })
}

fn manifest_lines(content: &[u8]) -> impl Iterator<Item = (&str, &str)> {
    std::str::from_utf8(content)
        .unwrap_or("")
        .lines()
        .filter_map(|l| l.trim().split_once(' '))
        .map(|(k, v)| (k, v.trim()))
}

/// `use …`, `lazy use …`, `type use …` — the three moments an import runs.
fn timed_use(line: &str) -> Option<(Timing, &str)> {
    let (head, rest) = line.split_once(' ')?;
    match head {
        "use" => Some((Timing::Load, rest)),
        "lazy" => Some((Timing::Lazy, rest.strip_prefix("use ")?)),
        "type" => Some((Timing::Erased, rest.strip_prefix("use ")?)),
        _ => None,
    }
}

/// `x.kmini` ⇄ `x_part.kmini`, when both exist.
fn mate_of(path: &ProjectPath) -> Option<ProjectPath> {
    let stem = path.as_str().strip_suffix(".kmini")?;
    match stem.strip_suffix("_part") {
        Some(base) => Some(ProjectPath::new(format!("{base}.kmini"))),
        None => Some(ProjectPath::new(format!("{stem}_part.kmini"))),
    }
}

impl Extension for KminiAdapter {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        // `@…` lines mark the declaration that follows them.
        let mut pending: Vec<(String, Vec<SmolStr>, Span)> = Vec::new();
        for (start, line) in line_spans(file.content) {
            let span = Span::new(start, start + line.len() as u32);
            let trimmed = line.trim();
            let declared = if let Some(name) = trimmed.strip_prefix("pub fn ") {
                Some(out.declaration(name.trim(), SymbolKind::Function, span, Reach::Exported))
            } else if let Some(name) = trimmed.strip_prefix("fn ") {
                Some(out.declaration(name.trim(), SymbolKind::Function, span, Reach::Private))
            } else {
                None
            };
            if let Some(id) = declared {
                for (path, args, span) in pending.drain(..) {
                    out.marker(MarkerTarget::Declaration(id), path, args, span);
                }
                continue;
            }
            if trimmed == "entry" {
                out.root(
                    RootTarget::WholeFile,
                    RootKind::Production,
                    Confidence::Certain,
                );
            } else if let Some(rest) = trimmed.strip_prefix("@!") {
                let (path, args) = marker_parts(rest);
                out.marker(MarkerTarget::File, path, args, span);
            } else if let Some(rest) = trimmed.strip_prefix('@') {
                let (path, args) = marker_parts(rest);
                pending.push((path.to_string(), args, span));
            } else if let Some(name) = trimmed.strip_prefix("call ") {
                out.reference(name.trim(), RefKind::Call, span);
            } else if let Some((timing, rest)) = timed_use(trimmed) {
                let mut parts = rest.split_whitespace();
                let Some(specifier) = parts.next() else {
                    continue;
                };
                let target = if specifier.starts_with("./") || specifier.starts_with("../") {
                    ImportTarget::Relative(SmolStr::new(specifier))
                } else {
                    ImportTarget::Package(SmolStr::new(specifier))
                };
                let shape = match parts.next() {
                    Some(name) => ImportShape::Bindings(vec![ImportBinding {
                        imported: SmolStr::new(name),
                        local: SmolStr::new(name),
                    }]),
                    None => ImportShape::SideEffect,
                };
                out.import_at(timing, target, shape, span, Confidence::Certain);
            } else if let Some(text) = trimmed.strip_prefix('#') {
                let text_start = span.end - text.trim_start().len() as u32;
                out.comment(span, Span::new(text_start, span.end));
            }
        }
    }

    fn resolve(&self, from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
        if let Some(rest) = specifier.strip_prefix("./") {
            let dir = match from.as_str().rfind('/') {
                Some(i) => &from.as_str()[..i],
                None => "",
            };
            let candidate = if dir.is_empty() {
                ProjectPath::new(format!("{rest}.kmini"))
            } else {
                ProjectPath::new(format!("{dir}/{rest}.kmini"))
            };
            if cx.contains(&candidate) {
                return Resolution::File(candidate);
            }
            return Resolution::Unresolved;
        }
        // A bare specifier is a manifest-declared package — the same query a
        // native adapter runs, against the same context.
        match cx.package(specifier).and_then(|p| p.entry.clone()) {
            Some(entry) => Resolution::File(entry),
            None => Resolution::Unresolved,
        }
    }

    fn roots(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<ProjectRoot> {
        manifest_lines(manifest.content)
            .filter(|(k, _)| *k == "entry")
            .map(|(_, v)| ProjectPath::new(v))
            .filter(|p| cx.contains(p))
            .map(|file| ProjectRoot {
                file,
                kind: RootKind::Production,
                confidence: Confidence::Certain,
            })
            .collect()
    }

    fn packages(&self, manifest: &SourceFile<'_>, cx: &ResolveContext<'_>) -> Vec<PackageEntry> {
        let dir = match manifest.path.as_str().rfind('/') {
            Some(i) => &manifest.path.as_str()[..i],
            None => "",
        };
        let name = manifest_lines(manifest.content).find(|(k, _)| *k == "name");
        let entry = manifest_lines(manifest.content)
            .find(|(k, _)| *k == "entry")
            .map(|(_, v)| ProjectPath::new(v))
            .filter(|p| cx.contains(p));
        match name {
            Some((_, name)) => vec![PackageEntry {
                name: SmolStr::new(name),
                entry,
                dir: SmolStr::new(dir),
            }],
            None => Vec::new(),
        }
    }

    fn manifest_dependencies(
        &self,
        manifest: &SourceFile<'_>,
    ) -> Vec<kndo_contract::adapter::DependencyDeclaration> {
        manifest_lines(manifest.content)
            .filter(|(k, _)| *k == "dep")
            .map(|(_, v)| kndo_contract::adapter::DependencyDeclaration::name_only(SmolStr::new(v)))
            .collect()
    }

    fn sees(&self, path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
        mate_of(path)
            .filter(|m| cx.contains(m))
            .into_iter()
            .collect()
    }
}

kndo_sdk::export_extension!(KminiAdapter);
