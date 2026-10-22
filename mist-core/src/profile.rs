//! Connection profiles.
//!
//! Original Pueblo stores everything in the Windows registry under
//! `HKEY_CURRENT_USER\Software\Pueblo\`. We replace that with a TOML file
//! in the platform config dir (`directories::ProjectDirs`).
//!
//! Schema mirrors the registry tree — one section per world, one per player.

use crate::error::{PuebloError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub player: Option<String>,
    #[serde(default = "default_true")]
    pub mccp: bool,
    #[serde(default = "default_true")]
    pub naws: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub worlds: Vec<Profile>,
    #[serde(default)]
    pub last_world: Option<String>,
}

impl Config {
    /// Load from the default platform location, or return empty if absent.
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(&path)?;
        toml::from_str(&s).map_err(|e| PuebloError::Profile(e.to_string()))
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self).map_err(|e| PuebloError::Profile(e.to_string()))?;
        std::fs::write(&path, s)?;
        Ok(())
    }
}

pub fn config_path() -> Result<PathBuf> {
    // Application namespace.
    //
    // `directories` follows each platform's convention exactly:
    //   * Linux — XDG mandates a single flat directory, so the crate uses
    //             ONLY the `application` argument. qualifier and
    //             organization are discarded (they are literally named
    //             `_qualifier` / `_organization` in directories-5.0.1's
    //             `lin.rs`). Result: `~/.config/mist/`.
    //   * macOS — qualifier + organization + application:
    //             `~/Library/Application Support/com.wyldware/mist/`.
    //   * Windows — organization + application under RoamingAppData:
    //             `%APPDATA%\Wyldware\mist\config\`.
    //
    // Wyldware is the publisher of record for Mist and its sibling MUD
    // tooling; the Flatpak build carries the application ID
    // `com.wyldware.mist`.
    let dirs = directories::ProjectDirs::from("com.wyldware", "Wyldware", "mist")
        .ok_or_else(|| PuebloError::Profile("no platform config dir".into()))?;
    Ok(dirs.config_dir().join("config.toml"))
}