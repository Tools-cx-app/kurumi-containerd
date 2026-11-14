# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only

"""sudo python3 tests/container_lifecycle.py BINARY LXC_ROOTFS {systemd,openrc}."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def wait_for(description, predicate):
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.25)
    raise AssertionError(f"timed out: {description}")


def exited(pid):
    status = Path(f"/proc/{pid}/status")
    try:
        return "\nState:\tZ" in status.read_text()
    except FileNotFoundError:
        return True


def main():
    assert os.geteuid() == 0, "requires root and namespace/mount privileges"
    binary = str(Path(sys.argv[1]).resolve(strict=True))
    archive = str(Path(sys.argv[2]).resolve(strict=True))
    init_system = sys.argv[3]
    assert init_system in {"systemd", "openrc"}, init_system
    with tempfile.TemporaryDirectory(prefix="kurumi-lifecycle-") as temporary:
        directory = Path(temporary)
        root = directory / "rootfs"
        home = directory / "home"
        registry = home / ".kurumi-containerd"
        registry.mkdir(parents=True)
        name = f"lifecycle-{init_system}-{os.getpid()}"
        (registry / "config.json").write_text(json.dumps([{
            "runtime": {"stop_timeout_seconds": 30},
            "container": {"name": name, "hostname": name, "rootfs": str(root),
                          "init": "/sbin/init", "foreground": False, "network": "none"},
        }]))
        environment = dict(os.environ, HOME=str(home), NO_COLOR="1")

        def cli(*arguments, check=True):
            result = subprocess.run([binary, "--name", name, *arguments], env=environment,
                                    text=True, capture_output=True, timeout=90)
            print(f"$ {' '.join(arguments)}\n{result.stdout}{result.stderr}", flush=True)
            if check:
                assert result.returncode == 0, f"{arguments}: exit {result.returncode}"
            return result

        cli("install", archive)
        # Distribution init reloads the hostname from its own configuration.
        (root / "etc/hostname").write_text(name + "\n")
        markers = root / "var/lib/kurumi-lifecycle"
        markers.mkdir(parents=True)
        ready = markers / "ready"
        stopped = markers / "stopped"
        if init_system == "systemd":
            service = root / "etc/systemd/system/kurumi-lifecycle.service"
            service.write_text("""[Unit]
Description=Kurumi lifecycle shutdown witness
[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/touch /var/lib/kurumi-lifecycle/ready
ExecStop=/bin/touch /var/lib/kurumi-lifecycle/stopped
[Install]
WantedBy=multi-user.target
""")
            wants = service.parent / "multi-user.target.wants"
            wants.mkdir(exist_ok=True)
            (wants / service.name).symlink_to("../" + service.name)
        else:
            service = root / "etc/init.d/kurumi-lifecycle"
            service.write_text("""#!/sbin/openrc-run
depend() { after localmount; }
start() { touch /var/lib/kurumi-lifecycle/ready; }
stop() { touch /var/lib/kurumi-lifecycle/stopped; }
""")
            service.chmod(0o755)
            runlevel = root / "etc/runlevels/default"
            runlevel.mkdir(parents=True, exist_ok=True)
            (runlevel / service.name).symlink_to("/etc/init.d/" + service.name)

        def running():
            wait_for("init starts its lifecycle service", ready.exists)
            pid = int(cli("pid").stdout.strip())
            assert pid > 1 and not exited(pid), f"init {pid} is not alive"
            info = cli("info").stdout
            assert "Active: active (running)" in info, info
            monitor = int(next(line.split(": ", 1)[1] for line in info.splitlines()
                               if line.startswith("Monitor PID: ")))
            assert not exited(monitor), f"monitor {monitor} is not alive"
            assert name in cli("show").stdout
            cli("run", "/bin/sh", "-ec",
                'test "$(hostname)" = "$1"; '
                'test "$(awk \'/^Pid:/ {print $2}\' /proc/1/status)" = 1; '
                'test -f /var/lib/kurumi-lifecycle/ready; '
                'printf lifecycle-exec > /var/lib/kurumi-lifecycle/exec', "sh", name)
            assert (markers / "exec").read_text() == "lifecycle-exec"
            assert cli("run", "/bin/sh", "-c", "exit 23", check=False).returncode == 23
            if init_system == "systemd":
                wait_for("systemd finishes booting", lambda: cli(
                    "run", "/bin/systemctl", "is-system-running", check=False
                ).stdout.strip() in {"running", "degraded"})
            else:
                cli("run", "/bin/rc-status", "default")
            return pid, monitor

        def cleaned_up(processes):
            assert stopped.exists(), "init did not execute the service's graceful shutdown hook"
            wait_for("init and monitor exit", lambda: all(exited(pid) for pid in processes))
            assert "Active: inactive (dead)" in cli("info").stdout
            assert cli("pid", check=False).returncode != 0, "stopped container still has a PID"
            assert name not in cli("show").stdout, "stopped container remains listed"
            assert not (Path("/sys/fs/cgroup/kurumi-containerd") / name).exists()
            mounts = Path("/proc/self/mountinfo").read_text().splitlines()
            assert not any(line.split()[4].startswith(str(root) + "/") for line in mounts), \
                "container mounts leaked into the host mount namespace"

        try:
            cli("start")
            processes = running()
            ready.unlink()
            cli("restart")
            assert stopped.exists(), "restart bypassed graceful shutdown"
            wait_for("previous init and monitor exit", lambda: all(exited(pid) for pid in processes))
            processes = running()
            stopped.unlink()
            cli("stop")
            cleaned_up(processes)
            ready.unlink()
            stopped.unlink()
            cli("start")
            processes = running()
            cli("stop")
            cleaned_up(processes)
            print(f"PASS: {init_system}: install/start/run/restart/stop/start/stop", flush=True)
        except BaseException:
            cli("info", check=False)
            cli("show", check=False)
            if init_system == "systemd":
                cli("run", "/bin/journalctl", "--no-pager", "-b", "-n", "100", check=False)
            else:
                cli("run", "/bin/rc-status", "--all", check=False)
            raise
        finally:
            if cli("pid", check=False).returncode == 0:
                cli("stop")


if __name__ == "__main__":
    main()
