//! Roles and member moderation: the Roles page of Server Settings (create,
//! edit name / colour / permissions, delete) and the member actions of the
//! user menu (give or take roles, timeout, kick, ban). Discord's hierarchy
//! is applied before offering anything: a role can be edited, given or
//! taken only below the user's highest role, members only moderated when
//! they rank lower (the owner outranks everyone), and only permissions
//! the user has can be granted.

use iced::Task;

use super::guild_admin::admin_error_text;
use super::permissions::{self, ALL, MANAGE_ROLES};
use super::{App, Done, Message};
use crate::net::Event;
use crate::net::model::*;
use crate::net::rest::guild::{ModAction, RoleEdit};

/// Role name limit.
pub const ROLE_NAME_MAX: usize = 100;
/// Reason limit (Discord's audit log keeps 512 characters).
pub const REASON_MAX: usize = 512;

/// Role colours offered, like the official palette (0 = default).
pub const ROLE_COLORS: [u32; 20] = [
    0x1abc9c, 0x2ecc71, 0x3498db, 0x9b59b6, 0xe91e63, 0xf1c40f, 0xe67e22, 0xe74c3c, 0x95a5a6,
    0x607d8b, 0x11806a, 0x1f8b4c, 0x206694, 0x71368a, 0xad1457, 0xc27c0e, 0xa84300, 0x992d22,
    0x979c9f, 0x546e7a,
];

/// Server Settings pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsPage {
    #[default]
    Overview,
    Roles,
}

/// What a moderation dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKind {
    Timeout,
    Kick,
    Ban,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RoleUi {
    Page(SettingsPage),
    Select(RoleId),
    Create,
    Name(String),
    Color(u32),
    Permission(u64, bool),
    Reset,
    Save,
    AskDelete,
    ConfirmDelete,
    /// "Roles" in a member's menu.
    OpenMemberRoles {
        guild: GuildId,
        user: UserId,
    },
    MemberRole(RoleId, bool),
    OpenModeration {
        guild: GuildId,
        user: UserId,
        kind: ModKind,
    },
    Reason(String),
    BanHistory(u32),
    TimeoutLength(u64),
    /// `true` removes the timeout instead.
    ConfirmModeration(bool),
    CloseDialog,
}

#[derive(Debug, Clone)]
pub enum RoleDone {
    Created {
        guild: GuildId,
        result: Result<Role, String>,
    },
    Saved {
        guild: GuildId,
        role: RoleId,
        edit: RoleEdit,
        result: Result<(), String>,
    },
    Deleted {
        guild: GuildId,
        role: RoleId,
        result: Result<(), String>,
    },
    MemberRole {
        guild: GuildId,
        user: UserId,
        role: RoleId,
        on: bool,
        result: Result<(), String>,
    },
    Moderated {
        guild: GuildId,
        user: UserId,
        result: Result<String, String>,
    },
}

/// The role being edited.
#[derive(Debug, Clone, PartialEq)]
pub struct RoleForm {
    pub role: RoleId,
    /// `@everyone`: permissions only.
    pub everyone: bool,
    pub name: String,
    pub color: u32,
    pub permissions: u64,
    original: (String, u32, u64),
    pub busy: bool,
    pub error: Option<String>,
}

impl RoleForm {
    pub fn new(r: &Role, everyone: bool) -> Self {
        Self {
            role: r.id,
            everyone,
            name: r.name.clone(),
            color: r.color,
            permissions: r.permissions,
            original: (r.name.clone(), r.color, r.permissions),
            busy: false,
            error: None,
        }
    }

    pub fn edit(&self) -> RoleEdit {
        let (name, color, perms) = &self.original;
        RoleEdit {
            name: (!self.everyone && self.name.trim() != name.trim())
                .then(|| self.name.trim().to_owned()),
            color: (!self.everyone && self.color != *color).then_some(self.color),
            permissions: (self.permissions != *perms).then_some(self.permissions),
            ..Default::default()
        }
    }

    pub fn dirty(&self) -> bool {
        !self.edit().is_empty()
    }

