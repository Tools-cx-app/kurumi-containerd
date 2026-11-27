// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{collections::BTreeMap, net::Ipv4Addr, path::PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub runtime: RuntimeConfig,
    pub container: ContainerConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    #[serde(default = "default_stop_timeout")]
    pub stop_timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerConfig {
    pub name: String,
    #[serde(default)]
    pub uuid: Option<Uuid>,
    #[serde(default)]
    pub rootfs: Option<PathBuf>,
    #[serde(default)]
    pub rootfs_image: Option<PathBuf>,
    #[serde(default = "default_hostname")]
    pub hostname: String,
    #[serde(default = "default_init")]
    pub init: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binfmt: Option<BinfmtConfig>,
    #[serde(default)]
    pub isolation: IsolationMode,
    #[serde(default)]
    pub foreground: bool,
    #[serde(default)]
    pub volatile: bool,
    #[serde(default)]
    pub network: NetworkMode,
    #[serde(default)]
    pub network_options: NetworkConfig,
    #[serde(default)]
    pub android: AndroidConfig,
    #[serde(default)]
    pub resources: ResourceConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub mounts: Vec<BindMount>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub environment_file: Option<PathBuf>,
}

/// An ELF architecture and a host-native, statically linked userspace interpreter.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BinfmtConfig {
    pub arch: BinfmtArch,
    pub interpreter: PathBuf,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BinfmtArch {
    #[serde(alias = "arm64", alias = "armv8")]
    Aarch64,
    Alpha,
    #[serde(alias = "armhf", alias = "arm32", alias = "armel", alias = "armv7")]
    Arm,
    Armeb,
    Cris,
    Hexagon,
    Hppa,
    #[serde(alias = "x86")]
    I386,
    #[serde(alias = "loong64", alias = "loongarch")]
    Loongarch64,
    M68k,
    Microblaze,
    Mips,
    Mips64,
    Mips64el,
    Mipsel,
    Mipsn32,
    Mipsn32el,
    Ppc,
    Ppc64,
    Ppc64le,
    Riscv32,
    Riscv64,
    S390x,
    Sh4,
    Sh4eb,
    Sparc,
    Sparc32plus,
    Sparc64,
    #[serde(alias = "amd64")]
    X86_64,
    Xtensa,
    Xtensaeb,
}

impl BinfmtArch {
    /// Returns whether this target uses the runtime binary's native ELF ABI.
    #[must_use]
    pub const fn is_native(self) -> bool {
        match self {
            Self::Aarch64 => cfg!(target_arch = "aarch64"),
            Self::Arm => cfg!(all(target_arch = "arm", target_endian = "little")),
            Self::Armeb => cfg!(all(target_arch = "arm", target_endian = "big")),
            Self::I386 => cfg!(target_arch = "x86"),
            Self::Loongarch64 => cfg!(target_arch = "loongarch64"),
            Self::Mips => cfg!(all(target_arch = "mips", target_endian = "big")),
            Self::Mipsel => cfg!(all(target_arch = "mips", target_endian = "little")),
            Self::Mips64 => cfg!(all(target_arch = "mips64", target_endian = "big")),
            Self::Mips64el => cfg!(all(target_arch = "mips64", target_endian = "little")),
            Self::Ppc => cfg!(target_arch = "powerpc"),
            Self::Ppc64 => cfg!(all(target_arch = "powerpc64", target_endian = "big")),
            Self::Ppc64le => cfg!(all(target_arch = "powerpc64", target_endian = "little")),
            Self::Riscv32 => cfg!(target_arch = "riscv32"),
            Self::Riscv64 => cfg!(target_arch = "riscv64"),
            Self::S390x => cfg!(target_arch = "s390x"),
            Self::Sparc => cfg!(target_arch = "sparc"),
            Self::Sparc64 => cfg!(target_arch = "sparc64"),
            Self::X86_64 => cfg!(target_arch = "x86_64"),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum IsolationMode {
    #[default]
    Isolated,
    #[cfg(feature = "servicectl")]
    SharedPidIpc,
}

impl std::fmt::Display for IsolationMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Isolated => "isolated",
            #[cfg(feature = "servicectl")]
            Self::SharedPidIpc => "shared-pid-ipc",
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkConfig {
    pub address: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub prefix: u8,
    pub bridge: String,
    pub gateway_bridge: String,
    pub dns: Vec<String>,
    pub ports: Vec<PortForward>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            address: Ipv4Addr::new(172, 28, 0, 2),
            gateway: Ipv4Addr::new(172, 28, 0, 1),
            prefix: 16,
            bridge: "kurumi-br0".to_owned(),
            gateway_bridge: String::new(),
            dns: vec!["1.1.1.1".to_owned(), "8.8.8.8".to_owned()],
            ports: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortForward {
    pub host: u16,
    pub container: u16,
    #[serde(default)]
    pub protocol: Protocol,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct AndroidConfig {
    pub storage: bool,
    pub gpu: bool,
    pub binder: bool,
    pub termux_x11: bool,
    pub virgl: bool,
    pub pulse_audio: bool,
    pub selinux_permissive: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourceConfig {
    pub memory_bytes: Option<u64>,
    pub cpu_quota: Option<u64>,
    pub cpu_period: Option<u64>,
    pub pids: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SecurityConfig {
    pub read_only_sys: bool,
    pub allow_user_namespaces: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            read_only_sys: true,
            allow_user_namespaces: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NetworkMode {
    #[default]
    Host,
    None,
    Nat,
    Gateway,
    Dhcp,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BindMount {
    pub source: PathBuf,
    pub target: PathBuf,
    #[serde(default)]
    pub read_only: bool,
}

fn default_stop_timeout() -> u64 {
    15
}

fn default_hostname() -> String {
    String::new()
}

fn default_init() -> PathBuf {
    PathBuf::from("/sbin/init")
}
