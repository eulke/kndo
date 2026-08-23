mod convert {
    pub(super) fn usize(v: &str) -> usize {
        v.len()
    }
}
pub(super) fn run() {
    let _ = convert::usize("x");
}
