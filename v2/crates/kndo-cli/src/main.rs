//! One call deep: the binary is the library plus process plumbing, so everything it
//! does is reachable from tests through imports.

use std::io::Write;

fn main() {
    let out = kndo_cli::run_args(std::env::args_os());
    if !out.stdout.is_empty() {
        let _ = std::io::stdout().write_all(out.stdout.as_bytes());
    }
    if !out.stderr.is_empty() {
        let _ = std::io::stderr().write_all(out.stderr.as_bytes());
    }
    std::process::exit(out.code);
}
