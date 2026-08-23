use crate::flags::HiArgs;

mod flags;
mod logger;

fn main() {
    let args: HiArgs = flags::parse();
    let _ = args;
}
