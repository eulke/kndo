use crate::flags::HiArgs;

mod flags;

fn main() {
    let args: HiArgs = flags::parse();
    let _ = args;
}
