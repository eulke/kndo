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
        let body = self.body_hits(span)?;
        Some(body.iter().all(|&c| c == 0))
    }

    /// The fraction of the body's instrumented lines that executed — the
    /// declaration line excluded, as above; `None` when no body line is
    /// instrumented. Line records only: a function record says whether the
    /// function ran, never how much of it.
    pub fn function_coverage(&self, span: Span) -> Option<f64> {
        let body = self.body_hits(span)?;
        let hit = body.iter().filter(|&&c| c > 0).count();
        Some(hit as f64 / body.len() as f64)
    }

    /// Hit counts of the instrumented lines below the declaration line. A
    /// function that fits on its declaration line has no body line to read,
    /// and no record to answer with.
    fn body_hits(&self, span: Span) -> Option<Vec<u64>> {
        let first = self.line_of(span.start);
        let last = self.line_of(span.end.saturating_sub(1).max(span.start));
        if last <= first {
            return None;
        }
        let body: Vec<u64> = self
            .lines
            .range(first + 1..=last)
            .map(|(_, c)| *c)
            .collect();
        (!body.is_empty()).then_some(body)
    }

    fn line_of(&self, byte: u32) -> u32 {
        self.line_starts.partition_point(|&s| s <= byte) as u32
    }
}

/// Records → judgeable coverage, given the run's file contents (the line table each
/// span query maps through). Records outside the project — paths naming no
/// discovered file under [`locate`]'s rule — are skipped; records with nothing
/// mappable are no coverage at all. The mapping half of ingestion, engine-side
/// always: an extension states records in the report's own spelling, never a
/// line table and never a guess about the project's layout.
pub fn assemble(
    records: CoverageRecords,
    contents: &BTreeMap<ProjectPath, &[u8]>,
) -> Option<Coverage> {
    let by_name = ByName::over(contents);
    let mut files: BTreeMap<ProjectPath, FileCoverage> = BTreeMap::new();
    for (reported, rec) in records.files {
        let Some(path) = by_name.locate(&reported, contents) else {
            continue;
        };
        let content = contents[&path];
        // Two report entries naming one file (a Java source's classes reported
        // apart) accumulate, the same rule as repeated lcov sections.
        let entry = files.entry(path).or_insert_with(|| FileCoverage {
            line_starts: line_starts(content),
            lines: BTreeMap::new(),
            functions: Vec::new(),
        });
        for (line, hits) in rec.lines {
            *entry.lines.entry(line).or_insert(0) += hits;
        }
        entry.functions.extend(rec.functions);
    }
    for fc in files.values_mut() {
        fc.functions.sort_unstable();
    }
    (!files.is_empty()).then_some(Coverage { files })
}

/// Discovered paths by file name — the candidates a reported path can mean.
struct ByName<'a> {
    names: BTreeMap<&'a str, Vec<&'a ProjectPath>>,
}

impl<'a> ByName<'a> {
    fn over(contents: &'a BTreeMap<ProjectPath, &[u8]>) -> Self {
        let mut names: BTreeMap<&str, Vec<&ProjectPath>> = BTreeMap::new();
        for path in contents.keys() {
            let name = path.as_str().rsplit('/').next().unwrap_or(path.as_str());
            names.entry(name).or_default().push(path);
        }
        ByName { names }
    }

    /// The project file a reported path names: the path itself when the project
    /// has it, else the ONE project file that ends with the reported path or
    /// that the reported path ends with, at a `/` boundary. A Go profile keys by
    /// import path (`github.com/x/y/render.go` for `render.go`), JaCoCo by
    /// package and source name (`demo/Classify.java` for
    /// `src/main/java/demo/Classify.java`), coverage.py by the path under a
    /// source root it records separately. Two project files sharing the spelling
    /// leave the record unmapped: crediting the wrong file would be a guess.
    fn locate(
        &self,
        reported: &ProjectPath,
        contents: &BTreeMap<ProjectPath, &[u8]>,
    ) -> Option<ProjectPath> {
        if contents.contains_key(reported) {
            return Some(reported.clone());
        }
        let r = reported.as_str();
        let name = r.rsplit('/').next().unwrap_or(r);
        let mut found: Option<&ProjectPath> = None;
        for candidate in self.names.get(name).into_iter().flatten() {
            let p = candidate.as_str();
            if suffix_at_boundary(r, p) || suffix_at_boundary(p, r) {
                if found.is_some() {
                    return None;
                }
                found = Some(candidate);
            }
        }
        found.cloned()
    }
}

