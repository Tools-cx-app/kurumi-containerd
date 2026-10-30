// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

#[test]
fn graphical_terminal_is_unavailable_on_android_or_headless_linux() {
    assert!(!terminal_available(true, true, true));
    assert!(!terminal_available(false, false, true));
    assert!(!terminal_available(false, true, false));
    assert!(terminal_available(false, true, true));
}
