//! Pure `ProjectPath` string manipulation.
//!
//! Nothing here touches the filesystem (the adapter purity rule) and nothing is
//! language-specific: relative-specifier joining is the same operation for a TS `import`, a
//! CSS `@import`, a Go relative path, or the design-time `src="../static/…"` a Thymeleaf
//! template carries. It lived in `kndo-adapter-toolkit` while adapters were its only audience;
//! a PLUGIN needing the same arithmetic is what moved it here, since the toolkit's audience is
//! adapters and copying twenty lines into a second crate is the duplication this codebase
//! forbids. `kndo_adapter_toolkit::paths` re-exports it, so every adapter call site is
//! unchanged. One implementation, tested once.

/// Directory part of a `/`-separated project path (`""` for root-level files).
pub fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Joins a specifier onto a base directory, normalizing `.` and `..` segments. A leading `/`
/// restarts from the project root. Pure string manipulation.
pub fn join(base_dir: &str, spec: &str) -> String {
    let mut stack: Vec<&str> = if base_dir.is_empty() {
        vec![]
    } else {
        base_dir.split('/').collect()
    };
    let spec = match spec.strip_prefix('/') {
        Some(rest) => {
            stack.clear();
            rest
        }
        None => spec,
    };
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            s => stack.push(s),
        }
    }
    stack.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirname_handles_nested_and_root() {
        assert_eq!(dirname("src/mod/a.ts"), "src/mod");
        assert_eq!(dirname("a.ts"), "");
    }

    #[test]
    fn join_normalizes_dot_segments() {
        assert_eq!(join("src/mod", "./b"), "src/mod/b");
        assert_eq!(join("src/mod", "../shared"), "src/shared");
        assert_eq!(join("src", "../../escape"), "escape");
        assert_eq!(join("src", "/from-root"), "from-root");
        assert_eq!(join("", "./top"), "top");
    }
}
