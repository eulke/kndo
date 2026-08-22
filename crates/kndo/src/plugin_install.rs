//! `kndo plugin install/list/remove` — RFC 0015 §4: registry semantics without a registry
//! service. A coordinate (`github.com/<owner>/<repo>[@vX.Y.Z]`) resolves to a GitHub release
//! of that repo carrying a componentized `.wasm` plus a `checksums.txt` (the same artifact
//! convention kndo itself releases under, RFC 0014 §3 / docs/plugins/authoring.md §9);
//! everything verified lands in the global plugin directory beside a `plugins.lock` that makes
//! the installed set reproducible and auditable.
//!
//! Verification before any write: checksum (SHA-256 against the release's own manifest) and
//! identity binding (RFC 0015 §2 — the fetched component's descriptor must declare exactly the
//! coordinate it was fetched *by*, so nothing can impersonate an id it wasn't fetched from).
//! Dependencies recurse (`kndo:*` resolve as no-ops against this build's built-ins); a whole
//! transaction stages first and commits last, so a failure anywhere leaves the directory and
//! lockfile untouched.
//!
//! Network and WASM probing are injected (`ReleaseSource` + a probe closure) so every policy
//! in this module — conflict handling, staging atomicity, lockfile shape — is testable without
//! either.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- coordinates

/// A parsed install coordinate (RFC 0015 §2): `github.com/<owner>/<repo>`, optionally
/// `@<tag>`-qualified. The host part is fixed to GitHub in v1 (the RFC leaves the grammar
/// extensible; nothing here assumes otherwise).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coordinate {
    pub owner: String,
    pub repo: String,
    pub tag: Option<String>,
}

impl Coordinate {
    pub fn parse(spec: &str) -> Result<Coordinate, InstallError> {
        if kndo_core::plugin::is_reserved_id(spec) {
            return Err(InstallError::BuiltinCoordinate(spec.to_string()));
        }
        let (id, tag) = split_tag(spec)?;
        let (owner, repo) = split_owner_repo(id, spec)?;
        Ok(Coordinate { owner, repo, tag })
    }

    /// The identity half — what the fetched descriptor must declare as its `id` (RFC 0015 §2),
    /// and the lockfile key. Never carries the tag.
    pub fn id(&self) -> String {
        format!("github.com/{}/{}", self.owner, self.repo)
    }

    /// Deterministic on-disk name in the global directory. `/` is not portable in a file name;
    /// `__` never appears in a GitHub owner/repo (single underscores can), so the mapping is
    /// unambiguous both ways.
    fn file_name(&self) -> String {
        format!("github.com__{}__{}.wasm", self.owner, self.repo)
    }
}

/// `"id@tag"` → `(id, Some(tag))`; a trailing bare `@` is malformed, not "no tag".
fn split_tag(spec: &str) -> Result<(&str, Option<String>), InstallError> {
    match spec.split_once('@') {
        Some((id, tag)) if !tag.is_empty() => Ok((id, Some(tag.to_string()))),
        Some(_) => Err(InstallError::BadCoordinate(spec.to_string())),
        None => Ok((spec, None)),
    }
}

fn split_owner_repo(id: &str, spec: &str) -> Result<(String, String), InstallError> {
    let segments: Vec<&str> = id.split('/').collect();
    match segments.as_slice() {
        ["github.com", owner, repo] if !owner.is_empty() && !repo.is_empty() => {
            Ok((owner.to_string(), repo.to_string()))
        }
        _ => Err(InstallError::BadCoordinate(spec.to_string())),
    }
}

// ---------------------------------------------------------------- errors

#[derive(Debug)]
pub enum InstallError {
    BadCoordinate(String),
    BuiltinCoordinate(String),
    NoGlobalDir,
    /// Two different explicit tags were required for the same coordinate in one transaction —
    /// RFC 0015 §4: fail with both requirers named, never guess.
    VersionConflict {
        id: String,
        first: (String, String),
        second: (String, String),
    },
    /// An explicit tag disagrees with what's already installed (the cross-transaction variant
    /// of the same policy).
    InstalledVersionMismatch {
        id: String,
        installed: String,
        requested: String,
        requirer: String,
    },
    Fetch(String),
    ReleaseShape {
        id: String,
        problem: String,
    },
    ChecksumMismatch {
        asset: String,
        expected: String,
        actual: String,
    },
    /// Identity binding (RFC 0015 §2): the component's descriptor id is not the coordinate it
    /// was fetched by.
    IdentityMismatch {
        coordinate: String,
        declared: String,
    },
    Probe(String),
    NotInstalled(String),
    Io(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Grouped by concern so each helper stays a small match (the dogfooded CRAP
        // discipline); exactly one group owns each variant.
        let message = self
            .usage_message()
            .or_else(|| self.policy_message())
            .or_else(|| self.verification_message())
            .unwrap_or_else(|| self.transport_message());
        f.write_str(&message)
    }
}

impl InstallError {
    /// The caller asked for something malformed or unsatisfiable before any I/O happened.
    fn usage_message(&self) -> Option<String> {
        match self {
            InstallError::BadCoordinate(s) => Some(format!(
                "`{s}` is not an install coordinate — expected github.com/<owner>/<repo> \
                 optionally followed by @<tag> (RFC 0015 §2)"
            )),
            InstallError::BuiltinCoordinate(s) => Some(format!(
                "`{s}` names a built-in plugin: it is compiled into this kndo and never \
                 installed — see `kndo doctor` for whether it activates here"
            )),
            InstallError::NoGlobalDir => Some(
                "no global plugin directory could be determined (no platform data dir and no \
                 KNDO_PLUGIN_DIR override)"
                    .to_string(),
            ),
            _ => None,
        }
    }

