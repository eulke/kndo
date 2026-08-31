//! Manifest dependencies for activation: the NAMES a `pom.xml` or Gradle build
//! script declares, each under both spellings a rule's author might write — the
//! full `groupId:artifactId` coordinate and the artifact id alone. Matching the
//! bare artifact id can in principle match two groups publishing the same name;
//! that is the keep-alive direction (an extension activating for a project that
//! uses a same-named artifact can only contribute roots nobody asked for, never
//! invent a finding), and it is the only spelling an author can reasonably be
//! expected to write.
//!
//! Both scanners are deliberately shallow — line-shaped scans over
//! machine-regular files, not XML/Groovy parsers: dependency NAMES are all
//! activation reads, and everything else in these files is none of this
//! adapter's business.

use kndo_contract::adapter::SourceFile;
use smol_str::SmolStr;

pub fn dependencies(manifest: &SourceFile<'_>) -> Vec<SmolStr> {
    let name = manifest
        .path
        .as_str()
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let Ok(text) = std::str::from_utf8(manifest.content) else {
        return Vec::new();
    };
    let mut out = match name {
        "pom.xml" => maven(text),
        "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts" => {
            gradle(text)
        }
        _ => Vec::new(),
    };
    out.sort();
    out.dedup();
    out
}

/// `<dependency>` blocks inside `<dependencies>`: pair each `<groupId>` with
/// its `<artifactId>` in document order.
fn maven(text: &str) -> Vec<SmolStr> {
    let mut out = Vec::new();
    let mut in_dependencies = false;
    let mut group: Option<&str> = None;
    let mut artifact: Option<&str> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.contains("<dependencies>") {
            in_dependencies = true;
        }
        if line.contains("</dependencies>") {
            in_dependencies = false;
        }
        if !in_dependencies {
            continue;
        }
        if line.contains("<dependency>") || line.contains("</dependency>") {
            group = None;
            artifact = None;
        }
        if let Some(v) = tag_value(line, "groupId") {
            group = Some(v);
        }
        if let Some(v) = tag_value(line, "artifactId") {
            artifact = Some(v);
        }
        if let (Some(g), Some(a)) = (group, artifact) {
            out.push(SmolStr::new(format!("{g}:{a}")));
            out.push(SmolStr::new(a));
            group = None;
            artifact = None;
        }
    }
    out
}

fn tag_value<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = line.find(&open)? + open.len();
    let end = line.find(&close)?;
    (start <= end).then(|| line[start..end].trim())
}

/// Quoted `group:artifact[:version]` coordinates anywhere in the script — the
/// shape every dependency notation shares (`implementation "g:a:v"`,
/// `api('g:a')`, version catalogs excluded by their own syntax).
fn gradle(text: &str) -> Vec<SmolStr> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        for quote in ['"', '\''] {
            let mut rest = line;
            while let Some(start) = rest.find(quote) {
                let after = &rest[start + 1..];
                let Some(end) = after.find(quote) else {
                    break;
                };
                let literal = &after[..end];
                rest = &after[end + 1..];
                let mut parts = literal.split(':');
                if let (Some(g), Some(a)) = (parts.next(), parts.next()) {
                    let extra = parts.next();
                    let well_formed = !g.is_empty()
                        && !a.is_empty()
                        && parts.next().is_none()
                        && g.chars().all(|c| c.is_alphanumeric() || ".-_".contains(c))
                        && a.chars().all(|c| c.is_alphanumeric() || ".-_".contains(c))
                        && extra.is_none_or(|v| !v.is_empty());
                    if well_formed {
                        out.push(SmolStr::new(format!("{g}:{a}")));
                        out.push(SmolStr::new(a));
                    }
                }
            }
        }
    }
    out
}
