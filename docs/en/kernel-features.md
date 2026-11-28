# Host kernel requirements

[简体中文](../zh/kernel-features.md) | [English](../en/kernel-features.md)

Linux and Android containers use the running host kernel. When configuring a kernel, start with the common requirements and add the facilities used by your container's JSON configuration. Kconfig symbols vary by kernel version. For drivers that support `=m`, install the matching modules and make them available to load on the device.

## Required for every container

| Purpose | Kernel options or interfaces | Runtime use |
| --- | --- | --- |
| Mount and hostname isolation | `CONFIG_NAMESPACES`, `CONFIG_UTS_NS` | Create mount and UTS namespaces, bind mounts, change mount propagation and call `pivot_root`. |
| Kernel filesystems | `CONFIG_PROC_FS`, `CONFIG_PROC_SYSCTL`, `CONFIG_SYSFS`, `CONFIG_TMPFS` | Read process identity and boot ID from `/proc`, mount `/proc` and `/sys`, and provide `/run`, `/tmp`, `/dev` and `/dev/shm` on tmpfs. |
| Console | `CONFIG_TTY`, `CONFIG_UNIX98_PTYS` | Both foreground and background starts allocate a host PTY and mount a private devpts instance. Enable `CONFIG_DEVPTS_MULTIPLE_INSTANCES` on older kernels that offer it. |
| Syscall filtering | `CONFIG_SECCOMP`, `CONFIG_SECCOMP_FILTER` | Init and commands executed in the container install a classic BPF filter. |
| Daemon and runtime state | `CONFIG_NET`, `CONFIG_UNIX`, `CONFIG_SHMEM`, `CONFIG_FILE_LOCKING` | The daemon uses Unix domain sockets. Its state files use `memfd_create` and `flock`. Check `CONFIG_MEMFD_CREATE` on kernels that expose it. |
| Process management | `pidfd_open`, `pidfd_send_signal`, `CONFIG_EPOLL` | Identify and signal processes and wait for exit via pidfds. Upstream Linux provides both syscalls from 5.3. Vendor kernels may backport them. |
| Host network handle | `/proc/self/ns/net` (upstream `CONFIG_NET_NS`) | The monitor opens this handle in every network mode. Only non-`host` modes call `unshare` to create a new network namespace. |
| Native executables | `CONFIG_BINFMT_ELF` | Run Kurumi and native ELF programs. An init or command implemented as a `#!` script also needs `CONFIG_BINFMT_SCRIPT` and its interpreter. |

## Configuration-specific requirements

