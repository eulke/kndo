//! The reference external adapter: a deliberately tiny invented language
//! ("kmini") hand-scanned with no dependency beyond the SDK — a tree-sitter
//! grammar would mean cross-compiling C to wasm32, a choice for native adapters,
//! not a requirement of the ABI. The author-facing surface is the point: this
//! crate implements the same [`LanguageAdapter`] a native adapter does, sink,
//! resolve context and all, and two lines at the bottom export it as a component.
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
//! # text                 a comment (the Comments stream)
//! ```
//! A `kmini.pkg` manifest declares `name <package>`, `entry <path>`, and
//! `dep <name>` lines. Files `x.kmini` and `x_part.kmini` are one compilation
//! unit — a pure function of path and file set, as the contract demands.

use kndo_contract::adapter::{
    AdapterSpec, LanguageAdapter, PackageEntry, ProjectRoot, Resolution, ResolveContext,
    SourceFile,
};
use kndo_contract::evidence::{
    EvidenceSink, EvidenceStream, EvidenceStreams, ImportBinding, ImportShape, ImportTarget,
    Reach, RefKind, RootKind, RootTarget, SymbolKind,
};
use kndo_contract::vocab::{Confidence, ProjectPath, Span};
use smol_str::SmolStr;

pub struct KminiAdapter {
    spec: AdapterSpec,
}

impl Default for KminiAdapter {
    fn default() -> Self {
        KminiAdapter {
            spec: AdapterSpec::builder("kmini", 1)
                .extensions(&["kmini"])
                .emits(EvidenceStreams::of(&[EvidenceStream::Comments]))
                .manifests(&["**/kmini.pkg"])
                .build(),
        }
    }
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

/// `x.kmini` ⇄ `x_part.kmini`, when both exist.
fn mate_of(path: &ProjectPath) -> Option<ProjectPath> {
    let stem = path.as_str().strip_suffix(".kmini")?;
    match stem.strip_suffix("_part") {
        Some(base) => Some(ProjectPath::new(format!("{base}.kmini"))),
        None => Some(ProjectPath::new(format!("{stem}_part.kmini"))),
    }
}

impl LanguageAdapter for KminiAdapter {
    fn spec(&self) -> &AdapterSpec {
        &self.spec
    }

    fn extract(&self, file: &SourceFile<'_>, out: &mut EvidenceSink) {
        for (start, line) in line_spans(file.content) {
            let span = Span::new(start, start + line.len() as u32);
            let trimmed = line.trim();
            if trimmed == "entry" {
                out.root(RootTarget::WholeFile, RootKind::Production, Confidence::Certain);
            } else if let Some(name) = trimmed.strip_prefix("pub fn ") {
                out.declaration(name.trim(), SymbolKind::Function, span, Reach::Exported);
            } else if let Some(name) = trimmed.strip_prefix("fn ") {
                out.declaration(name.trim(), SymbolKind::Function, span, Reach::Private);
            } else if let Some(name) = trimmed.strip_prefix("call ") {
                out.reference(name.trim(), RefKind::Call, span);
            } else if let Some(rest) = trimmed.strip_prefix("use ") {
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
                out.import(target, shape, span, Confidence::Certain);
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

    fn manifest_dependencies(&self, manifest: &SourceFile<'_>) -> Vec<SmolStr> {
        manifest_lines(manifest.content)
            .filter(|(k, _)| *k == "dep")
            .map(|(_, v)| SmolStr::new(v))
            .collect()
    }

    fn unit_mates(&self, path: &ProjectPath, cx: &ResolveContext<'_>) -> Vec<ProjectPath> {
        mate_of(path).filter(|m| cx.contains(m)).into_iter().collect()
    }
}

kndo_sdk::export_adapter!(KminiAdapter);
