# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only

"""sudo python3 tests/shared_pid_ipc_bench.py BINARY [--iterations 20]."""
import argparse
import os
import shutil
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

from shared_isolation import Container, INIT, preflight


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[min(int((len(ordered) - 1) * fraction), len(ordered) - 1)]


def measure(containers, iterations):
    results = {mode: ([], []) for mode in containers}
    for iteration in range(iterations):
        modes = list(containers)
        if iteration % 2:
            modes.reverse()
        for mode in modes:
            container = containers[mode]
            startup, run = results[mode]
            (container.root / "ready").unlink(missing_ok=True)
            started = time.perf_counter()
            container.cli("start")
            deadline = time.monotonic() + 30
            while not (container.root / "ready").exists():
                if time.monotonic() >= deadline:
                    raise TimeoutError(f"{container.name}: supervisor did not become ready")
                time.sleep(0.001)
            startup.append(time.perf_counter() - started)

            started = time.perf_counter()
            container.cli("run", "/supervisor", "-l")
            run.append(time.perf_counter() - started)
            container.cli("stop")
    return results


def report(mode, label, samples):
    median = statistics.median(samples) * 1000
    p95 = percentile(samples, 0.95) * 1000
    print(f"{mode:16} {label:18} median={median:8.2f} ms  p95={p95:8.2f} ms")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=lambda value: str(Path(value).resolve(strict=True)))
    parser.add_argument("--iterations", type=int, default=20)
    args = parser.parse_args()
    if args.iterations < 1:
        parser.error("--iterations must be positive")

    with tempfile.TemporaryDirectory(prefix="kurumi-shared-bench-") as temporary:
        directory = Path(temporary)
        if not preflight(directory):
            return
        if not Path("/sys/fs/cgroup/cgroup.controllers").exists():
            print("SKIP: shared-pid-ipc benchmark requires cgroup v2")
            return
        writable = Path("/sys/fs/cgroup") / f"kurumi-bench-probe-{os.getpid()}"
        try:
            writable.mkdir()
            writable.rmdir()
        except OSError as error:
            print(f"SKIP: shared-pid-ipc benchmark needs a writable cgroup v2 subtree: {error}")
            return
        if not shutil.which("cc"):
            print("SKIP: static mini supervisor benchmark requires a C compiler")
            return
        source = directory / "init.c"
        source.write_text(INIT)
        subprocess.run(["cc", "-static", "-O2", str(source), "-o", str(directory / "init")],
                       check=True)

        containers = {}
        for mode in ("isolated", "shared-pid-ipc"):
            container = Container(args.binary, directory, mode)
            if mode == "isolated":
                container.entry["container"].pop("isolation")
                container.save()
            containers[mode] = container

        try:
            results = measure(containers, args.iterations)
        finally:
            for container in containers.values():
                container.close()

        print(f"Iterations: {args.iterations}; lower is faster; times include CLI overhead")
        print(f"{'Mode':16} {'Operation':18} {'Median / P95'}")
        for mode, (startup, run) in results.items():
            report(mode, "start to ready", startup)
            report(mode, "run round-trip", run)


if __name__ == "__main__":
    main()
