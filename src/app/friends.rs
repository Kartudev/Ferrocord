//! Home page: Friends (Online / All / Implicit / Pending / Add Friend),
//! Message Requests and the "Find or start a conversation" switcher.
//!
//! Relationships come from READY and the `RELATIONSHIP_*` events
//! (`ChatState::relationships`); every action here is one REST call whose
//! result is mirrored locally on success (the gateway echo is idempotent).

use std::collections::HashSet;

use iced::Task;

use super::{App, Done, Message, Ui, View};
use crate::net::Event;
use crate::net::model::*;

/// Which page the Home view shows when no DM is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomePage {
    #[default]
    Friends,
    Requests,
}

/// Tabs of the Friends page, in header order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FriendsTab {
    #[default]
    Online,
    All,
    Implicit,
    Pending,
    AddFriend,
}

impl FriendsTab {
    pub const LIST: [FriendsTab; 4] = [Self::Online, Self::All, Self::Implicit, Self::Pending];

    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::All => "All",
            Self::Implicit => "Implicit",
            Self::Pending => "Pending",
            Self::AddFriend => "Add Friend",
        }
    }
}

/// Result of the "Add Friend" form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddStatus {
    Sending,
    Sent(String),
    Failed(String),
}

/// What a switcher result opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchTarget {
    Channel(ChannelId),
    /// A friend without an open DM: opens one.
    User(UserId),
    Guild(GuildId),
}

/// One row of the switcher.
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchHit {
    pub target: SwitchTarget,
    pub label: String,
    /// Server name of a channel, username of a person.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Switcher {
    pub query: String,
}

#[derive(Debug, Clone, Default)]
pub struct FriendsState {
    pub page: HomePage,
    pub tab: FriendsTab,
    /// Message Requests: the "Spam" tab instead of "Requests".
    pub spam: bool,
    pub query: String,
    pub add_input: String,
    pub add_status: Option<AddStatus>,
    /// Users with a relationship call in flight (buttons disabled).
    pub busy: HashSet<UserId>,
    /// Message requests with a call in flight.
    pub busy_requests: HashSet<ChannelId>,
    /// "Remove Friend" asks once before it goes.
    pub confirm_remove: Option<UserId>,
    /// A DM being opened for this user (the "Message" button).
    pub opening_dm: Option<UserId>,
    pub switcher: Option<Switcher>,
}

/// Friends page input.
#[derive(Debug, Clone)]
pub enum FriendsUi {
    OpenFriends,
    OpenRequests,
    Tab(FriendsTab),
    /// Message Requests: show the spam tab.
    SpamTab(bool),
    Query(String),
    AddInput(String),
    SubmitAdd,
    /// Accept an incoming request, or send one to an implicit relationship.
    Add(UserId),
    /// Ignore an incoming request, cancel an outgoing one, or (after
    /// `AskRemove`) remove a friend.
    Remove(UserId),
    AskRemove(UserId),
    CancelRemove,
    /// Open (or create) the DM with this user.
    Message(UserId),
    AcceptRequest(ChannelId),
    RejectRequest(ChannelId),
    OpenSwitcher,
    CloseSwitcher,
    SwitcherQuery(String),
    SwitcherPick(SwitchTarget),
    /// Enter in the switcher: opens the first result.
    SwitcherSubmit,
}

#[derive(Debug, Clone)]
pub enum FriendsDone {
    Relationship {
        user: UserId,
        /// The relationship after the call (`None`: removed).
        after: Option<RelationshipKind>,
        result: Result<(), String>,
    },
    AddSent {
        name: String,
        result: Result<(), String>,
    },
    DmOpened {
        user: UserId,
        result: Result<(Box<Channel>, Vec<User>), String>,
    },
    Request {
        channel: ChannelId,
        accepted: bool,
        result: Result<(), String>,
    },
}

/// One person on a Friends tab.
#[derive(Debug, Clone, PartialEq)]
pub struct FriendRow {
    pub user: UserId,
    pub kind: RelationshipKind,
    pub name: String,
    pub username: String,
}

fn msg(d: FriendsDone) -> Message {
    Message::Done(Done::Friends(Box::new(d)))
}

fn err(e: crate::error::Error) -> String {
    e.user_message()
}

