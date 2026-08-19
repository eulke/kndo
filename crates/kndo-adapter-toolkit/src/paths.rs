//! Pure `ProjectPath` string manipulation shared by every adapter's resolver.
//!
//! Nothing here touches the filesystem (the adapter purity rule, RFC 0002 §6) and nothing is
//! language-specific: relative-specifier joining is the same operation for a TS `import`, a
//! CSS `@import`, or a Go relative path. One implementation, tested once.

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
