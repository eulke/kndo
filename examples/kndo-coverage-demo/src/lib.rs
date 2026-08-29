//! kndo-coverage-demo — the reference external coverage ingester for kndo-plugin-api's
//! `coverage-ingester` world. Parses a deliberately trivial invented format so the crate
//! needs nothing beyond `wit-bindgen`: one `path:line=hits` record per line (`#` comments
//! and blank lines skipped). The point is proving the unidirectional ABI end to end — the
//! host locates and freshness-checks the report, pushes the bytes in, and this guest
//! returns line facts the host alone writes into its sink and rebases — not modeling a real
//! coverage tool (real formats ship as native built-ins in `kndo-plugin-coverage`; this
//! world exists for the formats kndo doesn't ship).

// kndo:allow-file untested this guest crate is exercised end-to-end by the WASM-bridge
// integration tests, which build it as a subprocess and run the component in-process —
// reachability the host repo's graph cannot see.
// (No crap pragma, unlike the sibling demos: nothing here is complex enough to score.)

// Marks the dependency used explicitly — the macro invocation below is a fully-qualified
// path with no `use`, which kndo's own Rust adapter (a static extractor, not a macro
// expander) has no way to trace back to the `wit-bindgen` crate on its own.
use wit_bindgen as _;

wit_bindgen::generate!({
    path: "../../crates/kndo-plugin-api/wit/plugin.wit",
    world: "coverage-ingester",
});

struct DemoIngester;

impl Guest for DemoIngester {
    fn descriptor() -> PluginDescriptor {
        PluginDescriptor {
            id: "coverage-demo".to_string(),
            version: "1".to_string(),
            detection: vec!["a cov.demo report at the project root".to_string()],
            // Where the HOST looks for this ingester's report — the guest itself never
            // touches the filesystem (the world has no imports at all).
            requested_file_access: vec!["cov.demo".to_string()],
            activation: vec![ActivationRule::FileExists("cov.demo".to_string())],
            dependencies: Vec::new(),
        }
    }

    /// `path:line=hits`, one record per line. Paths are recorded as the report states
    /// them — root/package rebasing is the host's job, uniformly for every ingester.
    fn ingest_coverage(_path: String, content: Vec<u8>) -> IngestedCoverage {
        let mut lines = Vec::new();
        let text = String::from_utf8_lossy(&content);
        for record in text.lines() {
            let record = record.trim();
            if record.is_empty() || record.starts_with('#') {
                continue;
            }
            let Some((path, rest)) = record.rsplit_once(':') else {
                continue;
            };
            let Some((line, hits)) = rest.split_once('=') else {
                continue;
            };
            if let (Ok(line), Ok(hits)) = (line.parse(), hits.parse()) {
                lines.push(CoverageLine {
                    path: path.to_string(),
                    line,
                    hits,
                });
            }
        }
        IngestedCoverage {
            lines,
            sources: vec!["demo-format v1".to_string()],
        }
    }
}

export!(DemoIngester);
