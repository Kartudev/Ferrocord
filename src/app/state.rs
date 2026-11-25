//! Pure state types and reducers. Unit-tested without iced.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::net::Event;
use crate::net::extras::ChannelNotify;
use crate::net::model::*;

use super::permissions;

/// Messages kept in memory for the open conversation.
pub const MESSAGE_WINDOW: usize = 200;
/// Hard cap while the user scrolls back through history.
pub const MESSAGE_WINDOW_MAX: usize = 600;
/// A typing indicator disappears after this long without a refresh.
pub const TYPING_TTL_MS: u64 = 10_000;
/// Consecutive messages by the same author within this window are grouped.
pub const GROUP_WINDOW_MS: u64 = 7 * 60 * 1000;

#[derive(Debug, Clone, Default)]
pub struct MessageWindow {
    /// Oldest first.
    pub messages: VecDeque<Message>,
    /// Older messages exist on the server (or in SQLite).
    pub reached_start: bool,
    pub loading: bool,
    /// History was fetched from the network since connect.
    pub synced: bool,
    /// The window shows an old part of the channel (after a jump): newer
    /// messages exist that are not loaded, so live messages are not
    /// appended.
    pub detached: bool,
}

impl MessageWindow {
    pub fn oldest(&self) -> Option<MessageId> {
        self.messages.iter().find(|m| !m.pending).map(|m| m.id)
    }

    pub fn newest(&self) -> Option<MessageId> {
        self.messages
            .iter()
            .rev()
            .find(|m| !m.pending)
            .map(|m| m.id)
    }

    /// Inserts or replaces one message, keeping id order. A server message
    /// whose nonce matches a pending local message replaces it.
    pub fn upsert(&mut self, msg: Message) {
        if let Some(n) = &msg.nonce
            && let Some(pos) = self
                .messages
                .iter()
                .position(|m| m.pending && m.nonce.as_ref() == Some(n))
        {
            self.messages.remove(pos);
        }
        if let Some(existing) = self.messages.iter_mut().find(|m| m.id == msg.id) {
            *existing = msg;
            return;
        }
        // Pending messages stay at the end; real ones are ordered by id.
        let pos = self
            .messages
            .iter()
            .rposition(|m| !m.pending && m.id < msg.id)
            .map_or(0, |p| p + 1);
        let pos = if msg.pending {
            self.messages.len()
        } else {
            pos
        };
        self.messages.insert(pos, msg);
    }

    pub fn get(&self, id: MessageId) -> Option<&Message> {
        self.messages.iter().find(|m| m.id == id)
    }

    pub fn get_mut(&mut self, id: MessageId) -> Option<&mut Message> {
        self.messages.iter_mut().find(|m| m.id == id)
    }

    /// Replaces the whole window (used after a jump to a message).
    pub fn replace(&mut self, msgs: Vec<Message>, detached: bool) {
        self.messages = msgs.into_iter().collect();
        self.messages.make_contiguous().sort_by_key(|m| m.id);
        self.detached = detached;
        self.loading = false;
        self.synced = true;
        self.reached_start = false;
    }

    fn trim_front(&mut self, max: usize) {
        while self.messages.len() > max {
            self.messages.pop_front();
            self.reached_start = false;
        }
    }

    fn trim_back(&mut self, max: usize) {
        while self.messages.len() > max {
            self.messages.pop_back();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Typing {
    pub user_id: UserId,
    pub until_ms: u64,
}

/// Indicator shown on a guild icon / DM button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Badge {
    pub unread: bool,
    pub mentions: u32,
}

/// How a message mentions the current user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionKind {
    None,
    /// `<@me>` or any message in a DM / group DM.
    Direct,
    /// A role the user has.
    Role,
    /// `@everyone` / `@here`.
    Everyone,
}

/// A desktop notification to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub channel_id: ChannelId,
    pub guild_id: Option<GuildId>,
}

/// Side effects requested by the reducer; executed by `app::update`.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Notify(Notification),
    /// Mark the channel read on the server up to this message.
    Ack(ChannelId, MessageId),
}

/// Context the reducer needs from the UI.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ctx {
    pub now_ms: u64,
    /// Channel currently open in the main pane.
    pub open_channel: Option<ChannelId>,
    /// Whether the window has keyboard focus.
    pub window_focused: bool,
    /// Include message text in notifications.
    pub notification_preview: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ChatState {
    pub me: Option<User>,
    /// In display order.
    pub guilds: Vec<Guild>,
    pub channels: HashMap<ChannelId, Channel>,
    pub users: HashMap<UserId, User>,
    pub messages: HashMap<ChannelId, MessageWindow>,
    pub read_states: HashMap<ChannelId, ReadState>,
    pub typing: HashMap<ChannelId, Vec<Typing>>,
    pub my_roles: HashMap<GuildId, Vec<RoleId>>,
    pub member_roles: HashMap<(GuildId, UserId), Vec<RoleId>>,
    pub muted_guilds: HashSet<GuildId>,
    pub muted_channels: HashSet<ChannelId>,
    /// Notification overrides per channel (level, timed mutes). Mutes in
    /// here are mirrored in `muted_channels`.
    pub channel_notify: HashMap<ChannelId, ChannelNotify>,
    /// The server rail: folders and ungrouped servers in display order.
    pub folders: super::folders::Folders,
    /// Servers where @everyone/@here do not count as mentions.
    pub suppress_everyone: HashSet<GuildId>,
    /// Servers where role mentions do not count.
    pub suppress_roles: HashSet<GuildId>,
    /// Last known status per user (friends, members of subscribed guilds).
    pub presence: HashMap<UserId, Presence>,
    /// Last known activities per user (popout "Playing ...", custom status).
    pub activities: HashMap<UserId, Vec<Activity>>,
    /// The user's own status.
    pub my_status: Presence,
    /// Nitro type of the current user (upload limit).
    pub premium_type: u8,
    /// Member list of the guild whose sidebar is open.
    pub member_list: Option<super::members::MemberListState>,
}

/// Cap on remembered presences.
const MAX_PRESENCES: usize = 20_000;

/// Result of toggling a reaction locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactionChange {
    Added,
    Removed,
}

impl ChatState {
    pub fn me_id(&self) -> Option<UserId> {
        self.me.as_ref().map(|u| u.id)
    }

    pub fn user_name(&self, id: UserId) -> Option<&str> {
        self.users.get(&id).map(User::display_name)
    }

    pub fn guild(&self, id: GuildId) -> Option<&Guild> {
        self.guilds.iter().find(|g| g.id == id)
    }

    /// People shown in a DM's side panel: the other recipients, plus the
    /// current user in a group DM (as the official client does), sorted by
    /// display name. Recipients we have no user object for are skipped.
    pub fn dm_participants(&self, c: &Channel) -> Vec<&User> {
        let mut out: Vec<&User> = c
            .recipients
            .iter()
            .filter_map(|id| self.users.get(id))
            .collect();
        if c.kind == ChannelKind::GroupDm
            && let Some(me) = &self.me
            && !c.recipients.contains(&me.id)
        {
            out.push(me);
        }
        out.sort_by_cached_key(|u| u.display_name().to_lowercase());
        out
    }

    /// Loads READY (or the boot snapshot) wholesale.
    pub fn load_ready(&mut self, r: &ReadyData) {
        self.me = r.me.clone();
        self.guilds = r.guilds.clone();
        let mut folders = super::folders::Folders::from_settings(&r.guild_folders, &r.guild_order);
        folders.sync(&self.guilds.iter().map(|g| g.id).collect::<Vec<_>>());
        self.folders = folders;
        self.resort_guilds();
        self.channels = r.channels.iter().map(|c| (c.id, c.clone())).collect();
        for u in r.users.iter().chain(r.me.iter()) {
            self.users.insert(u.id, u.clone());
        }
        self.read_states = r.read_states.iter().copied().collect();
        self.my_roles = r.my_roles.iter().cloned().collect();
        self.muted_guilds = r.muted_guilds.iter().copied().collect();
        self.muted_channels = r.muted_channels.iter().copied().collect();
        self.channel_notify = r.channel_notify.iter().copied().collect();
        self.suppress_everyone = r.suppress_everyone.iter().copied().collect();
        self.suppress_roles = r.suppress_roles.iter().copied().collect();
        // Presences from READY replace what we knew (stale after a
        // reconnect); our own status comes from the settings.
        self.presence = r.presences.iter().copied().collect();
        self.activities = r.activities.iter().cloned().collect();
        if let Some(s) = r.my_status {
            self.my_status = s;
        }
        if r.me.is_some() {
            self.premium_type = r.premium_type;
        }
        // Cached windows stay, but must be re-synced from the network.
        for w in self.messages.values_mut() {
            w.synced = false;
        }
        self.messages.retain(|c, _| self.channels.contains_key(c));
    }

