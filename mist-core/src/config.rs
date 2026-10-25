//! Worlds and accounts — the persistent data model behind the world's
//! and accounts lists.
//!
//! Two distinct concepts, deliberately kept separate (UI-REQS §2.1):
//!
//! * A **world** is a server endpoint. It knows where to connect and how
//!   to present that connection — theme mode, encoding, whether to
//!   negotiate MCCP.
//! * An **account** is a credential set scoped to one world. The same
//!   login used on two worlds is two account records, because that is
//!   what a user actually needs to manage.
//!
//! The file format is TOML. The on-disk layout is a `[[worlds]]` array
//! of world tables and an `[[accounts]]` array of account tables, so
//! that reordering a world does not rewrite unrelated records:
//!
//! ```toml
//! [[worlds]]
//! name = "Wyldware"
//! host = "wyldware.com"
//! port = 2020
//! order = 0
//! theme_mode = "ansi-priority"
//!
//! [[accounts]]
//! world = "Wyldware"
//! login = "willow"
//! character = "willow"
//! autoconnect = false
//! ```
//!
//! Passwords are *not* stored here in the clear. See
//! [`PasswordRef`] and the credential-storage module for how the secret
//! itself is kept.

use crate::error::{PuebloError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How world-supplied colours interact with the theme.
///
/// This mirrors `crate::theme::Precedence` but is declared here so the
/// config schema does not depend on the theme module's internals. The
/// conversion lives in [`Precedence::from_config`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    /// The world's own colours win.
    #[default]
    AnsiPriority,
    /// The theme's palette is authoritative for indexed references.
    ThemePriority,
}

impl ThemeMode {
    /// Convert to the theme module's precedence enum.
    pub fn to_precedence(self) -> crate::theme::Precedence {
        match self {
            ThemeMode::AnsiPriority => crate::theme::Precedence::AnsiPriority,
            ThemeMode::ThemePriority => crate::theme::Precedence::ThemePriority,
        }
    }
}

/// The character encoding a world expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Encoding {
    /// UTF-8. The default, and what modern MUSH servers use.
    #[default]
    Utf8,
    /// IBM code page 437 — the original MUD character set.
    Cp437,
    /// ISO-8859-1.
    Latin1,
}

/// A server endpoint and its display/behaviour settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct World {
    /// User-visible label, e.g. "Wyldware".
    pub name: String,
    /// DNS name or IP address.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// Position in the user's list. The list is sorted by this, then by
    /// name, so the order is stable even if two records share a value.
    #[serde(default)]
    pub order: i32,
    /// Whether world colours or theme colours win.
    #[serde(default)]
    pub theme_mode: ThemeMode,
    /// Optional per-world theme override, by theme name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Character encoding for this world.
    #[serde(default)]
    pub encoding: Encoding,
    /// Which account to log in with automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autologin_account: Option<String>,
    /// Whether to negotiate MCCP compression with this world.
    #[serde(default = "default_true")]
    pub mccp: bool,
    /// Whether to offer the terminal type and window size.
    #[serde(default = "default_true")]
    pub naws: bool,
    /// Opt-in login-sequence autodetection (UI-REQS §2.3).
    ///
    /// Off by default: a wrong guess can send a password to the wrong
    /// prompt, and some servers log failed logins.
    #[serde(default)]
    pub login_autodetect: bool,
    /// Free-form note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// When this world was last connected to, as a Unix timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_connected: Option<i64>,
}

fn default_true() -> bool {
    true
}

impl World {
    /// A minimal world record. Everything else takes its default.
    pub fn new(name: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        Self {
            name: name.into(),
            host: host.into(),
            port,
            order: 0,
            theme_mode: ThemeMode::default(),
            theme: None,
            encoding: Encoding::default(),
            autologin_account: None,
            mccp: true,
            naws: true,
            login_autodetect: false,
            notes: None,
            last_connected: None,
        }
    }
}

