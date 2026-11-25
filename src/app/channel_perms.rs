//! Channel permissions: the overwrites of a server channel or category
//! (`@everyone`, roles, members), each permission allowed, denied or left
//! to the server-wide value, like the official "Permissions" tab. Offered
//! with Manage Permissions (Manage Roles in that channel); only bits the
//! user has there can be changed. "Save Changes" sends one
//! `PUT /channels/{c}/permissions/{id}` per changed overwrite and one
//! `DELETE` per removed one; CHANNEL_UPDATE brings the result back (applied
//! locally at once).

use iced::Task;

use super::guild_admin::admin_error_text;
use super::permissions::{ALL, MANAGE_ROLES};
use super::role_admin::sorted_roles;
use super::{App, Done, Message};
use crate::net::model::*;

/// One permission's state in an overwrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tri {
    Deny,
    Inherit,
    Allow,
}

/// A role that can get an overwrite (the "Add a role" picker).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleChoice {
    pub id: RoleId,
    pub name: String,
}

impl std::fmt::Display for RoleChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermsUi {
    Open(ChannelId),
    /// Shows the overwrite of this role or member.
    Select(Snowflake),
    AddRole(RoleChoice),
    Set(u64, Tri),
    /// Removes the selected overwrite (`@everyone` is cleared instead).
    Remove,
    Reset,
    Save,
    Close,
}

#[derive(Debug, Clone)]
pub enum PermsDone {
    Saved {
        channel: ChannelId,
        overwrites: Vec<PermissionOverwrite>,
        result: Result<(), String>,
    },
}

/// The open "Channel Permissions" dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct PermsForm {
    pub channel: ChannelId,
    pub guild: GuildId,
    pub kind: ChannelKind,
    original: Vec<PermissionOverwrite>,
    /// Working copy; `@everyone` is always present (all neutral when the
    /// channel has none).
    pub overwrites: Vec<PermissionOverwrite>,
    pub selected: Snowflake,
    pub busy: bool,
    pub error: Option<String>,
}

impl PermsForm {
    pub fn new(c: &Channel, guild: GuildId) -> Self {
        let mut overwrites = c.overwrites.clone();
        if !overwrites.iter().any(|o| !o.member && o.id == guild) {
            overwrites.insert(0, everyone(guild));
        }
        Self {
            channel: c.id,
            guild,
            kind: c.kind,
            original: c.overwrites.clone(),
            overwrites,
            selected: guild,
            busy: false,
            error: None,
        }
    }

    pub fn current(&self) -> Option<&PermissionOverwrite> {
        self.overwrites.iter().find(|o| o.id == self.selected)
    }

    /// The state of `bit` in the selected overwrite.
    pub fn tri(&self, bit: u64) -> Tri {
        match self.current() {
            Some(o) if o.allow & bit != 0 => Tri::Allow,
            Some(o) if o.deny & bit != 0 => Tri::Deny,
            _ => Tri::Inherit,
        }
    }

    fn set(&mut self, bit: u64, t: Tri) {
        let sel = self.selected;
        if let Some(o) = self.overwrites.iter_mut().find(|o| o.id == sel) {
            o.allow &= !bit;
            o.deny &= !bit;
            match t {
                Tri::Allow => o.allow |= bit,
                Tri::Deny => o.deny |= bit,
                Tri::Inherit => {}
            }
        }
    }

    /// What "Save Changes" sends: overwrites to put (new or changed) and
    /// ids to delete. A neutral `@everyone` the channel never had is not
    /// sent.
    pub fn changes(&self) -> (Vec<PermissionOverwrite>, Vec<Snowflake>) {
        let mut put = Vec::new();
        for o in &self.overwrites {
            match self.original.iter().find(|x| x.id == o.id) {
                Some(x) if x == o => {}
                Some(_) => put.push(*o),
                None if o.id == self.guild && o.allow == 0 && o.deny == 0 => {}
                None => put.push(*o),
            }
        }
        let delete = self
            .original
            .iter()
            .filter(|x| !self.overwrites.iter().any(|o| o.id == x.id))
            .map(|x| x.id)
            .collect();
        (put, delete)
    }

    pub fn dirty(&self) -> bool {
        let (p, d) = self.changes();
        !p.is_empty() || !d.is_empty()
    }

    /// The overwrites the channel has after saving.
    pub fn saved(&self) -> Vec<PermissionOverwrite> {
        self.overwrites
            .iter()
            .filter(|o| {
                !(o.id == self.guild
                    && o.allow == 0
                    && o.deny == 0
                    && !self.original.iter().any(|x| x.id == o.id))
            })
            .copied()
            .collect()
    }

    pub fn reset(&mut self) {
        let sel = self.selected;
        let mut fresh = self.original.clone();
        if !fresh.iter().any(|o| !o.member && o.id == self.guild) {
            fresh.insert(0, everyone(self.guild));
        }
        self.overwrites = fresh;
        if !self.overwrites.iter().any(|o| o.id == sel) {
            self.selected = self.guild;
        }
        self.error = None;
    }
}

