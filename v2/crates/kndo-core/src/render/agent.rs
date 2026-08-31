//! Agent format 1 — a token-frugal, line-oriented text projection of the envelope
//! for LLM context windows. The finding's [`kndo_contract::subject::FindingId`] is
//! the reference handle — stable across runs, which no per-run numbering could be —
//! so lines carry the id and nothing is numbered. Sections appear only when they
//! have content; the `result:` line always carries every count, so absence reads as
//! zero, never as unknown.
//!
//! Label-first counts (`findings 2`, `roots 1`) keep the grammar identical at every
//! quantity. The header stamps the format version and the envelope schema it
//! projects; the crate version deliberately does not appear — these bytes move only
//! when the format or the report moves, which is what lets a golden pin them.

use crate::report::{REPORT_SCHEMA, Report};
use kndo_contract::evidence::DiagnosticLevel;
use kndo_contract::finding::Finding;

/// Moves only when the line grammar changes meaning; new envelope content
/// rendering through the existing grammar is not a bump.
const AGENT_FORMAT: u32 = 1;

impl Report {
    /// The agent rendering: a newline-terminated text document, byte-pinned by the
    /// `agent_format_matches_its_committed_golden` gate. A pure projection of the
    /// report — everything here is readable from [`Report::to_json`] output.
    pub fn to_agent(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "kndo agent format {AGENT_FORMAT} ({REPORT_SCHEMA})\n"
        ));
        out.push_str(&format!(
            "result: findings {} · baselined {} · fixed {} · files {}/{} claimed\n",
            self.findings.len(),
            self.baselined,
            self.fixed.len(),
            self.run.files_claimed,
            self.run.files_discovered,
        ));
        if let Some(health) = &self.health {
            out.push_str(&format!(
                "health: {} · implicated {} of {}",
                health.score_text(),
                health.implicated,
                health.subjects
            ));
            for c in &health.by_category {
                out.push_str(&format!(" · {} {}", c.category.as_str(), c.findings));
            }
            out.push('\n');
        }
        if !self.run.extensions.is_empty() {
            let list: Vec<String> = self
                .run
                .extensions
                .iter()
                .map(|e| format!("{} {}", e.id, e.files))
                .collect();
            out.push_str(&format!("extensions: {}\n", list.join(" · ")));
        }
        section(&mut out, "findings", &self.findings);
        section(&mut out, "fixed", &self.fixed);
        if !self.abstained.is_empty() {
            out.push_str("abstained:\n");
            for a in &self.abstained {
                out.push_str(&format!("- {}: {}\n", a.category.as_str(), a.reason));
            }
        }
        if self.suppressed.total > 0 {
            out.push_str(&format!("suppressed: {}", self.suppressed.total));
            for (category, n) in &self.suppressed.by_category {
                out.push_str(&format!(" · {} {n}", category.as_str()));
            }
            out.push('\n');
        }
        if !self.plugins.is_empty() {
            out.push_str("plugins:\n");
            for p in &self.plugins {
                out.push_str(&format!(
                    "- {}: roots {} · findings {}\n",
                    p.coordinate, p.roots, p.findings
                ));
                for line in &p.dropped {
                    out.push_str(&format!("  dropped: {line}\n"));
                }
                if p.content_budget_cut {
                    out.push_str("  content budget cut — its findings may be partial\n");
                }
            }
        }
        if !self.diagnostics.is_empty() {
            out.push_str("diagnostics:\n");
            for d in &self.diagnostics {
                out.push_str(&format!(
                    "- [{}] {}: {}\n",
                    level_word(d.level),
                    d.path.as_str(),
                    d.message
                ));
            }
        }
        out
    }
}

fn section(out: &mut String, header: &str, findings: &[Finding]) {
    if findings.is_empty() {
        return;
    }
    out.push_str(header);
    out.push_str(":\n");
    for f in findings {
        out.push_str(&format!(
            "[{}] {} {} · {} {} · {}\n  {}\n",
            f.id.as_str(),
            f.severity.as_str(),
            f.category.as_str(),
            f.subject.kind().as_str(),
            f.location(),
            f.confidence.as_str(),
            f.message,
        ));
    }
}

