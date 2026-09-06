mod deep;

pub fn used_inside() {
    deep::down();
}

pub fn never_used() {}

fn shared_with_children() {}
