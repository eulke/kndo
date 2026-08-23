pub(crate) struct Stats {
    n: usize,
}

impl Stats {
    pub(crate) fn new() -> Stats {
        Stats { n: 0 }
    }

    pub(crate) fn combine(self, other: Stats) -> Stats {
        Stats { n: self.n + other.n }
    }
}

impl std::ops::Add for Stats {
    type Output = Stats;

    fn add(self, rhs: Stats) -> Stats {
        Stats { n: self.n + rhs.n }
    }
}

impl<'a> std::ops::Add<&'a Stats> for Stats {
    type Output = Stats;

    fn add(self, rhs: &'a Stats) -> Stats {
        Stats { n: self.n + rhs.n }
    }
}
