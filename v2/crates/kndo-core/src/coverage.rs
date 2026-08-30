//! The engine's half of coverage ingestion — format-blind by construction. An
//! ingesting extension states RECORDS (what its report format claims, contract
//! vocabulary); this module maps them against the project the run actually
//! discovered and answers span queries for the analyses. No format name appears
//! here: the engine exposes the tools for an ingester to exist and judges what
//! any of them delivers, uniformly.

use kndo_contract::evidence::CoverageRecords;
use kndo_contract::vocab::{ProjectPath, Span};
use std::collections::BTreeMap;

pub struct Coverage {
    pub files: BTreeMap<ProjectPath, FileCoverage>,
}

pub struct FileCoverage {
    /// Byte offset where each 1-based line starts, from the discovered content.
    line_starts: Vec<u32>,
    /// Instrumented lines (1-based) → hit count.
    lines: BTreeMap<u32, u64>,
    /// Function records: (declaration line, hit count), sorted by line.
    functions: Vec<(u32, u64)>,
}

/// Whether a function whose declaration spans `span` went unexecuted. `None` means
/// the coverage cannot tell (no record overlaps) — the caller falls back to weaker
/// evidence rather than accusing. Function records are the primary evidence — a
/// declaration line executes at module load, so line hits alone would call every
/// loaded function tested; line records are the fallback for producers that emit
/// no function records.
impl FileCoverage {
    pub fn function_untested(&self, span: Span) -> Option<bool> {
        let first = self.line_of(span.start);
        let last = self.line_of(span.end.saturating_sub(1).max(span.start));
        if let Some((_, count)) = self
            .functions
            .iter()
            .filter(|(line, _)| (first..=last).contains(line))
            .min_by_key(|(line, _)| *line)
        {
            return Some(*count == 0);
        }
        // Line fallback: the body's lines, excluding the declaration line itself
        // (module load executes it).
        let body: Vec<u64> = self
            .lines
            .range(first + 1..=last)
            .map(|(_, c)| *c)
            .collect();
        if body.is_empty() {
            return None;
        }
        Some(body.iter().all(|&c| c == 0))
    }

    fn line_of(&self, byte: u32) -> u32 {
        self.line_starts.partition_point(|&s| s <= byte) as u32
    }
}

/// Records → judgeable coverage, given the run's file contents (the line table each
/// span query maps through). Records outside the project — paths matching no
/// discovered file — are skipped; records with nothing mappable are no coverage at
/// all. The mapping half of ingestion, engine-side always: an extension states
/// records, never a line table.
pub fn assemble(
    records: CoverageRecords,
    contents: &BTreeMap<ProjectPath, &[u8]>,
) -> Option<Coverage> {
    let mut files = BTreeMap::new();
    for (path, rec) in records.files {
        let Some(content) = contents.get(&path) else {
            continue;
        };
        let mut functions = rec.functions;
        functions.sort_unstable();
        files.insert(
            path,
            FileCoverage {
                line_starts: line_starts(content),
                lines: rec.lines,
                functions,
            },
        );
    }
    (!files.is_empty()).then_some(Coverage { files })
}

/// Byte offset of each line's first byte — the one line table both coverage and
/// suppression map spans through.
pub fn line_starts(content: &[u8]) -> Vec<u32> {
    let mut starts = vec![0u32];
    for (i, &b) in content.iter().enumerate() {
        if b == b'\n' {
            starts.push(i as u32 + 1);
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_contract::evidence::FileRecords;

    fn contents<'a>(entries: &[(&str, &'a str)]) -> BTreeMap<ProjectPath, &'a [u8]> {
        entries
            .iter()
            .map(|(p, c)| (ProjectPath::new(*p), c.as_bytes()))
            .collect()
    }

    fn records(entries: &[(&str, &[(u32, u64)], &[(u32, u64)])]) -> CoverageRecords {
        CoverageRecords {
            files: entries
                .iter()
                .map(|(path, lines, functions)| {
                    (
                        ProjectPath::new(*path),
                        FileRecords {
                            lines: lines.iter().copied().collect(),
                            functions: functions.to_vec(),
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn function_records_win_over_the_loaded_declaration_line() {
        let src = "function a() {\n  return 1;\n}\nfunction b() {\n  return 2;\n}\n";
        let map = contents(&[("src/x.js", src)]);
        let cov = assemble(
            records(&[(
                "src/x.js",
                &[(1, 1), (2, 3), (4, 1), (5, 0)],
                &[(1, 3), (4, 0)],
            )]),
            &map,
        )
        .expect("assembles");
        let fc = &cov.files[&ProjectPath::new("src/x.js")];
        // a: bytes 0..28 (lines 1-3); b: bytes 29..57 (lines 4-6).
        assert_eq!(fc.function_untested(Span::new(0, 28)), Some(false));
        assert_eq!(fc.function_untested(Span::new(29, 57)), Some(true));
    }

    #[test]
    fn line_fallback_ignores_the_declaration_line() {
        let src = "function a() {\n  return 1;\n}\n";
        let map = contents(&[("src/y.js", src)]);
        let cov =
            assemble(records(&[("src/y.js", &[(1, 1), (2, 0)], &[])]), &map).expect("assembles");
        let fc = &cov.files[&ProjectPath::new("src/y.js")];
        assert_eq!(fc.function_untested(Span::new(0, 28)), Some(true));
    }

    #[test]
    fn unmappable_records_are_no_coverage() {
        let map = contents(&[("src/z.js", "x\n")]);
        assert!(assemble(records(&[("elsewhere/other.js", &[(1, 1)], &[])]), &map).is_none());
    }
}
