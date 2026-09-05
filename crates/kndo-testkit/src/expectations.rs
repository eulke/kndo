//! Fixture expectations: a fixture's claims as data, checked against the run so a
//! comment can never contradict a pin — the claim IS a pin.
//!
//! `expectations.toml` sits beside `expected.json`:
//!
//! ```toml
//! [[dead]]                    # must be reported (any category unless narrowed)
//! subject = "src/lib.js#legacy"
//! category = "unused"         # optional
//! why = "exported through module.exports and never imported"
//!
//! [[alive]]                   # must NOT be reported by unused/internal-only
//! subject = "src/handlers/alpha.ts"
//! category = "unused"         # optional: narrows the categories checked
//! why = "alive only through the narrowed dynamic import"
//!
//! [[known_gap]]               # what should hold once fixed, held open on purpose
//! subject = "src/lib.js#legacy"
//! expect = "dead"             # "dead" | "alive"
//! category = "unused"         # optional
//! why = "the export site counts as a use today"
//! fix = "M8.c js-ts (T10)"
//! ```
//!
//! Subjects use the query contract's spelling: `path`, `path#name`,
//! `path#Owner.member`; a dependency is `dep:<manifest path>:<name>`, a package
//! `pkg:<manifest path>:<name>`, a directory `dir:<path>`, an import
//! `import:<path>:<specifier as written>`, a suppression `suppression:<path>`. A known gap
//! is a ledger entry with teeth: the day the tree closes it, the check fails, so
//! the entry must be promoted to `dead`/`alive` in the same commit — the ledger
//! cannot rot.

