//! The output side of the contract: a verdict on one subject. Identity, kind and
//! location all derive from [`Subject`]; there is no parallel field to keep agreeing.

use crate::subject::{FindingId, Subject};
use crate::vocab::{Category, Confidence};
use serde::{Deserialize, Serialize};

/// Declared worst-first so `Ord` is display order; gates compare via
/// [`Severity::at_least`], the one place the inversion lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    /// The one text spelling — the same one serde's `lowercase` writes into
    /// JSON, shared so no frontend keeps its own table (a test ties the two).
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

impl Severity {
    pub fn at_least(self, floor: Severity) -> bool {
        self <= floor
    }
}

/// 1-based, inclusive line range — display data the engine derives from the
/// subject's byte span against the file's actual newlines. Lines are exact in any
/// encoding (a newline is one byte); columns are deliberately absent — every
/// interchange format counts them in its own unit, and a slightly-wrong column is
/// worse than none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LineSpan {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Finding {
    pub id: FindingId,
    pub category: Category,
    pub severity: Severity,
    pub confidence: Confidence,
    pub subject: Subject,
    /// Where the subject's span sits in its file, when the subject has one.
    /// Never identity: moving code changes lines and must not change the finding.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lines: Option<LineSpan>,
    pub message: String,
}

impl Finding {
    pub fn new(
        category: Category,
        severity: Severity,
        confidence: Confidence,
        subject: Subject,
        discriminator: &str,
        message: impl Into<String>,
    ) -> Self {
        Finding {
            id: FindingId::derive(&category, &subject, discriminator),
            category,
            severity,
            confidence,
            subject,
            lines: None,
            message: message.into(),
        }
    }

    /// The one display spelling of WHERE this finding points: the subject's
    /// rendering, with `:line` after the path when the engine resolved one.
    pub fn location(&self) -> String {
        match (&self.subject, self.lines) {
            (Subject::Symbol { path, selector, .. }, Some(lines)) => {
                format!("{}:{} — {}", path.as_str(), lines.start, selector.render())
            }
            (Subject::Suppression { path, .. }, Some(lines)) => {
                format!("{}:{} — allow", path.as_str(), lines.start)
            }
            _ => self.subject.render(),
        }
    }
}

/// The canonical display/serialization order — one source, used by every frontend and
/// by the byte-identity gates: severity, then category, then path, then id as the
/// total tie-break (the id already folds the selector, so same-file symbols order
/// stably, if opaquely).
pub fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        (a.severity, a.category.as_str(), a.subject.path().as_str())
            .cmp(&(b.severity, b.category.as_str(), b.subject.path().as_str()))
            .then_with(|| a.id.as_str().cmp(b.id.as_str()))
    });
}