    /// Keeps `guilds` in rail order.
    pub fn resort_guilds(&mut self) {
        let order = self.folders.flat_order();
        self.guilds
            .sort_by_key(|g| order.iter().position(|x| *x == g.id).unwrap_or(usize::MAX));
    }

    /// Whether `c` is muted right now (timed mutes expire).
    pub fn is_channel_muted(&self, c: ChannelId) -> bool {
        self.muted_channels.contains(&c)
            && self
                .channel_notify
                .get(&c)
                .is_none_or(|n| n.is_muted(crate::mock::now_ms()))
    }

    pub fn channel_notify(&self, c: ChannelId) -> ChannelNotify {
        self.channel_notify.get(&c).copied().unwrap_or_default()
    }

    /// Stores a channel's notification override (and mirrors its mute).
    pub fn set_channel_notify(&mut self, c: ChannelId, n: ChannelNotify) {
        if n.is_default() {
            self.channel_notify.remove(&c);
        } else {
            self.channel_notify.insert(c, n);
        }
        if n.is_muted(crate::mock::now_ms()) {
            self.muted_channels.insert(c);
        } else {
            self.muted_channels.remove(&c);
        }
    }

    /// Server-wide permission bits of the current user in `g` (0 when
    /// the server is unknown).
    pub fn guild_permissions(&self, g: GuildId) -> u64 {
        let Some(guild) = self.guild(g) else {
            return 0;
        };
        let mine = self.my_roles.get(&g).map(Vec::as_slice).unwrap_or(&[]);
        permissions::base(guild, self.me_id(), mine)
    }

    /// Whether the current user has `bit` server-wide in `g`.
    pub fn can_in_guild(&self, g: GuildId, bit: u64) -> bool {
        self.guild_permissions(g) & bit != 0
    }

    /// Permission bits of the current user in `c`, overwrites applied
    /// (threads use their parent's). Everything in DMs; 0 when the channel
    /// or its server is unknown.
    pub fn channel_permissions(&self, c: ChannelId) -> u64 {
        let Some(ch) = self.channels.get(&c) else {
            return 0;
        };
        let Some(g) = ch.guild_id else {
            return permissions::ALL;
        };
        self.channel_bits(g, self.guild_permissions(g), ch)
    }

    /// `ch`'s bits given the server-wide `base` of its server `g`.
    fn channel_bits(&self, g: GuildId, base: u64, ch: &Channel) -> u64 {
        let ch = match ch.kind {
            ChannelKind::Thread => ch
                .parent_id
                .and_then(|p| self.channels.get(&p))
                .unwrap_or(ch),
            _ => ch,
        };
        let mine = self.my_roles.get(&g).map(Vec::as_slice).unwrap_or(&[]);
        permissions::in_channel(base, g, self.me_id(), mine, &ch.overwrites)
    }

    /// Visibility filter for one server's channels: `None` when the
    /// permissions are not known (everything is shown), else the server-wide
    /// bits to apply each channel's overwrites to.
    fn view_base(&self, g: GuildId) -> Option<u64> {
        let guild = self.guild(g)?;
        permissions::known(guild).then(|| self.guild_permissions(g))
    }

    fn visible_with(&self, g: GuildId, base: Option<u64>, ch: &Channel) -> bool {
        base.is_none_or(|b| self.channel_bits(g, b, ch) & permissions::VIEW_CHANNEL != 0)
    }

    /// Whether the current user can see `c` (View Channel). DMs and
    /// channels of servers whose roles are not known count as visible.
    pub fn can_view(&self, c: ChannelId) -> bool {
        let Some(ch) = self.channels.get(&c) else {
            return false;
        };
        match ch.guild_id {
            Some(g) => self.visible_with(g, self.view_base(g), ch),
            None => true,
        }
    }

    /// Whether the current user may pin, unpin and suppress embeds in `c`:
    /// always in DMs; in servers with Manage Messages there (owner,
    /// Administrator, roles, channel overwrites).
    pub fn can_manage_messages(&self, c: ChannelId) -> bool {
        self.channel_permissions(c) & permissions::MANAGE_MESSAGES != 0
    }

    /// A channel of `g` where the current user may create an invite: `pref`
    /// when allowed there, else the first one in list order.
    pub fn invite_channel(&self, g: GuildId, pref: Option<ChannelId>) -> Option<ChannelId> {
        let ok = |c: &Channel| {
            c.guild_id == Some(g)
                && c.kind != ChannelKind::Category
                && c.kind != ChannelKind::Thread
                && self.channel_permissions(c.id)
                    & (permissions::CREATE_INSTANT_INVITE | permissions::VIEW_CHANNEL)
                    == permissions::CREATE_INSTANT_INVITE | permissions::VIEW_CHANNEL
        };
        if let Some(c) = pref.and_then(|c| self.channels.get(&c))
            && ok(c)
        {
            return Some(c.id);
        }
        self.channel_tree(g)
            .into_iter()
            .flat_map(|(_, kids)| kids)
            .find(|c| ok(c))
            .map(|c| c.id)
    }

    /// Whether the current user owns `g`.
    pub fn owns_guild(&self, g: GuildId) -> bool {
        let me = self.me_id();
        me.is_some() && self.guild(g).is_some_and(|guild| guild.owner_id == me)
    }

    /// Applies one gateway event. Returns effects for the app to run.
    pub fn apply_event(&mut self, ev: &Event, ctx: &Ctx) -> Vec<Effect> {
        let mut fx = Vec::new();
        match ev {
            Event::Ready(r) => self.load_ready(r),
            Event::MemberRoles(list) => {
                let me = self.me_id();
                for (g, u, roles) in list {
                    if Some(*u) == me {
                        self.my_roles.insert(*g, roles.clone());
                    }
                    self.member_roles.insert((*g, *u), roles.clone());
                }
            }
            Event::MessageCreate(m) => self.on_message_create(m, ctx, &mut fx),
            Event::MessageUpdate(p) => {
                if let Some(w) = self.messages.get_mut(&p.channel_id)
                    && let Some(m) = w.messages.iter_mut().find(|m| m.id == p.id)
                {
                    m.apply_patch(p);
                }
            }
            Event::MessageDelete { channel_id, id } => {
                if let Some(w) = self.messages.get_mut(channel_id) {
                    w.messages.retain(|m| m.id != *id);
                }
            }
            Event::MessageDeleteBulk { channel_id, ids } => {
                if let Some(w) = self.messages.get_mut(channel_id) {
                    w.messages.retain(|m| !ids.contains(&m.id));
                }
            }
            Event::TypingStart {
                channel_id,
                user_id,
                user,
                ..
            } => {
                if Some(*user_id) != self.me_id() {
                    if let Some(u) = user {
                        self.users.entry(u.id).or_insert_with(|| u.clone());
                    }
                    let list = self.typing.entry(*channel_id).or_default();
                    let until_ms = ctx.now_ms + TYPING_TTL_MS;
                    match list.iter_mut().find(|t| t.user_id == *user_id) {
                        Some(t) => t.until_ms = until_ms,
                        None => list.push(Typing {
                            user_id: *user_id,
                            until_ms,
                        }),
                    }
                }
            }
            Event::MessageAck {
                channel_id,
                message_id,
                mention_count,
            } => {
                let rs = self.read_states.entry(*channel_id).or_default();
                if rs.last_read.is_none_or(|l| *message_id >= l) {
                    rs.last_read = Some(*message_id);
                    rs.mention_count = *mention_count;
                }
            }
            Event::ChannelUpsert(c, users) => {
                for u in users {
                    self.users.insert(u.id, u.clone());
                }
                self.channels.insert(c.id, (**c).clone());
            }
            Event::ChannelDelete(id) => {
                self.channels.remove(id);
                self.messages.remove(id);
                self.read_states.remove(id);
            }
            Event::GuildUpsert {
                guild,
                channels,
                partial,
                ..
            } => {
                if !partial {
                    self.channels.retain(|_, c| c.guild_id != Some(guild.id));
                }
                for c in channels {
                    self.channels.insert(c.id, c.clone());
                }
                match self.guilds.iter_mut().find(|g| g.id == guild.id) {
                    Some(g) => *g = (**guild).clone(),
                    None => self.guilds.push((**guild).clone()),
                }
                self.folders
                    .sync(&self.guilds.iter().map(|g| g.id).collect::<Vec<_>>());
                self.resort_guilds();
            }
            Event::RoleUpsert { guild_id, role } => {
                if let Some(g) = self.guilds.iter_mut().find(|g| g.id == *guild_id) {
                    match g.roles.iter_mut().find(|r| r.id == role.id) {
                        Some(r) => *r = role.clone(),
                        None => g.roles.push(role.clone()),
                    }
                }
            }
            Event::RoleDelete { guild_id, role_id } => {
                if let Some(g) = self.guilds.iter_mut().find(|g| g.id == *guild_id) {
                    g.roles.retain(|r| r.id != *role_id);
                }
                if let Some(mine) = self.my_roles.get_mut(guild_id) {
                    mine.retain(|r| r != role_id);
                }
                for ((g, _), roles) in self.member_roles.iter_mut() {
                    if g == guild_id {
                        roles.retain(|r| r != role_id);
                    }
                }
            }
            Event::GuildFolders(list) => {
                self.folders = super::folders::Folders::from_settings(list, &[]);
                self.folders
                    .sync(&self.guilds.iter().map(|g| g.id).collect::<Vec<_>>());
                self.resort_guilds();
            }
            Event::NotifySettings { channels, .. } => {
                for (c, n) in channels {
                    self.set_channel_notify(*c, *n);
                }
            }
            Event::GuildDelete(id) => {
                self.guilds.retain(|g| g.id != *id);
                self.folders.forget(*id);
                let gone: Vec<ChannelId> = self
                    .channels
                    .values()
                    .filter(|c| c.guild_id == Some(*id))
                    .map(|c| c.id)
                    .collect();
                for c in gone {
                    self.channels.remove(&c);
                    self.messages.remove(&c);
                }
            }
            Event::Reaction(r) => self.apply_reaction(r),
            Event::PresenceUpdate {
                user_id,
                status,
                activities,
            } => {
                self.set_presence(*user_id, *status);
                self.set_activities(*user_id, activities.clone());
            }
            Event::MemberList(u) => {
                if let Some(l) = self.member_list.as_mut() {
                    l.apply(u);
                }
                for op in &u.ops {
                    match op {
                        MemberOp::Sync { items, .. } => {
                            for i in items {
                                self.learn_member(i);
                            }
                        }
                        MemberOp::Insert { item, .. } | MemberOp::Update { item, .. } => {
                            self.learn_member(item)
                        }
                        _ => {}
                    }
                }
            }
            Event::GuildEmojis { guild_id, emojis } => {
                if let Some(g) = self.guilds.iter_mut().find(|g| g.id == *guild_id) {
                    g.emojis.clone_from(emojis);
                }
            }
            Event::GuildStickers { guild_id, stickers } => {
                if let Some(g) = self.guilds.iter_mut().find(|g| g.id == *guild_id) {
                    g.stickers.clone_from(stickers);
                }
            }
            // Voice and calls are handled by `app::voice`.
            Event::VoiceState(_)
            | Event::VoiceServer(_)
            | Event::Call(_)
            | Event::CallDelete(_)
            | Event::Connection(_)
            | Event::Resumed
            | Event::AuthFailed
            | Event::Fatal(_) => {}
        }
        fx
    }