    /// RFC 0015 §4's never-guess policies: version disagreements and unmanaged removals.
    fn policy_message(&self) -> Option<String> {
        match self {
            InstallError::VersionConflict { id, first, second } => Some(format!(
                "version conflict for {id}: {} requires {} but {} requires {} — kndo does not \
                 guess between explicit tags (RFC 0015 §4); align the requirers and retry",
                first.1, first.0, second.1, second.0
            )),
            InstallError::InstalledVersionMismatch {
                id,
                installed,
                requested,
                requirer,
            } => Some(format!(
                "{id} is already installed at {installed}, but {requirer} requires {requested} \
                 — `kndo plugin remove {id}` first if the change is intended"
            )),
            InstallError::NotInstalled(s) => Some(format!(
                "{s} is not in plugins.lock — only plugins installed by `kndo plugin install` \
                 can be removed by it (hand-dropped files are yours to manage)"
            )),
            _ => None,
        }
    }

    /// The download arrived but failed a trust gate — always "nothing was installed".
    fn verification_message(&self) -> Option<String> {
        match self {
            InstallError::ChecksumMismatch {
                asset,
                expected,
                actual,
            } => Some(format!(
                "checksum mismatch for {asset}: checksums.txt says {expected}, downloaded \
                 bytes hash to {actual} — nothing was installed"
            )),
            InstallError::IdentityMismatch {
                coordinate,
                declared,
            } => Some(format!(
                "identity mismatch: the component fetched by {coordinate} declares id \
                 `{declared}` — a plugin must declare exactly the coordinate it is fetched \
                 from (RFC 0015 §2); nothing was installed"
            )),
            InstallError::Probe(s) => Some(format!("component rejected: {s}")),
            _ => None,
        }
    }

    /// Everything that reaches or leaves the machine: network, release shape, filesystem.
    fn transport_message(&self) -> String {
        match self {
            InstallError::Fetch(s) => format!("fetch failed: {s}"),
            InstallError::ReleaseShape { id, problem } => format!(
                "release of {id} is not installer-ready: {problem} (expected one .wasm asset \
                 plus checksums.txt — docs/plugins/authoring.md §9)"
            ),
            InstallError::Io(s) => s.clone(),
            _ => unreachable!("every other variant is owned by an earlier message group"),
        }
    }
}

impl std::error::Error for InstallError {}

// ---------------------------------------------------------------- injected edges

/// One release asset, as the source lists it. `url` is whatever the same source's `download`
/// accepts — for GitHub, the API asset URL (works for public and private repos alike).
#[derive(Debug, Clone)]
pub struct AssetInfo {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag: String,
    pub assets: Vec<AssetInfo>,
}

/// Where releases come from — the network edge, injected so the transaction logic tests
/// without one. The one real implementation is [`GitHubReleaseSource`].
pub trait ReleaseSource {
    fn resolve(&self, coord: &Coordinate) -> Result<ReleaseInfo, InstallError>;
    fn download(&self, asset: &AssetInfo) -> Result<Vec<u8>, InstallError>;
}

/// What identity binding needs from a probed component — a strict subset of
/// `PluginDescriptor`, so tests can fake the probe without a real WASM build.
#[derive(Debug, Clone)]
pub struct ProbedDescriptor {
    pub id: String,
    pub version: String,
    pub dependencies: Vec<String>,
}

// ---------------------------------------------------------------- lockfile

/// `plugins.lock`, beside the installed `.wasm` files (RFC 0015 §4): coordinate → version →
/// sha256 → file. TOML, written sorted (BTreeMap) so the file is diff-stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockEntry {
    pub version: String,
    pub sha256: String,
    pub file: String,
}

pub type Lock = BTreeMap<String, LockEntry>;

const LOCK_FILE: &str = "plugins.lock";

pub fn read_lock(dir: &Path) -> Result<Lock, InstallError> {
    let path = dir.join(LOCK_FILE);
    if !path.is_file() {
        return Ok(Lock::new());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| InstallError::Io(format!("reading {}: {e}", path.display())))?;
    let table: toml::Table = content
        .parse()
        .map_err(|e| InstallError::Io(format!("parsing {}: {e}", path.display())))?;
    Ok(table
        .into_iter()
        .filter_map(|(id, v)| lock_entry(&v).map(|entry| (id, entry)))
        .collect())
}

fn lock_entry(value: &toml::Value) -> Option<LockEntry> {
    let get = |key: &str| value.get(key)?.as_str().map(str::to_string);
    Some(LockEntry {
        version: get("version")?,
        sha256: get("sha256")?,
        file: get("file")?,
    })
}

