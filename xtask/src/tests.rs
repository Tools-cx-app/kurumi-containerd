// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

#[path = "tests/android.rs"]
mod android_scripts;
#[cfg(target_os = "linux")]
#[path = "tests/native.rs"]
mod native;

use std::{
    ffi::OsString,
    fs::{self, File},
    io::Read,
};

use clap::Parser;
use tar::Archive;
use xz2::read::XzDecoder;
use zip::ZipArchive;

use crate::{
    Cli, Task, android,
    build::{ANDROID_ABIS, BuildFlags, Target, build_command},
    capture, command, package, release_version, run, validate_version, workspace,
};

#[test]
fn builds_preserve_profile_features_and_android_platform() {
    for (target, abi) in [
        ("aarch64-linux-android", "arm64-v8a"),
        ("armv7-linux-androideabi", "armeabi-v7a"),
        ("x86_64-linux-android", "x86_64"),
        ("i686-linux-android", "x86"),
    ] {
        let target = target.parse::<Target>().unwrap();
        assert_eq!(target, abi.parse().unwrap());
        for release in [false, true] {
            let flags = BuildFlags {
                release,
                ..BuildFlags::default()
            };
            let command = build_command(Some(target), &flags).unwrap();
            let args = command.get_args().collect::<Vec<_>>();
            assert_eq!(&args[..5], ["ndk", "--platform", "21", "--target", abi]);
            assert_eq!(args.contains(&"--release".as_ref()), release);
            assert_eq!(
                args.contains(&"kurumi-containerd/servicectl".as_ref()),
                release
            );
            assert!(args.windows(2).any(|pair| pair == ["--exclude", "xtask"]));
        }
        assert!(
            build_command(
                Some(target),
                &BuildFlags {
                    with_tui: true,
                    ..BuildFlags::default()
                }
            )
            .is_err()
        );
    }
    let flags = BuildFlags {
        release: true,
        with_tui: true,
        verbose: true,
    };
    let command =
        build_command(Some("aarch64-unknown-linux-gnu".parse().unwrap()), &flags).unwrap();
    let args = command.get_args().map(OsString::from).collect::<Vec<_>>();
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--package", "kurumi-containerd"])
    );
    assert!(args.contains(&"kurumi-containerd/tui".into()));
    assert!(args.contains(&"kurumi-containerd/servicectl".into()));
    assert!(args.contains(&"--verbose".into()));
}

