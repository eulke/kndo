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
}