fn write_lock(dir: &Path, lock: &Lock) -> Result<(), InstallError> {
    let mut table = toml::Table::new();
    for (id, entry) in lock {
        let mut e = toml::Table::new();
        e.insert("version".into(), toml::Value::String(entry.version.clone()));
        e.insert("sha256".into(), toml::Value::String(entry.sha256.clone()));
        e.insert("file".into(), toml::Value::String(entry.file.clone()));
        table.insert(id.clone(), toml::Value::Table(e));
    }
    let path = dir.join(LOCK_FILE);
    std::fs::write(
        &path,
        toml::to_string(&table).expect("toml tables always serialize"),
    )
    .map_err(|e| InstallError::Io(format!("writing {}: {e}", path.display())))
}

// ---------------------------------------------------------------- install

/// What one install transaction did — everything `kndo plugin install` prints comes from here.
#[derive(Debug, Default)]
pub struct InstallReport {
    /// `(coordinate id, tag)` actually written this transaction, in install order.
    pub installed: Vec<(String, String)>,
    /// Coordinates that were already satisfied (locked at a compatible version).
    pub already_present: Vec<String>,
    /// `kndo:*` dependencies that resolved against this build's built-ins (no-ops).
    pub builtin_deps: Vec<String>,
    /// `kndo:*` dependencies this build does *not* compile in — never fatal (RFC 0015 §3),
    /// but worth a line: those conventions won't be analyzed.
    pub unknown_builtins: Vec<String>,
}

/// Install `spec` (and its transitive dependencies) into the global plugin directory, using
/// the real GitHub source and the real WASM probe. The library-level entry `kndo plugin
/// install` calls.
pub fn install(spec: &str) -> Result<InstallReport, InstallError> {
    let dir = global_dir()?;
    let builtin_ids: Vec<String> = crate::default_plugins()
        .iter()
        .map(|p| p.descriptor().id.to_string())
        .collect();
    install_with(
        spec,
        &dir,
        &GitHubReleaseSource::from_env(),
        wasm_probe,
        &builtin_ids,
    )
}

/// The whole transaction, with its edges injected. Stage-then-commit: every fetch, checksum,
/// and identity check happens before the first byte lands in `dir` or the lockfile changes.
pub fn install_with(
    spec: &str,
    dir: &Path,
    source: &dyn ReleaseSource,
    probe: impl Fn(&[u8]) -> Result<ProbedDescriptor, String>,
    builtin_ids: &[String],
) -> Result<InstallReport, InstallError> {
    Coordinate::parse(spec)?; // validate the typed spec before any I/O
    let mut tx = Transaction {
        source,
        probe: &probe,
        builtin_ids,
        lock: read_lock(dir)?,
        report: InstallReport::default(),
        staged: Vec::new(),
        requested: BTreeMap::new(),
        queue: vec![(spec.to_string(), "the command line".to_string())],
    };
    tx.run()?;
    let Transaction {
        mut lock,
        mut report,
        staged,
        ..
    } = tx;
    commit(dir, staged, &mut lock, &mut report)?;
    Ok(report)
}

/// One staged component, fully verified: `(coordinate, release tag, bytes, sha256)`.
type Staged = (Coordinate, String, Vec<u8>, String);

/// One install transaction's working state — the worklist recursion over `dependencies`
/// (RFC 0015 §4 step 3) with the §4 conflict ledger (`requested`).
struct Transaction<'a> {
    source: &'a dyn ReleaseSource,
    probe: &'a dyn Fn(&[u8]) -> Result<ProbedDescriptor, String>,
    builtin_ids: &'a [String],
    lock: Lock,
    report: InstallReport,
    staged: Vec<Staged>,
    /// id → (explicit tag or "", requirer) for everything this transaction asked for.
    requested: BTreeMap<String, (String, String)>,
    queue: Vec<(String, String)>,
}

impl Transaction<'_> {
    fn run(&mut self) -> Result<(), InstallError> {
        while let Some((spec, requirer)) = self.queue.pop() {
            self.handle(&spec, &requirer)?;
        }
        Ok(())
    }

    fn handle(&mut self, spec: &str, requirer: &str) -> Result<(), InstallError> {
        if let Some(coord) = self.admitted(spec, requirer)? {
            self.stage(coord)?;
        }
        Ok(())
    }

    /// Everything that can rule a request out *before* the network: built-in no-ops,
    /// malformed coordinates, duplicates within the transaction, already-installed.
    fn admitted(&mut self, spec: &str, requirer: &str) -> Result<Option<Coordinate>, InstallError> {
        if kndo_core::plugin::is_reserved_id(spec) {
            note_builtin(spec, self.builtin_ids, &mut self.report);
            return Ok(None);
        }
        let coord = Coordinate::parse(spec)?;
        Ok(self.wanted(&coord, requirer)?.then_some(coord))
    }

    fn wanted(&mut self, coord: &Coordinate, requirer: &str) -> Result<bool, InstallError> {
        if !record_request(coord, requirer, &mut self.requested)? {
            return Ok(false); // same coordinate already being handled this transaction
        }
        let satisfied = satisfied_by_lock(coord, requirer, &self.lock, &mut self.report)?;
        Ok(!satisfied)
    }

    /// Fetch, verify (checksum + identity binding), enqueue dependencies, stage.
    fn stage(&mut self, coord: Coordinate) -> Result<(), InstallError> {
        let (tag, bytes, sha) = fetch_verified(&coord, self.source)?;
        let descriptor = (self.probe)(&bytes).map_err(InstallError::Probe)?;
        if descriptor.id != coord.id() {
            return Err(InstallError::IdentityMismatch {
                coordinate: coord.id(),
                declared: descriptor.id,
            });
        }
        for dep in descriptor.dependencies {
            self.queue.push((dep, coord.id()));
        }
        self.staged.push((coord, tag, bytes, sha));
        Ok(())
    }
}

