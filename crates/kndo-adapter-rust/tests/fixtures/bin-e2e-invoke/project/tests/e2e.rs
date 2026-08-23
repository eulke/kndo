use std::process::Command;

#[test]
fn exits_cleanly() {
    let status = Command::new(env!("CARGO_BIN_EXE_tool"))
        .status()
        .expect("spawn");
    assert!(status.success());
}
