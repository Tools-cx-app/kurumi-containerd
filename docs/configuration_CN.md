# 配置参考

[[zh](configuration_CN.md) / [en](configuration.md)]

[文档目录](README_CN.md)

<a id="file-location-and-container-selection"></a>

## 文件位置与容器选择

CLI 和 TUI 读取 `$HOME/.kurumi-containerd/config.json`。`HOME` 必须是绝对路径，
以运行进程的环境为准；使用 `sudo -H` 时通常是 root 的 HOME。
配置文件需要手动创建。文件缺失、JSON 无效、未知字段或非法值都会报错，
不会自动创建配置或使用兜底配置。

顶层是非空列表，每项都是完整的容器配置：

```json
[
  {
    "runtime": {},
    "container": {
      "name": "debian",
      "rootfs": "/srv/kurumi/debian/rootfs"
    }
  }
]
```

每项必须包含 `runtime` 和 `container`。名称区分大小写，且不能重复。
只有一项时自动选择；有多项时，在子命令前写 `--name NAME`。
`backup CONTAINER_ID OUTPUT` 使用位置参数 ID 匹配 `container.name`，不允许同时指定
`--name`。TUI 列出全部条目。`check` 不读取 HOME 或配置。

不再支持外层 `name`/`file` 指针、独立 TOML 加载、`-c/--config` 或
`KURUMI_CONTAINERD_CONFIG`。旧安装请参阅[迁移指南](migration-config-pointer_CN.md)。

<a id="paths-and-persistent-identity"></a>

## 路径与持久化身份

以下相对主机路径以 `config.json` 所在目录为基准：`rootfs`、`rootfs_image`、
bind mount 的 `source`、`environment_file`。路径不会展开 `~` 或环境变量。
`init` 和挂载 `target` 是容器内部路径，即使以 `/` 开头也不是主机路径。

普通运行时命令会为选中条目补写缺失的 UUID。原子重写保留其他条目、文件所有者、权限，
以及指向配置文件的符号链接。已有 UUID 保持不变。安装、备份和 TUI 的只读刷新不会补写
UUID。请在 rootfs 归档之外单独备份这份 JSON。

<a id="runtime-settings"></a>

## 运行时设置

| `runtime` 字段 | 默认值 | 含义与校验 |
| --- | --- | --- |
| `stop_timeout_seconds` | `15` | 优雅停止超时，单位秒；必须大于零 |

运行时文件在 Linux 上位于 `/run/kurumi-containerd`，在 Android 上位于
`$TMP/kurumi-containerd`。目录必须归 root 所有，不能允许组或其他用户写入，
也不能是符号链接。没有可配置的工作目录字段。

<a id="container-settings"></a>

## 容器设置

| `container` 字段 | 默认值 | 含义与校验 |
| --- | --- | --- |
| `name` | 必填 | 仅 ASCII 字母、数字、`.`、`_`、`-`；非空，不能为 `.` 或 `..` |
| `uuid` | 未设置 | 持久化 UUID；普通运行时加载时补写缺失值 |
| `rootfs` | 未设置 | 主机目录；与 `rootfs_image` 必须且只能配置一个 |
| `rootfs_image` | 未设置 | 主机文件系统镜像或块设备路径 |
| `hostname` | 容器名称 | 省略或为空时使用 `name` |
| `init` | `/sbin/init` | rootfs 内的绝对路径 |
| `foreground` | `false` | start/restart 使用前台控制台 |
| `volatile` | `false` | 将 OverlayFS 可写层放在 tmpfs，修改会被丢弃 |
| `network` | `host` | `host`、`none`、`nat`、`gateway` 或 `dhcp` |
| `network_options` | 见下文 | 网络地址、网桥、DNS 和端口转发 |
| `resources` | 不配置限制 | 下文的 cgroup 限制 |
| `security` | 见下文 | 内核视图和嵌套 user namespace |
| `environment` | `{}` | 字符串形式的环境变量 |
| `environment_file` | 未设置 | 主机环境变量文件路径 |
| `mounts` | `[]` | 下文的 bind mount |
| `android` | 所有开关为 `false` | 下文的 Android 主机集成 |