fn note_builtin(dep: &str, builtin_ids: &[String], report: &mut InstallReport) {
    if builtin_ids.iter().any(|b| b == dep) {
        report.builtin_deps.push(dep.to_string());
    } else {
        report.unknown_builtins.push(dep.to_string());
    }
}

/// A bare request is compatible with anything; explicit tags must agree.
fn tags_compatible(a: &str, b: &str) -> bool {
    a.is_empty() || b.is_empty() || a == b
}

/// Track what this transaction asked for; `Ok(false)` = duplicate request, already compatible.
fn record_request(
    coord: &Coordinate,
    requirer: &str,
    requested: &mut BTreeMap<String, (String, String)>,
) -> Result<bool, InstallError> {
    let tag = coord.tag.clone().unwrap_or_default();
    match requested.get(&coord.id()) {
        None => {
            requested.insert(coord.id(), (tag, requirer.to_string()));
            Ok(true)
        }
        Some((prior, _)) if tags_compatible(prior, &tag) => Ok(false),
        Some((prior, prior_requirer)) => Err(InstallError::VersionConflict {
            id: coord.id(),
            first: (prior.clone(), prior_requirer.clone()),
            second: (tag, requirer.to_string()),
        }),
    }
}

/// `Ok(true)` = already installed at a compatible version (no-op, RFC 0015 §4 step 3).
fn satisfied_by_lock(
    coord: &Coordinate,
    requirer: &str,
    lock: &Lock,
    report: &mut InstallReport,
) -> Result<bool, InstallError> {
    let Some(entry) = lock.get(&coord.id()) else {
        return Ok(false);
    };
    match &coord.tag {
        Some(tag) if *tag != entry.version => Err(InstallError::InstalledVersionMismatch {
            id: coord.id(),
            installed: entry.version.clone(),
            requested: tag.clone(),
            requirer: requirer.to_string(),
        }),
        _ => {
            report.already_present.push(coord.id());
            Ok(true)
        }
    }
}

/// Resolve, download, and checksum-verify one coordinate's release: `(tag, wasm bytes, sha256)`.
fn fetch_verified(
    coord: &Coordinate,
    source: &dyn ReleaseSource,
) -> Result<(String, Vec<u8>, String), InstallError> {
    let release = source.resolve(coord)?;
    let (wasm_asset, checksums_asset) = installer_assets(coord, &release)?;
    let expected = fetch_expected_sha(coord, source, checksums_asset, &wasm_asset.name)?;
    let (bytes, sha) = download_verified(source, wasm_asset, &expected)?;
    Ok((release.tag.clone(), bytes, sha))
}

fn fetch_expected_sha(
    coord: &Coordinate,
    source: &dyn ReleaseSource,
    checksums_asset: &AssetInfo,
    wasm_name: &str,
) -> Result<String, InstallError> {
    let checksums = source.download(checksums_asset)?;
    expected_sha(coord, &checksums, wasm_name)
}

fn download_verified(
    source: &dyn ReleaseSource,
    asset: &AssetInfo,
    expected: &str,
) -> Result<(Vec<u8>, String), InstallError> {
    let bytes = source.download(asset)?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(InstallError::ChecksumMismatch {
            asset: asset.name.clone(),
            expected: expected.to_string(),
            actual,
        });
    }
    Ok((bytes, actual))
}

/// The authoring.md §9 shape: exactly one `.wasm` asset, plus `checksums.txt`.
fn installer_assets<'r>(
    coord: &Coordinate,
    release: &'r ReleaseInfo,
) -> Result<(&'r AssetInfo, &'r AssetInfo), InstallError> {
    let wasm: Vec<&AssetInfo> = release
        .assets
        .iter()
        .filter(|a| a.name.ends_with(".wasm"))
        .collect();
    let shape_err = |problem: String| InstallError::ReleaseShape {
        id: coord.id(),
        problem,
    };
    let wasm = match wasm.as_slice() {
        [one] => *one,
        [] => {
            return Err(shape_err(format!(
                "release {} has no .wasm asset",
                release.tag
            )))
        }
        many => {
            let names: Vec<&str> = many.iter().map(|a| a.name.as_str()).collect();
            return Err(shape_err(format!(
                "release {} has {} .wasm assets ({}) — the coordinate must be unambiguous",
                release.tag,
                many.len(),
                names.join(", ")
            )));
        }
    };
    let checksums = release
        .assets
        .iter()
        .find(|a| a.name == "checksums.txt")
        .ok_or_else(|| shape_err(format!("release {} has no checksums.txt", release.tag)))?;
    Ok((wasm, checksums))
}