#[test]
fn failed_subprocesses_return_errors() {
    assert!(run(command("sh").args(["-c", "exit 7"])).is_err());
    let error = capture(command("sh").args(["-c", "echo failed >&2; exit 9"]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("failed") && error.contains('9'));
}

#[test]
fn rejects_invalid_versions_and_version_codes() {
    assert_eq!(
        release_version(concat!("v", env!("CARGO_PKG_VERSION"))).unwrap(),
        env!("CARGO_PKG_VERSION")
    );
    assert!(release_version("v999.0.0").is_err());
    for version in ["0.3.0", "0.3.1-rc.1", "0.3.1-rc-2"] {
        assert!(validate_version(version).is_ok());
    }
    for version in ["", "v0.3.0", "0.3", "0.3.0-", "../0.3.0", "0.3.0\n"] {
        assert!(validate_version(version).is_err());
    }
    for code in ["0", "-1", "2147483648"] {
        assert!(
            Cli::try_parse_from([
                "xtask",
                "android-module",
                "--binaries",
                ".",
                "--version-code",
                code
            ])
            .is_err()
        );
    }
}

#[test]
fn archives_preserve_names_payloads_and_permissions() {
    let work = tempfile::tempdir().unwrap();
    let binary = work.path().join("runtime");
    fs::write(&binary, b"test runtime").unwrap();
    for target in ["aarch64-linux-android", "x86_64-unknown-linux-musl"] {
        for release in [false, true] {
            let args = package::PackageArgs {
                target: target.parse().unwrap(),
                flags: BuildFlags {
                    release,
                    ..BuildFlags::default()
                },
                binary: Some(binary.clone()),
                output: work.path().join("dist"),
                version: "0.3.1-rc.1".into(),
            };
            package::package(&args).unwrap();
            let name = package::artifact_name(&args);
            assert_eq!(name.ends_with("-debug"), !release);
            assert!(name.contains(args.target.platform()));
            let file = File::open(args.output.join(format!("{name}.tar.xz"))).unwrap();
            let mut archive = Archive::new(XzDecoder::new(file));
            let mut count = 0;
            for entry in archive.entries().unwrap() {
                let mut entry = entry.unwrap();
                let path = entry.path().unwrap().into_owned();
                let file = path.strip_prefix(&name).unwrap();
                let is_binary = file == std::path::Path::new("kurumi-containerd");
                assert_eq!(
                    entry.header().mode().unwrap(),
                    if is_binary { 0o755 } else { 0o644 }
                );
                assert_eq!(entry.header().uid().unwrap(), 0);
                assert_eq!(entry.header().gid().unwrap(), 0);
                let mut contents = Vec::new();
                entry.read_to_end(&mut contents).unwrap();
                let expected = if is_binary {
                    fs::read(&binary)
                } else {
                    fs::read(workspace().join(file))
                };
                assert_eq!(contents, expected.unwrap());
                count += 1;
            }
            assert_eq!(count, 5);
        }
    }
}

#[test]
fn module_requires_all_abis_and_preserves_zip_payloads() {
    let work = tempfile::tempdir().unwrap();
    let args = android::ModuleArgs {
        binaries: work.path().join("artifacts"),
        output: work.path().join("dist"),
        version: "0.3.1-rc.1".into(),
        version_code: 42,
    };
    for abi in ANDROID_ABIS {
        let directory = args
            .binaries
            .join(format!("kurumi-containerd-android-{abi}"));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("kurumi-containerd"), abi).unwrap();
    }
    let missing = args
        .binaries
        .join("kurumi-containerd-android-x86/kurumi-containerd");
    fs::write(&missing, "").unwrap();
    assert!(android::package(&args).is_err());
    assert!(!args.output.exists());
    fs::write(missing, "x86").unwrap();
    android::package(&args).unwrap();
    let file = File::open(
        args.output
            .join("kurumi-containerd-0.3.1-rc.1-android-module.zip"),
    )
    .unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut metadata = String::new();
    archive
        .by_name("module.prop")
        .unwrap()
        .read_to_string(&mut metadata)
        .unwrap();
    assert!(metadata.contains("version=0.3.1-rc.1\nversionCode=42\n"));
    for (path, expected, mode) in ANDROID_ABIS
        .into_iter()
        .map(|abi| {
            (
                format!("binaries/{abi}/kurumi-containerd"),
                abi.as_bytes().to_vec(),
                0o755,
            )
        })
        .chain(
            [
                "customize.sh",
                "service.sh",
                "bin/kurumi-containerd",
                "system/bin/kurumi-containerd",
                "config.example.json",
            ]
            .into_iter()
            .map(|file| {
                let source = if file == "config.example.json" {
                    workspace().join(file)
                } else {
                    workspace().join("android/module").join(file)
                };
                let mode = if file == "config.example.json" {
                    0o644
                } else {
                    0o755
                };
                (file.to_owned(), fs::read(source).unwrap(), mode)
            }),
        )
    {
        let mut entry = archive.by_name(&path).unwrap();
        assert_eq!(entry.unix_mode().unwrap() & 0o777, mode);
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).unwrap();
        assert_eq!(contents, expected);
    }
}

#[test]
fn tui_variant_keeps_historical_package_suffix_order() {
    let cli = Cli::parse_from([
        "xtask",
        "package",
        "--target",
        "x86_64-unknown-linux-gnu",
        "--with-tui",
    ]);
    let Task::Package(args) = cli.command else {
        panic!("expected package")
    };
    assert!(package::artifact_name(&args).ends_with("-with-tui-debug"));
}