普通加载要求 rootfs 已存在；目录 rootfs 还要求配置的 init 路径存在。
`install` 允许目标缺失：将本地归档安装到 `rootfs`，或通过 `--size SIZE`
创建稀疏 ext4 `rootfs_image`。覆盖已有目标需要 `--force`。
参阅[安装 rootfs](usage_CN.md#install-a-local-rootfs)。

下列示例是合并到选中条目的对象片段，不是完整配置文件。
[config.example.json](../config.example.json) 提供较完整的示例；
请调整其中的路径，并删除不需要的挂载。

<a id="networking"></a>

## 网络

| 模式 | 行为 |
| --- | --- |
| `host` | 共享主机网络 |
| `none` | 使用隔离的 network namespace |
| `nat` | 配置私有网络、主机 NAT 以及可选端口转发 |
| `gateway` | 将 veth 接入已有 `gateway_bridge`，使用静态地址 |
| `dhcp` | 接入已有 `gateway_bridge`，在 rootfs 内运行 DHCP 客户端 |

```json
{
  "container": {
    "network": "nat",
    "network_options": {
      "address": "172.28.0.2",
      "gateway": "172.28.0.1",
      "prefix": 16,
      "bridge": "kurumi-br0",
      "gateway_bridge": "",
      "dns": ["1.1.1.1", "8.8.8.8"],
      "ports": [{"host": 8080, "container": 80, "protocol": "tcp"}]
    }
  }
}
```

以上地址、前缀、网桥名称和 DNS 为默认值，`ports` 默认为空列表。
`address` 和 `gateway` 必须是 IPv4 地址；DNS 条目必须是 IP 地址。
前缀不能大于 32。每条转发规则必须提供非零的 16 位 `host` 和 `container` 端口；
`protocol` 为 `tcp`（默认）或 `udp`。同一协议内不允许重复的主机端口或容器端口。
不支持端口范围。

NAT 和网桥配置使用主机上的 `ip`、`iptables` 可执行文件。
`gateway` 和 `dhcp` 要求 `gateway_bridge` 是有效的已有网桥名称；运行时不会创建或管理
该网桥。DHCP 按顺序使用 rootfs 内找到的第一个客户端：`udhcpc`、`dhclient`、`dhcpcd`。

```json
{
  "container": {
    "network": "dhcp",
    "network_options": {"gateway_bridge": "br0"}
  }
}
```

网络和资源设置的修改在下次启动后生效。`enter` 和 `run` 加入正在运行的容器实际使用的
namespace 和 cgroup。

<a id="resource-limits-and-security"></a>

## 资源限制与安全

```json
{
  "container": {
    "resources": {
      "memory_bytes": 536870912,
      "cpu_quota": 100000,
      "cpu_period": 100000,
      "pids": 512
    },
    "security": {
      "read_only_sys": true,
      "allow_user_namespaces": false
    }
  }
}
```

资源字段均为可选的无符号整数。可用时使用 cgroup v2，否则使用 cgroup v1。
`memory_bytes` 至少为 4194304（4 MiB）。CPU quota 和 period 的单位是微秒，
必须同时配置；`cpu_quota` 至少为 1000，`cpu_period` 必须大于零。
`pids` 必须在 1 到 4194304 之间。请求的 controller 不可用时会报错。

`read_only_sys` 默认为 `true`，`allow_user_namespaces` 默认为 `false`。
只有需要非特权 user namespace 的可信工作负载才应开启后者，例如 Docker userns-remap、
Podman、Flatpak 或 Bubblewrap。保留正常加固的 `/proc`、`/sys`，另外提供全新的
`/run/kurumi-containerd/proc`（可写）和 `/run/kurumi-containerd/sys`（只读）挂载。
额外的 proc 视图可能暴露主机内核 sysctl，从而削弱隔离。容器 root 保留 capabilities，
因此这里不构成针对恶意工作负载的安全边界。

<a id="environment-and-bind-mounts"></a>

## 环境变量与 bind mount

```json
{
  "container": {
    "environment_file": "./container.env",
    "environment": {"LANG": "C.UTF-8"},
    "mounts": [
      {"source": "./shared", "target": "/mnt/shared", "read_only": false}
    ]
  }
}
```

环境变量文件支持 `KEY=VALUE`、可选的 `export`、注释、带引号或空的值，以及包含 `=`
的值。内联配置覆盖文件中的同名变量。键必须是有效的环境变量名，值不能包含 NUL 字节。
环境变量文件必须存在。

每个挂载需要已存在的主机 `source`，以及绝对、非根路径、没有父目录跳转的容器 `target`。
`read_only` 默认为 `false`。Bind mount 将主机数据暴露给容器，rootfs 备份不包含其源路径。

<a id="android-integration"></a>

## Android 集成

`container.android` 的所有字段都是布尔值，默认为 `false`：

| 字段 | 主机资源或效果 |
| --- | --- |
| `storage` | 将 `/storage/emulated/0` 绑定挂载到容器 |
| `gpu` | 暴露匹配的 DRI render、KGSL、Mali 和 video 设备 |
| `binder` | 暴露 `/dev/binder`、`/dev/hwbinder` 和 `/dev/vndbinder` |
| `termux_x11` | 将 Termux X11 的 `X5` socket 绑定到 `/tmp/.X11-unix/X5` |
| `virgl` | 将 Termux 的 `.virgl_test` socket 绑定到 `/tmp/.virgl_test` |
| `pulse_audio` | 将 Termux 的 `.pulse-socket` 绑定到 `/tmp/.pulse-socket` |
| `selinux_permissive` | 请求在容器生命周期内使用 permissive SELinux 处理 |

这些选项需要 Android 构建和相应主机资源；不支持的主机或缺失的所需资源会报错。
它们用于暴露资源，不会安装或监管 Android 应用或对应的主机服务。
安装与开机启动见 [Android root 模块](android-module_CN.md)。