/// The `sha256sum` line format: `<hex><spaces><name>`, one per asset.
fn expected_sha(
    coord: &Coordinate,
    checksums: &[u8],
    asset_name: &str,
) -> Result<String, InstallError> {
    std::str::from_utf8(checksums)
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let (hex, name) = line.trim().split_once(char::is_whitespace)?;
                (name.trim().trim_start_matches('*') == asset_name).then(|| hex.to_string())
            })
        })
        .ok_or_else(|| InstallError::ReleaseShape {
            id: coord.id(),
            problem: format!("checksums.txt has no entry for {asset_name}"),
        })
}

/// Write every staged component and the updated lockfile — the only step that mutates
/// anything, reached only with every download verified.
fn commit(
    dir: &Path,
    staged: Vec<(Coordinate, String, Vec<u8>, String)>,
    lock: &mut Lock,
    report: &mut InstallReport,
) -> Result<(), InstallError> {
    if staged.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| InstallError::Io(format!("creating {}: {e}", dir.display())))?;
    for (coord, tag, bytes, sha) in staged {
        let file = coord.file_name();
        let path = dir.join(&file);
        std::fs::write(&path, &bytes)
            .map_err(|e| InstallError::Io(format!("writing {}: {e}", path.display())))?;
        lock.insert(
            coord.id(),
            LockEntry {
                version: tag.clone(),
                sha256: sha,
                file,
            },
        );
        report.installed.push((coord.id(), tag));
    }
    write_lock(dir, lock)
}

// ---------------------------------------------------------------- list / remove

/// One row of `kndo plugin list`.
#[derive(Debug)]
pub struct InstalledPlugin {
    pub id: String,
    pub version: String,
    pub file: String,
}

/// The managed set (from `plugins.lock`) plus any hand-dropped `.wasm` files the lock doesn't
/// know about — visible, not hidden, same doctor-style honesty as everywhere else.
pub fn list() -> Result<(PathBuf, Vec<InstalledPlugin>, Vec<String>), InstallError> {
    let dir = global_dir()?;
    let lock = read_lock(&dir)?;
    let managed: Vec<InstalledPlugin> = lock
        .iter()
        .map(|(id, e)| InstalledPlugin {
            id: id.clone(),
            version: e.version.clone(),
            file: e.file.clone(),
        })
        .collect();
    let unmanaged = unmanaged_wasm_files(&dir, &lock);
    Ok((dir, managed, unmanaged))
}

fn unmanaged_wasm_files(dir: &Path, lock: &Lock) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name.ends_with(".wasm"))
        .filter(|name| !lock.values().any(|entry| entry.file == *name))
        .collect();
    names.sort();
    names
}

/// Remove one managed plugin: its `.wasm` and its lock entry. Dependents are deliberately not
/// blocked on (RFC 0015 §3: a missing dependency is never fatal — `kndo doctor` reports the
/// gap afterwards, which is the designed degradation).
pub fn remove(spec: &str) -> Result<String, InstallError> {
    let dir = global_dir()?;
    remove_from(spec, &dir)
}

pub fn remove_from(spec: &str, dir: &Path) -> Result<String, InstallError> {
    let coord = Coordinate::parse(spec)?;
    let (lock, entry) = take_lock_entry(dir, &coord)?;
    delete_component_file(&dir.join(&entry.file))?;
    write_lock(dir, &lock)?;
    Ok(entry.version)
}

fn take_lock_entry(dir: &Path, coord: &Coordinate) -> Result<(Lock, LockEntry), InstallError> {
    let mut lock = read_lock(dir)?;
    let entry = lock
        .remove(&coord.id())
        .ok_or_else(|| InstallError::NotInstalled(coord.id()))?;
    Ok((lock, entry))
}

fn delete_component_file(path: &Path) -> Result<(), InstallError> {
    if path.is_file() {
        std::fs::remove_file(path)
            .map_err(|e| InstallError::Io(format!("removing {}: {e}", path.display())))?;
    }
    Ok(())
}

fn global_dir() -> Result<PathBuf, InstallError> {
    crate::activation::global_plugin_dir().ok_or(InstallError::NoGlobalDir)
}

