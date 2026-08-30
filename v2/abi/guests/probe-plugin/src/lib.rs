//! The reference external plugin. It exercises the whole plugin world from the
//! guest side: rule-based activation, a contributed root (it declares
//! `mutates-graph` true), findings under a declared rule — one of them built from
//! a scoped `read-file` probe — plus two deliberate misbehaviors (an undeclared
//! rule, a missing target) whose DROPS the host must report on the contribution:
//! the containment model, observed from outside the process.

use kndo_sdk::plugin::{
    Guest, export, graph_contains, graph_paths, read_file,
};
use kndo_sdk::wire::{
    Activation, ActivationRule, Confidence, ContributedFinding, ContributedRoot, PluginSeverity,
    PluginSpec, PluginTarget, RootKind, RuleDescriptor,
};

struct ProbePlugin;

impl Guest for ProbePlugin {
    fn spec() -> PluginSpec {
        PluginSpec {
            coordinate: "demo:probe".to_string(),
            version: 1,
            activation: Activation::AnyRule(vec![ActivationRule::FileExists(
                "*.kmini".to_string(),
            )]),
            dependencies: Vec::new(),
            requested_file_access: vec!["config.probe".to_string()],
            rules: vec![RuleDescriptor {
                name: "note".to_string(),
                description: "reports what the probe observed".to_string(),
            }],
        }
    }

    fn mutates_graph() -> bool {
        true
    }

    fn contribute_roots() -> Vec<ContributedRoot> {
        let mut roots = Vec::new();
        // Anchor liveness the language cannot see, when the target exists.
        if graph_contains(&"wired.kmini".to_string()) {
            roots.push(ContributedRoot {
                target: PluginTarget::File("wired.kmini".to_string()),
                kind: RootKind::Production,
                confidence: Confidence::Certain,
            });
        }
        // A root at a file no graph holds — the host must drop it, described.
        roots.push(ContributedRoot {
            target: PluginTarget::File("nowhere.kmini".to_string()),
            kind: RootKind::Production,
            confidence: Confidence::Certain,
        });
        roots
    }

    fn report_findings() -> Vec<ContributedFinding> {
        let first = graph_paths().into_iter().next().unwrap_or_default();
        let seen = match read_file(&"config.probe".to_string()) {
            Some(bytes) => format!("config.probe is {} bytes", bytes.len()),
            None => "config.probe unreadable".to_string(),
        };
        vec![
            ContributedFinding {
                rule: "note".to_string(),
                severity: PluginSeverity::Info,
                target: PluginTarget::File(first),
                message: seen,
            },
            // Under a rule the spec never declared — the host must drop it.
            ContributedFinding {
                rule: "ghost".to_string(),
                severity: PluginSeverity::Info,
                target: PluginTarget::File("wired.kmini".to_string()),
                message: "never lands".to_string(),
            },
        ]
    }
}

export!(ProbePlugin with_types_in kndo_sdk::plugin);
