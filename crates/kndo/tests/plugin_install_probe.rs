//! The install transaction against a *genuine* WASM component (RFC 0015 §4): the unit tests in
//! `plugin_install.rs` fake the probe to exercise every policy; this suite keeps the one edge
//! they can't — `kndo::plugin_install::wasm_probe`, i.e. the real `kndo-plugin-api` loader —
//! honest. The hooks-demo component declares `id: "hooks-demo"`, so fetching it *by* any
//! coordinate must trip identity binding (RFC 0015 §2) and leave the directory untouched: a
//! plain-named component is installable by hand-drop only, never by coordinate — which is
//! exactly §2's "depending on something requires it to be addressable" rule enforced at the
//! only gate a fetch passes through.

use std::path::{Path, PathBuf};
use std::process::Command;

use kndo::plugin_install::{
    install_with, AssetInfo, Coordinate, InstallError, ReleaseInfo, ReleaseSource,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/kndo has two ancestors up to the workspace root")
        .to_path_buf()
}

fn build_component(example_dir: &str, wasm_name: &str) -> Vec<u8> {
    let demo_dir = workspace_root().join(example_dir);
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        .current_dir(&demo_dir)
        .status()
        .unwrap_or_else(|e| panic!("failed to invoke cargo for {example_dir}: {e}"));
    assert!(status.success(), "{example_dir} guest build failed");

    let core_wasm = std::fs::read(
        demo_dir
            .join("target/wasm32-unknown-unknown/release")
            .join(wasm_name),
    )
    .expect("reading the built guest module");

    wit_component::ComponentEncoder::default()
        .module(&core_wasm)
        .expect("attaching the core module to the component encoder")
        .encode()
        .expect("encoding the component")
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// One canned release: the component under `plugin.wasm` plus a correct `checksums.txt` — the
/// installer-ready shape of docs/plugins/authoring.md §9, so the *only* thing that can fail
/// downstream is what this suite is about: the real probe and identity binding.
struct OneRelease {
    wasm: Vec<u8>,
}

impl ReleaseSource for OneRelease {
    fn resolve(&self, _coord: &Coordinate) -> Result<ReleaseInfo, InstallError> {
        Ok(ReleaseInfo {
            tag: "v1.0.0".to_string(),
            assets: vec![
                AssetInfo {
                    name: "plugin.wasm".to_string(),
                    url: "wasm".to_string(),
                },
                AssetInfo {
                    name: "checksums.txt".to_string(),
                    url: "sums".to_string(),
                },
            ],
        })
    }

    fn download(&self, asset: &AssetInfo) -> Result<Vec<u8>, InstallError> {
        match asset.url.as_str() {
            "wasm" => Ok(self.wasm.clone()),
            _ => Ok(format!("{}  plugin.wasm\n", sha256_hex(&self.wasm)).into_bytes()),
        }
    }
}

#[test]
fn a_real_component_with_a_plain_id_fails_identity_binding() {
    let wasm = build_component(
        "examples/kndo-plugin-hooks-demo",
        "kndo_plugin_hooks_demo.wasm",
    );
    let dir = tempfile::tempdir().expect("temp global plugin dir");

    let err = install_with(
        "github.com/someone/hooks-demo",
        dir.path(),
        &OneRelease { wasm },
        kndo::plugin_install::wasm_probe,
        &[],
    )
    .expect_err("a plain-named component must not be installable by coordinate");

    match err {
        InstallError::IdentityMismatch {
            coordinate,
            declared,
        } => {
            assert_eq!(coordinate, "github.com/someone/hooks-demo");
            assert_eq!(declared, "hooks-demo");
        }
        other => panic!("expected IdentityMismatch, got: {other}"),
    }
    // Checksum verification passed, the component genuinely loaded — and still nothing landed.
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "identity rejection must leave the directory untouched"
    );
}
