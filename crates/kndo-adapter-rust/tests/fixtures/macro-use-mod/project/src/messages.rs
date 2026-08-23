static ERRORED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[macro_export]
macro_rules! eprintln_locked {
    ($($tt:tt)*) => {{
        eprintln!($($tt)*);
    }};
}

macro_rules! err_message {
    ($($tt:tt)*) => {
        crate::messages::set_errored();
        eprintln_locked!($($tt)*);
    };
}

pub(crate) fn set_errored() {
    ERRORED.store(true, std::sync::atomic::Ordering::SeqCst);
}
