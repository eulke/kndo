//! Textual detection of serde trait impl headers. Deterministic,
//! single-line scan — idiomatic code writes the header on one line
//! (`impl<'a> serde::Serialize for Message<'a> {`); a hand-wrapped header simply contributes
//! nothing (degrade toward silence).

/// Which serde trait an impl header names — decides WHICH members the machinery invokes.
/// Module-private: the lib boundary trades in plain `(owner, member)` pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SerdeTrait {
    Serialize,
    Deserialize,
    Visitor,
}

/// Every `(trait, owner base name)` pair the source's serde impl headers declare:
/// `impl Serialize for Config` → `(Serialize, "Config")`, `impl<'a> serde::Serialize for
/// Message<'a>` → `(Serialize, "Message")`, `impl<'de> Visitor<'de> for ConfigVisitor` →
/// `(Visitor, "ConfigVisitor")`. The trait matches by its path's LAST segment, so both bare
/// and `serde::`/`serde::de::`-qualified forms land; a same-named local trait would
/// over-mark at `Probable` — silence-direction only, and gated anyway on the manifest
/// actually depending on serde.
fn serde_impl_owners(source: &str) -> Vec<(SerdeTrait, String)> {
    let mut out = Vec::new();
    for line in source.lines() {
        let Some(rest) = line.trim_start().strip_prefix("impl") else {
            continue;
        };
        // Skip the generics list, angle-bracket balanced (`impl<'de, T: Serialize>`).
        let rest = skip_generics(rest);
        let Some((trait_part, type_part)) = rest.split_once(" for ") else {
            continue;
        };
        let Some(kind) = serde_trait(trait_part) else {
            continue;
        };
        if let Some(owner) = base_ident(type_part) {
            out.push((kind, owner.to_string()));
        }
    }
    out
}

/// The subset of `members` (`(owner, member)` pairs from the graph's symbol table) that
/// the source's serde impl headers make machinery-invoked — the plugin's whole answer for
/// one file, so the trait/member curation never crosses the module boundary.
pub(crate) fn machinery_marks<'a>(
    source: &str,
    members: &[(&'a str, &'a str)],
) -> Vec<(&'a str, &'a str)> {
    let impls = serde_impl_owners(source);
    members
        .iter()
        .filter(|(owner, member)| {
            impls
                .iter()
                .any(|(kind, t)| t == owner && machinery_members(*kind, member))
        })
        .copied()
        .collect()
}

/// The members a serde trait's machinery invokes on an implementor — curated per trait:
/// never called by name from user code, always through serde's dispatch.
fn machinery_members(kind: SerdeTrait, member: &str) -> bool {
    match kind {
        SerdeTrait::Serialize => member == "serialize",
        SerdeTrait::Deserialize => deserialize_member(member),
        SerdeTrait::Visitor => visitor_member(member),
    }
}

fn deserialize_member(member: &str) -> bool {
    member == "deserialize" || member == "deserialize_in_place"
}

fn visitor_member(member: &str) -> bool {
    member == "expecting" || member.starts_with("visit_")
}

fn skip_generics(rest: &str) -> &str {
    let rest = rest.trim_start();
    if !rest.starts_with('<') {
        return rest;
    }
    match generics_end(rest.as_bytes()) {
        Some(end) => rest[end + 1..].trim_start(),
        None => "",
    }
}

/// The index of the `>` closing the angle-bracket group opening at byte 0.
fn generics_end(bytes: &[u8]) -> Option<usize> {
    let mut depth = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'<' {
            depth += 1;
        } else if b == b'>' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

const SERDE_TRAITS: [(&str, SerdeTrait); 4] = [
    ("Serialize", SerdeTrait::Serialize),
    ("Deserialize", SerdeTrait::Deserialize),
    ("DeserializeSeed", SerdeTrait::Deserialize),
    ("Visitor", SerdeTrait::Visitor),
];

fn serde_trait(trait_part: &str) -> Option<SerdeTrait> {
    let last = trait_part.trim().rsplit("::").next()?;
    let base = last.split('<').next()?.trim();
    SERDE_TRAITS
        .iter()
        .find(|(name, _)| *name == base)
        .map(|&(_, kind)| kind)
}

fn base_ident(type_part: &str) -> Option<&str> {
    let t = type_part.trim();
    let end = t
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
        .unwrap_or(t.len());
    t[..end].rsplit("::").next().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_supported_impl_header_shapes_parse() {
        let src = "impl Serialize for Config {\n\
                   impl<'a> serde::Serialize for Message<'a> {\n\
                   impl<'de> Deserialize<'de> for ConfigSet {\n\
                   impl<'de> Visitor<'de> for ConfigVisitor {\n\
                   impl<'de, T: Clone> serde::de::Visitor<'de> for Wrap<T> {\n\
                   impl Display for NotSerde {\n\
                   impl Widget {\n";
        let owners = serde_impl_owners(src);
        assert_eq!(
            owners,
            vec![
                (SerdeTrait::Serialize, "Config".to_string()),
                (SerdeTrait::Serialize, "Message".to_string()),
                (SerdeTrait::Deserialize, "ConfigSet".to_string()),
                (SerdeTrait::Visitor, "ConfigVisitor".to_string()),
                (SerdeTrait::Visitor, "Wrap".to_string()),
            ]
        );
    }

    #[test]
    fn machinery_members_are_curated_per_trait() {
        assert!(machinery_members(SerdeTrait::Serialize, "serialize"));
        assert!(!machinery_members(SerdeTrait::Serialize, "helper"));
        assert!(machinery_members(SerdeTrait::Deserialize, "deserialize"));
        assert!(machinery_members(SerdeTrait::Visitor, "expecting"));
        assert!(machinery_members(SerdeTrait::Visitor, "visit_str"));
        assert!(!machinery_members(SerdeTrait::Visitor, "serialize"));
    }
}
