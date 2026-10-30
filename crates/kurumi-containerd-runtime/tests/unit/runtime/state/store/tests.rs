// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

#[test]
fn selects_state_and_recovery_paths_from_workdir_and_name() {
    let store = StateStore::new(Path::new("/run/kurumi"), "demo");
    assert_eq!(store.state_dir(), Path::new("/run/kurumi/state"));
    assert_eq!(store.recovery_dir(), Path::new("/run/kurumi/recovery"));
    assert_eq!(store.state_path(), Path::new("/run/kurumi/state/demo.json"));
}
