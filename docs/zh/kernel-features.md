# 主机内核配置

[简体中文](../zh/kernel-features.md) | [English](../en/kernel-features.md)

Linux 和 Android 容器使用宿主正在运行的内核。编译内核时，先核对下表，再按容器的 JSON 配置选取附加功能。Kconfig 符号以目标内核版本为准。支持 `=m` 的驱动也可以作为模块提供，设备上需要安装匹配的模块并能在使用时加载。

## 每个容器都需要

| 用途 | 内核选项或接口 | 运行时如何使用 |
| --- | --- | --- |
| 挂载和主机名隔离 | `CONFIG_NAMESPACES`、`CONFIG_UTS_NS` | 创建 mount 和 UTS namespace，执行 bind mount、挂载传播切换和 `pivot_root`。 |
| 内核文件系统 | `CONFIG_PROC_FS`、`CONFIG_PROC_SYSCTL`、`CONFIG_SYSFS`、`CONFIG_TMPFS` | 读取 `/proc` 中的进程身份和 boot ID，挂载 `/proc`、`/sys`，用 tmpfs 提供 `/run`、`/tmp`、`/dev` 和 `/dev/shm`。 |
| 控制台 | `CONFIG_TTY`、`CONFIG_UNIX98_PTYS` | 前台和后台都分配宿主 PTY，并挂载私有 devpts。在仍提供 `CONFIG_DEVPTS_MULTIPLE_INSTANCES` 的旧内核上需启用它。 |
| 系统调用过滤 | `CONFIG_SECCOMP`、`CONFIG_SECCOMP_FILTER` | init 和容器内执行的命令都安装 classic BPF 过滤器。 |
| 守护进程与运行状态 | `CONFIG_NET`、`CONFIG_UNIX`、`CONFIG_SHMEM`、`CONFIG_FILE_LOCKING` | 守护进程使用 Unix 域套接字，状态文件使用 `memfd_create` 和 `flock`。在提供 `CONFIG_MEMFD_CREATE` 的内核上确认该项已启用。 |
| 进程管理 | `pidfd_open`、`pidfd_send_signal`、`CONFIG_EPOLL` | 通过 pidfd 标识进程、发信号并等待退出。上游 Linux 自 5.3 起同时提供这两个系统调用，厂商内核可有回移植。 |
| 宿主网络句柄 | `/proc/self/ns/net`（上游 `CONFIG_NET_NS`） | monitor 在所有网络模式下都打开此句柄。只有非 `host` 模式才调用 `unshare` 创建新的 network namespace。 |
| 本机可执行文件 | `CONFIG_BINFMT_ELF` | 执行 Kurumi 和本机 ELF 程序。若 init 或命令使用带 `#!` 的脚本，还需要 `CONFIG_BINFMT_SCRIPT` 和对应解释器。 |

## 按配置启用

