use crate::stats::Stats;

mod stats;

fn main() {
    let a = Stats::new();
    let b = Stats::new();
    let _ = a.combine(b);
}
