# Android root 模块

[[zh](android-module_CN.md) / [en](android-module.md)]

[文档目录](README_CN.md)

## 要求与安装

通过 Magisk、KernelSU 或 APatch Manager 安装
`kurumi-containerd-<version>-android-module.zip`，然后重启。
要求 Android API 21 或更高版本，以及可用的 `timeout` 命令。
ZIP 包含 `arm64-v8a`、`armeabi-v7a`、`x86_64`、`x86` 二进制，
安装时自动选择设备架构。

模块提供特权 CLI 和启动脚本。容器操作仍要求内核支持其使用的 namespace、挂载和资源
管理功能。请只运行可信的 rootfs 和命令。

## 持久化文件与命令

模块将持久化数据放在模块安装目录之外：

```text
/data/adb/kurumi-containerd/
├── .kurumi-containerd/config.json
├── autostart.txt
├── boot.log
└── tmp/
```

按照[配置参考](configuration_CN.md)创建 `config.json`，每个条目包含容器的完整配置。
安装器创建私有目录和初始 `autostart.txt`，但不会生成容器配置或安装 rootfs。
持久化目录不能是符号链接。

在 root shell 中使用包装命令：

```sh
/data/adb/modules/kurumi-containerd/bin/kurumi-containerd check
/data/adb/modules/kurumi-containerd/bin/kurumi-containerd --name debian info
```

包装脚本设置 `HOME=/data/adb/kurumi-containerd`，并将 `TMP`、`TMPDIR` 指向其下的
`tmp` 目录。`KURUMI_CONTAINERD_HOME` 可覆盖包装脚本和启动脚本使用的持久化根目录；
编辑配置时必须使用相同目录。root 管理器支持 system 挂载时可使用
`system/bin/kurumi-containerd` 便捷入口，否则使用上面的模块完整路径。

## 开机启动

在 `autostart.txt` 中每行填写一个精确匹配 JSON `container.name` 的名称。
空行和以 `#` 开头的行会被忽略：

```text
debian
alpine
```

所有自启动条目都应设置 `container.foreground` 为 `false`。
等待 `sys.boot_completed=1` 后，脚本按顺序处理条目：先检查容器是否已运行，
再将停止的容器启动到后台。每次 CLI 调用限时 120 秒。
某个条目失败时记录日志并继续处理后续条目，不会自动重试。

开机启动失败后查看 `/data/adb/kurumi-containerd/boot.log`。
存在自启动文件时，启动脚本将自身输出重定向到这个文件；它不是持久化的容器控制台日志。
参阅 [CLI 输出说明](usage_CN.md#output-and-diagnostics)。

## 停止容器或禁用自启动

在 root shell 中停止容器：

```sh
/data/adb/modules/kurumi-containerd/bin/kurumi-containerd --name debian stop
```

从 `autostart.txt` 删除对应行会影响后续开机启动。在 root 管理器中禁用模块会禁用
开机启动；停止当前运行的容器仍应显式使用 `stop`。
备份 rootfs 时请单独保存 `config.json`。
可选的存储、GPU、Binder 和 socket 集成见[配置参考](configuration_CN.md#android-integration)。
