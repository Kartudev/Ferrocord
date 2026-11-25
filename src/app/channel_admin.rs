//! Channel administration: create a channel or category, edit a channel
//! (name, topic, NSFW, category), delete it and move it up or down. Offered
//! only with Manage Channels (`app::permissions`); the gateway's
//! CHANNEL_CREATE / UPDATE / DELETE events bring the result back (applied
//! locally in mock mode).

use iced::Task;

use super::guild_admin::admin_error_text;
use super::permissions::MANAGE_CHANNELS;
use super::state::ChatState;
use super::{App, Done, Message, View};
use crate::net::Event;
use crate::net::model::*;
use crate::net::rest::guild::{ChannelEdit, NewChannel, NewKind};

/// Longest channel name Discord accepts.
pub const NAME_MAX: usize = 100;
/// Longest channel topic Discord accepts.
pub const TOPIC_MAX: usize = 1024;

/// UI events of the channel dialogs and menu items.
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelUi {
    /// "Create Channel" (in `parent` when given) or "Create Category".
    OpenCreate {
        guild: GuildId,
        parent: Option<ChannelId>,
        category: bool,
    },
    CreateKind(NewKind),
    CreateName(String),
    CreateParent(ParentChoice),
    SubmitCreate,
    OpenEdit(ChannelId),
    EditName(String),
    EditTopic(String),
    EditNsfw(bool),
    EditParent(ParentChoice),
    SubmitEdit,
    AskDelete(ChannelId),
    ConfirmDelete,
    /// Moves a channel (or category) one place up (-1) or down (+1).
    Move(ChannelId, i32),
    Close,
}

/// Answers of the channel REST calls.
#[derive(Debug, Clone)]
pub enum ChannelDone {
    Created {
        guild: GuildId,
        result: Result<Channel, String>,
    },
    Edited {
        channel: ChannelId,
        edit: ChannelEdit,
        result: Result<(), String>,
    },
    Deleted {
        channel: ChannelId,
        result: Result<(), String>,
    },
    Reordered {
        /// Positions before the move, restored on failure.
        previous: Vec<(ChannelId, i32)>,
        result: Result<(), String>,
    },
}

/// An entry of the category picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentChoice {
    pub id: Option<ChannelId>,
    pub name: String,
}

