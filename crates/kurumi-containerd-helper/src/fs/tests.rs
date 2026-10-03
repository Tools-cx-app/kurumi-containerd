use std::fs;

#[test]
fn exchanges_paths_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let left = directory.path().join("left");
    let right = directory.path().join("right");
    fs::write(&left, "left").unwrap();
    fs::write(&right, "right").unwrap();

    super::rename_exchange(&left, &right).unwrap();

    assert_eq!(fs::read_to_string(left).unwrap(), "right");
    assert_eq!(fs::read_to_string(right).unwrap(), "left");
}
