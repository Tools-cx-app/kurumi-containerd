# 故障排查

[简体中文](../zh/troubleshooting.md) | [English](../en/troubleshooting.md)

[文档目录](README.md)

先保留完整错误链，再使用 `--verbose` 查看诊断。Linux 示例假定运行程序位于 PATH：

```sh
sudo kurumi-containerd check
sudo -H kurumi-containerd --name debian --verbose info
```

release 诊断写入 stderr，debug 诊断写入 stdout。普通结果输出到 stdout。
后台 monitor 启动后将标准流重定向到 `/dev/null`；重定向 CLI 的 stderr 不会得到持续的
容器控制台日志。需要观察启动过程时使用 `start --foreground`。

## 配置或容器选择失败

| 现象 | 检查与处理 |
| --- | --- |
| 找不到配置或名称 | 核对当前进程的 HOME；`sudo -H` 通常使用 root 的 HOME。名称必须精确匹配 `container.name` |
| 多个条目无法自动选择 | 将 `--name NAME` 放在子命令前；backup 则只用位置参数名称 |
| 未知字段或 JSON 解析失败 | 配置必须是 JSON 列表，包含完整的 `runtime` 和 `container`；不允许注释、尾逗号和旧 TOML/指针字段 |
| rootfs 或 init 不存在 | 核对路径基准、归档顶层结构及 init 路径；安装缺失 rootfs 应使用 `install` |
| 安装失败后出现新配置 | 安装先保存默认条目再解包；修正该条目后重试 |

相对主机路径以配置文件所在目录为基准，不是当前工作目录；不会展开 `~` 或环境变量。
完整规则见[配置参考](configuration.md)。

## 启动或 cgroup 失败

`check` 不读取配置，成功也不能证明所有选项均可用。启动仍需挂载权限、合适的
rootfs/init，以及选中网络、安全和资源选项所需能力。受限的外层容器中，即使是 root
也可能没有 namespace 或 mount 权限。

请求资源限制时，确认 cgroup controller 可用且层级可写。cgroup v2 的 hierarchy root
和 `kurumi-containerd` 父节点都需启用所请求的 controller；不要清空已有的
`cgroup.subtree_control` 设置来排错。共享模式无论是否配置限制，都需要专属可写的
cgroup v2 子树及 cgroup namespace；不要求 `cgroup.kill`。

`shared-pid-ipc` 还要求编译时启用 `servicectl`，并使用保持前台的 supervisor。
普通 systemd 系统实例不受支持。缺少 PID/IPC namespace 时不会自动切换模式。
详见[共享模式服务](servicectl.md)。

## 登录、命令与控制台

- shell 文件存在却报 `No such file or directory`：检查架构、脚本解释器和动态加载器。
- Alpine 登录失败：尝试 `enter --shell /bin/ash` 或 `enter --shell '/bin/busybox ash'`。
- 管道没有执行：`run` 直接传递参数，需显式调用 `run sh -c '...'`。
- 找不到 `tui`：使用带 `tui` feature 的构建。交互/前台 TUI 操作还需要 Linux 图形终端
  `xterm`、`kitty` 或 `alacritty`；无图形环境或 Android 不支持这些操作。
- 前台需要关机：依次按 Escape、Ctrl-Q。退出 `enter` shell 不会停止容器。

## 安装与备份

安装只接受本地 tar/压缩 tar/ZIP。目录目标不允许 `--size`，镜像目标必须指定它，
并需要 loop、挂载及 ext4 格式化工具。现有目标需 `--force`；先确认选中条目和路径。

备份要求容器停止、输出目录已存在且位于 rootfs 外，输出文件不能已存在。
先卸载 rootfs 子挂载；特殊文件和内核文件系统会导致拒绝。ZIP 有元数据损失，tar
恢复也存在 xattr 限制。不要把 rootfs 归档当作完整容器备份；单独保存配置和外部挂载源。
详见[备份与恢复](usage.md#back-up-a-container)。

## 状态恢复和 Android 启动

`scan` 需要受保护的宿主恢复记录与匹配的容器内身份，不会仅凭某个 PID 收养进程。
共享模式还会验证 worker、挂载 namespace、启动代次和专属 cgroup。不要手工编辑
状态记录或把删除运行目录作为常规修复；先核对进程身份和完整错误链。

Android 模块应使用模块 wrapper，以保持 HOME/TMP 一致。检查
`/data/adb/kurumi-containerd/boot.log`、`autostart.txt` 中的精确名称和
`foreground: false`。每次启动尝试有 120 秒超时且不自动重试。详见
[Android 部署](android-module.md)。