    fn learn_member(&mut self, item: &MemberItem) {
        if let MemberItem::Member(m) = item {
            // Newer payloads carry the current cosmetics: keep them fresh.
            match self.users.get_mut(&m.user.id) {
                Some(u) if *u != m.user => *u = m.user.clone(),
                Some(_) => {}
                None => {
                    self.users.insert(m.user.id, m.user.clone());
                }
            }
            self.set_presence(m.user.id, m.presence);
            self.set_activities(m.user.id, m.activities.clone());
        }
    }

    pub fn set_presence(&mut self, user: UserId, status: Presence) {
        if Some(user) == self.me_id() {
            // The own status is authoritative from settings/our setter;
            // presence echoes only refine online/idle/dnd.
            if status.is_online() {
                self.my_status = status;
            }
            return;
        }
        if self.presence.len() >= MAX_PRESENCES && !self.presence.contains_key(&user) {
            self.presence.clear();
        }
        self.presence.insert(user, status);
    }

    pub fn set_activities(&mut self, user: UserId, list: Vec<Activity>) {
        if list.is_empty() {
            self.activities.remove(&user);
            return;
        }
        if self.activities.len() >= MAX_PRESENCES && !self.activities.contains_key(&user) {
            self.activities.clear();
        }
        self.activities.insert(user, list);
    }

    pub fn activities_of(&self, user: UserId) -> &[Activity] {
        self.activities.get(&user).map_or(&[], Vec::as_slice)
    }

    /// Status of `user` for the dot next to their avatar.
    pub fn presence_of(&self, user: UserId) -> Presence {
        if Some(user) == self.me_id() {
            return self.my_status;
        }
        self.presence.get(&user).copied().unwrap_or_default()
    }

    // -- messages: lookup and local edits ------------------------------------

    pub fn message(&self, c: ChannelId, id: MessageId) -> Option<&Message> {
        self.messages.get(&c)?.get(id)
    }

    /// Reply header for `m`: the snapshot it came with, refreshed from the
    /// loaded window when the original is there (edits, deletions).
    pub fn reply_preview(&self, m: &Message) -> Option<ReplyPreview> {
        let base = m.referenced.as_deref()?;
        let Some(id) = base.message_id.or(m.reply_to) else {
            return Some(base.clone());
        };
        if let Some(orig) = self.message(m.channel_id, id) {
            return Some(ReplyPreview::from_message(orig));
        }
        Some(base.clone())
    }

    /// Applies an edit confirmed by the server.
    pub fn set_content(&mut self, c: ChannelId, id: MessageId, content: &str) {
        if let Some(m) = self.messages.get_mut(&c).and_then(|w| w.get_mut(id)) {
            m.content = content.to_owned();
            m.edited = true;
        }
    }

    /// Optimistically hides the embeds of a message (`SUPPRESS_EMBEDS`).
    /// Returns what to restore on failure and the new flags to send.
    pub fn suppress_embeds(
        &mut self,
        c: ChannelId,
        id: MessageId,
    ) -> Option<(Vec<Embed>, u64, u64)> {
        let m = self.messages.get_mut(&c).and_then(|w| w.get_mut(id))?;
        let (old_embeds, old_flags) = (std::mem::take(&mut m.embeds), m.flags);
        m.flags |= FLAG_SUPPRESS_EMBEDS;
        Some((old_embeds, old_flags, m.flags))
    }

    /// Undoes [`Self::suppress_embeds`] after a refusal.
    pub fn restore_embeds(&mut self, c: ChannelId, id: MessageId, embeds: Vec<Embed>, flags: u64) {
        if let Some(m) = self.messages.get_mut(&c).and_then(|w| w.get_mut(id)) {
            m.embeds = embeds;
            m.flags = flags;
        }
    }

    pub fn remove_message(&mut self, c: ChannelId, id: MessageId) {
        if let Some(w) = self.messages.get_mut(&c) {
            w.messages.retain(|m| m.id != id);
        }
    }

    // -- reactions -------------------------------------------------------------

    /// Toggles the current user's reaction locally (optimistic). Returns
    /// what happened, so the caller can send the matching request.
    pub fn toggle_reaction(
        &mut self,
        c: ChannelId,
        id: MessageId,
        emoji: &Emoji,
    ) -> Option<ReactionChange> {
        let m = self.messages.get_mut(&c)?.get_mut(id)?;
        match m.reactions.iter().position(|r| r.emoji == *emoji) {
            Some(i) if m.reactions[i].me => {
                let r = &mut m.reactions[i];
                r.me = false;
                r.count = r.count.saturating_sub(1);
                if r.count == 0 {
                    m.reactions.remove(i);
                }
                Some(ReactionChange::Removed)
            }
            Some(i) => {
                let r = &mut m.reactions[i];
                r.me = true;
                r.count += 1;
                Some(ReactionChange::Added)
            }
            None => {
                m.reactions.push(Reaction {
                    emoji: emoji.clone(),
                    count: 1,
                    me: true,
                });
                Some(ReactionChange::Added)
            }
        }
    }

    /// Reverts [`toggle_reaction`](Self::toggle_reaction) after a failed
    /// request.
    pub fn revert_reaction(
        &mut self,
        c: ChannelId,
        id: MessageId,
        emoji: &Emoji,
        change: ReactionChange,
    ) {
        match change {
            ReactionChange::Added => self.set_reaction(c, id, emoji, false),
            ReactionChange::Removed => self.set_reaction(c, id, emoji, true),
        }
    }

    /// Sets the current user's presence in a reaction, idempotently.
    fn set_reaction(&mut self, c: ChannelId, id: MessageId, emoji: &Emoji, me: bool) {
        let Some(m) = self.messages.get_mut(&c).and_then(|w| w.get_mut(id)) else {
            return;
        };
        let pos = m.reactions.iter().position(|r| r.emoji == *emoji);
        match (pos, me) {
            (Some(i), true) => {
                let r = &mut m.reactions[i];
                if !r.me {
                    r.me = true;
                    r.count += 1;
                }
            }
            (None, true) => m.reactions.push(Reaction {
                emoji: emoji.clone(),
                count: 1,
                me: true,
            }),
            (Some(i), false) => {
                let r = &mut m.reactions[i];
                if r.me {
                    r.me = false;
                    r.count = r.count.saturating_sub(1);
                }
                if r.count == 0 {
                    m.reactions.remove(i);
                }
            }
            (None, false) => {}
        }
    }

