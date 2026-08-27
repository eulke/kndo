//! The shipping binary's feature list must not drift from the library's.
//!
//! `kndo-cli` sets `default-features = false` on `kndo` and re-declares the default set by hand,
//! deliberately: that is what lets CI build the "shell" configuration without editing the
//! manifest. The cost is two lists that must agree, and they silently did not — `kndo:thymeleaf`
//! was registered in `kndo` and shipped in no binary at all, which looked exactly like a plugin
//! that does not work. Nothing failed; the plugin simply never ran.
//!
//! So: every feature `kndo` turns on by default must be passed through and turned on here too.

use std::path::{Path, PathBuf};

fn manifest(crate_dir: &str) -> toml::Table {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .join(crate_dir)
        .join("Cargo.toml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
        .parse()
        .expect("a valid manifest")
}

fn features(manifest: &toml::Table) -> &toml::Table {
    manifest["features"].as_table().expect("[features]")
}

fn defaults(manifest: &toml::Table) -> Vec<String> {
    features(manifest)["default"]
        .as_array()
        .expect("default = [...]")
        .iter()
        .map(|v| v.as_str().expect("a feature name").to_string())
        .collect()
}

#[test]
fn every_default_library_feature_is_shipped_by_the_binary() {
    let library = manifest("kndo");
    let cli = manifest("kndo-cli");
    let cli_defaults = defaults(&cli);
    let cli_features = features(&cli);

    let missing: Vec<&String> = defaults(&library)
        .iter()
        .filter(|f| !cli_defaults.contains(f))
        .cloned()
        .collect::<Vec<String>>()
        .leak()
        .iter()
        .collect();
    assert!(
        missing.is_empty(),
        "kndo enables these by default and the shipped binary does not — add each to \
         kndo-cli's `default` list AND to its passthrough features: {missing:?}"
    );

    // A name in `default` that forwards nothing would be just as silent.
    for feature in &cli_defaults {
        let forwards = cli_features
            .get(feature)
            .and_then(toml::Value::as_array)
            .is_some_and(|deps| {
                deps.iter()
                    .filter_map(toml::Value::as_str)
                    .any(|d| d.starts_with("kndo/"))
            });
        assert!(
            forwards,
            "kndo-cli's `{feature}` is in `default` but forwards nothing to `kndo/{feature}`"
        );
    }
}
