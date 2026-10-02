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
    assert "checking host capabilities" in output, output
    assert "HOME" not in output and "config pointer" not in output, output
    status, output = run("info")
    assert status != 0 and "HOME" in output, output

    with tempfile.TemporaryDirectory(prefix="kurumi-pointer-") as temporary:
        home = Path(temporary)
        environment["HOME"] = str(home)
        environment["KURUMI_CONTAINERD_CONFIG"] = str(home / "obsolete.toml")
        pointer = home / ".kurumi-containerd" / "config.json"
        status, output = run("info")
        assert status != 0 and str(pointer) in output, output
        assert "obsolete.toml" not in output, output
        pointer.parent.mkdir()
        pointer.write_text(json.dumps([{"name": "Display name", "file": "container.toml"}]))
        toml = pointer.parent / "container.toml"
        status, output = run("install", str(home / "missing.tar"))
        assert status != 0 and str(toml) in output, output
        # Fail during TOML validation, before privileged runtime operations.
        toml.write_text("[runtime]\n[container]\nname='bad/name'\nrootfs='rootfs'\n")
        status, output = run("install", str(home / "missing.tar"))
        assert status != 0 and "container name" in output, output
        pointer.write_text(json.dumps([
            {"name": "Display name", "file": "container.toml"},
            {"name": "other", "file": "other.toml"},
        ]))
        status, output = run("info")
        assert status != 0 and "--name" in output, output
        status, output = run("--name", "other", "install", str(home / "missing.tar"))
        assert status != 0 and str(pointer.parent / "other.toml") in output, output
        status, output = run("--name", "missing", "info")
        assert status != 0 and "no config pointer named" in output, output
        for flag in ["-c", "--config"]:
            status, output = run(flag, str(toml), "info")
            assert status != 0 and "unexpected argument" in output, output
    print("PASS: HOME lookup, check bypass, legacy rejection, JSON-to-TOML install loading")


if __name__ == "__main__":
    main()
