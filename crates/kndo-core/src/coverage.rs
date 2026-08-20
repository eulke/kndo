//! Ingested coverage (ADR 0005: "coverage is ingested, never measured"): per-file,
//! line-granular hit counts as coverage producers report them, plus the machinery `crap`
//! (RFC 0005 §10) needs to turn them into a per-function `cov(m)` fraction.
//!
//! Coverage deliberately does NOT live on the [`crate::graph::ProjectGraph`] or in its
//! snapshot: a report's freshness varies independently of source content hashes, and caching
//! it into the graph would serve stale coverage on every warm run — the exact "stale
//! certainty" ADR 0005's freshness policy exists to prevent. The engine re-reads reports each
//! run (they're small) and hands the map to `analysis::run_all` as a separate input.
//!
//! Parsing lives in plugins ([`crate::plugin::Plugin::ingest_coverage`], with the lcov
//! built-in in `plugin.rs`); this module owns only the format-neutral model and the
//! span→fraction math.

use std::collections::HashMap;

use crate::adapter::{ProjectPath, Span};

/// One file's instrumented lines → execution counts, exactly as reported.
#[derive(Debug, Default, Clone)]
pub struct FileCoverage {
    pub lines: HashMap<u32, u64>,
}

/// Everything ingested this run, keyed by project-relative path.
#[derive(Debug, Default)]
pub struct CoverageMap {
    pub files: HashMap<ProjectPath, FileCoverage>,
    /// Human-readable provenance per ingested report ("coverage-lcov coverage/lcov.info
    /// (2d old)"), recorded by the *host* after each successful ingest — it located the
    /// report and checked its freshness, so it owns saying what was used. Surfaced by
    /// health's crap category so consumers can judge the source (ADR 0005).
    pub sources: Vec<String>,
}

impl CoverageMap {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// RFC 0005 §10's `cov(m)`, approximated at line granularity (ADR 0005: "line-level lcov
    /// ⇒ statement-level approximation"): the fraction of *instrumented* lines inside the
    /// function's span that executed. `None` when the file appears in no report, or the span
    /// contains no instrumented lines (a function the instrumenter skipped entirely) — both
    /// are "coverage unknown", not "coverage zero", and the caller decides what unknown means
    /// (`crap` applies the RFC's cov = 0 + "coverage: none" flag).
    pub fn function_coverage(&self, path: &ProjectPath, span: Span) -> Option<f64> {
        let file = self.files.get(path)?;
        let mut instrumented = 0usize;
        let mut covered = 0usize;
        for (&line, &hits) in &file.lines {
            if line >= span.start.0 && line <= span.end.0 {
                instrumented += 1;
                if hits > 0 {
                    covered += 1;
                }
            }
        }
        if instrumented == 0 {
            return None;
        }
        Some(covered as f64 / instrumented as f64)
    }
}

/// The typed sink [`crate::plugin::Plugin::ingest_coverage`] writes through — the core owns
/// the map; plugins only ever add validated facts to it (RFC 0003's sink discipline).
#[derive(Debug, Default)]
pub struct CoverageSink {
    map: CoverageMap,
}

impl CoverageSink {
    pub fn add_line(&mut self, path: ProjectPath, line: u32, hits: u64) {
        // Multiple records for one line (lcov emits them across test suites) accumulate —
        // a line any suite ran is covered.
        *self
            .map
            .files
            .entry(path)
            .or_default()
            .lines
            .entry(line)
            .or_insert(0) += hits;
    }

    pub fn add_source(&mut self, source: String) {
        self.map.sources.push(source);
    }

    pub fn into_map(self) -> CoverageMap {
        self.map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smol_str::SmolStr;

    fn span(start: u32, end: u32) -> Span {
        Span {
            start: (start, 1),
            end: (end, 1),
        }
    }

    #[test]
    fn function_coverage_is_the_covered_fraction_of_instrumented_lines_in_span() {
        let mut sink = CoverageSink::default();
        let p = ProjectPath(SmolStr::new("a.ts"));
        sink.add_line(p.clone(), 2, 1);
        sink.add_line(p.clone(), 3, 0);
        sink.add_line(p.clone(), 4, 5);
        sink.add_line(p.clone(), 40, 0); // outside the span — not this function's problem
        let map = sink.into_map();
        let cov = map.function_coverage(&p, span(1, 10)).unwrap();
        assert!((cov - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn unreported_files_and_uninstrumented_spans_are_unknown_not_zero() {
        let mut sink = CoverageSink::default();
        let p = ProjectPath(SmolStr::new("a.ts"));
        sink.add_line(p.clone(), 50, 1);
        let map = sink.into_map();
        assert!(map
            .function_coverage(&ProjectPath(SmolStr::new("other.ts")), span(1, 10))
            .is_none());
        assert!(map.function_coverage(&p, span(1, 10)).is_none());
    }

    #[test]
    fn repeated_line_records_accumulate() {
        let mut sink = CoverageSink::default();
        let p = ProjectPath(SmolStr::new("a.ts"));
        sink.add_line(p.clone(), 2, 0);
        sink.add_line(p.clone(), 2, 3);
        let map = sink.into_map();
        assert!((map.function_coverage(&p, span(1, 5)).unwrap() - 1.0).abs() < 1e-9);
    }
}
