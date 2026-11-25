//! Server administration: the server dropdown actions (Invite People,
//! Server Settings, Leave Server) and the Server Settings screen
//! (Overview: name and icon; Delete Server). Every action is offered only
//! when the permissions allow it (`app::permissions`), and Discord still
//! has the last word: a refusal is shown where the action was started.

use std::path::PathBuf;

use iced::Task;

use super::permissions::MANAGE_GUILD;
use super::servers::{NAME_MAX, NAME_MIN};
use super::{App, Done, Message, Ui, View};
use crate::error::Error;
use crate::net::mfa::{MfaChallenge, MfaMethod, clean_code};
use crate::net::model::*;
use crate::net::rest::guild::GuildEdit;

/// UI events of the admin dialogs and the Server Settings screen.
#[derive(Debug, Clone, PartialEq)]
pub enum AdminUi {
    /// Header chevron: opens (or closes) the server dropdown.
    ToggleMenu(GuildId),
    OpenInvite(GuildId),
    CopyInvite,
    OpenSettings(GuildId),
    CloseSettings,
    NameChanged(String),
    PickIcon,
    RemoveIcon,
    /// Drops the unsaved changes.
    Reset,
    Save,
    AskLeave(GuildId),
    ConfirmLeave,
    AskDelete,
    DeleteTyped(String),
    ConfirmDelete,
    /// The two-factor code typed in the Delete Server dialog.
    MfaCode(String),
    /// Authenticator app or backup code.
    MfaUse(MfaMethod),
    /// Closes the open confirmation / invite dialog.
    CloseDialog,
}

/// Answers of the admin REST calls.
#[derive(Debug, Clone)]
pub enum AdminDone {
    Invite {
        guild: GuildId,
        result: Result<String, String>,
    },
    Saved {
        guild: GuildId,
        name: Option<String>,
        result: Result<(), String>,
    },
    IconPicked(Option<Result<(PathBuf, String), String>>),
    Left {
        guild: GuildId,
        result: Result<(), String>,
    },
    Deleted {
        guild: GuildId,
        result: Result<(), String>,
    },
    /// Deleting needs a two-factor code first.
    DeleteNeedsMfa {
        guild: GuildId,
        challenge: Box<MfaChallenge>,
    },
}

/// A small dialog over the window.
#[derive(Debug, Clone, PartialEq)]
pub enum AdminDialog {
    Invite(InviteState),
    Leave(Confirm),
    Delete(DeleteForm),
}

#[derive(Debug, Clone, PartialEq)]
pub struct InviteState {
    pub guild: GuildId,
    /// Channel the invite points at.
    pub channel: ChannelId,
    pub code: Option<String>,
    pub error: Option<String>,
    pub copied: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Confirm {
    pub guild: GuildId,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeleteForm {
    pub guild: GuildId,
    pub typed: String,
    pub busy: bool,
    pub error: Option<String>,
    /// Discord asked for a two-factor code.
    pub mfa: Option<MfaStep>,
}

/// The two-factor step of a sensitive action.
#[derive(Debug, Clone, PartialEq)]
pub struct MfaStep {
    pub challenge: MfaChallenge,
    pub method: MfaMethod,
    pub code: String,
}

impl MfaStep {
    pub fn new(challenge: MfaChallenge) -> Self {
        let method = if challenge.allows(MfaMethod::Totp) {
            MfaMethod::Totp
        } else {
            MfaMethod::Backup
        };
        Self {
            challenge,
            method,
            code: String::new(),
        }
    }

    /// The code to send, when it looks like one.
    pub fn code(&self) -> Option<String> {
        clean_code(self.method, &self.code)
    }
}

impl DeleteForm {
    /// Like the official client: the server name has to be typed exactly.
    pub fn matches(&self, name: &str) -> bool {
        !self.typed.is_empty() && self.typed.trim() == name.trim()
    }
}

/// New icon picked in the Overview page.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum IconChange {
    #[default]
    Keep,
    Remove,
    Set {
        path: PathBuf,
        uri: String,
    },
}

/// The Server Settings screen ("Overview").
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsForm {
    pub guild: GuildId,
    /// Name when the screen opened (or after the last save).
    pub saved_name: String,
    pub name: String,
    pub icon: IconChange,
    pub busy: bool,
    pub error: Option<String>,
}

impl SettingsForm {
    pub fn new(guild: &Guild) -> Self {
        Self {
            guild: guild.id,
            saved_name: guild.name.clone(),
            name: guild.name.clone(),
            icon: IconChange::Keep,
            busy: false,
            error: None,
        }
    }

