use super::create_mountpoint_file;

#[test]
fn existing_mountpoint_file_is_not_truncated() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target");
    std::fs::write(&path, "existing content").unwrap();

    create_mountpoint_file(&path).unwrap();

    assert_eq!(std::fs::read_to_string(path).unwrap(), "existing content");
}
