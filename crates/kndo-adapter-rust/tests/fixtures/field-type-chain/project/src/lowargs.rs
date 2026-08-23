pub(crate) struct LowArgs {
    pub(crate) context_separator: ContextSeparator,
}

impl LowArgs {
    pub(crate) fn parse() -> LowArgs {
        LowArgs {
            context_separator: ContextSeparator,
        }
    }
}

pub(crate) struct ContextSeparator;

impl ContextSeparator {
    pub(crate) fn into_bytes(&self) -> usize {
        0
    }
}
