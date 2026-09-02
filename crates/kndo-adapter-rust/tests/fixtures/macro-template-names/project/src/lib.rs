mod messages;

pub fn run() -> bool {
    log_err!("boom");
    messages::caller()
}
