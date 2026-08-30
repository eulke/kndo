//! Coverage ingestion: lcov, read from the conventional paths (`lcov.info`,
//! `coverage/lcov.info`) directly on disk — coverage is run output and usually
//! gitignored, so the discovery walk deliberately does not see it. Function records
//! (`FN`/`FNDA`) are the primary evidence — a declaration line executes at module
//! load, so line hits alone would call every loaded function tested; `DA` lines are
//! the fallback for producers that emit no function records. Everything unparseable
//! or unmappable degrades to absence, and absence never accuses.

use kndo_contract::vocab::{ProjectPath, Span};
use std::collections::BTreeMap;
use std::path::Path;

pub struct Coverage {
    pub files: BTreeMap<ProjectPath, FileCoverage>,
}

pub struct FileCoverage {
    /// Byte offset where each 1-based line starts, from the discovered content.
    line_starts: Vec<u32>,
    /// Instrumented lines (1-based) → hit count.
    lines: BTreeMap<u32, u64>,
    /// Function records: (declaration line, hit count), FN joined with FNDA by name.
    functions: Vec<(u32, u64)>,
}

/// Whether a function whose declaration spans `span` went unexecuted. `None` means
/// the coverage cannot tell (no record overlaps) — the caller falls back to weaker
/// evidence rather than accusing.
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
        // DA fallback: the body's lines, excluding the declaration line itself
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

/// The conventional lcov locations, tried in order; first parseable one wins.
pub fn ingest(root: &Path, contents: &BTreeMap<ProjectPath, &[u8]>) -> Option<Coverage> {
    for candidate in ["lcov.info", "coverage/lcov.info"] {
        if let Ok(text) = std::fs::read_to_string(root.join(candidate))
            && let Some(cov) = parse_lcov(&text, contents)
        {
            return Some(cov);
        }
    }
    None
}

/// One lcov stream. Records outside the project (paths that match no discovered
/// file) are skipped; a stream with no mappable records is no coverage at all.
fn parse_lcov(text: &str, contents: &BTreeMap<ProjectPath, &[u8]>) -> Option<Coverage> {
    let mut files = BTreeMap::new();
    let mut current: Option<(ProjectPath, FileCoverage)> = None;
    let mut fn_lines: BTreeMap<String, u32> = BTreeMap::new();

    for line in text.lines() {
        let line = line.trim();
        if let Some(path) = line.strip_prefix("SF:") {
            let path = ProjectPath::new(path.replace('\\', "/"));
            current = contents.get(&path).map(|content| {
                let fc = FileCoverage {
                    line_starts: line_starts(content),
                    lines: BTreeMap::new(),
                    functions: Vec::new(),
                };
                (path, fc)
            });
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
                let (path, mut fc) = current.take().unwrap();
                fc.functions.sort_unstable();
                files.insert(path, fc);
                fn_lines.clear();
            }
        }
    }
    if let Some((path, mut fc)) = current.take() {
        fc.functions.sort_unstable();
        files.insert(path, fc);
    }
    (!files.is_empty()).then_some(Coverage { files })
}

/// Byte offset of each line's first byte — the one line table both coverage and
/// suppression map spans through.
pub(crate) fn line_starts(content: &[u8]) -> Vec<u32> {
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

    fn contents<'a>(entries: &[(&str, &'a str)]) -> BTreeMap<ProjectPath, &'a [u8]> {
        entries
            .iter()
            .map(|(p, c)| (ProjectPath::new(*p), c.as_bytes()))
            .collect()
    }

    #[test]
    fn function_records_win_over_the_loaded_declaration_line() {
        let src = "function a() {\n  return 1;\n}\nfunction b() {\n  return 2;\n}\n";
        let map = contents(&[("src/x.js", src)]);
        let cov = parse_lcov(
            "SF:src/x.js\nFN:1,a\nFN:4,b\nFNDA:3,a\nFNDA:0,b\nDA:1,1\nDA:2,3\nDA:4,1\nDA:5,0\nend_of_record\n",
            &map,
        )
        .expect("parses");
        let fc = &cov.files[&ProjectPath::new("src/x.js")];
        // a: bytes 0..28 (lines 1-3); b: bytes 29..57 (lines 4-6).
        assert_eq!(fc.function_untested(Span::new(0, 28)), Some(false));
        assert_eq!(fc.function_untested(Span::new(29, 57)), Some(true));
    }

    #[test]
    fn da_fallback_ignores_the_declaration_line() {
        let src = "function a() {\n  return 1;\n}\n";
        let map = contents(&[("src/y.js", src)]);
        let cov = parse_lcov("SF:src/y.js\nDA:1,1\nDA:2,0\nend_of_record\n", &map).expect("parses");
        let fc = &cov.files[&ProjectPath::new("src/y.js")];
        assert_eq!(fc.function_untested(Span::new(0, 28)), Some(true));
    }

    #[test]
    fn unmappable_streams_are_no_coverage() {
        let map = contents(&[("src/z.js", "x\n")]);
        assert!(parse_lcov("SF:elsewhere/other.js\nDA:1,1\nend_of_record\n", &map).is_none());
    }
}
