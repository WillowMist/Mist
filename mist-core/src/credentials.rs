//! Credential storage — the OS keyring, with an obscured-plaintext
//! fallback.
//!
//! UI-REQS §2.4 locks the policy:
//!
//! * Primary: the `keyring` crate — Secret Service on Linux, Keychain
//!   on macOS, Credential Manager on Windows. The config stores only an
//!   opaque handle.
//! * Fallback: if no keyring service is reachable (common on headless
//!   servers and minimal Linux installs), the password is written to the
//!   config **obscured, not encrypted** — a reversible transform, so it
//!   is not sitting in plain sight in `cat config.toml`. We do not claim
//!   it is secure.
//! * Never silently downgrade. The user is told which mode is in
//!   effect, every time, in the account editor.
//!
//! The obscured transform is deliberately simple and self-describing so
//! a user who finds it in their config file knows exactly what it is:
//! the password, XORed with a fixed key and base64-encoded. Anyone with
//! the source can recover it. That is the point — it stops shoulder
//! surfing and `grep`, not a determined attacker.

use crate::config::PasswordRef;
use crate::error::{PuebloError, Result};

/// The service name Mist registers under in the OS keyring.
pub const KEYRING_SERVICE: &str = "com.wyldware.mist";

/// Where a password ended up being stored.
///
/// The UI shows this to the user verbatim — the difference between
/// "stored in your system keyring" and "stored obscured in the config
/// file" matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageMode {
    /// The OS keyring holds the secret. Nothing sensitive is in the
    /// config file.
    Keyring,
    /// No keyring was reachable; the config holds an obscured copy.
    ObscuredFallback,
}

impl StorageMode {
    /// A short human-readable description, for the account editor.
    pub fn describe(self) -> &'static str {
        match self {
            StorageMode::Keyring => {
                "Password stored in your system keyring."
            }
            StorageMode::ObscuredFallback => {
                "No system keyring available; password stored obscured in the \
                 config file. Do not share this file."
            }
        }
    }
}

/// A fixed XOR key for the obscured fallback.
///
/// This is deliberately not a secret. Its job is to make the stored
/// value not look like a password at a glance.
const OBSCURE_KEY: &[u8] = b"mist-obscured-password-v1";

/// Obfuscate a password for the config-file fallback.
///
/// The transform is XOR with a fixed key, then base64. It is
/// reversible, and the doc comment says so plainly — we do not want a
/// user believing their password is encrypted when it is not.
pub fn obscure(password: &str) -> String {
    use base64::Engine as _;
    let bytes: Vec<u8> = password
        .as_bytes()
        .iter()
        .enumerate()
        .map(|(i, &b)| b ^ OBSCURE_KEY[i % OBSCURE_KEY.len()])
        .collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Recover a password obscured by [`obscure`].
pub fn reveal(obscured: &str) -> Result<String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(obscured)
        .map_err(|e| PuebloError::Profile(format!("obscured password is not valid base64: {e}")))?;
    let plain: Vec<u8> = bytes
        .iter()
        .enumerate()
        .map(|(i, &b)| b ^ OBSCURE_KEY[i % OBSCURE_KEY.len()])
        .collect();
    String::from_utf8(plain)
        .map_err(|e| PuebloError::Profile(format!("obscured password is not valid UTF-8: {e}")))
}

/// Is an OS keyring reachable on this machine?
///
/// This probes the keyring rather than assuming. On Linux it asks the
/// Secret Service; on macOS and Windows the native store is always
/// present, so those report `true` without a round trip.
pub fn keyring_available() -> bool {
    keyring::Entry::new(KEYRING_SERVICE, "probe").is_ok()
}

/// Store `password` for `world`/`login`, preferring the OS keyring and
/// falling back to the obscured config form.
///
/// Returns the reference to record in the config, and which mode was
/// used so the caller can tell the user.
pub fn store(
    password: &str,
    world: &str,
    login: &str,
) -> Result<(PasswordRef, StorageMode)> {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, &account_key(world, login)) {
        if entry.set_password(password).is_ok() {
            return Ok((
                PasswordRef::Keyring {
                    service: KEYRING_SERVICE.to_string(),
                    account: account_key(world, login),
                },
                StorageMode::Keyring,
            ));
        }
    }

    // No keyring, or the keyring refused the write. Fall back, but the
    // caller must surface that this happened.
    Ok((
        PasswordRef::Obscured {
            value: obscure(password),
        },
        StorageMode::ObscuredFallback,
    ))
}

/// Retrieve a password from wherever it is stored.
///
/// Returns `Ok(None)` when the account has no password.
pub fn retrieve(reference: &PasswordRef) -> Result<Option<String>> {
    match reference {
        PasswordRef::None => Ok(None),
        PasswordRef::Obscured { value } => reveal(value).map(Some),
        PasswordRef::Keyring { service, account } => {
            let entry = keyring::Entry::new(service, account)
                .map_err(|e| PuebloError::Profile(format!("keyring unavailable: {e}")))?;
            match entry.get_password() {
                Ok(p) => Ok(Some(p)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(PuebloError::Profile(format!("keyring read failed: {e}"))),
            }
        }
    }
}

/// Delete a stored password.
///
/// Best-effort: a keyring that has already forgotten the entry is not
/// an error, and an obscured password just becomes `None`.
pub fn remove(reference: &PasswordRef) -> Result<()> {
    match reference {
        PasswordRef::None => Ok(()),
        PasswordRef::Obscured { .. } => Ok(()),
        PasswordRef::Keyring { service, account } => {
            let entry = keyring::Entry::new(service, account)
                .map_err(|e| PuebloError::Profile(format!("keyring unavailable: {e}")))?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(PuebloError::Profile(format!("keyring delete failed: {e}"))),
            }
        }
    }
}

/// The keyring account name for a world/login pair.
///
/// The world name is included so two worlds with the same login do not
/// collide in the keyring.
fn account_key(world: &str, login: &str) -> String {
    format!("{world}/{login}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obscure_is_reversible() {
        let password = "correct horse battery staple";
        let obscured = obscure(password);
        assert_ne!(obscured, password);
        assert_eq!(reveal(&obscured).unwrap(), password);
    }

    #[test]
    fn obscure_handles_empty_and_unicode() {
        for p in ["", "pässwörd", "🔑 emoji pass"] {
            assert_eq!(reveal(&obscure(p)).unwrap(), p);
        }
    }

    #[test]
    fn reveal_rejects_garbage() {
        assert!(reveal("not base64 !!!").is_err());
    }

    #[test]
    fn the_obscured_form_is_not_the_plaintext() {
        // The whole point of the fallback: `grep` on the config file
        // must not turn up the password.
        let obscured = obscure("hunter2");
        assert!(!obscured.contains("hunter2"));
    }

    #[test]
    fn account_keys_are_scoped_by_world() {
        assert_ne!(account_key("A", "b"), account_key("B", "b"));
    }

    #[test]
    fn storing_and_retrieving_round_trips_through_the_obscured_form() {
        // The keyring path may not be available in a test environment,
        // so exercise the fallback deterministically.
        let reference = PasswordRef::Obscured {
            value: obscure("s3cret"),
        };
        assert_eq!(retrieve(&reference).unwrap().as_deref(), Some("s3cret"));
    }

    #[test]
    fn retrieving_no_password_is_none() {
        assert_eq!(retrieve(&PasswordRef::None).unwrap(), None);
    }

    #[test]
    fn storage_modes_describe_themselves() {
        assert!(StorageMode::Keyring.describe().contains("keyring"));
        assert!(StorageMode::ObscuredFallback.describe().contains("obscured"));
    }
}
