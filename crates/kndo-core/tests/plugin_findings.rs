//! The plugin-findings acceptance bar, exercised end to end with a real third-party-shaped rule:
//! declare → emit → render → baseline → suppress → gate opt-in, through a genuine
//! `Engine::check`. Also proves the structural guarantees: the namespaced category and
//! `convention` group are host-assembled, findings are advisory without a `[plugins.gate]`
//! opt-in, an undeclared rule is dropped loudly, the noise ceiling truncates loudly, the
//! finding round runs on the warm snapshot path, and a findings-only plugin
//! (`mutates_graph = false`) keeps the graph fast paths available while still emitting.

use kndo_core::adapter::{
    AdapterDescriptor, Declaration, FileClaim, FileFacts, LanguageAdapter, ManifestFacts,
    RawSuppression, ResolveCtx, SourceFile, Span, SuppressionScope,
};
use kndo_core::engine::{BaselineOp, ConfigOverrides, Engine, RunMode, Severity};
use kndo_core::plugin::{
    ContentView, FindingSink, GraphView, Plugin, PluginDescriptor, PluginSeverity, PluginTarget,
    RuleDescriptor,
};
use kndo_core::vocab::{Confidence, SymbolKind};
use smol_str::SmolStr;

/// `decl <name>` declares an exported function; `allow <category>` emits a file-scoped
/// suppression — the two facts this suite needs from a language.
struct MiniAdapter;

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
            visibility_ladder: Vec::new(),
            cycle_policy: kndo_core::adapter::CyclePolicy {
                file_cycles: kndo_core::adapter::CycleTolerance::Idiomatic,
                package_cycles: kndo_core::adapter::CycleTolerance::Idiomatic,
            },
            resolves_dependency_usage: false,
            package_test_dirs: Vec::new(),
            builtin_member_types: Vec::new(),
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
        for (i, line) in text.lines().enumerate() {
            let line_no = (i + 1) as u32;
            if let Some(name) = line.strip_prefix("decl ") {
                facts.declarations.push(Declaration {
                    name: SmolStr::new(name),
                    kind: SymbolKind::Function,
                    span: Span {
                        start: (line_no, 1),
                        end: (line_no, 1),
                    },
                    exported: true,
                    visibility: kndo_core::adapter::VisibilityLevel(1),
                    member_of: None,
                    signature_span: None,
                    implicitly_invoked: false,
                    nested_scope: false,
                    visibility_inherited: false,
                    markers: Vec::new(),
                });
            } else if let Some(category) = line.strip_prefix("allow ") {
                facts.suppressions.push(RawSuppression {
                    span: Span {
                        start: (line_no, 1),
                        end: (line_no, 1),
                    },
                    category: SmolStr::new(category),
                    subject: None,
                    reason: None,
                    scope: SuppressionScope::File,
                });
            }
        }
        facts
    }

    fn extract_manifest(&self, _file: &SourceFile<'_>, _ctx: &ResolveCtx<'_>) -> ManifestFacts {
        ManifestFacts::default()
    }

    fn resolve(
        &self,
        _spec: &kndo_core::adapter::ImportSpec,
        _ctx: &ResolveCtx<'_>,
    ) -> kndo_core::adapter::Resolution {
        kndo_core::adapter::Resolution::Unresolved
    }
}

/// The third-party-shaped rule plugin: flags every symbol starting with `flag`, and (to prove
/// the declaration contract) also emits under a rule it never declared. `mutates_graph =
/// false`: a findings-only plugin must not cost the graph fast paths — and the finding round
/// must run for it anyway.
struct RulePlugin {
    severity: PluginSeverity,
    emit_undeclared: bool,
    flood: usize,
}

impl RulePlugin {
    fn warning() -> Self {
        RulePlugin {
            severity: PluginSeverity::Warning,
            emit_undeclared: false,
            flood: 0,
        }
    }
}

