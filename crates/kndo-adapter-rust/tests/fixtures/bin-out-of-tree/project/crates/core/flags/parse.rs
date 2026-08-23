use crate::flags::hiargs::HiArgs;

pub(crate) fn parse() -> HiArgs {
    crate::logger::Logger::init();
    HiArgs
}
