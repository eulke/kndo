//! One call deep: the binary is the library plus process plumbing, so everything it
//! does is reachable from tests through imports. The ambient facts — is stdout a
//! terminal, what does `KNDO_FORMAT` say — are read here once and handed in as
//! data; the library never touches the environment.

use std::io::{IsTerminal, Write};

fn main() {
    // The Rust runtime starts every process with SIGPIPE ignored, so a write to
    // a pipe whose reader exited surfaces as EPIPE. kndo's output is DESIGNED
    // to be piped (`kndo check | jq`, the json-when-piped default), so the
    // Unix-filter disposition is the correct one: restore the default and die
    // silently with signal 13 (exit 141 in a shell) the way grep and git do.
    // The CLI only — serve is a protocol conversation, not a filter, and ends
    // cleanly when its transport closes.
    #[cfg(unix)]
    // SAFETY: resetting a handler to SIG_DFL, before any other thread exists.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let host = kndo_cli::Host {
        tty: std::io::stdout().is_terminal(),
        format_env: std::env::var("KNDO_FORMAT").ok(),
        no_color: std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
    };
    let out = kndo_cli::run_args(std::env::args_os(), host);
    if !out.stdout.is_empty() {
        let _ = std::io::stdout().write_all(out.stdout.as_bytes());
    }
    if !out.stderr.is_empty() {
        let _ = std::io::stderr().write_all(out.stderr.as_bytes());
    }
    std::process::exit(out.code);
}
