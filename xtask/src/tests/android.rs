// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    env,
    fs::{self, Permissions},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};

use crate::{build::ANDROID_ABIS, run, workspace};

fn tool(name: &str) -> PathBuf {
    env::split_paths(&env::var_os("PATH").unwrap())
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("Missing test tool: {name}"))
}

fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn module(destination: &Path) -> Result<()> {
    copy_directory(&workspace().join("android/module"), destination)?;
    for abi in ANDROID_ABIS {
        let directory = destination.join("binaries").join(abi);
        fs::create_dir_all(&directory)?;
        fs::write(directory.join("kurumi-containerd"), abi)?;
    }
    Ok(())
}

fn shell(script: &Path, data: &Path) -> Command {
    let mut command = Command::new(tool("timeout"));
    command
        .args(["15", "sh"])
        .arg(script)
        .env("KURUMI_CONTAINERD_HOME", data);
    command
}

fn installer(script: &Path, module: &Path, data: &Path, arch: &str) -> Command {
    let mut command = shell(script, data);
    command
        .env("MODPATH", module)
        .env("ARCH", arch)
        .env("API", "21")
        .env("BOOTMODE", "true");
    command
}

#[test]
fn installers_select_abis_and_reject_unsupported_or_linked_paths() -> Result<()> {
    let work = tempfile::tempdir()?;
    let data = work.path().join("data");
    let script = work.path().join("install.sh");
    fs::write(
        &script,
        "ui_print() { :; }; abort() { exit 1; }; \
         set_perm() { chmod \"$4\" \"$1\"; }; . \"$MODPATH/customize.sh\"\n",
    )?;
    for (arch, abi) in [
        ("arm64", "arm64-v8a"),
        ("arm", "armeabi-v7a"),
        ("x64", "x86_64"),
        ("x86", "x86"),
    ] {
        let destination = work.path().join(arch);
        module(&destination)?;
        let unsupported = work.path().join(format!("unsupported-{arch}"));
        module(&unsupported)?;
        assert!(
            !installer(&script, &unsupported, &data, "riscv64")
                .status()?
                .success()
        );
        assert!(
            !installer(&script, &unsupported, &data, arch)
                .env("API", "20")
                .status()?
                .success()
        );
        assert!(
            !installer(&script, &unsupported, &data, arch)
                .env("BOOTMODE", "false")
                .status()?
                .success()
        );

        let linked_data = work.path().join(format!("linked-data-{arch}"));
        let linked_target = work.path().join(format!("linked-target-{arch}"));
        fs::create_dir(&linked_data)?;
        fs::create_dir(&linked_target)?;
        fs::set_permissions(&linked_target, Permissions::from_mode(0o755))?;
        symlink(&linked_target, linked_data.join(".kurumi-containerd"))?;
        assert!(
            !installer(&script, &unsupported, &linked_data, arch)
                .status()?
                .success()
        );
        assert_eq!(
            linked_target.metadata()?.permissions().mode() & 0o777,
            0o755
        );

        let no_timeout = work.path().join(format!("no-timeout-{arch}"));
        fs::create_dir(&no_timeout)?;
        for name in ["sh", "chmod", "mkdir", "mv", "rm"] {
            symlink(tool(name), no_timeout.join(name))?;
        }
        assert!(
            !installer(&script, &unsupported, &data, arch)
                .env("PATH", no_timeout)
                .status()?
                .success()
        );
        run(&mut installer(&script, &destination, &data, arch))?;
        assert_eq!(
            fs::read_to_string(destination.join("bin/kurumi-containerd.bin"))?,
            abi
        );
        assert!(!destination.join("binaries").exists());
        assert_eq!(data.metadata()?.permissions().mode() & 0o777, 0o700);
        if arch != "arm64" {
            assert_eq!(fs::read_to_string(data.join("autostart.txt"))?, "keep\n");
        }
        fs::write(data.join("autostart.txt"), "keep\n")?;
    }
    Ok(())
}

#[test]
fn service_waits_for_boot_and_continues_after_start_failure() -> Result<()> {
    let work = tempfile::tempdir()?;
    let module = work.path().join("module");
    copy_directory(&workspace().join("android/module"), &module)?;
    let data = work.path().join("data");
    let tools = work.path().join("tools");
    fs::create_dir(&data)?;
    fs::create_dir(&tools)?;
    let calls = work.path().join("calls");
    let state = work.path().join("boot-state");
    let binary = module.join("bin/kurumi-containerd.bin");
    fs::write(
        &binary,
        "#!/bin/sh\n\
         printf '%s|%s|%s|%s\\n' \"$HOME\" \"$1\" \"$2\" \"$3\" >> \"$CALLS\"\n\
         [ \"$(cat \"$BOOT_STATE\")\" = 1 ] || exit 99\n\
         [ \"$3\" != pid ] || { [ \"$2\" = running ]; exit $?; }\n\
         [ \"$2\" != broken ]\n",
    )?;
    fs::set_permissions(binary, Permissions::from_mode(0o755))?;
    fs::write(&state, "0")?;
    for (name, contents) in [
        ("getprop", "#!/bin/sh\ncat \"$BOOT_STATE\"\n"),
        ("sleep", "#!/bin/sh\nprintf 1 > \"$BOOT_STATE\"\n"),
    ] {
        let path = tools.join(name);
        fs::write(&path, contents)?;
        fs::set_permissions(path, Permissions::from_mode(0o755))?;
    }
    fs::write(
        data.join("autostart.txt"),
        "# comment\n\nrunning\nbroken\nspace name\nlast",
    )?;
    let path = env::join_paths([tools].into_iter().chain(env::split_paths(
        &env::var_os("PATH").context("Missing PATH")?,
    )))?;
    let mut service = shell(&module.join("service.sh"), &data);
    service
        .env("PATH", path)
        .env("CALLS", &calls)
        .env("BOOT_STATE", &state);
    run(&mut service)?;
    let rows = fs::read_to_string(&calls)?;
    let expected = [
        ("running", "pid"),
        ("broken", "pid"),
        ("broken", "start"),
        ("space name", "pid"),
        ("space name", "start"),
        ("last", "pid"),
        ("last", "start"),
    ]
    .map(|(name, command)| format!("{}|--name|{name}|{command}\n", data.display()))
    .concat();
    assert_eq!(rows, expected);
    assert!(
        fs::read_to_string(data.join("boot.log"))?
            .to_lowercase()
            .contains("failed")
    );
    fs::write(module.join("disable"), "")?;
    run(&mut service)?;
    assert_eq!(fs::read_to_string(calls)?, rows);
    Ok(())
}