/// Where a password actually lives.
///
/// The config file never holds the secret itself. It holds a reference
/// to wherever the secret is kept — see UI-REQS §2.4 for the storage
/// policy.
///
/// Internally tagged so the TOML reads naturally:
/// `password = { kind = "keyring", service = "wyldware", account = "willow" }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum PasswordRef {
    /// No password is stored for this account.
    #[default]
    None,
    /// The password lives in the OS keyring under this service/account
    /// pair. The config stores only the handle.
    Keyring {
        /// The keyring service name, e.g. "wyldware".
        service: String,
        /// The keyring account name, e.g. "willow".
        account: String,
    },
    /// No keyring was reachable, so the password is in the config file
    /// obscured — a reversible transform, not encryption. The UI must
    /// tell the user this every time.
    Obscured {
        /// The obscured password.
        value: String,
    },
}

/// A credential set against one world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Foreign key to [`World::name`].
    pub world: String,
    /// User-visible label, e.g. "Willow on Wyldware" or "main alt".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The login name.
    pub login: String,
    /// Where the password lives.
    #[serde(default)]
    pub password: PasswordRef,
    /// Character name, for MUSH-style post-login selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
    /// Connect with this account without asking.
    #[serde(default)]
    pub autoconnect: bool,
    /// Position within the world's account list.
    #[serde(default)]
    pub order: i32,
}

impl Account {
    /// A minimal account record with no stored password.
    pub fn new(world: impl Into<String>, login: impl Into<String>) -> Self {
        Self {
            world: world.into(),
            label: None,
            login: login.into(),
            password: PasswordRef::None,
            character: None,
            autoconnect: false,
            order: 0,
        }
    }
}

/// The whole configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// The user's worlds, sorted by [`World::order`].
    #[serde(default)]
    pub worlds: Vec<World>,
    /// The user's accounts, grouped by their world.
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// The world to open at startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_world: Option<String>,
    /// The theme to use when a world does not override it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_theme: Option<String>,
}

impl Config {
    /// Load from the default platform location, or return empty if the
    /// file does not exist.
    ///
    /// A malformed file is an error rather than a silent reset: losing
    /// a user's worlds because of a typo would be worse than refusing
    /// to start.
    pub fn load() -> Result<Self> {
        Self::load_from(&config_path()?)
    }

    /// Load from an explicit path.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(path)?;
        toml::from_str(&s).map_err(|e| PuebloError::Profile(e.to_string()))
    }

    /// Write to the default platform location, creating parent
    /// directories as needed.
    pub fn save(&self) -> Result<()> {
        self.save_to(&config_path()?)
    }

    /// Write to an explicit path.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self)
            .map_err(|e| PuebloError::Profile(e.to_string()))?;
        std::fs::write(path, s)?;
        Ok(())
    }

    /// The worlds in display order: by `order`, then by name.
    pub fn sorted_worlds(&self) -> Vec<&World> {
        let mut out: Vec<&World> = self.worlds.iter().collect();
        out.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
        out
    }

    /// The accounts belonging to `world`, in display order.
    pub fn accounts_for(&self, world: &str) -> Vec<&Account> {
        let mut out: Vec<&Account> = self.accounts.iter().filter(|a| a.world == world).collect();
        out.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.login.cmp(&b.login)));
        out
    }

    /// Find a world by name.
    pub fn world(&self, name: &str) -> Option<&World> {
        self.worlds.iter().find(|w| w.name == name)
    }

    /// Find an account by world and login.
    pub fn account(&self, world: &str, login: &str) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|a| a.world == world && a.login == login)
    }

    /// Renumber the `order` fields of the worlds to match the given
    /// sequence, so the user's arrangement is preserved exactly.
    pub fn apply_world_order<I: IntoIterator<Item = String>>(&mut self, names: I) {
        for (i, name) in names.into_iter().enumerate() {
            if let Some(w) = self.worlds.iter_mut().find(|w| w.name == name) {
                w.order = i as i32;
            }
        }
    }

    /// Renumber the `order` fields of one world's accounts.
    pub fn apply_account_order<I: IntoIterator<Item = String>>(
        &mut self,
        world: &str,
        logins: I,
    ) {
        for (i, login) in logins.into_iter().enumerate() {
            if let Some(a) = self
                .accounts
                .iter_mut()
                .find(|a| a.world == world && a.login == login)
            {
                a.order = i as i32;
            }
        }
    }

    /// Add a world, keeping the `order` field contiguous.
    pub fn add_world(&mut self, world: World) {
        let mut world = world;
        world.order = self.worlds.len() as i32;
        self.worlds.push(world);
    }

    /// Add an account to a world, keeping that world's `order` fields
    /// contiguous.
    pub fn add_account(&mut self, account: Account) {
        let mut account = account;
        account.order = self.accounts_for(&account.world).len() as i32;
        self.accounts.push(account);
    }

    /// Remove a world and every account belonging to it.
    ///
    /// Accounts are scoped to a world, so removing the world removes
    /// them too — leaving them behind would produce orphans that the
    /// account list could never show.
    pub fn remove_world(&mut self, name: &str) {
        self.worlds.retain(|w| w.name != name);
        self.accounts.retain(|a| a.world != name);
        if self.last_world.as_deref() == Some(name) {
            self.last_world = None;
        }
    }
}