/// Name on the friends list: the friend nickname, else the display name.
fn row_name(chat: &super::state::ChatState, r: &Relationship) -> (String, String) {
    let u = chat.users.get(&r.user_id);
    let name = r
        .nickname
        .clone()
        .or_else(|| u.map(|u| u.display_name().to_owned()))
        .unwrap_or_else(|| "Unknown user".into());
    (name, u.map(|u| u.username.clone()).unwrap_or_default())
}

fn matches(query: &str, row: &FriendRow) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || row.name.to_lowercase().contains(&q) || row.username.to_lowercase().contains(&q)
}

/// People on `tab`, filtered by `query`, sorted like the official client:
/// by name; Pending lists incoming requests before outgoing ones.
pub fn rows(chat: &super::state::ChatState, tab: FriendsTab, query: &str) -> Vec<FriendRow> {
    let wanted = |r: &Relationship| match tab {
        FriendsTab::Online => {
            r.kind == RelationshipKind::Friend && chat.presence_of(r.user_id) != Presence::Offline
        }
        FriendsTab::All => r.kind == RelationshipKind::Friend,
        FriendsTab::Implicit => r.kind == RelationshipKind::Implicit,
        FriendsTab::Pending => matches!(
            r.kind,
            RelationshipKind::Incoming | RelationshipKind::Outgoing
        ),
        FriendsTab::AddFriend => false,
    };
    let mut v: Vec<FriendRow> = chat
        .relationships
        .values()
        .filter(|r| wanted(r))
        .map(|r| {
            let (name, username) = row_name(chat, r);
            FriendRow {
                user: r.user_id,
                kind: r.kind,
                name,
                username,
            }
        })
        .filter(|row| matches(query, row))
        .collect();
    v.sort_by_cached_key(|r| {
        (
            r.kind != RelationshipKind::Incoming,
            r.name.to_lowercase(),
            r.user,
        )
    });
    v
}

/// Incoming friend requests (the red badge on Friends and Pending).
pub fn incoming_count(chat: &super::state::ChatState) -> usize {
    chat.relationships
        .values()
        .filter(|r| r.kind == RelationshipKind::Incoming)
        .count()
}

/// The username typed in "Add Friend", or why it cannot be sent. Old
/// `name#1234` tags are refused like the official client does now.
pub fn parse_username(input: &str) -> Result<String, &'static str> {
    let s = input.trim().trim_start_matches('@').to_lowercase();
    if s.is_empty() {
        return Err("Type a username first.");
    }
    if s.contains('#') {
        return Err("Discord no longer uses #tags. Type the username only.");
    }
    if s.len() < 2
        || s.len() > 32
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return Err("That doesn't look like a Discord username.");
    }
    Ok(s)
}

/// Switcher results for `query`: DMs, friends without a DM, text channels
/// and servers (at most `limit`). An empty query lists the recent DMs.
pub fn switcher_hits(chat: &super::state::ChatState, query: &str, limit: usize) -> Vec<SwitchHit> {
    let q = query
        .trim()
        .trim_start_matches(['#', '@', '*'])
        .to_lowercase();
    let hit = |s: &str| q.is_empty() || s.to_lowercase().contains(&q);
    let mut out = Vec::new();
    let mut dm_users = HashSet::new();
    for c in chat.dm_list() {
        if c.kind == ChannelKind::Dm {
            dm_users.extend(c.recipients.iter().copied());
        }
        let label = c.label(|u| chat.user_name(u));
        let username = (c.kind == ChannelKind::Dm)
            .then(|| c.recipients.first().and_then(|u| chat.users.get(u)))
            .flatten()
            .map(|u| u.username.clone());
        if hit(&label) || username.as_deref().is_some_and(hit) {
            out.push(SwitchHit {
                target: SwitchTarget::Channel(c.id),
                label,
                detail: username,
            });
        }
    }
    if q.is_empty() {
        out.truncate(limit);
        return out;
    }
    let mut friends: Vec<SwitchHit> = chat
        .relationships
        .values()
        .filter(|r| r.kind == RelationshipKind::Friend && !dm_users.contains(&r.user_id))
        .filter_map(|r| {
            let (name, username) = row_name(chat, r);
            (hit(&name) || hit(&username)).then_some(SwitchHit {
                target: SwitchTarget::User(r.user_id),
                label: name,
                detail: Some(username),
            })
        })
        .collect();
    friends.sort_by_cached_key(|h| h.label.to_lowercase());
    out.extend(friends);
    for g in &chat.guilds {
        let mut chans: Vec<&Channel> = chat
            .channels
            .values()
            .filter(|c| {
                c.guild_id == Some(g.id)
                    && c.kind.is_textual()
                    && c.kind != ChannelKind::Thread
                    && c.name.as_deref().is_some_and(hit)
            })
            .collect();
        chans.sort_by_key(|c| (c.position, c.id));
        out.extend(chans.into_iter().map(|c| SwitchHit {
            target: SwitchTarget::Channel(c.id),
            label: format!("#{}", c.name.as_deref().unwrap_or_default()),
            detail: Some(g.name.clone()),
        }));
    }
    out.extend(
        chat.guilds
            .iter()
            .filter(|g| hit(&g.name))
            .map(|g| SwitchHit {
                target: SwitchTarget::Guild(g.id),
                label: g.name.clone(),
                detail: None,
            }),
    );
    out.truncate(limit);
    out
}