| Configuration or workload | Additional requirements |
| --- | --- |
| Default `isolated` mode | `CONFIG_PID_NS` and `CONFIG_IPC_NS`. Satisfy the target kernel's IPC dependencies, such as `CONFIG_SYSVIPC` or `CONFIG_POSIX_MQUEUE`. |
| `shared-pid-ipc` | Build Kurumi with `servicectl`. The kernel needs `CONFIG_CGROUPS`, cgroup namespaces (`CLONE_NEWCGROUP`) and a mounted, writable, dedicated cgroup v2 subtree. A foreground supervisor manages services with the host-visible PID and IPC namespaces. `cgroup.kill` is optional. |
| systemd or `resources` | `CONFIG_CGROUPS` and a writable hierarchy. `memory_bytes` uses `CONFIG_MEMCG`, `pids` uses `CONFIG_CGROUP_PIDS`, and `cpu_quota` uses `CONFIG_CGROUP_SCHED`, `CONFIG_FAIR_GROUP_SCHED` and `CONFIG_CFS_BANDWIDTH`. On v2 the requested controllers must be available to enable at the root and the `kurumi-containerd` parent. On v1, systemd uses `pids` when no limits are configured. |
| `network: "none"` | `CONFIG_NET_NS` for a separate network namespace. |
| `nat`, `gateway`, `dhcp` | `CONFIG_NET_NS`, `CONFIG_INET`, `CONFIG_VETH`, `CONFIG_BRIDGE`, and `ip` on the host and in the rootfs. `gateway` and `dhcp` attach to an existing bridge. `dhcp` also needs a client in the rootfs and packet sockets (`CONFIG_PACKET`) for clients that use them. |
| `nat` and port forwarding | Writable `net.ipv4.ip_forward`, host `iptables` and its kernel backend. Rules use IPv4 filtering, conntrack, comments and MASQUERADE. Port mappings also use DNAT. See the backend options below. |
| `rootfs_image` | `CONFIG_EXT4_FS` for ext4 or `CONFIG_BTRFS_FS` for btrfs. Regular image files also need `CONFIG_BLK_DEV_LOOP`, `/dev/loop-control` and an available loop device. Block devices are mounted directly. Directory rootfs uses the filesystem containing the directory. |
| `volatile: true` | `CONFIG_OVERLAY_FS` and extended attributes on the tmpfs upper layer (`CONFIG_TMPFS_XATTR`). |
| Foreign execution with `binfmt` | `CONFIG_BINFMT_MISC`, an enabled registry, `F` flag support and a host interpreter. Upstream Linux supports `F` from 4.8. |
| `security.allow_user_namespaces` | Nested workloads that create user namespaces need `CONFIG_USER_NS`, permission under host policy and an available namespace quota. |
| Android Binder / GPU | Binder uses `CONFIG_ANDROID_BINDER_IPC` and the host Binder device nodes. GPU access needs the device-specific driver and nodes. Android security policy must permit access. |

Common `nat` kernel options include `CONFIG_NETFILTER`, `CONFIG_NF_CONNTRACK` and `CONFIG_NF_NAT`. Run `iptables --version` to identify the host backend.

- `legacy` uses `CONFIG_IP_NF_IPTABLES`, `CONFIG_IP_NF_FILTER`, `CONFIG_IP_NF_NAT`, `CONFIG_NETFILTER_XT_MATCH_CONNTRACK`, `CONFIG_NETFILTER_XT_MATCH_COMMENT` and `CONFIG_NETFILTER_XT_TARGET_MASQUERADE`. Port mapping uses the DNAT target from `CONFIG_NETFILTER_XT_NAT`.
- `nf_tables` uses `CONFIG_NF_TABLES` and IPv4 conntrack, NAT and masquerade expressions (`CONFIG_NFT_CT`, `CONFIG_NFT_NAT`, `CONFIG_NFT_MASQ`). If the installed iptables implements an extension through xtables compatibility, also enable `CONFIG_NFT_COMPAT` and that extension.

## Inspect the running host

Run `kurumi-containerd check` with the privileges used to start containers (`sudo` on Linux or a root shell on Android). It probes namespaces and seccomp in disposable child processes. Filesystem results reflect the types currently registered in `/proc/filesystems`. Controller results reflect visible cgroup mounts. Applying resource limits also needs write access.

To distinguish build settings from access restrictions, read `/proc/config.gz` on kernels that export their configuration (`CONFIG_IKCONFIG_PROC`). Linux distributions may also install `/boot/config-$(uname -r)`. For example, inspect the settings for default isolation and seccomp with the following command.

```sh
options='^(# )?CONFIG_(NAMESPACES|UTS_NS|PID_NS|IPC_NS|SECCOMP|SECCOMP_FILTER)(=| )'
if test -r /proc/config.gz; then
    zcat /proc/config.gz | grep -E "$options"
else
    grep -E "$options" "/boot/config-$(uname -r)"
fi
```

`=y` indicates a built-in feature, `=m` a module, and `# CONFIG_… is not set` a disabled option. An `unavailable` result from `check` can also reflect permissions, SELinux, an unloaded module or an unmounted hierarchy. `start` validates the selected configuration. See the [configuration reference](configuration.md).
