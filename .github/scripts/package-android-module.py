#!/usr/bin/env python3
"""Package the four existing Android binaries as a manager-installable module."""
import argparse
from pathlib import Path
import re
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[2]
ABIS = ("arm64-v8a", "armeabi-v7a", "x86_64", "x86")
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binaries", type=Path, required=True)
parser.add_argument("--output", type=Path, default=Path("dist"))
parser.add_argument("--version", default=tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"])
parser.add_argument("--version-code", type=int, required=True)
args = parser.parse_args()
if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", args.version):
    parser.error("Unsupported version")
if not 0 < args.version_code <= 2147483647:
    parser.error("version-code must be a positive 32-bit integer")
payloads = {}
for abi in ABIS:
    binary = args.binaries / f"kurumi-containerd-android-{abi}" / "kurumi-containerd"
    if not binary.is_file() or not binary.stat().st_size:
        parser.error(f"Missing or empty binary: {binary}")
    payloads[abi] = binary
args.output.mkdir(parents=True, exist_ok=True)
destination = args.output / f"kurumi-containerd-{args.version}-android-module.zip"
with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED) as archive:
    archive.writestr("module.prop", f"id=kurumi-containerd\nname=KurumiContainerd\n"
                     f"version={args.version}\nversionCode={args.version_code}\n"
                     "author=KurumiContainerd contributors\n"
                     "description=Linux containers with configurable boot startup\n")
    module = ROOT / "android/module"
    for path in sorted(module.rglob("*")):
        if path.is_file():
            info = zipfile.ZipInfo(path.relative_to(module).as_posix())
            mode = path.stat().st_mode & 0o777
            info.external_attr = ((0o100000 | mode) << 16)
            archive.writestr(info, path.read_bytes(), compress_type=zipfile.ZIP_DEFLATED)
    for abi, binary in payloads.items():
        archive.write(binary, f"binaries/{abi}/kurumi-containerd")
    archive.write(ROOT / "LICENSE", "LICENSE")
    archive.write(ROOT / "kurumi-containerd.example.toml", "kurumi-containerd.example.toml")
print(destination)