/// Results shown in the switcher.
pub const SWITCHER_LIMIT: usize = 12;

pub fn switcher_input_id() -> iced::widget::Id {
    iced::widget::Id::new("quick-switcher")
}

pub fn add_friend_input_id() -> iced::widget::Id {
    iced::widget::Id::new("add-friend")
}

impl App {
    /// The Friends / Message Requests page is showing.
    pub fn home_page(&self) -> Option<HomePage> {
        matches!(self.view, View::Home(None)).then_some(self.friends.page)
    }

    pub(super) fn update_friends(&mut self, u: FriendsUi) -> Task<Message> {
        let f = &mut self.friends;
        match u {
            FriendsUi::OpenFriends | FriendsUi::OpenRequests => {
                f.page = if matches!(u, FriendsUi::OpenFriends) {
                    HomePage::Friends
                } else {
                    HomePage::Requests
                };
                f.confirm_remove = None;
                self.discover = None;
                if matches!(self.view, View::Home(None)) {
                    return Task::none();
                }
                self.switch_view(View::Home(None))
            }
            FriendsUi::Tab(t) => {
                f.tab = t;
                f.confirm_remove = None;
                if t == FriendsTab::AddFriend {
                    return iced::widget::operation::focus(add_friend_input_id());
                }
                Task::none()
            }
            FriendsUi::SpamTab(spam) => {
                f.spam = spam;
                Task::none()
            }
            FriendsUi::Query(q) => {
                f.query = q.chars().take(100).collect();
                Task::none()
            }
            FriendsUi::AddInput(s) => {
                f.add_input = s.chars().take(40).collect();
                if !matches!(f.add_status, Some(AddStatus::Sending)) {
                    f.add_status = None;
                }
                Task::none()
            }
            FriendsUi::SubmitAdd => self.submit_add_friend(),
            FriendsUi::Add(user) => {
                let after = match self.chat.relationship(user) {
                    Some(RelationshipKind::Incoming) => RelationshipKind::Friend,
                    _ => RelationshipKind::Outgoing,
                };
                self.relationship_call(user, Some(after))
            }
            FriendsUi::AskRemove(user) => {
                f.confirm_remove = Some(user);
                Task::none()
            }
            FriendsUi::CancelRemove => {
                f.confirm_remove = None;
                Task::none()
            }
            FriendsUi::Remove(user) => {
                // Removing a friend needs the confirmation first; requests
                // go at once like in the official client.
                if self.chat.relationship(user) == Some(RelationshipKind::Friend)
                    && self.friends.confirm_remove != Some(user)
                {
                    self.friends.confirm_remove = Some(user);
                    return Task::none();
                }
                self.friends.confirm_remove = None;
                self.relationship_call(user, None)
            }
            FriendsUi::Message(user) => self.message_user(user),
            FriendsUi::AcceptRequest(c) => self.message_request_call(c, true),
            FriendsUi::RejectRequest(c) => self.message_request_call(c, false),
            FriendsUi::OpenSwitcher => {
                self.close_overlays();
                self.friends.switcher = Some(Switcher::default());
                iced::widget::operation::focus(switcher_input_id())
            }
            FriendsUi::CloseSwitcher => {
                f.switcher = None;
                Task::none()
            }
            FriendsUi::SwitcherQuery(q) => {
                if let Some(s) = f.switcher.as_mut() {
                    s.query = q.chars().take(100).collect();
                }
                Task::none()
            }
            FriendsUi::SwitcherSubmit => {
                let first = f
                    .switcher
                    .as_ref()
                    .and_then(|s| switcher_hits(&self.chat, &s.query, 1).into_iter().next());
                match first {
                    Some(h) => self.update_friends(FriendsUi::SwitcherPick(h.target)),
                    None => Task::none(),
                }
            }
            FriendsUi::SwitcherPick(t) => {
                self.friends.switcher = None;
                match t {
                    SwitchTarget::Channel(c) => self.update_ui(Ui::SelectChannel(c)),
                    SwitchTarget::Guild(g) => self.update_ui(Ui::SelectGuild(g)),
                    SwitchTarget::User(u) => self.message_user(u),
                }
            }
        }
    }

