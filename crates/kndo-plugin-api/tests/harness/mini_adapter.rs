//! A minimal text-driven `LanguageAdapter` shared by the plugin compliance suite and the
//! compat-matrix test (each pulls this in via `#[path]` — `tests/harness/` is not itself a
//! test binary). The same tiny grammar `kndo-core`'s own `DiffMockAdapter` uses (`decl
//! <name>`, `ref <name>`, `root-file`, `import ./sibling.mock`, `callsite <callee>
//! <literal>`), reimplemented here rather than depending on kndo-core's private test module.
//! Its only job is giving the WASM plugin under test a real graph to query and mutate.

use kndo_core::adapter::{
    AdapterDescriptor, Declaration, FileClaim, FileFacts, ImportKind, ImportSpec, LanguageAdapter,
    ManifestFacts, RawImport, RawReference, RawRoot, RawRootTarget, Resolution, ResolveCtx,
    SourceFile, Span,
};
use kndo_core::vocab::{Confidence, RefKind, RootKind, SymbolKind};
use smol_str::SmolStr;

pub struct MiniAdapter;

impl LanguageAdapter for MiniAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            activation: Vec::new(),
            dependencies: Vec::new(),
            id: SmolStr::new("mini"),
            facts_schema_version: 1,
            file_globs: vec![SmolStr::new("**/*.mock")],
            manifest_globs: vec![],
            grammar_version: SmolStr::new("mini"),
            visibility_ladder: vec![
                kndo_core::adapter::VisibilityRung {
                    scope: kndo_core::adapter::VisibilityScope::Unit,
                    label: SmolStr::new("private"),
                    surface_transitive: false,
                },
                kndo_core::adapter::VisibilityRung {
                    scope: kndo_core::adapter::VisibilityScope::Public,
                    label: SmolStr::new("exported"),
                    surface_transitive: true,
                },
            ],
            cycle_policy: kndo_core::adapter::CyclePolicy {
                file_cycles: kndo_core::adapter::CycleTolerance::Idiomatic,
                package_cycles: kndo_core::adapter::CycleTolerance::Idiomatic,
            },
            resolves_dependency_usage: false,
        }
    }

    fn claim(&self, path: &kndo_core::adapter::ProjectPath) -> Option<FileClaim> {
        path.0.ends_with(".mock").then(|| FileClaim {
            language: SmolStr::new("mini"),
            class: Default::default(),
        })
    }

    fn claim_manifest(&self, _path: &kndo_core::adapter::ProjectPath) -> bool {
        false
    }

    fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
        let text = std::str::from_utf8(file.content).unwrap_or("");
        let mut facts = FileFacts::default();
        for line in text.lines() {
            if line == "root-file" {
                facts.roots.push(RawRoot {
                    kind: RootKind::Production,
                    target: RawRootTarget::WholeFile,
                    confidence: Confidence::Certain,
                });
            } else if let Some(spec) = line.strip_prefix("import ") {
                facts.imports.push(RawImport {
                    specifier: SmolStr::new(spec),
                    kind: ImportKind::Relative,
                    span: Span::default(),
                    side_effect_only: true,
                    type_only: false,
                    confidence: Confidence::Certain,
                    bindings: vec![],
                    reexported: false,
                    opaque_namespace_use: false,
                    local_alias: None,
                });
            } else if let Some(name) = line.strip_prefix("decl ") {
                facts.declarations.push(Declaration {
                    name: SmolStr::new(name),
                    kind: SymbolKind::Function,
                    span: Span::default(),
                    exported: true,
                    visibility: kndo_core::adapter::VisibilityLevel(1),
                    member_of: None,
                    signature_span: None,
                });
            } else if let Some(name) = line.strip_prefix("ref ") {
                facts.references.push(RawReference {
                    name: SmolStr::new(name),
                    scope_context: None,
                    span: Span::default(),
                    within: None,
                    kind: RefKind::Read,
                });
            } else if let Some(rest) = line.strip_prefix("callsite ") {
                // `callsite <callee> <literal>` — the RFC 0017 §5.4 fact, mini-syntax form.
                if let Some((callee, literal)) = rest.split_once(' ') {
                    facts
                        .string_call_args
                        .push(kndo_core::adapter::StringCallArg {
                            callee: SmolStr::new(callee),
                            literal: SmolStr::new(literal),
                            span: Span::default(),
                        });
                }
            }
        }
        facts
    }

    fn extract_manifest(&self, _file: &SourceFile<'_>, _ctx: &ResolveCtx<'_>) -> ManifestFacts {
        ManifestFacts::default()
    }

    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
        let Some(rel) = spec.specifier.strip_prefix("./") else {
            return Resolution::Unresolved;
        };
        let dir = spec.from.0.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let candidate = if dir.is_empty() {
            rel.to_string()
        } else {
            format!("{dir}/{rel}")
        };
        let path = kndo_core::adapter::ProjectPath(SmolStr::new(candidate));
        if ctx.contains(&path) {
            Resolution::File(path, Confidence::Certain)
        } else {
            Resolution::Unresolved
        }
    }
}
