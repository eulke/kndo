// Cargo runs this before the crate compiles: a tooling target entered through
// its own `fn main`, which nothing in the crate ever names.
fn main() {
    println!("cargo:rerun-if-changed=src");
    emit_feature();
}

fn emit_feature() {
    println!("cargo:rustc-cfg=demo");
}
