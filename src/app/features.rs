//! Channel-header popovers (notification settings, pins, inbox), the
//! "Add a Server" dialogs and the Discover page.

use iced::Task;

use super::panels::{InboxState, PinsState, Popover};
use super::servers::{
    CreateForm, DiscoverState, JoinForm, JoinStep, ServerDialog, invite_error_text,
};
use super::{App, Done, Message, Ui, View, now_ms};
use crate::net::extras::{
    self, ChannelNotify, MuteDuration, NotifyLevel, SearchQuery, icon_data_uri,
};
use crate::net::model::Message as Msg;
use crate::net::model::*;

/// Header popovers' state.
#[derive(Debug, Clone, Default)]
pub struct Panels {
    pub open: Option<Popover>,
    pub pins: PinsState,
    pub inbox: InboxState,
}

fn err(e: crate::error::Error) -> String {
    e.user_message()
}

impl App {
    pub(super) fn update_extras(&mut self, u: Ui) -> Task<Message> {
        if let Some(t) = self.update_rail(u.clone()) {
            return t;
        }
        if let Some(t) = self.update_context(u.clone()) {
            return t;
        }
        match u {
            // -- header
            Ui::TogglePopover(k) => self.toggle_popover(k),
            Ui::ClosePopover => {
                self.panels.open = None;
                Task::none()
            }
            Ui::SetChannelLevel(c, l) => self.set_channel_level(c, l),
            Ui::MuteChannel(c, d) => self.mute_channel(c, Some(d)),
            Ui::UnmuteChannel(c) => self.mute_channel(c, None),
            Ui::Unpin(c, m) => self.unpin(c, m),
            Ui::InboxMore => self.load_inbox(false),
            Ui::InboxGuildOnly(on) => {
                if self.panels.inbox.set_guild_only(on) {
                    self.load_inbox(true)
                } else {
                    Task::none()
                }
            }
            // -- add a server
            Ui::OpenAddServer => {
                self.close_overlays();
                self.server_dialog = Some(ServerDialog::Choose);
                Task::none()
            }
            Ui::CloseAddServer => {
                self.server_dialog = None;
                Task::none()
            }
            Ui::AddServerBack => {
                if matches!(
                    self.server_dialog,
                    Some(ServerDialog::Create(_) | ServerDialog::Join(_))
                ) {
                    self.server_dialog = Some(ServerDialog::Choose);
                }
                Task::none()
            }
            Ui::AddServerCreate => {
                let name = self
                    .chat
                    .me
                    .as_ref()
                    .map(|u| u.display_name().to_owned())
                    .unwrap_or_default();
                self.server_dialog = Some(ServerDialog::Create(CreateForm::for_user(&name)));
                iced::widget::operation::focus(crate::ui::servers::create_name_id())
            }
            Ui::AddServerJoin => {
                self.server_dialog = Some(ServerDialog::Join(JoinForm::default()));
                iced::widget::operation::focus(crate::ui::servers::join_input_id())
            }
            Ui::CreateNameChanged(s) => {
                if let Some(ServerDialog::Create(f)) = self.server_dialog.as_mut() {
                    f.name = s.chars().take(super::servers::NAME_MAX).collect();
                    f.error = None;
                }
                Task::none()
            }
            Ui::PickServerIcon => self.pick_server_icon(),
            Ui::ClearServerIcon => {
                if let Some(ServerDialog::Create(f)) = self.server_dialog.as_mut() {
                    f.icon_path = None;
                    f.icon_uri = None;
                }
                Task::none()
            }
            Ui::SubmitCreate => self.submit_create(),
            Ui::JoinInputChanged(s) => {
                if let Some(ServerDialog::Join(f)) = self.server_dialog.as_mut() {
                    f.set_input(s);
                }
                Task::none()
            }
            Ui::SubmitJoin => self.submit_join_lookup(),
            Ui::ConfirmJoin => self.confirm_join(),
            // -- discover
            Ui::OpenDiscover => self.open_discover(),
            Ui::CloseDiscover => {
                self.discover = None;
                Task::none()
            }
            Ui::DiscoverInput(s) => {
                if let Some(d) = self.discover.as_mut() {
                    d.input = s;
                }
                Task::none()
            }
            Ui::DiscoverSubmit => self.load_discover(0),
            Ui::DiscoverCategory(c) => {
                let changed = self.discover.as_mut().is_some_and(|d| d.select_category(c));
                if changed {
                    self.load_discover(0)
                } else {
                    Task::none()
                }
            }
            Ui::DiscoverMore => {
                let off = self.discover.as_ref().map(DiscoverState::next_offset);
                match off {
                    Some(o) => self.load_discover(o),
                    None => Task::none(),
                }
            }
            Ui::DiscoverDetails(g) => {
                if let Some(d) = self.discover.as_mut() {
                    d.preview = g;
                }
                Task::none()
            }
            Ui::DiscoverJoin(g) => self.discover_join(g),
            _ => Task::none(),
        }
    }

