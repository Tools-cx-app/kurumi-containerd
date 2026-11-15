# Copyright (C) 2026 Tools-cx-app
# SPDX-License-Identifier: GPL-v3-only
"""sudo python3 tests/shared_isolation.py BINARY [--vm-reboot-filter-probe].

Builds a disposable static rootfs. Never issues a valid reboot request. The
optional invalid-magic syscall probe additionally requires KURUMI_DISPOSABLE_VM=1.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time


INIT = r"""
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/reboot.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

static volatile sig_atomic_t stopping;
static void stop(int sig) { (void)sig; stopping = 1; }
static void marker(const char *path) {
    FILE *f = fopen(path, "w");
    if (!f) _exit(90);
    fprintf(f, "%ld\n", (long)getpid());
    if (fclose(f)) _exit(91);
}
static int orphan(const char *path, int storm) {
    pid_t first = fork();
    if (first < 0) return 92;
    if (!first) {
        if (setsid() < 0) _exit(93);
        pid_t second = fork();
        if (second < 0) _exit(94);
        if (second) _exit(0);
        signal(SIGTERM, SIG_IGN);
        signal(SIGHUP, SIG_IGN);
        close(STDIN_FILENO);
        close(STDOUT_FILENO);
        close(STDERR_FILENO);
        marker(path);
        if (storm) {
            for (int i = 0; i < 16; ++i) {
                pid_t child = fork();
                if (!child) { for (;;) pause(); }
                if (child < 0) _exit(95);
                usleep(20000);
            }
        }
        for (;;) pause();
    }
    int status;
    if (waitpid(first, &status, 0) != first) return 96;
    return WIFEXITED(status) ? WEXITSTATUS(status) : 97;
}
int main(int argc, char **argv) {
    if (argc > 1) {
        if (!strcmp(argv[1], "sentinel")) { for (;;) pause(); }
        if (!strcmp(argv[1], "-l")) return 0;
        if (!strcmp(argv[1], "exit")) return atoi(argv[2]);
        if (!strcmp(argv[1], "orphan")) return orphan(argv[2], 0);
        if (!strcmp(argv[1], "storm")) return orphan(argv[2], 1);
        if (!strcmp(argv[1], "term")) { raise(SIGTERM); return 98; }
        if (!strcmp(argv[1], "reboot-probe")) {
            /* Invalid magic is harmless even if filtering is broken. */
            errno = 0;
            long result = syscall(SYS_reboot, 0, 0, 0, NULL);
            return result == -1 && errno == EPERM ? 0 : 99;
        }
        if (!strcmp(argv[1], "exec-check")) { marker("/exec-ready"); return 0; }
        if (!strcmp(argv[1], "login")) return 0;
        return 89;
    }
    signal(SIGTERM, stop);
    signal(SIGINT, stop);
    if (orphan("/orphan", 0)) return 88;
    if (getenv("REBOOT_PROBE")) {
        errno = 0;
        long result = syscall(SYS_reboot, 0, 0, 0, NULL);
        if (result != -1 || errno != EPERM) return 87;
        marker("/filter-probe");
    }
    marker("/ready");
    if (access("/fail-boot", F_OK) == 0) return 42;
    while (!stopping || access("/stubborn", F_OK) == 0) usleep(100000);
    marker("/stopped");
    return 0;
}
"""


def wait_for(description, predicate, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.05)
    raise AssertionError("timed out: " + description)


def identity(pid):
    try:
        return Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[19]
    except FileNotFoundError:
        return None


def gone(processes):
    # A zombie still has /proc identity, and must actually be reaped.
    return all(identity(pid) != start for pid, start in processes.items())


def members(group):
    result = set()
    if group.exists():
        for path in group.rglob("cgroup.procs"):
            try:
                result.update(map(int, path.read_text().split()))
            except FileNotFoundError:
                pass
    return result


class Container:
    def __init__(self, binary, directory, suffix):
        self.binary = binary
        self.name = f"shared-{os.getpid()}-{suffix}"
        self.root = directory / suffix
        self.root.mkdir()
        shutil.copy2(directory / "init", self.root / "supervisor")
        shutil.copy2(directory / "init", self.root / "init")
        (self.root / "etc").mkdir()
        (self.root / "etc/passwd").write_text("root:x:0:0:root:/:/init\n")
        (self.root / "etc/group").write_text("root:x:0:\n")
        self.home = directory / (suffix + "-home")
        registry = self.home / ".kurumi-containerd"
        registry.mkdir(parents=True)
        self.config = registry / "config.json"
        self.entry = {"runtime": {"stop_timeout_seconds": 3}, "container": {
            "name": self.name, "rootfs": str(self.root), "init": "/supervisor",
            "isolation": "shared-pid-ipc", "network": "host"}}
        self.save()
        self.env = dict(os.environ, HOME=str(self.home), NO_COLOR="1")
        self.state = Path("/run/kurumi-containerd/state") / (self.name + ".json")
        self.group = None

    def save(self):
        self.config.write_text(json.dumps([self.entry]))

    def cli(self, *args, check=True, timeout=40):
        result = subprocess.run([self.binary, "--name", self.name, *args], env=self.env,
                                text=True, capture_output=True, timeout=timeout)
        if check and result.returncode:
            raise AssertionError(f"{self.name}: {args}: {result.returncode}\n"
                                 f"{result.stdout}{result.stderr}")
        return result

    def running(self):
        wait_for("supervisor readiness", (self.root / "ready").exists)
        state = json.loads(self.state.read_text())
        assert state["isolation"] == "shared-pid-ipc", state
        pid = int(self.cli("pid").stdout.strip())
        assert pid > 1 and identity(pid), pid
        for namespace in ("pid", "ipc"):
            host = Path(f"/proc/self/ns/{namespace}")
            child = Path(f"/proc/{pid}/ns/{namespace}")
            if host.exists():
                assert child.exists() and os.readlink(host) == os.readlink(child), namespace
            else:
                assert not child.exists(), namespace
                print(f"PASS: {namespace} namespace interface absent; start/run do not require it")
        assert os.readlink(f"/proc/{pid}/ns/mnt") != os.readlink("/proc/self/ns/mnt")
        assert os.readlink(f"/proc/{pid}/ns/uts") != os.readlink("/proc/self/ns/uts")
        membership = state["cgroup_path"]
        assert membership.endswith("/kurumi-containerd/" + self.name), membership
        self.group = Path("/sys/fs/cgroup") / membership.lstrip("/")
        assert self.group.is_dir(), self.group
        assert pid in members(self.group)
        self.cli("run", "/init", "exec-check")
        assert (self.root / "exec-ready").exists()
        assert "shared-pid-ipc" in self.cli("info").stdout
        return state

    def snapshot(self):
        pids = members(self.group) if self.group else set()
        if self.state.exists():
            state = json.loads(self.state.read_text())
            for key in ("pid", "init_pid", "monitor_pid", "worker_pid"):
                if state.get(key):
                    pids.add(state[key])
        return {pid: identity(pid) for pid in pids if identity(pid) is not None}

    def cleaned(self, processes):
        wait_for("all descendants and supervisors reaped", lambda: gone(processes))
        if self.group:
            wait_for("container cgroup removed", lambda: not self.group.exists())
        assert not self.state.exists(), f"cleaned generation retained state: {self.state}"
        assert self.cli("pid", check=False).returncode != 0
        mounts = Path("/proc/self/mountinfo").read_text().splitlines()
        assert not any(line.split()[4].startswith(str(self.root) + "/") for line in mounts)

    def close(self):
        result = self.cli("stop", check=False)
        if self.group and self.group.exists():
            raise AssertionError(f"cleanup failed: {result.stdout}{result.stderr}")


def preflight(directory):
    if os.geteuid() != 0:
        print("SKIP: shared lifecycle requires root")
        return False
    if not shutil.which("unshare") or not shutil.which("mount"):
        print("SKIP: mount/UTS capability probe requires util-linux unshare and mount")
        return False
    probe = subprocess.run(["unshare", "--mount", "--uts", "--propagation", "private",
                            "sh", "-ec", 'mount -t tmpfs tmpfs "$1"; umount "$1"',
                            "sh", str(directory)], text=True, capture_output=True)
    if probe.returncode:
        print("SKIP: mount/UTS namespace or mount privileges unavailable: " + probe.stderr.strip())
        return False
    return True


def refusal_without_cgroup(container):
    # Overmount only in a disposable mount namespace, never the host namespace.
    result = subprocess.run([
        "unshare", "--mount", "--propagation", "private", "sh", "-ec",
        'mount -t tmpfs tmpfs /sys/fs/cgroup; echo KURUMI_CGROUP_HIDDEN >&2; exec "$@"', "sh",
        container.binary, "--name", container.name, "start"], env=container.env,
        text=True, capture_output=True, timeout=40)
    output = result.stdout + result.stderr
    if "KURUMI_CGROUP_HIDDEN" not in result.stderr:
        print("SKIP: cannot hide cgroup mount for refusal probe: " + output.strip())
        return
    diagnostic = output.replace("KURUMI_CGROUP_HIDDEN", "")
    assert result.returncode != 0 and "cgroup" in diagnostic.lower(), diagnostic
    assert not (container.root / "ready").exists(), "workload ran without cgroup v2"
    print("PASS: missing cgroup v2 explicitly refused before supervisor execution")


def lifecycle(binary, directory, vm_probe):
    a = Container(binary, directory, "a")
    b = Container(binary, directory, "b")
    sentinel = subprocess.Popen([str(directory / "init"), "sentinel"])
    sentinel_pid = sentinel.pid
    sentinel_start = identity(sentinel_pid)

    def sentinel_alive():
        assert identity(sentinel_pid) == sentinel_start, "host sentinel was terminated"

    try:
        refusal_without_cgroup(a)
        writable = Path("/sys/fs/cgroup") / f"kurumi-probe-{os.getpid()}"
        try:
            writable.mkdir()
            writable.rmdir()
        except OSError as error:
            print(f"SKIP: shared lifecycle needs a writable cgroup v2 subtree: {error}")
            return
        if not Path("/sys/fs/cgroup/cgroup.controllers").exists():
            print("SKIP: shared lifecycle needs cgroup v2; refusal case verified")
            return
        a.cli("start")
        first = a.running()
        has_cgroup_kill = (a.group / "cgroup.kill").exists()
        assert a.cli("run", "/init", "exit", "23", check=False).returncode == 23
        assert a.cli("run", "/init", "exit", "0").returncode == 0
        assert a.cli("run", "/init", "term", check=False).returncode == 128 + signal.SIGTERM
        # Exercise interactive enter through a real PTY without distro PAM/login.
        import pty
        master, slave = pty.openpty()
        try:
            login = subprocess.Popen([binary, "--name", a.name, "enter"], env=a.env,
                                     stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
            assert login.wait(timeout=15) == 0
        finally:
            os.close(master)
            os.close(slave)
        a.cli("run", "/init", "orphan", "/exec-orphan")
        wait_for("exec double fork", (a.root / "exec-orphan").exists)
        orphan_pid = int((a.root / "exec-orphan").read_text())
        nested = a.group / "nested/service"
        nested.mkdir(parents=True)
        (nested / "cgroup.procs").write_text(str(orphan_pid))
        assert orphan_pid in members(a.group)
        b.cli("start")
        second = b.running()
        assert first["cgroup_path"] != second["cgroup_path"]
        assert not members(a.group).intersection(members(b.group))
        info = b.cli("info").stdout
        tasks = int(next(line.split(": ", 1)[1] for line in info.splitlines()
                         if line.startswith("Tasks: ")))
        assert tasks == len(members(b.group)), (tasks, members(b.group))
        # Omitting isolation must still create independent namespaces or fail.
        default = Container(binary, directory, "default")
        default.entry["container"].pop("isolation")
        default.save()
        try:
            result = default.cli("start", check=False)
            if result.returncode == 0:
                state = json.loads(default.state.read_text())
                pid = state["init_pid"]
                assert state.get("isolation", "isolated") == "isolated"
                for ns in ("pid", "ipc"):
                    host = Path(f"/proc/self/ns/{ns}")
                    assert host.exists(), "isolated start succeeded without namespace support"
                    assert os.readlink(f"/proc/{pid}/ns/{ns}") != os.readlink(host)
                default.cli("stop")
            else:
                assert not (default.root / "ready").exists(), result.stdout + result.stderr
                print("SKIP: default isolated execution unavailable; no shared downgrade observed")
        finally:
            default.close()
        # Current config cannot relabel or redirect the running generation.
        a.entry["container"]["isolation"] = "isolated"
        a.save()
        a.cli("run", "/init", "exec-check")
        assert "shared-pid-ipc" in a.cli("info").stdout
        a.state.unlink()
        a.cli("scan")
        recovered = json.loads(a.state.read_text())
        assert recovered["isolation"] == "shared-pid-ipc"
        assert recovered["monitor_pid"] == first["monitor_pid"]
        assert b.cli("pid").stdout.strip() == str(second.get("pid", second.get("init_pid")))
        a.cli("run", "/init", "storm", "/stop-storm")
        wait_for("forking concurrently with stop", (a.root / "stop-storm").exists)
        old = a.snapshot()
        a.cli("stop")
        a.cleaned(old)
        assert (a.root / "stopped").exists(), "graceful supervisor hook omitted"
        b.cli("run", "/init", "exec-check")
        sentinel_alive()
        a.entry["container"]["isolation"] = "shared-pid-ipc"
        a.save()
        (a.root / "ready").unlink()
        a.cli("start")
        a.running()
        old = a.snapshot()
        (a.root / "ready").unlink()
        concurrent = subprocess.Popen([binary, "--name", a.name, "run", "/init", "exit", "0"],
                                      env=a.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        a.cli("restart")
        a.running()
        wait_for("restart reaps old generation", lambda: gone(old))
        concurrent_out, concurrent_err = concurrent.communicate(timeout=10)
        assert concurrent.returncode in (0, 1, 125, 137), (
            f"unexpected concurrent exec status {concurrent.returncode}: "
            f"{concurrent_out}{concurrent_err}"
        )
        (a.root / "stubborn").touch()
        old = a.snapshot()
        a.cli("stop")
        a.cleaned(old)
        (a.root / "stubborn").unlink()
        (a.root / "ready").unlink()
        a.cli("start")
        a.running()
        old = a.snapshot()
        os.kill(json.loads(a.state.read_text())["init_pid"], signal.SIGHUP)
        a.cleaned(old)
        assert a.cli("pid", check=False).returncode != 0, "SIGHUP unexpectedly rebooted"
        (a.root / "ready").unlink()
        a.cli("start")
        a.running()
        # Leave descendants for the worker after loss of its monitor.
        a.cli("run", "/init", "storm", "/storm")
        wait_for("setsid fork storm", (a.root / "storm").exists)
        time.sleep(0.4)
        old = a.snapshot()
        state = json.loads(a.state.read_text())
        os.kill(state["monitor_pid"], signal.SIGKILL)
        a.cleaned(old)
        sentinel_alive()
        old_b = b.snapshot()
        state_b = json.loads(b.state.read_text())
        os.kill(state_b.get("pid", state_b.get("init_pid")), signal.SIGKILL)
        b.cleaned(old_b)
        sentinel_alive()
        # A workload that exits in startup still leaves no daemonized descendants.
        (a.root / "ready").unlink(missing_ok=True)
        (a.root / "fail-boot").touch()
        a.cli("start", check=False)
        wait_for("failed startup descendants cleaned", lambda: not a.group.exists())
        pid_file = a.root / "orphan"
        if pid_file.exists():
            pid = int(pid_file.read_text())
            wait_for("failed startup orphan reaped", lambda: identity(pid) is None)
        (a.root / "fail-boot").unlink()
        if vm_probe:
            a.entry["container"]["environment"] = {"REBOOT_PROBE": "1"}
            a.save()
        a.cli("start")
        a.running()
        if vm_probe:
            assert (a.root / "filter-probe").exists(), "init filter probe did not complete"
            a.cli("run", "/init", "reboot-probe")
            print("PASS: VM invalid-magic reboot syscall denied with EPERM")
        else:
            print("SKIP: reboot filter probe is VM opt-in; no reboot syscall issued")
        old = a.snapshot()
        a.cli("stop")
        a.cleaned(old)
        sentinel_alive()
        if has_cgroup_kill:
            print("SKIP: kernel provides cgroup.kill; old-kernel fallback needs a kernel without it")
        else:
            print("PASS: lifecycle cleanup on kernel without cgroup.kill")
        if all(Path(f"/proc/self/ns/{ns}").exists() for ns in ("pid", "ipc")):
            print("SKIP: CONFIG_PID_NS=n / CONFIG_IPC_NS=n acceptance needs a real matching kernel")
        print("PASS: shared start/run/enter/status/scan/two instances/stop/restart/crash/monitor loss")
    finally:
        try:
            a.close()
        finally:
            try:
                b.close()
            finally:
                if identity(sentinel_pid) == sentinel_start:
                    sentinel.kill()
                sentinel.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=lambda value: str(Path(value).resolve(strict=True)))
    parser.add_argument("--vm-reboot-filter-probe", action="store_true")
    args = parser.parse_args()
    if args.vm_reboot_filter_probe:
        assert os.environ.get("KURUMI_DISPOSABLE_VM") == "1", "requires disposable VM acknowledgement"
    with tempfile.TemporaryDirectory(prefix="kurumi-shared-") as temporary:
        directory = Path(temporary)
        if not preflight(directory):
            return
        # check remains independent of HOME and deliberately invalid config paths.
        result = subprocess.run([args.binary, "check"], env=dict(os.environ, HOME="relative-invalid"),
                                text=True, capture_output=True, timeout=30)
        assert "HOME" not in result.stderr and "config.json" not in result.stderr, result.stderr
        source = directory / "init.c"
        source.write_text(INIT)
        if not shutil.which("cc"):
            print("SKIP: static mini supervisor requires a C compiler")
            return
        subprocess.run(["cc", "-static", "-O2", str(source), "-o", str(directory / "init")], check=True)
        if args.vm_reboot_filter_probe:
            baseline = subprocess.run([str(directory / "init"), "reboot-probe"], check=False)
            if baseline.returncode == 0:
                print("SKIP: VM already denies reboot without Kurumi; filter denial cannot be attributed")
                args.vm_reboot_filter_probe = False
            else:
                assert baseline.returncode == 99, "invalid-magic baseline failed unexpectedly"
        lifecycle(args.binary, directory, args.vm_reboot_filter_probe)


if __name__ == "__main__":
    main()
