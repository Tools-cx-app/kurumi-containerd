#[test]
fn context_keeps_io_source() {
    use std::error::Error as _;

    use super::ErrorContext as _;

    let dir = tempfile::tempdir().unwrap();
    let error = std::fs::read(dir.path().join("missing"))
        .context("reading test file")
        .unwrap_err();
    assert!(error.to_string().starts_with("reading test file:"));
    assert!(error.source().is_some());
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .kind(),
        std::io::ErrorKind::NotFound
    );
}
