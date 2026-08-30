//! Relative-specifier resolution against the project's known files, in Node/TS
//! candidate order: the path as written, extension candidates, the `.js`-names-the-
//! compiled-file swap, then directory `index.*`. The candidate list is fixed, so the
//! first hit is deterministic; anything unplaceable is `Unresolved` — keep-alive,
//! never an accusation.

use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;

const EXTS: [&str; 7] = [".ts", ".tsx", ".d.ts", ".js", ".jsx", ".mjs", ".cjs"];

pub fn resolve(from: &ProjectPath, specifier: &str, cx: &ResolveContext<'_>) -> Resolution {
    if !specifier.starts_with('.') {
        return Resolution::Unresolved;
    }
    let dir = match from.as_str().rfind('/') {
        Some(i) => &from.as_str()[..i],
        None => "",
    };
    let Some(joined) = normalize(dir, specifier) else {
        return Resolution::Unresolved;
    };
    for candidate in candidates(&joined) {
        let path = ProjectPath::new(candidate);
        if cx.contains(&path) {
            return Resolution::File(path);
        }
    }
    Resolution::Unresolved
}

/// Joins and collapses `.`/`..` segments. `None` when the specifier escapes the
/// project root — nothing inside the project can be meant.
fn normalize(dir: &str, spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

fn candidates(joined: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !joined.is_empty() {
        out.push(joined.to_string());
        for e in EXTS {
            out.push(format!("{joined}{e}"));
        }
        // In a TS project, `./x.js` names the COMPILED file; the source beside it is
        // the `.ts`/`.tsx`.
        if let Some(stem) = joined.strip_suffix(".js") {
            out.push(format!("{stem}.ts"));
            out.push(format!("{stem}.tsx"));
        } else if let Some(stem) = joined.strip_suffix(".jsx") {
            out.push(format!("{stem}.tsx"));
        }
    }
    let prefix = if joined.is_empty() {
        String::new()
    } else {
        format!("{joined}/")
    };
    for e in EXTS {
        out.push(format!("{prefix}index{e}"));
    }
    out
}
