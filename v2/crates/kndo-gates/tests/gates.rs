//! One `#[test]` per registry entry; the generated workflow runs each by exact name
//! and greps this harness's `--list` output for existence.

use kndo_gates::{render_ci, workflow_path};

#[test]
fn contract_fingerprint_is_intentional() {
    let committed = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kndo-contract/fingerprint.txt"
    ))
    .expect("fingerprint.txt exists — run `cargo xtask gen-fingerprint`");
    assert_eq!(
        committed.trim(),
        kndo_contract::contract_fingerprint_hex(),
        "\nthe contract's shape changed. If that is deliberate, run \
         `cargo xtask gen-fingerprint` and commit the new fingerprint.txt in the SAME \
         commit — the diff is the announcement. If it is not deliberate, you changed \
         a contract type without meaning to.\n"
    );
}

#[test]
fn generated_ci_is_current() {
    let committed = std::fs::read_to_string(workflow_path())
        .expect("the generated workflow exists — run `cargo xtask gen-ci`");
    assert_eq!(
        committed,
        render_ci(),
        "\n.github/workflows/v2.yml drifted from kndo-gates' registry — \
         run `cargo xtask gen-ci` and commit the result\n"
    );
}