    /// What "Save Changes" sends.
    pub fn edit(&self) -> GuildEdit {
        let name = self.name.trim();
        GuildEdit {
            name: (name != self.saved_name.trim()).then(|| name.to_owned()),
            icon: match &self.icon {
                IconChange::Keep => None,
                IconChange::Remove => Some(None),
                IconChange::Set { uri, .. } => Some(Some(uri.clone())),
            },
        }
    }

    pub fn dirty(&self) -> bool {
        !self.edit().is_empty()
    }

    pub fn name_ok(&self) -> bool {
        (NAME_MIN..=NAME_MAX).contains(&self.name.trim().chars().count())
    }

    pub fn can_save(&self) -> bool {
        !self.busy && self.dirty() && self.name_ok()
    }

    pub fn reset(&mut self) {
        self.name = self.saved_name.clone();
        self.icon = IconChange::Keep;
        self.error = None;
    }
}

/// Admin state kept by the app.
#[derive(Debug, Clone, Default)]
pub struct Admin {
    pub dialog: Option<AdminDialog>,
    pub settings: Option<SettingsForm>,
    /// Page of the Server Settings screen.
    pub page: super::role_admin::SettingsPage,
    pub roles: super::role_admin::RolesPage,
    pub bans: super::bans::BansPage,
}

/// Wording for a refused admin call.
pub fn admin_error_text(e: &crate::error::Error) -> String {
    match e {
        Error::Http { status: 403, .. } => "You do not have permission to do that.".into(),
        Error::Http {
            status: 400 | 401,
            message,
        } if message.to_lowercase().contains("two factor")
            || message.to_lowercase().contains("mfa") =>
        {
            "Discord asks for two-factor authentication for this; do it in the official client."
                .into()
        }
        Error::Http { status: 429, .. } => {
            "Discord is rate limiting this. Wait a minute and try again.".into()
        }
        Error::MfaRequired(c) if !c.supported() => "Discord asks for a two-factor check this app \
             cannot do (text message, password or security key); do it in the official client."
            .into(),
        Error::MfaRequired(_) => {
            "Discord asks for two-factor authentication for this; do it in the official client."
                .into()
        }
        other => other.user_message(),
    }
}

fn admin(r: Result<impl Sized, crate::error::Error>) -> Result<(), String> {
    r.map(|_| ()).map_err(|e| admin_error_text(&e))
}

impl App {
    /// Whether the dropdown offers "Invite People" for `g`.
    pub fn can_invite(&self, g: GuildId) -> bool {
        self.chat.invite_channel(g, None).is_some()
    }

    /// Whether "Server Settings" is offered for `g`: Manage Server
    /// (Overview) or Manage Roles (Roles).
    pub fn can_open_server_settings(&self, g: GuildId) -> bool {
        self.can_manage_guild(g) || self.can_manage_roles(g) || self.can_ban(g)
    }

    /// Whether the user may edit the Overview of `g`.
    pub fn can_manage_guild(&self, g: GuildId) -> bool {
        self.chat.can_in_guild(g, MANAGE_GUILD)
    }

