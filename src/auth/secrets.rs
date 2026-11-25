//! Secret storage. This is the ONLY module allowed to call the `keyring` crate.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use secrecy::{ExposeSecret, SecretString};

use crate::config::APP_ID;
use crate::error::{Error, Result};

/// Keyring account key under which the user token is stored.
pub const TOKEN_KEY: &str = "session";

/// A secret value. Zeroised on drop; `Debug` prints `[REDACTED]`.
#[derive(Clone)]
pub struct Secret(SecretString);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::from(value.into()))
    }

    /// Exposes the secret. Callers must not log or persist the result.
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}

/// Abstraction over the OS keyring so tests and `--mock` mode never touch
/// the real Secret Service.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Secret>>;
    fn set(&self, key: &str, value: &Secret) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

/// OS keyring: Secret Service on Linux (KWallet/GNOME Keyring), Keychain on
/// macOS, Credential Manager on Windows.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringStore;

fn keyring_err(e: keyring::Error) -> Error {
    // keyring errors describe the platform failure; they never include the
    // secret itself.
    Error::Keyring(match e {
        keyring::Error::NoDefaultStore => "no Secret Service / keychain is available".into(),
        keyring::Error::NoStorageAccess(_) => "access to the keyring was denied".into(),
        keyring::Error::PlatformFailure(_) => "the keyring reported a platform failure".into(),
        keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(..) => {
            "the stored secret has an invalid encoding".into()
        }
        other => other.to_string(),
    })
}

impl KeyringStore {
    fn entry(key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(APP_ID, key).map_err(keyring_err)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<Secret>> {
        match Self::entry(key)?.get_password() {
            Ok(v) => Ok(Some(Secret::new(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_err(e)),
        }
    }

    fn set(&self, key: &str, value: &Secret) -> Result<()> {
        Self::entry(key)?
            .set_password(value.expose())
            .map_err(keyring_err)
    }

    fn delete(&self, key: &str) -> Result<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_err(e)),
        }
    }
}

/// In-memory secret store for tests and `--mock` mode.
#[derive(Default, Clone)]
pub struct MemoryStore(Arc<Mutex<HashMap<String, Secret>>>);

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<Secret>> {
        let map = self
            .0
            .lock()
            .map_err(|_| Error::Keyring("memory store poisoned".into()))?;
        Ok(map.get(key).cloned())
    }

    fn set(&self, key: &str, value: &Secret) -> Result<()> {
        let mut map = self
            .0
            .lock()
            .map_err(|_| Error::Keyring("memory store poisoned".into()))?;
        map.insert(key.to_owned(), value.clone());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        let mut map = self
            .0
            .lock()
            .map_err(|_| Error::Keyring("memory store poisoned".into()))?;
        map.remove(key);
        Ok(())
    }
}

/// Runs a blocking keyring call off the UI thread.
pub async fn load_token(store: Arc<dyn SecretStore>) -> Result<Option<Secret>> {
    tokio::task::spawn_blocking(move || store.get(TOKEN_KEY))
        .await
        .map_err(|_| Error::Cancelled)?
}

pub async fn save_token(store: Arc<dyn SecretStore>, token: Secret) -> Result<()> {
    tokio::task::spawn_blocking(move || store.set(TOKEN_KEY, &token))
        .await
        .map_err(|_| Error::Cancelled)?
}

pub async fn clear_token(store: Arc<dyn SecretStore>) -> Result<()> {
    tokio::task::spawn_blocking(move || store.delete(TOKEN_KEY))
        .await
        .map_err(|_| Error::Cancelled)?
}

/// Token of one saved account (account switcher).
pub async fn load_account_token(
    store: Arc<dyn SecretStore>,
    id: crate::net::model::UserId,
) -> Result<Option<Secret>> {
    let key = super::accounts::token_key(id);
    tokio::task::spawn_blocking(move || store.get(&key))
        .await
        .map_err(|_| Error::Cancelled)?
}

/// Saves the active token (`session`) and, with `id`, the same token as
/// that saved account's.
pub async fn save_session(
    store: Arc<dyn SecretStore>,
    token: Secret,
    id: Option<crate::net::model::UserId>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        store.set(TOKEN_KEY, &token)?;
        match id {
            Some(id) => store.set(&super::accounts::token_key(id), &token),
            None => Ok(()),
        }
    })
    .await
    .map_err(|_| Error::Cancelled)?
}

/// Forgets a saved account's token; with `active`, the `session` entry too.
pub async fn forget_account(
    store: Arc<dyn SecretStore>,
    id: crate::net::model::UserId,
    active: bool,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        store.delete(&super::accounts::token_key(id))?;
        if active {
            store.delete(TOKEN_KEY)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| Error::Cancelled)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let s = Secret::new("super-secret-token");
        assert_eq!(format!("{s:?}"), "[REDACTED]");
    }

    #[test]
    fn memory_store_roundtrip() {
        let store = MemoryStore::new();
        assert!(store.get(TOKEN_KEY).unwrap().is_none());
        store.set(TOKEN_KEY, &Secret::new("abc")).unwrap();
        assert_eq!(store.get(TOKEN_KEY).unwrap().unwrap().expose(), "abc");
        store.delete(TOKEN_KEY).unwrap();
        assert!(store.get(TOKEN_KEY).unwrap().is_none());
        // Deleting a missing entry is not an error.
        store.delete(TOKEN_KEY).unwrap();
    }

    #[tokio::test]
    async fn async_helpers_use_blocking_pool() {
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
        save_token(store.clone(), Secret::new("t0k")).await.unwrap();
        assert_eq!(
            load_token(store.clone()).await.unwrap().unwrap().expose(),
            "t0k"
        );
        clear_token(store.clone()).await.unwrap();
        assert!(load_token(store).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn account_tokens_are_separate_from_the_session() {
        use crate::net::model::Snowflake;
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
        save_session(store.clone(), Secret::new("a.a.a"), Some(Snowflake(1)))
            .await
            .unwrap();
        save_session(store.clone(), Secret::new("b.b.b"), Some(Snowflake(2)))
            .await
            .unwrap();
        // The last login is the active session; both accounts keep theirs.
        assert_eq!(
            load_token(store.clone()).await.unwrap().unwrap().expose(),
            "b.b.b"
        );
        let a = load_account_token(store.clone(), Snowflake(1))
            .await
            .unwrap();
        assert_eq!(a.unwrap().expose(), "a.a.a");
        // Removing an inactive account leaves the session alone.
        forget_account(store.clone(), Snowflake(1), false)
            .await
            .unwrap();
        assert!(
            load_account_token(store.clone(), Snowflake(1))
                .await
                .unwrap()
                .is_none()
        );
        assert!(load_token(store.clone()).await.unwrap().is_some());
        // Removing the active one logs it out.
        forget_account(store.clone(), Snowflake(2), true)
            .await
            .unwrap();
        assert!(load_token(store.clone()).await.unwrap().is_none());
        assert!(
            load_account_token(store, Snowflake(2))
                .await
                .unwrap()
                .is_none()
        );
    }
}
