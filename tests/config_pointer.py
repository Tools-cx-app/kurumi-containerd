# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only

"""Unprivileged CLI checks: python3 tests/config_pointer.py <runtime-binary>."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    binary = str(Path(sys.argv[1]).resolve())
    environment = dict(os.environ, NO_COLOR="1")
    environment.pop("HOME", None)

    def run(*args):
        result = subprocess.run(
            [binary, *args], env=environment, capture_output=True, text=True, timeout=20
        )
        return result.returncode, result.stdout + result.stderr

    _, output = run("check")
    assert "HOME" not in output and "config.json" not in output, output
    status, output = run("info")
    assert status != 0 and "HOME" in output, output

    with tempfile.TemporaryDirectory(prefix="kurumi-json-") as temporary:
        home = Path(temporary)
        environment["HOME"] = str(home)
        environment["KURUMI_CONTAINERD_CONFIG"] = str(home / "obsolete.toml")
        path = home / ".kurumi-containerd" / "config.json"
        status, output = run("info")
        assert status != 0 and str(path) in output, output
        assert "obsolete.toml" not in output, output
        path.parent.mkdir()
        path.write_text(json.dumps([{"name": "old", "file": "old.toml"}]))
        status, output = run("info")
        assert status != 0 and "container.name" in output, output

        entries = [{"runtime": {}, "container": {"name": "bad/name", "rootfs": "rootfs"}}]
        path.write_text(json.dumps(entries))
        status, output = run("install", str(home / "missing.tar"))
        assert status != 0 and "container name" in output, output
        entries[0]["container"]["name"] = "debian"
        entries.append({"runtime": {}, "container": {"name": "other", "rootfs": "other-rootfs"}})
        path.write_text(json.dumps(entries))
        status, output = run("info")
        assert status != 0 and "--name" in output, output
        status, output = run("--name", "other", "info")
        assert status != 0 and str(path.parent / "other-rootfs") in output, output
        status, output = run("--name", "missing", "info")
        assert status != 0 and "no container named" in output, output
        status, output = run("backup", "other", str(home / "backup.tar"))
        assert status != 0 and str(path.parent / "other-rootfs") in output, output
        status, output = run("backup", "Other", str(home / "backup.tar"))
        assert status != 0 and "no container named" in output, output
        status, output = run("--name", "debian", "backup", "other", str(home / "backup.tar"))
        assert status != 0 and "do not supply --name" in output, output
        assert path.read_text() == json.dumps(entries), "read-only commands rewrote JSON"
        for flag in ["-c", "--config"]:
            status, output = run(flag, "old.toml", "info")
            assert status != 0 and "unexpected argument" in output, output
    print("PASS: HOME lookup, check bypass, legacy rejection, unified JSON name and backup selection")


if __name__ == "__main__":
    main()
