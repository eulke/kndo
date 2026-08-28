//! JSON extraction: a non-source language — no declarations, no imports, no references, no
//! roots, nothing to measure. `FileFacts::default()` plus, at most, one parse-failure
//! diagnostic when the content isn't valid JSON ("surviving broken code, but saying so"
//! applies even to a format with nothing else to extract).

use kndo_core::adapter::{AdapterDiagnostic, DiagnosticLevel, FileFacts};

pub(crate) fn extract(content: &[u8]) -> FileFacts {
    let mut out = FileFacts::default();
    if let Err(e) = serde_json::from_slice::<serde_json::Value>(content) {
        out.diagnostics.push(AdapterDiagnostic {
            level: DiagnosticLevel::Warn,
            message: format!("invalid JSON: {e}"),
            span: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_json_extracts_no_facts_and_no_diagnostics() {
        let f = extract(br#"{"a": [1, 2, {"b": true}], "c": null}"#);
        assert!(f.declarations.is_empty());
        assert!(f.imports.is_empty());
        assert!(f.references.is_empty());
        assert!(f.roots.is_empty());
        assert!(f.functions.is_empty());
        assert_eq!(f.unit, None);
        assert!(f.diagnostics.is_empty());
    }

    #[test]
    fn an_empty_object_and_array_are_both_valid() {
        assert!(extract(b"{}").diagnostics.is_empty());
        assert!(extract(b"[]").diagnostics.is_empty());
    }

    #[test]
    fn malformed_json_surfaces_one_warning_diagnostic_not_a_panic() {
        let f = extract(b"{ this is not json ");
        assert_eq!(f.diagnostics.len(), 1);
        assert_eq!(f.diagnostics[0].level, DiagnosticLevel::Warn);
        assert!(f.diagnostics[0].message.contains("invalid JSON"));
        // Still a well-formed (empty) FileFacts otherwise — broken input never panics or
        // fabricates partial declarations for a language that never had any to begin with.
        assert!(f.declarations.is_empty());
    }

    #[test]
    fn json_with_comments_is_rejected_as_malformed_not_tolerated_as_jsonc() {
        // JSONC/JSON5 are a deliberate non-goal — comments make the content malformed JSON.
        let f = extract(b"{\n  // a comment\n  \"a\": 1\n}\n");
        assert_eq!(f.diagnostics.len(), 1);
    }
}
