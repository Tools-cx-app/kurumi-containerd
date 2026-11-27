# KurumiContainerd 使用文档

[简体中文](../zh/README.md) | [English](../en/README.md)

[项目概览](../../README_CN.md) · [语言选择](../README.md)

KurumiContainerd 在 Linux 和 Android 上管理特权 Linux 系统容器。运行容器需要
root、匹配主机架构的 rootfs，以及相应内核功能。容器共享主机内核，root 保留
capabilities；请使用可信的 rootfs 和命令。

第一次使用时，按[快速开始](getting-started.md)完成构建、主机检查、安装和启动。
已有安装仍使用 TOML 或 `name/file` 指针时，先阅读[配置迁移](migration-config-pointer.md)。
Android root 模块用户可直接从 [Android 部署](android-module.md)开始。

| 文档 | 解决的问题 |
| --- | --- |
| [快速开始](getting-started.md) | 构建程序、安装第一个 rootfs、验证运行并停止容器 |
| [CLI 与 TUI](usage.md) | 选择容器、生命周期、命令执行、备份恢复与终端管理 |
| [配置参考](configuration.md) | JSON 字段、默认值、路径、隔离、网络、资源和 Android 选项 |
| [故障排查](troubleshooting.md) | 配置选择、启动、cgroup、控制台、备份与恢复错误 |
| [Android 部署](android-module.md) | Magisk、KernelSU、APatch 模块、持久化目录和开机启动 |
| [共享模式服务](servicectl.md) | Dinit、s6-rc、servicectl 的依赖、安装、启动、回滚与验收 |
| [配置迁移](migration-config-pointer.md) | 将旧 TOML 和指针条目转换为单一 JSON 登记文件 |
| [共享模式兼容说明](shared-isolation-release.md) | feature、配置、Rust API 和运行状态兼容性 |
| [运行时架构](architecture.md) | 七个 crate 的职责、生命周期、宿主资源和状态信任 |
| [开发与验证](development.md) | 本地检查、feature 构建、跨平台编译、特权测试和基准 |
| [实现状态与限制](status.md) | 当前能力、部署前提与尚未覆盖的场景 |

文档中的 `kurumi-containerd` 假定程序已加入 PATH；源码构建可替换为
`./target/release/kurumi-containerd`。Linux 示例统一使用 `sudo -H`，因此配置位于
root 的 HOME。备份 rootfs 时，请另行保存 `config.json` 和 bind mount 源数据。
