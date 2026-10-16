use super::*;

#[test]
fn report_represents_available_and_unavailable_capabilities() {
    let report = HostCheckReport {
        capabilities: vec![
            capability("available", true, "working"),
            capability("missing", false, "not installed"),
        ],
    };

    assert!(report.capabilities[0].available);
    assert!(!report.capabilities[1].available);
}

#[test]
fn default_checker_implements_host_check() {
    fn assert_implementation<T: HostCheck>() {}

    assert_implementation::<HostCapabilities>();
}
