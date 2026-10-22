//! egui application shell.
//!
//! v1: tabbed MDI — one tab per active world connection. Each tab has its own
//! scrollback, input line, and connection state.

pub mod app;
pub mod connect;
pub mod images;
pub mod session;

pub use app::MistApp;