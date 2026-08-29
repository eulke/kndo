//! The v2 binary at its M0 size: enough surface for the release pipeline to have a
//! real artifact to build, package, checksum, install and run on every platform CI
//! promises — before any analysis code exists to hide that plumbing's failures.

const VERSION: &str = concat!("kndo ", env!("CARGO_PKG_VERSION"), "-dev (v2 skeleton)");

fn main() {
    println!("{VERSION}");
}
