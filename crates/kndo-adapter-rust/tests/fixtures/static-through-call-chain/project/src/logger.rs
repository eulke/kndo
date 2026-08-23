pub(crate) struct Logger(());

const LOGGER: &Logger = &Logger(());

impl Logger {
    pub(crate) fn init() {
        set(LOGGER);
    }
}

fn set(_l: &Logger) {}