impl Plugin for RulePlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("test-plugin"),
            version: SmolStr::new("1"),
            detection: vec![],
            requested_file_access: vec![],
            activation: vec![],
            dependencies: vec![],
        }
    }

    fn mutates_graph(&self) -> bool {
        false
    }

    fn rules(&self) -> Vec<RuleDescriptor> {
        vec![RuleDescriptor {
            name: SmolStr::new("no-flag-symbols"),
            description: SmolStr::new("symbols named flag* are forbidden by this test"),
            severity: self.severity,
        }]
    }

    fn contribute_findings(
        &self,
        graph: &GraphView<'_>,
        _content: &ContentView<'_>,
        out: &mut FindingSink,
    ) {
        for file in graph.files() {
            for symbol in graph.symbols_in(&file.path) {
                if symbol.name.starts_with("flag") {
                    out.add(
                        "no-flag-symbols",
                        PluginTarget::symbol(file.path.clone(), symbol.name.clone()),
                        Confidence::Probable,
                        format!("symbol `{}` uses the forbidden prefix", symbol.name),
                    );
                }
            }
        }
        if self.emit_undeclared {
            out.add(
                "ghost-rule",
                PluginTarget::file(kndo_core::adapter::ProjectPath(SmolStr::new("a.mock"))),
                Confidence::Certain,
                "should be dropped",
            );
        }
        for i in 0..self.flood {
            out.add(
                "no-flag-symbols",
                PluginTarget::file(kndo_core::adapter::ProjectPath(SmolStr::new("a.mock"))),
                Confidence::Possible,
                format!("flood {i}"),
            );
        }
    }
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp project");
    for (name, content) in files {
        std::fs::write(dir.path().join(name), content).unwrap();
    }
    dir
}

fn check(engine: &mut Engine) -> kndo_core::engine::RunResult {
    engine.check(RunMode::Full)
}

fn open(dir: &tempfile::TempDir, plugin: RulePlugin, use_cache: bool) -> Engine {
    Engine::open_with_plugins(
        dir.path(),
        ConfigOverrides {
            use_cache,
            threads: Some(1),
            min_confidence: None,
        },
        vec![Box::new(MiniAdapter)],
        vec![Box::new(plugin)],
    )
    .expect("open")
}

#[test]
fn a_declared_rule_emits_namespaced_advisory_findings() {
    let dir = project(&[("a.mock", "decl fine\ndecl flag_me\n")]);
    let mut engine = open(&dir, RulePlugin::warning(), false);
    let result = check(&mut engine);

    let f = result
        .findings
        .iter()
        .find(|f| f.category.starts_with("plugin:"))
        .expect("the plugin finding must be in the output");
    assert_eq!(f.category, "plugin:test-plugin/no-flag-symbols");
    assert_eq!(f.group, kndo_core::vocab::Group::Convention);
    assert_eq!(
        f.severity,
        Severity::Warning,
        "declared severity is displayed"
    );
    assert!(
        f.advisory,
        "no [plugins.gate] entry → advisory: visible, attributed, inert to gates"
    );
    assert_eq!(f.location.symbol.as_deref(), Some("flag_me"));
    assert_eq!(f.subject_kind, "function");
    assert!(
        result
            .findings
            .iter()
            .all(|f| { f.category.starts_with("plugin:") || !f.advisory }),
        "core findings are never advisory"
    );
}

#[test]
fn an_undeclared_rule_is_dropped_with_a_diagnostic() {
    let dir = project(&[("a.mock", "decl fine\n")]);
    let mut engine = open(
        &dir,
        RulePlugin {
            severity: PluginSeverity::Warning,
            emit_undeclared: true,
            flood: 0,
        },
        false,
    );
    let result = check(&mut engine);
    assert!(
        !result.findings.iter().any(|f| f.category.contains("ghost")),
        "an emission under an undeclared rule must never become a finding"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("undeclared rule 'ghost-rule'")),
        "and the drop must be loud: {:?}",
        result.diagnostics
    );
}

