# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only

"""python3 tests/exec_interposition.py; requires Linux, Cargo and a host C compiler."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    root = Path(__file__).resolve().parent.parent
    build = subprocess.run(
        ["cargo", "test", "--locked", "-p", "kurumi-containerd-helper", "--lib",
         "--no-run", "--message-format=json"],
        cwd=root, capture_output=True, text=True, check=True,
    )
    artifacts = [json.loads(line) for line in build.stdout.splitlines()]
    binary = next(item["executable"] for item in artifacts
                  if item.get("reason") == "compiler-artifact"
                  and item.get("profile", {}).get("test") and item.get("executable"))
    with tempfile.TemporaryDirectory(prefix="kurumi-exec-interposition-") as temporary:
        directory = Path(temporary)
        source = directory / "interposer.c"
        library = directory / "interposer.so"
        source.write_text("""#include <unistd.h>
int execve(const char *path, char *const argv[], char *const envp[]) {
    (void)path; (void)argv; (void)envp;
    _exit(99);
}
""")
        subprocess.run(["cc", "-shared", "-fPIC", str(source), "-o", str(library)], check=True)
        environment = dict(os.environ, LD_PRELOAD=str(library))
        probe = subprocess.run(
            [sys.executable, "-c",
             "import ctypes; ctypes.CDLL(None).execve(b'/bin/sh', None, None)"],
            env=environment,
        )
        assert probe.returncode == 99, "execve interposer was not loaded"
        subprocess.run(
            [binary, "--exact", "process::tests::closes_non_stdio_descriptors_on_exec"],
            env=dict(environment, KURUMI_FD_EXEC_TEST="1"), check=True,
        )
    print("PASS: helper executes the shell without invoking the host execve interposer")


if __name__ == "__main__":
    main()
