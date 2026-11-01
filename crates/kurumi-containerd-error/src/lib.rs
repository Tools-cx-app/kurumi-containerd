// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

//! Shared configuration and runtime errors.

pub mod config;
pub use config::ConfigError;

#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "runtime")]
pub use runtime::{ErrorContext, Result, RuntimeError};

#[cfg(test)]
#[path = "../tests/unit/tests.rs"]
mod tests;