fn everyone(guild: GuildId) -> PermissionOverwrite {
    PermissionOverwrite {
        id: guild,
        member: false,
        allow: 0,
        deny: 0,
    }
}

impl App {
    /// Whether the overwrites of `c` can be edited: a server channel and
    /// Manage Permissions (Manage Roles) in it.
    pub fn can_manage_permissions(&self, c: ChannelId) -> bool {
        let Some(ch) = self.chat.channels.get(&c) else {
            return false;
        };
        ch.guild_id.is_some()
            && ch.kind != ChannelKind::Thread
            && self.chat.channel_permissions(c) & MANAGE_ROLES != 0
    }

    /// Bits the user may allow or deny in `c`: the ones they have there.
    pub fn channel_grantable(&self, c: ChannelId) -> u64 {
        let p = self.chat.channel_permissions(c);
        if p == ALL { ALL } else { p }
    }

    /// The overwrite targets of the dialog, in the official order:
    /// `@everyone`, the roles (highest first), then members.
    pub fn perms_targets(&self, f: &PermsForm) -> Vec<(Snowflake, String, u32, bool)> {
        let mut out = vec![(f.guild, "@everyone".to_owned(), 0, false)];
        if let Some(g) = self.chat.guild(f.guild) {
            for r in sorted_roles(g) {
                if r.id != g.id && f.overwrites.iter().any(|o| !o.member && o.id == r.id) {
                    out.push((r.id, r.name.clone(), r.color, false));
                }
            }
        }
        for o in f.overwrites.iter().filter(|o| o.member) {
            let name = self
                .chat
                .users
                .get(&o.id)
                .map(|u| u.display_name().to_owned())
                .unwrap_or_else(|| format!("Member {}", o.id));
            out.push((o.id, name, 0, true));
        }
        out
    }

    /// Roles without an overwrite yet, for "Add a role".
    pub fn perms_addable(&self, f: &PermsForm) -> Vec<RoleChoice> {
        let Some(g) = self.chat.guild(f.guild) else {
            return vec![];
        };
        sorted_roles(g)
            .into_iter()
            .filter(|r| r.id != g.id && !f.overwrites.iter().any(|o| o.id == r.id))
            .map(|r| RoleChoice {
                id: r.id,
                name: r.name.clone(),
            })
            .collect()
    }

    pub(super) fn update_perms(&mut self, u: PermsUi) -> Task<Message> {
        if let PermsUi::Open(c) = u {
            self.close_overlays();
            if !self.can_manage_permissions(c) {
                return Task::none();
            }
            if let Some(ch) = self.chat.channels.get(&c)
                && let Some(g) = ch.guild_id
            {
                self.perms_dialog = Some(PermsForm::new(ch, g));
            }
            return Task::none();
        }
        let grantable = self
            .perms_dialog
            .as_ref()
            .map_or(0, |f| self.channel_grantable(f.channel));
        let Some(f) = self.perms_dialog.as_mut() else {
            return Task::none();
        };
        match u {
            PermsUi::Open(_) => {}
            PermsUi::Select(id) => {
                if f.overwrites.iter().any(|o| o.id == id) {
                    f.selected = id;
                }
            }
            PermsUi::AddRole(r) => {
                if !f.overwrites.iter().any(|o| o.id == r.id) {
                    f.overwrites.push(PermissionOverwrite {
                        id: r.id,
                        member: false,
                        allow: 0,
                        deny: 0,
                    });
                }
                f.selected = r.id;
                f.error = None;
            }
            PermsUi::Set(bit, t) => {
                if !f.busy && grantable & bit == bit {
                    f.set(bit, t);
                    f.error = None;
                }
            }
            PermsUi::Remove => {
                if f.busy {
                    return Task::none();
                }
                if f.selected == f.guild {
                    if let Some(o) = f.overwrites.iter_mut().find(|o| o.id == f.guild) {
                        o.allow = 0;
                        o.deny = 0;
                    }
                } else {
                    let sel = f.selected;
                    f.overwrites.retain(|o| o.id != sel);
                    f.selected = f.guild;
                }
            }
            PermsUi::Reset => f.reset(),
            PermsUi::Save => return self.save_perms(),
            PermsUi::Close => {
                if !f.busy {
                    self.perms_dialog = None;
                }
            }
        }
        Task::none()
    }