    pub(super) fn update_admin(&mut self, u: AdminUi) -> Task<Message> {
        match u {
            AdminUi::ToggleMenu(g) => {
                let open = matches!(
                    self.ctx_menu,
                    Some(super::context::CtxMenu {
                        target: super::context::CtxTarget::Guild(x),
                        ..
                    }) if x == g
                );
                self.close_overlays();
                if !open {
                    let m = self.theme.metrics;
                    self.ctx_menu = Some(super::context::CtxMenu {
                        target: super::context::CtxTarget::Guild(g),
                        at: iced::Point::new(m.rail_width + 8.0, m.header_height + 4.0),
                    });
                }
                Task::none()
            }
            AdminUi::OpenInvite(g) => self.open_invite(g),
            AdminUi::CopyInvite => {
                let Some(AdminDialog::Invite(i)) = self.admin.dialog.as_mut() else {
                    return Task::none();
                };
                let Some(code) = &i.code else {
                    return Task::none();
                };
                i.copied = true;
                iced::clipboard::write(crate::net::rest::guild::invite_url(code))
            }
            AdminUi::OpenSettings(g) => {
                self.close_menus();
                let Some(guild) = self.chat.guild(g) else {
                    return Task::none();
                };
                if !self.can_open_server_settings(g) {
                    return Task::none();
                }
                self.admin.settings = Some(SettingsForm::new(guild));
                self.admin.dialog = None;
                use super::role_admin::{RoleUi, SettingsPage};
                let first = if self.can_manage_guild(g) {
                    SettingsPage::Overview
                } else if self.can_manage_roles(g) {
                    SettingsPage::Roles
                } else {
                    SettingsPage::Bans
                };
                self.update_roles(RoleUi::Page(first))
            }
            AdminUi::CloseSettings => {
                self.admin.settings = None;
                self.admin.dialog = None;
                self.admin.roles = Default::default();
                self.admin.bans = Default::default();
                Task::none()
            }
            AdminUi::NameChanged(s) => {
                if let Some(f) = self.admin.settings.as_mut() {
                    f.name = s.chars().take(NAME_MAX).collect();
                    f.error = None;
                }
                Task::none()
            }
            AdminUi::PickIcon => self.pick_guild_icon(),
            AdminUi::RemoveIcon => {
                if let Some(f) = self.admin.settings.as_mut() {
                    let has_icon = self.chat.guild(f.guild).is_some_and(|g| g.icon.is_some());
                    f.icon = if has_icon {
                        IconChange::Remove
                    } else {
                        IconChange::Keep
                    };
                }
                Task::none()
            }
            AdminUi::Reset => {
                if let Some(f) = self.admin.settings.as_mut() {
                    f.reset();
                }
                Task::none()
            }
            AdminUi::Save => self.save_guild(),
            AdminUi::AskLeave(g) => {
                self.close_menus();
                if self.chat.guild(g).is_none() || self.chat.owns_guild(g) {
                    return Task::none();
                }
                self.admin.dialog = Some(AdminDialog::Leave(Confirm {
                    guild: g,
                    busy: false,
                    error: None,
                }));
                Task::none()
            }
            AdminUi::ConfirmLeave => self.leave_guild(),
            AdminUi::AskDelete => {
                let Some(f) = &self.admin.settings else {
                    return Task::none();
                };
                if !self.chat.owns_guild(f.guild) {
                    return Task::none();
                }
                self.admin.dialog = Some(AdminDialog::Delete(DeleteForm {
                    guild: f.guild,
                    typed: String::new(),
                    busy: false,
                    error: None,
                    mfa: None,
                }));
                iced::widget::operation::focus(crate::ui::server_settings::delete_input_id())
            }
            AdminUi::DeleteTyped(s) => {
                if let Some(AdminDialog::Delete(d)) = self.admin.dialog.as_mut() {
                    d.typed = s.chars().take(NAME_MAX).collect();
                    d.error = None;
                }
                Task::none()
            }
            AdminUi::ConfirmDelete => self.delete_guild(),
            AdminUi::MfaCode(s) => {
                if let Some(AdminDialog::Delete(d)) = self.admin.dialog.as_mut()
                    && let Some(m) = d.mfa.as_mut()
                {
                    m.code = s.chars().take(16).collect();
                    d.error = None;
                }
                Task::none()
            }
            AdminUi::MfaUse(method) => {
                if let Some(AdminDialog::Delete(d)) = self.admin.dialog.as_mut()
                    && let Some(m) = d.mfa.as_mut()
                    && m.challenge.allows(method)
                {
                    m.method = method;
                    m.code.clear();
                    d.error = None;
                }
                iced::widget::operation::focus(crate::ui::server_settings::mfa_input_id())
            }
            AdminUi::CloseDialog => {
                let busy = match &self.admin.dialog {
                    Some(AdminDialog::Leave(c)) => c.busy,
                    Some(AdminDialog::Delete(d)) => d.busy,
                    _ => false,
                };
                if !busy {
                    self.admin.dialog = None;
                }
                Task::none()
            }
        }
    }