    /// Applies MESSAGE_REACTION_* from the gateway. Our own add/remove echo
    /// is idempotent with the optimistic update.
    pub fn apply_reaction(&mut self, ev: &ReactionEvent) {
        let me = self.me_id();
        match ev {
            ReactionEvent::Add {
                channel_id,
                message_id,
                user_id,
                emoji,
            } => {
                let Some(m) = self
                    .messages
                    .get_mut(channel_id)
                    .and_then(|w| w.get_mut(*message_id))
                else {
                    return;
                };
                let is_me = Some(*user_id) == me;
                match m.reactions.iter_mut().find(|r| r.emoji == *emoji) {
                    Some(r) => {
                        if is_me {
                            if !r.me {
                                r.me = true;
                                r.count += 1;
                            }
                        } else {
                            r.count += 1;
                        }
                    }
                    None => m.reactions.push(Reaction {
                        emoji: emoji.clone(),
                        count: 1,
                        me: is_me,
                    }),
                }
            }
            ReactionEvent::Remove {
                channel_id,
                message_id,
                user_id,
                emoji,
            } => {
                let is_me = Some(*user_id) == me;
                let Some(m) = self
                    .messages
                    .get_mut(channel_id)
                    .and_then(|w| w.get_mut(*message_id))
                else {
                    return;
                };
                let Some(i) = m.reactions.iter().position(|r| r.emoji == *emoji) else {
                    return;
                };
                let r = &mut m.reactions[i];
                if is_me {
                    // Already removed locally: nothing to do.
                    if r.me {
                        r.me = false;
                        r.count = r.count.saturating_sub(1);
                    }
                } else {
                    r.count = r.count.saturating_sub(1);
                }
                if m.reactions[i].count == 0 {
                    m.reactions.remove(i);
                }
            }
            ReactionEvent::RemoveAll {
                channel_id,
                message_id,
            } => {
                if let Some(m) = self
                    .messages
                    .get_mut(channel_id)
                    .and_then(|w| w.get_mut(*message_id))
                {
                    m.reactions.clear();
                }
            }
            ReactionEvent::RemoveEmoji {
                channel_id,
                message_id,
                emoji,
            } => {
                if let Some(m) = self
                    .messages
                    .get_mut(channel_id)
                    .and_then(|w| w.get_mut(*message_id))
                {
                    m.reactions.retain(|r| r.emoji != *emoji);
                }
            }
        }
    }

    fn on_message_create(&mut self, m: &Message, ctx: &Ctx, fx: &mut Vec<Effect>) {
        let me = self.me_id();
        self.users
            .entry(m.author.id)
            .and_modify(|u| *u = m.author.clone())
            .or_insert_with(|| m.author.clone());
        if let Some(g) = m.guild_id
            && !m.author_roles.is_empty()
        {
            self.member_roles
                .insert((g, m.author.id), m.author_roles.clone());
        }
        if let Some(c) = self.channels.get_mut(&m.channel_id)
            && c.last_message_id.is_none_or(|l| m.id > l)
        {
            c.last_message_id = Some(m.id);
        }
        if let Some(list) = self.typing.get_mut(&m.channel_id) {
            list.retain(|t| t.user_id != m.author.id);
        }
        // Only keep a window for channels that already have one (opened
        // before); others are fetched when opened.
        if let Some(w) = self.messages.get_mut(&m.channel_id)
            && (!w.detached || own_pending_echo(w, m))
        {
            w.upsert(m.clone());
            w.trim_front(MESSAGE_WINDOW_MAX);
        }

        let own = Some(m.author.id) == me;
        let viewing = ctx.open_channel == Some(m.channel_id) && ctx.window_focused;
        if own || viewing {
            let rs = self.read_states.entry(m.channel_id).or_default();
            rs.last_read = Some(m.id);
            rs.mention_count = 0;
            if viewing && !own {
                fx.push(Effect::Ack(m.channel_id, m.id));
            }
            return;
        }
        let muted = self.is_channel_muted(m.channel_id)
            || m.guild_id.is_some_and(|g| self.muted_guilds.contains(&g));
        // Like the official client: a direct @mention (or a DM) always
        // badges and notifies, even in a muted channel/server; @everyone,
        // @here and role mentions respect mutes and the per-server
        // "suppress" settings.
        let counted = match self.mention_kind(m) {
            MentionKind::None => false,
            MentionKind::Direct => true,
            MentionKind::Everyone => {
                !muted
                    && !m
                        .guild_id
                        .is_some_and(|g| self.suppress_everyone.contains(&g))
            }
            MentionKind::Role => {
                !muted && !m.guild_id.is_some_and(|g| self.suppress_roles.contains(&g))
            }
        };
        if counted {
            self.read_states
                .entry(m.channel_id)
                .or_default()
                .mention_count += 1;
            fx.push(Effect::Notify(self.notification_for(m, ctx)));
        }
    }

    fn is_private(&self, c: ChannelId) -> bool {
        self.channels.get(&c).is_some_and(|c| c.kind.is_private())
    }

    /// Whether `m` should ping the current user: direct mention, role
    /// mention, @everyone/@here, or any message in a DM.
    pub fn mentions_me(&self, m: &Message) -> bool {
        self.mention_kind(m) != MentionKind::None
    }

    /// How `m` mentions the current user (strongest kind wins).
    pub fn mention_kind(&self, m: &Message) -> MentionKind {
        let Some(me) = self.me_id() else {
            return MentionKind::None;
        };
        if m.author.id == me {
            return MentionKind::None;
        }
        if self.is_private(m.channel_id)
            || m.guild_id.is_none() && !self.channels.contains_key(&m.channel_id)
            || m.mentions.contains(&me)
        {
            return MentionKind::Direct;
        }
        if let Some(g) = m.guild_id
            && let Some(roles) = self.my_roles.get(&g)
            && m.mention_roles.iter().any(|r| roles.contains(r))
        {
            return MentionKind::Role;
        }
        if m.mention_everyone {
            return MentionKind::Everyone;
        }
        MentionKind::None
    }

    fn notification_for(&self, m: &Message, ctx: &Ctx) -> Notification {
        let author = m.author.display_name().to_owned();
        let title = match self.channels.get(&m.channel_id) {
            Some(c) if c.kind.is_private() => author.clone(),
            Some(c) => {
                let guild = m
                    .guild_id
                    .and_then(|g| self.guild(g))
                    .map(|g| g.name.as_str())
                    .unwrap_or("");
                format!(
                    "{author} (#{}, {guild})",
                    c.name.as_deref().unwrap_or("channel")
                )
            }
            None => author.clone(),
        };
        let body = if ctx.notification_preview {
            let blocks = super::markdown::parse(&m.content);
            let mut text = super::markdown::to_plain(&blocks, &|i| self.resolve_mention(i));
            if text.is_empty() && !m.attachments.is_empty() {
                text = "Sent an attachment".into();
            }
            if text.chars().count() > 200 {
                text = text.chars().take(200).collect::<String>() + "…";
            }
            text
        } else {
            "New message".into()
        };
        Notification {
            title,
            body,
            channel_id: m.channel_id,
            guild_id: m.guild_id,
        }
    }

    /// Text for mention nodes (`@name`, `#channel`, `@role`).
    pub fn resolve_mention(&self, i: &super::markdown::Inline) -> Option<String> {
        use super::markdown::Inline;
        match i {
            Inline::UserMention(id) => Some(format!(
                "@{}",
                self.user_name(*id).unwrap_or("unknown-user")
            )),
            Inline::ChannelMention(id) => Some(format!(
                "#{}",
                self.channels
                    .get(id)
                    .and_then(|c| c.name.as_deref())
                    .unwrap_or("unknown-channel")
            )),
            Inline::RoleMention(id) => Some(format!(
                "@{}",
                self.guilds
                    .iter()
                    .flat_map(|g| g.roles.iter())
                    .find(|r| r.id == *id)
                    .map(|r| r.name.as_str())
                    .unwrap_or("unknown-role")
            )),
            _ => None,
        }
    }

    pub fn role(&self, id: RoleId) -> Option<&Role> {
        self.guilds
            .iter()
            .flat_map(|g| g.roles.iter())
            .find(|r| r.id == id)
    }

    // -- read state queries ----------------------------------------------------

    pub fn is_unread(&self, c: ChannelId) -> bool {
        let Some(ch) = self.channels.get(&c) else {
            return false;
        };
        let Some(last) = ch.last_message_id else {
            return false;
        };
        match self.read_states.get(&c).and_then(|r| r.last_read) {
            Some(read) => last > read,
            // No read state: we cannot know what was read. Treat it as read
            // rather than lighting up every channel never opened; the next
            // message (or MESSAGE_ACK) creates a read state.
            None => false,
        }
    }

    /// Unread mentions plus unread DMs over every known channel: the number
    /// on the tray badge.
    pub fn attention_count(&self) -> u32 {
        self.read_states
            .iter()
            .filter(|(c, _)| self.channels.contains_key(c))
            .fold(0u32, |n, (_, r)| n.saturating_add(r.mention_count))
    }

