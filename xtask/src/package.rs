// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    fs::{self, File},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use tar::{Builder, Header};
use tempfile::NamedTempFile;
use xz2::write::XzEncoder;

use crate::{
    build::{BuildFlags, Target},
    capture, cargo, command, run, validate_version, workspace,
};

const DOCUMENTS: [&str; 4] = [
    "LICENSE",
    "README.md",
    "README_CN.md",
    "config.example.json",
];

#[derive(Args)]
pub struct PackageArgs {
    /// Linux target triple, Android target triple, or Android ABI.
    #[arg(long)]
    pub target: Target,
    #[command(flatten)]
    pub flags: BuildFlags,
    /// Use an existing binary at this path instead of the Cargo target directory.
    #[arg(long)]
    pub binary: Option<PathBuf>,
    /// Destination directory, relative to the workspace root.
    #[arg(long, default_value = "dist")]
    pub output: PathBuf,
    /// Package version; defaults to the workspace version.
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    pub version: String,
}

pub fn validate(args: &PackageArgs) -> Result<()> {
    validate_version(&args.version)?;
    args.flags.validate(Some(args.target))
}

pub fn package(args: &PackageArgs) -> Result<()> {
    validate(args)?;
    let binary = if let Some(binary) = &args.binary {
        workspace().join(binary)
    } else {
        let metadata: serde_json::Value = serde_json::from_str(&capture(cargo().args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
        ]))?)?;
        let directory = metadata["target_directory"]
            .as_str()
            .context("Cargo metadata is missing target_directory")?;
        Path::new(directory)
            .join(args.target.triple)
            .join(args.flags.profile())
            .join("kurumi-containerd")
    };
    ensure!(
        binary.is_file() && binary.metadata()?.len() > 0,
        "Missing or empty binary: {}",
        binary.display()
    );
    let output = workspace().join(&args.output);
    fs::create_dir_all(&output)?;
    let name = artifact_name(args);
    archive(&binary, &output.join(format!("{name}.tar.xz")), &name)?;
    if let Some((deb_arch, rpm_arch)) = args.target.native_arches() {
        native_packages(args, &binary, &output, &name, deb_arch, rpm_arch)?;
    }
    println!("Packaged {name} in {}", output.display());
    Ok(())
}

pub fn artifact_name(args: &PackageArgs) -> String {
    format!(
        "kurumi-containerd-{}-{}-{}{}{}",
        args.version,
        args.target.platform(),
        args.target,
        if args.flags.with_tui { "-with-tui" } else { "" },
        if args.flags.release { "" } else { "-debug" }
    )
}

fn archive(binary: &Path, destination: &Path, name: &str) -> Result<()> {
    let temporary = NamedTempFile::new_in(destination.parent().context("Missing output parent")?)?;
    let encoder = XzEncoder::new(temporary.reopen()?, 6);
    let mut archive = Builder::new(encoder);
    append_file(
        &mut archive,
        binary,
        &format!("{name}/kurumi-containerd"),
        0o755,
    )?;
    for document in DOCUMENTS {
        append_file(
            &mut archive,
            &workspace().join(document),
            &format!("{name}/{document}"),
            0o644,
        )?;
    }
    archive.into_inner()?.finish()?;
    temporary.persist(destination)?;
    Ok(())
}

fn append_file(
    archive: &mut Builder<XzEncoder<File>>,
    source: &Path,
    destination: &str,
    mode: u32,
) -> Result<()> {
    let mut file = File::open(source).with_context(|| format!("Read {}", source.display()))?;
    let mut header = Header::new_gnu();
    header.set_size(file.metadata()?.len());
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    archive.append_data(&mut header, destination, &mut file)?;
    Ok(())
}

fn install(source: &Path, destination: &Path, mode: u32) -> Result<()> {
    fs::create_dir_all(destination.parent().context("Missing install parent")?)?;
    fs::copy(source, destination)?;
    fs::set_permissions(destination, fs::Permissions::from_mode(mode))?;
    Ok(())
}

