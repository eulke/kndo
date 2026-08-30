//! The containment model, end to end: a plugin can only assert graph facts, and
//! everything it asserts is either applied or dropped with a described line on its
//! contribution — misdirected targets, undeclared rules, a root from a plugin that
//! disclaimed graph mutation, a content budget that cut. Plugin findings ride the
//! same suppression pass as first-party ones.

use kndo::{
    Activation, Category, Confidence, Config, ContentView, GraphView, Plugin, PluginSeverity,
    PluginSink, PluginSpec, PluginTarget, ProjectPath, RootKind, RunMode, Session, Snapshot,
    Subject, Threads,
};
use kndo_testkit::{MockAdapter, TempProject};

/// A plugin that emits exactly what the test scripts — including things a correct
/// plugin never would, because the drops are the behavior under test.
struct ScriptedPlugin {
    spec: PluginSpec,
    mutates: bool,
    /// Emitted from `contribute_roots` (the sanctioned hook).
    contribute: Vec<(PluginTarget, RootKind, Confidence)>,
    /// Emitted from `report_findings` — the containment probe: a non-mutating
    /// plugin trying to smuggle a root through the shared sink.
    report_roots: Vec<(PluginTarget, RootKind, Confidence)>,
    findings: Vec<(&'static str, PluginTarget)>,
    /// Read every graph file through the content view, to drain its budget.
    read_everything: bool,
}

impl ScriptedPlugin {
    fn new(spec: PluginSpec, mutates: bool) -> Self {
        ScriptedPlugin {
            spec,
            mutates,
            contribute: Vec::new(),
            report_roots: Vec::new(),
            findings: Vec::new(),
            read_everything: false,
        }
    }
}

impl Plugin for ScriptedPlugin {
    fn spec(&self) -> &PluginSpec {
        &self.spec
    }
    fn mutates_graph(&self) -> bool {
        self.mutates
    }
    fn contribute_roots(
        &self,
        _graph: &GraphView<'_>,
        _content: &ContentView<'_>,
        out: &mut PluginSink,
    ) {
        for (target, kind, confidence) in &self.contribute {
            out.root(target.clone(), *kind, *confidence);
        }
    }
    fn report_findings(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut PluginSink,
    ) {
        for (target, kind, confidence) in &self.report_roots {
            out.root(target.clone(), *kind, *confidence);
        }
        if self.read_everything {
            for path in graph.paths() {
                let _ = content.read(path);
            }
        }
        for (rule, target) in &self.findings {
            out.finding(rule, PluginSeverity::Info, target.clone(), "scripted");
        }
    }
}

fn fixture() -> TempProject {
    let p = TempProject::new();
    p.file(
        "main.kmock",
        "root-file\nimport ./lib { helper }\ncall helper\nfn dead_one\n",
    );
    p.file("lib.kmock", "pub fn helper\n");
    p.file("orphan.kmock", "fn floats\n");
    p
}

fn analyze(p: &TempProject, plugins: Vec<Box<dyn Plugin>>) -> Snapshot {
    Session::open(
        p.root(),
        Config {
            threads: Threads::Auto,
            use_cache: false,
        },
        vec![Box::new(MockAdapter::new())],
    )
    .expect("open")
    .with_plugins(plugins)
    .analyze(RunMode::Full)
    .expect("analyze")
}

fn file(path: &str) -> PluginTarget {
    PluginTarget::File(ProjectPath::new(path))
}

fn symbol(path: &str, name: &str) -> PluginTarget {
    PluginTarget::Symbol {
        path: ProjectPath::new(path),
        name: name.into(),
    }
}

#[test]
fn misdirected_contributions_drop_with_described_lines() {
    let mut m = ScriptedPlugin::new(
        PluginSpec::builder("test:m", 1)
            .activation(Activation::Always)
            .rule("hello", "a declared rule")
            .build(),
        true,
    );
    m.contribute = vec![
        (
            file("missing.kmock"),
            RootKind::Production,
            Confidence::Certain,
        ),
        (
            symbol("main.kmock", "no_such_fn"),
            RootKind::Production,
            Confidence::Certain,
        ),
        // The one that lands: a DECLARATION-targeted anchor keeps `dead_one`.
        (
            symbol("main.kmock", "dead_one"),
            RootKind::Production,
            Confidence::Certain,
        ),
    ];
    m.findings = vec![
        ("ghost", file("main.kmock")),
        ("hello", file("missing.kmock")),
        ("hello", symbol("lib.kmock", "helper")),
    ];

    let snap = analyze(&fixture(), vec![Box::new(m)]);
    let contribution = &snap.plugins[0];
    assert_eq!(
        (contribution.roots, contribution.findings),
        (1, 1),
        "one root and one finding land; the rest drop: {contribution:#?}"
    );
    assert_eq!(
        contribution.dropped,
        [
            "root not applied: file missing.kmock does not resolve in the graph",
            "root not applied: symbol no_such_fn in main.kmock does not resolve in the graph",
            "finding under undeclared rule `ghost`",
            "finding not applied: file missing.kmock does not resolve in the graph",
        ]
    );
    assert!(!contribution.content_budget_cut);

    // The applied declaration root keeps `dead_one` — an outside anchor reaches
    // symbols, not only whole files.
    assert!(
        !snap
            .findings
            .iter()
            .any(|f| format!("{:?}", f.subject).contains("dead_one")),
        "the plugin-rooted declaration is no longer dead: {:#?}",
        snap.findings
    );

    // The applied finding carries the namespaced category and the real symbol
    // subject, at advisory confidence.
    let hello = snap
        .findings
        .iter()
        .find(|f| f.category.as_str() == "plugin:test:m/hello")
        .expect("the declared-rule finding lands");
    assert_eq!(hello.confidence, Confidence::Probable);
    assert!(matches!(&hello.subject, Subject::Symbol { path, .. } if path.as_str() == "lib.kmock"));
}

#[test]
fn a_non_mutating_plugin_cannot_smuggle_roots_through_the_sink() {
    let mut n = ScriptedPlugin::new(
        PluginSpec::builder("test:n", 1)
            .activation(Activation::Always)
            .build(),
        false,
    );
    n.report_roots = vec![(
        file("orphan.kmock"),
        RootKind::Production,
        Confidence::Certain,
    )];

    let snap = analyze(&fixture(), vec![Box::new(n)]);
    assert_eq!(
        snap.plugins[0].dropped,
        ["root refused: file orphan.kmock — the plugin declares mutates_graph() == false"]
    );
    assert_eq!(snap.plugins[0].roots, 0);
    assert!(
        snap.findings
            .iter()
            .any(|f| f.subject.path().as_str() == "orphan.kmock"
                && matches!(f.subject, Subject::File { .. })),
        "the refused root changed nothing: the orphan is still dead"
    );
}

#[test]
fn the_content_budget_cut_is_reported_on_the_contribution() {
    let p = fixture();
    // Past the file budget by construction; every read is charged once.
    for i in 0..kndo::CONTENT_MAX_FILES {
        p.file(&format!("bulk_{i:03}.kmock"), "fn filler\n");
    }
    let mut n = ScriptedPlugin::new(
        PluginSpec::builder("test:n", 1)
            .activation(Activation::Always)
            .requested_file_access(&["*.kmock"])
            .build(),
        false,
    );
    n.read_everything = true;

    let snap = analyze(&p, vec![Box::new(n)]);
    assert!(
        snap.plugins[0].content_budget_cut,
        "reading past the budget is visible on the contribution, never silent"
    );
}

#[test]
fn plugin_findings_ride_the_same_suppression_pass() {
    let p = fixture();
    p.file(
        "lib.kmock",
        "# kndo:allow-file plugin:test:m/hello\npub fn helper\n",
    );
    let mut m = ScriptedPlugin::new(
        PluginSpec::builder("test:m", 1)
            .activation(Activation::Always)
            .rule("hello", "a declared rule")
            .build(),
        false,
    );
    m.findings = vec![("hello", symbol("lib.kmock", "helper"))];

    let snap = analyze(&p, vec![Box::new(m)]);
    assert!(
        !snap
            .findings
            .iter()
            .any(|f| f.category.as_str() == "plugin:test:m/hello"),
        "the pragma suppressed the plugin finding"
    );
    let category = Category::parse("plugin:test:m/hello").expect("namespaced category parses");
    assert!(
        snap.suppressed
            .by_category
            .iter()
            .any(|(c, n)| *c == category && *n == 1),
        "the suppression is counted under the namespaced category: {:#?}",
        snap.suppressed
    );

    // Without the plugin the pragma suppresses nothing — and still is not STALE:
    // an un-judged category (nothing ships or runs for it) makes its allows
    // un-judgeable, the same flicker rule first-party abstentions get.
    let without = analyze(&p, Vec::new());
    assert!(
        !without
            .findings
            .iter()
            .any(|f| f.category.as_str() == "stale"),
        "an allow over an un-judged plugin category is never called stale: {:#?}",
        without.findings
    );
}
