//! SARIF 2.1.0 — the interchange rendering for code-scanning consumers. The
//! mapping is the contract's own vocabulary, projected: `category` → `rule.id`
//! (one rule per distinct category, first-appearance order over the already-sorted
//! findings); `severity` → `level` (SARIF has no `info`, so it becomes `note`);
//! the finding id → `partialFingerprints.kndoFindingId` — result matching across
//! runs is exactly what SARIF fingerprints are for, and kndo ids are span-free by
//! design; every subject's path → `artifactLocation.uri` (project-relative, `/`
//! separators — every [`Subject`] anchors to a path, so no location is ever
//! omitted or fabricated). Spans are byte offsets in this contract, so regions
//! use SARIF's binary form (`byteOffset`/`byteLength`) — never an invented line.
//!
//! This renders THE REPORT's findings: with a baseline in play those are the new
//! ones, and SARIF consumers that manage their own alert lifecycle (closing
//! alerts absent from an upload) want the full set — upload from a baseline-free
//! run. `properties` carries `confidence` and the subject vocabulary as serde
//! writes them, so the spelling cannot drift from the envelope's.

use crate::report::Report;
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::Subject;
use kndo_contract::vocab::{Confidence, SubjectKind};

impl Report {
    /// The SARIF rendering: a JSON value (not newline-terminated — the frontend
    /// frames it), a pure projection of the report.
    pub fn to_sarif(&self) -> String {
        let mut rules: Vec<Rule> = Vec::new();
        let mut rule_index_of = std::collections::HashMap::new();
        for f in &self.findings {
            if !rule_index_of.contains_key(f.category.as_str()) {
                rule_index_of.insert(f.category.as_str(), rules.len());
                rules.push(Rule {
                    id: f.category.as_str().to_string(),
                    short_description: Message {
                        text: format!("kndo {} finding", f.category.as_str()),
                    },
                });
            }
        }
        let results = self
            .findings
            .iter()
            .map(|f| SarifResult {
                rule_id: f.category.as_str().to_string(),
                rule_index: rule_index_of[f.category.as_str()],
                level: level(f.severity),
                message: Message {
                    text: f.message.clone(),
                },
                locations: vec![location_of(f)],
                partial_fingerprints: Fingerprints {
                    kndo_finding_id: f.id.as_str().to_string(),
                },
                properties: ResultProperties {
                    confidence: f.confidence,
                    subject_kind: f.subject.kind(),
                    symbol: match &f.subject {
                        Subject::Symbol { selector, .. } => Some(selector.render()),
                        _ => None,
                    },
                    name: match &f.subject {
                        Subject::Package { name, .. } | Subject::Dependency { name, .. } => {
                            Some(name.to_string())
                        }
                        _ => None,
                    },
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
                        version: env!("CARGO_PKG_VERSION"),
                        information_uri: "https://github.com/eulke/kondo",
                        rules,
                    },
                },
                results,
            }],
        };
        serde_json::to_string_pretty(&sarif).expect("sarif serializes")
    }
}

/// SARIF's level vocabulary has no `info`; its own third member is `note`.
fn level(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "note",
    }
}

fn location_of(finding: &Finding) -> Location {
    let region = match &finding.subject {
        Subject::Symbol { span, .. } | Subject::Suppression { span, .. } => Some(Region {
            byte_offset: span.start,
            byte_length: span.end - span.start,
        }),
        _ => None,
    };
    Location {
        physical_location: PhysicalLocation {
            artifact_location: ArtifactLocation {
                uri: finding.subject.path().as_str().to_string(),
            },
            region,
        },
    }
}

#[derive(serde::Serialize)]
struct Sarif {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<Run>,
}

#[derive(serde::Serialize)]
struct Run {
    tool: Tool,
    results: Vec<SarifResult>,
}

#[derive(serde::Serialize)]
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
}

#[derive(serde::Serialize)]
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
    locations: Vec<Location>,
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
    confidence: Confidence,
    subject_kind: SubjectKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Location {
    physical_location: PhysicalLocation,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PhysicalLocation {
    artifact_location: ArtifactLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<Region>,
}

#[derive(serde::Serialize)]
struct ArtifactLocation {
    uri: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Region {
    byte_offset: u32,
    byte_length: u32,
}

#[cfg(test)]
mod tests {
    use crate::report::{REPORT_SCHEMA, Report, RunInfo};
    use crate::suppress::SuppressedSummary;
    use kndo_contract::finding::{Finding, Severity};
    use kndo_contract::subject::{Subject, SymbolSelector};
    use kndo_contract::vocab::{Category, Confidence, ProjectPath, Span};
    use smol_str::SmolStr;

    fn report(findings: Vec<Finding>) -> Report {
        Report {
            run: RunInfo {
                schema: REPORT_SCHEMA,
                files_discovered: 0,
                files_claimed: 0,
                extensions: Vec::new(),
            },
            findings,
            fixed: Vec::new(),
            baselined: 0,
            abstained: Vec::new(),
            suppressed: SuppressedSummary::default(),
            plugins: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn finding(category: Category, severity: Severity, subject: Subject) -> Finding {
        Finding::new(category, severity, Confidence::Certain, subject, "", "msg")
    }

    #[test]
    fn the_contract_mapping_holds() {
        let symbol = Subject::Symbol {
            path: ProjectPath::new("src/a.py"),
            selector: SymbolSelector::Member {
                owner: SmolStr::new("Store"),
                name: SmolStr::new("_drop"),
            },
            span: Span::new(120, 180),
        };
        let dependency = Subject::Dependency {
            owner_manifest: ProjectPath::new("package.json"),
            name: SmolStr::new("left-pad"),
        };
        let r = report(vec![
            finding(Category::UNUSED, Severity::Warning, symbol.clone()),
            finding(Category::INTERNAL_ONLY, Severity::Info, symbol),
            finding(Category::UNUSED, Severity::Warning, dependency),
        ]);
        let v: serde_json::Value = serde_json::from_str(&r.to_sarif()).unwrap();

        assert_eq!(v["version"], "2.1.0");
        let run = &v["runs"][0];
        assert_eq!(run["tool"]["driver"]["name"], "kndo");
        // One rule per distinct category; results reference by index.
        let rules = run["tool"]["driver"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        let results = run["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["ruleId"], "unused");
        assert_eq!(results[2]["ruleIndex"], 0, "same category, same rule");
        assert_eq!(results[1]["level"], "note", "info speaks SARIF's note");

        // Byte spans project to SARIF's binary region — never an invented line.
        let region = &results[0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["byteOffset"], 120);
        assert_eq!(region["byteLength"], 60);
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/a.py"
        );
        // A span-less subject still locates — every subject has a path.
        let dep_loc = &results[2]["locations"][0]["physicalLocation"];
        assert_eq!(dep_loc["artifactLocation"]["uri"], "package.json");
        assert!(dep_loc.get("region").is_none());

        // Identity travels as the fingerprint; vocabulary as serde spells it.
        assert_eq!(
            results[0]["partialFingerprints"]["kndoFindingId"],
            r.findings[0].id.as_str()
        );
        assert_eq!(results[0]["properties"]["confidence"], "certain");
        assert_eq!(results[0]["properties"]["subjectKind"], "symbol");
        assert_eq!(results[0]["properties"]["symbol"], "Store._drop");
        assert_eq!(results[2]["properties"]["name"], "left-pad");
        assert!(results[2]["properties"].get("symbol").is_none());
    }

    #[test]
    fn a_clean_report_is_valid_sarif_with_zero_results() {
        let v: serde_json::Value = serde_json::from_str(&report(Vec::new()).to_sarif()).unwrap();
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
