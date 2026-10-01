#!/usr/bin/env python3
"""Smoke-test packaging with a host ELF binary; requires dpkg-deb and rpm tools."""
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
TARGET = {"x86_64": "x86_64", "aarch64": "aarch64"}[platform.machine()] + "-unknown-linux-gnu"

with tempfile.TemporaryDirectory() as directory:
    work = Path(directory)
    for source in ("LICENSE", "README.md", "README_CN.md", "kurumi-containerd.example.toml"):
        shutil.copy2(ROOT / source, work / source)
    binary = work / "target" / TARGET / "release" / "kurumi-containerd"
    binary.parent.mkdir(parents=True)
    shutil.copy2(shutil.which("true"), binary)
    for version in ("0.2.1", "0.2.2-rc.1"):
        subprocess.run(
            ["bash", str(ROOT / ".github/scripts/package-release.sh")],
            cwd=work,
            env=dict(os.environ, TARGET=TARGET, PLATFORM="linux", VERSION=version),
            check=True,
        )
        name = f"kurumi-containerd-{version}-linux-{TARGET}"
        with tarfile.open(work / "dist" / f"{name}.tar.xz") as archive:
            entry = archive.getmember(f"{name}/kurumi-containerd")
            assert entry.mode == 0o755 and entry.uid == entry.gid == 0
            assert archive.extractfile(entry).read() == binary.read_bytes()
        deb = work / "dist" / f"{name}.deb"
        extracted = work / f"extracted-{version}"
        subprocess.run(["dpkg-deb", "-x", str(deb), str(extracted)], check=True)
        assert (extracted / "usr/bin/kurumi-containerd").read_bytes() == binary.read_bytes()
        rpm = work / "dist" / f"{name}.rpm"
        listing = subprocess.check_output(["rpm", "-qpl", str(rpm)], text=True)
        assert "/usr/bin/kurumi-containerd\n" in listing
        assert "/usr/share/doc/kurumi-containerd/LICENSE\n" in listing
    subprocess.run(
        ["bash", str(ROOT / ".github/scripts/package-release.sh")],
        cwd=work,
        env=dict(os.environ, TARGET=TARGET, PLATFORM="android", VERSION="0.2.1"),
        check=True,
    )
    assert len(list((work / "dist").glob("*android*"))) == 1
print("Release packaging smoke test passed")
