#!/usr/bin/env python3
"""Exercise actual module scripts in a temporary simulated Android environment."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
MODULE = ROOT / "android/module"
assert (MODULE / "customize.sh").is_file(), "Android module installer missing"


def run(script, env, check=True):
    return subprocess.run(["sh", str(script)], env=dict(os.environ, **env),
                          check=check, capture_output=True, text=True, timeout=15)


with tempfile.TemporaryDirectory() as directory:
    work = Path(directory)
    data = work / "data"
    tools = work / "tools"
    tools.mkdir()
    for arch, abi in (("arm64", "arm64-v8a"), ("arm", "armeabi-v7a"),
                      ("x64", "x86_64"), ("x86", "x86")):
        module = work / arch
        shutil.copytree(MODULE, module)
        for payload in ("arm64-v8a", "armeabi-v7a", "x86_64", "x86"):
            binary = module / "binaries" / payload / "kurumi-containerd"
            binary.parent.mkdir(parents=True)
            binary.write_text(payload)
        installer = work / "install.sh"
        installer.write_text('ui_print() { :; }; abort() { exit 1; }; '
                             'set_perm() { chmod "$4" "$1"; }; '
                             '. "$MODPATH/customize.sh"\n')
        env = dict(MODPATH=str(module), ARCH=arch, API="21", BOOTMODE="true",
                   KURUMI_CONTAINERD_HOME=str(data))
        unsupported = work / f"unsupported-{arch}"
        shutil.copytree(MODULE, unsupported)
        result = run(installer, dict(MODPATH=str(unsupported), ARCH="riscv64", API="21",
                                      BOOTMODE="true", KURUMI_CONTAINERD_HOME=str(data)), check=False)
        assert result.returncode
        result = run(installer, dict(MODPATH=str(unsupported), ARCH=arch, API="20",
                                      BOOTMODE="true", KURUMI_CONTAINERD_HOME=str(data)), check=False)
        assert result.returncode
        run(installer, env)
        assert (module / "bin/kurumi-containerd.bin").read_text() == abi
        assert not (module / "binaries").exists()
        assert data.stat().st_mode & 0o777 == 0o700
        (data / "autostart.txt").write_text("keep\n")
        assert (data / "autostart.txt").read_text() == "keep\n"

    module = work / "arm64"
    calls = work / "calls"
    binary = module / "bin/kurumi-containerd.bin"
    binary.write_text('#!/bin/sh\n'
                      'printf "%s|%s|%s|%s\\n" "$HOME" "$1" "$2" "$3" >> "$CALLS"\n'
                      '[ "$(cat "$BOOT_STATE")" = 1 ] || exit 99\n'
                      '[ "$3" != pid ] || { [ "$2" = running ]; exit $?; }\n'
                      '[ "$2" != broken ]\n')
    binary.chmod(0o755)
    state = work / "boot-state"
    state.write_text("0")
    for name, content in {
        "getprop": '#!/bin/sh\ncat "$BOOT_STATE"\n',
        "sleep": '#!/bin/sh\nprintf 1 > "$BOOT_STATE"\n',
    }.items():
        tool = tools / name
        tool.write_text(content)
        tool.chmod(0o755)
    (data / "autostart.txt").write_text("# comment\n\nrunning\nbroken\nspace name\nlast")
    env = dict(PATH=f"{tools}:{os.environ['PATH']}", CALLS=str(calls),
               BOOT_STATE=str(state), KURUMI_CONTAINERD_HOME=str(data))
    run(module / "service.sh", env)
    rows = calls.read_text().splitlines()
    assert rows == [f"{data}|--name|{name}|{command}" for name, command in
                    (("running", "pid"), ("broken", "pid"), ("broken", "start"),
                     ("space name", "pid"), ("space name", "start"),
                     ("last", "pid"), ("last", "start"))], rows
    assert "failed" in (data / "boot.log").read_text().lower()
    (module / "disable").touch()
    run(module / "service.sh", env)
    assert calls.read_text().splitlines() == rows
    result = run(module / "bin/kurumi-containerd", env, check=False)
    assert result.returncode != 126  # wrapper executes payload, not a shell permission error

    binaries = work / "artifacts"
    for abi in ("arm64-v8a", "armeabi-v7a", "x86_64", "x86"):
        payload = binaries / f"kurumi-containerd-android-{abi}" / "kurumi-containerd"
        payload.parent.mkdir(parents=True)
        payload.write_bytes(abi.encode())
    subprocess.run(["python3", str(ROOT / ".github/scripts/package-android-module.py"),
                    "--binaries", str(binaries), "--output", str(work / "dist"),
                    "--version", "0.2.1", "--version-code", "42"], check=True)
    with zipfile.ZipFile(work / "dist/kurumi-containerd-0.2.1-android-module.zip") as archive:
        metadata = archive.read("module.prop").decode()
        assert "version=0.2.1\n" in metadata and "versionCode=42\n" in metadata
        for abi in ("arm64-v8a", "armeabi-v7a", "x86_64", "x86"):
            assert archive.read(f"binaries/{abi}/kurumi-containerd") == abi.encode()
        assert "service.sh" in archive.namelist()
    print("Android module smoke tests passed")