    fn submit_add_friend(&mut self) -> Task<Message> {
        if matches!(self.friends.add_status, Some(AddStatus::Sending)) {
            return Task::none();
        }
        let name = match parse_username(&self.friends.add_input) {
            Ok(n) => n,
            Err(e) => {
                self.friends.add_status = Some(AddStatus::Failed(e.into()));
                return Task::none();
            }
        };
        self.friends.add_status = Some(AddStatus::Sending);
        if self.cfg.mock {
            return Task::done(msg(FriendsDone::AddSent {
                name,
                result: Ok(()),
            }));
        }
        let rest = self.rest.clone();
        Task::perform(
            {
                let name = name.clone();
                async move { rest.send_friend_request(&name).await }
            },
            move |r| {
                msg(FriendsDone::AddSent {
                    name: name.clone(),
                    result: r.map_err(|e| match e {
                        // Unknown user: Discord answers 400 (code 80004).
                        crate::error::Error::Http { status: 400, .. } => {
                            "Hm, didn't work. Double check that the username is correct.".to_owned()
                        }
                        e => err(e),
                    }),
                })
            },
        )
    }

    /// Adds (accept / send request) or removes a relationship.
    fn relationship_call(
        &mut self,
        user: UserId,
        after: Option<RelationshipKind>,
    ) -> Task<Message> {
        if !self.friends.busy.insert(user) {
            return Task::none();
        }
        if self.cfg.mock {
            return Task::done(msg(FriendsDone::Relationship {
                user,
                after,
                result: Ok(()),
            }));
        }
        let rest = self.rest.clone();
        Task::perform(
            async move {
                match after {
                    Some(_) => rest.add_friend(user).await,
                    None => rest.remove_relationship(user).await,
                }
            },
            move |r| {
                msg(FriendsDone::Relationship {
                    user,
                    after,
                    result: r.map_err(err),
                })
            },
        )
    }

    fn message_user(&mut self, user: UserId) -> Task<Message> {
        if let Some(c) = self.chat.dm_with(user) {
            return self.update_ui(Ui::SelectChannel(c));
        }
        if self.friends.opening_dm.is_some() {
            return Task::none();
        }
        self.friends.opening_dm = Some(user);
        if self.cfg.mock {
            let c = Channel {
                id: Snowflake(9_000_000 + user.0),
                kind: ChannelKind::Dm,
                guild_id: None,
                name: None,
                topic: None,
                position: 0,
                parent_id: None,
                last_message_id: None,
                recipients: vec![user],
                nsfw: false,
                overwrites: vec![],
                message_request: false,
                spam: false,
            };
            return Task::done(msg(FriendsDone::DmOpened {
                user,
                result: Ok((Box::new(c), vec![])),
            }));
        }
        let rest = self.rest.clone();
        Task::perform(async move { rest.open_dm(user).await }, move |r| {
            msg(FriendsDone::DmOpened {
                user,
                result: r.map(|(c, us)| (Box::new(c), us)).map_err(err),
            })
        })
    }

    fn message_request_call(&mut self, c: ChannelId, accept: bool) -> Task<Message> {
        if !self.friends.busy_requests.insert(c) {
            return Task::none();
        }
        if self.cfg.mock {
            return Task::done(msg(FriendsDone::Request {
                channel: c,
                accepted: accept,
                result: Ok(()),
            }));
        }
        let rest = self.rest.clone();
        Task::perform(
            async move {
                if accept {
                    rest.accept_message_request(c).await
                } else {
                    rest.reject_message_request(c).await
                }
            },
            move |r| {
                msg(FriendsDone::Request {
                    channel: c,
                    accepted: accept,
                    result: r.map_err(err),
                })
            },
        )
    }