/// The platform's config directory for Mist.
///
/// See `profile::config_path` for the platform-by-platform reasoning —
/// this returns the directory rather than the file so callers can put
/// more than one artefact in it.
pub fn config_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("com.wyldware", "Wyldware", "mist")
        .ok_or_else(|| PuebloError::Profile("no platform config dir".into()))?;
    Ok(dirs.config_dir().to_path_buf())
}

/// The path of the configuration file.
pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config {
        let mut c = Config::default();
        c.add_world(World::new("Wyldware", "wyldware.com", 2020));
        c.add_world(World::new("Aardvark", "aardvark.example", 23));
        c.add_account(Account::new("Wyldware", "willow"));
        c.add_account(Account::new("Aardvark", "willow"));
        c
    }

    #[test]
    fn worlds_sort_by_order_then_name() {
        let c = sample();
        let names: Vec<&str> = c.sorted_worlds().iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["Wyldware", "Aardvark"]);
    }

    #[test]
    fn accounts_are_scoped_to_a_world() {
        let c = sample();
        assert_eq!(c.accounts_for("Wyldware").len(), 1);
        assert_eq!(c.accounts_for("Aardvark").len(), 1);
        assert_eq!(c.accounts_for("Nowhere").len(), 0);
    }

    #[test]
    fn removing_a_world_removes_its_accounts() {
        let mut c = sample();
        c.remove_world("Wyldware");
        assert!(c.world("Wyldware").is_none());
        assert!(c.accounts_for("Wyldware").is_empty());
        assert_eq!(c.accounts.len(), 1);
    }

    #[test]
    fn reordering_worlds_rewrites_order_fields() {
        let mut c = sample();
        c.apply_world_order(["Aardvark".to_string(), "Wyldware".to_string()]);
        let names: Vec<&str> = c.sorted_worlds().iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["Aardvark", "Wyldware"]);
    }

    #[test]
    fn config_round_trips_through_toml() {
        let c = sample();
        let s = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        // A minimal world record — the shape a hand-written config
        // would have — must load without complaint.
        let s = r#"
[[worlds]]
name = "Minimal"
host = "example.com"
port = 23
"#;
        let c: Config = toml::from_str(s).unwrap();
        let w = c.world("Minimal").unwrap();
        assert_eq!(w.theme_mode, ThemeMode::AnsiPriority);
        assert_eq!(w.encoding, Encoding::Utf8);
        assert!(w.mccp);
        assert!(w.naws);
        assert!(!w.login_autodetect);
    }

    #[test]
    fn password_ref_is_a_tagged_enum() {
        let s = r#"
[[accounts]]
world = "W"
login = "l"
password = { kind = "keyring", service = "wyldware", account = "willow" }
"#;
        let c: Config = toml::from_str(s).unwrap();
        match &c.accounts[0].password {
            PasswordRef::Keyring { service, account } => {
                assert_eq!(service, "wyldware");
                assert_eq!(account, "willow");
            }
            other => panic!("expected a keyring ref, got {other:?}"),
        }
    }

    #[test]
    fn theme_mode_maps_onto_the_theme_precedence() {
        assert_eq!(
            ThemeMode::AnsiPriority.to_precedence(),
            crate::theme::Precedence::AnsiPriority
        );
        assert_eq!(
            ThemeMode::ThemePriority.to_precedence(),
            crate::theme::Precedence::ThemePriority
        );
    }
}
