use std::fs;

use tempfile::NamedTempFile;

use super::ConsoleLog;

#[test]
fn shared_log_labels_complete_lines_across_reads_and_partial_output() {
    let target = NamedTempFile::new().unwrap();
    let writer = || {
        fs::OpenOptions::new()
            .append(true)
            .open(target.path())
            .unwrap()
    };
    let mut first = ConsoleLog::new(writer(), "first");
    let mut second = ConsoleLog::new(writer(), "second");
    first.append(b"part").unwrap();
    second.append(b"other\r\n").unwrap();
    first.append(b"ial\n\xff\0").unwrap();
    first.flush().unwrap();
    first.flush().unwrap();
    assert_eq!(
        fs::read(target.path()).unwrap(),
        b"[second] other\r\n[first] partial\n[first] \xff\0\n"
    );
}
