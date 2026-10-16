use crate::{ContainerInfo, ContainerRuntime, ContainerState, Result};

/// Read-only container capabilities used by status-oriented callers.
pub trait ContainerQuery {
    /// Returns current state and resource usage.
    ///
    /// # Errors
    /// Returns an error when state or resource information cannot be read.
    fn info(&self) -> Result<ContainerInfo>;
    /// Returns the validated init PID.
    ///
    /// # Errors
    /// Returns an error when the container is not running.
    fn pid(&self) -> Result<i32>;
    /// Lists live containers.
    ///
    /// # Errors
    /// Returns an error when runtime state cannot be read.
    fn list(&self) -> Result<Vec<ContainerState>>;
}

impl<T: ContainerRuntime + ?Sized> ContainerQuery for T {
    fn info(&self) -> Result<ContainerInfo> {
        ContainerRuntime::info(self)
    }
    fn pid(&self) -> Result<i32> {
        ContainerRuntime::pid(self)
    }
    fn list(&self) -> Result<Vec<ContainerState>> {
        ContainerRuntime::list(self)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/query/tests.rs"]
mod tests;
