use flags::{FLAGS, render};

#[test]
fn every_flag_renders() {
    for flag in FLAGS.iter() {
        assert!(!render(*flag).is_empty());
    }
}
