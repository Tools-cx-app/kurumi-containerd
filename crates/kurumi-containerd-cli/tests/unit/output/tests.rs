use super::*;

#[test]
fn pid_is_numeric_and_foreground_reports_completion() {
    let mut out = Vec::new();
    pid(&mut out, 1234).unwrap();
    assert_eq!(out, b"1234\n");
    out.clear();
    started(&mut out, "demo", 1234, true).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("demo"));
    assert!(text.contains("finished"));
    assert!(!text.contains("started"));
}

#[test]
fn empty_results_are_explicit_and_writes_are_fallible() {
    let mut out = Vec::new();
    containers(&mut out, &[]).unwrap();
    recovered(&mut out, &[]).unwrap();
    check(&mut out, "Namespaces", false, "permission denied").unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("Running containers: 0"));
    assert!(text.contains("No containers required recovery"));
    assert!(text.contains("unavailable (permission denied)"));
    assert!(!text.contains("INFO"));
    let mut full = &mut [][..];
    let error = pid(&mut full, 1234).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WriteZero);
}

#[test]
fn only_output_broken_pipes_are_ignored() {
    let error = write(|_| Err(io::ErrorKind::BrokenPipe.into())).unwrap_err();
    assert!(error.is_broken_pipe());
    let error = write(|_| Err(io::ErrorKind::PermissionDenied.into())).unwrap_err();
    assert!(!error.is_broken_pipe());
}