/// `longer` ends with `/shorter`.
fn suffix_at_boundary(longer: &str, shorter: &str) -> bool {
    longer.len() > shorter.len()
        && longer.ends_with(shorter)
        && longer.as_bytes()[longer.len() - shorter.len() - 1] == b'/'
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

    type FileSpec<'a> = (&'a str, &'a [(u32, u64)], &'a [(u32, u64)]);

    fn records(entries: &[FileSpec<'_>]) -> CoverageRecords {
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

    /// A one-line function has no body line below its declaration: the range
    /// is empty, never inverted — the first real producer's report (pytest-cov
    /// on flask) carried hundreds of these.
    #[test]
    fn a_one_line_function_without_a_function_record_is_unknown() {
        let src = "def f(): return 1\ndef g():\n    return 2\n";
        let map = contents(&[("src/o.py", src)]);
        let cov = assemble(
            records(&[("src/o.py", &[(1, 1), (2, 1), (3, 0)], &[])]),
            &map,
        )
        .expect("assembles");
        let fc = &cov.files[&ProjectPath::new("src/o.py")];
        assert_eq!(fc.function_untested(Span::new(0, 17)), None);
        assert_eq!(fc.function_untested(Span::new(18, 35)), Some(true));
    }

    #[test]
    fn the_covered_fraction_counts_body_lines_only() {
        let src = "def f(x):\n    if x:\n        return 1\n    return 2\n";
        let map = contents(&[("src/p.py", src)]);
        let cov = assemble(
            records(&[("src/p.py", &[(1, 1), (2, 5), (3, 0), (4, 5)], &[(1, 5)])]),
            &map,
        )
        .expect("assembles");
        let fc = &cov.files[&ProjectPath::new("src/p.py")];
        let whole = Span::new(0, src.len() as u32);
        assert_eq!(fc.function_coverage(whole), Some(2.0 / 3.0));
        assert_eq!(fc.function_untested(whole), Some(false));
        assert_eq!(fc.function_coverage(Span::new(0, 9)), None);
    }

    /// A Go profile keys by import path, JaCoCo by package and source name:
    /// each maps onto the one project file spelled that way, and an ambiguous
    /// spelling maps onto nothing.
    #[test]
    fn a_reports_own_spelling_maps_onto_the_one_file_it_names() {
        let map = contents(&[
            (
                "render/json.go",
                "package render\nfunc a() {\n\treturn\n}\n",
            ),
            (
                "src/main/java/demo/Classify.java",
                "class Classify {\n  int f() {\n    return 1;\n  }\n}\n",
            ),
            ("a/util.py", "x\n"),
            ("b/util.py", "x\n"),
        ]);
        let cov = assemble(
            records(&[
                ("github.com/x/y/render/json.go", &[(2, 1), (3, 1)], &[]),
                ("demo/Classify.java", &[(2, 0), (3, 0)], &[(2, 0)]),
                ("util.py", &[(1, 1)], &[]),
            ]),
            &map,
        )
        .expect("assembles");
        assert!(cov.files.contains_key(&ProjectPath::new("render/json.go")));
        assert!(
            cov.files
                .contains_key(&ProjectPath::new("src/main/java/demo/Classify.java"))
        );
        assert_eq!(
            cov.files.len(),
            2,
            "the ambiguous `util.py` maps onto nothing"
        );
    }

    #[test]
    fn unmappable_records_are_no_coverage() {
        let map = contents(&[("src/z.js", "x\n")]);
        assert!(assemble(records(&[("elsewhere/other.js", &[(1, 1)], &[])]), &map).is_none());
    }
}
