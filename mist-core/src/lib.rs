//! Pueblo/UE rewrite — protocol, scripting, markup.
//!
//! No UI lives here. This crate is `no_std`-friendly in spirit (we use only
//! `core` types where possible) so it can be embedded in any frontend.

pub mod ansi;
pub mod error;
pub mod markup;
pub mod profile;
pub mod telnet;
pub mod tintin;

pub use error::PuebloError;

/// Crate version (used by `pueblo --version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");