"""Privileged P1 regressions: sudo python3 tests/p1_runtime.py <runtime-binary>."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


INIT = r"""
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(void) {
    const char *mode = getenv("MODE");
    if (strcmp(mode, "console") == 0) {
        int fd = open("/dev/console", O_RDWR);
        if (fd < 0 || !isatty(fd)) goto wait;
        close(fd);
    }
    if (strcmp(mode, "output") == 0) {
        char buffer[4096];
        memset(buffer, 'x', sizeof(buffer));
        for (int i = 0; i < 256; i++) {
            size_t done = 0;
            while (done < sizeof(buffer)) {
                ssize_t n = write(1, buffer + done, sizeof(buffer) - done);
                if (n <= 0) goto wait;
                done += n;
            }
        }
    }
    close(open("/passed", O_WRONLY | O_CREAT, 0600));
wait:
    for (;;) pause();
}
"""


def run_case(binary, directory, mode):
    root = directory / mode
    root.mkdir()
    (root / "init").write_bytes((directory / "init").read_bytes())
    (root / "init").chmod(0o755)
    name = f"p1-review-{os.getpid()}-{mode}"
    config = directory / f"{mode}.toml"
    source = (
        f'[runtime]\n[container]\nname="{name}"\nrootfs="{root}"\n'
        f'init="/init"\n[container.environment]\nMODE="{mode}"\n'
    )
    if mode == "cgroup":
        source += (
            "[container.resources]\nmemory_bytes=67108864\npids=32\n"
            "cpu_quota=10000\ncpu_period=100000\n"
        )
    config.write_text(source)
    home = directory / f"{mode}-home"
    pointer_dir = home / ".kurumi-containerd"
    pointer_dir.mkdir(parents=True)
    (pointer_dir / "config.json").write_text(json.dumps([{"name": name, "file": str(config)}]))
    environment = dict(os.environ, HOME=str(home))
    command = [binary]
    try:
        result = subprocess.run(command + ["start"], env=environment, capture_output=True, timeout=20)
        assert result.returncode == 0, result.stdout.decode() + result.stderr.decode()
        deadline = time.monotonic() + 5
        while not (root / "passed").exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        assert (root / "passed").exists(), f"{mode}: init did not finish its check"
        if mode == "cgroup":
            cgroup = Path("/sys/fs/cgroup/kurumi-containerd") / name
            for file, expected in [("memory.max", "67108864"), ("pids.max", "32"),
                                   ("cpu.max", "10000 100000")]:
                assert (cgroup / file).read_text().strip() == expected
    finally:
        subprocess.run(command + ["stop"], env=environment, capture_output=True, timeout=20)


def main():
    assert os.geteuid() == 0, "requires root and a writable cgroup v2 hierarchy"
    binary = str(Path(sys.argv[1]).resolve())
    failures = []
    with tempfile.TemporaryDirectory(prefix="kurumi-p1-") as temporary:
        directory = Path(temporary)
        source = directory / "init.c"
        source.write_text(INIT)
        subprocess.run(["cc", "-static", str(source), "-o", str(directory / "init")], check=True)
        for mode in ["cgroup", "console", "output"]:
            if mode == "cgroup":
                controllers = Path("/sys/fs/cgroup/cgroup.controllers").read_text().split()
                if not {"cpu", "memory", "pids"}.issubset(controllers):
                    print("SKIP: cgroup: host does not expose cpu, memory and pids controllers")
                    continue
            try:
                run_case(binary, directory, mode)
                print(f"PASS: {mode}")
            except (AssertionError, subprocess.TimeoutExpired) as error:
                failures.append(mode)
                print(f"FAIL: {mode}: {error}")
    assert not failures, f"failed regressions: {failures}"


if __name__ == "__main__":
    main()
