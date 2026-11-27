# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only

"""sudo python3 tests/binfmt.py BINARY STATIC_QEMU GUEST_ARCH GUEST_CC.

Uses a foreign static init to check boot, exec, enter, reboot, concurrent rule ownership,
normal shutdown and startup-failure cleanup. Requires binfmt_misc, mount/PID
namespaces, and a compiler that can statically link for GUEST_ARCH.
"""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import pty
import shutil
import subprocess
import sys
import tempfile
import time


INIT = r"""
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/reboot.h>
#include <unistd.h>

static volatile sig_atomic_t stopped;
static void stop(int sig) { (void)sig; stopped = 1; }

int main(int argc, char **argv) {
    if (argc == 4 && strcmp(argv[3], "-l") == 0) argc--;
    if (argc > 1) {
        if (strcmp(argv[1], "reboot") == 0) return reboot(RB_AUTOBOOT) < 0 ? 21 : 22;
        if (strcmp(argv[1], "exec-check") != 0 || argc != 3 ||
            strcmp(argv[2], "hello world") != 0) return 19;
        puts("foreign-exec-ok");
        return 17;
    }
    if (getpid() != 1) return 20;
    signal(SIGTERM, stop);
    close(open("/ready", O_WRONLY | O_CREAT, 0600));
    while (!stopped) pause();
    return 0;
}
"""


def wait_for(description, predicate):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.05)
    raise AssertionError(f"timed out: {description}")


@contextmanager
def mounted_registry():
    directory = Path(tempfile.mkdtemp(prefix="kurumi-binfmt-"))
    registry = directory / "registry"
    registry.mkdir()
    mounted = False
    try:
        subprocess.run(["mount", "-t", "binfmt_misc", "-o", "nosuid,nodev,noexec",
                        "binfmt_misc", str(registry)], check=True)
        mounted = True
        yield directory, registry
    finally:
        if mounted:
            # If detach fails, preserve the directory rather than removing rules.
            subprocess.run(["umount", "-l", str(registry)], check=True)
        shutil.rmtree(directory)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("interpreter", type=Path)
    parser.add_argument("arch")
    parser.add_argument("compiler")
    parser.add_argument("--cflag", action="append", default=[], help="extra compiler argument (repeatable)")
    parser.add_argument("--private-mount-namespace", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    assert os.geteuid() == 0, "requires root and namespace/mount privileges"
    if not args.private_mount_namespace:
        os.execvp("unshare", ["unshare", "--mount", "--propagation", "private",
                             sys.executable, __file__, "--private-mount-namespace", *sys.argv[1:]])
    binary = str(args.binary.resolve(strict=True))
    interpreter = str(args.interpreter.resolve(strict=True))
    subprocess.run([binary, "check"], check=True)
    with mounted_registry() as (directory, registry):
        # A unique interpreter path identifies this test's rules during cleanup.
        private_interpreter = directory / "qemu-static"
        shutil.copy2(interpreter, private_interpreter)
        interpreter = str(private_interpreter)
        assert (registry / "status").read_text().strip() == "enabled", "host binfmt_misc is disabled"
        initial = {entry.name: entry.read_bytes() for entry in registry.iterdir()
                   if entry.name != "register"}
        source = directory / "init.c"
        source.write_text(INIT)
        guest = directory / "guest-init"
        subprocess.run([args.compiler, "-static", *args.cflag, str(source), "-o", str(guest)], check=True)
        running = []

        def entries():
            return {entry.name for entry in registry.iterdir()} - {"status", "register"}

        def owned_entries():
            return {name for name in entries() - baseline
                    if f"interpreter {interpreter}\n" in (registry / name).read_text()}

        def cli(environment, *arguments, check=True, stdin=None):
            result = subprocess.run([binary, *arguments], env=environment, text=True,
                                    capture_output=True, timeout=20, stdin=stdin)
            if check:
                assert result.returncode == 0, result.stdout + result.stderr
            return result

        def configure(index, mounts=()):
            root = directory / f"root-{index}"
            root.mkdir()
            (root / "init").write_bytes(guest.read_bytes())
            (root / "init").chmod(0o755)
            (root / "etc").mkdir()
            (root / "etc/passwd").write_text("root:x:0:0:root:/:/init\n")
            (root / "etc/group").write_text("root:x:0:\n")
            home = directory / f"home-{index}"
            config_dir = home / ".kurumi-containerd"
            config_dir.mkdir(parents=True)
            (config_dir / "config.json").write_text(json.dumps([{
                "runtime": {"stop_timeout_seconds": 3},
                "container": {"name": f"binfmt-{os.getpid()}-{index}", "rootfs": str(root),
                              "init": "/init", "mounts": list(mounts),
                              "binfmt": {"arch": args.arch, "interpreter": interpreter}},
            }]))
            return root, dict(os.environ, HOME=str(home), NO_COLOR="1")

        baseline = entries()
        try:
            for index in range(2):
                root, environment = configure(index)
                running.append(environment)
                cli(environment, "start")
                wait_for("foreign init", lambda: (root / "ready").exists())
                assert len(entries() - baseline) == index + 1
                assert not any(root.rglob("*qemu*")), "interpreter was copied into rootfs"
                result = cli(environment, "run", "/init", "exec-check", "hello world", check=False)
                assert result.returncode == 17, result.stdout + result.stderr
                assert "foreign-exec-ok" in result.stdout
                master, slave = pty.openpty()
                try:
                    result = cli(environment, "enter", "--shell", "/init exec-check 'hello world'",
                                 check=False, stdin=slave)
                finally:
                    os.close(slave)
                    os.close(master)
                assert result.returncode == 17, f"enter exit {result.returncode}: {result.stdout}{result.stderr}"
                assert "foreign-exec-ok" in result.stdout
                if index == 0:
                    rules = entries()
                    (root / "ready").unlink()
                    cli(environment, "run", "/init", "reboot", check=False)
                    wait_for("foreign init after reboot", lambda: (root / "ready").exists())
                    assert entries() == rules, "reboot changed the monitor's binfmt rule"
            cli(running[0], "stop")
            running.pop(0)
            wait_for("first rule cleanup", lambda: len(entries() - baseline) == 1)
            assert cli(running[0], "run", "/init", "exec-check", "hello world", check=False).returncode == 17
            cli(running[0], "stop")
            running.pop()
            wait_for("all rule cleanup", lambda: entries() == baseline)

            root, environment = configure("failure", [{"source": str(source), "target": "/bad"}])
            (root / "bad").symlink_to("/outside")
            result = cli(environment, "start", check=False)
            assert result.returncode != 0
            assert "bind target traverses symlink" in result.stdout + result.stderr
            wait_for("failed boot rule cleanup", lambda: entries() == baseline)
            for name, content in initial.items():
                assert (registry / name).read_bytes() == content, f"modified existing rule/status: {name}"
            print("PASS: foreign init, exec, enter, reboot, concurrent rules, shutdown and failed-boot cleanup")
        finally:
            for environment in running:
                cli(environment, "stop", check=False)
            # Never leave this test's global rules behind after an assertion.
            for name in owned_entries():
                (registry / name).write_text("-1")


if __name__ == "__main__":
    main()