    pub fn mention_count(&self, c: ChannelId) -> u32 {
        self.read_states.get(&c).map_or(0, |r| r.mention_count)
    }

    pub fn guild_badge(&self, g: GuildId) -> Badge {
        let muted_guild = self.muted_guilds.contains(&g);
        let mut b = Badge::default();
        let base = self.view_base(g);
        for c in self.channels.values().filter(|c| c.guild_id == Some(g)) {
            if !c.kind.is_textual() || !self.visible_with(g, base, c) {
                continue;
            }
            b.mentions += self.mention_count(c.id);
            if !muted_guild && !self.is_channel_muted(c.id) && self.is_unread(c.id) {
                b.unread = true;
            }
        }
        b
    }

    /// Unread DMs, newest first (shown as avatars under the Home button).
    pub fn unread_dms(&self) -> Vec<(&Channel, u32)> {
        self.dm_list()
            .into_iter()
            .filter_map(|c| {
                let n = self.mention_count(c.id);
                (n > 0).then_some((c, n))
            })
            .collect()
    }

    pub fn dm_badge(&self) -> Badge {
        let mentions = self.unread_dms().iter().map(|(_, n)| n).sum();
        Badge {
            unread: mentions > 0,
            mentions,
        }
    }

    /// Marks `c` read locally. Returns the message id to ACK on the server,
    /// if anything changed.
    pub fn mark_read(&mut self, c: ChannelId) -> Option<MessageId> {
        let last = self.channels.get(&c)?.last_message_id?;
        let rs = self.read_states.entry(c).or_default();
        if rs.last_read == Some(last) && rs.mention_count == 0 {
            return None;
        }
        rs.last_read = Some(last);
        rs.mention_count = 0;
        Some(last)
    }

    // -- lists -------------------------------------------------------------

    /// DMs and group DMs, most recent activity first.
    pub fn dm_list(&self) -> Vec<&Channel> {
        let mut v: Vec<&Channel> = self
            .channels
            .values()
            .filter(|c| c.kind.is_private())
            .collect();
        v.sort_by(|a, b| {
            b.last_message_id
                .unwrap_or(b.id)
                .cmp(&a.last_message_id.unwrap_or(a.id))
        });
        v
    }

    /// Channel list of a guild: uncategorised channels first, then each
    /// category with its children, all sorted like the official client
    /// (text before voice, then position, then id).
    pub fn channel_tree(&self, g: GuildId) -> Vec<(Option<&Channel>, Vec<&Channel>)> {
        let base = self.view_base(g);
        let in_guild: Vec<&Channel> = self
            .channels
            .values()
            .filter(|c| c.guild_id == Some(g) && c.kind != ChannelKind::Thread)
            .collect();
        let sort_key = |c: &&Channel| {
            let voice = matches!(c.kind, ChannelKind::Voice | ChannelKind::Stage);
            (voice, c.position, c.id)
        };
        let mut cats: Vec<&Channel> = in_guild
            .iter()
            .copied()
            .filter(|c| c.kind == ChannelKind::Category)
            .collect();
        cats.sort_by_key(|c| (c.position, c.id));
        let mut top: Vec<&Channel> = in_guild
            .iter()
            .copied()
            .filter(|c| {
                c.kind != ChannelKind::Category
                    && c.parent_id.is_none_or(|p| !cats.iter().any(|k| k.id == p))
                    && self.visible_with(g, base, c)
            })
            .collect();
        top.sort_by_key(sort_key);
        let mut out = vec![(None, top)];
        for cat in cats {
            let mut kids: Vec<&Channel> = in_guild
                .iter()
                .copied()
                .filter(|c| c.parent_id == Some(cat.id) && c.kind != ChannelKind::Category)
                .filter(|c| self.visible_with(g, base, c))
                .collect();
            // Like the official client: a category shows while it holds a
            // visible channel or can be seen itself.
            if kids.is_empty() && !self.visible_with(g, base, cat) {
                continue;
            }
            kids.sort_by_key(sort_key);
            out.push((Some(cat), kids));
        }
        out
    }

    /// First text channel of a guild (default selection).
    pub fn default_channel(&self, g: GuildId) -> Option<ChannelId> {
        self.channel_tree(g)
            .into_iter()
            .flat_map(|(_, kids)| kids)
            .find(|c| c.kind.is_textual())
            .map(|c| c.id)
    }

    /// Author name colour: colour of the highest role that has one.
    pub fn author_color(&self, guild: Option<GuildId>, m: &Message) -> Option<u32> {
        let g = self.guild(guild?)?;
        let roles = if m.author_roles.is_empty() {
            self.member_roles.get(&(g.id, m.author.id))?
        } else {
            &m.author_roles
        };
        g.roles
            .iter()
            .filter(|r| r.color != 0 && roles.contains(&r.id))
            .max_by_key(|r| r.position)
            .map(|r| r.color)
    }

