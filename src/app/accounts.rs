//! Account switcher: saved accounts, switching, adding and removing.
//!
//! Switching is in place (no restart): the old session is torn down like a
//! logout (gateway, voice, chat state, synced settings, the SQLite cache),
//! but its token stays saved; then the other account's token is read from
//! the keyring and validated like a restored session. Gateway events and
//! token validations carry a generation, so a late event of the old
//! account never lands in the new one.

use iced::Task;

use super::{App, Done, Message, View, composer, gifs, settings, voice};
use crate::auth::accounts::{self, Accounts, Upsert};
use crate::auth::secrets::{self, Secret};
use crate::auth::{AuthEvent, AuthState, QrState};
use crate::net::ConnectionState;
use crate::net::model::{User, UserId};
use crate::net::rest::RestClient;

impl App {
    /// Reads the saved list (`pref.accounts`).
    pub(super) fn load_accounts(&mut self) {
        let json = match self.store.meta(accounts::META_KEY) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!(error = %e, "could not read saved accounts");
                None
            }
        };
        self.accounts = Accounts::parse(json.as_deref());
    }

    fn save_accounts(&self) -> Task<Message> {
        if self.cfg.mock {
            return Task::none();
        }
        let store = self.store.clone();
        let json = self.accounts.to_json();
        Task::perform(
            async move {
                store
                    .blocking(move |s| s.set_meta(accounts::META_KEY, &json))
                    .await
            },
            |r| {
                if let Err(e) = r {
                    tracing::warn!(error = %e, "could not save the account list");
                }
                Message::Done(Done::Ignored)
            },
        )
    }

    /// A token was validated as `user`: it is the active account now.
    /// Saves the token as the session and, if the list has room, as this
    /// account's.
    pub(super) fn remember_account(&mut self, user: &User) -> Task<Message> {
        self.active_account = Some(user.id);
        let up = self.accounts.upsert(user);
        let save_list = match up {
            Upsert::Added | Upsert::Updated { changed: true } => self.save_accounts(),
            Upsert::Updated { changed: false } | Upsert::Full => Task::none(),
        };
        if up == Upsert::Full {
            self.toast = Some(format!(
                "You can switch between up to {} accounts: remove one to remember this one too.",
                accounts::MAX_ACCOUNTS
            ));
        }
        let (Some(t), false) = (self.token.clone(), self.cfg.mock) else {
            return save_list;
        };
        let id = (up != Upsert::Full).then_some(user.id);
        let s = self.secrets.clone();
        Task::batch([
            save_list,
            Task::perform(secrets::save_session(s, t, id), |r| {
                Message::Done(Done::TokenSaved(r.map_err(|e| e.user_message())))
            }),
        ])
    }

    /// READY brought fresh names / avatar for the active account.
    pub(super) fn refresh_account(&mut self, me: &User) -> Task<Message> {
        if self.accounts.contains(me.id)
            && self.accounts.upsert(me) == (Upsert::Updated { changed: true })
        {
            return self.save_accounts();
        }
        Task::none()
    }

    /// Tears down everything that belongs to the signed-in account (logout,
    /// switch, add account). Tokens are not touched here.
    pub(super) fn reset_session(&mut self) -> Task<Message> {
        self.settings_open = false;
        self.status_menu = false;
        // Account settings belong to the account; local ones stay.
        self.settings = settings::SettingsState::new(self.settings.local.clone());
        self.gateway.set_custom_status(None);
        self.apply_settings();
        let me = self.chat.me_id();
        let _ = self.voice.leave(me);
        self.voice = voice::Voice::with_prefs(self.voice.prefs.clone());
        let video = if self.video.is_some() {
            self.update_ui(super::Ui::CloseVideo)
        } else {
            Task::none()
        };
        for (_, h) in self.upload_handles.drain() {
            h.abort();
        }
        self.uploads.clear();
        self.draft.clear();
        self.token = None;
        self.active_account = None;
        self.gateway_generation += 1;
        self.validation_generation += 1;
        self.connection = ConnectionState::Disconnected;
        self.chat = Default::default();
        self.view = View::Home(None);
        self.composer = composer::Composer::new();
        self.drafts = Default::default();
        self.last_typing_sent.clear();
        self.revealed_spoilers.clear();
        self.reply = None;
        self.editing = None;
        self.pending_delete = None;
        self.menu = None;
        self.ctx_menu = None;
        self.emoji_picker = None;
        self.gif_picker = None;
        self.gif_favorites = gifs::Favorites::default();
        self.gif_prefetched = false;
        self.sticker_picker = None;
        self.search = Default::default();
        self.panels = Default::default();
        self.server_dialog = None;
        self.discover = None;
        self.rail = Default::default();
        self.pending_select = None;
        self.session_id.clear();
        self.highlight = None;
        self.pending_link = None;
        self.lightbox = None;
        self.profiles = Default::default();
        self.qr = None;
        self.web_login = None;
        self.token_input.clear();
        if self.cfg.mock {
            return video;
        }
        if let Ok(r) = RestClient::new(&self.cfg.api_url, &self.cfg.cdn_url, None) {
            self.rest = r;
        }
        // The cache holds the old account's servers and messages.
        let store = self.store.clone();
        Task::batch([
            video,
            Task::perform(async move { store.blocking(|s| s.clear()).await }, |_| {
                Message::Done(Done::Ignored)
            }),
        ])
    }

    /// "Log Out": the active account leaves the switcher and the keyring.
    pub(super) fn forget_active_account(&mut self, id: Option<UserId>) -> Task<Message> {
        if let Some(id) = id {
            self.accounts.remove(id);
        }
        let save_list = self.save_accounts();
        if self.cfg.mock {
            return save_list;
        }
        let s = self.secrets.clone();
        let forget = match id {
            Some(id) => Task::perform(secrets::forget_account(s, id, true), |_| {
                Message::Done(Done::Ignored)
            }),
            None => Task::perform(secrets::clear_token(s), |_| Message::Done(Done::Ignored)),
        };
        Task::batch([save_list, forget])
    }

    /// Shows the login screen with a fresh QR code.
    fn show_login(&mut self) -> Task<Message> {
        self.auth = AuthState::LoggedOut { error: None };
        if self.fonts_ready {
            self.qr_generation += 1;
            self.qr = Some(if self.cfg.mock {
                super::qr_ready("https://discord.com/ra/mock-fingerprint-not-real")
            } else {
                QrState::Connecting
            });
        }
        self.request_images()
    }

    /// "Add an account": back to the login screen. The current account
    /// stays saved (and stays the restored one until another logs in).
    pub(super) fn add_account(&mut self) -> Task<Message> {
        let reset = self.reset_session();
        Task::batch([reset, self.show_login()])
    }

    pub(super) fn switch_account(&mut self, id: UserId) -> Task<Message> {
        self.status_menu = false;
        if self.active_account == Some(id) && !matches!(self.auth, AuthState::LoggedOut { .. }) {
            return Task::none();
        }
        if self.cfg.mock {
            // No keyring in mock mode: pretend the account logged in.
            let user = self.accounts.get(id).map(|a| a.user());
            let reset = self.reset_session();
            if let Some(user) = user {
                self.boot_mock_as(user);
            }
            return reset;
        }
        let s = self.secrets.clone();
        Task::perform(secrets::load_account_token(s, id), move |r| {
            Message::Done(Done::AccountToken {
                id,
                result: r.map_err(|e| e.user_message()),
            })
        })
    }

    pub(super) fn on_account_token(
        &mut self,
        id: UserId,
        result: Result<Option<Secret>, String>,
    ) -> Task<Message> {
        match result {
            Ok(Some(token)) => {
                let reset = self.reset_session();
                self.auth = AuthState::LoggedOut { error: None }.apply(AuthEvent::Submitted);
                self.active_account = Some(id);
                Task::batch([reset, self.start_gateway_with(token)])
            }
            Ok(None) => {
                // The list and the keyring disagree (keyring wiped by hand).
                self.accounts.remove(id);
                self.toast = Some("That account's saved login is gone: log in to it again.".into());
                self.save_accounts()
            }
            Err(e) => {
                self.toast = Some(format!("Could not switch accounts: {e}"));
                Task::none()
            }
        }
    }

    /// The "x" next to a saved account.
    pub(super) fn remove_account(&mut self, id: UserId) -> Task<Message> {
        if self.active_account == Some(id) && !matches!(self.auth, AuthState::LoggedOut { .. }) {
            return self.logout();
        }
        if !self.accounts.remove(id) {
            return Task::none();
        }
        let save_list = self.save_accounts();
        if self.cfg.mock {
            return save_list;
        }
        let s = self.secrets.clone();
        Task::batch([
            save_list,
            Task::perform(secrets::forget_account(s, id, false), |_| {
                Message::Done(Done::Ignored)
            }),
        ])
    }

    /// Avatars of the saved accounts, while the switcher shows them.
    pub(super) fn account_avatar_urls(&self) -> Vec<String> {
        if !(self.status_menu || matches!(self.auth, AuthState::LoggedOut { .. })) {
            return Vec::new();
        }
        self.accounts
            .list
            .iter()
            .filter_map(|a| self.rest.avatar_url(&a.user(), super::cosmetics::AV_LIST))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::mock;
    use crate::net::Event;
    use crate::net::model::Snowflake;

    fn app() -> App {
        App::boot(Config::for_tests()).0
    }

    const ALT: UserId = Snowflake(9001);

    #[test]
    fn mock_boot_has_saved_accounts() {
        let a = app();
        assert_eq!(a.accounts.len(), 3);
        assert_eq!(a.active_account, Some(mock::user(1).id));
    }

    #[test]
    fn switch_replaces_the_session() {
        let mut a = app();
        a.composer.set_text("half-written secret");
        a.status_menu = true;
        let _ = a.update(Message::Ui(super::super::Ui::SwitchAccount(ALT)));
        assert_eq!(a.active_account, Some(ALT));
        assert_eq!(a.chat.me_id(), Some(ALT));
        assert!(a.auth.is_logged_in());
        assert!(!a.status_menu);
        assert!(a.composer.text().trim().is_empty());
        // Both accounts stay saved.
        assert_eq!(a.accounts.len(), 3);
    }

    #[test]
    fn switching_to_the_active_account_does_nothing() {
        let mut a = app();
        let generation = a.gateway_generation;
        let _ = a.switch_account(mock::user(1).id);
        assert_eq!(a.gateway_generation, generation);
    }

    #[test]
    fn keyring_token_starts_a_validation() {
        let mut a = app();
        let _ = a.on_account_token(ALT, Ok(Some(Secret::new("a.b.c"))));
        assert_eq!(a.auth, AuthState::Validating);
        assert_eq!(a.active_account, Some(ALT));
        assert!(a.chat.guilds.is_empty(), "old account's servers are gone");
        assert_eq!(a.token.as_ref().map(Secret::expose), Some("a.b.c"));
    }

    #[test]
    fn missing_keyring_entry_drops_the_account() {
        let mut a = app();
        let _ = a.on_account_token(ALT, Ok(None));
        assert!(!a.accounts.contains(ALT));
        assert!(a.toast.is_some());
        // Still signed in to the current one.
        assert!(a.auth.is_logged_in());
    }

    #[test]
    fn add_account_keeps_the_saved_ones() {
        let mut a = app();
        let _ = a.update(Message::Ui(super::super::Ui::AddAccount));
        assert!(matches!(a.auth, AuthState::LoggedOut { error: None }));
        assert_eq!(a.accounts.len(), 3);
        assert!(a.chat.me.is_none());
        assert_eq!(a.active_account, None);
    }

    #[test]
    fn remove_inactive_and_active() {
        let mut a = app();
        let _ = a.update(Message::Ui(super::super::Ui::RemoveAccount(ALT)));
        assert!(!a.accounts.contains(ALT));
        assert!(a.auth.is_logged_in());
        // The "x" on the active account is a logout.
        let me = mock::user(1).id;
        let _ = a.remove_account(me);
        assert!(!a.accounts.contains(me));
        assert!(matches!(a.auth, AuthState::LoggedOut { .. }));
    }

    #[test]
    fn logout_forgets_only_the_active_account() {
        let mut a = app();
        let _ = a.update(Message::Ui(super::super::Ui::Logout));
        assert_eq!(a.accounts.len(), 2);
        assert!(!a.accounts.contains(mock::user(1).id));
    }

    #[test]
    fn stale_gateway_events_are_dropped() {
        let mut a = app();
        let old = a.gateway_generation;
        let _ = a.add_account();
        let _ = a.update(Message::Gateway(old, Event::Ready(Box::new(mock::ready()))));
        assert!(a.chat.guilds.is_empty());
        assert!(matches!(a.auth, AuthState::LoggedOut { .. }));
        // The current connection's events still land.
        let now = a.gateway_generation;
        let _ = a.update(Message::Gateway(now, Event::Ready(Box::new(mock::ready()))));
        assert!(!a.chat.guilds.is_empty());
    }

    #[test]
    fn stale_validation_is_dropped() {
        let mut a = app();
        let old = a.validation_generation;
        let _ = a.on_account_token(ALT, Ok(Some(Secret::new("a.b.c"))));
        let _ = a.update(Message::Done(Done::TokenValidated {
            result: Ok(mock::user(1)),
            revoked: false,
            generation: old,
        }));
        assert_eq!(a.auth, AuthState::Validating);
        // A revoke from the old account never logs the new one out.
        let _ = a.update(Message::Done(Done::TokenValidated {
            result: Err("revoked".into()),
            revoked: true,
            generation: old,
        }));
        assert_eq!(a.auth, AuthState::Validating);
        assert_eq!(a.accounts.len(), 3);
    }

    #[test]
    fn validation_remembers_the_account() {
        let mut a = app();
        let _ = a.add_account();
        let mut u = mock::user(4);
        u.id = Snowflake(777);
        let _ = a.on_account_token(u.id, Ok(Some(Secret::new("x.y.z"))));
        let generation = a.validation_generation;
        let _ = a.update(Message::Done(Done::TokenValidated {
            result: Ok(u.clone()),
            revoked: false,
            generation,
        }));
        assert!(a.auth.is_logged_in());
        assert!(a.accounts.contains(u.id));
        assert_eq!(a.active_account, Some(u.id));
    }
}