use kndo_contract::finding::Finding;
use kndo_contract::subject::Subject;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectations {
    #[serde(default)]
    pub dead: Vec<Dead>,
    #[serde(default)]
    pub alive: Vec<Alive>,
    #[serde(default)]
    pub known_gap: Vec<KnownGap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dead {
    pub subject: String,
    #[serde(default)]
    pub category: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alive {
    pub subject: String,
    #[serde(default)]
    pub category: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownGap {
    pub subject: String,
    pub expect: Expect,
    #[serde(default)]
    pub category: Option<String>,
    pub why: String,
    pub fix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Expect {
    Dead,
    Alive,
}

/// The categories an `alive` claim guards against when it names none: the two
/// accusations that say "nothing beyond here uses this".
pub const ALIVE_CATEGORIES: &[&str] = &["unused", "internal-only"];

/// One reported finding, spelled the way an expectation names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported {
    pub category: String,
    pub subject: String,
}

impl Reported {
    pub fn of(finding: &Finding) -> Self {
        Reported {
            category: finding.category.as_str().to_string(),
            subject: spell(&finding.subject),
        }
    }
}

/// The expectation spelling of a subject — the query contract's, so `kndo
/// describe` and an expectation name a thing identically.
pub fn spell(subject: &Subject) -> String {
    match subject {
        Subject::File { path } => path.as_str().to_string(),
        Subject::Symbol { path, selector, .. } => {
            format!("{}#{}", path.as_str(), selector.render())
        }
        Subject::Dependency {
            owner_manifest,
            name,
        } => format!("dep:{}:{}", owner_manifest.as_str(), name),
        Subject::Package { manifest, name } => format!("pkg:{}:{}", manifest.as_str(), name),
        Subject::Directory { path } => format!("dir:{}", path.as_str()),
        // The identity part, not the display label: `import:src/a.js:./x`
        // keeps its spelling, and a second such import is `./x #2`.
        Subject::Import { path, .. } | Subject::Suppression { path, .. } => {
            let kind = subject.kind().as_str();
            format!("{kind}:{}:{}", path.as_str(), subject.identity_part())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// A `dead` claim nothing reported.
    DeadMissing { subject: String, why: String },
    /// An `alive` claim a finding accused.
    AliveReported {
        subject: String,
        category: String,
        why: String,
    },
    /// A known gap the tree closed: promote it in the same commit.
    GapClosed {
        subject: String,
        expect: Expect,
        fix: String,
    },
    /// A subject no file or declaration of the fixture spells.
    UnknownSubject { subject: String },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::DeadMissing { subject, why } => {
                write!(f, "expected `{subject}` reported ({why}) — nothing did")
            }
            Violation::AliveReported {
                subject,
                category,
                why,
            } => write!(
                f,
                "expected `{subject}` alive ({why}) — {category} accused it"
            ),
            Violation::GapClosed {
                subject,
                expect,
                fix,
            } => write!(
                f,
                "known gap on `{subject}` (expect {expect:?}, fix: {fix}) no longer holds — \
                 the tree closed it: promote the entry in this commit"
            ),
            Violation::UnknownSubject { subject } => {
                write!(f, "`{subject}` names nothing the fixture declares")
            }
        }
    }
}

impl Expectations {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// Every claim against the run. `exists` answers whether a subject spelling
    /// names something the fixture's graph holds (a file, a declaration); a
    /// dependency spelling is trusted as written.
    pub fn check(&self, reported: &[Reported], exists: &dyn Fn(&str) -> bool) -> Vec<Violation> {
        let mut out = Vec::new();
        // Manifest-, directory- and import-addressed subjects are trusted as
        // written: only files and declarations have a graph to check against.
        let trusted = ["dep:", "pkg:", "dir:", "import:", "suppression:"];
        let known =
            |subject: &str| trusted.iter().any(|p| subject.starts_with(p)) || exists(subject);
        let hit = |subject: &str, category: Option<&str>, defaults: &[&str]| {
            reported.iter().any(|r| {
                r.subject == subject
                    && match category {
                        Some(c) => r.category == c,
                        None => defaults.is_empty() || defaults.contains(&r.category.as_str()),
                    }
            })
        };
        for d in &self.dead {
            if !known(&d.subject) {
                out.push(Violation::UnknownSubject {
                    subject: d.subject.clone(),
                });
                continue;
            }
            if !hit(&d.subject, d.category.as_deref(), &[]) {
                out.push(Violation::DeadMissing {
                    subject: d.subject.clone(),
                    why: d.why.clone(),
                });
            }
        }
        for a in &self.alive {
            if !known(&a.subject) {
                out.push(Violation::UnknownSubject {
                    subject: a.subject.clone(),
                });
                continue;
            }
            let accused = reported.iter().find(|r| {
                r.subject == a.subject
                    && match a.category.as_deref() {
                        Some(c) => r.category == c,
                        None => ALIVE_CATEGORIES.contains(&r.category.as_str()),
                    }
            });
            if let Some(r) = accused {
                out.push(Violation::AliveReported {
                    subject: a.subject.clone(),
                    category: r.category.clone(),
                    why: a.why.clone(),
                });
            }
        }
        for g in &self.known_gap {
            if !known(&g.subject) {
                out.push(Violation::UnknownSubject {
                    subject: g.subject.clone(),
                });
                continue;
            }
            let reported_now = match g.expect {
                Expect::Dead => hit(&g.subject, g.category.as_deref(), &[]),
                Expect::Alive => hit(&g.subject, g.category.as_deref(), ALIVE_CATEGORIES),
            };
            // A gap expecting `dead` holds while nothing reports it; one expecting
            // `alive` holds while something still accuses it.
            let holds = match g.expect {
                Expect::Dead => !reported_now,
                Expect::Alive => reported_now,
            };
            if !holds {
                out.push(Violation::GapClosed {
                    subject: g.subject.clone(),
                    expect: g.expect,
                    fix: g.fix.clone(),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reported(pairs: &[(&str, &str)]) -> Vec<Reported> {
        pairs
            .iter()
            .map(|(c, s)| Reported {
                category: c.to_string(),
                subject: s.to_string(),
            })
            .collect()
    }

    #[test]
    fn claims_are_checked_and_gaps_have_teeth() {
        let e = Expectations::parse(
            r#"
[[dead]]
subject = "src/a.ts#dead"
why = "nothing names it"

[[alive]]
subject = "src/b.ts#used"
why = "called from a.ts"

[[known_gap]]
subject = "src/c.ts#hidden"
expect = "dead"
why = "kept by the export site"
fix = "M8.c"

[[known_gap]]
subject = "src/d.ts#wrongly"
expect = "alive"
why = "accused through a name collision"
fix = "M8.b"
"#,
        )
        .expect("parses");
        let all_exist = |_: &str| true;
        // The tree as the ledger describes it: no violations.
        let now = reported(&[("unused", "src/a.ts#dead"), ("unused", "src/d.ts#wrongly")]);
        assert!(e.check(&now, &all_exist).is_empty());
        // The dead claim unreported, the alive claim accused, both gaps closed.
        let later = reported(&[
            ("internal-only", "src/b.ts#used"),
            ("unused", "src/c.ts#hidden"),
        ]);
        let v = e.check(&later, &all_exist);
        assert_eq!(v.len(), 4, "{v:?}");
        assert!(
            matches!(&v[0], Violation::DeadMissing { subject, .. } if subject == "src/a.ts#dead")
        );
        assert!(
            matches!(&v[1], Violation::AliveReported { category, .. } if category == "internal-only")
        );
        assert!(matches!(
            &v[2],
            Violation::GapClosed {
                expect: Expect::Dead,
                ..
            }
        ));
        assert!(matches!(
            &v[3],
            Violation::GapClosed {
                expect: Expect::Alive,
                ..
            }
        ));
    }

    #[test]
    fn unknown_subjects_and_unknown_keys_are_refused() {
        let e = Expectations::parse("[[alive]]\nsubject = \"src/x.ts#nope\"\nwhy = \"typo\"\n")
            .expect("parses");
        let v = e.check(&[], &|_| false);
        assert!(
            matches!(&v[0], Violation::UnknownSubject { subject } if subject == "src/x.ts#nope")
        );
        assert!(Expectations::parse("[[dead]]\nsubjekt = \"x\"\nwhy = \"\"\n").is_err());
        assert!(
            Expectations::parse(
                "[[known_gap]]\nsubject = \"x\"\nexpect = \"maybe\"\nwhy = \"\"\nfix = \"\"\n"
            )
            .is_err()
        );
    }

    #[test]
    fn a_dependency_subject_is_trusted_as_written() {
        let e = Expectations::parse(
            "[[dead]]\nsubject = \"dep:package.json:lodash\"\nwhy = \"never imported\"\n",
        )
        .expect("parses");
        let now = reported(&[("unused", "dep:package.json:lodash")]);
        assert!(e.check(&now, &|_| false).is_empty());
    }
}