    pub fn name_ok(&self) -> bool {
        (1..=ROLE_NAME_MAX).contains(&self.name.trim().chars().count())
    }

    pub fn can_save(&self) -> bool {
        !self.busy && self.dirty() && self.name_ok()
    }

    pub fn reset(&mut self) {
        let (name, color, perms) = self.original.clone();
        self.name = name;
        self.color = color;
        self.permissions = perms;
        self.error = None;
    }
}

/// State of the Roles page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RolesPage {
    pub form: Option<RoleForm>,
    pub creating: bool,
    /// The delete confirmation is open.
    pub confirm_delete: bool,
    pub deleting: bool,
}

/// A member dialog opened from the user menu.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberDialog {
    Roles {
        guild: GuildId,
        user: UserId,
        /// Role toggles in flight.
        pending: Vec<RoleId>,
        error: Option<String>,
    },
    Moderate(ModForm),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModForm {
    pub guild: GuildId,
    pub user: UserId,
    pub kind: ModKind,
    pub reason: String,
    pub ban_history: u32,
    pub timeout: u64,
    pub busy: bool,
    pub error: Option<String>,
}

impl ModForm {
    pub fn action(&self, remove_timeout: bool) -> ModAction {
        let reason = self.reason.trim().to_owned();
        match self.kind {
            ModKind::Kick => ModAction::Kick { reason },
            ModKind::Ban => ModAction::Ban {
                delete_seconds: self.ban_history,
                reason,
            },
            ModKind::Timeout => ModAction::Timeout {
                seconds: (!remove_timeout).then_some(self.timeout),
                reason,
            },
        }
    }
}

/// The roles of `g` in list order (highest first, `@everyone` last).
pub fn sorted_roles(g: &Guild) -> Vec<&Role> {
    let mut v: Vec<&Role> = g.roles.iter().collect();
    v.sort_by_key(|r| (r.id == g.id, std::cmp::Reverse(r.position), r.id));
    v
}

