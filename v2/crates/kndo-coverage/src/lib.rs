//! The lcov coverage extension — ALL the format knowledge in one crate, and only
//! format knowledge: the parser turns an lcov stream into the contract's records
//! ("what the report states"), and the built-in ingester is that parser behind
//! the same [`Extension`] trait everything else implements. No I/O and no
//! engine dependency by design: the same crate compiles natively (the built-in)
//! and to WASM (the reference external ingester), so the two can never drift
//! apart by prose — and mapping records onto the project is the ENGINE's job,
//! uniformly for every ingester, never done here.
//!
//! A format earns its parser here with a fixture captured from a real producer;
//! lcov is the one that has. Function records (`FN`/`FNDA`) are the primary
//! evidence — a declaration line executes at module load, so line hits alone
//! would call every loaded function tested; `DA` lines are the fallback for
//! producers that emit no function records. Everything unparseable degrades to
//! absence, and absence never accuses.

use kndo_contract::extension::{Activation, Extension, ExtensionSpec, MutatesGraph};
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The record types are contract vocabulary (`kndo-contract`'s evidence module):
/// what an ingesting extension returns, re-exported here where the parser that
/// produces them lives.
pub use kndo_contract::evidence::{CoverageRecords, FileRecords};

/// One lcov stream, to records — needs no file contents, which is what lets the
/// same parse run inside a WASM guest. A stream with no records at all is `None`.
pub fn parse_lcov_records(text: &str) -> Option<CoverageRecords> {
    let mut files: BTreeMap<ProjectPath, FileRecords> = BTreeMap::new();
    let mut current: Option<(ProjectPath, FileRecords)> = None;
    let mut fn_lines: BTreeMap<String, u32> = BTreeMap::new();

    for line in text.lines() {
        let line = line.trim();
        if let Some(path) = line.strip_prefix("SF:") {
            let path = ProjectPath::new(path.replace('\\', "/"));
            current = Some((path, FileRecords::default()));
            fn_lines.clear();
        } else if let Some((_, fc)) = &mut current {
            if let Some(rest) = line.strip_prefix("DA:") {
                if let Some((l, c)) = rest.split_once(',')
                    && let (Ok(l), Ok(c)) = (
                        l.parse::<u32>(),
                        c.split(',').next().unwrap_or("0").parse::<u64>(),
                    )
                {
                    *fc.lines.entry(l).or_insert(0) += c;
                }
            } else if let Some(rest) = line.strip_prefix("FN:") {
                if let Some((l, name)) = rest.split_once(',')
                    && let Ok(l) = l.parse::<u32>()
                {
                    fn_lines.insert(name.to_string(), l);
                }
            } else if let Some(rest) = line.strip_prefix("FNDA:") {
                if let Some((count, name)) = rest.split_once(',')
                    && let Ok(count) = count.parse::<u64>()
                    && let Some(&l) = fn_lines.get(name)
                {
                    fc.functions.push((l, count));
                }
            } else if line == "end_of_record" {
                let (path, fc) = current.take().unwrap();
                files.insert(path, fc);
                fn_lines.clear();
            }
        }
    }
    if let Some((path, fc)) = current.take() {
        files.insert(path, fc);
    }
    (!files.is_empty()).then_some(CoverageRecords { files })
}

static SPEC: LazyLock<ExtensionSpec> = LazyLock::new(|| {
    ExtensionSpec::builder("kndo:coverage-lcov", 1)
        // MutatesGraph::No is load-bearing: an ingester contributes analysis
        // input, never graph facts, and Yes here would turn the persisted graph
        // cache off for every project, because this extension is always on.
        .conduct(Activation::Always, MutatesGraph::No)
        // The conventional lcov locations, tried in order; the first report that
        // parses AND maps onto the project wins.
        .reads_reports(&["lcov.info", "coverage/lcov.info"])
        .build()
});

/// The built-in lcov ingester. Coverage is run output and usually gitignored, so
/// the discovery walk deliberately never sees it; the spec's `reads_reports`
/// paths are the sanctioned way in, and the ENGINE does the reading — this
/// extension only turns bytes into records.
pub struct LcovPlugin;

impl Extension for LcovPlugin {
    fn spec(&self) -> &ExtensionSpec {
        &SPEC
    }

    fn ingest(&self, _report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        parse_lcov_records(std::str::from_utf8(content).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_and_line_records_parse_as_the_report_states_them() {
        let records = parse_lcov_records(
            "SF:src/x.js\nFN:1,a\nFN:4,b\nFNDA:3,a\nFNDA:0,b\nDA:2,3\nDA:5,0\nend_of_record\n\
             SF:not/in/project.js\nDA:1,1\nend_of_record\n",
        )
        .expect("parses");
        // The parser keeps the report's own view — mapping against the project
        // is the engine's half, never done here.
        assert_eq!(records.files.len(), 2);
        let x = &records.files[&ProjectPath::new("src/x.js")];
        assert_eq!(x.functions, [(1, 3), (4, 0)]);
        assert_eq!(x.lines.get(&2), Some(&3));
        assert_eq!(x.lines.get(&5), Some(&0));
    }

    #[test]
    fn an_empty_or_foreign_stream_is_absence() {
        assert!(parse_lcov_records("").is_none());
        assert!(parse_lcov_records("not lcov at all\n").is_none());
    }
}