// ---------------------------------------------------------------- real edges

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The real probe: land the verified bytes in a temp file (the WASM host loads from a path)
/// and let `kndo-plugin-api`'s own loader — reserved-namespace rejection included — vet them.
/// Public so the integration suite can run [`install_with`] against genuine components.
pub fn wasm_probe(bytes: &[u8]) -> Result<ProbedDescriptor, String> {
    let dir = tempfile_dir().map_err(|e| e.to_string())?;
    let path = dir.join("probe.wasm");
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let result = kndo_plugin_api::WasmPlugin::load(&path)
        .map(|plugin| {
            let d = kndo_core::plugin::Plugin::descriptor(&plugin);
            ProbedDescriptor {
                id: d.id.to_string(),
                version: d.version.to_string(),
                dependencies: d.dependencies.iter().map(|s| s.to_string()).collect(),
            }
        })
        .map_err(|e| e.to_string());
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn tempfile_dir() -> std::io::Result<PathBuf> {
    let base = std::env::temp_dir().join(format!("kndo-install-{}", std::process::id()));
    std::fs::create_dir_all(&base)?;
    Ok(base)
}

/// GitHub's release API, honoring the ambient environment: `GITHUB_TOKEN`/`GH_TOKEN` for
/// private repos (RFC 0015 §4: "the fetch uses the user's existing credentials"), the
/// platform trust store for TLS, and `HTTPS_PROXY`-style variables for proxies.
pub struct GitHubReleaseSource {
    agent: ureq::Agent,
    token: Option<String>,
}

impl GitHubReleaseSource {
    pub fn from_env() -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let config = ureq::Agent::config_builder()
            .tls_config(tls)
            .http_status_as_error(false)
            .build();
        GitHubReleaseSource {
            agent: config.new_agent(),
            token: std::env::var("GITHUB_TOKEN")
                .or_else(|_| std::env::var("GH_TOKEN"))
                .ok(),
        }
    }

    fn get(&self, url: &str, accept: &str) -> Result<(u16, Vec<u8>), InstallError> {
        let mut request = self
            .agent
            .get(url)
            .header("User-Agent", "kndo-plugin-install")
            .header("Accept", accept)
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = &self.token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let mut response = request
            .call()
            .map_err(|e| InstallError::Fetch(format!("GET {url}: {e}")))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            // Components are small by design (authoring.md §2 budgets); 64 MiB is a
            // generous ceiling that still stops a runaway body.
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| InstallError::Fetch(format!("reading {url}: {e}")))?;
        Ok((status, body))
    }
}

impl ReleaseSource for GitHubReleaseSource {
    fn resolve(&self, coord: &Coordinate) -> Result<ReleaseInfo, InstallError> {
        let url = release_url(coord);
        let (status, body) = self.get(&url, "application/vnd.github+json")?;
        ensure_release_found(status, coord, &url)?;
        parse_release(coord, &body)
    }

    fn download(&self, asset: &AssetInfo) -> Result<Vec<u8>, InstallError> {
        // The API asset URL + octet-stream works for public and private repos alike (the
        // browser_download_url does not carry auth for private ones).
        let (status, body) = self.get(&asset.url, "application/octet-stream")?;
        if status != 200 {
            return Err(InstallError::Fetch(format!(
                "downloading {}: HTTP {status}",
                asset.name
            )));
        }
        Ok(body)
    }
}

fn release_url(coord: &Coordinate) -> String {
    match &coord.tag {
        Some(tag) => format!(
            "https://api.github.com/repos/{}/{}/releases/tags/{tag}",
            coord.owner, coord.repo
        ),
        None => format!(
            "https://api.github.com/repos/{}/{}/releases/latest",
            coord.owner, coord.repo
        ),
    }
}

fn ensure_release_found(status: u16, coord: &Coordinate, url: &str) -> Result<(), InstallError> {
    if status == 404 {
        return Err(InstallError::Fetch(format!(
            "{} has no matching release (404) — the repo may not exist, have no releases, \
             or be private (set GITHUB_TOKEN for private repos)",
            coord.id()
        )));
    }
    if status != 200 {
        return Err(InstallError::Fetch(format!("GET {url}: HTTP {status}")));
    }
    Ok(())
}

fn parse_release(coord: &Coordinate, body: &[u8]) -> Result<ReleaseInfo, InstallError> {
    let json: serde_json::Value = serde_json::from_slice(body)
        .map_err(|e| InstallError::Fetch(format!("release JSON for {}: {e}", coord.id())))?;
    let tag = json
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| InstallError::Fetch(format!("release of {} has no tag_name", coord.id())))?
        .to_string();
    let assets = json
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|list| list.iter().filter_map(parse_asset).collect())
        .unwrap_or_default();
    Ok(ReleaseInfo { tag, assets })
}

