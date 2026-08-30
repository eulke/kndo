pub(crate) struct HiArgs {
    n: usize,
}

impl HiArgs {
    pub(crate) fn parse() -> HiArgs {
        HiArgs { n: 0 }
    }

    pub(crate) fn matcher(&self) -> usize {
        self.n
    }
}