    /// Closes every popover-like overlay before opening another one.
    pub(super) fn close_overlays(&mut self) {
        self.menu = None;
        self.emoji_picker = None;
        self.status_menu = false;
        self.panels.open = None;
        self.rail.menu = None;
        self.ctx_menu = None;
    }

    // -- popovers ------------------------------------------------------------------

    fn toggle_popover(&mut self, kind: Popover) -> Task<Message> {
        if self.panels.open == Some(kind) {
            self.panels.open = None;
            return Task::none();
        }
        self.close_overlays();
        self.panels.open = Some(kind);
        match kind {
            Popover::Bell => Task::none(),
            Popover::Pins => self.load_pins(),
            Popover::Inbox => self.load_inbox(true),
        }
    }

    fn load_pins(&mut self) -> Task<Message> {
        let Some(c) = self.open_chan() else {
            return Task::none();
        };
        let generation = self.panels.pins.begin(c);
        if self.cfg.mock {
            let items = self.mock_pins(c);
            self.panels.pins.finish(generation, Ok(items));
            return Task::none();
        }
        let rest = self.rest.clone();
        Task::perform(async move { rest.pins(c).await }, move |r| {
            Message::Done(Done::Pins {
                generation,
                result: r.map_err(err),
            })
        })
    }

    pub(super) fn on_pins(
        &mut self,
        generation: u64,
        result: Result<Vec<Msg>, String>,
    ) -> Task<Message> {
        if let Ok(items) = &result {
            self.remember_authors(items);
        }
        self.panels.pins.finish(generation, result);
        self.request_images()
    }

