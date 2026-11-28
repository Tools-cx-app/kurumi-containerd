// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

#[test]
fn merges_installed_and_loaded_system_units_by_exact_name() {
    let body = "[systemd files]\na.service disabled enabled\nb.service enabled enabled\n[systemd units]\nb.service loaded failed failed Test\nc@one.service loaded active running Test\n[end services]\n";
    let (manager, rows) = inventory(body).unwrap();
    assert!(manager == Manager::Systemd);
    let fields: Vec<_> = rows
        .iter()
        .map(|s| (s.name.as_str(), s.state.as_str(), s.startup.as_str()))
        .collect();
    assert_eq!(
        fields,
        [
            ("a.service", "inactive", "disabled"),
            ("b.service", "failed", "enabled"),
            ("c@one.service", "active", "—")
        ]
    );
    assert!(inventory(body.trim_end_matches("[end services]\n")).is_err());
    assert_eq!(
        Operation::Restart
            .command(manager, "c@one.service")
            .unwrap(),
        [
            "systemctl",
            "--system",
            "--no-pager",
            "--no-ask-password",
            "restart",
            "--",
            "c@one.service"
        ]
    );
    assert_eq!(
        Operation::Journal.command(manager, "a.service").unwrap(),
        ["journalctl", "--no-pager", "-n", "100", "-u", "a.service"]
    );
}

#[test]
fn merges_openrc_status_and_default_runlevel_without_exposing_options() {
    let body = "[openrc services]\nsshd\nnet.eth0\n-user\n*\n[openrc status]\nRunlevel: default\n sshd   [ started ]\n[openrc default]\n     sshd | default\n[end services]\n";
    let (manager, rows) = inventory(body).unwrap();
    assert!(manager == Manager::OpenRc);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name, "net.eth0");
    assert_eq!(rows[0].state, "stopped");
    assert_eq!(rows[1].state, "started");
    assert_eq!(rows[1].startup, "default");
    assert_eq!(
        Operation::Stop.command(manager, "sshd").unwrap(),
        ["rc-service", "sshd", "stop"]
    );
    assert_eq!(
        Operation::Disable.command(manager, "sshd").unwrap(),
        ["rc-update", "del", "sshd", "default"]
    );
}
