//! The reference external coverage ingester: `kndo-coverage`'s OWN lcov parser,
//! compiled to WASM. The built-in native ingester and this component share their
//! parsing code by construction — the "synchronized by prose" native/host pair
//! the v1 postmortem recorded cannot exist here, because there is one parser.
//! The guest states records; the host maps them against the project and builds
//! the line tables, uniformly for every ingester.

use kndo_sdk::ingester::{Guest, export};
use kndo_sdk::wire::{
    Activation, CoverageRecord, CoverageRecords, PluginSpec,
};

struct RecordsIngester;

impl Guest for RecordsIngester {
    fn spec() -> PluginSpec {
        PluginSpec {
            coordinate: "demo:lcov-records".to_string(),
            version: 1,
            activation: Activation::Always,
            dependencies: Vec::new(),
            // For an ingester these are the well-known, root-relative report
            // paths, tried in order.
            requested_file_access: vec![
                "lcov.info".to_string(),
                "coverage/lcov.info".to_string(),
            ],
            rules: Vec::new(),
        }
    }

    fn ingest(_path: String, content: Vec<u8>) -> Option<CoverageRecords> {
        let text = String::from_utf8(content).ok()?;
        let records = kndo_coverage::parse_lcov_records(&text)?;
        let mut lines = Vec::new();
        let mut functions = Vec::new();
        for (path, file) in records.files {
            let path = path.as_str().to_string();
            for (line, hits) in file.lines {
                lines.push(CoverageRecord {
                    path: path.clone(),
                    line,
                    hits,
                });
            }
            for (line, hits) in file.functions {
                functions.push(CoverageRecord {
                    path: path.clone(),
                    line,
                    hits,
                });
            }
        }
        Some(CoverageRecords { lines, functions })
    }
}

export!(RecordsIngester with_types_in kndo_sdk::ingester);