    fn save_perms(&mut self) -> Task<Message> {
        let Some(f) = self.perms_dialog.as_mut() else {
            return Task::none();
        };
        if f.busy || !f.dirty() {
            return Task::none();
        }
        f.busy = true;
        f.error = None;
        let (put, delete) = f.changes();
        let (channel, overwrites) = (f.channel, f.saved());
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Perms(PermsDone::Saved {
                channel,
                overwrites,
                result: Ok(()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(
            async move {
                for o in &put {
                    rest.set_overwrite(channel, o).await?;
                }
                for id in delete {
                    rest.delete_overwrite(channel, id).await?;
                }
                Ok::<(), crate::error::Error>(())
            },
            move |r| {
                Message::Done(Done::Perms(PermsDone::Saved {
                    channel,
                    overwrites: overwrites.clone(),
                    result: r.map_err(|e| admin_error_text(&e)),
                }))
            },
        )
    }

    pub(super) fn on_perms_done(&mut self, d: PermsDone) -> Task<Message> {
        let PermsDone::Saved {
            channel,
            overwrites,
            result,
        } = d;
        match result {
            Ok(()) => {
                // CHANNEL_UPDATE confirms; show the new permissions now.
                if let Some(ch) = self.chat.channels.get_mut(&channel) {
                    ch.overwrites = overwrites;
                }
                if self
                    .perms_dialog
                    .as_ref()
                    .is_some_and(|f| f.channel == channel)
                {
                    self.perms_dialog = None;
                }
                self.toast = Some("Channel permissions saved.".into());
            }
            Err(e) => {
                if let Some(f) = self.perms_dialog.as_mut().filter(|f| f.channel == channel) {
                    f.busy = false;
                    f.error = Some(e);
                } else {
                    self.toast = Some(format!("Could not save the permissions: {e}"));
                }
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Ui;
    use crate::app::permissions::{SEND_MESSAGES, VIEW_CHANNEL};
    use crate::mock;

    const G: GuildId = Snowflake(10);
    const MODS: RoleId = Snowflake(20);

    fn channel(overwrites: Vec<PermissionOverwrite>) -> Channel {
        Channel {
            id: Snowflake(30),
            kind: ChannelKind::Text,
            guild_id: Some(G),
            name: Some("general".into()),
            topic: None,
            position: 0,
            parent_id: None,
            last_message_id: None,
            recipients: vec![],
            nsfw: false,
            overwrites,
        }
    }

    fn ow(id: Snowflake, allow: u64, deny: u64) -> PermissionOverwrite {
        PermissionOverwrite {
            id,
            member: false,
            allow,
            deny,
        }
    }

    #[test]
    fn neutral_everyone_is_not_sent() {
        let f = PermsForm::new(&channel(vec![]), G);
        assert_eq!(f.overwrites.len(), 1);
        assert!(!f.dirty());
        assert!(f.saved().is_empty());
    }

    #[test]
    fn tri_state_and_changes() {
        let mut f = PermsForm::new(&channel(vec![ow(MODS, SEND_MESSAGES, 0)]), G);
        // Hide from @everyone.
        f.set(VIEW_CHANNEL, Tri::Deny);
        assert_eq!(f.tri(VIEW_CHANNEL), Tri::Deny);
        // Mods: allow View, back to neutral on Send.
        f.selected = MODS;
        f.set(VIEW_CHANNEL, Tri::Allow);
        f.set(SEND_MESSAGES, Tri::Inherit);
        assert_eq!(f.tri(SEND_MESSAGES), Tri::Inherit);
        let (put, delete) = f.changes();
        assert_eq!(put, vec![ow(G, 0, VIEW_CHANNEL), ow(MODS, VIEW_CHANNEL, 0)]);
        assert!(delete.is_empty());
        // Removing the role overwrite deletes it; reset brings it back.
        f.overwrites.retain(|o| o.id != MODS);
        assert_eq!(f.changes().1, vec![MODS]);
        f.reset();
        assert!(!f.dirty());
        assert_eq!(f.selected, MODS, "still there after the reset");
    }

    #[test]
    fn dialog_needs_manage_permissions_and_saves() {
        let mut a = App::boot(crate::config::Config::for_tests()).0;
        let help = Snowflake(412);
        // Owner of Game Night.
        assert!(a.can_manage_permissions(help));
        let _ = a.update(Message::Ui(Ui::Perms(PermsUi::Open(help))));
        let f = a.perms_dialog.clone().unwrap();
        assert_eq!(f.guild, mock::GUILD_GAMES);
        let add = a.perms_addable(&f);
        assert!(add.iter().any(|r| r.id == mock::ROLE_GAMES_MODS));
        let mods = add
            .into_iter()
            .find(|r| r.id == mock::ROLE_GAMES_MODS)
            .unwrap();
        let _ = a.update(Message::Ui(Ui::Perms(PermsUi::AddRole(mods))));
        let _ = a.update(Message::Ui(Ui::Perms(PermsUi::Set(
            SEND_MESSAGES,
            Tri::Deny,
        ))));
        assert!(a.perms_dialog.as_ref().unwrap().dirty());
        let _ = a.update(Message::Ui(Ui::Perms(PermsUi::Save)));
        let _ = a.on_perms_done(PermsDone::Saved {
            channel: help,
            overwrites: a.perms_dialog.as_ref().unwrap().saved(),
            result: Ok(()),
        });
        assert!(a.perms_dialog.is_none());
        let ch = &a.chat.channels[&help];
        assert!(
            ch.overwrites
                .iter()
                .any(|o| o.id == mock::ROLE_GAMES_MODS && o.deny == SEND_MESSAGES)
        );
        // Not offered in a server the user does not manage.
        assert!(!a.can_manage_permissions(Snowflake(999_999)));
    }
}
