mod hiargs;
mod search;

fn main() {
    let args = crate::hiargs::HiArgs::parse();
    search::run(&args);
}
