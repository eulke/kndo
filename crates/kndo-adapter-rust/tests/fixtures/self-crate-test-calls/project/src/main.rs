fn main() {
    let code = selfy::top_api();
    std::process::exit(code as i32 - 42);
}
