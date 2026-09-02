//! The Unix-filter contract: the binary dies silently (default SIGPIPE
//! disposition, signal 13) when its stdout reader goes away mid-stream — never
//! a broken-pipe panic. kndo's output is designed to be piped (`kndo check |
//! jq`, the json-when-piped default), so `main` restores `SIG_DFL` at startup
//! and `| head`/`| grep -q` behave the way they do with every other filter.
#![cfg(unix)]

use std::io::Read;
use std::process::{Command, Stdio};

/// A project whose `kndo check --format json` output is guaranteed to exceed a
/// pipe's buffer capacity (64 KiB on Linux by default): 1500 unreferenced
/// files produce an `unused` finding apiece at well over 100 bytes of JSON
/// each. The size is asserted, not assumed, by
/// `the_fixture_really_overflows_a_pipe_buffer`, so an analysis change that
/// shrinks the output says so here instead of the SIGPIPE test silently losing
/// its trigger.
fn big_output_fixture() -> kndo_testkit::TempProject {
    let p = kndo_testkit::TempProject::new();
    p.file(
        "package.json",
        r#"{ "name": "demo", "main": "src/index.js" }"#,
    );
    p.file("src/index.js", "export function api() { return 1; }\n");
    for i in 0..1500 {
        p.file(
            &format!("src/dead{i}.js"),
            &format!("export function dead{i}() {{}}\n"),
        );
    }
    p
}

fn kndo() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kndo"))
}

#[test]
fn the_fixture_really_overflows_a_pipe_buffer() {
    let fixture = big_output_fixture();
    let output = kndo()
        .args([
            "check",
            "--format",
            "json",
            "--no-cache",
            "--fail-on",
            "never",
        ])
        .current_dir(fixture.root())
        .output()
        .expect("running kndo check on the fixture");
    assert!(
        output.status.success(),
        "kndo check must succeed on the fixture (stderr: {})",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.len() > 128 * 1024,
        "fixture output must exceed any common pipe buffer to make the SIGPIPE \
         test deterministic — got only {} bytes",
        output.stdout.len()
    );
}

#[test]
fn closing_the_pipe_kills_kndo_with_sigpipe_not_a_panic() {
    use std::os::unix::process::ExitStatusExt;

    let fixture = big_output_fixture();
    let mut child = kndo()
        .args([
            "check",
            "--format",
            "json",
            "--no-cache",
            "--fail-on",
            "never",
        ])
        .current_dir(fixture.root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning kndo with a piped stdout");

    // Read one byte (proves the child is writing), then drop the read end. The
    // output exceeds the pipe buffer (asserted by the sibling test), so a later
    // write must hit the closed pipe and raise SIGPIPE.
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut byte = [0u8; 1];
    stdout.read_exact(&mut byte).expect("first output byte");
    drop(stdout);

    let status = child.wait().expect("waiting for kndo");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("piped stderr")
        .read_to_string(&mut stderr)
        .expect("reading stderr");

    assert!(
        !stderr.contains("panicked"),
        "kndo must not panic on a closed pipe — stderr was:\n{stderr}"
    );
    assert_eq!(
        status.signal(),
        Some(libc::SIGPIPE),
        "kndo must die from SIGPIPE (the default Unix filter behavior), got \
         {status:?} with stderr:\n{stderr}"
    );
}