fn native_packages(
    args: &PackageArgs,
    binary: &Path,
    output: &Path,
    name: &str,
    deb_arch: &str,
    rpm_arch: &str,
) -> Result<()> {
    let work = tempfile::tempdir()?;
    let payload = work.path().join("rpm/SOURCES/payload");
    install(binary, &payload.join("usr/bin/kurumi-containerd"), 0o755)?;
    for document in DOCUMENTS {
        install(
            &workspace().join(document),
            &payload
                .join("usr/share/doc/kurumi-containerd")
                .join(document),
            0o644,
        )?;
    }
    let version = args.version.replacen('-', "~", 1).replace('-', ".");
    let depends = debian_dependencies(binary)?;
    let control = payload.join("DEBIAN");
    fs::create_dir(&control)?;
    fs::write(
        control.join("control"),
        format!(
            "Package: kurumi-containerd\nVersion: {version}\nSection: admin\n\
             Priority: optional\nArchitecture: {deb_arch}\n\
             Maintainer: KurumiContainerd contributors <noreply@kurumi-containerd.invalid>\n\
             Depends: {depends}\n\
             Description: Privileged Linux container runtime and command-line tool\n"
        ),
    )?;
    let deb = output.join(format!("{name}.deb"));
    run(command("dpkg-deb")
        .args(["--build", "--root-owner-group"])
        .arg(&payload)
        .arg(&deb))?;
    ensure!(
        capture(command("dpkg-deb").arg("-f").arg(&deb).arg("Architecture"))?.trim() == deb_arch
            && capture(command("dpkg-deb").arg("-f").arg(&deb).arg("Version"))?.trim() == version,
        "Debian package metadata does not match the requested architecture/version"
    );
    fs::remove_dir_all(control)?;

    let rpm_top = work.path().join("rpm");
    for directory in ["BUILD", "BUILDROOT", "RPMS", "SPECS", "SRPMS"] {
        fs::create_dir_all(rpm_top.join(directory))?;
    }
    let spec = rpm_top.join("SPECS/kurumi-containerd.spec");
    fs::write(
        &spec,
        format!(
            "Name: kurumi-containerd\nVersion: {version}\nRelease: 1\n\
             Summary: Privileged Linux container runtime and command-line tool\n\
             License: GPL-3.0-only\n\n%description\n\
             Privileged Linux container runtime and command-line tool.\n\n\
             %prep\n%build\n%install\nmkdir -p %{{buildroot}}\n\
             cp -a %{{_sourcedir}}/payload/. %{{buildroot}}/\n\n%files\n\
             %attr(0755,root,root) /usr/bin/kurumi-containerd\n\
             %dir %attr(0755,root,root) /usr/share/doc/kurumi-containerd\n\
             %license %attr(0644,root,root) /usr/share/doc/kurumi-containerd/LICENSE\n\
             %doc %attr(0644,root,root) /usr/share/doc/kurumi-containerd/README.md\n\
             %doc %attr(0644,root,root) /usr/share/doc/kurumi-containerd/README_CN.md\n\
             %doc %attr(0644,root,root) /usr/share/doc/kurumi-containerd/config.example.json\n"
        ),
    )?;
    // Preserve Cargo output: host strip/debug tools cannot process every target.
    run(command("rpmbuild")
        .args(["--target", rpm_arch, "--define"])
        .arg(format!("_topdir {}", rpm_top.display()))
        .args([
            "--define",
            "_build_id_links none",
            "--define",
            "debug_package %{nil}",
            "--define",
            "__os_install_post %{nil}",
            "-bb",
        ])
        .arg(spec))?;
    let rpms = fs::read_dir(rpm_top.join("RPMS").join(rpm_arch))?
        .map(|entry| Ok(entry?.path()))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "rpm"))
        .collect::<Vec<_>>();
    ensure!(rpms.len() == 1, "Expected one RPM, found {}", rpms.len());
    let rpm = output.join(format!("{name}.rpm"));
    fs::copy(&rpms[0], &rpm)?;
    ensure!(
        capture(command("rpm").args(["-qp", "--qf", "%{ARCH}"]).arg(&rpm))? == rpm_arch
            && capture(command("rpm").args(["-qp", "--qf", "%{VERSION}"]).arg(&rpm))? == version,
        "RPM metadata does not match the requested architecture/version"
    );
    Ok(())
}

/// Inspects target ELF metadata without executing a cross-built binary.
fn debian_dependencies(binary: &Path) -> Result<String> {
    let versions = capture(command("readelf").arg("--version-info").arg(binary))?;
    let glibc = versions
        .split("GLIBC_")
        .skip(1)
        .filter_map(|suffix| {
            let version = suffix
                .split(|character: char| !character.is_ascii_digit() && character != '.')
                .next()?;
            let parts = version
                .split('.')
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            (parts.len() >= 2).then_some((parts, version))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .context("No GLIBC version found in target ELF")?;
    let mut depends = format!("libc6 (>= {})", glibc.1);
    let dynamic = capture(command("readelf").arg("-d").arg(binary))?;
    for line in dynamic.lines().filter(|line| line.contains("(NEEDED)")) {
        let library = line
            .split_once('[')
            .and_then(|(_, suffix)| suffix.split_once(']'))
            .map(|(library, _)| library)
            .context("Malformed readelf shared library entry")?;
        match library {
            "libc.so.6" | "libm.so.6" | "libpthread.so.0" | "libdl.so.2" | "librt.so.1"
            | "libutil.so.1" => {}
            "libgcc_s.so.1" => depends.push_str(", libgcc-s1"),
            "liblzma.so.5" => depends.push_str(", liblzma5"),
            library if library.starts_with("ld-linux") && library.contains(".so.") => {}
            _ => bail!("Unmapped Debian dependency: {library}"),
        }
    }
    Ok(depends)
}
