//! `kndo.toml` — the CLI's persisted invocation defaults. Two laws, both from
//! measured v1 incidents: only keys with a LIVING consumer exist (a commented-out
//! key is still a promise), and an unknown key is a refused invocation, never
//! silence — `deny_unknown_fields` makes a typo indistinguishable from a wrong
//! flag instead of a quietly ignored one. The template `kndo init` writes and
//! this parse struct are held together by a test: every template key parses,
//! every struct key appears in the template.
//!
//! Precedence has ONE spelling, [`effective`]: flag > `KNDO_FORMAT` (format
//! only — the environment has no opinion on gates or selection) > `kndo.toml` >
//! built-in default. No second merge site.

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct FileConfig {
    #[serde(default)]
    pub check: CheckTable,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CheckTable {
    /// Lowest severity that fails the run.
    pub fail_on: Option<crate::FailOn>,
    /// Report format when neither the flag nor `KNDO_FORMAT` says.
    pub format: Option<crate::Format>,
    /// Judge only these categories.
    #[serde(default)]
    pub only: Vec<String>,
    /// Judge everything except these categories.
    #[serde(default)]
    pub skip: Vec<String>,
}

/// The template `kndo init` writes: every key above, present and commented out —
/// the file parses empty, and uncommenting any line turns on exactly that key.
pub const TEMPLATE: &str = "\
# kndo.toml — invocation defaults for this project.
# Flags beat KNDO_FORMAT beats this file beats built-in defaults.
# An unknown key refuses the run: a typo is never silently ignored.

[check]
# Lowest severity that fails the run: error | warning | info | never.
#fail-on = \"warning\"

# Report format when neither --format nor KNDO_FORMAT says:
# human | json | agent | sarif.
#format = \"human\"

# Judge only these categories (or use `skip` for the complement).
#only = [\"unused\"]
#skip = [\"duplicate\"]
";

/// Read `<root>/kndo.toml` if present. A file that exists but does not parse —
/// bad TOML, an unknown key, a wrong value — is an error the caller refuses on.
pub fn load(root: &Path) -> Result<FileConfig, String> {
    let path = root.join("kndo.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FileConfig::default()),
        Err(e) => return Err(format!("could not read kndo.toml: {e}")),
    };
    toml::from_str(&text).map_err(|e| format!("kndo.toml: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_and_the_struct_are_one_list() {
        // The template parses (commented = empty config)…
        let parsed: FileConfig = toml::from_str(TEMPLATE).expect("template parses");
        assert!(parsed.check.fail_on.is_none());

        // …every commented key in it, uncommented, parses too…
        let uncommented: String = TEMPLATE
            .lines()
            .map(|l| l.strip_prefix('#').filter(|s| s.contains('=')).unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n");
        let full: FileConfig = toml::from_str(&uncommented).expect("uncommented template parses");
        assert!(full.check.fail_on.is_some());
        assert!(full.check.format.is_some());
        assert!(!full.check.only.is_empty());
        assert!(!full.check.skip.is_empty());

        // …and every struct key appears in the template, so a new key cannot
        // ship without its line (the one-list rule, held by this test).
        for key in ["fail-on", "format", "only", "skip"] {
            assert!(
                TEMPLATE.contains(key),
                "template is missing the `{key}` line"
            );
        }
    }

    #[test]
    fn an_unknown_key_is_an_error_not_silence() {
        let err = toml::from_str::<FileConfig>("[check]\nfail-onn = \"warning\"\n")
            .expect_err("typo must not parse");
        assert!(err.to_string().contains("fail-onn"), "{err}");
    }
}