impl App {
    fn me_top(&self, g: &Guild) -> Option<i32> {
        let Some(me) = self.chat.me_id() else {
            return Some(-1);
        };
        let mine = self
            .chat
            .my_roles
            .get(&g.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        permissions::top_position(g, me, mine)
    }

    fn member_top(&self, g: &Guild, user: UserId) -> Option<i32> {
        let roles = self
            .chat
            .member_roles
            .get(&(g.id, user))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        permissions::top_position(g, user, roles)
    }

    pub fn can_manage_roles(&self, g: GuildId) -> bool {
        self.chat.can_in_guild(g, MANAGE_ROLES)
    }

    /// Whether `role` of `g` can be edited (and given / taken, except
    /// `@everyone`): Manage Roles and a higher role (`@everyone` is
    /// always below).
    pub fn can_edit_role(&self, g: GuildId, role: RoleId) -> bool {
        let Some(guild) = self.chat.guild(g) else {
            return false;
        };
        let Some(r) = guild.roles.iter().find(|r| r.id == role) else {
            return false;
        };
        self.can_manage_roles(g)
            && (role == g || permissions::outranks(self.me_top(guild), Some(r.position)))
    }

    /// Permission bits the user may grant in a role: only their own,
    /// unless they have every permission.
    pub fn grantable(&self, g: GuildId) -> u64 {
        let p = self.chat.guild_permissions(g);
        if p == ALL { ALL } else { p }
    }

    /// Whether `user` can be moderated with `bit` in `g`: the permission,
    /// not oneself, and a lower rank.
    pub fn can_moderate(&self, g: GuildId, user: UserId, bit: u64) -> bool {
        let Some(guild) = self.chat.guild(g) else {
            return false;
        };
        Some(user) != self.chat.me_id()
            && self.chat.can_in_guild(g, bit)
            && permissions::outranks(self.me_top(guild), self.member_top(guild, user))
    }

    /// Roles the user could give `user` or take from them in `g`.
    pub fn assignable_roles(&self, g: GuildId) -> Vec<RoleId> {
        let Some(guild) = self.chat.guild(g) else {
            return vec![];
        };
        sorted_roles(guild)
            .into_iter()
            .filter(|r| r.id != g && self.can_edit_role(g, r.id))
            .map(|r| r.id)
            .collect()
    }

    fn settings_guild(&self) -> Option<GuildId> {
        self.admin.settings.as_ref().map(|f| f.guild)
    }

    pub(super) fn update_roles(&mut self, u: RoleUi) -> Task<Message> {
        match u {
            RoleUi::Page(p) => {
                let Some(g) = self.settings_guild() else {
                    return Task::none();
                };
                let allowed = match p {
                    SettingsPage::Overview => self.can_manage_guild(g),
                    SettingsPage::Roles => self.can_manage_roles(g),
                };
                if allowed {
                    self.admin.page = p;
                    self.admin.roles = RolesPage::default();
                    if p == SettingsPage::Overview
                        && let Some(f) = self.admin.settings.as_mut()
                    {
                        f.reset();
                    }
                }
                Task::none()
            }
            RoleUi::Select(r) => {
                let Some(g) = self.settings_guild() else {
                    return Task::none();
                };
                if !self.can_edit_role(g, r) {
                    return Task::none();
                }
                if let Some(role) = self
                    .chat
                    .guild(g)
                    .and_then(|x| x.roles.iter().find(|x| x.id == r))
                {
                    self.admin.roles = RolesPage {
                        form: Some(RoleForm::new(role, r == g)),
                        ..Default::default()
                    };
                }
                Task::none()
            }
            RoleUi::Create => self.create_role(),
            RoleUi::Name(s) => {
                if let Some(f) = self.admin.roles.form.as_mut() {
                    f.name = s.chars().take(ROLE_NAME_MAX).collect();
                    f.error = None;
                }
                Task::none()
            }
            RoleUi::Color(c) => {
                if let Some(f) = self.admin.roles.form.as_mut() {
                    f.color = c;
                }
                Task::none()
            }
            RoleUi::Permission(bit, on) => {
                let grantable = self.settings_guild().map_or(0, |g| self.grantable(g));
                if let Some(f) = self.admin.roles.form.as_mut()
                    && grantable & bit == bit
                {
                    if on {
                        f.permissions |= bit;
                    } else {
                        f.permissions &= !bit;
                    }
                }
                Task::none()
            }
            RoleUi::Reset => {
                if let Some(f) = self.admin.roles.form.as_mut() {
                    f.reset();
                }
                Task::none()
            }
            RoleUi::Save => self.save_role(),
            RoleUi::AskDelete => {
                if self.admin.roles.form.as_ref().is_some_and(|f| !f.everyone) {
                    self.admin.roles.confirm_delete = true;
                }
                Task::none()
            }
            RoleUi::ConfirmDelete => self.delete_role(),
            RoleUi::OpenMemberRoles { guild, user } => {
                self.close_overlays();
                if self.assignable_roles(guild).is_empty() {
                    return Task::none();
                }
                self.member_dialog = Some(MemberDialog::Roles {
                    guild,
                    user,
                    pending: vec![],
                    error: None,
                });
                Task::none()
            }
            RoleUi::MemberRole(role, on) => self.toggle_member_role(role, on),
            RoleUi::OpenModeration { guild, user, kind } => {
                self.close_overlays();
                let bit = match kind {
                    ModKind::Timeout => permissions::MODERATE_MEMBERS,
                    ModKind::Kick => permissions::KICK_MEMBERS,
                    ModKind::Ban => permissions::BAN_MEMBERS,
                };
                if !self.can_moderate(guild, user, bit) {
                    return Task::none();
                }
                self.member_dialog = Some(MemberDialog::Moderate(ModForm {
                    guild,
                    user,
                    kind,
                    reason: String::new(),
                    ban_history: 0,
                    timeout: 60 * 60,
                    busy: false,
                    error: None,
                }));
                Task::none()
            }
            RoleUi::Reason(s) => {
                if let Some(MemberDialog::Moderate(f)) = self.member_dialog.as_mut() {
                    f.reason = s.chars().take(REASON_MAX).collect();
                }
                Task::none()
            }
            RoleUi::BanHistory(s) => {
                if let Some(MemberDialog::Moderate(f)) = self.member_dialog.as_mut() {
                    f.ban_history = s;
                }
                Task::none()
            }
            RoleUi::TimeoutLength(s) => {
                if let Some(MemberDialog::Moderate(f)) = self.member_dialog.as_mut() {
                    f.timeout = s;
                }
                Task::none()
            }
            RoleUi::ConfirmModeration(remove) => self.moderate(remove),
            RoleUi::CloseDialog => {
                if self.admin.roles.confirm_delete {
                    if !self.admin.roles.deleting {
                        self.admin.roles.confirm_delete = false;
                    }
                    return Task::none();
                }
                let busy = matches!(&self.member_dialog, Some(MemberDialog::Moderate(f)) if f.busy);
                if !busy {
                    self.member_dialog = None;
                }
                Task::none()
            }
        }
    }

    fn create_role(&mut self) -> Task<Message> {
        let Some(g) = self.settings_guild() else {
            return Task::none();
        };
        if !self.can_manage_roles(g) || self.admin.roles.creating {
            return Task::none();
        }
        self.admin.roles.creating = true;
        let edit = RoleEdit {
            name: Some("new role".into()),
            permissions: Some(0),
            ..Default::default()
        };
        if self.cfg.mock {
            let id = Snowflake(crate::mock::now_ms().max(1_000_000));
            return Task::done(Message::Done(Done::Roles(RoleDone::Created {
                guild: g,
                result: Ok(Role {
                    id,
                    name: "new role".into(),
                    color: 0,
                    position: 1,
                    permissions: 0,
                }),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(async move { rest.create_role(g, &edit).await }, move |r| {
            Message::Done(Done::Roles(RoleDone::Created {
                guild: g,
                result: r.map_err(|e| admin_error_text(&e)),
            }))
        })
    }

    fn save_role(&mut self) -> Task<Message> {
        let Some(g) = self.settings_guild() else {
            return Task::none();
        };
        let Some(f) = self.admin.roles.form.as_mut() else {
            return Task::none();
        };
        if !f.can_save() {
            if !f.name_ok() {
                f.error = Some(format!(
                    "The role name must be 1 to {ROLE_NAME_MAX} characters long."
                ));
            }
            return Task::none();
        }
        f.busy = true;
        f.error = None;
        let (role, edit) = (f.role, f.edit());
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Roles(RoleDone::Saved {
                guild: g,
                role,
                edit,
                result: Ok(()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(
            {
                let edit = edit.clone();
                async move { rest.modify_role(g, role, &edit).await }
            },
            move |r| {
                Message::Done(Done::Roles(RoleDone::Saved {
                    guild: g,
                    role,
                    edit: edit.clone(),
                    result: r.map_err(|e| admin_error_text(&e)),
                }))
            },
        )
    }

    fn delete_role(&mut self) -> Task<Message> {
        let Some(g) = self.settings_guild() else {
            return Task::none();
        };
        let page = &mut self.admin.roles;
        let Some(role) = page.form.as_ref().filter(|f| !f.everyone).map(|f| f.role) else {
            return Task::none();
        };
        if !page.confirm_delete || page.deleting {
            return Task::none();
        }
        page.deleting = true;
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Roles(RoleDone::Deleted {
                guild: g,
                role,
                result: Ok(()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(async move { rest.delete_role(g, role).await }, move |r| {
            Message::Done(Done::Roles(RoleDone::Deleted {
                guild: g,
                role,
                result: r.map_err(|e| admin_error_text(&e)),
            }))
        })
    }

    /// Gives or takes a role at once (optimistic); a failure undoes it.
    fn toggle_member_role(&mut self, role: RoleId, on: bool) -> Task<Message> {
        let Some(MemberDialog::Roles {
            guild,
            user,
            pending,
            error,
        }) = self.member_dialog.as_mut()
        else {
            return Task::none();
        };
        let (g, u) = (*guild, *user);
        if pending.contains(&role) {
            return Task::none();
        }
        pending.push(role);
        *error = None;
        if !self.can_edit_role(g, role) || role == g {
            if let Some(MemberDialog::Roles { pending, .. }) = self.member_dialog.as_mut() {
                pending.retain(|r| *r != role);
            }
            return Task::none();
        }
        self.set_member_role_local(g, u, role, on);
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Roles(RoleDone::MemberRole {
                guild: g,
                user: u,
                role,
                on,
                result: Ok(()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.set_member_role(g, u, role, on).await },
            move |r| {
                Message::Done(Done::Roles(RoleDone::MemberRole {
                    guild: g,
                    user: u,
                    role,
                    on,
                    result: r.map_err(|e| admin_error_text(&e)),
                }))
            },
        )
    }

    fn set_member_role_local(&mut self, g: GuildId, u: UserId, role: RoleId, on: bool) {
        let mut roles = self
            .chat
            .member_roles
            .get(&(g, u))
            .cloned()
            .unwrap_or_default();
        roles.retain(|r| *r != role);
        if on {
            roles.push(role);
        }
        let ctx = self.ctx();
        let _ = self
            .chat
            .apply_event(&Event::MemberRoles(vec![(g, u, roles)]), &ctx);
    }

    fn moderate(&mut self, remove_timeout: bool) -> Task<Message> {
        let Some(MemberDialog::Moderate(f)) = self.member_dialog.as_mut() else {
            return Task::none();
        };
        if f.busy {
            return Task::none();
        }
        f.busy = true;
        f.error = None;
        let (g, u, action) = (f.guild, f.user, f.action(remove_timeout));
        let done = match (&action, remove_timeout) {
            (ModAction::Kick { .. }, _) => "kicked",
            (ModAction::Ban { .. }, _) => "banned",
            (ModAction::Timeout { .. }, false) => "timed out",
            (ModAction::Timeout { .. }, true) => "no longer timed out",
        };
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Roles(RoleDone::Moderated {
                guild: g,
                user: u,
                result: Ok(done.into()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.moderate(g, u, &action).await },
            move |r| {
                Message::Done(Done::Roles(RoleDone::Moderated {
                    guild: g,
                    user: u,
                    result: r.map(|_| done.to_owned()).map_err(|e| admin_error_text(&e)),
                }))
            },
        )
    }

    pub(super) fn on_roles_done(&mut self, d: RoleDone) -> Task<Message> {
        let ctx = self.ctx();
        match d {
            RoleDone::Created { guild, result } => {
                self.admin.roles.creating = false;
                match result {
                    Ok(role) => {
                        let _ = self.chat.apply_event(
                            &Event::RoleUpsert {
                                guild_id: guild,
                                role: role.clone(),
                            },
                            &ctx,
                        );
                        if self.settings_guild() == Some(guild) {
                            self.admin.roles.form = Some(RoleForm::new(&role, false));
                        }
                    }
                    Err(e) => self.toast = Some(format!("Could not create the role: {e}")),
                }
            }
            RoleDone::Saved {
                guild,
                role,
                edit,
                result,
            } => {
                let form = self.admin.roles.form.as_mut().filter(|f| f.role == role);
                match result {
                    Ok(()) => {
                        let current = self
                            .chat
                            .guild(guild)
                            .and_then(|g| g.roles.iter().find(|r| r.id == role))
                            .cloned();
                        if let Some(mut r) = current {
                            if let Some(n) = edit.name {
                                r.name = n;
                            }
                            if let Some(c) = edit.color {
                                r.color = c;
                            }
                            if let Some(p) = edit.permissions {
                                r.permissions = p;
                            }
                            if let Some(f) = form {
                                let everyone = f.everyone;
                                *f = RoleForm::new(&r, everyone);
                            }
                            let _ = self.chat.apply_event(
                                &Event::RoleUpsert {
                                    guild_id: guild,
                                    role: r,
                                },
                                &ctx,
                            );
                        }
                        self.toast = Some("Role saved.".into());
                    }
                    Err(e) => {
                        if let Some(f) = form {
                            f.busy = false;
                            f.error = Some(e);
                        }
                    }
                }
            }
            RoleDone::Deleted {
                guild,
                role,
                result,
            } => {
                self.admin.roles.deleting = false;
                match result {
                    Ok(()) => {
                        self.admin.roles = RolesPage::default();
                        let _ = self.chat.apply_event(
                            &Event::RoleDelete {
                                guild_id: guild,
                                role_id: role,
                            },
                            &ctx,
                        );
                    }
                    Err(e) => {
                        self.admin.roles.confirm_delete = false;
                        if let Some(f) = self.admin.roles.form.as_mut() {
                            f.error = Some(e);
                        }
                    }
                }
            }
            RoleDone::MemberRole {
                guild,
                user,
                role,
                on,
                result,
            } => {
                if let Err(e) = &result {
                    self.set_member_role_local(guild, user, role, !on);
                    if !matches!(self.member_dialog, Some(MemberDialog::Roles { .. })) {
                        self.toast = Some(format!("Could not change the role: {e}"));
                    }
                }
                if let Some(MemberDialog::Roles { pending, error, .. }) =
                    self.member_dialog.as_mut()
                {
                    pending.retain(|r| *r != role);
                    if let Err(e) = result {
                        *error = Some(e);
                    }
                }
            }
            RoleDone::Moderated {
                guild: _,
                user,
                result,
            } => match result {
                Ok(done) => {
                    self.member_dialog = None;
                    let name = self
                        .chat
                        .users
                        .get(&user)
                        .map(|u| u.display_name().to_owned())
                        .unwrap_or_else(|| "The member".into());
                    self.toast = Some(format!("{name} was {done}."));
                }
                Err(e) => {
                    if let Some(MemberDialog::Moderate(f)) = self.member_dialog.as_mut() {
                        f.busy = false;
                        f.error = Some(e);
                    }
                }
            },
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Ui;
    use crate::app::guild_admin::AdminUi;
    use crate::mock;

    fn app() -> App {
        App::boot(crate::config::Config::for_tests()).0
    }

    fn ru(a: &mut App, u: RoleUi) {
        let _ = a.update(Message::Ui(Ui::Roles(u)));
    }

    const G: GuildId = mock::GUILD_GAMES;

    #[test]
    fn hierarchy_gates_roles_and_members() {
        let mut a = app();
        // Owner of Game Night: every role, every member.
        assert!(a.can_manage_roles(G));
        assert!(a.can_edit_role(G, mock::ROLE_GAMES_MODS));
        assert!(a.can_moderate(G, Snowflake(2), permissions::BAN_MEMBERS));
        assert!(
            !a.can_moderate(G, mock::ME, permissions::KICK_MEMBERS),
            "not oneself"
        );
        // Rust: plain member, nothing.
        assert!(!a.can_manage_roles(mock::GUILD_RUST));
        assert!(!a.can_moderate(mock::GUILD_RUST, Snowflake(2), permissions::KICK_MEMBERS));
        // Not the owner any more, a moderator (Players < Mods) with Manage
        // Roles and Kick Members.
        if let Some(g) = a.chat.guilds.iter_mut().find(|g| g.id == G) {
            g.owner_id = Some(Snowflake(3));
            for r in &mut g.roles {
                if r.id == mock::ROLE_GAMES_MODS {
                    r.permissions = MANAGE_ROLES | permissions::KICK_MEMBERS;
                }
            }
        }
        a.chat.my_roles.insert(G, vec![mock::ROLE_GAMES_MODS]);
        assert!(!a.can_edit_role(G, mock::ROLE_GAMES_MODS), "own top role");
        assert!(a.can_edit_role(G, mock::ROLE_GAMES_PLAYERS));
        assert!(a.can_edit_role(G, G), "@everyone");
        assert_eq!(a.assignable_roles(G), vec![mock::ROLE_GAMES_PLAYERS]);
        assert_eq!(
            a.grantable(G),
            mock::EVERYONE_PERMISSIONS | MANAGE_ROLES | permissions::KICK_MEMBERS
        );
        // Alice is a Player: below. Carol owns the server: never.
        assert!(a.can_moderate(G, Snowflake(2), permissions::KICK_MEMBERS));
        assert!(!a.can_moderate(G, Snowflake(2), permissions::BAN_MEMBERS));
        assert!(!a.can_moderate(G, Snowflake(3), permissions::KICK_MEMBERS));
        // A fellow moderator ranks the same: no.
        a.chat
            .member_roles
            .insert((G, Snowflake(4)), vec![mock::ROLE_GAMES_MODS]);
        assert!(!a.can_moderate(G, Snowflake(4), permissions::KICK_MEMBERS));
        // Only grantable bits can be switched on.
        let _ = a.update(Message::Ui(Ui::Admin(AdminUi::OpenSettings(G))));
        ru(&mut a, RoleUi::Page(SettingsPage::Roles));
        ru(&mut a, RoleUi::Select(mock::ROLE_GAMES_PLAYERS));
        ru(&mut a, RoleUi::Permission(permissions::ADMINISTRATOR, true));
        ru(&mut a, RoleUi::Permission(permissions::KICK_MEMBERS, true));
        let f = a.admin.roles.form.as_ref().map(|f| f.permissions);
        assert_eq!(f.map(|p| p & permissions::ADMINISTRATOR), Some(0));
        assert_ne!(f.map(|p| p & permissions::KICK_MEMBERS), Some(0));
    }

    #[test]
    fn create_edit_and_delete_a_role() {
        let mut a = app();
        let _ = a.update(Message::Ui(Ui::Admin(AdminUi::OpenSettings(G))));
        ru(&mut a, RoleUi::Page(SettingsPage::Roles));
        assert_eq!(a.admin.page, SettingsPage::Roles);
        ru(&mut a, RoleUi::Create);
        let _ = a.on_roles_done(RoleDone::Created {
            guild: G,
            result: Ok(Role {
                id: Snowflake(4999),
                name: "new role".into(),
                color: 0,
                position: 1,
                permissions: 0,
            }),
        });
        assert_eq!(
            a.admin.roles.form.as_ref().map(|f| f.role),
            Some(Snowflake(4999))
        );
        ru(&mut a, RoleUi::Name("Builders".into()));
        ru(&mut a, RoleUi::Color(ROLE_COLORS[2]));
        ru(
            &mut a,
            RoleUi::Permission(permissions::MANAGE_CHANNELS, true),
        );
        let edit = a
            .admin
            .roles
            .form
            .as_ref()
            .map(RoleForm::edit)
            .unwrap_or_default();
        assert_eq!(edit.name.as_deref(), Some("Builders"));
        assert_eq!(edit.permissions, Some(permissions::MANAGE_CHANNELS));
        ru(&mut a, RoleUi::Save);
        let _ = a.on_roles_done(RoleDone::Saved {
            guild: G,
            role: Snowflake(4999),
            edit,
            result: Ok(()),
        });
        let r = a
            .chat
            .guild(G)
            .and_then(|g| g.roles.iter().find(|r| r.id == Snowflake(4999)))
            .cloned();
        assert_eq!(r.as_ref().map(|r| r.name.as_str()), Some("Builders"));
        assert_eq!(r.map(|r| r.color), Some(ROLE_COLORS[2]));
        assert!(!a.admin.roles.form.as_ref().is_some_and(RoleForm::dirty));
        // @everyone: no rename, no delete.
        ru(&mut a, RoleUi::Select(G));
        ru(&mut a, RoleUi::Name("everybody".into()));
        assert_eq!(
            a.admin.roles.form.as_ref().map(|f| f.edit().name),
            Some(None)
        );
        ru(&mut a, RoleUi::AskDelete);
        assert!(!a.admin.roles.confirm_delete);
        // Delete.
        ru(&mut a, RoleUi::Select(Snowflake(4999)));
        ru(&mut a, RoleUi::AskDelete);
        ru(&mut a, RoleUi::ConfirmDelete);
        assert!(a.admin.roles.deleting);
        let _ = a.on_roles_done(RoleDone::Deleted {
            guild: G,
            role: Snowflake(4999),
            result: Ok(()),
        });
        assert!(
            a.chat
                .guild(G)
                .is_some_and(|g| g.roles.iter().all(|r| r.id != Snowflake(4999)))
        );
        assert!(a.admin.roles.form.is_none());
    }

    #[test]
    fn member_roles_toggle_and_roll_back() {
        let mut a = app();
        let alice = Snowflake(2);
        ru(
            &mut a,
            RoleUi::OpenMemberRoles {
                guild: G,
                user: alice,
            },
        );
        assert!(a.member_dialog.is_some());
        ru(&mut a, RoleUi::MemberRole(mock::ROLE_GAMES_MODS, true));
        assert!(a.chat.member_roles[&(G, alice)].contains(&mock::ROLE_GAMES_MODS));
        // A second click while in flight is ignored.
        ru(&mut a, RoleUi::MemberRole(mock::ROLE_GAMES_MODS, false));
        assert!(a.chat.member_roles[&(G, alice)].contains(&mock::ROLE_GAMES_MODS));
        let _ = a.on_roles_done(RoleDone::MemberRole {
            guild: G,
            user: alice,
            role: mock::ROLE_GAMES_MODS,
            on: true,
            result: Err("nope".into()),
        });
        assert!(!a.chat.member_roles[&(G, alice)].contains(&mock::ROLE_GAMES_MODS));
        assert!(matches!(
            &a.member_dialog,
            Some(MemberDialog::Roles { pending, error: Some(_), .. }) if pending.is_empty()
        ));
    }

    #[test]
    fn moderation_dialogs() {
        let mut a = app();
        let alice = Snowflake(2);
        ru(
            &mut a,
            RoleUi::OpenModeration {
                guild: G,
                user: alice,
                kind: ModKind::Ban,
            },
        );
        ru(&mut a, RoleUi::BanHistory(3600));
        ru(&mut a, RoleUi::Reason("spam".into()));
        let Some(MemberDialog::Moderate(f)) = &a.member_dialog else {
            panic!("no dialog");
        };
        assert_eq!(
            f.action(false),
            ModAction::Ban {
                delete_seconds: 3600,
                reason: "spam".into()
            }
        );
        ru(&mut a, RoleUi::ConfirmModeration(false));
        // Busy: not closable.
        ru(&mut a, RoleUi::CloseDialog);
        assert!(a.member_dialog.is_some());
        let _ = a.on_roles_done(RoleDone::Moderated {
            guild: G,
            user: alice,
            result: Err("Missing Permissions".into()),
        });
        assert!(
            matches!(&a.member_dialog, Some(MemberDialog::Moderate(f)) if !f.busy && f.error.is_some())
        );
        ru(&mut a, RoleUi::ConfirmModeration(false));
        let _ = a.on_roles_done(RoleDone::Moderated {
            guild: G,
            user: alice,
            result: Ok("banned".into()),
        });
        assert!(a.member_dialog.is_none());
        assert!(
            a.toast
                .as_deref()
                .is_some_and(|t| t.ends_with("was banned."))
        );
        // Timeout removal sends no end time.
        ru(
            &mut a,
            RoleUi::OpenModeration {
                guild: G,
                user: alice,
                kind: ModKind::Timeout,
            },
        );
        let Some(MemberDialog::Moderate(f)) = &a.member_dialog else {
            panic!("no dialog");
        };
        assert_eq!(
            f.action(true),
            ModAction::Timeout {
                seconds: None,
                reason: String::new()
            }
        );
        // Not oneself.
        ru(&mut a, RoleUi::CloseDialog);
        ru(
            &mut a,
            RoleUi::OpenModeration {
                guild: G,
                user: mock::ME,
                kind: ModKind::Kick,
            },
        );
        assert!(a.member_dialog.is_none());
    }
}
