//! One call deep: the binary is the library plus stdio, so everything it does is
//! reachable from tests through imports.

fn main() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    kndo_serve::serve(stdin.lock(), stdout.lock())
}
