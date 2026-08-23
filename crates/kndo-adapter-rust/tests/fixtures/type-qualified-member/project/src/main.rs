use crate::thing::Thing;

mod thing;

fn main() {
    let t = Thing::from_low_args(1);
    t.consume();
}
