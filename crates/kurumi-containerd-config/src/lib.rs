//! Configuration schema, loading, path resolution, and validation for `KurumiContainerd`.

mod config;
mod environment;
mod model;
mod pointer;
pub use pointer::ConfigPointer;

pub use config::{ConfigError, Result};
pub use environment::parse_environment;
pub use model::{
    AndroidConfig, BindMount, Config, ContainerConfig, NetworkConfig, NetworkMode, PortForward,
    Protocol, ResourceConfig, RuntimeConfig, SecurityConfig,
};

#[cfg(test)]
mod tests;
