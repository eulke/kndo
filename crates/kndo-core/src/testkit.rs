//! Shared test infrastructure: an in-memory [`MockAdapter`] driving a tiny text DSL, used by
//! this crate's own graph/analysis tests. Gated behind the `testkit` feature (in addition to
//! `cfg(test)`) so a downstream crate's test suite that needs a real end-to-end
//! `LanguageAdapter` — kndo-plugin-api's WASM compliance tests, previously — can depend on the
//! canonical mock via `kndo-core = { features = ["testkit"] }` instead of reimplementing its
//! grammar because this module used to be private.
//!
//! kndo-core must never depend on a real language adapter (that would invert the ignorance
//! rule) — `MockAdapter` is the one exception, and it lives only here, behind this feature.

// MockAdapter's DSL touches most of the adapter-facts and vocab vocabularies (every fact
// kind an adapter can emit, since it exists to drive every graph-assembly code path under
// test) — glob-importing both is more honest than hand-tracking which symbol each keyword
// needs, matching the same pattern `graph::tests` uses for the same reason.
#[allow(unused_imports)]
use crate::adapter::*;
#[allow(unused_imports)]
use crate::vocab::*;
use smol_str::SmolStr;

pub struct MockAdapter;

impl LanguageAdapter for MockAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            activation: Vec::new(),
            dependencies: Vec::new(),
            id: SmolStr::new("mock"),
            facts_schema_version: 1,
            file_globs: vec![SmolStr::new("**/*.mock")],
            manifest_globs: vec![],
            grammar_version: SmolStr::new("n/a"),
            visibility_ladder: vec![
                crate::adapter::VisibilityRung {
                    scope: crate::adapter::VisibilityScope::Unit,
                    label: SmolStr::new("private"),
                    surface_transitive: false,
                },
                crate::adapter::VisibilityRung {
                    scope: crate::adapter::VisibilityScope::Public,
                    label: SmolStr::new("exported"),
                    surface_transitive: true,
                },
            ],
            cycle_policy: crate::adapter::CyclePolicy {
                file_cycles: crate::adapter::CycleTolerance::Hazard,
                package_cycles: crate::adapter::CycleTolerance::Hazard,
            },
            resolves_dependency_usage: true,
            package_test_dirs: Vec::new(),
        }
    }

    fn claim(&self, path: &ProjectPath) -> Option<FileClaim> {
        path.0.ends_with(".mock").then(|| {
            // Role by filename convention, mirroring real adapters' classification:
            // "*.test.*" → Test, "*.config.*" → Tooling, everything else Production.
            let role = if path.0.contains(".test.") {
                FileRole::Test
            } else if path.0.contains(".config.") {
                FileRole::Tooling
            } else {
                FileRole::Production
            };
            FileClaim {
                language: SmolStr::new("mock"),
                class: FileClass {
                    role,
                    origin: FileOrigin::Authored,
                },
            }
        })
    }

    fn claim_manifest(&self, path: &ProjectPath) -> bool {
        path.0.rsplit('/').next() == Some("manifest.json")
    }

    fn extract(&self, file: &SourceFile<'_>) -> FileFacts {
        // Content format for the mock: one directive per line.
        //   decl <name>                 -> an exported Function declaration
        //   private-decl <name>         -> an unexported Function declaration
        //   import <specifier> [binding[,binding...]]     -> RawImport { reexported: false }
        //   reexport <specifier> [binding[,binding...]]   -> RawImport { reexported: true }
        //   import-opaque <specifier>                     -> RawImport { opaque_namespace_use: true }
        //   import-visible <specifier>                    -> RawImport { module_names_visible: true }
        //       binding := name          -> ImportBinding { local: name, imported: Some(name) }
        //                | local=imported -> ImportBinding { local, imported: Some(imported) }
        //                | local=          -> ImportBinding { local, imported: None } (default)
        //   ref <name>                  -> a RawReference to that name
        //   root-file                   -> a Production root targeting this whole file
        //   root-decl <name>            -> a Production root targeting the named declaration
        //   dynamic                     -> an un-narrowed DynamicUse (eval-style)
        //   dynamic-narrowed <dir>      -> a DynamicUse narrowed to that project dir
        //   suppress <category>         -> a Declaration-scope RawSuppression
        //   unit <key>                  -> FileFacts::unit (package-scoped resolution)
        // detected-generated -> FileFacts::detected_origin = Generated
        let text = std::str::from_utf8(file.content).unwrap_or("");
        let mut facts = FileFacts::default();
        for line in text.lines() {
            if let Some(key) = line.strip_prefix("unit ") {
                facts.unit = Some(SmolStr::new(key));
            } else if line == "detected-generated" {
                // Content-derived origin override.
                facts.detected_origin = Some(FileOrigin::Generated);
            } else if let Some(name) = line.strip_prefix("decl ") {
                facts.declarations.push(Declaration {
                    name: SmolStr::new(name),
                    kind: SymbolKind::Function,
                    span: Span::default(),
                    exported: true,
                    visibility: VisibilityLevel(1),
                    member_of: None,
                    signature_span: None,
                    implicitly_invoked: false,
                    nested_scope: false,
                    visibility_inherited: false,
                });
            } else if let Some(name) = line.strip_prefix("private-decl ") {
                facts.declarations.push(Declaration {
                    name: SmolStr::new(name),
                    kind: SymbolKind::Function,
                    span: Span::default(),
                    exported: false,
                    visibility: VisibilityLevel(0),
                    member_of: None,
                    signature_span: None,
                    implicitly_invoked: false,
                    nested_scope: false,
                    visibility_inherited: false,
                });
            } else if let Some(rest) = line
                .strip_prefix("member-decl ")
                .or_else(|| line.strip_prefix("member-decl-exported "))
            {
                // `member-decl <owner> <name>` — an unexported member declaration
                //: bare name, structured owner. The `-exported` variant
                // declares at ladder level 1 (`Public` on the mock ladder) for the
                // fallback's visibility-scoped candidacy.
                let exported = line.starts_with("member-decl-exported ");
                let mut parts = rest.splitn(2, ' ');
                let owner = parts.next().unwrap_or("");
                let name = parts.next().unwrap_or("");
                facts.declarations.push(Declaration {
                    name: SmolStr::new(name),
                    kind: SymbolKind::Method,
                    span: Span::default(),
                    exported,
                    visibility: VisibilityLevel(exported as u8),
                    member_of: Some(SmolStr::new(owner)),
                    signature_span: None,
                    implicitly_invoked: false,
                    nested_scope: false,
                    visibility_inherited: false,
                });
            } else if let Some(rest) = line.strip_prefix("member-implicit ") {
                // `member-implicit <owner> <name>` — a machinery-dispatched member
                // (the machinery-dispatch rule): invoked through its owner,
                // never by name at the call site.
                let mut parts = rest.splitn(2, ' ');
                let owner = parts.next().unwrap_or("");
                let name = parts.next().unwrap_or("");
                facts.declarations.push(Declaration {
                    name: SmolStr::new(name),
                    kind: SymbolKind::Method,
                    span: Span::default(),
                    exported: false,
                    visibility: VisibilityLevel(0),
                    member_of: Some(SmolStr::new(owner)),
                    signature_span: None,
                    implicitly_invoked: true,
                    nested_scope: false,
                    visibility_inherited: false,
                });
            } else if let Some(rest) = line
                .strip_prefix("import ")
                .or_else(|| line.strip_prefix("reexport-opaque "))
                .or_else(|| line.strip_prefix("reexport "))
                .or_else(|| line.strip_prefix("import-opaque "))
                .or_else(|| line.strip_prefix("import-visible "))
            {
                // `reexport-opaque <spec>` — a re-exported glob (`export * from './x'`,
                // Rust `pub use x::*`): reexported with no bindings, namespace-opaque.
                let reexported =
                    line.starts_with("reexport ") || line.starts_with("reexport-opaque ");
                let opaque_namespace_use =
                    line.starts_with("import-opaque ") || line.starts_with("reexport-opaque ");
                // `import-visible <spec>` — the language's scoping rule rather than a glob:
                // every top-level name of the target's unit is legal HERE unqualified
                // (Kotlin's `import p.*`, Swift's `import SomeKit`).
                let module_names_visible = line.starts_with("import-visible ");
                let mut parts = rest.splitn(2, ' ');
                let spec = parts.next().unwrap_or("");
                let bindings = parts
                    .next()
                    .map(|tokens| {
                        tokens
                            .split(',')
                            .map(|tok| match tok.split_once('=') {
                                Some((local, imported)) => ImportBinding {
                                    local: SmolStr::new(local),
                                    imported: (!imported.is_empty())
                                        .then(|| SmolStr::new(imported)),
                                },
                                None => ImportBinding {
                                    local: SmolStr::new(tok),
                                    imported: Some(SmolStr::new(tok)),
                                },
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                facts.imports.push(RawImport {
                    specifier: SmolStr::new(spec),
                    kind: ImportKind::Relative,
                    span: Span::default(),
                    side_effect_only: false,
                    type_only: false,
                    confidence: Confidence::Certain,
                    bindings,
                    reexported,
                    opaque_namespace_use,
                    module_names_visible,
                    local_alias: None,
                });
            } else if let Some(rest) = line.strip_prefix("import-as ") {
                // `import-as <alias> <specifier>` — an explicitly-aliased namespace
                // import (the `local_alias`).
                let mut parts = rest.splitn(2, ' ');
                let alias = parts.next().unwrap_or("");
                let spec = parts.next().unwrap_or("");
                facts.imports.push(RawImport {
                    specifier: SmolStr::new(spec),
                    kind: ImportKind::Relative,
                    span: Span::default(),
                    side_effect_only: false,
                    type_only: false,
                    confidence: Confidence::Certain,
                    bindings: Vec::new(),
                    reexported: false,
                    opaque_namespace_use: false,
                    module_names_visible: false,
                    local_alias: Some(SmolStr::new(alias)),
                });
            } else if let Some(rest) = line.strip_prefix("member-type ") {
                // `member-type <owner> <member> <yields> [p0,p1,…]` — a member-type
                // fact; the optional 4th token lists the type parameters in order.
                let mut parts = rest.splitn(4, ' ');
                facts.member_types.push(crate::adapter::RawMemberType {
                    owner: SmolStr::new(parts.next().unwrap_or("")),
                    member: SmolStr::new(parts.next().unwrap_or("")),
                    yields: SmolStr::new(parts.next().unwrap_or("")),
                    yields_params: parts
                        .next()
                        .map(|list| list.split(',').map(SmolStr::new).collect())
                        .unwrap_or_default(),
                });
            } else if let Some(name) = line.strip_prefix("invokes-executable ") {
                // A declared subprocess invocation of a workspace executable target
                // (the invoked-program rule).
                facts.invoked_executables.push(SmolStr::new(name));
            } else if let Some(name) = line.strip_prefix("unit-name ") {
                // The name importers bind this unit by.
                facts.unit_name = Some(SmolStr::new(name));
            } else if let Some(rest) = line.strip_prefix("qref ") {
                // `qref <qualifier> <name>` — a qualified reference
                // (the `scope_context`).
                let mut parts = rest.splitn(2, ' ');
                let qualifier = parts.next().unwrap_or("");
                let name = parts.next().unwrap_or("");
                facts.references.push(RawReference {
                    name: SmolStr::new(name),
                    scope_context: Some(SmolStr::new(qualifier)),
                    span: Span::default(),
                    within: None,
                    kind: RefKind::Read,
                });
            } else if let Some(rest) = line.strip_prefix("ref-in ") {
                // `ref-in <within> <name>` — a reference executing inside the named
                // declaration (symbol attribution).
                let mut parts = rest.splitn(2, ' ');
                let within = parts.next().unwrap_or("");
                let name = parts.next().unwrap_or("");
                facts.references.push(RawReference {
                    name: SmolStr::new(name),
                    scope_context: None,
                    span: Span::default(),
                    within: Some(SmolStr::new(within)),
                    kind: RefKind::Read,
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
                // `callsite <callee> <literal>` — a string-literal call-site fact.
                if let Some((callee, literal)) = rest.split_once(' ') {
                    facts.string_call_args.push(crate::adapter::StringCallArg {
                        callee: SmolStr::new(callee),
                        literal: SmolStr::new(literal),
                        span: Span::default(),
                    });
                }
            } else if line == "root-file" {
                facts.roots.push(RawRoot {
                    kind: RootKind::Production,
                    target: RawRootTarget::WholeFile,
                    confidence: Confidence::Certain,
                });
            } else if let Some(name) = line.strip_prefix("root-decl ") {
                facts.roots.push(RawRoot {
                    kind: RootKind::Production,
                    target: RawRootTarget::Declaration(SmolStr::new(name)),
                    confidence: Confidence::Certain,
                });
            } else if let Some(dir) = line.strip_prefix("dynamic-narrowed ") {
                facts.dynamics.push(crate::adapter::DynamicUse {
                    span: Span::default(),
                    reason: SmolStr::new("mock dynamic"),
                    narrowed_to: Some(SmolStr::new(dir)),
                });
            } else if line == "dynamic" {
                facts.dynamics.push(crate::adapter::DynamicUse {
                    span: Span::default(),
                    reason: SmolStr::new("mock dynamic"),
                    narrowed_to: None,
                });
            } else if let Some(category) = line.strip_prefix("suppress ") {
                facts.suppressions.push(crate::adapter::RawSuppression {
                    span: Span::default(),
                    category: SmolStr::new(category),
                    subject: None,
                    reason: None,
                    scope: crate::adapter::SuppressionScope::Declaration,
                });
            } else if let Some(rest) = line.strip_prefix("test-region ") {
                // `test-region <start-line> <end-line>` — a sub-file test region
                // (`FileFacts::test_spans`).
                let mut parts = rest.splitn(2, ' ');
                let a: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let b: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(a);
                facts.test_spans.push(Span {
                    start: (a, 1),
                    end: (b, 999),
                });
            } else if let Some(rest) = line.strip_prefix("mod-link ") {
                // `mod-link <specifier> <line>` — a module-linking side-effect import
                // (Rust's `mod x;` shape: side_effect_only + local_alias), sited at the
                // given line so test-region containment is exercisable.
                let mut parts = rest.splitn(2, ' ');
                let spec = parts.next().unwrap_or("");
                let line_no: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                facts.imports.push(RawImport {
                    specifier: SmolStr::new(spec),
                    kind: ImportKind::Relative,
                    span: Span {
                        start: (line_no, 1),
                        end: (line_no, 10),
                    },
                    side_effect_only: true,
                    type_only: false,
                    confidence: Confidence::Certain,
                    bindings: Vec::new(),
                    reexported: false,
                    opaque_namespace_use: false,
                    module_names_visible: false,
                    local_alias: Some(SmolStr::new(spec.rsplit('/').next().unwrap_or(spec))),
                });
            } else if let Some(rest) = line.strip_prefix("decl-at ") {
                // `decl-at <line> <name>` — an exported declaration spanning that line
                // (for test-region containment: derived Test roots, crap/health skips).
                let mut parts = rest.splitn(2, ' ');
                let line_no: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let name = parts.next().unwrap_or("");
                facts.declarations.push(Declaration {
                    name: SmolStr::new(name),
                    kind: SymbolKind::Function,
                    span: Span {
                        start: (line_no, 1),
                        end: (line_no, 50),
                    },
                    exported: true,
                    visibility: VisibilityLevel(1),
                    member_of: None,
                    signature_span: None,
                    implicitly_invoked: false,
                    nested_scope: false,
                    visibility_inherited: false,
                });
            } else if let Some(rest) = line.strip_prefix("import-at ") {
                // `import-at <line> <specifier>` — a plain import sited at a line (for
                // dependency-hygiene's test-region site role).
                let mut parts = rest.splitn(2, ' ');
                let line_no: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let spec = parts.next().unwrap_or("");
                facts.imports.push(RawImport {
                    specifier: SmolStr::new(spec),
                    kind: ImportKind::Relative,
                    span: Span {
                        start: (line_no, 1),
                        end: (line_no, 10),
                    },
                    side_effect_only: false,
                    type_only: false,
                    confidence: Confidence::Certain,
                    bindings: Vec::new(),
                    reexported: false,
                    opaque_namespace_use: false,
                    module_names_visible: false,
                    local_alias: None,
                });
            }
        }
        facts
    }

    fn extract_manifest(&self, file: &SourceFile<'_>, ctx: &ResolveCtx<'_>) -> ManifestFacts {
        // Content format for the mock manifest: one directive per line.
        //   dep <name>                  -> a prod-scope declared dependency
        //   dep-version <name> <ver>    -> a prod-scope declared dependency with a
        //                                  literal version
        //   dep-inherited <name>        -> a prod-scope dependency whose version comes
        //                                  from the shared pool (`ManifestDependency::inherited`)
        //   workspace-dep <name> <ver>  -> an entry in ManifestFacts::workspace_dependencies
        //   root <path>       -> a Production root targeting that known file, if it exists
        //   cli-invoke <name> -> a script-invoked dependency name
        // declares-surface -> ManifestFacts::declares_surface = true
        //   private           -> ManifestFacts::private = true (app mode)
        //   name <pkg>        -> the package's declared name
        //   entry <path>      -> a resolved entry (what a sibling's bare-name import lands on)
        let text = std::str::from_utf8(file.content).unwrap_or("");
        let mut facts = ManifestFacts::default();
        for line in text.lines() {
            if let Some(name) = line.strip_prefix("dep ") {
                facts.dependencies.push(ManifestDependency {
                    name: SmolStr::new(name),
                    version_req: SmolStr::new("*"),
                    scope: DependencyScope::Prod,
                    inherited: false,
                });
            } else if let Some(name) = line.strip_prefix("dep-inherited ") {
                facts.dependencies.push(ManifestDependency {
                    name: SmolStr::new(name),
                    version_req: SmolStr::new("workspace"),
                    scope: DependencyScope::Prod,
                    inherited: true,
                });
            } else if let Some(rest) = line.strip_prefix("dep-version ") {
                let mut parts = rest.splitn(2, ' ');
                let name = parts.next().unwrap_or("");
                let version_req = parts.next().unwrap_or("*");
                facts.dependencies.push(ManifestDependency {
                    name: SmolStr::new(name),
                    version_req: SmolStr::new(version_req),
                    scope: DependencyScope::Prod,
                    inherited: false,
                });
            } else if let Some(rest) = line.strip_prefix("workspace-dep ") {
                let mut parts = rest.splitn(2, ' ');
                let name = parts.next().unwrap_or("");
                let version_req = parts.next().unwrap_or("*");
                facts.workspace_dependencies.push(ManifestDependency {
                    name: SmolStr::new(name),
                    version_req: SmolStr::new(version_req),
                    scope: DependencyScope::Prod,
                    inherited: false,
                });
            } else if let Some(p) = line.strip_prefix("root ") {
                let target = ProjectPath(SmolStr::new(p));
                if ctx.contains(&target) {
                    facts.roots.push(ManifestRoot {
                        kind: RootKind::Production,
                        target,
                        confidence: Confidence::Certain,
                    });
                }
            } else if let Some(name) = line.strip_prefix("cli-invoke ") {
                facts.script_invoked_names.push(SmolStr::new(name));
            } else if let Some(rest) = line.strip_prefix("executable ") {
                // `executable <name> <path>` — a named executable target (the
                // invoked-program rule), resolved to its entry if the file exists.
                let mut parts = rest.splitn(2, ' ');
                let name = parts.next().unwrap_or("");
                let entry = ProjectPath(SmolStr::new(parts.next().unwrap_or("")));
                if ctx.contains(&entry) {
                    facts.executables.push(crate::adapter::ExecutableTarget {
                        name: SmolStr::new(name),
                        entry,
                    });
                }
            } else if let Some(name) = line.strip_prefix("name ") {
                facts.package_name = Some(SmolStr::new(name));
            } else if let Some(p) = line.strip_prefix("entry ") {
                let target = ProjectPath(SmolStr::new(p));
                if ctx.contains(&target) {
                    facts.resolved_entries.push((target, Confidence::Certain));
                }
            } else if line == "declares-surface" {
                // Explicit entry-point surface (`exports` map equivalent) — the
                // deep-import contract gate.
                facts.declares_surface = true;
            } else if line == "private" {
                facts.private = true;
            }
        }
        facts
    }

    fn resolve(&self, spec: &ImportSpec, ctx: &ResolveCtx<'_>) -> Resolution {
        // Trivial resolver: relative specifiers are exact sibling filenames; anything
        // else is a bare dependency name.
        if let Some(rel) = spec.specifier.strip_prefix("./") {
            let dir = spec.from.0.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let candidate = if dir.is_empty() {
                rel.to_string()
            } else {
                format!("{dir}/{rel}")
            };
            let path = ProjectPath(SmolStr::new(candidate));
            return if ctx.contains(&path) {
                Resolution::File(path, Confidence::Certain)
            } else {
                Resolution::Unresolved
            };
        }
        // Workspace member by name — mirrors the real adapters' precedence (an in-repo
        // name match outranks the external-dependency fallback below).
        if let Some(member) = ctx.workspace_member(&spec.specifier) {
            return match &member.entry {
                Some((target, confidence)) => Resolution::WorkspaceMember {
                    name: spec.specifier.clone(),
                    target: target.clone(),
                    confidence: *confidence,
                    // The same dir-ownership rule real adapters apply, so the
                    // same-package edge derivation is exercisable from graph tests.
                    same_package: !member.dir.is_empty()
                        && spec.from.0.starts_with(&format!("{}/", member.dir)),
                },
                None => Resolution::Unresolved,
            };
        }
        // Confidence is deliberately observable here so tests can tell whether the
        // manifest's declared dependencies actually reached this resolver call —
        // otherwise wiring `declared_dependencies` through the pipeline is untestable
        // from graph.rs (the real shadowing rule itself is already covered where it's
        // implemented, kndo-adapter-toolkit's stdlib module).
        let confidence = if ctx.is_declared_dependency(&spec.specifier) {
            Confidence::Certain
        } else {
            Confidence::Probable
        };
        Resolution::Dependency(spec.specifier.clone(), confidence)
    }
}