    fn load_inbox(&mut self, reset: bool) -> Task<Message> {
        let (generation, before) = self.panels.inbox.begin(reset);
        let guild = match (self.panels.inbox.guild_only, self.view) {
            (true, View::Guild { guild, .. }) => Some(guild),
            _ => None,
        };
        if self.cfg.mock {
            let items = self.mock_mentions(guild);
            self.panels.inbox.finish(generation, Ok(items));
            return Task::none();
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.mentions(before, guild).await },
            move |r| {
                Message::Done(Done::Mentions {
                    generation,
                    result: r.map_err(err),
                })
            },
        )
    }

    pub(super) fn on_mentions(
        &mut self,
        generation: u64,
        result: Result<Vec<Msg>, String>,
    ) -> Task<Message> {
        let result = result.map(|mut items| {
            // Mention payloads carry no `guild_id` on some accounts.
            for m in &mut items {
                if m.guild_id.is_none() {
                    m.guild_id = self
                        .chat
                        .channels
                        .get(&m.channel_id)
                        .and_then(|c| c.guild_id);
                }
            }
            items
        });
        if let Ok(items) = &result {
            self.remember_authors(items);
        }
        self.panels.inbox.finish(generation, result);
        self.request_images()
    }

    fn remember_authors(&mut self, items: &[Msg]) {
        for m in items {
            self.chat
                .users
                .entry(m.author.id)
                .or_insert_with(|| m.author.clone());
        }
    }

    fn guild_of(&self, c: ChannelId) -> Option<GuildId> {
        self.chat.channels.get(&c).and_then(|c| c.guild_id)
    }

    fn set_channel_level(&mut self, c: ChannelId, level: NotifyLevel) -> Task<Message> {
        let previous = self.chat.channel_notify(c);
        if previous.level == level {
            return Task::none();
        }
        self.chat
            .set_channel_notify(c, ChannelNotify { level, ..previous });
        if self.cfg.mock {
            return Task::none();
        }
        let guild = self.guild_of(c);
        let body = extras::channel_override_body(c, extras::level_override(level));
        self.save_notify(guild, c, previous, body)
    }

    /// `Some(d)` mutes for `d`, `None` unmutes.
    fn mute_channel(&mut self, c: ChannelId, d: Option<MuteDuration>) -> Task<Message> {
        let previous = self.chat.channel_notify(c);
        let now = now_ms();
        let (next, over) = match d {
            Some(d) => (d.apply(previous.level, now), extras::mute_override(d, now)),
            None => (
                ChannelNotify {
                    muted: false,
                    mute_end_ms: None,
                    ..previous
                },
                extras::unmute_override(),
            ),
        };
        self.chat.set_channel_notify(c, next);
        if self.cfg.mock {
            return Task::none();
        }
        let guild = self.guild_of(c);
        self.save_notify(guild, c, previous, extras::channel_override_body(c, over))
    }

    fn save_notify(
        &mut self,
        guild: Option<GuildId>,
        channel: ChannelId,
        previous: ChannelNotify,
        body: serde_json::Value,
    ) -> Task<Message> {
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.patch_guild_settings(guild, body).await },
            move |r| {
                Message::Done(Done::NotifySaved {
                    channel,
                    previous,
                    result: r.map_err(err),
                })
            },
        )
    }

    pub(super) fn on_notify_saved(
        &mut self,
        channel: ChannelId,
        previous: ChannelNotify,
        result: Result<(), String>,
    ) -> Task<Message> {
        if let Err(e) = result {
            self.chat.set_channel_notify(channel, previous);
            self.toast = Some(format!("Could not save notification settings: {e}"));
        }
        Task::none()
    }

    fn unpin(&mut self, c: ChannelId, m: MessageId) -> Task<Message> {
        if self.cfg.mock {
            self.panels.pins.remove(m);
            return Task::none();
        }
        let rest = self.rest.clone();
        Task::perform(async move { rest.unpin(c, m).await }, move |r| {
            Message::Done(Done::Unpinned {
                channel: c,
                message: m,
                result: r.map_err(err),
            })
        })
    }

    pub(super) fn on_unpinned(
        &mut self,
        channel: ChannelId,
        message: MessageId,
        result: Result<(), String>,
    ) -> Task<Message> {
        match result {
            Ok(()) if self.panels.pins.channel == Some(channel) => {
                self.panels.pins.remove(message);
            }
            Ok(()) => {}
            Err(e) => self.toast = Some(format!("Could not unpin the message: {e}")),
        }
        Task::none()
    }

    // -- mock data for the popovers ------------------------------------------------

    fn mock_pins(&self, c: ChannelId) -> Vec<Msg> {
        let mut v: Vec<Msg> = self
            .chat
            .messages
            .get(&c)
            .map(|w| {
                w.messages
                    .iter()
                    .filter(|m| !m.content.is_empty() && !m.pending)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        v.reverse();
        v.into_iter().step_by(2).take(3).collect()
    }

    fn mock_mentions(&self, guild: Option<GuildId>) -> Vec<Msg> {
        let mut v: Vec<Msg> = self
            .chat
            .messages
            .values()
            .flat_map(|w| w.messages.iter())
            .filter(|m| self.chat.mentions_me(m))
            .filter(|m| guild.is_none() || m.guild_id == guild)
            .cloned()
            .collect();
        v.sort_by_key(|m| std::cmp::Reverse(m.id));
        v
    }

    // -- add a server ---------------------------------------------------------------

    fn pick_server_icon(&mut self) -> Task<Message> {
        if self.cfg.mock {
            return Task::none();
        }
        Task::perform(pick_icon_file(), |r| Message::Done(Done::IconPicked(r)))
    }

    pub(super) fn on_icon_picked(
        &mut self,
        r: Option<Result<(std::path::PathBuf, String), String>>,
    ) -> Task<Message> {
        if let Some(ServerDialog::Create(f)) = self.server_dialog.as_mut() {
            match r {
                Some(Ok((path, uri))) => {
                    f.icon_path = Some(path);
                    f.icon_uri = Some(uri);
                    f.error = None;
                }
                Some(Err(e)) => f.error = Some(e),
                None => {}
            }
        }
        Task::none()
    }

    fn submit_create(&mut self) -> Task<Message> {
        let Some(ServerDialog::Create(f)) = self.server_dialog.as_mut() else {
            return Task::none();
        };
        if !f.can_submit() {
            return Task::none();
        }
        f.busy = true;
        f.error = None;
        let (name, icon) = (f.name.trim().to_owned(), f.icon_uri.clone());
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Created(Ok((Snowflake(900_001), name)))));
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.create_guild(&name, icon.as_deref()).await },
            |r| Message::Done(Done::Created(r.map_err(err))),
        )
    }

    pub(super) fn on_created(&mut self, r: Result<(GuildId, String), String>) -> Task<Message> {
        match r {
            Ok((id, name)) => self.entered_server(id, name),
            Err(e) => {
                if let Some(ServerDialog::Create(f)) = self.server_dialog.as_mut() {
                    f.busy = false;
                    f.error = Some(e);
                }
                Task::none()
            }
        }
    }

    /// A server was created or joined: close the dialog and open it as
    /// soon as the gateway delivers it (immediately when we already have
    /// it).
    fn entered_server(&mut self, id: GuildId, name: String) -> Task<Message> {
        self.server_dialog = None;
        if self.cfg.mock {
            // No gateway in mock mode: add the server locally.
            let guild = Guild {
                id,
                name,
                icon: None,
                roles: vec![],
                owner_id: self.chat.me_id(),
                emojis: vec![],
                stickers: vec![],
                premium_tier: 0,
            };
            let ctx = self.ctx();
            let _ = self.chat.apply_event(
                &crate::net::Event::GuildUpsert {
                    guild: Box::new(guild),
                    channels: vec![],
                    partial: false,
                    voice_states: vec![],
                },
                &ctx,
            );
        }
        if self.chat.guild(id).is_some() {
            self.pending_select = None;
            return self.update_ui(Ui::SelectGuild(id));
        }
        self.pending_select = Some(id);
        Task::none()
    }

    fn submit_join_lookup(&mut self) -> Task<Message> {
        let Some(ServerDialog::Join(f)) = self.server_dialog.as_mut() else {
            return Task::none();
        };
        // "Continue" on a preview joins it.
        if matches!(f.step, JoinStep::Preview(_)) {
            return self.confirm_join();
        }
        let Some((generation, code)) = f.begin_resolve() else {
            return Task::none();
        };
        if self.cfg.mock {
            let preview = crate::mock::invite_preview(&code);
            return Task::done(Message::Done(Done::Invite {
                generation,
                result: preview.ok_or_else(|| "This invite is invalid or has expired.".into()),
            }));
        }
        let rest = self.rest.clone();
        Task::perform(async move { rest.invite(&code).await }, move |r| {
            Message::Done(Done::Invite {
                generation,
                result: r.map_err(|e| invite_error_text(&e, false)),
            })
        })
    }

    pub(super) fn on_invite(
        &mut self,
        generation: u64,
        result: Result<extras::InvitePreview, String>,
    ) -> Task<Message> {
        if let Some(ServerDialog::Join(f)) = self.server_dialog.as_mut() {
            f.resolved(generation, result);
        }
        self.request_images()
    }

    fn confirm_join(&mut self) -> Task<Message> {
        let Some(ServerDialog::Join(f)) = self.server_dialog.as_mut() else {
            return Task::none();
        };
        let Some((generation, code)) = f.begin_join() else {
            return Task::none();
        };
        let (gid, name) = match &f.step {
            JoinStep::Joining(p) => (p.guild_id, p.name.clone()),
            _ => return Task::none(),
        };
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Joined {
                generation,
                result: Ok((gid, name)),
            }));
        }
        let rest = self.rest.clone();
        let session = self.session_id.clone();
        Task::perform(
            async move { rest.join_invite(&code, Some(&session)).await },
            move |r| {
                Message::Done(Done::Joined {
                    generation,
                    result: r.map_err(|e| invite_error_text(&e, true)),
                })
            },
        )
    }

    pub(super) fn on_joined(
        &mut self,
        generation: u64,
        result: Result<(GuildId, String), String>,
    ) -> Task<Message> {
        match result {
            Ok((id, name)) => self.entered_server(id, name),
            Err(e) => {
                if let Some(ServerDialog::Join(f)) = self.server_dialog.as_mut() {
                    f.join_failed(generation, e);
                }
                Task::none()
            }
        }
    }

    // -- discover -----------------------------------------------------------------------

    fn open_discover(&mut self) -> Task<Message> {
        self.close_overlays();
        self.server_dialog = None;
        if self.discover.is_some() {
            return Task::none();
        }
        self.discover = Some(DiscoverState::default());
        let list = Task::batch([
            self.load_discover(0),
            iced::widget::operation::focus(crate::ui::servers::discover_search_id()),
        ]);
        if self.cfg.mock {
            if let Some(d) = self.discover.as_mut() {
                d.categories = crate::mock::discover_categories();
            }
            return list;
        }
        let rest = self.rest.clone();
        Task::batch([
            list,
            Task::perform(async move { rest.discovery_categories().await }, |r| {
                Message::Done(Done::DiscoverCategories(r.map_err(err)))
            }),
        ])
    }

    fn load_discover(&mut self, offset: u32) -> Task<Message> {
        let Some(d) = self.discover.as_mut() else {
            return Task::none();
        };
        let (generation, query, category, offset) = d.begin(offset);
        if self.cfg.mock {
            let page = crate::mock::discover_page(&query, category);
            d.finish(generation, Ok(page));
            return Task::none();
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.discover(&query, category, offset).await },
            move |r| {
                Message::Done(Done::DiscoverPage {
                    generation,
                    result: r.map_err(err),
                })
            },
        )
    }

    pub(super) fn on_discover_page(
        &mut self,
        generation: u64,
        result: Result<extras::DiscoverPage, String>,
    ) -> Task<Message> {
        if let Some(d) = self.discover.as_mut() {
            d.finish(generation, result);
        }
        self.request_images()
    }

    fn discover_join(&mut self, g: GuildId) -> Task<Message> {
        if self.chat.guild(g).is_some() {
            // Already a member: just open it.
            self.discover = None;
            return self.update_ui(Ui::SelectGuild(g));
        }
        let Some(d) = self.discover.as_mut() else {
            return Task::none();
        };
        if d.joining.is_some() {
            return Task::none();
        }
        d.joining = Some(g);
        if self.cfg.mock {
            return Task::done(Message::Done(Done::DiscoverJoined {
                guild: g,
                result: Ok(()),
            }));
        }
        let rest = self.rest.clone();
        let session = self.session_id.clone();
        Task::perform(
            async move { rest.join_discoverable(g, Some(&session)).await },
            move |r| {
                Message::Done(Done::DiscoverJoined {
                    guild: g,
                    result: r.map_err(|e| invite_error_text(&e, true)),
                })
            },
        )
    }

    pub(super) fn on_discover_joined(
        &mut self,
        guild: GuildId,
        result: Result<(), String>,
    ) -> Task<Message> {
        let name = self
            .discover
            .as_ref()
            .and_then(|d| d.guild(guild))
            .map(|g| g.name.clone())
            .unwrap_or_default();
        if let Some(d) = self.discover.as_mut() {
            d.joining = None;
        }
        match result {
            Ok(()) => {
                self.discover = None;
                self.entered_server(guild, name)
            }
            Err(e) => {
                if let Some(d) = self.discover.as_mut() {
                    d.error = Some(e);
                }
                Task::none()
            }
        }
    }

    // -- search filters -----------------------------------------------------------------

    /// Resolves `from:` / `in:` names against what the client knows.
    pub(super) fn parse_search(&self, input: &str) -> Result<SearchQuery, String> {
        let guild = match self.view {
            View::Guild { guild, .. } => Some(guild),
            View::Home(_) => None,
        };
        let user = |name: &str| {
            let n = name.to_lowercase();
            self.chat
                .users
                .values()
                .find(|u| u.username.to_lowercase() == n || u.display_name().to_lowercase() == n)
                .map(|u| u.id)
        };
        let channel = |name: &str| {
            let n = name.to_lowercase();
            self.chat
                .channels
                .values()
                .find(|c| {
                    c.guild_id == guild && c.name.as_deref().is_some_and(|x| x.to_lowercase() == n)
                })
                .map(|c| c.id)
        };
        extras::parse_search_query(input, user, channel)
    }
}

/// Server icon file dialog: `(path, data URI)`, the reason the file was
/// refused, or `None` when cancelled.
pub(super) async fn pick_icon_file() -> Option<Result<(std::path::PathBuf, String), String>> {
    let picked = rfd::AsyncFileDialog::new()
        .set_title("Choose a server icon")
        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
        .pick_file()
        .await?;
    let path = picked.path().to_path_buf();
    let too_big = tokio::fs::metadata(&path)
        .await
        .map(|m| m.len() > extras::MAX_ICON_BYTES as u64)
        .unwrap_or(false);
    if too_big {
        return Some(Err("That image is too large (4 MB at most).".to_owned()));
    }
    Some(match tokio::fs::read(&path).await {
        Ok(bytes) => icon_data_uri(&bytes)
            .map(|uri| (path, uri))
            .map_err(str::to_owned),
        Err(_) => Err("That file could not be read.".to_owned()),
    })
}
