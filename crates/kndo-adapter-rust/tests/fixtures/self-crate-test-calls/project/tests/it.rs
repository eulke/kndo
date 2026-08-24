#[test]
fn calls_top_api() {
    let v = selfy::top_api();
    assert_eq!(v, 42);
}
