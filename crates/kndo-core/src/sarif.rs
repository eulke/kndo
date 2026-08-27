//! `--format sarif` — SARIF 2.1.0 rendering of a
//! [`RunResult`], for GitHub code scanning and every other SARIF consumer. Renders
//! **core-side** like JSON and the agent format: every frontend emits
//! byte-identical SARIF.
//!
//! The mapping, per the contract: `category` → `rule.id`; `severity` → SARIF `level`
//! (error/warning → themselves, info → `note`); the `related` evidence chain →
//! `relatedLocations`; `confidence` → `properties.confidence`. One `run` object per kndo run.
//! The stable finding id travels as `partialFingerprints.kndoFindingId` — exactly what SARIF
//! fingerprints exist for (result matching across runs), and kndo's ids are already
//! line-number-free by design. Paths are project-relative with `/`
//! separators — SARIF's preferred artifact form, and what code-scanning UIs resolve against
//! the repository root.
//!
//! Diff modes render only the current (`new`) findings — SARIF models "the results of this
//! run", and a `fixed` finding is by definition not a result of this run; the JSON envelope
//! remains the format that carries the full delta.

use crate::engine::{Finding, RunResult, Severity, KNDO_VERSION};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Sarif {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<Run>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Run {
    tool: Tool,
    results: Vec<SarifResult>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Tool {
    driver: Driver,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Driver {
    name: &'static str,
    version: &'static str,
    information_uri: &'static str,
    rules: Vec<Rule>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Rule {
    id: String,
    short_description: Message,
    properties: RuleProperties,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RuleProperties {
    /// kndo's taxonomy group (defect | waste | risk | hygiene) — a stable coarse filter for
    /// SARIF consumers, same role it plays in the JSON envelope.
    group: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    text: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SarifResult {
    rule_id: String,
    rule_index: usize,
    level: &'static str,
    message: Message,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    locations: Vec<SarifLocation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    related_locations: Vec<SarifLocation>,
    partial_fingerprints: Fingerprints,
    properties: ResultProperties,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Fingerprints {
    kndo_finding_id: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultProperties {
    confidence: crate::vocab::Confidence,
    subject_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SarifLocation {
    physical_location: PhysicalLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<Message>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PhysicalLocation {
    artifact_location: ArtifactLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<Region>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactLocation {
    uri: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Region {
    start_line: u32,
    start_column: u32,
    end_line: u32,
    /// kndo spans are end-exclusive (contracts the Span) and so is SARIF's `endColumn`
    /// ("the column number of the character following the end of the region") — a direct map.
    end_column: u32,
}

fn level(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "note",
    }
}

fn location_of(finding: &Finding) -> Vec<SarifLocation> {
    let Some(path) = &finding.location.path else {
        return Vec::new(); // a location-less finding (e.g. duplicate file groups) omits it
    };
    vec![SarifLocation {
        physical_location: PhysicalLocation {
            artifact_location: ArtifactLocation {
                uri: path.0.to_string(),
            },
            region: finding.location.range.map(|r| Region {
                start_line: r.start.0,
                start_column: r.start.1,
                end_line: r.end.0,
                end_column: r.end.1,
            }),
        },
        message: None,
    }]
}

pub fn render(result: &RunResult) -> String {
    // Rules: one per distinct category, in first-appearance order (findings are already
    // id-sorted, so this is deterministic).
    let mut rules: Vec<Rule> = Vec::new();
    let mut rule_index_of = std::collections::HashMap::new();
    for f in &result.findings {
        if !rule_index_of.contains_key(f.category.as_str()) {
            rule_index_of.insert(f.category.as_str(), rules.len());
            rules.push(Rule {
                id: f.category.to_string(),
                short_description: Message {
                    text: format!("kndo {} finding", f.category),
                },
                properties: RuleProperties {
                    group: f.group.to_string(),
                },
            });
        }
    }

    let results: Vec<SarifResult> = result
        .findings
        .iter()
        .map(|f| SarifResult {
            rule_id: f.category.to_string(),
            rule_index: rule_index_of[f.category.as_str()],
            level: level(f.severity),
            message: Message {
                text: f.message.clone(),
            },
            locations: location_of(f),
            related_locations: f
                .related
                .iter()
                .map(|r| SarifLocation {
                    physical_location: PhysicalLocation {
                        artifact_location: ArtifactLocation {
                            uri: r.path.0.to_string(),
                        },
                        region: r.range.map(|s| Region {
                            start_line: s.start.0,
                            start_column: s.start.1,
                            end_line: s.end.0,
                            end_column: s.end.1,
                        }),
                    },
                    message: r.note.as_ref().map(|n| Message {
                        text: format!("[{}] {n}", r.role),
                    }),
                })
                .collect(),
            partial_fingerprints: Fingerprints {
                kndo_finding_id: f.id.clone(),
            },
            properties: ResultProperties {
                confidence: f.confidence,
                subject_kind: f.subject_kind.to_string(),
                symbol: f.location.symbol.clone(),
                package: f.location.package.clone(),
            },
        })
        .collect();

    let sarif = Sarif {
        schema: "https://json.schemastore.org/sarif-2.1.0.json",
        version: "2.1.0",
        runs: vec![Run {
            tool: Tool {
                driver: Driver {
                    name: "kndo",
                    version: KNDO_VERSION,
                    information_uri: "https://github.com/eulke/kondo",
                    rules,
                },
            },
            results,
        }],
    };
    serde_json::to_string_pretty(&sarif)
        .unwrap_or_else(|e| format!("{{\"error\": \"failed to serialize SARIF: {e}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ProjectPath, Span};
    use crate::engine::{Location, RelatedLocation, SuppressedSummary};
    use crate::vocab::Confidence;
    use smol_str::SmolStr;

    fn finding(category: &str, severity: Severity, path: Option<&str>) -> Finding {
        Finding {
            advisory: false,
            id: format!("kndo-{category}-x"),
            category: crate::vocab::Category::new(category),
            group: crate::vocab::Group::Waste,
            subject_kind: crate::vocab::SubjectKind::new("function"),
            severity,
            confidence: Confidence::Certain,
            message: format!("a {category} finding"),
            location: Location {
                path: path.map(|p| ProjectPath(SmolStr::new(p))),
                range: Some(Span {
                    start: (3, 1),
                    end: (7, 2),
                }),
                symbol: Some("thing".to_string()),
                package: None,
            },
            related: vec![RelatedLocation {
                role: "cycle-hop".to_string(),
                path: ProjectPath(SmolStr::new("b.ts")),
                range: None,
                note: Some("imports a.ts".to_string()),
            }],
            rolled_up: None,
            delta: None,
            delta_origin: None,
        }
    }

    fn run_result(findings: Vec<Finding>) -> RunResult {
        RunResult {
            findings,
            fixed: Vec::new(),
            diagnostics: Vec::new(),
            abstained: Vec::new(),
            files_discovered: 0,
            files_claimed: 0,
            symbols: 0,
            dependencies: 0,
            edges: 0,
            mode: "full".to_string(),
            base_ref: None,
            started_at: "1970-01-01T00:00:00Z".to_string(),
            duration_ms: 1,
            project_root: "/tmp/x".to_string(),
            adapters: Vec::new(),
            plugins: Vec::new(),
            budget: None,
            cache_enabled: false,
            cache_hits: 0,
            baseline: None,
            suppressed: SuppressedSummary::default(),
            health: None,
            timings: Vec::new(),
            plugin_contributions: Vec::new(),
        }
    }

    #[test]
    fn renders_valid_sarif_shape_with_the_contract_mapping() {
        let result = run_result(vec![
            finding("unused", Severity::Warning, Some("a.ts")),
            finding("cyclic", Severity::Info, Some("a.ts")),
            finding("unused", Severity::Warning, Some("c.ts")),
        ]);
        let text = render(&result);
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["version"], "2.1.0");
        let run = &v["runs"][0];
        assert_eq!(run["tool"]["driver"]["name"], "kndo");
        // One rule per distinct category, results referencing by index.
        let rules = run["tool"]["driver"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        let results = run["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["ruleId"], "unused");
        assert_eq!(results[0]["level"], "warning");
        assert_eq!(results[1]["level"], "note", "info maps to note");
        assert_eq!(
            results[0]["partialFingerprints"]["kndoFindingId"],
            "kndo-unused-x"
        );
        assert_eq!(results[0]["properties"]["confidence"], "certain");
        // The related chain lands in relatedLocations with role-prefixed messages.
        assert_eq!(
            results[0]["relatedLocations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "b.ts"
        );
        assert_eq!(
            results[0]["relatedLocations"][0]["message"]["text"],
            "[cycle-hop] imports a.ts"
        );
        // Region maps kndo's 1-indexed, end-exclusive span directly.
        let region = &results[0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 3);
        assert_eq!(region["endColumn"], 2);
    }

    #[test]
    fn location_less_findings_omit_locations_rather_than_fabricate() {
        let mut f = finding("duplicate", Severity::Info, None);
        f.related = Vec::new();
        let text = render(&run_result(vec![f]));
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(v["runs"][0]["results"][0].get("locations").is_none());
    }

    #[test]
    fn a_clean_run_is_valid_sarif_with_zero_results() {
        let text = render(&run_result(Vec::new()));
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["runs"][0]["results"].as_array().unwrap().len(), 0);
        assert_eq!(
            v["runs"][0]["tool"]["driver"]["rules"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
}
