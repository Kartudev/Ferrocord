//! Saved accounts for the account switcher.
//!
//! Each saved account's token lives in the keyring under
//! [`token_key`]; the list itself (id, names, avatar hash: what the
//! switcher shows) is not secret and lives in `meta("pref.accounts")`, so
//! it survives the cache wipe on logout and switch. The keyring `session`
//! entry keeps holding the active token, so older builds still restore it.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::net::model::{User, UserId};

/// Discord's own client keeps up to five accounts.
pub const MAX_ACCOUNTS: usize = 5;

/// `meta` key of the saved list (`pref.*` survives `Store::clear`).
pub const META_KEY: &str = "pref.accounts";

/// Keyring key of one saved account's token.
pub fn token_key(id: UserId) -> String {
    format!("account.{}", id.0)
}

/// What the switcher shows for an account. Never holds the token.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedAccount {
    pub id: UserId,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
}

impl fmt::Debug for SavedAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Names are personal data: ids only.
        f.debug_struct("SavedAccount")
            .field("id", &self.id)
            .finish()
    }
}

impl SavedAccount {
    pub fn from_user(u: &User) -> Self {
        Self {
            id: u.id,
            username: u.username.clone(),
            global_name: u.global_name.clone(),
            avatar: u.avatar.clone(),
        }
    }

    /// A `User` for the avatar and name widgets.
    pub fn user(&self) -> User {
        User {
            id: self.id,
            username: self.username.clone(),
            global_name: self.global_name.clone(),
            avatar: self.avatar.clone(),
            ..Default::default()
        }
    }
}

/// The saved accounts, in the order they were added.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Accounts {
    pub list: Vec<SavedAccount>,
}

/// Result of [`Accounts::upsert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upsert {
    /// Already saved; names/avatar refreshed if they changed.
    Updated {
        changed: bool,
    },
    Added,
    /// [`MAX_ACCOUNTS`] are saved already: not remembered.
    Full,
}

impl Accounts {
    /// Parses the stored list; a damaged value is an empty list (the
    /// tokens are still in the keyring, the user logs in again).
    pub fn parse(json: Option<&str>) -> Self {
        let list: Vec<SavedAccount> = json
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default();
        let mut out = Self::default();
        for a in list {
            if out.list.len() < MAX_ACCOUNTS && !out.contains(a.id) {
                out.list.push(a);
            }
        }
        out
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.list).unwrap_or_else(|_| "[]".into())
    }

    pub fn contains(&self, id: UserId) -> bool {
        self.list.iter().any(|a| a.id == id)
    }

    pub fn get(&self, id: UserId) -> Option<&SavedAccount> {
        self.list.iter().find(|a| a.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn upsert(&mut self, u: &User) -> Upsert {
        let fresh = SavedAccount::from_user(u);
        if let Some(a) = self.list.iter_mut().find(|a| a.id == u.id) {
            let changed = *a != fresh;
            *a = fresh;
            return Upsert::Updated { changed };
        }
        if self.list.len() >= MAX_ACCOUNTS {
            return Upsert::Full;
        }
        self.list.push(fresh);
        Upsert::Added
    }

    /// `true` if it was saved.
    pub fn remove(&mut self, id: UserId) -> bool {
        let before = self.list.len();
        self.list.retain(|a| a.id != id);
        self.list.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::model::Snowflake;

    fn user(n: u64) -> User {
        User {
            id: Snowflake(n),
            username: format!("user{n}"),
            ..Default::default()
        }
    }

    #[test]
    fn upsert_adds_updates_and_caps() {
        let mut a = Accounts::default();
        assert_eq!(a.upsert(&user(1)), Upsert::Added);
        assert_eq!(a.upsert(&user(1)), Upsert::Updated { changed: false });
        let mut renamed = user(1);
        renamed.global_name = Some("One".into());
        assert_eq!(a.upsert(&renamed), Upsert::Updated { changed: true });
        assert_eq!(
            a.get(Snowflake(1)).unwrap().global_name.as_deref(),
            Some("One")
        );
        for n in 2..=MAX_ACCOUNTS as u64 {
            assert_eq!(a.upsert(&user(n)), Upsert::Added);
        }
        assert_eq!(a.upsert(&user(99)), Upsert::Full);
        assert_eq!(a.len(), MAX_ACCOUNTS);
        // A saved one still refreshes when the list is full.
        assert_eq!(a.upsert(&user(2)), Upsert::Updated { changed: false });
    }

    #[test]
    fn remove_keeps_order() {
        let mut a = Accounts::default();
        for n in 1..=3 {
            a.upsert(&user(n));
        }
        assert!(a.remove(Snowflake(2)));
        assert!(!a.remove(Snowflake(2)));
        let ids: Vec<u64> = a.list.iter().map(|x| x.id.0).collect();
        assert_eq!(ids, [1, 3]);
    }

    #[test]
    fn json_roundtrip_and_damage() {
        let mut a = Accounts::default();
        a.upsert(&user(7));
        let json = a.to_json();
        assert!(json.contains("\"7\""), "ids stay strings: {json}");
        assert_eq!(Accounts::parse(Some(&json)), a);
        assert!(Accounts::parse(Some("not json")).is_empty());
        assert!(Accounts::parse(None).is_empty());
        // Duplicates and overflow from a hand-edited value are dropped.
        let mut many: Vec<SavedAccount> =
            (1..=8).map(|n| SavedAccount::from_user(&user(n))).collect();
        many.insert(1, SavedAccount::from_user(&user(1)));
        let parsed = Accounts::parse(serde_json::to_string(&many).ok().as_deref());
        assert_eq!(parsed.len(), MAX_ACCOUNTS);
        assert_eq!(parsed.list[1].id, Snowflake(2));
    }

    #[test]
    fn debug_hides_names() {
        let s = format!("{:?}", SavedAccount::from_user(&user(5)));
        assert!(!s.contains("user5"));
    }

    #[test]
    fn token_keys_are_per_account() {
        assert_eq!(token_key(Snowflake(42)), "account.42");
        assert_ne!(token_key(Snowflake(42)), crate::auth::secrets::TOKEN_KEY);
    }
}
