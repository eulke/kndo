//! `cargo xtask package` — the one place that knows what a kndo release artifact is called
//! and what is inside it.
//!
//! Four channels consume those artifacts and none of them can call Rust: the installer shell
//! script, the GitHub Action, the Homebrew formula template, and the published install docs.
//! Before this module each of them spelled the name and assumed the layout on its own, and a
//! measurement of that arrangement found three of the four broken — the Action asked for
//! `-gnu` triples the release never builds *and* dropped the tag's leading `v`; the installer
//! copied `kndo` from the extraction root, where the staged directory means it never is; the
//! docs' copy-pasteable one-liner had both faults. None of it could be noticed before a tag was
//! pushed, because nothing in CI ever produced an artifact and then consumed one.
//!
//! So: this module is the producer (`release.yml` calls it instead of hand-rolling `tar` and
//! one `tar` line per platform), and `tests/release_channels.rs` reads the four consumers
//! and asserts each one spells exactly what [`artifact`] produces. A channel that drifts fails
//! a test on the PR that drifts it, not on the release that ships it.

use std::path::{Path, PathBuf};

/// How a target's archive is packed. Every released target gets `.tar.gz`.
///
/// This was an enum with a `Zip` arm for `x86_64-pc-windows-msvc`, dropped along with the
/// target (RFC 0014 §3.3): `tree-sitter-scss`'s build script passes `-Wno-unused-parameter` to
/// the compiler unconditionally, which `cl.exe` refuses, so the Windows binary could not be
/// built at all — a fact the `cross-platform` CI job surfaced before a tag ever ran the release
/// matrix. Kept as a single-variant type rather than deleted outright: the *shape* of "a target
/// declares how it is packed" is what the four consumers agree with, and it is what a second
/// format would slot back into. The `zip` writer itself is gone — dead code is not kept against
/// a maybe, which is the verdict kndo would report on it.
/// conventional expectation on each platform, and what every consumer already assumes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Archive {
    TarGz,
}

impl Archive {
    pub fn extension(self) -> &'static str {
        match self {
            Archive::TarGz => "tar.gz",
        }
    }
}

/// One released platform: everything the release pipeline and its consumers need to agree on.
pub struct Target {
    /// The Rust target triple. Also the last field of every artifact name.
    pub triple: &'static str,
    /// The GitHub runner that builds it.
    pub runner: &'static str,
    /// Built through `cross` rather than natively. musl needs it (a real static binary,
    /// independent of the host's glibc); macOS builds on its own runner.
    pub cross: bool,
    /// `(uname -s, uname -m)` pairs that must resolve to this triple. This is what makes the
    /// installer's and the Action's platform detection checkable: a triple nothing maps to is a
    /// triple nobody can install.
    pub uname: &'static [(&'static str, &'static str)],
    pub archive: Archive,
}

impl Target {
    /// The binary's file name inside the archive.
    ///
    /// One name for every target now that Windows is not published; it stays a method rather
    /// than a constant because it is the per-target question the consumers ask, and a target
    /// with a different convention would answer it differently.
    pub fn binary(&self) -> &'static str {
        "kndo"
    }
}

/// Every platform a release publishes. `release.yml`'s build matrix is checked against this
/// list, not written beside it.
///
/// Linux is **musl**, deliberately: a static binary that runs on any distribution regardless of
/// its glibc. Anything asking for `-gnu` is asking for an asset that does not exist.
pub const TARGETS: &[Target] = &[
    Target {
        triple: "x86_64-unknown-linux-musl",
        runner: "ubuntu-latest",
        cross: true,
        uname: &[("Linux", "x86_64")],
        archive: Archive::TarGz,
    },
    Target {
        triple: "aarch64-unknown-linux-musl",
        runner: "ubuntu-latest",
        cross: true,
        uname: &[("Linux", "aarch64"), ("Linux", "arm64")],
        archive: Archive::TarGz,
    },
    Target {
        triple: "x86_64-apple-darwin",
        runner: "macos-latest",
        cross: false,
        uname: &[("Darwin", "x86_64")],
        archive: Archive::TarGz,
    },
    Target {
        triple: "aarch64-apple-darwin",
        runner: "macos-latest",
        cross: false,
        uname: &[("Darwin", "arm64")],
        archive: Archive::TarGz,
    },
];

pub fn target(triple: &str) -> Option<&'static Target> {
    TARGETS.iter().find(|t| t.triple == triple)
}

/// Files that accompany the binary in every archive, relative to the workspace root.
pub const EXTRA_FILES: &[&str] = &["LICENSE", "README.md"];

/// The name and shape of one release artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// The single directory the archive contains, and the archive's own base name:
    /// `kndo-v1.2.0-aarch64-apple-darwin`.
    pub stem: String,
    /// The archive file name: the stem plus `.tar.gz`.
    pub file_name: String,
    /// The binary's path *inside* the archive. Every archive nests its contents under `stem`,
    /// which is why every consumer must strip exactly one leading component.
    pub binary_in_archive: String,
}