| 配置或场景 | 附加要求 |
| --- | --- |
| 默认 `isolated` | `CONFIG_PID_NS`、`CONFIG_IPC_NS`。满足目标内核的 IPC 依赖，例如 `CONFIG_SYSVIPC` 或 `CONFIG_POSIX_MQUEUE`。 |
| `shared-pid-ipc` | 构建 Kurumi 时启用 `servicectl`。内核需要 `CONFIG_CGROUPS`、cgroup namespace（`CLONE_NEWCGROUP`）和已挂载、可写的专属 cgroup v2 子树。PID/IPC 与宿主共享，由前台 supervisor 管理服务。`cgroup.kill` 可选。 |
| systemd 或 `resources` | `CONFIG_CGROUPS` 和可写层级。`memory_bytes` 对应 `CONFIG_MEMCG`，`pids` 对应 `CONFIG_CGROUP_PIDS`，`cpu_quota` 对应 `CONFIG_CGROUP_SCHED`、`CONFIG_FAIR_GROUP_SCHED`、`CONFIG_CFS_BANDWIDTH`。v2 需能在根和 `kurumi-containerd` 父级启用请求的 controller。在 v1 上，systemd 即使未设限制也会使用 `pids`。 |
| `network: "none"` | `CONFIG_NET_NS`，只创建独立网络空间。 |
| `nat`、`gateway`、`dhcp` | `CONFIG_NET_NS`、`CONFIG_INET`、`CONFIG_VETH`、`CONFIG_BRIDGE`，宿主和 rootfs 内均需 `ip`。`gateway`、`dhcp` 使用已有网桥。`dhcp` 还需 rootfs 内的 DHCP 客户端及其所用的 packet socket 支持（`CONFIG_PACKET`）。 |
| `nat` 和端口转发 | 可写的 `net.ipv4.ip_forward`、宿主 `iptables` 及其对应的内核后端。需要 IPv4 filter、conntrack、规则注释和 MASQUERADE，配置端口转发时还需要 DNAT。见下文。 |
| `rootfs_image` | ext4 镜像需要 `CONFIG_EXT4_FS`，btrfs 镜像需要 `CONFIG_BTRFS_FS`。普通镜像文件还需要 `CONFIG_BLK_DEV_LOOP`、`/dev/loop-control` 和可用的 loop 设备。块设备直接挂载。目录 rootfs 使用承载该目录的宿主文件系统。 |
| `volatile: true` | `CONFIG_OVERLAY_FS`。tmpfs 充当可写上层，需要支持扩展属性（`CONFIG_TMPFS_XATTR`）。 |
| `binfmt` 跨架构执行 | `CONFIG_BINFMT_MISC`、启用的注册表、`F` 标志支持和宿主解释器。`F` 自上游 Linux 4.8 起可用。 |
| `security.allow_user_namespaces` | 嵌套工作负载创建 user namespace 时需要 `CONFIG_USER_NS`，且宿主策略和 namespace 配额允许创建。 |
| Android Binder / GPU | Binder 需要 `CONFIG_ANDROID_BINDER_IPC` 和宿主 Binder 设备节点。GPU 需要设备对应的驱动和节点，Android 安全策略需允许访问。 |

`nat` 的常见内核选项包括 `CONFIG_NETFILTER`、`CONFIG_NF_CONNTRACK`、`CONFIG_NF_NAT`。运行 `iptables --version` 可以确认宿主选择的后端。

- `legacy` 使用 `CONFIG_IP_NF_IPTABLES`、`CONFIG_IP_NF_FILTER`、`CONFIG_IP_NF_NAT`、`CONFIG_NETFILTER_XT_MATCH_CONNTRACK`、`CONFIG_NETFILTER_XT_MATCH_COMMENT` 和 `CONFIG_NETFILTER_XT_TARGET_MASQUERADE`。端口映射使用 `CONFIG_NETFILTER_XT_NAT` 的 DNAT target。
- `nf_tables` 使用 `CONFIG_NF_TABLES` 及 IPv4 的 conntrack、NAT 和 masquerade 表达式（`CONFIG_NFT_CT`、`CONFIG_NFT_NAT`、`CONFIG_NFT_MASQ`）。所用的 iptables 版本若通过 xtables 兼容层实现扩展，还需要 `CONFIG_NFT_COMPAT` 和相应扩展。

## 核对运行中的主机

先以启动容器时的权限运行 `kurumi-containerd check`（Linux 上可用 `sudo`，Android 上可在 root shell 中运行）。namespace 和 seccomp 探测在临时子进程中执行。文件系统来自 `/proc/filesystems`，其中仅列出已注册的类型。cgroup controller 来自当前可见的挂载，实际配置资源限制还需写权限。

若要区分编译选项与当前权限，可在导出内核配置的设备上查看 `/proc/config.gz`（`CONFIG_IKCONFIG_PROC`）。Linux 发行版也可能将配置保存在 `/boot/config-$(uname -r)`。以下命令核对默认隔离模式和 seccomp 所需的选项。

```sh
options='^(# )?CONFIG_(NAMESPACES|UTS_NS|PID_NS|IPC_NS|SECCOMP|SECCOMP_FILTER)(=| )'
if test -r /proc/config.gz; then
    zcat /proc/config.gz | grep -E "$options"
else
    grep -E "$options" "/boot/config-$(uname -r)"
fi
```

配置项为 `=y` 表示编入内核，`=m` 表示编为模块，`# CONFIG_… is not set` 表示关闭。`check` 中的 `unavailable` 也可能来自权限、SELinux、未加载的模块或未挂载的层级。所选配置的最终验证在 `start` 时完成。详见[配置参考](configuration.md)。
