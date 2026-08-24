//! The gate opt-in: `[plugins.gate]` in `kndo.toml`. The file itself is parsed once by
//! [`crate::config`]; this module owns the gate table's vocabulary and semantics, which
//! affect only how plugin findings map onto the severity channel — never the graph, core
//! findings, or any cached artifact.
//!
//! ```toml
//! [plugins.gate]
//! "github.com/acme/kndo-deprecations" = "warning"        # whole plugin: gate, capped at warning
//! "github.com/acme/kndo-deprecations/v1-api" = "off"     # per-rule override wins
//! ```
//!
//! Semantics: no entry → the finding is advisory (visible, attributed,
//! inert to exit codes). An entry gates the findings it covers at `min(declared severity,
//! configured level)` — config can lower a rule's declared severity, never raise it. `"off"`
//! pins advisory explicitly (useful under a broader plugin-level opt-in). Reading is
//! tolerant: a missing file is empty config; a malformed file or unknown level value is
//! reported as a problem string (surfaced as a run diagnostic) and otherwise ignored.

use std::collections::BTreeMap;

use crate::engine::Severity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateLevel {
    Off,
    At(Severity),
}

#[derive(Debug, Default)]
pub(crate) struct PluginsGate {
    entries: BTreeMap<String, GateLevel>,
}

impl PluginsGate {
    /// Parse the `[plugins.gate]` value out of an already-parsed `kndo.toml`
    /// ([`crate::config::KndoConfig`] owns the single file read). `None` (table absent) is
    /// empty config; problems (unknown level, wrong value type) come back as strings —
    /// never a failure.
    pub(crate) fn from_table(gate: Option<&toml::Value>) -> (PluginsGate, Vec<String>) {
        match gate.and_then(|g| g.as_table()) {
            Some(entries) => parse_entries(entries),
            None => (PluginsGate::default(), Vec::new()),
        }
    }

    /// The effective gate for one finding: the per-rule key (`<coordinate>/<rule>`) wins over
    /// the whole-plugin key (`<coordinate>`); no key → `None` (advisory).
    pub(crate) fn resolve(&self, plugin_id: &str, rule: &str) -> Option<GateLevel> {
        self.entries
            .get(&format!("{plugin_id}/{rule}"))
            .or_else(|| self.entries.get(plugin_id))
            .copied()
    }
}

fn parse_entries(entries: &toml::Table) -> (PluginsGate, Vec<String>) {
    let mut gate = PluginsGate::default();
    let mut problems = Vec::new();
    for (key, value) in entries {
        match value.as_str().and_then(parse_level) {
            Some(level) => {
                gate.entries.insert(key.clone(), level);
            }
            None => problems.push(format!(
                "kndo.toml [plugins.gate] \"{key}\" = {value}: expected \"off\", \
                 \"error\", \"warning\", or \"info\" — entry ignored"
            )),
        }
    }
    (gate, problems)
}

/// The level vocabulary as a table (the enum-mirror shape the WIT conversion tables use).
const LEVELS: &[(&str, GateLevel)] = &[
    ("off", GateLevel::Off),
    ("error", GateLevel::At(Severity::Error)),
    ("warning", GateLevel::At(Severity::Warning)),
    ("info", GateLevel::At(Severity::Info)),
];

fn parse_level(raw: &str) -> Option<GateLevel> {
    LEVELS
        .iter()
        .find(|(name, _)| *name == raw)
        .map(|(_, l)| *l)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate_from(toml_body: &str) -> (PluginsGate, Vec<String>) {
        let table: toml::Table = toml_body.parse().unwrap();
        PluginsGate::from_table(table.get("plugins").and_then(|p| p.get("gate")))
    }

    #[test]
    fn absent_table_is_empty_config() {
        let (gate, problems) = PluginsGate::from_table(None);
        assert!(gate.resolve("x", "y").is_none() && problems.is_empty());
        let (gate, problems) = gate_from("[analysis]\n");
        assert!(gate.resolve("x", "y").is_none() && problems.is_empty());
    }

    #[test]
    fn rule_key_wins_over_plugin_key_and_levels_parse() {
        let (gate, problems) = gate_from(
            "[plugins.gate]\n\
             \"github.com/a/p\" = \"warning\"\n\
             \"github.com/a/p/quiet-rule\" = \"off\"\n",
        );
        assert!(problems.is_empty());
        assert_eq!(
            gate.resolve("github.com/a/p", "loud-rule"),
            Some(GateLevel::At(Severity::Warning))
        );
        assert_eq!(
            gate.resolve("github.com/a/p", "quiet-rule"),
            Some(GateLevel::Off)
        );
        assert_eq!(gate.resolve("github.com/other", "any"), None);
    }

    #[test]
    fn malformed_values_are_reported_not_fatal() {
        // A malformed FILE is config.rs's problem now (single parse); malformed values
        // within the table stay this module's.
        let (gate, problems) = gate_from("[plugins.gate]\n\"a\" = \"loud\"\n\"b\" = 3\n");
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(gate.resolve("a", "r").is_none());
    }
}