fn parse_asset(asset: &serde_json::Value) -> Option<AssetInfo> {
    Some(AssetInfo {
        name: asset.get("name")?.as_str()?.to_string(),
        url: asset.get("url")?.as_str()?.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Serves canned releases; asset "URLs" are just keys into a byte map. Descriptors are
    /// encoded *in* the wasm bytes as JSON, so the fake probe is a parse — the install logic
    /// can't tell the difference, which is the point.
    struct FakeSource {
        releases: HashMap<String, ReleaseInfo>,
        bodies: RefCell<HashMap<String, Vec<u8>>>,
    }

    impl FakeSource {
        fn new() -> Self {
            FakeSource {
                releases: HashMap::new(),
                bodies: RefCell::new(HashMap::new()),
            }
        }

        /// Publish a release for `spec` (`github.com/x/y` or `github.com/x/y@vN`) whose wasm
        /// bytes declare `descriptor_id` + `deps`. Returns the wasm bytes for assertions.
        fn publish(
            &mut self,
            spec: &str,
            tag: &str,
            descriptor_id: &str,
            deps: &[&str],
        ) -> Vec<u8> {
            let bytes = fake_component(descriptor_id, deps);
            self.publish_raw(spec, tag, bytes.clone(), &sha256_hex(&bytes));
            bytes
        }

        fn publish_raw(&mut self, spec: &str, tag: &str, bytes: Vec<u8>, sha: &str) {
            let wasm_url = format!("{spec}#wasm");
            let sums_url = format!("{spec}#sums");
            let wasm_name = "plugin.wasm".to_string();
            self.bodies.borrow_mut().insert(wasm_url.clone(), bytes);
            self.bodies.borrow_mut().insert(
                sums_url.clone(),
                format!("{sha}  {wasm_name}\n").into_bytes(),
            );
            self.releases.insert(
                spec.to_string(),
                ReleaseInfo {
                    tag: tag.to_string(),
                    assets: vec![
                        AssetInfo {
                            name: wasm_name,
                            url: wasm_url,
                        },
                        AssetInfo {
                            name: "checksums.txt".to_string(),
                            url: sums_url,
                        },
                    ],
                },
            );
        }
    }

    impl ReleaseSource for FakeSource {
        fn resolve(&self, coord: &Coordinate) -> Result<ReleaseInfo, InstallError> {
            let key = match &coord.tag {
                Some(tag) => format!("{}@{tag}", coord.id()),
                None => coord.id(),
            };
            self.releases
                .get(&key)
                .cloned()
                .ok_or_else(|| InstallError::Fetch(format!("no release for {key}")))
        }

        fn download(&self, asset: &AssetInfo) -> Result<Vec<u8>, InstallError> {
            self.bodies
                .borrow()
                .get(&asset.url)
                .cloned()
                .ok_or_else(|| InstallError::Fetch(format!("no body for {}", asset.url)))
        }
    }

    fn fake_component(id: &str, deps: &[&str]) -> Vec<u8> {
        serde_json::json!({ "id": id, "version": "1", "dependencies": deps })
            .to_string()
            .into_bytes()
    }

    fn fake_probe(bytes: &[u8]) -> Result<ProbedDescriptor, String> {
        let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        Ok(ProbedDescriptor {
            id: v["id"].as_str().unwrap_or_default().to_string(),
            version: v["version"].as_str().unwrap_or_default().to_string(),
            dependencies: v["dependencies"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|d| d.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    fn no_builtins() -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn coordinates_parse_strictly() {
        let c = Coordinate::parse("github.com/acme/framework-plugin@v2.1.0").unwrap();
        assert_eq!(c.id(), "github.com/acme/framework-plugin");
        assert_eq!(c.tag.as_deref(), Some("v2.1.0"));
        assert_eq!(c.file_name(), "github.com__acme__framework-plugin.wasm");
        assert!(Coordinate::parse("github.com/acme/repo")
            .unwrap()
            .tag
            .is_none());

        for bad in [
            "gitlab.com/a/b",
            "github.com/a",
            "github.com/a/b/c",
            "github.com//b",
            "github.com/a/b@",
            "just-a-name",
        ] {
            assert!(Coordinate::parse(bad).is_err(), "{bad} should be rejected");
        }
        assert!(matches!(
            Coordinate::parse("kndo:nextjs"),
            Err(InstallError::BuiltinCoordinate(_))
        ));
    }

    #[test]
    fn install_writes_component_and_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = FakeSource::new();
        let bytes = source.publish("github.com/acme/p", "v1.0.0", "github.com/acme/p", &[]);

        let report = install_with(
            "github.com/acme/p",
            dir.path(),
            &source,
            fake_probe,
            &no_builtins(),
        )
        .unwrap();

        assert_eq!(
            report.installed,
            vec![("github.com/acme/p".to_string(), "v1.0.0".to_string())]
        );
        let installed = dir.path().join("github.com__acme__p.wasm");
        assert_eq!(std::fs::read(installed).unwrap(), bytes);
        let lock = read_lock(dir.path()).unwrap();
        let entry = &lock["github.com/acme/p"];
        assert_eq!(entry.version, "v1.0.0");
        assert_eq!(entry.sha256, sha256_hex(&bytes));
        assert_eq!(entry.file, "github.com__acme__p.wasm");
    }

    #[test]
    fn dependencies_close_transitively_and_builtins_are_noops() {
        // The RFC 0015 §1 chain: the company plugin depends on another repo's plugin, a
        // built-in this build has, and a built-in it doesn't.
        let dir = tempfile::tempdir().unwrap();
        let mut source = FakeSource::new();
        source.publish(
            "github.com/company/framework",
            "v3.0.0",
            "github.com/company/framework",
            &[
                "github.com/company/base",
                "kndo:nextjs",
                "kndo:not-in-this-build",
            ],
        );
        source.publish(
            "github.com/company/base",
            "v1.2.0",
            "github.com/company/base",
            &[],
        );

        let report = install_with(
            "github.com/company/framework",
            dir.path(),
            &source,
            fake_probe,
            &["kndo:nextjs".to_string()],
        )
        .unwrap();

        let mut installed = report.installed.clone();
        installed.sort();
        assert_eq!(
            installed,
            vec![
                ("github.com/company/base".to_string(), "v1.2.0".to_string()),
                (
                    "github.com/company/framework".to_string(),
                    "v3.0.0".to_string()
                ),
            ]
        );
        assert_eq!(report.builtin_deps, vec!["kndo:nextjs"]);
        assert_eq!(report.unknown_builtins, vec!["kndo:not-in-this-build"]);
        assert_eq!(read_lock(dir.path()).unwrap().len(), 2);
    }

    #[test]
    fn checksum_mismatch_aborts_before_any_write() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = FakeSource::new();
        let bytes = fake_component("github.com/acme/p", &[]);
        source.publish_raw("github.com/acme/p", "v1.0.0", bytes, &"0".repeat(64));

        let err = install_with(
            "github.com/acme/p",
            dir.path(),
            &source,
            fake_probe,
            &no_builtins(),
        )
        .unwrap_err();

        assert!(
            matches!(err, InstallError::ChecksumMismatch { .. }),
            "{err}"
        );
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn identity_binding_rejects_an_impersonating_component() {
        // RFC 0015 §2: fetched by one coordinate, declaring another — rejected, nothing
        // written, even though the checksum was genuine.
        let dir = tempfile::tempdir().unwrap();
        let mut source = FakeSource::new();
        source.publish("github.com/acme/p", "v1.0.0", "github.com/other/name", &[]);

        let err = install_with(
            "github.com/acme/p",
            dir.path(),
            &source,
            fake_probe,
            &no_builtins(),
        )
        .unwrap_err();

        assert!(
            matches!(err, InstallError::IdentityMismatch { .. }),
            "{err}"
        );
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn conflicting_explicit_tags_fail_with_both_requirers_named() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = FakeSource::new();
        source.publish(
            "github.com/acme/root",
            "v1.0.0",
            "github.com/acme/root",
            &["github.com/acme/shared@v1.0.0", "github.com/acme/mid"],
        );
        source.publish(
            "github.com/acme/mid",
            "v2.0.0",
            "github.com/acme/mid",
            &["github.com/acme/shared@v2.0.0"],
        );
        source.publish(
            "github.com/acme/shared@v1.0.0",
            "v1.0.0",
            "github.com/acme/shared",
            &[],
        );
        source.publish(
            "github.com/acme/shared@v2.0.0",
            "v2.0.0",
            "github.com/acme/shared",
            &[],
        );

        let err = install_with(
            "github.com/acme/root",
            dir.path(),
            &source,
            fake_probe,
            &no_builtins(),
        )
        .unwrap_err();

        match err {
            InstallError::VersionConflict { id, first, second } => {
                assert_eq!(id, "github.com/acme/shared");
                let requirers = [first.1, second.1];
                assert!(requirers.contains(&"github.com/acme/root".to_string()));
                assert!(requirers.contains(&"github.com/acme/mid".to_string()));
            }
            other => panic!("expected VersionConflict, got {other}"),
        }
        // The whole transaction failed — nothing landed, not even the unconflicted root.
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn already_installed_is_a_noop_and_a_different_tag_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = FakeSource::new();
        source.publish("github.com/acme/p", "v1.0.0", "github.com/acme/p", &[]);
        install_with(
            "github.com/acme/p",
            dir.path(),
            &source,
            fake_probe,
            &no_builtins(),
        )
        .unwrap();

        let again = install_with(
            "github.com/acme/p",
            dir.path(),
            &source,
            fake_probe,
            &no_builtins(),
        )
        .unwrap();
        assert!(again.installed.is_empty());
        assert_eq!(again.already_present, vec!["github.com/acme/p"]);

        let err = install_with(
            "github.com/acme/p@v9.9.9",
            dir.path(),
            &source,
            fake_probe,
            &no_builtins(),
        )
        .unwrap_err();
        assert!(
            matches!(err, InstallError::InstalledVersionMismatch { .. }),
            "{err}"
        );
    }

    #[test]
    fn remove_deletes_the_component_and_its_lock_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = FakeSource::new();
        source.publish("github.com/acme/p", "v1.0.0", "github.com/acme/p", &[]);
        install_with(
            "github.com/acme/p",
            dir.path(),
            &source,
            fake_probe,
            &no_builtins(),
        )
        .unwrap();

        let version = remove_from("github.com/acme/p", dir.path()).unwrap();
        assert_eq!(version, "v1.0.0");
        assert!(!dir.path().join("github.com__acme__p.wasm").exists());
        assert!(read_lock(dir.path()).unwrap().is_empty());

        assert!(matches!(
            remove_from("github.com/acme/p", dir.path()),
            Err(InstallError::NotInstalled(_))
        ));
    }

    #[test]
    fn checksum_lines_accept_the_binary_marker() {
        // `sha256sum -b` writes `<hex> *<name>` — the convention kndo's own release job could
        // legitimately produce; both spellings must verify.
        let coord = Coordinate::parse("github.com/a/b").unwrap();
        let sums = format!("{}  *plugin.wasm\n", "a".repeat(64));
        assert_eq!(
            expected_sha(&coord, sums.as_bytes(), "plugin.wasm").unwrap(),
            "a".repeat(64)
        );
        assert!(expected_sha(&coord, sums.as_bytes(), "other.wasm").is_err());
    }
}
