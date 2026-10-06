//! Configuration schema, loading, path resolution, and validation for `KurumiContainerd`.

mod config;
mod environment;
mod model;
mod pointer;
pub use pointer::ConfigPointer;

pub use config::resolve_container_path;
pub use config::{ConfigError, Result};
pub use environment::parse_environment;
pub use model::{
    AndroidConfig, BindMount, Config, ContainerConfig, NetworkConfig, NetworkMode, PortForward,
    Protocol, ResourceConfig, RuntimeConfig, SecurityConfig,
};

/// Validates a configuration value, including any normalization required by
/// the configuration format.
pub trait Validate {
    /// The error returned when validation fails.
    type Error;

    /// Validates and, where required, normalizes the value.
    ///
    /// # Errors
    ///
    /// Returns the validation error for an invalid value.
    fn validate(&mut self) -> std::result::Result<(), Self::Error>;
}

#[cfg(test)]
mod tests;