    /// Names of users currently typing in `c`.
    pub fn typing_names(&self, c: ChannelId, now_ms: u64) -> Vec<String> {
        self.typing
            .get(&c)
            .map(|list| {
                list.iter()
                    .filter(|t| t.until_ms > now_ms)
                    .map(|t| self.user_name(t.user_id).unwrap_or("Someone").to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Drops expired typing entries. Returns true if anything is left.
    pub fn prune_typing(&mut self, now_ms: u64) -> bool {
        self.typing.retain(|_, list| {
            list.retain(|t| t.until_ms > now_ms);
            !list.is_empty()
        });
        !self.typing.is_empty()
    }

    // -- history -----------------------------------------------------------

    /// Merges a page of history. `older_page` means it was loaded by
    /// scrolling up (`before` = oldest known); `full` that the server
    /// returned a full page (so older messages may exist).
    pub fn insert_history(
        &mut self,
        c: ChannelId,
        msgs: Vec<Message>,
        older_page: bool,
        full: bool,
    ) {
        for m in &msgs {
            self.users
                .entry(m.author.id)
                .or_insert_with(|| m.author.clone());
        }
        let w = self.messages.entry(c).or_default();
        w.loading = false;
        if older_page {
            if !full {
                w.reached_start = true;
            }
        } else if !full && w.messages.is_empty() {
            w.reached_start = true;
        }
        for m in msgs {
            w.upsert(m);
        }
        if older_page {
            w.trim_back(MESSAGE_WINDOW_MAX);
        } else {
            w.trim_front(MESSAGE_WINDOW_MAX);
        }
    }

    /// Shrinks a window that is no longer visible back to its newest
    /// `MESSAGE_WINDOW` messages.
    pub fn trim_inactive(&mut self, c: ChannelId) {
        if let Some(w) = self.messages.get_mut(&c) {
            w.trim_front(MESSAGE_WINDOW);
        }
    }

    /// Adds an optimistic local message.
    pub fn push_pending(
        &mut self,
        c: ChannelId,
        content: String,
        nonce: String,
        now_ms: u64,
        reply: Option<MessageId>,
    ) -> Option<Message> {
        let me = self.me.clone()?;
        let referenced = reply.map(|id| {
            Box::new(
                self.message(c, id)
                    .map(ReplyPreview::from_message)
                    .unwrap_or_else(|| ReplyPreview::deleted(Some(id))),
            )
        });
        let guild_id = self.channels.get(&c).and_then(|c| c.guild_id);
        let author_roles = guild_id
            .and_then(|g| self.my_roles.get(&g).cloned())
            .unwrap_or_default();
        let m = Message {
            id: Snowflake::from_timestamp_ms(now_ms, fastrand::u64(0..4096)),
            channel_id: c,
            guild_id,
            author: me,
            content,
            edited: false,
            attachments: vec![],
            embeds: vec![],
            flags: 0,
            stickers: vec![],
            mentions: vec![],
            mention_roles: vec![],
            mention_everyone: false,
            author_roles,
            reply_to: reply,
            referenced,
            reactions: vec![],
            nonce: Some(nonce),
            kind: if reply.is_some() { 19 } else { 0 },
            pending: true,
            failed: false,
            call: None,
        };
        self.messages.entry(c).or_default().upsert(m.clone());
        Some(m)
    }

    pub fn mark_failed(&mut self, c: ChannelId, nonce: &str) {
        if let Some(w) = self.messages.get_mut(&c) {
            for m in w.messages.iter_mut() {
                if m.nonce.as_deref() == Some(nonce) && m.pending {
                    m.pending = false;
                    m.failed = true;
                }
            }
        }
    }
}

/// A server echo of one of our pending messages is always applied.
fn own_pending_echo(w: &MessageWindow, m: &Message) -> bool {
    m.nonce.as_ref().is_some_and(|n| {
        w.messages
            .iter()
            .any(|p| p.pending && p.nonce.as_ref() == Some(n))
    })
}

/// Whether message `b` continues the group started by `a` (same author,
/// within 7 minutes, no reply/system message).
pub fn continues_group(a: &Message, b: &Message) -> bool {
    a.author.id == b.author.id
        && b.reply_to.is_none()
        && b.kind == 0
        && matches!(a.kind, 0 | 19)
        && b.timestamp_ms().saturating_sub(a.timestamp_ms()) < GROUP_WINDOW_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;

    fn ctx() -> Ctx {
        Ctx {
            now_ms: 1_700_000_000_000,
            open_channel: None,
            window_focused: true,
            notification_preview: true,
        }
    }

    #[test]
    fn attention_count_is_unread_mentions_and_dms() {
        let mut s = loaded();
        let expected: u32 = s
            .read_states
            .iter()
            .filter(|(c, _)| s.channels.contains_key(c))
            .map(|(_, r)| r.mention_count)
            .sum();
        assert!(expected > 0, "mock data has unread mentions");
        assert_eq!(s.attention_count(), expected);
        // Reading a channel removes its mentions from the badge.
        let (c, n) = s
            .read_states
            .iter()
            .find(|(_, r)| r.mention_count > 0)
            .map(|(c, r)| (*c, r.mention_count))
            .unwrap();
        s.read_states.get_mut(&c).unwrap().mention_count = 0;
        assert_eq!(s.attention_count(), expected - n);
        // Read states of channels we no longer know do not count.
        s.channels.clear();
        assert_eq!(s.attention_count(), 0);
    }

    #[test]
    fn dm_participants_lists_recipients_and_me_in_groups() {
        let s = loaded();
        let ids = |c: ChannelId| -> Vec<UserId> {
            s.dm_participants(&s.channels[&c])
                .iter()
                .map(|u| u.id)
                .collect()
        };
        // 1:1 DM: just the other person.
        assert_eq!(ids(mock::DM_ALICE), s.channels[&mock::DM_ALICE].recipients);
        // Group DM: everyone, including me, sorted by display name.
        let group = s.dm_participants(&s.channels[&mock::DM_GROUP]);
        assert_eq!(
            group.len(),
            s.channels[&mock::DM_GROUP].recipients.len() + 1
        );
        assert!(group.iter().any(|u| u.id == mock::ME));
        let names: Vec<String> = group
            .iter()
            .map(|u| u.display_name().to_lowercase())
            .collect();
        assert!(names.windows(2).all(|w| w[0] <= w[1]), "{names:?}");
    }

    fn loaded() -> ChatState {
        let mut s = ChatState::default();
        s.load_ready(&mock::ready());
        s
    }

    fn msg_in(s: &ChatState, ch: ChannelId, author: User, content: &str) -> Message {
        let mut m = mock::text_message(
            ch,
            Snowflake::from_timestamp_ms(1_800_000_000_000, 1),
            author,
            content,
        );
        m.guild_id = s.channels.get(&ch).and_then(|c| c.guild_id);
        m
    }

    #[test]
    fn ready_orders_guilds_by_settings() {
        let s = loaded();
        let r = mock::ready();
        assert_eq!(s.guilds.first().unwrap().id, r.guild_order[0]);
        assert_eq!(s.guilds.len(), r.guilds.len());
    }

    #[test]
    fn new_message_in_other_channel_marks_unread() {
        let mut s = loaded();
        let ch = mock::CH_RANDOM;
        s.mark_read(ch);
        assert!(!s.is_unread(ch));
        let m = msg_in(&s, ch, mock::user(2), "hello");
        let fx = s.apply_event(&Event::MessageCreate(Box::new(m)), &ctx());
        assert!(fx.is_empty(), "plain message: no notification");
        assert!(s.is_unread(ch));
        assert_eq!(s.mention_count(ch), 0);
        assert!(s.guild_badge(mock::GUILD_RUST).unread);
    }

    #[test]
    fn mention_increments_and_notifies() {
        let mut s = loaded();
        let ch = mock::CH_RANDOM;
        s.mark_read(ch);
        let mut m = msg_in(&s, ch, mock::user(2), "hey <@1>");
        m.mentions = vec![mock::ME];
        let fx = s.apply_event(&Event::MessageCreate(Box::new(m)), &ctx());
        assert_eq!(s.mention_count(ch), 1);
        assert!(matches!(&fx[0], Effect::Notify(n) if n.body.contains("hey @")));
        assert!(s.guild_badge(mock::GUILD_RUST).mentions >= 1);
    }

    #[test]
    fn role_and_everyone_mentions() {
        let mut s = loaded();
        let ch = mock::CH_RANDOM;
        let mut m = msg_in(&s, ch, mock::user(2), "@everyone");
        m.mention_everyone = true;
        assert!(s.mentions_me(&m));
        let mut m = msg_in(&s, ch, mock::user(2), "<@&900>");
        m.mention_roles = vec![mock::ROLE_MOD];
        s.my_roles.insert(mock::GUILD_RUST, vec![mock::ROLE_MOD]);
        assert!(s.mentions_me(&m));
        s.my_roles.insert(mock::GUILD_RUST, vec![]);
        assert!(!s.mentions_me(&m));
    }

    #[test]
    fn dm_always_counts_as_mention() {
        let mut s = loaded();
        let dm = mock::DM_ALICE;
        s.mark_read(dm);
        let m = msg_in(&s, dm, mock::user(2), "psst");
        let fx = s.apply_event(&Event::MessageCreate(Box::new(m)), &ctx());
        assert_eq!(s.mention_count(dm), 1);
        assert!(s.dm_badge().mentions >= 1);
        assert!(matches!(&fx[0], Effect::Notify(n) if n.body == "psst"));
    }

    #[test]
    fn notification_preview_can_be_disabled() {
        let mut s = loaded();
        let m = msg_in(&s, mock::DM_ALICE, mock::user(2), "secret");
        let mut c = ctx();
        c.notification_preview = false;
        let fx = s.apply_event(&Event::MessageCreate(Box::new(m)), &c);
        assert!(matches!(&fx[0], Effect::Notify(n) if n.body == "New message"));
    }

    #[test]
    fn viewing_channel_acks_instead_of_counting() {
        let mut s = loaded();
        let ch = mock::CH_GENERAL;
        s.messages.entry(ch).or_default();
        let mut m = msg_in(&s, ch, mock::user(2), "<@1>");
        m.mentions = vec![mock::ME];
        let id = m.id;
        let mut c = ctx();
        c.open_channel = Some(ch);
        let fx = s.apply_event(&Event::MessageCreate(Box::new(m)), &c);
        assert_eq!(fx, vec![Effect::Ack(ch, id)]);
        assert!(!s.is_unread(ch));
        assert_eq!(s.mention_count(ch), 0);
        assert_eq!(s.messages[&ch].messages.back().unwrap().id, id);
    }

    #[test]
    fn unfocused_window_still_notifies() {
        let mut s = loaded();
        let m = msg_in(&s, mock::DM_ALICE, mock::user(2), "hi");
        let mut c = ctx();
        c.open_channel = Some(mock::DM_ALICE);
        c.window_focused = false;
        let fx = s.apply_event(&Event::MessageCreate(Box::new(m)), &c);
        assert!(matches!(fx[0], Effect::Notify(_)));
    }

    #[test]
    fn muted_guild_does_not_notify_or_show_unread() {
        let mut s = loaded();
        s.muted_guilds.insert(mock::GUILD_RUST);
        let mut m = msg_in(&s, mock::CH_RANDOM, mock::user(2), "@everyone");
        m.mention_everyone = true;
        let fx = s.apply_event(&Event::MessageCreate(Box::new(m)), &ctx());
        assert!(fx.is_empty());
        assert!(!s.guild_badge(mock::GUILD_RUST).unread);
        // @everyone in a muted server does not add a badge...
        let before = s.mention_count(mock::CH_RANDOM);
        assert_eq!(before, 0);
        // ...but a direct @mention still does, and notifies.
        let mut m = msg_in(&s, mock::CH_RANDOM, mock::user(2), "<@1>");
        m.mentions = vec![mock::ME];
        let fx = s.apply_event(&Event::MessageCreate(Box::new(m)), &ctx());
        assert_eq!(s.mention_count(mock::CH_RANDOM), 1);
        assert!(matches!(fx[0], Effect::Notify(_)));
    }

    #[test]
    fn muted_channel_role_mentions_do_not_badge() {
        let mut s = loaded();
        s.muted_channels.insert(mock::CH_RANDOM);
        s.my_roles.insert(mock::GUILD_RUST, vec![mock::ROLE_MOD]);
        let mut m = msg_in(&s, mock::CH_RANDOM, mock::user(2), "<@&900>");
        m.mention_roles = vec![mock::ROLE_MOD];
        assert_eq!(s.mention_kind(&m), MentionKind::Role);
        let fx = s.apply_event(&Event::MessageCreate(Box::new(m)), &ctx());
        assert!(fx.is_empty());
        assert_eq!(s.mention_count(mock::CH_RANDOM), 0);
    }

    #[test]
    fn suppress_settings_are_respected() {
        let mut s = loaded();
        s.suppress_everyone.insert(mock::GUILD_RUST);
        let mut m = msg_in(&s, mock::CH_GENERAL, mock::user(2), "@everyone");
        m.mention_everyone = true;
        let fx = s.apply_event(&Event::MessageCreate(Box::new(m)), &ctx());
        assert!(fx.is_empty());
        assert_eq!(s.mention_count(mock::CH_GENERAL), 0);
        s.suppress_roles.insert(mock::GUILD_RUST);
        s.my_roles.insert(mock::GUILD_RUST, vec![mock::ROLE_MOD]);
        let mut m = msg_in(&s, mock::CH_GENERAL, mock::user(2), "<@&900>");
        m.mention_roles = vec![mock::ROLE_MOD];
        assert!(
            s.apply_event(&Event::MessageCreate(Box::new(m)), &ctx())
                .is_empty()
        );
    }

    #[test]
    fn channel_without_read_state_is_not_unread() {
        let mut s = loaded();
        s.read_states.remove(&mock::CH_GENERAL);
        assert!(!s.is_unread(mock::CH_GENERAL));
    }

    #[test]
    fn own_message_marks_read() {
        let mut s = loaded();
        let ch = mock::CH_RANDOM;
        let m = msg_in(&s, ch, s.me.clone().unwrap(), "mine");
        s.apply_event(&Event::MessageCreate(Box::new(m)), &ctx());
        assert!(!s.is_unread(ch));
    }

    #[test]
    fn ack_event_updates_read_state() {
        let mut s = loaded();
        let ch = mock::CH_RANDOM;
        let last = s.channels[&ch].last_message_id.unwrap();
        s.apply_event(
            &Event::MessageAck {
                channel_id: ch,
                message_id: last,
                mention_count: 0,
            },
            &ctx(),
        );
        assert!(!s.is_unread(ch));
        // An older ACK does not move the marker back.
        s.apply_event(
            &Event::MessageAck {
                channel_id: ch,
                message_id: Snowflake(1),
                mention_count: 5,
            },
            &ctx(),
        );
        assert!(!s.is_unread(ch));
        assert_eq!(s.mention_count(ch), 0);
    }

    #[test]
    fn mark_read_returns_ack_target_once() {
        let mut s = loaded();
        let ch = mock::CH_ANNOUNCEMENTS;
        assert!(s.is_unread(ch));
        let id = s.mark_read(ch);
        assert_eq!(id, s.channels[&ch].last_message_id);
        assert_eq!(s.mark_read(ch), None);
    }

    #[test]
    fn typing_expires_and_is_cleared_by_message() {
        let mut s = loaded();
        let ch = mock::CH_GENERAL;
        let c = ctx();
        s.apply_event(
            &Event::TypingStart {
                channel_id: ch,
                guild_id: None,
                user_id: mock::user(2).id,
                timestamp_s: 0,
                user: None,
            },
            &c,
        );
        assert_eq!(s.typing_names(ch, c.now_ms).len(), 1);
        assert!(s.typing_names(ch, c.now_ms + TYPING_TTL_MS + 1).is_empty());
        let m = msg_in(&s, ch, mock::user(2), "done typing");
        s.apply_event(&Event::MessageCreate(Box::new(m)), &c);
        assert!(s.typing_names(ch, c.now_ms).is_empty());
        assert!(!s.prune_typing(c.now_ms));
    }

    #[test]
    fn own_typing_is_ignored() {
        let mut s = loaded();
        s.apply_event(
            &Event::TypingStart {
                channel_id: mock::CH_GENERAL,
                guild_id: None,
                user_id: mock::ME,
                timestamp_s: 0,
                user: None,
            },
            &ctx(),
        );
        assert!(s.typing.is_empty());
    }

    #[test]
    fn pending_message_replaced_by_server_echo() {
        let mut s = loaded();
        let ch = mock::CH_GENERAL;
        let p = s
            .push_pending(ch, "hi".into(), "n1".into(), ctx().now_ms, None)
            .unwrap();
        assert!(s.messages[&ch].messages.back().unwrap().pending);
        let mut echo = msg_in(&s, ch, s.me.clone().unwrap(), "hi");
        echo.nonce = Some("n1".into());
        s.apply_event(&Event::MessageCreate(Box::new(echo.clone())), &ctx());
        let w = &s.messages[&ch];
        assert!(w.messages.iter().all(|m| m.id != p.id));
        assert_eq!(w.messages.iter().filter(|m| m.id == echo.id).count(), 1);
    }

    #[test]
    fn failed_send_is_marked() {
        let mut s = loaded();
        let ch = mock::CH_GENERAL;
        s.push_pending(ch, "x".into(), "n2".into(), 0, None);
        s.mark_failed(ch, "n2");
        assert!(s.messages[&ch].messages.back().unwrap().failed);
    }

    #[test]
    fn update_and_delete() {
        let mut s = loaded();
        let ch = mock::CH_GENERAL;
        s.insert_history(ch, mock::history(ch), false, true);
        let target = s.messages[&ch].messages[0].id;
        s.apply_event(
            &Event::MessageUpdate(Box::new(MessagePatch {
                id: target,
                channel_id: ch,
                content: Some("edited!".into()),
                edited: true,
                ..Default::default()
            })),
            &ctx(),
        );
        let m = &s.messages[&ch].messages[0];
        assert_eq!(m.content, "edited!");
        assert!(m.edited);
        s.apply_event(
            &Event::MessageDelete {
                channel_id: ch,
                id: target,
            },
            &ctx(),
        );
        assert!(s.messages[&ch].messages.iter().all(|m| m.id != target));
    }

    #[test]
    fn window_is_bounded() {
        let mut s = loaded();
        let ch = mock::CH_GENERAL;
        let author = mock::user(2);
        let page: Vec<Message> = (0..MESSAGE_WINDOW_MAX as u64 + 50)
            .map(|i| mock::text_message(ch, Snowflake(10_000 + i), author.clone(), "x"))
            .collect();
        s.insert_history(ch, page, false, true);
        assert_eq!(s.messages[&ch].messages.len(), MESSAGE_WINDOW_MAX);
        s.trim_inactive(ch);
        assert_eq!(s.messages[&ch].messages.len(), MESSAGE_WINDOW);
        // The newest are kept.
        assert_eq!(
            s.messages[&ch].newest(),
            Some(Snowflake(10_000 + MESSAGE_WINDOW_MAX as u64 + 49))
        );
    }

    #[test]
    fn older_page_partial_reaches_start() {
        let mut s = loaded();
        let ch = mock::CH_GENERAL;
        s.insert_history(ch, mock::history(ch), false, true);
        assert!(!s.messages[&ch].reached_start);
        s.insert_history(ch, vec![], true, false);
        assert!(s.messages[&ch].reached_start);
    }

    #[test]
    fn channel_tree_groups_by_category() {
        let s = loaded();
        let tree = s.channel_tree(mock::GUILD_RUST);
        // Uncategorised group first, then categories by position.
        assert!(tree[0].0.is_none());
        assert_eq!(tree[1].0.unwrap().name.as_deref(), Some("Information"));
        for (_, kids) in &tree {
            let voice_pos = kids.iter().position(|c| c.kind == ChannelKind::Voice);
            if let Some(v) = voice_pos {
                assert!(kids[v..].iter().all(|c| c.kind == ChannelKind::Voice));
            }
        }
        assert!(s.default_channel(mock::GUILD_RUST).is_some());
    }

    #[test]
    fn author_color_uses_highest_coloured_role() {
        let s = loaded();
        let mut m = mock::text_message(mock::CH_GENERAL, Snowflake(5), mock::user(3), "x");
        m.author_roles = vec![mock::ROLE_MEMBER, mock::ROLE_MOD];
        assert_eq!(
            s.author_color(Some(mock::GUILD_RUST), &m),
            Some(mock::MOD_COLOR)
        );
        m.author_roles = vec![];
        assert_eq!(s.author_color(Some(mock::GUILD_RUST), &m), None);
    }

    #[test]
    fn grouping() {
        let a = mock::text_message(
            Snowflake(1),
            Snowflake::from_timestamp_ms(1_700_000_000_000, 0),
            mock::user(2),
            "a",
        );
        let mut b = mock::text_message(
            Snowflake(1),
            Snowflake::from_timestamp_ms(1_700_000_060_000, 0),
            mock::user(2),
            "b",
        );
        assert!(continues_group(&a, &b));
        b.id = Snowflake::from_timestamp_ms(1_700_000_000_000 + GROUP_WINDOW_MS + 1, 0);
        assert!(!continues_group(&a, &b));
        let c = mock::text_message(
            Snowflake(1),
            Snowflake::from_timestamp_ms(1_700_000_010_000, 0),
            mock::user(3),
            "c",
        );
        assert!(!continues_group(&a, &c));
    }

    #[test]
    fn guild_and_channel_lifecycle() {
        let mut s = loaded();
        let g = Guild {
            id: Snowflake(777),
            name: "New".into(),
            icon: None,
            roles: vec![],
            owner_id: None,
            emojis: vec![],
            stickers: vec![],
            premium_tier: 0,
        };
        let ch = Channel {
            id: Snowflake(778),
            kind: ChannelKind::Text,
            guild_id: Some(g.id),
            name: Some("hello".into()),
            topic: None,
            position: 0,
            parent_id: None,
            last_message_id: None,
            recipients: vec![],
            nsfw: false,
            overwrites: vec![],
        };
        s.apply_event(
            &Event::GuildUpsert {
                guild: Box::new(g.clone()),
                channels: vec![ch.clone()],
                partial: false,
                voice_states: vec![],
            },
            &ctx(),
        );
        assert!(s.guild(g.id).is_some());
        assert!(s.channels.contains_key(&ch.id));
        s.apply_event(&Event::GuildDelete(g.id), &ctx());
        assert!(s.guild(g.id).is_none());
        assert!(!s.channels.contains_key(&ch.id));
    }

    // -- notification overrides, permissions, folders -----------------------------

    #[test]
    fn channel_notify_mirrors_mutes_and_forgets_defaults() {
        use crate::net::extras::{MuteDuration, NotifyLevel};
        let mut s = loaded();
        let c = mock::CH_RANDOM;
        let now = crate::mock::now_ms();
        s.set_channel_notify(c, MuteDuration::Hour1.apply(NotifyLevel::Mentions, now));
        assert!(s.is_channel_muted(c));
        assert_eq!(s.channel_notify(c).level, NotifyLevel::Mentions);
        // An expired timed mute no longer counts.
        s.set_channel_notify(
            c,
            ChannelNotify {
                level: NotifyLevel::Default,
                muted: true,
                mute_end_ms: Some(now.saturating_sub(1)),
            },
        );
        assert!(!s.is_channel_muted(c));
        s.set_channel_notify(c, ChannelNotify::default());
        assert!(s.channel_notify.is_empty());
        assert!(!s.muted_channels.contains(&c));
    }

    #[test]
    fn notify_settings_events_update_state() {
        use crate::net::extras::{MuteDuration, NotifyLevel};
        let mut s = loaded();
        let n = MuteDuration::Forever.apply(NotifyLevel::Nothing, 0);
        s.apply_event(
            &Event::NotifySettings {
                guild: Some(mock::GUILD_RUST),
                channels: vec![(mock::CH_HELP, n)],
            },
            &ctx(),
        );
        assert!(s.is_channel_muted(mock::CH_HELP));
        assert_eq!(s.channel_notify(mock::CH_HELP).level, NotifyLevel::Nothing);
    }

    #[test]
    fn manage_messages_depends_on_roles_owner_and_dms() {
        let mut s = loaded();
        // DMs: always.
        assert!(s.can_manage_messages(mock::DM_ALICE));
        // Server channel, plain member: no.
        assert!(!s.can_manage_messages(mock::CH_GENERAL));
        // A role with Manage Messages.
        if let Some(g) = s.guilds.iter_mut().find(|g| g.id == mock::GUILD_RUST)
            && let Some(r) = g.roles.iter_mut().find(|r| r.id == mock::ROLE_MEMBER)
        {
            r.permissions = 1 << 13;
        }
        assert!(s.can_manage_messages(mock::CH_GENERAL));
        // Owner.
        let mut s = loaded();
        if let Some(g) = s.guilds.iter_mut().find(|g| g.id == mock::GUILD_RUST) {
            g.owner_id = s.me.as_ref().map(|u| u.id);
        }
        assert!(s.can_manage_messages(mock::CH_GENERAL));
        // Unknown channel.
        assert!(!s.can_manage_messages(Snowflake(999_999)));
    }

    #[test]
    fn folders_follow_ready_and_guild_events() {
        let mut r = mock::ready();
        r.guild_folders = vec![
            GuildFolder {
                id: Some(5),
                name: Some("Dev".into()),
                color: Some(0xff0000),
                guild_ids: vec![mock::GUILD_ICED, mock::GUILD_RUST],
            },
            GuildFolder::single(mock::GUILD_CAFE),
        ];
        let mut s = ChatState::default();
        s.load_ready(&r);
        // Folder members first, then the single, then servers the settings
        // did not mention (appended).
        let order: Vec<GuildId> = s.guilds.iter().map(|g| g.id).collect();
        assert_eq!(
            order,
            vec![
                mock::GUILD_ICED,
                mock::GUILD_RUST,
                mock::GUILD_CAFE,
                mock::GUILD_LINUX,
                mock::GUILD_GAMES
            ]
        );
        assert_eq!(s.folders.folder(5).unwrap().name.as_deref(), Some("Dev"));
        // Leaving a server removes it from its folder too.
        s.apply_event(&Event::GuildDelete(mock::GUILD_RUST), &ctx());
        assert_eq!(
            s.folders.folder(5).unwrap().guild_ids,
            vec![mock::GUILD_ICED]
        );
        // Another session changes the folders.
        s.apply_event(
            &Event::GuildFolders(vec![GuildFolder::single(mock::GUILD_GAMES)]),
            &ctx(),
        );
        assert_eq!(s.guilds.first().map(|g| g.id), Some(mock::GUILD_GAMES));
    }

    #[test]
    fn hidden_channels_are_filtered_by_permissions() {
        use super::permissions::{MANAGE_MESSAGES, VIEW_CHANNEL};
        let mut s = loaded();
        let g = mock::GUILD_GAMES;
        // The mock user owns Game Night; no @everyone role is known there,
        // so everything is visible.
        assert!(s.owns_guild(g));
        assert_eq!(
            s.channel_tree(g)
                .iter()
                .map(|(_, k)| k.len())
                .sum::<usize>(),
            3
        );
        // Another owner, @everyone without View Channel, one channel shown
        // to a role the user has.
        let me = s.me_id();
        let role = Snowflake(4900);
        if let Some(guild) = s.guilds.iter_mut().find(|x| x.id == g) {
            guild.owner_id = Some(Snowflake(3));
            guild.roles = vec![
                Role {
                    id: g,
                    name: "@everyone".into(),
                    color: 0,
                    position: 0,
                    permissions: 0,
                },
                Role {
                    id: role,
                    name: "Players".into(),
                    color: 0,
                    position: 1,
                    permissions: 0,
                },
            ];
        }
        s.my_roles.insert(g, vec![role]);
        let shown = Snowflake(412);
        if let Some(c) = s.channels.get_mut(&shown) {
            c.overwrites = vec![PermissionOverwrite {
                id: role,
                member: false,
                allow: VIEW_CHANNEL | MANAGE_MESSAGES,
                deny: 0,
            }];
        }
        let tree = s.channel_tree(g);
        let ids: Vec<ChannelId> = tree
            .iter()
            .flat_map(|(_, k)| k.iter().map(|c| c.id))
            .collect();
        assert_eq!(ids, vec![shown]);
        // The category holds a visible channel, so it stays.
        assert!(tree.iter().any(|(c, _)| c.is_some()));
        assert!(s.can_view(shown));
        assert!(!s.can_view(Snowflake(411)));
        assert!(s.can_manage_messages(shown));
        assert!(!s.can_manage_messages(Snowflake(411)));
        // Hidden channels do not light the server up.
        s.read_states.insert(
            Snowflake(411),
            ReadState {
                last_read: None,
                mention_count: 4,
            },
        );
        assert_eq!(s.guild_badge(g).mentions, 0);
        // A member overwrite denying it hides it again.
        if let (Some(c), Some(me)) = (s.channels.get_mut(&shown), me) {
            c.overwrites.push(PermissionOverwrite {
                id: me,
                member: true,
                allow: 0,
                deny: VIEW_CHANNEL,
            });
        }
        assert!(
            s.channel_tree(g)
                .iter()
                .all(|(c, k)| c.is_none() && k.is_empty())
        );
        // Role events keep the roles current.
        s.apply_event(
            &Event::RoleDelete {
                guild_id: g,
                role_id: role,
            },
            &Ctx::default(),
        );
        assert!(s.my_roles[&g].is_empty());
        assert_eq!(s.guild(g).map(|x| x.roles.len()), Some(1));
        s.apply_event(
            &Event::RoleUpsert {
                guild_id: g,
                role: Role {
                    id: g,
                    name: "@everyone".into(),
                    color: 0,
                    position: 0,
                    permissions: VIEW_CHANNEL,
                },
            },
            &Ctx::default(),
        );
        assert_eq!(s.guild_permissions(g), VIEW_CHANNEL);
        assert!(s.can_view(Snowflake(411)));
        assert_eq!(s.invite_channel(g, None), None);
    }
}
