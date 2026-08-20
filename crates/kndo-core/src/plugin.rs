//! The `Plugin` contract (contracts/core-traits.md §3, RFC 0003).
//!
//! Adapters describe what code *is*; plugins describe what an ecosystem *means* by it.
//! All hooks are optional; the same trait serves built-ins (statically linked) and external
//! WASM components (bridged via `kndo-plugin-api`, ADR 0003). `GraphView` is read-only;
//! mutation happens only through typed sinks the core validates and attributes.

use smol_str::SmolStr;

use crate::adapter::ProjectPath;
use crate::vocab::FileClass;

#[derive(Debug, Clone)]
pub struct PluginDescriptor {
    pub id: SmolStr,
    pub version: SmolStr,
    /// Auto-detection predicates ("package.json depends on react") — shown by `kndo doctor`.
    pub detection: Vec<SmolStr>,
    /// Globs whose content the host will provide; no ambient fs/net (RFC 0003 §5).
    pub requested_file_access: Vec<SmolStr>,
}

/// Read-only view over the assembled graph. Grows with graph assembly in M1; the type exists
/// now so hook signatures are stable from the first commit.
#[derive(Debug, Default)]
pub struct GraphView {}

/// Typed sinks — the only mutation path plugins have. The core validates every contribution
/// (no dangling ids, no new kinds) and attributes it (`Provenance::Plugin`).
#[derive(Debug, Default)]
pub struct RootSink {}
#[derive(Debug, Default)]
pub struct EdgeSink {}
#[derive(Debug, Default)]
pub struct AnnotationSink {}

pub trait Plugin: Send + Sync {
    fn descriptor(&self) -> PluginDescriptor;

    /// Adjust a file's role/origin beyond language defaults (e.g. `*.stories.tsx` → tooling).
    fn classify_file(&self, _path: &ProjectPath, _current: FileClass) -> Option<FileClass> {
        None
    }

    /// Framework entry points: routes, DI-registered beans, handlers…
    fn contribute_roots(&self, _graph: &GraphView, _out: &mut RootSink) {}

    /// Edges invisible to the language: DI wiring, template → class, route → handler…
    fn contribute_edges(&self, _graph: &GraphView, _out: &mut EdgeSink) {}

    /// Mark symbols externally consumed (FFI, serialization targets, public SDK surface).
    fn annotate_symbols(&self, _graph: &GraphView, _out: &mut AnnotationSink) {}

    /// Parse one coverage report into per-file line coverage (ADR 0005, RFC 0003 §2's
    /// `ingest_coverage` hook). Content arrives via the host — the report was matched by this
    /// plugin's `descriptor().requested_file_access` globs and freshness-checked before this
    /// is called; no ambient fs. Paths inside the report must be normalized to
    /// project-relative form by the plugin (it alone knows the format's path conventions).
    fn ingest_coverage(
        &self,
        _path: &ProjectPath,
        _content: &[u8],
        _out: &mut crate::coverage::CoverageSink,
    ) {
    }
}

/// The built-in lcov ingester (ADR 0005's first launch format — the lingua franca:
/// jest/vitest/nyc, llvm-cov, gcov, Go via converters). Statically linked, same trait external
/// WASM plugins implement (RFC 0003: "the same trait serves built-ins").
pub struct LcovPlugin;

impl Plugin for LcovPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("coverage-lcov"),
            version: SmolStr::new("1"),
            detection: vec![SmolStr::new("an lcov.info file at a well-known path")],
            // Well-known locations (ADR 0005: "located by config or well-known paths");
            // config-based locations land with the config parser.
            requested_file_access: vec![
                SmolStr::new("coverage/lcov.info"),
                SmolStr::new("lcov.info"),
            ],
        }
    }

    /// The lcov subset that matters: `SF:<path>` opens a file section, `DA:<line>,<hits>`
    /// records one instrumented line, `end_of_record` closes it — everything else (function/
    /// branch records, checksums) is ignored, since kndo maps lines to functions itself via
    /// symbol spans. Absolute `SF:` paths are relativized when they contain the project's
    /// layout; ones that can't be are kept verbatim and simply match nothing — degrade to
    /// silence, never to a wrong file.
    fn ingest_coverage(
        &self,
        _path: &ProjectPath,
        content: &[u8],
        out: &mut crate::coverage::CoverageSink,
    ) {
        let Ok(text) = std::str::from_utf8(content) else {
            return;
        };
        let mut current: Option<ProjectPath> = None;
        for line in text.lines() {
            let line = line.trim_end();
            if let Some(sf) = line.strip_prefix("SF:") {
                let normalized = sf.trim().trim_start_matches("./").replace('\\', "/");
                current = Some(ProjectPath(SmolStr::new(normalized)));
            } else if let Some(da) = line.strip_prefix("DA:") {
                if let Some(file) = &current {
                    let mut parts = da.splitn(3, ',');
                    let line_no = parts.next().and_then(|s| s.trim().parse::<u32>().ok());
                    let hits = parts.next().and_then(|s| s.trim().parse::<u64>().ok());
                    if let (Some(line_no), Some(hits)) = (line_no, hits) {
                        out.add_line(file.clone(), line_no, hits);
                    }
                }
            } else if line == "end_of_record" {
                current = None;
            }
        }
    }
}
