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

#[test]
fn background_log_strips_split_terminal_colors_from_module_errors() {
    let target = NamedTempFile::new().unwrap();
    let mut log = ConsoleLog::new(target.reopen().unwrap(), "arch");
    log.append(b"\x1b[0;1;").unwrap();
    log.flush().unwrap();
    log.append(b"31mFailed to find module 'autofs4'\x1b")
        .unwrap();
    log.append(b"[0m\r\n").unwrap();
    assert_eq!(
        fs::read_to_string(target.path()).unwrap(),
        "[arch] Failed to find module 'autofs4'\r\n"
    );
}

#[test]
fn background_log_preserves_unicode_and_literal_brackets_in_terminal_links() {
    let target = NamedTempFile::new().unwrap();
    let mut log = ConsoleLog::new(target.reopen().unwrap(), "arch");
    let linked = "\x1b]8;;file:///log\x1b\\日志📋 [0m\x1b]8;;\x07\n";
    for byte in linked.as_bytes().chunks(1) {
        log.append(byte).unwrap();
    }
    assert_eq!(
        fs::read_to_string(target.path()).unwrap(),
        "[arch] 日志📋 [0m\n"
    );
}
