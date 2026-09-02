//! Cobertura XML, as coverage.py writes it (and istanbul's cobertura reporter):
//! `<class filename="…">` holding `<line number="…" hits="…"/>` elements. The
//! `filename` is the report's own spelling, relative to a `<source>` root the
//! report records apart — the engine's mapping finds the project file it names.
//! `<method>` elements carry no declaration line, so this format states line
//! records only.

use crate::merge_record;
use kndo_contract::evidence::{CoverageRecords, FileRecords};
use kndo_contract::extension::{Activation, Extension, ExtensionSpec, MutatesGraph};
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub fn parse_cobertura_records(text: &str) -> Option<CoverageRecords> {
    let doc = roxmltree::Document::parse_with_options(text, crate::xml_options()).ok()?;
    if doc.root_element().tag_name().name() != "coverage" {
        return None;
    }
    let mut files: BTreeMap<ProjectPath, FileRecords> = BTreeMap::new();
    for class in doc.descendants().filter(|n| n.has_tag_name("class")) {
        let Some(filename) = class.attribute("filename") else {
            continue;
        };
        let filename = filename.replace('\\', "/");
        let filename = filename.trim_start_matches("./");
        if filename.is_empty() {
            continue;
        }
        let mut rec = FileRecords::default();
        for line in class.descendants().filter(|n| n.has_tag_name("line")) {
            if let (Some(Ok(number)), Some(Ok(hits))) = (
                line.attribute("number").map(str::parse::<u32>),
                line.attribute("hits").map(str::parse::<u64>),
            ) {
                *rec.lines.entry(number).or_insert(0) += hits;
            }
        }
        if !rec.lines.is_empty() {
            merge_record(&mut files, ProjectPath::new(filename), rec);
        }
    }
    (!files.is_empty()).then_some(CoverageRecords { files })
}

static SPEC: LazyLock<ExtensionSpec> = LazyLock::new(|| {
    ExtensionSpec::builder("kndo:coverage-cobertura", 1)
        .conduct(Activation::Always, MutatesGraph::No)
        .reads_reports(&[
            "coverage.xml",
            "cobertura.xml",
            "coverage/cobertura-coverage.xml",
        ])
        .build()
});

/// The built-in Cobertura ingester; the engine reads the report, this turns
/// bytes into records.
pub struct CoberturaPlugin;

impl Extension for CoberturaPlugin {
    fn spec(&self) -> &ExtensionSpec {
        &SPEC
    }

    fn ingest(&self, _report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        parse_cobertura_records(std::str::from_utf8(content).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// coverage.py's shape: a DOCTYPE-less document with a `<sources>` root the
    /// filenames are relative to, and one `<class>` per file.
    #[test]
    fn class_filenames_carry_their_line_hits() {
        let records = parse_cobertura_records(
            r#"<?xml version="1.0" ?>
<coverage version="7.16.0" line-rate="0.5">
 <sources><source>/abs/project/src</source></sources>
 <packages><package name="flask"><classes>
  <class name="app.py" filename="flask/app.py" line-rate="0.5"><methods/>
   <lines><line number="1" hits="1"/><line number="2" hits="0" branch="true" condition-coverage="50% (1/2)"/></lines>
  </class>
 </classes></package></packages>
</coverage>"#,
        )
        .expect("parses");
        let app = &records.files[&ProjectPath::new("flask/app.py")];
        assert_eq!(app.lines.get(&1), Some(&1));
        assert_eq!(app.lines.get(&2), Some(&0));
        assert!(
            app.functions.is_empty(),
            "no declaration lines in this format"
        );
    }

    #[test]
    fn a_foreign_or_broken_document_is_absence() {
        assert!(parse_cobertura_records("<report/>").is_none());
        assert!(parse_cobertura_records("<coverage>").is_none());
        assert!(parse_cobertura_records("").is_none());
    }
}
