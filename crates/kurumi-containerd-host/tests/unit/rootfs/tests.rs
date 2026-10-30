// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

#[test]
fn constructs_rootfs_from_paths_without_config() {
    let rootfs = Rootfs::from_paths(Some(Path::new("/rootfs")), None, Path::new("/mount/test"));

    assert_eq!(rootfs.configured.as_deref(), Some(Path::new("/rootfs")));
    assert_eq!(rootfs.image, None);
    assert_eq!(rootfs.mountpoint, Path::new("/mount/test"));
}