#[test]
fn the_noise_ceiling_truncates_loudly() {
    let dir = project(&[("a.mock", "decl fine\n")]);
    let mut engine = open(
        &dir,
        RulePlugin {
            severity: PluginSeverity::Warning,
            emit_undeclared: false,
            flood: 502,
        },
        false,
    );
    let result = check(&mut engine);
    let plugin_findings = result
        .findings
        .iter()
        .filter(|f| f.category.starts_with("plugin:"))
        .count();
    assert_eq!(plugin_findings, 500, "capped at the ceiling");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("capped at 500")),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn baseline_and_inline_suppression_apply_uniformly() {
    // Baseline: an acknowledged plugin finding disappears from later runs like any finding.
    let dir = project(&[("a.mock", "decl flag_me\n")]);
    let mut engine = open(&dir, RulePlugin::warning(), false);
    assert_eq!(
        check(&mut engine)
            .findings
            .iter()
            .filter(|f| f.category.starts_with("plugin:"))
            .count(),
        1
    );
    engine.baseline(BaselineOp::Create);
    assert_eq!(
        check(&mut engine)
            .findings
            .iter()
            .filter(|f| f.category.starts_with("plugin:"))
            .count(),
        0,
        "baselined plugin findings are acknowledged exactly like core ones"
    );

    // Inline suppression: the namespaced category is a plain category string to kndo:allow.
    let dir = project(&[(
        "a.mock",
        "allow plugin:test-plugin/no-flag-symbols\ndecl flag_me\n",
    )]);
    let mut engine = open(&dir, RulePlugin::warning(), false);
    let result = check(&mut engine);
    assert_eq!(
        result
            .findings
            .iter()
            .filter(|f| f.category.starts_with("plugin:"))
            .count(),
        0,
        "suppressed: {:?}",
        result.findings
    );
    assert!(result.suppressed.inline >= 1);
}

#[test]
fn the_gate_opt_in_makes_findings_gate_eligible_and_caps_severity() {
    // Whole-plugin opt-in: advisory=false, severity capped at the configured level even
    // though the rule declares Error.
    let dir = project(&[("a.mock", "decl flag_me\n")]);
    std::fs::write(
        dir.path().join("kndo.toml"),
        "[plugins.gate]\n\"test-plugin\" = \"info\"\n",
    )
    .unwrap();
    let mut engine = open(
        &dir,
        RulePlugin {
            severity: PluginSeverity::Error,
            emit_undeclared: false,
            flood: 0,
        },
        false,
    );
    let f = check(&mut engine)
        .findings
        .into_iter()
        .find(|f| f.category.starts_with("plugin:"))
        .expect("finding present");
    assert!(!f.advisory, "gate opt-in makes it gate-eligible");
    assert_eq!(
        f.severity,
        Severity::Info,
        "config lowers declared Error to Info — cap, never raise"
    );

    // Per-rule "off" wins over the plugin-level opt-in.
    std::fs::write(
        dir.path().join("kndo.toml"),
        "[plugins.gate]\n\"test-plugin\" = \"warning\"\n\
         \"test-plugin/no-flag-symbols\" = \"off\"\n",
    )
    .unwrap();
    let mut engine = open(&dir, RulePlugin::warning(), false);
    let f = check(&mut engine)
        .findings
        .into_iter()
        .find(|f| f.category.starts_with("plugin:"))
        .expect("finding present");
    assert!(
        f.advisory,
        "per-rule off pins advisory under a broader opt-in"
    );
}

#[test]
fn the_finding_round_runs_on_the_warm_snapshot_path() {
    let dir = project(&[("a.mock", "decl flag_me\n")]);
    let count = |engine: &mut Engine| {
        check(engine)
            .findings
            .iter()
            .filter(|f| f.category.starts_with("plugin:"))
            .count()
    };
    // Two engines over the same cached project: the second run serves the graph from the
    // snapshot (a findings-only plugin never bypasses it) and must STILL produce the finding
    // — findings are output, recomputed every run, never persisted.
    let mut cold = open(&dir, RulePlugin::warning(), true);
    assert_eq!(count(&mut cold), 1);
    drop(cold);
    let mut warm = open(&dir, RulePlugin::warning(), true);
    let warm_result = check(&mut warm);
    assert!(warm_result.cache_hits > 0, "second run must be warm");
    assert_eq!(
        warm_result
            .findings
            .iter()
            .filter(|f| f.category.starts_with("plugin:"))
            .count(),
        1,
        "the finding round runs on the snapshot fast path too"
    );
}
