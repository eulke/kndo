//! Go's coverprofile, as `go test -coverprofile` writes it: a `mode:` header,
//! then one block per line — `file:startLine.col,endLine.col statements count`
//! — keyed by import path (`github.com/x/y/render.go`); the engine's mapping
//! finds the project file under the module. Every line of a block takes the
//! block's count, and blocks sharing a line accumulate; there are no function
//! records in this format.

use crate::merge_record;
use kndo_contract::evidence::{CoverageRecords, FileRecords};
use kndo_contract::extension::{Activation, Extension, ExtensionSpec, MutatesGraph};
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub fn parse_gocover_records(text: &str) -> Option<CoverageRecords> {
    let mut lines = text.lines();
    if !lines
        .next()
        .is_some_and(|first| first.trim().starts_with("mode:"))
    {
        return None;
    }
    let mut files: BTreeMap<ProjectPath, FileRecords> = BTreeMap::new();
    for line in lines {
        let line = line.trim();
        let Some((file, rest)) = line.rsplit_once(':') else {
            continue;
        };
        let mut fields = rest.split_whitespace();
        let (Some(span), Some(_statements), Some(count)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let line_of = |position: &str| {
            position
                .split_once('.')
                .and_then(|(l, _column)| l.parse::<u32>().ok())
        };
        let (Some((start, end)), Ok(count)) = (span.split_once(','), count.parse::<u64>()) else {
            continue;
        };
        let (Some(start), Some(end)) = (line_of(start), line_of(end)) else {
            continue;
        };
        let mut rec = FileRecords::default();
        for l in start..=end.max(start) {
            rec.lines.insert(l, count);
        }
        merge_record(&mut files, ProjectPath::new(file.replace('\\', "/")), rec);
    }
    (!files.is_empty()).then_some(CoverageRecords { files })
}

static SPEC: LazyLock<ExtensionSpec> = LazyLock::new(|| {
    ExtensionSpec::builder("kndo:coverage-go", 1)
        .conduct(Activation::Always, MutatesGraph::No)
        .reads_reports(&["coverage.out", "cover.out", "coverage.txt"])
        .build()
});

/// The built-in Go coverprofile ingester; the engine reads the report, this
/// turns bytes into records.
pub struct GoCoverPlugin;

impl Extension for GoCoverPlugin {
    fn spec(&self) -> &ExtensionSpec {
        &SPEC
    }

    fn ingest(&self, _report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        parse_gocover_records(std::str::from_utf8(content).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_expand_to_their_lines_and_accumulate_where_they_share_one() {
        let records = parse_gocover_records(
            "mode: set\nexample.com/cov/calc.go:4.24,4.41 1 1\nexample.com/cov/calc.go:12.28,13.11 1 0\nexample.com/cov/calc.go:13.11,15.3 1 0\nnot a block\n",
        )
        .expect("parses");
        let calc = &records.files[&ProjectPath::new("example.com/cov/calc.go")];
        assert_eq!(calc.lines.get(&4), Some(&1));
        assert_eq!(calc.lines.get(&12), Some(&0));
        assert_eq!(calc.lines.get(&13), Some(&0), "two blocks meet on line 13");
        assert_eq!(calc.lines.get(&15), Some(&0));
        assert!(calc.functions.is_empty());
    }

    #[test]
    fn a_stream_without_the_mode_header_is_not_a_profile() {
        assert!(parse_gocover_records("SF:x.go\nend_of_record\n").is_none());
        assert!(parse_gocover_records("mode: set\n").is_none());
    }
}
