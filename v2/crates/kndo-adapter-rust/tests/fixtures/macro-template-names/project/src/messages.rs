macro_rules! log_err {
    ($msg:expr) => {
        crate::messages::set_flag($msg)
    };
}

pub(crate) fn set_flag(_m: &str) {}

pub(crate) fn local_only() -> bool {
    true
}

pub(crate) fn caller() -> bool {
    local_only()
}