    pub(super) fn on_friends_done(&mut self, d: FriendsDone) -> Task<Message> {
        match d {
            FriendsDone::Relationship {
                user,
                after,
                result,
            } => {
                self.friends.busy.remove(&user);
                match result {
                    Ok(()) => {
                        // Mirror the change at once; the gateway echo
                        // brings the same state.
                        let ev = match after {
                            Some(kind) => Event::RelationshipUpsert(
                                Box::new(Relationship {
                                    user_id: user,
                                    kind,
                                    nickname: None,
                                }),
                                None,
                            ),
                            None => Event::RelationshipRemove(user),
                        };
                        let ctx = self.ctx();
                        let _ = self.chat.apply_event(&ev, &ctx);
                    }
                    Err(e) => self.toast = Some(format!("Could not update the friend list: {e}")),
                }
                Task::none()
            }
            FriendsDone::AddSent { name, result } => {
                self.friends.add_status = Some(match result {
                    Ok(()) => {
                        self.friends.add_input.clear();
                        AddStatus::Sent(name)
                    }
                    Err(e) => AddStatus::Failed(e),
                });
                Task::none()
            }
            FriendsDone::DmOpened { user, result } => {
                if self.friends.opening_dm == Some(user) {
                    self.friends.opening_dm = None;
                }
                match result {
                    Ok((c, users)) => {
                        let id = c.id;
                        let ctx = self.ctx();
                        let _ = self.chat.apply_event(&Event::ChannelUpsert(c, users), &ctx);
                        self.update_ui(Ui::SelectChannel(id))
                    }
                    Err(e) => {
                        self.toast = Some(format!("Could not open the conversation: {e}"));
                        Task::none()
                    }
                }
            }
            FriendsDone::Request {
                channel,
                accepted,
                result,
            } => {
                self.friends.busy_requests.remove(&channel);
                match result {
                    Ok(()) if accepted => {
                        if let Some(c) = self.chat.channels.get_mut(&channel) {
                            c.message_request = false;
                            c.spam = false;
                        }
                        self.update_ui(Ui::SelectChannel(channel))
                    }
                    Ok(()) => {
                        let ctx = self.ctx();
                        let _ = self.chat.apply_event(&Event::ChannelDelete(channel), &ctx);
                        Task::none()
                    }
                    Err(e) => {
                        self.toast = Some(format!("Could not answer the message request: {e}"));
                        Task::none()
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::ChatState;

    fn chat() -> ChatState {
        let mut c = ChatState::default();
        for (id, name, kind) in [
            (2u64, "alice", RelationshipKind::Friend),
            (3, "bob", RelationshipKind::Friend),
            (4, "carol", RelationshipKind::Incoming),
            (5, "dave", RelationshipKind::Outgoing),
            (6, "eve", RelationshipKind::Implicit),
            (7, "mallory", RelationshipKind::Blocked),
        ] {
            c.users.insert(
                Snowflake(id),
                User {
                    id: Snowflake(id),
                    username: name.into(),
                    ..Default::default()
                },
            );
            c.relationships.insert(
                Snowflake(id),
                Relationship {
                    user_id: Snowflake(id),
                    kind,
                    nickname: None,
                },
            );
        }
        c.presence.insert(Snowflake(3), Presence::Idle);
        c
    }

    fn names(v: Vec<FriendRow>) -> Vec<String> {
        v.into_iter().map(|r| r.name).collect()
    }

    #[test]
    fn tabs_pick_the_right_people() {
        let c = chat();
        assert_eq!(names(rows(&c, FriendsTab::Online, "")), ["bob"]);
        assert_eq!(names(rows(&c, FriendsTab::All, "")), ["alice", "bob"]);
        assert_eq!(names(rows(&c, FriendsTab::Implicit, "")), ["eve"]);
        // Incoming first, then outgoing; blocked users never show.
        assert_eq!(names(rows(&c, FriendsTab::Pending, "")), ["carol", "dave"]);
        assert_eq!(names(rows(&c, FriendsTab::All, "ALI")), ["alice"]);
        assert_eq!(incoming_count(&c), 1);
    }

    #[test]
    fn nickname_wins() {
        let mut c = chat();
        if let Some(r) = c.relationships.get_mut(&Snowflake(2)) {
            r.nickname = Some("Zed".into());
        }
        assert_eq!(names(rows(&c, FriendsTab::All, "")), ["bob", "Zed"]);
        // The username still matches the search.
        assert_eq!(names(rows(&c, FriendsTab::All, "alice")), ["Zed"]);
    }

    #[test]
    fn usernames() {
        assert_eq!(parse_username("  @Ferris_Fan "), Ok("ferris_fan".into()));
        assert!(parse_username("").is_err());
        assert!(parse_username("name#1234").is_err());
        assert!(parse_username("bad name").is_err());
        assert!(parse_username("a").is_err());
    }

    #[test]
    fn relationship_events() {
        let mut c = chat();
        let ctx = crate::app::state::Ctx::default();
        let rel = |kind| Relationship {
            user_id: Snowflake(4),
            kind,
            nickname: None,
        };
        c.apply_event(
            &Event::RelationshipUpsert(Box::new(rel(RelationshipKind::Friend)), None),
            &ctx,
        );
        assert_eq!(c.relationship(Snowflake(4)), Some(RelationshipKind::Friend));
        c.apply_event(&Event::RelationshipRemove(Snowflake(4)), &ctx);
        assert_eq!(c.relationship(Snowflake(4)), None);
    }

    #[test]
    fn switcher_finds_dms_friends_channels_and_servers() {
        let mut c = chat();
        c.channels.insert(
            Snowflake(50),
            Channel {
                id: Snowflake(50),
                kind: ChannelKind::Dm,
                guild_id: None,
                name: None,
                topic: None,
                position: 0,
                parent_id: None,
                last_message_id: None,
                recipients: vec![Snowflake(2)],
                nsfw: false,
                overwrites: vec![],
                message_request: false,
                spam: false,
            },
        );
        let g = Guild {
            id: Snowflake(60),
            name: "Bobsleigh Club".into(),
            icon: None,
            roles: vec![],
            owner_id: None,
            emojis: vec![],
            stickers: vec![],
            premium_tier: 0,
        };
        c.guilds.push(g);
        c.channels.insert(
            Snowflake(61),
            Channel {
                id: Snowflake(61),
                kind: ChannelKind::Text,
                guild_id: Some(Snowflake(60)),
                name: Some("bob-fans".into()),
                topic: None,
                position: 0,
                parent_id: None,
                last_message_id: None,
                recipients: vec![],
                nsfw: false,
                overwrites: vec![],
                message_request: false,
                spam: false,
            },
        );
        // Empty query: recent DMs only.
        let recent = switcher_hits(&c, "", 10);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].target, SwitchTarget::Channel(Snowflake(50)));
        let hits: Vec<SwitchTarget> = switcher_hits(&c, "bob", 10)
            .into_iter()
            .map(|h| h.target)
            .collect();
        assert_eq!(
            hits,
            [
                SwitchTarget::User(Snowflake(3)),
                SwitchTarget::Channel(Snowflake(61)),
                SwitchTarget::Guild(Snowflake(60)),
            ]
        );
        // A friend with a DM shows as the DM, not twice.
        let alice: Vec<SwitchTarget> = switcher_hits(&c, "alice", 10)
            .into_iter()
            .map(|h| h.target)
            .collect();
        assert_eq!(alice, [SwitchTarget::Channel(Snowflake(50))]);
    }

    #[test]
    fn message_requests_stay_out_of_the_dm_list() {
        let mut c = ChatState::default();
        for (id, req, spam) in [(1u64, false, false), (2, true, false), (3, true, true)] {
            c.channels.insert(
                Snowflake(id),
                Channel {
                    id: Snowflake(id),
                    kind: ChannelKind::Dm,
                    guild_id: None,
                    name: None,
                    topic: None,
                    position: 0,
                    parent_id: None,
                    last_message_id: None,
                    recipients: vec![Snowflake(10 + id)],
                    nsfw: false,
                    overwrites: vec![],
                    message_request: req,
                    spam,
                },
            );
        }
        let ids = |v: Vec<&Channel>| v.into_iter().map(|c| c.id.0).collect::<Vec<_>>();
        assert_eq!(ids(c.dm_list()), [1]);
        assert_eq!(ids(c.message_requests(false)), [2]);
        assert_eq!(ids(c.message_requests(true)), [3]);
    }
}
