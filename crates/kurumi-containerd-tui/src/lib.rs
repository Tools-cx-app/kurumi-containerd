// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

//! Terminal interface for managing configured containers through the CLI.

mod action;
mod app;
mod backend;
mod registry;
mod state;
mod terminal;
mod view;

pub use app::run;
