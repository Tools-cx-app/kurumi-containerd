use std::process::Command;

use super::*;

#[test]
#[allow(clippy::zombie_processes)] // Reaped through helper process::waitpid below.
fn recognizes_only_namespace_reboot_signal() {
    let child = Command::new("sh")
        .args(["-c", "kill -HUP $$"])
        .spawn()
        .unwrap();
    let pid = i32::try_from(child.id()).unwrap();
    assert!(is_reboot_status(waitpid(pid, false).unwrap()));

    let pid = 42;
    assert!(!is_reboot_status(WaitStatus::Exited(pid, 0)));
    assert!(!is_reboot_status(WaitStatus::Exited(pid, 249)));
}
