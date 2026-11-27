// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

#[test]
fn command_lookup_uses_path_and_skips_non_executable_files() {
    use std::os::unix::fs::PermissionsExt as _;
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    for path in [&first, &second] {
        fs::create_dir(path).unwrap();
        fs::write(path.join("tool"), "#!/bin/sh\n").unwrap();
    }
    fs::set_permissions(second.join("tool"), fs::Permissions::from_mode(0o755)).unwrap();
    let env = [CString::new(format!("PATH={}:{}", first.display(), second.display())).unwrap()];
    assert_eq!(
        resolve_container_command("tool", &env),
        Some(second.join("tool"))
    );
    fs::set_permissions(first.join("tool"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        resolve_container_command("tool", &env),
        Some(first.join("tool"))
    );
    assert_eq!(resolve_container_command("missing", &env), None);
}

#[test]
fn parses_login_account() {
    let source = "root:x:0:0:root:/root:/bin/bash\nuser:x:1000:1000::/home/user:/bin/sh\n";
    assert_eq!(
        parse_passwd(source, "user"),
        Some(PasswdEntry {
            uid: 1000,
            gid: 1000,
            home: "/home/user".to_owned(),
            shell: "/bin/sh".to_owned()
        })
    );
    assert_eq!(parse_passwd(source, "missing"), None);
}

#[test]
fn parses_login_shell_command_without_evaluating_arguments() {
    for (command, expected) in [
        ("/bin/ash", vec!["/bin/ash", "-l"]),
        ("/bin/busybox ash", vec!["/bin/busybox", "ash", "-l"]),
        (
            "'/opt/my shell' --label 'two words' \"$HOME\"",
            vec!["/opt/my shell", "--label", "two words", "$HOME", "-l"],
        ),
    ] {
        let arguments = login_shell_arguments(command).unwrap();
        let arguments: Vec<_> = arguments.iter().map(|arg| arg.to_str().unwrap()).collect();
        assert_eq!(arguments, expected);
    }
    for command in [
        "",
        "ash",
        "'' ash",
        "/bin/ash '",
        "/bin/ash \\",
        "/bin/ash a\0b",
    ] {
        assert!(
            login_shell_arguments(command).is_err(),
            "accepted {command:?}"
        );
    }
}

#[test]
fn rejects_unsafe_login_names() {
    assert!(valid_login_name("root"));
    assert!(valid_login_name("service-user_1"));
    assert!(!valid_login_name(""));
    assert!(!valid_login_name("../../root"));
    assert!(!valid_login_name("user:name"));
}

#[test]
fn rejects_writable_runtime_directory_parent() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o777)).unwrap();
    let account = PasswdEntry {
        uid: 1000,
        gid: 1000,
        home: "/home/developer".to_owned(),
        shell: "/bin/sh".to_owned(),
    };
    assert!(prepare_runtime_directory(directory.path(), &account).is_err());
    assert!(!directory.path().join("1000").exists());
}

#[test]
fn preserves_command_exit_status() {
    assert_eq!(command_status(WaitStatus::Exited(123, 7)).unwrap(), 7);
    assert_eq!(
        command_status(WaitStatus::Signaled(123, Signal::Interrupt.into(), false)).unwrap(),
        130
    );
}

#[test]
fn captures_actual_network_and_cgroup_namespaces_without_configuration() {
    let process = Process::myself().unwrap();
    let namespaces = capture_namespaces(&process, IsolationMode::Isolated).unwrap();
    for name in ["net", "cgroup"] {
        if let Ok(expected) = process.open_relative(format!("ns/{name}")) {
            let (_, captured) = namespaces
                .iter()
                .find(|(captured, _)| *captured == name)
                .unwrap();
            assert_eq!(
                captured.metadata().unwrap().ino(),
                expected.metadata().unwrap().ino()
            );
        }
    }
}

#[test]
#[cfg(feature = "servicectl")]
fn shared_namespace_capture_omits_pid_and_ipc() {
    let process = Process::myself().unwrap();
    let namespaces = capture_namespaces(&process, IsolationMode::SharedPidIpc).unwrap();
    assert!(
        !namespaces
            .iter()
            .any(|(name, _)| matches!(*name, "pid" | "ipc"))
    );
    assert!(namespaces.iter().any(|(name, _)| *name == "mnt"));
}