fn level_word(level: DiagnosticLevel) -> &'static str {
    match level {
        DiagnosticLevel::Info => "info",
        DiagnosticLevel::Warn => "warn",
        DiagnosticLevel::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use crate::analysis::{Abstention, AbstentionReason, AbstentionScope};
    use crate::conduct::Contribution;
    use crate::report::{ExtensionRun, REPORT_SCHEMA, Report, ReportDiagnostic, RunInfo};
    use crate::suppress::SuppressedSummary;
    use kndo_contract::evidence::DiagnosticLevel;
    use kndo_contract::finding::{Finding, Severity};
    use kndo_contract::subject::{Subject, SymbolSelector};
    use kndo_contract::vocab::{Category, Confidence, ProjectPath, Span};
    use smol_str::SmolStr;

    fn empty_report() -> Report {
        Report {
            run: RunInfo {
                schema: REPORT_SCHEMA,
                files_discovered: 0,
                files_claimed: 0,
                extensions: Vec::new(),
            },
            health: None,
            findings: Vec::new(),
            fixed: Vec::new(),
            baselined: 0,
            abstained: Vec::new(),
            suppressed: SuppressedSummary::default(),
            plugins: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn symbol_finding(name: &str) -> Finding {
        Finding::new(
            Category::UNUSED,
            Severity::Warning,
            Confidence::Probable,
            Subject::Symbol {
                path: ProjectPath::new("src/lib.py"),
                selector: SymbolSelector::Free(SmolStr::new(name)),
                span: Span::new(10, 40),
            },
            "",
            format!("`{name}` is declared but nothing in the project uses it"),
        )
    }

    #[test]
    fn an_empty_report_is_header_and_result_line_only() {
        let text = empty_report().to_agent();
        assert_eq!(
            text,
            "kndo agent format 1 (kndo-v2/m6)\n\
             result: findings 0 · baselined 0 · fixed 0 · files 0/0 claimed\n"
        );
    }

    #[test]
    fn every_section_renders_and_only_when_it_has_content() {
        let mut report = empty_report();
        report.run.files_discovered = 6;
        report.run.files_claimed = 5;
        report.health = Some(crate::health::Health {
            implicated: 1,
            subjects: 8,
            by_category: vec![crate::health::CategoryCount {
                category: Category::UNUSED,
                findings: 1,
            }],
        });
        report.run.extensions.push(ExtensionRun {
            id: SmolStr::new("kndo:python"),
            files: 5,
        });
        report.findings.push(symbol_finding("_ghost"));
        report.fixed.push(symbol_finding("_gone"));
        report.baselined = 2;
        report.abstained.push(Abstention {
            category: Category::UNTESTED,
            reason: AbstentionReason::NoTestRootsAnywhere,
            scope: AbstentionScope::WholeRun,
        });
        report.suppressed = SuppressedSummary {
            total: 3,
            by_category: vec![(Category::UNUSED, 2), (Category::STALE, 1)],
        };
        report.plugins.push(Contribution {
            coordinate: SmolStr::new("kndo:express"),
            roots: 1,
            findings: 0,
            dropped: vec!["root target `x` resolved to nothing".to_string()],
            content_budget_cut: true,
        });
        report.diagnostics.push(ReportDiagnostic {
            path: ProjectPath::new("src/broken.py"),
            level: DiagnosticLevel::Warn,
            message: "parse error".to_string(),
        });

        let text = report.to_agent();
        assert!(text.contains("result: findings 1 · baselined 2 · fixed 1 · files 5/6 claimed\n"));
        assert!(text.contains("health: 87.5 · implicated 1 of 8 · unused 1\n"));
        assert!(text.contains("extensions: kndo:python 5\n"));
        assert!(text.contains("findings:\n["));
        assert!(text.contains("] warning unused · symbol src/lib.py — _ghost · probable\n"));
        assert!(text.contains("  `_ghost` is declared but nothing in the project uses it\n"));
        assert!(text.contains("fixed:\n["));
        assert!(text.contains("abstained:\n- untested:"));
        assert!(text.contains("suppressed: 3 · unused 2 · stale 1\n"));
        assert!(text.contains("- kndo:express: roots 1 · findings 0\n"));
        assert!(text.contains("  dropped: root target `x` resolved to nothing\n"));
        assert!(text.contains("  content budget cut — its findings may be partial\n"));
        assert!(text.contains("diagnostics:\n- [warn] src/broken.py: parse error\n"));
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn the_finding_id_is_the_reference_handle() {
        let report = {
            let mut r = empty_report();
            r.findings.push(symbol_finding("_ghost"));
            r
        };
        let id = report.findings[0].id.as_str().to_string();
        assert!(report.to_agent().contains(&format!("[{id}]")));
    }
}
