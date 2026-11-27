# 快速开始

[简体中文](../zh/getting-started.md) | [English](../en/getting-started.md)

[文档目录](README.md)

本页使用 Linux、目录 rootfs 和默认的 `isolated` 模式。准备一份可信、架构匹配的
Linux rootfs 归档，其顶层应直接包含 `sbin/init` 等文件。程序仅安装本地归档；
归档来源、签名或校验和由使用者确认。Android 模块的目录和启动方式见
[Android 部署](android-module.md)。

## 1. 构建并检查主机

在仓库根目录使用当前 stable Rust 工具链构建。工作区采用 Rust 2024 edition，
具体依赖的编译器要求以 Cargo 提示和 CI 工具链为准。

```sh
cargo build --release -p kurumi-containerd
sudo ./target/release/kurumi-containerd check
```

`check` 不读取 HOME 或配置，报告 namespace、挂载、cgroup 和网络工具能力。
启动时还会检查所选配置的实际前提。若需要终端管理界面，构建时添加
`--features tui`；若需要共享 PID/IPC，添加 `--features servicectl`。
两个 feature 均默认关闭，`--release` 不会自动启用它们。

## 2. 安装第一个 rootfs

以下命令假设 HOME 登记文件中尚无 `debian` 条目：

```sh
sudo -H ./target/release/kurumi-containerd --name debian install ./rootfs.tar.zst
```

安装会创建或扩充 `$HOME/.kurumi-containerd/config.json`，新条目的默认目录为
`$HOME/.kurumi-containerd/debian.rootfs`。此处 HOME 是 root 的 HOME。
如果名称已存在，则沿用原设置；已有 rootfs 需要显式 `--force` 才能替换。
配置先于解包保存，失败后可以编辑并重试；非法 JSON 不会被覆盖。

需要自定义路径时，先编辑 root 的登记文件再运行安装。最小配置如下：

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

`rootfs` 与 `rootfs_image` 必须且只能配置一个。新建稀疏 ext4 镜像时使用
`rootfs_image` 并向 `install` 传入 `--size 8G`；目录目标不接受 `--size`。
若新条目由安装自动创建，`--size` 会选择默认的 `debian.img`。
详见[安装说明](usage.md#install-a-local-rootfs)和[配置参考](configuration.md)。

## 3. 启动、验证并停止

```sh
sudo -H ./target/release/kurumi-containerd --name debian start
sudo -H ./target/release/kurumi-containerd --name debian info
sudo -H ./target/release/kurumi-containerd --name debian run uname -a
sudo -H ./target/release/kurumi-containerd --name debian enter
```

退出交互 shell 后，容器仍在运行。按需停止：

```sh
sudo -H ./target/release/kurumi-containerd --name debian stop
```

`start --foreground` 将控制台连接到当前终端；依次按 Escape、Ctrl-Q 可请求关机。
只有一个配置条目时可省略 `--name`，多个条目时必须明确选择。
`run` 的别名为 `exec`；需要管道或重定向时显式使用 `run sh -c '...'`。

## 4. 备份与后续配置

停止后可将 rootfs 导出到已有目录中的新文件：

```sh
sudo -H ./target/release/kurumi-containerd backup debian ./debian.zst
```

备份使用位置参数名称选择容器，不与 `--name` 混用；它不包含 `config.json`、
运行状态或 bind mount 源数据。格式和恢复限制见[备份与恢复](usage.md#back-up-a-container)。
网络、资源限制和挂载配置见[配置参考](configuration.md)。遇到失败时先按
[故障排查](troubleshooting.md)核对配置、主机能力和完整错误链。
