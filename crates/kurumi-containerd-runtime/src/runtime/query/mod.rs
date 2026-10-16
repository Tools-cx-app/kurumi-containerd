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
mod tests {
    use super::*;

    struct FakeRuntime;

    impl ContainerRuntime for FakeRuntime {
        fn install(&self, _: &std::path::Path, _: Option<u64>, _: bool) -> Result<()> {
            Ok(())
        }
        fn start(&self, _: bool) -> Result<ContainerState> {
            Err(crate::RuntimeError::Message("start".into()))
        }
        fn stop(&self) -> Result<()> {
            Ok(())
        }
        fn restart(&self, _: bool) -> Result<ContainerState> {
            Err(crate::RuntimeError::Message("restart".into()))
        }
        fn enter(&self, _: &str) -> Result<i32> {
            Ok(0)
        }
        fn run(&self, _: &[String]) -> Result<i32> {
            Ok(0)
        }
        fn info(&self) -> Result<ContainerInfo> {
            Err(crate::RuntimeError::Message("info".into()))
        }
        fn pid(&self) -> Result<i32> {
            Err(crate::RuntimeError::Message("pid".into()))
        }
        fn list(&self) -> Result<Vec<ContainerState>> {
            Ok(Vec::new())
        }
        fn scan(&self) -> Result<Vec<ContainerState>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn query_forwards_read_only_calls_and_errors() {
        let runtime = FakeRuntime;
        let query: &dyn ContainerQuery = &runtime;
        assert!(query.list().unwrap().is_empty());
        assert_eq!(query.info().unwrap_err().to_string(), "info");
        assert_eq!(query.pid().unwrap_err().to_string(), "pid");
    }
}
