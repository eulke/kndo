//! Regression guard: the binary must die silently (default SIGPIPE disposition, signal 13) when
//! its stdout reader goes away mid-stream — never panic with a broken-pipe backtrace. A
//! `kndo doctor | grep -q` pipeline closes the pipe after the
//! first match; `main` restores `SIG_DFL` at startup (Unix), making kndo behave like every
//! other Unix filter under `| head`/`| jq -e`/`| grep -q`.
#![cfg(unix)]

use std::io::Read;
use std::process::{Command, Stdio};

/// A project whose `kndo check --format json` output is guaranteed to exceed a pipe's buffer
/// capacity (64 KiB on Linux by default): 1500 unreferenced single-function files produce an
/// `unused` finding apiece (per-file, so the rollup can't collapse them the way it collapses
/// thousands of symbols inside one file), at well over 100 bytes of JSON each — ~480 KiB
/// total, measured. The size is asserted (not assumed) by
/// `the_fixture_really_overflows_a_pipe_buffer` below, so if an analysis change ever shrinks
/// the output below the threshold this suite says so explicitly instead of the SIGPIPE test
/// silently losing its trigger.
fn big_output_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("fixture dir");
    for i in 0..1500 {
        std::fs::write(
            dir.path().join(format!("dead{i}.js")),
            format!("function dead{i}() {{}}\n"),
        )
        .expect("writing a fixture file");
    }
    // One unclaimed file blocks the directory rollup (rollup.rs: any ineligible file blocks
    // every ancestor) — without it the 1500 findings collapse into a single "this whole
    // directory is unused" finding and the output shrinks below a pipe buffer.
    std::fs::write(dir.path().join("README.md"), "fixture\n").expect("writing the blocker");
    dir
}

fn kndo() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kndo"))
}

#[test]
fn the_fixture_really_overflows_a_pipe_buffer() {
    let fixture = big_output_fixture();
    let output = kndo()
        .args(["check", "--format", "json", "--no-cache"])
        .current_dir(fixture.path())
        .output()
        .expect("running kndo check on the fixture");
    assert!(
        output.status.success(),
        "kndo check must succeed on the fixture (stderr: {})",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.len() > 128 * 1024,
        "fixture output must exceed any common pipe buffer to make the SIGPIPE test \
         deterministic — got only {} bytes",
        output.stdout.len()
    );
}

#[test]
fn closing_the_pipe_kills_kndo_with_sigpipe_not_a_panic() {
    use std::os::unix::process::ExitStatusExt;

    let fixture = big_output_fixture();
    let mut child = kndo()
        .args(["check", "--format", "json", "--no-cache"])
        .current_dir(fixture.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning kndo with a piped stdout");

    // Read one byte (proves the child is producing output), then drop the read end. The
    // child's output exceeds the pipe buffer (asserted by the sibling test), so a later write
    // must hit the closed pipe and raise SIGPIPE.
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
        "kndo must die from SIGPIPE (the default Unix filter behavior), got {status:?} \
         with stderr:\n{stderr}"
    );
}
