use super::*;

#[test]
fn startup_errors_reach_parent_or_require_fallback_logging() {
    let (reader, mut writer) = pipe().unwrap();
    assert!(report_startup_error(
        Some(&mut writer),
        "boot failed: mount denied"
    ));
    drop(writer);
    let mut received = String::new();
    File::from(reader).read_to_string(&mut received).unwrap();
    assert_eq!(received, "boot failed: mount denied");
    assert!(!report_startup_error(None, "reboot failed"));
    let mut read_only: OwnedFd = File::open("/dev/null").unwrap().into();
    assert!(!report_startup_error(Some(&mut read_only), "boot failed"));
}
