# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only

"""sudo python3 tests/daemon.py BINARY. Uses a private mount namespace and socket directory."""

import concurrent.futures
import json
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time

SOCKET = Path("/var/kurumi/socket.sock")
INIT = r"""
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
static void stop(int sig) { (void)sig; _exit(0); }
int main(int argc, char **argv) {
    if (argc > 1 && !strcmp(argv[1], "exit")) {
        puts("command stdout"); fputs("command stderr\n", stderr); return 23;
    }
    signal(SIGTERM, stop);
    char output[4096]; memset(output, 'x', sizeof(output));
    for (int i = 0; i < 256; ++i) {
        size_t written = 0;
        while (written < sizeof(output)) {
            ssize_t n = write(1, output + written, sizeof(output) - written);
            if (n <= 0) return 1;
            written += n;
        }
    }
    close(open("/ready", O_WRONLY | O_CREAT, 0600));
    for (;;) pause();
}
"""


def wait_for(description, predicate):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.025)
    raise AssertionError("timed out waiting for " + description)


def daemon_pid():
    with socket.socket(socket.AF_UNIX) as stream:
        stream.connect(str(SOCKET))
        return struct.unpack("3i", stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))[0]


def live(pid):
    try:
        return Path(f"/proc/{pid}/stat").read_text().split(") ", 1)[1].split()[0] != "Z"
    except FileNotFoundError:
        return False


def main(binary):
    if "--inside" not in sys.argv:
        assert os.geteuid() == 0, "requires root"
        subprocess.run(["unshare", "--mount", "--propagation", "private", sys.executable,
                        __file__, binary, "--inside"], check=True)
        return
    SOCKET.parent.mkdir(mode=0o700, exist_ok=True)
    subprocess.run(["mount", "-t", "tmpfs", "-o", "mode=700", "tmpfs", str(SOCKET.parent)], check=True)
    current_daemon = None
    with tempfile.TemporaryDirectory(prefix="kurumi-daemon-") as temporary:
        directory = Path(temporary)
        rootfs = directory / "rootfs"
        rootfs.mkdir()
        source = directory / "init.c"
        source.write_text(INIT)
        subprocess.run(["cc", "-static", str(source), "-o", str(rootfs / "init")], check=True)
        registry = directory / ".kurumi-containerd"
        registry.mkdir()
        name = f"daemon-test-{os.getpid()}"
        (registry / "config.json").write_text(json.dumps([{
            "runtime": {"stop_timeout_seconds": 1},
            "container": {"name": name, "rootfs": str(rootfs), "init": "/init", "network": "host"}
        }]))
        environment = dict(os.environ, HOME=str(directory), NO_COLOR="1")

        def cli(*args, check=True):
            result = subprocess.run([binary, *args], env=environment, capture_output=True,
                                    text=True, timeout=40)
            if check:
                assert result.returncode == 0, result.stdout + result.stderr
            return result

        def detached():
            pid = daemon_pid()
            fields = Path(f"/proc/{pid}/stat").read_text().split(") ", 1)[1].split()
            assert fields[1] == "1", (pid, fields)
            assert fields[4] == "0" and int(fields[3]) != pid, (pid, fields)
            assert all(os.readlink(f"/proc/{pid}/fd/{fd}") == "/dev/null" for fd in range(3))
            return pid

        try:
            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                results = list(pool.map(lambda _: cli("--verbose", "show"), range(8)))
            assert any("starting daemon" in result.stdout + result.stderr for result in results)
            current_daemon = detached()
            assert daemon_pid() == current_daemon
            log = SOCKET.with_name("daemon.log")
            assert log.is_file() and log.stat().st_mode & 0o077 == 0
            assert cli("pid", check=False).returncode != 0
            cli("start")
            wait_for("background output drained", lambda: (rootfs / "ready").exists())
            init_pid = int(cli("pid").stdout)
            assert name in cli("show").stdout
            cli("scan")
            command = cli("run", "/init", "exit", check=False)
            assert command.returncode == 23 and "command stdout" in command.stdout
            assert "command stderr" in command.stderr
            assert not Path(f"/run/kurumi-containerd/state/{name}.json").exists()
            assert "operation=\"start\"" in log.read_text()
            assert "request completed" in log.read_text() and "exit_code=23" in log.read_text()
            cli("stop")
            assert not live(init_pid)
            (rootfs / "ready").unlink()
            with (directory / "foreground-output").open("w") as output:
                foreground = subprocess.Popen([binary, "start", "--foreground"], env=environment,
                                              stdin=subprocess.DEVNULL, stdout=output, stderr=output,
                                              start_new_session=True)
                try:
                    wait_for("foreground init readiness", lambda: (rootfs / "ready").exists())
                    os.kill(foreground.pid, signal.SIGINT)
                    foreground.wait(timeout=10)
                    assert cli("pid", check=False).returncode != 0
                finally:
                    cli("stop", check=False)
                    if foreground.poll() is None:
                        foreground.kill()
                    foreground.wait(timeout=5)
            (rootfs / "ready").unlink()
            cli("start")
            wait_for("second generation ready", lambda: (rootfs / "ready").exists())
            init_pid = int(cli("pid").stdout)
            os.kill(current_daemon, signal.SIGKILL)
            wait_for("daemon loss container cleanup", lambda: not live(init_pid))
            current_daemon = None
            cli("show")
            current_daemon = detached()
            assert cli("pid", check=False).returncode != 0
            (rootfs / "ready").unlink()
            cli("start")
            wait_for("container ready before reload", lambda: (rootfs / "ready").exists())
            init_pid = int(cli("pid").stdout)
            old_daemon = current_daemon
            cli("daemon-reload")
            current_daemon = detached()
            assert current_daemon != old_daemon and not live(old_daemon)
            assert not live(init_pid) and name not in cli("show").stdout
            os.kill(current_daemon, signal.SIGTERM)
            wait_for("daemon socket removal", lambda: not SOCKET.exists())
            current_daemon = None
            (registry / "config.json").unlink()
            cli("daemon-reload")
            current_daemon = detached()
            print("PASS: concurrent autostart, PPID 1, detached stdio, UDS, request log, run exit status, memory state, daemon-loss cleanup and daemon-reload")
        finally:
            if current_daemon and live(current_daemon):
                cli("stop", check=False)
                os.kill(current_daemon, signal.SIGTERM)
                wait_for("test daemon exit", lambda: not live(current_daemon))


if __name__ == "__main__":
    main(str(Path(sys.argv[1]).resolve(strict=True)))
