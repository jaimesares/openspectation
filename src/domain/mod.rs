//! Filesystem model of an OpenSpec workspace.
//!
//! Nothing in this module touches the terminal: it parses, aggregates and
//! exposes plain data so both the CLI renderer and the TUI can consume it.

pub mod change;
pub mod config;
pub mod delta;
pub mod editor;
pub mod filter;
pub mod history;
pub mod openspec;
pub mod scanner;
pub mod settings;
pub mod spec;
pub mod tasks;
pub mod template;
pub mod util;
pub mod watch;
pub mod workspace;
