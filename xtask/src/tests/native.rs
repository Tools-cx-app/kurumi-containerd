// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{env, fs};

use anyhow::{Result, bail};

use crate::{
    build::BuildFlags,
    capture, command,
    package::{PackageArgs, artifact_name, package},
    run, workspace,
};

#[test]
#[ignore = "requires dpkg-deb, rpm and rpmbuild"]
fn native_packages_preserve_payloads_and_prerelease_metadata() -> Result<()> {
    let target = match env::consts::ARCH {
        "x86_64" => "x86_64-unknown-linux-gnu",
        "aarch64" => "aarch64-unknown-linux-gnu",
        arch => bail!("No native packaging fixture for {arch}"),
    };
    let work = tempfile::tempdir()?;
    let binary = work.path().join("runtime");
    for version in ["0.3.0", "0.3.1-rc.1"] {
        for with_tui in [false, true] {
            for release in [false, true] {
                let mut payload = fs::read("/usr/bin/true")?;
                payload.extend_from_slice(format!("{version}-{with_tui}-{release}").as_bytes());
                fs::write(&binary, &payload)?;
                let args = PackageArgs {
                    target: target.parse().unwrap(),
                    flags: BuildFlags {
                        release,
                        with_tui,
                        verbose: false,
                    },
                    binary: Some(binary.clone()),
                    output: work.path().join("dist"),
                    version: version.into(),
                };
                package(&args)?;
                let name = artifact_name(&args);
                let deb = args.output.join(format!("{name}.deb"));
                let extracted = work.path().join(format!("extracted-{name}"));
                run(command("dpkg-deb").arg("-x").arg(&deb).arg(&extracted))?;
                assert_eq!(
                    fs::read(extracted.join("usr/bin/kurumi-containerd"))?,
                    payload
                );
                assert_eq!(
                    fs::read(
                        extracted.join("usr/share/doc/kurumi-containerd/config.example.json")
                    )?,
                    fs::read(workspace().join("config.example.json"))?
                );
                let rpm = args.output.join(format!("{name}.rpm"));
                let listing = capture(command("rpm").arg("-qpl").arg(rpm))?;
                assert!(
                    listing
                        .lines()
                        .any(|line| line == "/usr/bin/kurumi-containerd")
                );
                assert!(
                    listing
                        .lines()
                        .any(|line| line == "/usr/share/doc/kurumi-containerd/LICENSE")
                );
            }
        }
    }
    Ok(())
}
