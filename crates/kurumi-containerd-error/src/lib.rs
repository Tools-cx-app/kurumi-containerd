//! Shared configuration and runtime errors.

pub mod config;
pub use config::ConfigError;

/// A runtime operation result.
pub type Result<T> = std::result::Result<T, RuntimeError>;

/// An error from a host resource or container operation.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("{0}")]
    Message(String),
    #[error("{context}: {source}")]
    Context {
        context: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Proc(#[from] procfs::ProcError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Toml(#[from] toml::ser::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Nul(#[from] std::ffi::NulError),
    #[error(transparent)]
    Integer(#[from] std::num::TryFromIntError),
    #[error(transparent)]
    SystemTime(#[from] std::time::SystemTimeError),
    #[error(transparent)]
    StripPrefix(#[from] std::path::StripPrefixError),
}

/// Attaches context without discarding the underlying error.
pub trait ErrorContext<T> {
    /// Adds static context.
    ///
    /// # Errors
    /// Returns the original failure with context.
    fn context(self, context: &'static str) -> crate::Result<T>;
    /// Adds lazily constructed context.
    ///
    /// # Errors
    /// Returns the original failure with context.
    fn with_context(self, context: impl FnOnce() -> String) -> crate::Result<T>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> ErrorContext<T>
    for std::result::Result<T, E>
{
    fn context(self, context: &'static str) -> crate::Result<T> {
        self.with_context(|| context.to_owned())
    }

    fn with_context(self, context: impl FnOnce() -> String) -> crate::Result<T> {
        self.map_err(|source| RuntimeError::Context {
            context: context(),
            source: Box::new(source),
        })
    }
}

impl<T> ErrorContext<T> for Option<T> {
    fn context(self, context: &'static str) -> crate::Result<T> {
        self.with_context(|| context.to_owned())
    }

    fn with_context(self, context: impl FnOnce() -> String) -> crate::Result<T> {
        self.ok_or_else(|| RuntimeError::Message(context()))
    }
}

/// Returns a runtime error with a formatted message.
#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => {
        return Err($crate::RuntimeError::Message(format!($($arg)*)))
    };
}

/// Returns a runtime error when a condition is false.
#[macro_export]
macro_rules! ensure {
    ($condition:expr, $($arg:tt)*) => {
        if !$condition {
            $crate::bail!($($arg)*);
        }
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn context_keeps_io_source() {
        use std::error::Error as _;

        use super::ErrorContext as _;

        let dir = tempfile::tempdir().unwrap();
        let error = std::fs::read(dir.path().join("missing"))
            .context("reading test file")
            .unwrap_err();
        assert!(error.to_string().starts_with("reading test file:"));
        assert!(error.source().is_some());
        assert_eq!(
            error
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::NotFound
        );
    }
}
