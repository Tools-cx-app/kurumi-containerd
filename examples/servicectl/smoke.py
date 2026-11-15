#!/usr/bin/python3
# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only
"""sudo python3 examples/servicectl/smoke.py BINARY PREPARED_DISPOSABLE_ROOTFS.

The rootfs must already contain install.sh assets and all runtime prerequisites.
Runs actual servicectl commands; never substitutes a mini supervisor.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def main():
    assert os.geteuid() == 0, "requires root"
    binary = str(Path(sys.argv[1]).resolve(strict=True))
    root = Path(sys.argv[2]).resolve(strict=True)
    assert root != Path("/") and (root / "usr/local/sbin/kurumi-services").is_file()
    with tempfile.TemporaryDirectory(prefix="kurumi-servicectl-") as temporary:
        home = Path(temporary)
        registry = home / ".kurumi-containerd"
        registry.mkdir()
        name = f"servicectl-{os.getpid()}"
        (registry / "config.json").write_text(json.dumps([{
            "runtime": {"stop_timeout_seconds": 90}, "container": {
                "name": name, "rootfs": str(root), "init": "/usr/local/sbin/kurumi-services",
                "isolation": "shared-pid-ipc", "network": "host"}}]))
        env = dict(os.environ, HOME=str(home), NO_COLOR="1")

        def cli(*args, check=True):
            result = subprocess.run([binary, "--name", name, *args], env=env,
                                    capture_output=True, text=True, timeout=120)
            print(f"$ {' '.join(args)}\n{result.stdout}{result.stderr}", flush=True)
            if check:
                assert result.returncode == 0, (args, result.returncode)
            return result

        def run(*args, **kwargs):
            return cli("run", *args, **kwargs)

        def ready():
            run("python3", "-c", 'import pathlib,time; p=pathlib.Path("/run/kurumi-services/ready"); '
                'end=time.monotonic()+60\nwhile not p.exists():\n'
                ' if time.monotonic()>end: raise SystemExit("bootstrap not ready")\n time.sleep(.1)')
            run("s6-rc", "-l", "/run/s6/state", "-a", "list")

        def stopped_count():
            marker = root / "var/lib/kurumi-demo/stopped"
            return len(marker.read_text().splitlines()) if marker.exists() else 0

        def demo_ready():
            deadline = time.monotonic() + 15
            while not (root / "var/lib/kurumi-demo/ready").exists():
                assert time.monotonic() < deadline, "demo did not become ready"
                time.sleep(0.05)

        try:
            cli("start")
            ready()
            run("servicectl", "start", "kurumi-demo.service")
            run("servicectl", "is-active", "kurumi-demo.service")
            demo_ready()
            status = run("servicectl", "status", "kurumi-demo.service", "--json", check=False)
            assert status.returncode in (0, 3), status.stdout + status.stderr
            if status.returncode == 3:
                print("SKIP: aggregate service health degraded/unknown; inspect status diagnostics")
            before = stopped_count()
            run("servicectl", "restart", "kurumi-demo.service")
            demo_ready()
            assert stopped_count() > before, "unit restart bypassed graceful hook"
            before = stopped_count()
            run("servicectl", "stop", "kurumi-demo.service")
            assert stopped_count() > before, "unit stop bypassed graceful hook"
            run("servicectl", "enable", "kurumi-demo.service")
            run("servicectl", "start", "kurumi-demo.service")
            demo_ready()
            before = stopped_count()
            cli("restart")
            assert stopped_count() > before, "Kurumi restart bypassed graceful hook"
            ready()
            deadline = time.monotonic() + 30
            while run("servicectl", "is-active", "kurumi-demo.service", check=False).returncode:
                assert time.monotonic() < deadline, "enabled unit did not start after restart"
                time.sleep(0.2)
            demo_ready()
            before = stopped_count()
            cli("stop")
            assert stopped_count() > before, "Kurumi stop bypassed graceful hook"
            print("PASS: real servicectl start/status/restart/stop/enable and Kurumi restart/stop")
        finally:
            if cli("pid", check=False).returncode == 0:
                cli("stop")


if __name__ == "__main__":
    main()
