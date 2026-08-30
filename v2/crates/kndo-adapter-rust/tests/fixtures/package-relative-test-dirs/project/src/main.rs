mod work;

fn main() {
    let code = work::api();
    std::process::exit(code as i32 - 42);
}
