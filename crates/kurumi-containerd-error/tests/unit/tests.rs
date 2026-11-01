// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

#[test]
#[cfg(feature = "runtime")]
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

#[test]
fn config_context_keeps_io_source_without_runtime() {
    use std::error::Error as _;

    use super::config::ErrorContext as _;

    let error = Err::<(), _>(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        .context("reading configuration")
        .unwrap_err();
    assert!(matches!(error, super::ConfigError::Io { .. }));
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
}
