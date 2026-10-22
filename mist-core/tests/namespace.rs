//! Smoke test for the persistence namespace.
//!
//! `mist_core::profile::config_path()` uses
//! `ProjectDirs::from("com.wyldware", "Wyldware", "mist")`, which yields
//! platform-idiomatic paths:
//!
//!   Linux   `~/.config/mist/config.toml`
//!   macOS   `~/Library/Application Support/com.wyldware/mist/config.toml`
//!   Windows `%APPDATA%\Wyldware\mist\config\config.toml`
//!
//! The Flatpak application ID is `com.wyldware.mist`.
//!
//! We can only observe the Linux variant from here, so we assert the
//! structural invariants that hold everywhere: the path ends in
//! `mist/config.toml`, and the parent directory is `mist` (so a user
//! sees `~/.config/mist/`, not a publisher-prefixed directory).

use mist_core::profile::config_path;

#[test]
fn config_path_is_plain_mist_dir() {
    let path = config_path().expect("config_path must resolve on every platform");
    let s = path.to_string_lossy().to_lowercase();

    assert!(
        s.ends_with("mist/config.toml"),
        "config path should be <dir>/mist/config.toml, got: {s}"
    );

    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(|n| n.to_lowercase());
    assert_eq!(
        parent.as_deref(),
        Some("mist"),
        "config dir should be named 'mist', got: {s}"
    );

    eprintln!("resolved config path: {s}");
}