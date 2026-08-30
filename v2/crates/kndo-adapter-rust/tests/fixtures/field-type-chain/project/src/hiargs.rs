use crate::lowargs::LowArgs;

pub(crate) fn finish(low: &LowArgs) -> usize {
    low.context_separator.into_bytes()
}
