//! Configuration errors and context helpers.

/// A configuration loading or validation failure.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0}")]
    Invalid(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{context}: {source}")]
    TomlParse {
        context: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to serialize persistent TOML config: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    #[error(transparent)]
    PlainIo(#[from] std::io::Error),
    #[error("{context}: {source}")]
    Context {
        context: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// A configuration operation result.
pub type Result<T> = std::result::Result<T, ConfigError>;

/// Adds configuration-specific context while preserving error variants.
pub trait ErrorContext<T> {
    /// Adds static context.
    ///
    /// # Errors
    /// Returns the original failure with context.
    fn context(self, context: &'static str) -> Result<T>;
    /// Adds lazily constructed context.
    ///
    /// # Errors
    /// Returns the original failure with context.
    fn with_context(self, context: impl FnOnce() -> String) -> Result<T>;
}

impl<T, E: Into<ConfigError>> ErrorContext<T> for std::result::Result<T, E> {
    fn context(self, context: &'static str) -> Result<T> {
        self.with_context(|| context.to_owned())
    }

    fn with_context(self, context: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|source| match source.into() {
            ConfigError::PlainIo(source) => ConfigError::Io {
                context: context(),
                source,
            },
            source => ConfigError::Context {
                context: context(),
                source: Box::new(source),
            },
        })
    }
}

impl<T> ErrorContext<T> for Option<T> {
    fn context(self, context: &'static str) -> Result<T> {
        self.ok_or_else(|| ConfigError::Invalid(context.to_owned()))
    }

    fn with_context(self, context: impl FnOnce() -> String) -> Result<T> {
        self.ok_or_else(|| ConfigError::Invalid(context()))
    }
}

/// Returns a configuration validation error.
#[macro_export]
macro_rules! config_bail {
    ($($arg:tt)*) => { return Err($crate::ConfigError::Invalid(format!($($arg)*))) };
}

/// Validates a configuration condition.
#[macro_export]
macro_rules! config_ensure {
    ($condition:expr, $($arg:tt)*) => {
        if !$condition { $crate::config_bail!($($arg)*); }
    };
}
