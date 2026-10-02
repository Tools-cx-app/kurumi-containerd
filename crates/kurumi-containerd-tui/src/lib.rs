//! Terminal interface for managing configured containers through the CLI.

mod action;
mod app;
mod registry;
mod state;
mod terminal;
mod view;

pub use app::run;
