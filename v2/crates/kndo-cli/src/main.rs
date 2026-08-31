//! One call deep: the binary is the library plus process plumbing, so everything it
//! does is reachable from tests through imports. The ambient facts — is stdout a
//! terminal, what does `KNDO_FORMAT` say — are read here once and handed in as
//! data; the library never touches the environment.

use std::io::{IsTerminal, Write};

fn main() {
    let host = kndo_cli::Host {
        tty: std::io::stdout().is_terminal(),
        format_env: std::env::var("KNDO_FORMAT").ok(),
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
