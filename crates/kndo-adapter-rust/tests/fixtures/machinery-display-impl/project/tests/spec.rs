use errors::LoadError;

#[test]
fn carries_the_path() {
    let err = LoadError {
        path: "a.toml".to_string(),
    };
    assert_eq!(err.path, "a.toml");
}
