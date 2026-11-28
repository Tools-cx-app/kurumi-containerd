// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::path::Path;

use clap::CommandFactory;

use super::*;

#[test]
fn cli_definition_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn every_command_has_help_text() {
    for command in Cli::command().get_subcommands() {
        assert!(
            command.get_about().is_some(),
            "{} has no help text",
            command.get_name()
        );
    }
}

#[test]
fn exec_alias_parses_as_run() {
    let cli = Cli::try_parse_from(["kurumi-containerd", "exec", "sh", "-c", "true"]).unwrap();
    assert!(matches!(
        cli.command,
        Commands::Run { command } if command == ["sh", "-c", "true"]
    ));
}

#[test]
fn enter_accepts_custom_login_shell() {
    for (arguments, expected_user, expected_shell) in [
        (vec!["enter"], "root", None),
        (
            vec!["enter", "--shell", "/bin/ash"],
            "root",
            Some("/bin/ash"),
        ),
        (
            vec!["enter", "developer", "--shell", "/bin/sh"],
            "developer",
            Some("/bin/sh"),
        ),
        (
            vec!["enter", "--shell", "/bin/busybox ash"],
            "root",
            Some("/bin/busybox ash"),
        ),
    ] {
        let cli =
            Cli::try_parse_from(std::iter::once("kurumi-containerd").chain(arguments)).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Enter { user, shell }
                if user == expected_user && shell.as_deref() == expected_shell
        ));
    }
}

#[test]
fn install_parses_local_archive_options() {
    let cli = Cli::try_parse_from([
        "kurumi-containerd",
        "install",
        "rootfs.tar.zst",
        "--size",
        "8G",
        "--force",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Commands::Install { archive, size: Some(size), force: true }
            if archive == Path::new("rootfs.tar.zst") && size == 8 * 1024_u64.pow(3)
    ));
}

#[test]
fn package_import_requires_explicit_mappings_and_separates_inspection() {
    let cli = Cli::try_parse_from([
        "kurumi",
        "--name",
        "copy",
        "import",
        "x.kurumi",
        "--map",
        "mount.0=/new dir/a=b",
    ])
    .unwrap();
    assert!(
        matches!(cli.command, Commands::Import { mappings, inspect: false, .. } if mappings == [("mount.0".into(), PathBuf::from("/new dir/a=b"))])
    );
    for options in [
        vec!["--map", "bad"],
        vec!["--inspect", "--size", "8G"],
        vec!["--inspect", "--map", "mount.0=/data"],
    ] {
        assert!(
            Cli::try_parse_from(["kurumi", "import", "x.kurumi"].into_iter().chain(options))
                .is_err()
        );
    }
}

#[test]
fn rejects_config_options() {
    for flag in ["-c", "--config"] {
        assert!(Cli::try_parse_from(["kurumi-containerd", flag, "old.toml", "start"]).is_err());
    }
    assert!(Cli::try_parse_from(["kurumi-containerd", "start"]).is_ok());
}

#[test]
fn parses_binary_sizes() {
    assert_eq!(parse_size("512M").unwrap(), 512 * 1024_u64.pow(2));
    assert_eq!(parse_size("8G").unwrap(), 8 * 1024_u64.pow(3));
    assert_eq!(parse_size("16GiB").unwrap(), 16 * 1024_u64.pow(3));
}

#[test]
fn rejects_invalid_sizes() {
    for value in ["0", "-1G", "1PB", "18446744073709551615T"] {
        assert!(parse_size(value).is_err(), "accepted {value}");
    }
}
