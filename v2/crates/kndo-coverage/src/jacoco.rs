//! JaCoCo XML, as the jacoco Maven and Gradle plugins write it: a `<package
//! name>` holds `<sourcefile name>` elements with `<line nr ci>` (covered
//! instructions — executed exactly when `ci > 0`) and `<class sourcefilename>`
//! elements whose `<method line>` carry a `METHOD` counter — the function
//! record, the primary evidence. The report's spelling is `package/Source.java`;
//! the engine's mapping finds the project file under its source root.

use crate::merge_record;
use kndo_contract::evidence::{CoverageRecords, FileRecords};
use kndo_contract::extension::{Activation, Extension, ExtensionSpec, MutatesGraph};
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub fn parse_jacoco_records(text: &str) -> Option<CoverageRecords> {
    let doc = roxmltree::Document::parse_with_options(text, crate::xml_options()).ok()?;
    if doc.root_element().tag_name().name() != "report" {
        return None;
    }
    let mut files: BTreeMap<ProjectPath, FileRecords> = BTreeMap::new();
    for package in doc.descendants().filter(|n| n.has_tag_name("package")) {
        let pkg = package.attribute("name").unwrap_or("");
        let key = |source: &str| {
            if pkg.is_empty() {
                source.to_string()
            } else {
                format!("{pkg}/{source}")
            }
        };
        // Method records first, by the source file each class was compiled from.
        let mut functions: BTreeMap<String, Vec<(u32, u64)>> = BTreeMap::new();
        for class in package.children().filter(|n| n.has_tag_name("class")) {
            let Some(source) = class.attribute("sourcefilename") else {
                continue;
            };
            for method in class.children().filter(|n| n.has_tag_name("method")) {
                let line = method.attribute("line").and_then(|l| l.parse::<u32>().ok());
                let covered = method
                    .children()
                    .find(|c| c.has_tag_name("counter") && c.attribute("type") == Some("METHOD"))
                    .and_then(|c| c.attribute("covered"))
                    .and_then(|v| v.parse::<u64>().ok());
                if let (Some(line), Some(covered)) = (line, covered) {
                    functions
                        .entry(key(source))
                        .or_default()
                        .push((line, covered));
                }
            }
        }
        for sourcefile in package.children().filter(|n| n.has_tag_name("sourcefile")) {
            let Some(source) = sourcefile.attribute("name") else {
                continue;
            };
            let path = key(source);
            let mut rec = FileRecords {
                lines: BTreeMap::new(),
                functions: functions.remove(&path).unwrap_or_default(),
            };
            for line in sourcefile.children().filter(|n| n.has_tag_name("line")) {
                if let (Some(Ok(nr)), Some(Ok(ci))) = (
                    line.attribute("nr").map(str::parse::<u32>),
                    line.attribute("ci").map(str::parse::<u64>),
                ) {
                    *rec.lines.entry(nr).or_insert(0) += ci;
                }
            }
            if !rec.lines.is_empty() || !rec.functions.is_empty() {
                merge_record(&mut files, ProjectPath::new(path), rec);
            }
        }
    }
    (!files.is_empty()).then_some(CoverageRecords { files })
}

static SPEC: LazyLock<ExtensionSpec> = LazyLock::new(|| {
    ExtensionSpec::builder("kndo:coverage-jacoco", 1)
        .conduct(Activation::Always, MutatesGraph::No)
        .reads_reports(&[
            "target/site/jacoco/jacoco.xml",
            "build/reports/jacoco/test/jacocoTestReport.xml",
            "jacoco.xml",
        ])
        .build()
});

/// The built-in JaCoCo ingester; the engine reads the report, this turns bytes
/// into records.
pub struct JacocoPlugin;

impl Extension for JacocoPlugin {
    fn spec(&self) -> &ExtensionSpec {
        &SPEC
    }

    fn ingest(&self, _report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        parse_jacoco_records(std::str::from_utf8(content).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The jacoco Maven plugin's shape, DOCTYPE included — every report it
    /// writes declares one, and a parser that refused it would ingest nothing
    /// in the field while a DOCTYPE-less fixture passed.
    #[test]
    fn method_counters_are_function_records_and_ci_is_the_line_hit() {
        let records = parse_jacoco_records(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><!DOCTYPE report PUBLIC "-//JACOCO//DTD Report 1.1//EN" "report.dtd"><report name="demo"><package name="demo"><class name="demo/Classify" sourcefilename="Classify.java"><method name="grade" desc="(I)Ljava/lang/String;" line="7"><counter type="LINE" missed="3" covered="4"/><counter type="METHOD" missed="0" covered="1"/></method><method name="neverRan" desc="(I)I" line="22"><counter type="METHOD" missed="1" covered="0"/></method></class><sourcefile name="Classify.java"><line nr="7" mi="0" ci="3"/><line nr="11" mi="3" ci="0"/><line nr="22" mi="4" ci="0"/></sourcefile></package></report>"#,
        )
        .expect("parses");
        let file = &records.files[&ProjectPath::new("demo/Classify.java")];
        assert_eq!(file.functions, [(7, 1), (22, 0)]);
        assert_eq!(file.lines.get(&7), Some(&3));
        assert_eq!(file.lines.get(&11), Some(&0));
    }

    #[test]
    fn the_default_package_keys_by_source_name_alone() {
        let records = parse_jacoco_records(
            r#"<report name="x"><package name=""><sourcefile name="Main.java"><line nr="1" mi="0" ci="1"/></sourcefile></package></report>"#,
        )
        .expect("parses");
        assert!(records.files.contains_key(&ProjectPath::new("Main.java")));
        assert!(parse_jacoco_records("<coverage/>").is_none());
    }
}
