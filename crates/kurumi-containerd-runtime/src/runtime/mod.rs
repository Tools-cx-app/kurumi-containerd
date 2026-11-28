// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

//! Runtime orchestration, execution, and in-memory state.

mod backup;
mod boot;
mod execute;
mod install;
mod lifecycle;
pub(crate) mod query;
pub(crate) mod state;
mod supervisor;