    /// Closes the menus an admin action can be picked from.
    fn close_menus(&mut self) {
        self.ctx_menu = None;
        self.rail.menu = None;
    }

    fn open_invite(&mut self, g: GuildId) -> Task<Message> {
        self.close_menus();
        let pref = match self.view {
            View::Guild { guild, channel } if guild == g => channel,
            _ => None,
        };
        let Some(channel) = self.chat.invite_channel(g, pref) else {
            self.toast = Some("You cannot create invites in this server.".into());
            return Task::none();
        };
        self.admin.dialog = Some(AdminDialog::Invite(InviteState {
            guild: g,
            channel,
            code: None,
            error: None,
            copied: false,
        }));
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Admin(AdminDone::Invite {
                guild: g,
                result: Ok("ferro-mock".into()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(async move { rest.create_invite(channel).await }, move |r| {
            Message::Done(Done::Admin(AdminDone::Invite {
                guild: g,
                result: r.map_err(|e| admin_error_text(&e)),
            }))
        })
    }

    fn pick_guild_icon(&mut self) -> Task<Message> {
        if self.cfg.mock {
            return Task::none();
        }
        Task::perform(super::features::pick_icon_file(), |r| {
            Message::Done(Done::Admin(AdminDone::IconPicked(r)))
        })
    }

    fn save_guild(&mut self) -> Task<Message> {
        let Some(f) = self.admin.settings.as_mut() else {
            return Task::none();
        };
        if !f.can_save() {
            if !f.name_ok() {
                f.error = Some(format!(
                    "The server name must be {NAME_MIN} to {NAME_MAX} characters long."
                ));
            }
            return Task::none();
        }
        f.busy = true;
        f.error = None;
        let (guild, edit) = (f.guild, f.edit());
        let name = edit.name.clone();
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Admin(AdminDone::Saved {
                guild,
                name,
                result: Ok(()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.modify_guild(guild, &edit).await },
            move |r| {
                Message::Done(Done::Admin(AdminDone::Saved {
                    guild,
                    name: name.clone(),
                    result: admin(r),
                }))
            },
        )
    }

    fn leave_guild(&mut self) -> Task<Message> {
        let Some(AdminDialog::Leave(c)) = self.admin.dialog.as_mut() else {
            return Task::none();
        };
        if c.busy {
            return Task::none();
        }
        c.busy = true;
        c.error = None;
        let guild = c.guild;
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Admin(AdminDone::Left {
                guild,
                result: Ok(()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(async move { rest.leave_guild(guild).await }, move |r| {
            Message::Done(Done::Admin(AdminDone::Left {
                guild,
                result: admin(r),
            }))
        })
    }

    fn delete_guild(&mut self) -> Task<Message> {
        let Some(AdminDialog::Delete(d)) = self.admin.dialog.as_mut() else {
            return Task::none();
        };
        let name = self
            .chat
            .guild(d.guild)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        if d.busy || !d.matches(&name) {
            return Task::none();
        }
        // With a two-factor step: the code first, then the same request.
        let answer = match &d.mfa {
            Some(m) => match m.code() {
                Some(code) => Some((m.challenge.ticket.clone(), m.method, code)),
                None => {
                    d.error = Some(match m.method {
                        MfaMethod::Backup => "Enter an 8-character backup code.".into(),
                        _ => "Enter the 6-digit code from your authenticator app.".into(),
                    });
                    return Task::none();
                }
            },
            None => None,
        };
        d.busy = true;
        d.error = None;
        let guild = d.guild;
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Admin(AdminDone::Deleted {
                guild,
                result: Ok(()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(
            async move {
                match answer {
                    Some((ticket, method, code)) => {
                        let token = rest.mfa_finish(&ticket, method, &code).await?;
                        rest.delete_guild(guild, Some(&token)).await
                    }
                    None => rest.delete_guild(guild, None).await,
                }
            },
            move |r| {
                Message::Done(Done::Admin(match r {
                    Err(Error::MfaRequired(c)) if c.supported() => AdminDone::DeleteNeedsMfa {
                        guild,
                        challenge: c,
                    },
                    r => AdminDone::Deleted {
                        guild,
                        result: admin(r),
                    },
                }))
            },
        )
    }

    pub(super) fn on_admin_done(&mut self, d: AdminDone) -> Task<Message> {
        match d {
            AdminDone::DeleteNeedsMfa { guild, challenge } => {
                let Some(AdminDialog::Delete(d)) = self.admin.dialog.as_mut() else {
                    return Task::none();
                };
                if d.guild != guild {
                    return Task::none();
                }
                d.busy = false;
                // A second demand means the ticket ran out: ask again.
                d.error = d
                    .mfa
                    .is_some()
                    .then(|| "That code expired. Enter a new one.".to_owned());
                d.mfa = Some(MfaStep::new(*challenge));
                iced::widget::operation::focus(crate::ui::server_settings::mfa_input_id())
            }
            AdminDone::Invite { guild, result } => {
                if let Some(AdminDialog::Invite(i)) = self.admin.dialog.as_mut()
                    && i.guild == guild
                {
                    match result {
                        Ok(code) => i.code = Some(code),
                        Err(e) => i.error = Some(e),
                    }
                }
                Task::none()
            }
            AdminDone::IconPicked(r) => {
                if let Some(f) = self.admin.settings.as_mut() {
                    match r {
                        Some(Ok((path, uri))) => {
                            f.icon = IconChange::Set { path, uri };
                            f.error = None;
                        }
                        Some(Err(e)) => f.error = Some(e),
                        None => {}
                    }
                }
                Task::none()
            }
            AdminDone::Saved {
                guild,
                name,
                result,
            } => {
                let Some(f) = self.admin.settings.as_mut().filter(|f| f.guild == guild) else {
                    return Task::none();
                };
                f.busy = false;
                match result {
                    Ok(()) => {
                        if let Some(n) = name {
                            f.saved_name = n.clone();
                            f.name = n.clone();
                            // GUILD_UPDATE follows; show the new name now.
                            if let Some(g) = self.chat.guilds.iter_mut().find(|g| g.id == guild) {
                                g.name = n;
                            }
                        }
                        if self.cfg.mock
                            && f.icon == IconChange::Remove
                            && let Some(g) = self.chat.guilds.iter_mut().find(|g| g.id == guild)
                        {
                            g.icon = None;
                        }
                        f.icon = IconChange::Keep;
                        self.toast = Some("Server settings saved.".into());
                    }
                    Err(e) => f.error = Some(e),
                }
                Task::none()
            }
            AdminDone::Left { guild, result } | AdminDone::Deleted { guild, result } => {
                match result {
                    Ok(()) => self.guild_gone(guild),
                    Err(e) => {
                        match self.admin.dialog.as_mut() {
                            Some(AdminDialog::Leave(c)) if c.guild == guild => {
                                c.busy = false;
                                c.error = Some(e);
                            }
                            Some(AdminDialog::Delete(d)) if d.guild == guild => {
                                d.busy = false;
                                d.error = Some(e);
                            }
                            _ => self.toast = Some(e),
                        }
                        Task::none()
                    }
                }
            }
        }
    }

    /// The server was left or deleted: close everything about it. The
    /// gateway's GUILD_DELETE removes it (applied here in mock mode).
    fn guild_gone(&mut self, g: GuildId) -> Task<Message> {
        self.admin.dialog = None;
        if self.admin.settings.as_ref().is_some_and(|f| f.guild == g) {
            self.admin.settings = None;
        }
        if self.cfg.mock {
            let ctx = self.ctx();
            let _ = self
                .chat
                .apply_event(&crate::net::Event::GuildDelete(g), &ctx);
        }
        if matches!(self.view, View::Guild { guild, .. } if guild == g) {
            return self.update(Message::Ui(Ui::SelectHome));
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild(name: &str, icon: Option<&str>) -> Guild {
        Guild {
            id: Snowflake(7),
            name: name.into(),
            icon: icon.map(str::to_owned),
            roles: vec![],
            owner_id: None,
            emojis: vec![],
            stickers: vec![],
            premium_tier: 0,
        }
    }

    #[test]
    fn settings_form_tracks_changes() {
        let mut f = SettingsForm::new(&guild("Test", Some("h")));
        assert!(!f.dirty());
        assert!(!f.can_save());
        f.name = "  Test  ".into();
        assert!(!f.dirty(), "surrounding spaces are not a change");
        f.name = "Renamed".into();
        assert!(f.can_save());
        assert_eq!(f.edit().name.as_deref(), Some("Renamed"));
        assert_eq!(f.edit().icon, None);
        f.icon = IconChange::Remove;
        assert_eq!(f.edit().icon, Some(None));
        f.name = "x".into();
        assert!(!f.name_ok());
        assert!(!f.can_save());
        f.reset();
        assert!(!f.dirty());
        assert_eq!(f.name, "Test");
    }

    #[test]
    fn delete_needs_the_exact_name() {
        let mut d = DeleteForm {
            guild: Snowflake(7),
            typed: String::new(),
            busy: false,
            error: None,
            mfa: None,
        };
        assert!(!d.matches("Test"));
        d.typed = "test".into();
        assert!(!d.matches("Test"));
        d.typed = "Test ".into();
        assert!(d.matches("Test"));
    }

    #[test]
    fn error_wording() {
        use crate::error::Error;
        let e = Error::Http {
            status: 403,
            message: "Missing Permissions".into(),
        };
        assert!(admin_error_text(&e).contains("permission"));
        let e = Error::Http {
            status: 401,
            message: "Two factor is required for this operation".into(),
        };
        assert!(admin_error_text(&e).contains("two-factor"));
    }

    fn app() -> App {
        App::boot(crate::config::Config::for_tests()).0
    }

    fn admin_ui(a: &mut App, u: AdminUi) {
        let _ = a.update(Message::Ui(Ui::Admin(u)));
    }

    #[test]
    fn dropdown_offers_what_the_permissions_allow() {
        use crate::mock::{GUILD_GAMES, GUILD_RUST};
        let mut a = app();
        // Owned server: invite and settings, no leaving.
        assert!(a.can_invite(GUILD_GAMES));
        assert!(a.can_manage_guild(GUILD_GAMES));
        assert!(a.chat.owns_guild(GUILD_GAMES));
        // Member without Manage Server.
        assert!(!a.can_manage_guild(GUILD_RUST));
        admin_ui(&mut a, AdminUi::ToggleMenu(GUILD_GAMES));
        assert!(matches!(
            a.ctx_menu,
            Some(super::super::context::CtxMenu {
                target: super::super::context::CtxTarget::Guild(g),
                ..
            }) if g == GUILD_GAMES
        ));
        admin_ui(&mut a, AdminUi::ToggleMenu(GUILD_GAMES));
        assert!(a.ctx_menu.is_none());
        // Not allowed: nothing opens.
        admin_ui(&mut a, AdminUi::OpenSettings(GUILD_RUST));
        assert!(a.admin.settings.is_none());
        admin_ui(&mut a, AdminUi::AskLeave(GUILD_GAMES));
        assert!(a.admin.dialog.is_none());
    }

    #[test]
    fn rename_saves_and_updates_the_server() {
        use crate::mock::GUILD_GAMES;
        let mut a = app();
        admin_ui(&mut a, AdminUi::OpenSettings(GUILD_GAMES));
        assert!(a.admin.settings.is_some());
        admin_ui(&mut a, AdminUi::NameChanged("LAN Party".into()));
        admin_ui(&mut a, AdminUi::Save);
        assert!(a.admin.settings.as_ref().is_some_and(|f| f.busy));
        let _ = a.on_admin_done(AdminDone::Saved {
            guild: GUILD_GAMES,
            name: Some("LAN Party".into()),
            result: Ok(()),
        });
        assert_eq!(
            a.chat.guild(GUILD_GAMES).map(|g| g.name.as_str()),
            Some("LAN Party")
        );
        let f = a.admin.settings.as_ref().map(|f| (f.busy, f.dirty()));
        assert_eq!(f, Some((false, false)));
        // A refusal keeps the edit and shows why.
        admin_ui(&mut a, AdminUi::NameChanged("Other".into()));
        admin_ui(&mut a, AdminUi::Save);
        let _ = a.on_admin_done(AdminDone::Saved {
            guild: GUILD_GAMES,
            name: Some("Other".into()),
            result: Err("nope".into()),
        });
        let f = a.admin.settings.as_ref();
        assert_eq!(f.and_then(|f| f.error.as_deref()), Some("nope"));
        assert_eq!(f.map(|f| f.name.as_str()), Some("Other"));
        assert_eq!(
            a.chat.guild(GUILD_GAMES).map(|g| g.name.as_str()),
            Some("LAN Party")
        );
        // Escape closes the screen.
        let _ = a.update(Message::Ui(Ui::Escape));
        assert!(a.admin.settings.is_none());
    }

    #[test]
    fn delete_asks_for_a_two_factor_code() {
        let mut a = App::boot(crate::config::Config::for_tests()).0;
        let g = crate::mock::GUILD_GAMES;
        admin_ui(&mut a, AdminUi::OpenSettings(g));
        admin_ui(&mut a, AdminUi::AskDelete);
        admin_ui(&mut a, AdminUi::DeleteTyped("Game Night".into()));
        let c = MfaChallenge::parse(
            r#"{"code":60003,"mfa":{"ticket":"t","methods":[{"type":"totp","backup_codes_allowed":true}]}}"#,
        )
        .unwrap();
        let _ = a.on_admin_done(AdminDone::DeleteNeedsMfa {
            guild: g,
            challenge: Box::new(c.clone()),
        });
        let mfa = |a: &App| match &a.admin.dialog {
            Some(AdminDialog::Delete(d)) => d.clone(),
            _ => panic!("no delete dialog"),
        };
        assert_eq!(mfa(&a).mfa.as_ref().unwrap().method, MfaMethod::Totp);
        assert!(mfa(&a).error.is_none());
        // A short code is refused locally.
        admin_ui(&mut a, AdminUi::MfaCode("123".into()));
        admin_ui(&mut a, AdminUi::ConfirmDelete);
        assert!(!mfa(&a).busy);
        assert!(mfa(&a).error.unwrap().contains("6-digit"));
        // Backup codes switch the field.
        admin_ui(&mut a, AdminUi::MfaUse(MfaMethod::Backup));
        assert_eq!(mfa(&a).mfa.unwrap().method, MfaMethod::Backup);
        admin_ui(&mut a, AdminUi::MfaCode("abcd-1234".into()));
        assert_eq!(mfa(&a).mfa.unwrap().code().as_deref(), Some("abcd1234"));
        // A second demand means the ticket ran out.
        let _ = a.on_admin_done(AdminDone::DeleteNeedsMfa {
            guild: g,
            challenge: Box::new(c),
        });
        assert!(mfa(&a).error.unwrap().contains("expired"));
    }

    #[test]
    fn delete_needs_the_name_and_removes_the_server() {
        use crate::mock::GUILD_GAMES;
        let mut a = app();
        admin_ui(&mut a, AdminUi::OpenSettings(GUILD_GAMES));
        admin_ui(&mut a, AdminUi::AskDelete);
        admin_ui(&mut a, AdminUi::DeleteTyped("game night".into()));
        admin_ui(&mut a, AdminUi::ConfirmDelete);
        assert!(matches!(&a.admin.dialog, Some(AdminDialog::Delete(d)) if !d.busy));
        admin_ui(&mut a, AdminUi::DeleteTyped("Game Night".into()));
        admin_ui(&mut a, AdminUi::ConfirmDelete);
        assert!(matches!(&a.admin.dialog, Some(AdminDialog::Delete(d)) if d.busy));
        // Busy dialogs ignore Escape / outside clicks.
        admin_ui(&mut a, AdminUi::CloseDialog);
        assert!(a.admin.dialog.is_some());
        let _ = a.on_admin_done(AdminDone::Deleted {
            guild: GUILD_GAMES,
            result: Ok(()),
        });
        assert!(a.admin.dialog.is_none());
        assert!(a.admin.settings.is_none());
        assert!(a.chat.guild(GUILD_GAMES).is_none());
    }

    #[test]
    fn leave_confirms_and_reports_failures() {
        use crate::mock::GUILD_RUST;
        let mut a = app();
        assert!(matches!(a.view, View::Guild { guild, .. } if guild == GUILD_RUST));
        admin_ui(&mut a, AdminUi::AskLeave(GUILD_RUST));
        admin_ui(&mut a, AdminUi::ConfirmLeave);
        let _ = a.on_admin_done(AdminDone::Left {
            guild: GUILD_RUST,
            result: Err("Discord said no".into()),
        });
        assert!(matches!(
            &a.admin.dialog,
            Some(AdminDialog::Leave(c)) if !c.busy && c.error.as_deref() == Some("Discord said no")
        ));
        admin_ui(&mut a, AdminUi::ConfirmLeave);
        let _ = a.on_admin_done(AdminDone::Left {
            guild: GUILD_RUST,
            result: Ok(()),
        });
        assert!(a.admin.dialog.is_none());
        assert!(a.chat.guild(GUILD_RUST).is_none());
        assert!(matches!(a.view, View::Home(_)));
    }

    #[test]
    fn invite_opens_on_an_allowed_channel() {
        use crate::mock::{GUILD_GAMES, GUILD_RUST};
        let mut a = app();
        admin_ui(&mut a, AdminUi::OpenInvite(GUILD_RUST));
        assert!(a.admin.dialog.is_none(), "no invite permission there");
        admin_ui(&mut a, AdminUi::OpenInvite(GUILD_GAMES));
        let Some(AdminDialog::Invite(i)) = &a.admin.dialog else {
            panic!("no invite dialog");
        };
        assert_eq!(a.chat.channels[&i.channel].guild_id, Some(GUILD_GAMES));
        let _ = a.on_admin_done(AdminDone::Invite {
            guild: GUILD_GAMES,
            result: Ok("abc".into()),
        });
        admin_ui(&mut a, AdminUi::CopyInvite);
        assert!(matches!(
            &a.admin.dialog,
            Some(AdminDialog::Invite(i)) if i.copied && i.code.as_deref() == Some("abc")
        ));
    }
}