/// **The name and layout of a release artifact.** `tag` is the git tag verbatim, leading `v`
/// included (`v1.2.0`) — that is what `GITHUB_REF_NAME` holds and therefore what the producer
/// has always written; a consumer that strips it asks for a file that was never uploaded.
pub fn artifact(tag: &str, target: &Target) -> Artifact {
    let stem = format!("kndo-{tag}-{}", target.triple);
    Artifact {
        file_name: format!("{stem}.{}", target.archive.extension()),
        binary_in_archive: format!("{stem}/{}", target.binary()),
        stem,
    }
}

/// Stage and archive one release artifact. Returns the archive's path.
pub fn package(
    workspace: &Path,
    tag: &str,
    target: &Target,
    binary: &Path,
    out_dir: &Path,
) -> Result<PathBuf, String> {
    let art = artifact(tag, target);
    if !binary.is_file() {
        return Err(format!(
            "no binary at {} — build it first (cargo build --release --locked -p kndo-cli \
             --target {})",
            binary.display(),
            target.triple
        ));
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let archive_path = out_dir.join(&art.file_name);
    // A stale archive from a previous run must not survive into this one: `zip` appends
    // nothing but `tar` writing over a shorter file would leave the tail of the old one.
    let _ = std::fs::remove_file(&archive_path);

    let mut entries: Vec<(String, PathBuf)> =
        vec![(art.binary_in_archive.clone(), binary.to_path_buf())];
    for extra in EXTRA_FILES {
        let src = workspace.join(extra);
        if !src.is_file() {
            return Err(format!(
                "{} is missing from the workspace root",
                src.display()
            ));
        }
        entries.push((format!("{}/{extra}", art.stem), src));
    }

    match target.archive {
        Archive::TarGz => write_tar_gz(&archive_path, &entries)?,
    }
    Ok(archive_path)
}

fn write_tar_gz(path: &Path, entries: &[(String, PathBuf)]) -> Result<(), String> {
    let file = std::fs::File::create(path)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for (name, src) in entries {
        builder
            .append_path_with_name(src, name)
            .map_err(|e| format!("cannot add {} as {name}: {e}", src.display()))?;
    }
    builder
        .into_inner()
        .map_err(|e| format!("cannot finish {}: {e}", path.display()))?
        .finish()
        .map_err(|e| format!("cannot finish {}: {e}", path.display()))?;
    Ok(())
}

/// The `cargo xtask package` command line, resolved into a call to [`package`].
///
/// Lives here rather than in `main.rs` for the reason `lib.rs` states outright — "anything a
/// test needs to assert about a task lives here". It did not, and the proof was mechanical:
/// kndo's own `crap` analysis put this function at **0% coverage** on this repository. Argument
/// handling is where a release command goes wrong quietly (a defaulted tag, a target that is
/// not in the table, a binary path assembled from the wrong triple), so it is exactly the part
/// that should be callable from a test.
///
/// `root` is passed in rather than discovered so a test can point it at a fixture.
pub fn from_args(args: &[String], root: Result<PathBuf, String>) -> Result<PathBuf, String> {
    let flag = |name: &str| -> Option<&str> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
    };
    let triple = flag("--target").ok_or("--target <triple> is required")?;
    let target = target(triple).ok_or_else(|| {
        format!(
            "unknown target {triple} — releases build: {}",
            TARGETS
                .iter()
                .map(|t| t.triple)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let root = root?;
    // Default to the workspace version with the `v` the git tag carries, so a local
    // `cargo xtask package` produces exactly the name a release would.
    let owned_tag;
    let tag = match flag("--tag") {
        Some(t) => t,
        None => {
            owned_tag = format!("v{}", workspace_version(&root)?);
            &owned_tag
        }
    };
    let bin = flag("--bin")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            root.join("target")
                .join(triple)
                .join("release")
                .join(target.binary())
        });
    let out_dir = root.join(flag("--out-dir").unwrap_or("dist"));
    package(&root, tag, target, &bin, &out_dir)
}

/// The workspace's own `version`, read from the root manifest — the same string
/// `kndo --version` prints, so a locally packaged artifact is named like its release.
pub(crate) fn workspace_version(root: &std::path::Path) -> Result<String, String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| format!("cannot read the workspace manifest: {e}"))?;
    manifest
        .lines()
        .skip_while(|l| l.trim() != "[workspace.package]")
        .find_map(|l| l.strip_prefix("version = "))
        .map(|v| v.trim().trim_matches('"').to_string())
        .ok_or_else(|| "no [workspace.package] version in the workspace manifest".to_string())
}
