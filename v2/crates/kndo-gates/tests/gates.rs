//! One `#[test]` per registry entry; the generated workflow runs each by exact name
//! and greps this harness's `--list` output for existence.

use kndo_gates::{render_ci, workflow_path};

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