impl std::fmt::Display for ParentChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// The category picker of `g`: "No Category" then each category.
pub fn parent_choices(chat: &ChatState, g: GuildId) -> Vec<ParentChoice> {
    let mut cats: Vec<&Channel> = chat
        .channels
        .values()
        .filter(|c| c.guild_id == Some(g) && c.kind == ChannelKind::Category)
        .collect();
    cats.sort_by_key(|c| (c.position, c.id));
    std::iter::once(ParentChoice {
        id: None,
        name: "No Category".into(),
    })
    .chain(cats.into_iter().map(|c| ParentChoice {
        id: Some(c.id),
        name: c.name.clone().unwrap_or_default(),
    }))
    .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateForm {
    pub guild: GuildId,
    pub kind: NewKind,
    pub name: String,
    pub parent: Option<ChannelId>,
    pub busy: bool,
    pub error: Option<String>,
}

impl CreateForm {
    pub fn can_submit(&self) -> bool {
        !self.busy && valid_name(&self.name)
    }

    pub fn request(&self) -> NewChannel {
        NewChannel {
            name: self.name.trim().to_owned(),
            kind: self.kind,
            parent: self.parent,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditForm {
    pub channel: ChannelId,
    pub kind: ChannelKind,
    pub name: String,
    pub topic: String,
    pub nsfw: bool,
    pub parent: Option<ChannelId>,
    /// Values when the dialog opened.
    original: (String, String, bool, Option<ChannelId>),
    pub busy: bool,
    pub error: Option<String>,
}

impl EditForm {
    pub fn new(c: &Channel, parent: Option<ChannelId>) -> Self {
        let topic = c.topic.clone().unwrap_or_default();
        let name = c.name.clone().unwrap_or_default();
        Self {
            channel: c.id,
            kind: c.kind,
            name: name.clone(),
            topic: topic.clone(),
            nsfw: c.nsfw,
            parent,
            original: (name, topic, c.nsfw, parent),
            busy: false,
            error: None,
        }
    }

    /// Text-like channels have a topic and the NSFW switch.
    pub fn has_topic(&self) -> bool {
        matches!(self.kind, ChannelKind::Text | ChannelKind::Announcement)
    }

    pub fn is_category(&self) -> bool {
        self.kind == ChannelKind::Category
    }

    /// What "Save Changes" sends.
    pub fn edit(&self) -> ChannelEdit {
        let (name, topic, nsfw, parent) = &self.original;
        ChannelEdit {
            name: (self.name.trim() != name.trim()).then(|| self.name.trim().to_owned()),
            topic: (self.has_topic() && self.topic.trim() != topic.trim())
                .then(|| self.topic.trim().to_owned()),
            nsfw: (self.has_topic() && self.nsfw != *nsfw).then_some(self.nsfw),
            parent: (!self.is_category() && self.parent != *parent).then_some(self.parent),
        }
    }

    pub fn can_submit(&self) -> bool {
        !self.busy && valid_name(&self.name) && !self.edit().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeleteConfirm {
    pub channel: ChannelId,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChannelDialog {
    Create(CreateForm),
    Edit(EditForm),
    Delete(DeleteConfirm),
}

pub fn valid_name(name: &str) -> bool {
    (1..=NAME_MAX).contains(&name.trim().chars().count())
}

/// Live input transform of text channel names, like the official client:
/// lower case, spaces become dashes.
pub fn text_channel_input(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .flat_map(char::to_lowercase)
        .take(NAME_MAX)
        .collect()
}

fn takes_text_names(kind: ChannelKind) -> bool {
    matches!(kind, ChannelKind::Text | ChannelKind::Announcement)
}

/// The category `c` is listed under (a parent that is not a known
/// category counts as none, like `ChatState::channel_tree`).
fn listed_parent(chat: &ChatState, c: &Channel) -> Option<ChannelId> {
    c.parent_id.filter(|p| {
        chat.channels
            .get(p)
            .is_some_and(|k| k.kind == ChannelKind::Category)
    })
}

fn is_voice(k: ChannelKind) -> bool {
    matches!(k, ChannelKind::Voice | ChannelKind::Stage)
}

/// The channels `c` is ordered among, in list order: every category of
/// the server for a category; otherwise the channels under the same
/// category in the same group (text-like before voice, as listed).
pub fn siblings(chat: &ChatState, c: ChannelId) -> Vec<&Channel> {
    let Some(ch) = chat.channels.get(&c) else {
        return vec![];
    };
    let Some(g) = ch.guild_id else {
        return vec![];
    };
    let parent = listed_parent(chat, ch);
    let mut v: Vec<&Channel> = chat
        .channels
        .values()
        .filter(|x| x.guild_id == Some(g) && x.kind != ChannelKind::Thread)
        .filter(|x| {
            if ch.kind == ChannelKind::Category {
                x.kind == ChannelKind::Category
            } else {
                x.kind != ChannelKind::Category
                    && listed_parent(chat, x) == parent
                    && is_voice(x.kind) == is_voice(ch.kind)
            }
        })
        .collect();
    v.sort_by_key(|x| (x.position, x.id));
    v
}

/// New positions after moving `c` by `delta` places among its siblings:
/// the siblings are renumbered 0.. in their new order and only the ones
/// whose position changes are returned. `None` when it cannot move.
pub fn reorder(chat: &ChatState, c: ChannelId, delta: i32) -> Option<Vec<(ChannelId, i32)>> {
    let sibs = siblings(chat, c);
    let from = sibs.iter().position(|x| x.id == c)?;
    let to = from as i64 + i64::from(delta);
    if to < 0 || to >= sibs.len() as i64 {
        return None;
    }
    let mut ids: Vec<(ChannelId, i32)> = sibs.iter().map(|x| (x.id, x.position)).collect();
    let item = ids.remove(from);
    ids.insert(to as usize, item);
    let moves: Vec<(ChannelId, i32)> = ids
        .iter()
        .enumerate()
        .filter(|(i, (_, old))| *i as i32 != *old)
        .map(|(i, (id, _))| (*id, i as i32))
        .collect();
    (!moves.is_empty()).then_some(moves)
}

impl App {
    /// Whether the user may create channels in `g`.
    pub fn can_manage_channels(&self, g: GuildId) -> bool {
        self.chat.can_in_guild(g, MANAGE_CHANNELS)
    }

    /// Whether the user may edit, move or delete `c` (a server channel).
    pub fn can_manage_channel(&self, c: ChannelId) -> bool {
        self.chat
            .channels
            .get(&c)
            .is_some_and(|ch| ch.guild_id.is_some())
            && self.chat.channel_permissions(c) & MANAGE_CHANNELS != 0
    }

    pub(super) fn update_channel_admin(&mut self, u: ChannelUi) -> Task<Message> {
        match u {
            ChannelUi::OpenCreate {
                guild,
                parent,
                category,
            } => {
                self.close_overlays();
                if !self.can_manage_channels(guild) {
                    return Task::none();
                }
                self.channel_dialog = Some(ChannelDialog::Create(CreateForm {
                    guild,
                    kind: if category {
                        NewKind::Category
                    } else {
                        NewKind::Text
                    },
                    name: String::new(),
                    parent: if category { None } else { parent },
                    busy: false,
                    error: None,
                }));
                iced::widget::operation::focus(crate::ui::channel_admin::create_name_id())
            }
            ChannelUi::CreateKind(k) => {
                if let Some(ChannelDialog::Create(f)) = self.channel_dialog.as_mut()
                    && f.kind != NewKind::Category
                    && k != NewKind::Category
                {
                    f.kind = k;
                    if k == NewKind::Text {
                        f.name = text_channel_input(&f.name);
                    }
                }
                Task::none()
            }
            ChannelUi::CreateName(s) => {
                if let Some(ChannelDialog::Create(f)) = self.channel_dialog.as_mut() {
                    f.name = if f.kind == NewKind::Text {
                        text_channel_input(&s)
                    } else {
                        s.chars().take(NAME_MAX).collect()
                    };
                    f.error = None;
                }
                Task::none()
            }
            ChannelUi::CreateParent(p) => {
                if let Some(ChannelDialog::Create(f)) = self.channel_dialog.as_mut() {
                    f.parent = p.id;
                }
                Task::none()
            }
            ChannelUi::SubmitCreate => self.submit_create_channel(),
            ChannelUi::OpenEdit(c) => {
                self.close_overlays();
                if !self.can_manage_channel(c) {
                    return Task::none();
                }
                let Some(ch) = self.chat.channels.get(&c) else {
                    return Task::none();
                };
                let parent = listed_parent(&self.chat, ch);
                self.channel_dialog = Some(ChannelDialog::Edit(EditForm::new(ch, parent)));
                Task::none()
            }
            ChannelUi::EditName(s) => {
                if let Some(ChannelDialog::Edit(f)) = self.channel_dialog.as_mut() {
                    f.name = if takes_text_names(f.kind) {
                        text_channel_input(&s)
                    } else {
                        s.chars().take(NAME_MAX).collect()
                    };
                    f.error = None;
                }
                Task::none()
            }
            ChannelUi::EditTopic(s) => {
                if let Some(ChannelDialog::Edit(f)) = self.channel_dialog.as_mut() {
                    f.topic = s.chars().take(TOPIC_MAX).collect();
                }
                Task::none()
            }
            ChannelUi::EditNsfw(on) => {
                if let Some(ChannelDialog::Edit(f)) = self.channel_dialog.as_mut() {
                    f.nsfw = on;
                }
                Task::none()
            }
            ChannelUi::EditParent(p) => {
                if let Some(ChannelDialog::Edit(f)) = self.channel_dialog.as_mut() {
                    f.parent = p.id;
                }
                Task::none()
            }
            ChannelUi::SubmitEdit => self.submit_edit_channel(),
            ChannelUi::AskDelete(c) => {
                self.close_overlays();
                if !self.can_manage_channel(c) {
                    return Task::none();
                }
                self.channel_dialog = Some(ChannelDialog::Delete(DeleteConfirm {
                    channel: c,
                    busy: false,
                    error: None,
                }));
                Task::none()
            }
            ChannelUi::ConfirmDelete => self.delete_channel(),
            ChannelUi::Move(c, delta) => {
                self.close_overlays();
                self.move_channel(c, delta)
            }
            ChannelUi::Close => {
                let busy = match &self.channel_dialog {
                    Some(ChannelDialog::Create(f)) => f.busy,
                    Some(ChannelDialog::Edit(f)) => f.busy,
                    Some(ChannelDialog::Delete(d)) => d.busy,
                    None => false,
                };
                if !busy {
                    self.channel_dialog = None;
                }
                Task::none()
            }
        }
    }

    fn submit_create_channel(&mut self) -> Task<Message> {
        let Some(ChannelDialog::Create(f)) = self.channel_dialog.as_mut() else {
            return Task::none();
        };
        if !f.can_submit() {
            return Task::none();
        }
        f.busy = true;
        f.error = None;
        let (guild, req) = (f.guild, f.request());
        if self.cfg.mock {
            let id = Snowflake(crate::mock::now_ms().max(1_000_000));
            let channel = Channel {
                id,
                kind: ChannelKind::from_wire(req.kind.wire()),
                guild_id: Some(guild),
                name: Some(req.name.clone()),
                topic: None,
                position: i32::MAX / 2,
                parent_id: req.parent.filter(|_| req.kind != NewKind::Category),
                last_message_id: None,
                recipients: vec![],
                nsfw: false,
                overwrites: vec![],
            };
            return Task::done(Message::Done(Done::Channel(ChannelDone::Created {
                guild,
                result: Ok(channel),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.create_channel(guild, &req).await },
            move |r| {
                Message::Done(Done::Channel(ChannelDone::Created {
                    guild,
                    result: r.map_err(|e| admin_error_text(&e)),
                }))
            },
        )
    }

    fn submit_edit_channel(&mut self) -> Task<Message> {
        let Some(ChannelDialog::Edit(f)) = self.channel_dialog.as_mut() else {
            return Task::none();
        };
        if !f.can_submit() {
            if !valid_name(&f.name) {
                f.error = Some(format!("The name must be 1 to {NAME_MAX} characters long."));
            }
            return Task::none();
        }
        f.busy = true;
        f.error = None;
        let (channel, edit) = (f.channel, f.edit());
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Channel(ChannelDone::Edited {
                channel,
                edit,
                result: Ok(()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(
            {
                let edit = edit.clone();
                async move { rest.modify_channel(channel, &edit).await }
            },
            move |r| {
                Message::Done(Done::Channel(ChannelDone::Edited {
                    channel,
                    edit: edit.clone(),
                    result: r.map(|_| ()).map_err(|e| admin_error_text(&e)),
                }))
            },
        )
    }

    fn delete_channel(&mut self) -> Task<Message> {
        let Some(ChannelDialog::Delete(d)) = self.channel_dialog.as_mut() else {
            return Task::none();
        };
        if d.busy {
            return Task::none();
        }
        d.busy = true;
        d.error = None;
        let channel = d.channel;
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Channel(ChannelDone::Deleted {
                channel,
                result: Ok(()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.delete_channel(channel).await },
            move |r| {
                Message::Done(Done::Channel(ChannelDone::Deleted {
                    channel,
                    result: r.map(|_| ()).map_err(|e| admin_error_text(&e)),
                }))
            },
        )
    }

    /// Moves `c` at once (optimistic) and sends the new positions; a
    /// failure puts the old ones back.
    fn move_channel(&mut self, c: ChannelId, delta: i32) -> Task<Message> {
        if !self.can_manage_channel(c) {
            return Task::none();
        }
        let Some(guild) = self.chat.channels.get(&c).and_then(|ch| ch.guild_id) else {
            return Task::none();
        };
        let Some(moves) = reorder(&self.chat, c, delta) else {
            return Task::none();
        };
        let previous = self.apply_positions(&moves);
        if self.cfg.mock {
            return Task::none();
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.reorder_channels(guild, &moves).await },
            move |r| {
                Message::Done(Done::Channel(ChannelDone::Reordered {
                    previous: previous.clone(),
                    result: r.map(|_| ()).map_err(|e| admin_error_text(&e)),
                }))
            },
        )
    }

    /// Sets channel positions; returns the ones they replaced.
    fn apply_positions(&mut self, moves: &[(ChannelId, i32)]) -> Vec<(ChannelId, i32)> {
        let mut previous = Vec::with_capacity(moves.len());
        for (id, pos) in moves {
            if let Some(ch) = self.chat.channels.get_mut(id) {
                previous.push((*id, ch.position));
                ch.position = *pos;
            }
        }
        previous
    }

    pub(super) fn on_channel_done(&mut self, d: ChannelDone) -> Task<Message> {
        match d {
            ChannelDone::Created { guild, result } => match result {
                Ok(ch) => {
                    self.channel_dialog = None;
                    let (id, textual) = (ch.id, ch.kind.is_textual());
                    // The gateway's CHANNEL_CREATE brings it too; insert now
                    // so it can be opened right away.
                    let ctx = self.ctx();
                    let _ = self
                        .chat
                        .apply_event(&Event::ChannelUpsert(Box::new(ch), vec![]), &ctx);
                    if textual && matches!(self.view, View::Guild { guild: g, .. } if g == guild) {
                        return self.update(Message::Ui(super::Ui::SelectChannel(id)));
                    }
                    Task::none()
                }
                Err(e) => {
                    if let Some(ChannelDialog::Create(f)) = self.channel_dialog.as_mut() {
                        f.busy = false;
                        f.error = Some(e);
                    }
                    Task::none()
                }
            },
            ChannelDone::Edited {
                channel,
                edit,
                result,
            } => {
                match result {
                    Ok(()) => {
                        if matches!(&self.channel_dialog, Some(ChannelDialog::Edit(f)) if f.channel == channel)
                        {
                            self.channel_dialog = None;
                        }
                        if let Some(mut ch) = self.chat.channels.get(&channel).cloned() {
                            if let Some(n) = edit.name {
                                ch.name = Some(n);
                            }
                            if let Some(t) = edit.topic {
                                ch.topic = (!t.is_empty()).then_some(t);
                            }
                            if let Some(n) = edit.nsfw {
                                ch.nsfw = n;
                            }
                            if let Some(p) = edit.parent {
                                ch.parent_id = p;
                            }
                            let ctx = self.ctx();
                            let _ = self
                                .chat
                                .apply_event(&Event::ChannelUpsert(Box::new(ch), vec![]), &ctx);
                        }
                    }
                    Err(e) => {
                        if let Some(ChannelDialog::Edit(f)) = self.channel_dialog.as_mut() {
                            f.busy = false;
                            f.error = Some(e);
                        }
                    }
                }
                Task::none()
            }
            ChannelDone::Deleted { channel, result } => {
                match result {
                    Ok(()) => {
                        self.channel_dialog = None;
                        let guild = self.chat.channels.get(&channel).and_then(|c| c.guild_id);
                        // CHANNEL_DELETE follows; drop it now so nothing
                        // reopens it.
                        let ctx = self.ctx();
                        let _ = self.chat.apply_event(&Event::ChannelDelete(channel), &ctx);
                        if let View::Guild {
                            guild: g,
                            channel: Some(open),
                        } = self.view
                            && open == channel
                            && Some(g) == guild
                        {
                            let channel = self.chat.default_channel(g);
                            return self.switch_view(View::Guild { guild: g, channel });
                        }
                    }
                    Err(e) => {
                        if let Some(ChannelDialog::Delete(d)) = self.channel_dialog.as_mut() {
                            d.busy = false;
                            d.error = Some(e);
                        }
                    }
                }
                Task::none()
            }
            ChannelDone::Reordered { previous, result } => {
                if let Err(e) = result {
                    let _ = self.apply_positions(&previous);
                    self.toast = Some(format!("Could not move the channel: {e}"));
                }
                Task::none()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;

    fn app() -> App {
        App::boot(crate::config::Config::for_tests()).0
    }

    fn cu(a: &mut App, u: ChannelUi) {
        let _ = a.update(Message::Ui(super::super::Ui::Channel(u)));
    }

    /// Game Night (owned by the mock user): category 410 with #general
    /// (411), #help (412), #off-topic (413).
    const G: GuildId = mock::GUILD_GAMES;

    fn names(a: &App) -> Vec<String> {
        a.chat
            .channel_tree(G)
            .into_iter()
            .flat_map(|(_, k)| k.into_iter().map(|c| c.name.clone().unwrap_or_default()))
            .collect()
    }

    #[test]
    fn text_names_are_slugged_while_typing() {
        assert_eq!(text_channel_input("Off Topic"), "off-topic");
        assert!(valid_name(" a "));
        assert!(!valid_name("   "));
        assert!(!valid_name(&"x".repeat(NAME_MAX + 1)));
    }

    #[test]
    fn reorder_renumbers_siblings() {
        let a = app();
        assert_eq!(
            siblings(&a.chat, Snowflake(412))
                .iter()
                .map(|c| c.id.0)
                .collect::<Vec<_>>(),
            vec![411, 412, 413]
        );
        assert_eq!(reorder(&a.chat, Snowflake(411), -1), None);
        assert_eq!(reorder(&a.chat, Snowflake(413), 1), None);
        assert_eq!(
            reorder(&a.chat, Snowflake(413), -1),
            Some(vec![(Snowflake(413), 1), (Snowflake(412), 2)])
        );
        // Categories move among categories.
        assert!(
            siblings(&a.chat, Snowflake(410))
                .iter()
                .all(|c| c.kind == ChannelKind::Category)
        );
    }

    #[test]
    fn move_up_and_down_in_the_list() {
        let mut a = app();
        cu(&mut a, ChannelUi::Move(Snowflake(413), -1));
        assert_eq!(names(&a), vec!["general", "off-topic", "help"]);
        cu(&mut a, ChannelUi::Move(Snowflake(411), 1));
        assert_eq!(names(&a), vec!["off-topic", "general", "help"]);
        // A failed save puts the old order back.
        let previous = a.apply_positions(&[(Snowflake(411), 5)]);
        let _ = a.on_channel_done(ChannelDone::Reordered {
            previous,
            result: Err("no".into()),
        });
        assert_eq!(names(&a), vec!["off-topic", "general", "help"]);
        assert!(a.toast.is_some());
        // Not in a server the user cannot manage.
        let before = a.chat.channels[&Snowflake(212)].position;
        cu(&mut a, ChannelUi::Move(Snowflake(212), -1));
        assert_eq!(a.chat.channels[&Snowflake(212)].position, before);
    }

    #[test]
    fn create_a_channel_in_a_category() {
        let mut a = app();
        cu(
            &mut a,
            ChannelUi::OpenCreate {
                guild: G,
                parent: Some(Snowflake(410)),
                category: false,
            },
        );
        cu(&mut a, ChannelUi::CreateName("Game Plans".into()));
        let Some(ChannelDialog::Create(f)) = &a.channel_dialog else {
            panic!("no dialog");
        };
        assert_eq!(f.name, "game-plans");
        assert_eq!(f.parent, Some(Snowflake(410)));
        let req = f.request();
        cu(&mut a, ChannelUi::SubmitCreate);
        assert!(matches!(&a.channel_dialog, Some(ChannelDialog::Create(f)) if f.busy));
        let ch = Channel {
            id: Snowflake(499),
            kind: ChannelKind::Text,
            guild_id: Some(G),
            name: Some(req.name),
            topic: None,
            position: 3,
            parent_id: req.parent,
            last_message_id: None,
            recipients: vec![],
            nsfw: false,
            overwrites: vec![],
        };
        let _ = a.on_channel_done(ChannelDone::Created {
            guild: G,
            result: Ok(ch),
        });
        assert!(a.channel_dialog.is_none());
        assert_eq!(
            names(&a),
            vec!["general", "help", "off-topic", "game-plans"]
        );
        // Voice keeps capitals and spaces.
        cu(
            &mut a,
            ChannelUi::OpenCreate {
                guild: G,
                parent: None,
                category: false,
            },
        );
        cu(&mut a, ChannelUi::CreateKind(NewKind::Voice));
        cu(&mut a, ChannelUi::CreateName("Game Room".into()));
        assert!(
            matches!(&a.channel_dialog, Some(ChannelDialog::Create(f)) if f.name == "Game Room")
        );
        // Not offered where Manage Channels is missing.
        cu(&mut a, ChannelUi::Close);
        cu(
            &mut a,
            ChannelUi::OpenCreate {
                guild: mock::GUILD_RUST,
                parent: None,
                category: true,
            },
        );
        assert!(a.channel_dialog.is_none());
    }

    #[test]
    fn edit_sends_only_changes_and_applies_them() {
        let mut a = app();
        cu(&mut a, ChannelUi::OpenEdit(Snowflake(412)));
        let Some(ChannelDialog::Edit(f)) = &a.channel_dialog else {
            panic!("no dialog");
        };
        assert!(!f.can_submit(), "nothing changed yet");
        cu(&mut a, ChannelUi::EditTopic("Ask here".into()));
        cu(
            &mut a,
            ChannelUi::EditParent(ParentChoice {
                id: None,
                name: "No Category".into(),
            }),
        );
        let Some(ChannelDialog::Edit(f)) = &a.channel_dialog else {
            panic!("no dialog");
        };
        let edit = f.edit();
        assert_eq!(edit.name, None);
        assert_eq!(edit.topic.as_deref(), Some("Ask here"));
        assert_eq!(edit.parent, Some(None));
        cu(&mut a, ChannelUi::SubmitEdit);
        let _ = a.on_channel_done(ChannelDone::Edited {
            channel: Snowflake(412),
            edit,
            result: Ok(()),
        });
        assert!(a.channel_dialog.is_none());
        let ch = &a.chat.channels[&Snowflake(412)];
        assert_eq!(ch.topic.as_deref(), Some("Ask here"));
        assert_eq!(ch.parent_id, None);
        // A refusal keeps the dialog with the reason.
        cu(&mut a, ChannelUi::OpenEdit(Snowflake(411)));
        cu(&mut a, ChannelUi::EditName("lobby".into()));
        cu(&mut a, ChannelUi::SubmitEdit);
        let _ = a.on_channel_done(ChannelDone::Edited {
            channel: Snowflake(411),
            edit: ChannelEdit::default(),
            result: Err("You do not have permission to do that.".into()),
        });
        assert!(
            matches!(&a.channel_dialog, Some(ChannelDialog::Edit(f)) if !f.busy && f.error.is_some())
        );
    }

    #[test]
    fn delete_confirms_and_leaves_the_open_channel() {
        let mut a = app();
        let _ = a.update(Message::Ui(super::super::Ui::SelectGuild(G)));
        let _ = a.update(Message::Ui(super::super::Ui::SelectChannel(Snowflake(412))));
        cu(&mut a, ChannelUi::AskDelete(Snowflake(412)));
        cu(&mut a, ChannelUi::ConfirmDelete);
        // Busy: Escape / outside click do not close it.
        cu(&mut a, ChannelUi::Close);
        assert!(a.channel_dialog.is_some());
        let _ = a.on_channel_done(ChannelDone::Deleted {
            channel: Snowflake(412),
            result: Ok(()),
        });
        assert!(a.channel_dialog.is_none());
        assert!(!a.chat.channels.contains_key(&Snowflake(412)));
        assert!(matches!(a.view, View::Guild { channel, .. } if channel != Some(Snowflake(412))));
    }
}
