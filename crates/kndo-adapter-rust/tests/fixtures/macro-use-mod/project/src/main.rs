#[macro_use]
mod messages;

mod worker;

fn main() {
    err_message!("boot {}", 1);
    worker::work();
}
