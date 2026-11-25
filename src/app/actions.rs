//! Message actions: replies, edit, delete, reactions, emoji picker,
//! attachments and uploads, member list, status, search and jumps.
//!
//! These are `impl App` blocks split out of `mod.rs`; all the decisions
//! that can be pure live in `state.rs`, `emoji.rs`, `upload.rs`,
//! `search.rs` and `members.rs`.

use std::path::PathBuf;
use std::time::Duration;

use iced::{Point, Task};

use super::emoji::{EmojiPicker, PickerTarget};
use super::members::MemberListState;
use super::state::ReactionChange;
use super::upload::{self, ActiveUpload, Clipboard, UploadEvent};
use super::{App, Done, Message, PAGE, Ui, View, input, now_ms};
use crate::net::gateway::MemberSubscription;
use crate::net::model::Message as Msg;
use crate::net::model::*;
use crate::net::rest::{
    ReplyTarget, SEARCH_PAGE, SearchScope, UploadFile, check_upload, upload_limit,
};

/// "Replying to X" state above the composer.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplyDraft {
    pub channel: ChannelId,
    pub message: MessageId,
    pub author: User,
    /// "@ ON": ping the author.
    pub mention: bool,
}

/// An inline edit in progress.
#[derive(Debug, Clone)]
pub struct Editing {
    pub channel: ChannelId,
    pub message: MessageId,
    pub text: super::composer::Composer,
    pub original: String,
}

/// The open context menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MessageMenu {
    pub message: MessageId,
    pub at: Point,
}

/// Open emoji picker with the position it was opened at.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenPicker {
    pub picker: EmojiPicker,
    pub at: Point,
}

/// Largest pasted image we keep in memory.
const MAX_PASTE_BYTES: usize = 25 * 1024 * 1024;
/// Thumbnails are only decoded for images up to this size.
const MAX_PREVIEW_BYTES: u64 = 8 * 1024 * 1024;

pub fn composer_id() -> iced::widget::Id {
    iced::widget::Id::new("composer")
}

pub fn edit_id() -> iced::widget::Id {
    iced::widget::Id::new("edit-input")
}

pub fn search_id() -> iced::widget::Id {
    iced::widget::Id::new("search-input")
}

fn focus(id: iced::widget::Id) -> Task<Message> {
    iced::widget::operation::focus(id)
}

impl App {
    /// Whether a channel the user can act in is open.
    pub(super) fn open_chan(&self) -> Option<ChannelId> {
        self.open_channel()
            .filter(|c| self.chat.channels.contains_key(c))
    }

    fn is_mine(&self, m: &Msg) -> bool {
        Some(m.author.id) == self.chat.me_id() && !m.pending && !m.failed
    }

    /// Drops transient UI tied to the previous channel.
    pub(super) fn reset_channel_ui(&mut self) {
        self.reply = None;
        self.editing = None;
        self.menu = None;
        self.emoji_picker = None;
        // A picker left open would send to the newly opened channel.
        self.gif_picker = None;
        self.sticker_picker = None;
        self.pending_delete = None;
        self.highlight = None;
    }

    pub(super) fn update_action(&mut self, u: Ui) -> Task<Message> {
        let u = match self.on_ui_video(u) {
            Ok(task) => return task,
            Err(u) => u,
        };
        match u {
            Ui::StartReply(m) => self.start_reply(m),
            Ui::CancelReply => {
                self.reply = None;
                Task::none()
            }
            Ui::ToggleReplyMention => {
                let me = self.chat.me_id();
                if let Some(r) = self.reply.as_mut()
                    && Some(r.author.id) != me
                {
                    r.mention = !r.mention;
                }
                Task::none()
            }
            Ui::JumpTo { channel, message } => self.jump_to(channel, message),
            Ui::JumpToPresent => self.jump_to_present(),
            Ui::StartEdit(m) => self.start_edit(m),
            Ui::EditChanged(s) => {
                if let Some(e) = self.editing.as_mut() {
                    e.text.set_text(&s);
                }
                Task::none()
            }
            Ui::EditAction(a) => {
                if let Some(e) = self.editing.as_mut() {
                    e.text.perform(a);
                }
                Task::none()
            }
            Ui::SubmitEdit => self.submit_edit(),
            Ui::CancelEdit => {
                self.editing = None;
                focus(composer_id())
            }
            Ui::EditLast => {
                if !self.composer.is_empty() || self.editing.is_some() {
                    return Task::none();
                }
                let Some(c) = self.open_chan() else {
                    return Task::none();
                };
                let last = self.chat.messages.get(&c).and_then(|w| {
                    w.messages
                        .iter()
                        .rev()
                        .find(|m| self.is_mine(m) && matches!(m.kind, 0 | 19))
                        .map(|m| m.id)
                });
                match last {
                    Some(id) => self.start_edit(id),
                    None => Task::none(),
                }
            }
            Ui::DeleteClicked(m) => self.delete_clicked(m),
            Ui::ConfirmDelete => match self.pending_delete.take() {
                Some((c, m)) => self.delete_message(c, m),
                None => Task::none(),
            },
            Ui::CancelDelete => {
                self.pending_delete = None;
                Task::none()
            }
            Ui::Escape => self.escape(),
            Ui::ContextMenu(m) => {
                self.menu = Some(MessageMenu {
                    message: m,
                    at: input::cursor(),
                });
                self.emoji_picker = None;
                Task::none()
            }
            Ui::CloseMenu => {
                self.menu = None;
                Task::none()
            }
            Ui::CopyText(m) => {
                self.menu = None;
                let text = self
                    .open_chan()
                    .and_then(|c| self.chat.message(c, m))
                    .map(|m| m.content.clone())
                    .unwrap_or_default();
                iced::clipboard::write(text)
            }
            Ui::CopyLink(m) => {
                self.menu = None;
                let Some(c) = self.open_chan() else {
                    return Task::none();
                };
                let guild = self
                    .chat
                    .channels
                    .get(&c)
                    .and_then(|c| c.guild_id)
                    .map_or_else(|| "@me".to_owned(), |g| g.to_string());
                iced::clipboard::write(format!("https://discord.com/channels/{guild}/{c}/{m}"))
            }
            Ui::ToggleReaction(m, emoji) => match self.open_chan() {
                Some(c) => self.react(c, m, emoji),
                None => Task::none(),
            },
            Ui::OpenEmojiPicker(target) => {
                let guild = self
                    .open_chan()
                    .and_then(|c| self.chat.channels.get(&c))
                    .and_then(|c| c.guild_id)
                    .and_then(|g| self.chat.guild(g));
                let picker = EmojiPicker::open(target, guild);
                self.menu = None;
                self.emoji_picker = Some(OpenPicker {
                    picker,
                    at: input::cursor(),
                });
                // Focus the search box of the picker; fetch the server's
                // custom emoji images.
                Task::batch([
                    focus(iced::widget::Id::new("emoji-search")),
                    self.request_images(),
                ])
            }
            Ui::EmojiQuery(q) => {
                if let Some(p) = self.emoji_picker.as_mut() {
                    p.picker.query = q;
                }
                Task::none()
            }
            Ui::EmojiSection(s) => {
                if let Some(p) = self.emoji_picker.as_mut() {
                    p.picker.section = s;
                    p.picker.query.clear();
                    p.picker.locked = None;
                }
                // Another server's emoji images.
                self.request_images()
            }
            Ui::PickEmoji(e) => self.pick_emoji(e),
            Ui::SendLockedAsImage => self.send_locked_as_image(),
            Ui::DismissLocked => {
                if let Some(p) = self.emoji_picker.as_mut() {
                    p.picker.locked = None;
                }
                if let Some(p) = self.sticker_picker.as_mut() {
                    p.locked = None;
                }
                Task::none()
            }
            Ui::CloseEmojiPicker => {
                self.emoji_picker = None;
                Task::none()
            }
            Ui::OpenGifPicker => {
                if self.open_chan().is_none() {
                    return Task::none();
                }
                self.menu = None;
                self.emoji_picker = None;
                let (picker, generation) = super::gifs::GifPicker::open(input::cursor());
                self.gif_picker = Some(picker);
                self.gif_anims.clear();
                let fetch = self.fetch_gifs(generation, String::new());
                Task::batch([
                    focus(iced::widget::Id::new("gif-search")),
                    fetch,
                    self.load_gif_favorites(),
                    self.request_gif_anims(),
                    self.prefetch_gif_favorites(),
                ])
            }
            Ui::GifPage(page) => {
                if let Some(p) = self.gif_picker.as_mut() {
                    p.show(page);
                }
                // A new page starts at the top.
                self.gif_anims.scroll_y = 0.0;
                Task::batch([
                    iced::widget::operation::snap_to(
                        iced::widget::Id::new(super::gif_anim::GRID_ID),
                        iced::widget::operation::RelativeOffset::START,
                    ),
                    self.request_gif_thumbs(),
                    self.request_gif_anims(),
                ])
            }
            Ui::GifScrolled(y) => self.on_gif_scrolled(y),
            Ui::GifHoverEnter(url) => self.on_gif_hover(Some(url), None),
            Ui::GifHoverLeave(url) => self.on_gif_hover(None, Some(url)),
            Ui::ToggleGifFavorite(gif) => self.toggle_gif_favorite(&gif),
            Ui::GifQuery(q) => match self.gif_picker.as_mut() {
                Some(p) => delayed(
                    Done::GifSearchDue(p.set_query(q)),
                    super::gifs::SEARCH_DEBOUNCE_MS,
                ),
                None => Task::none(),
            },
            Ui::PickGif(url) => {
                self.gif_picker = None;
                match self.open_chan() {
                    Some(c) => self.send_text(c, url),
                    None => Task::none(),
                }
            }
            Ui::CloseGifPicker => {
                self.gif_picker = None;
                Task::none()
            }
            Ui::OpenStickerPicker => {
                if self.open_chan().is_none() {
                    return Task::none();
                }
                self.menu = None;
                self.emoji_picker = None;
                self.sticker_picker = Some(super::stickers::StickerPicker {
                    at: input::cursor(),
                    query: String::new(),
                    locked: None,
                });
                Task::batch([
                    focus(iced::widget::Id::new("sticker-search")),
                    self.request_images(),
                ])
            }
            Ui::StickerQuery(q) => {
                if let Some(p) = self.sticker_picker.as_mut() {
                    p.query = q;
                }
                Task::none()
            }
            Ui::PickSticker(id) => {
                let access = self.nitro_access();
                let locked = super::stickers::find_with_guild(&self.chat.guilds, id)
                    .is_some_and(|(g, s)| access.sticker(g, s).locked());
                if locked {
                    // Never sent as `sticker_ids` where the API would refuse
                    // it: the picker shows the lock and the explicit choice.
                    if let Some(p) = self.sticker_picker.as_mut() {
                        p.locked = Some(id);
                    }
                    return Task::none();
                }
                self.sticker_picker = None;
                self.send_sticker(id)
            }
            Ui::CloseStickerPicker => {
                self.sticker_picker = None;
                Task::none()
            }
            Ui::PickFiles => {
                if self.open_chan().is_none() {
                    return Task::none();
                }
                if self.cfg.mock {
                    self.toast = Some("File dialogs are disabled in mock mode.".into());
                    return Task::none();
                }
                Task::perform(
                    async {
                        // Native chooser through the desktop portal.
                        let picked = rfd::AsyncFileDialog::new()
                            .set_title("Upload files")
                            .pick_files()
                            .await;
                        let paths: Vec<PathBuf> = picked
                            .unwrap_or_default()
                            .into_iter()
                            .map(|f| f.path().to_path_buf())
                            .collect();
                        tokio::task::spawn_blocking(move || stat_paths(paths, false))
                            .await
                            .unwrap_or_default()
                    },
                    |r| Message::Done(Done::FilesReady(r)),
                )
            }
            Ui::FileDropped(path) => {
                if self.open_chan().is_none() || self.editing.is_some() {
                    return Task::none();
                }
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || stat_paths(vec![path], false))
                            .await
                            .unwrap_or_default()
                    },
                    |r| Message::Done(Done::FilesReady(r)),
                )
            }
            Ui::Paste => {
                if self.open_chan().is_none() || self.editing.is_some() || self.cfg.mock {
                    return Task::none();
                }
                Task::perform(
                    async {
                        tokio::task::spawn_blocking(|| {
                            match upload::read_clipboard_blocking(MAX_PASTE_BYTES) {
                                Clipboard::Files(p) => (stat_paths(p.clone(), false), p),
                                Clipboard::Image(bytes) => {
                                    let r = upload::save_pasted_image(&bytes)
                                        .map_err(|_| "Could not save the pasted image.".to_owned())
                                        .and_then(|p| upload::file_from_path(&p))
                                        .map(|mut f| {
                                            f.name = "image.png".into();
                                            (f, true)
                                        });
                                    (vec![r], vec![])
                                }
                                Clipboard::Nothing => (vec![], vec![]),
                            }
                        })
                        .await
                        .unwrap_or_default()
                    },
                    |(files, pasted)| Message::Done(Done::Pasted { files, pasted }),
                )
            }
            Ui::RemoveAttachment(i) => {
                if i < self.draft.len() {
                    let a = self.draft.remove(i);
                    return remove_temp(vec![a]);
                }
                Task::none()
            }
            Ui::CancelUpload(id) => {
                self.uploads.retain(|u| u.id != id);
                if let Some(h) = self.upload_handles.remove(&id) {
                    h.abort();
                }
                Task::none()
            }
            Ui::ToggleMembers => {
                self.members_open = !self.members_open;
                self.sync_members()
            }
            Ui::MembersScrolled(vp) => {
                let remaining =
                    vp.content_bounds().height - vp.bounds().height - vp.absolute_offset().y;
                if remaining < 300.0
                    && let Some(l) = &self.chat.member_list
                    && l.has_more()
                {
                    self.member_rows_wanted = (l.loaded_end() + super::members::RANGE_SIZE)
                        .min(super::members::MAX_RANGES * super::members::RANGE_SIZE);
                    return self.sync_members();
                }
                Task::none()
            }
            Ui::ToggleStatusMenu => {
                self.status_menu = !self.status_menu;
                Task::none()
            }
            Ui::SetStatus(p) => self.set_status(p),
            Ui::SearchChanged(s) => {
                self.search.input = s;
                self.search.open = true;
                Task::none()
            }
            Ui::SubmitSearch => self.run_search(0),
            Ui::SearchPage(next) => {
                let off = if next {
                    self.search.offset + SEARCH_PAGE
                } else {
                    self.search.offset.saturating_sub(SEARCH_PAGE)
                };
                self.run_search(off)
            }
            Ui::CloseSearch => {
                self.search.close();
                self.sync_members()
            }
            Ui::OpenChannel(c) => self.open_from_notification(c),
            // Handled in `mod.rs`.
            other => self.update_extras(other),
        }
    }

    // -- replies ----------------------------------------------------------------

    fn start_reply(&mut self, m: MessageId) -> Task<Message> {
        let Some(c) = self.open_chan() else {
            return Task::none();
        };
        let Some(msg) = self.chat.message(c, m).filter(|m| !m.pending && !m.failed) else {
            return Task::none();
        };
        let me = self.chat.me_id();
        self.reply = Some(ReplyDraft {
            channel: c,
            message: m,
            author: msg.author.clone(),
            // Like the official client: replying to yourself never pings.
            mention: Some(msg.author.id) != me,
        });
        self.menu = None;
        self.emoji_picker = None;
        focus(composer_id())
    }

    /// The reply to attach to the next message of `c`.
    pub(super) fn reply_target(&self, c: ChannelId) -> Option<ReplyTarget> {
        let r = self.reply.as_ref().filter(|r| r.channel == c)?;
        Some(ReplyTarget {
            message: r.message,
            guild: self.chat.channels.get(&c).and_then(|c| c.guild_id),
            mention: r.mention,
        })
    }

    // -- edit and delete ----------------------------------------------------------

    fn start_edit(&mut self, m: MessageId) -> Task<Message> {
        let Some(c) = self.open_chan() else {
            return Task::none();
        };
        let Some(msg) = self.chat.message(c, m) else {
            return Task::none();
        };
        if !self.is_mine(msg) || !matches!(msg.kind, 0 | 19) {
            return Task::none();
        }
        self.editing = Some(Editing {
            channel: c,
            message: m,
            text: super::composer::Composer::with_text(&msg.content),
            original: msg.content.clone(),
        });
        self.menu = None;
        self.emoji_picker = None;
        focus(edit_id())
    }

    fn submit_edit(&mut self) -> Task<Message> {
        let Some(e) = self.editing.clone() else {
            return Task::none();
        };
        let text = e.text.text().trim().to_owned();
        if text == e.original.trim() {
            self.editing = None;
            return focus(composer_id());
        }
        if text.is_empty() {
            // Like Discord: an empty edit offers to delete the message.
            self.editing = None;
            self.pending_delete = Some((e.channel, e.message));
            return Task::none();
        }
        if text.chars().count() > 2000 {
            self.toast = Some("Messages can be at most 2000 characters.".into());
            return Task::none();
        }
        self.editing = None;
        let (c, m) = (e.channel, e.message);
        if self.cfg.mock {
            self.chat.set_content(c, m, &text);
            return focus(composer_id());
        }
        let rest = self.rest.clone();
        tracing::info!(channel = %c, body = %crate::error::Redacted(&text), "editing message");
        Task::batch([
            focus(composer_id()),
            Task::perform(
                async move { rest.edit_message(c, m, &text).await },
                move |r| {
                    Message::Done(Done::Edited {
                        channel: c,
                        message: m,
                        result: r.map(Box::new).map_err(|e| e.user_message()),
                    })
                },
            ),
        ])
    }

    fn delete_clicked(&mut self, m: MessageId) -> Task<Message> {
        let Some(c) = self.open_chan() else {
            return Task::none();
        };
        let mine = self.chat.message(c, m).is_some_and(|msg| self.is_mine(msg));
        if !mine {
            return Task::none();
        }
        self.menu = None;
        if input::shift_down() {
            // Shift+click skips the confirmation, like the official client.
            return self.delete_message(c, m);
        }
        self.pending_delete = Some((c, m));
        Task::none()
    }

    /// The "X" on a message's embeds. Applied at once; put back if refused.
    pub(super) fn suppress_embeds(&mut self, m: MessageId) -> Task<Message> {
        let Some(c) = self.open_chan() else {
            return Task::none();
        };
        let allowed = self.chat.message(c, m).is_some_and(|msg| {
            super::embeds::can_suppress(msg, self.is_mine(msg), self.chat.can_manage_messages(c))
        });
        if !allowed {
            return Task::none();
        }
        let Some((embeds, old_flags, flags)) = self.chat.suppress_embeds(c, m) else {
            return Task::none();
        };
        if self.cfg.mock {
            return Task::none();
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.set_message_flags(c, m, flags).await },
            move |r| match r {
                Ok(_) => Message::Done(Done::Ignored),
                Err(e) => Message::Done(Done::SuppressFailed {
                    channel: c,
                    message: m,
                    embeds,
                    flags: old_flags,
                    error: e.user_message(),
                }),
            },
        )
    }

    fn delete_message(&mut self, c: ChannelId, m: MessageId) -> Task<Message> {
        self.chat.remove_message(c, m);
        if self.cfg.mock {
            return Task::none();
        }
        let rest = self.rest.clone();
        let store = self.store.clone();
        Task::perform(
            async move {
                let r = rest.delete_message(c, m).await;
                if r.is_ok() {
                    let _ = store.blocking(move |s| s.delete_messages(&[m])).await;
                }
                r
            },
            |r| match r {
                Ok(()) => Message::Done(Done::Ignored),
                Err(e) => Message::Done(Done::ActionFailed(format!(
                    "Could not delete the message: {}",
                    e.user_message()
                ))),
            },
        )
    }

    fn escape(&mut self) -> Task<Message> {
        if let Some(task) = self.escape_video() {
            return task;
        }
        if self.lightbox.take().is_some() {
            return Task::none();
        }
        if self.emoji_picker.take().is_some()
            || self.gif_picker.take().is_some()
            || self.sticker_picker.take().is_some()
            || self.menu.take().is_some()
        {
            return Task::none();
        }
        if self.profiles.popout.take().is_some() {
            self.anim.close_popout();
            return Task::none();
        }
        if self.rail.menu.take().is_some() || self.panels.open.take().is_some() {
            return Task::none();
        }
        if self.rail.folder_dialog.take().is_some()
            || self.server_dialog.take().is_some()
            || self.discover.take().is_some()
        {
            return Task::none();
        }
        if self.pending_delete.take().is_some() || self.pending_link.take().is_some() {
            return Task::none();
        }
        if self.member_dialog.is_some() || self.admin.roles.confirm_delete {
            return self.update_roles(super::role_admin::RoleUi::CloseDialog);
        }
        if self.perms_dialog.is_some() {
            return self.update_perms(super::channel_perms::PermsUi::Close);
        }
        if self.channel_dialog.is_some() {
            return self.update_channel_admin(super::channel_admin::ChannelUi::Close);
        }
        if self.admin.dialog.is_some() {
            return self.update_admin(super::guild_admin::AdminUi::CloseDialog);
        }
        if self.admin.settings.take().is_some() {
            return Task::none();
        }
        if self.settings_open {
            self.settings_open = false;
            return self.close_settings_screen();
        }
        if self.status_menu {
            self.status_menu = false;
            return Task::none();
        }
        if self.editing.take().is_some() {
            return focus(composer_id());
        }
        if self.search.panel_visible() || self.search.open {
            self.search.close();
            return self.sync_members();
        }
        if self.reply.take().is_some() {
            return Task::none();
        }
        Task::none()
    }

    // -- reactions ------------------------------------------------------------------

    pub(super) fn react(&mut self, c: ChannelId, m: MessageId, emoji: Emoji) -> Task<Message> {
        let Some(change) = self.chat.toggle_reaction(c, m, &emoji) else {
            return Task::none();
        };
        if self.cfg.mock {
            return Task::none();
        }
        let rest = self.rest.clone();
        let add = change == ReactionChange::Added;
        Task::perform(
            {
                let emoji = emoji.clone();
                async move { rest.set_reaction(c, m, &emoji, add).await }
            },
            move |r| match r {
                Ok(()) => Message::Done(Done::Ignored),
                Err(e) => Message::Done(Done::ReactionFailed {
                    channel: c,
                    message: m,
                    emoji: emoji.clone(),
                    change,
                    error: e.user_message(),
                }),
            },
        )
    }

    // -- GIF picker ------------------------------------------------------------------

    pub(super) fn gif_search_due(&mut self, generation: u64) -> Task<Message> {
        match self.gif_picker.as_mut().and_then(|p| p.due(generation)) {
            Some(q) => self.fetch_gifs(generation, q),
            None => Task::none(),
        }
    }

    /// Trending GIFs for a blank `query`, else search results.
    fn fetch_gifs(&mut self, generation: u64, query: String) -> Task<Message> {
        if self.cfg.mock {
            if let Some(p) = self.gif_picker.as_mut() {
                p.finish(generation, Ok(crate::mock::gifs(&query)));
            }
            return Task::none();
        }
        let rest = self.rest.clone();
        Task::perform(async move { rest.gifs(&query).await }, move |r| {
            Message::Done(Done::Gifs {
                generation,
                result: r.map_err(|e| e.user_message()),
            })
        })
    }

    /// Loads the saved GIFs (`GET /users/@me/settings-proto/2`) once per
    /// session, when the picker first opens; saves keep the list current.
    fn load_gif_favorites(&mut self) -> Task<Message> {
        if self.gif_favorites.list.is_some() || self.cfg.mock {
            return Task::none();
        }
        let rest = self.rest.clone();
        let generation = self.gif_favorites.generation;
        Task::perform(
            async move {
                let raw = rest
                    .frecency_settings()
                    .await
                    .map_err(|e| e.user_message())?;
                crate::net::frecency::favorite_gifs(&raw)
                    .map_err(|_| "unreadable settings".to_owned())
            },
            move |result| {
                Message::Done(Done::GifFavorites {
                    generation,
                    result,
                    rollback: None,
                })
            },
        )
    }

    /// The star on a GIF: saved at once, written with a read-modify-write
    /// of the frecency settings, rolled back if that fails.
    fn toggle_gif_favorite(&mut self, gif: &crate::net::model::Gif) -> Task<Message> {
        let (change, generation, before) = self.gif_favorites.toggle(gif);
        if self.cfg.mock {
            let list = self.gif_favorites.list.clone().unwrap_or_default();
            self.gif_favorites.finish(generation, Ok(list), None);
            return Task::none();
        }
        let rest = self.rest.clone();
        let lock = self.gif_favorites_lock.clone();
        Task::perform(
            async move {
                // One save at a time, so two quick stars never overwrite
                // each other's read.
                let _guard = lock.lock().await;
                rest.change_favorite_gif(&change)
                    .await
                    .map_err(|e| e.user_message())
            },
            move |result| {
                Message::Done(Done::GifFavorites {
                    generation,
                    result,
                    rollback: Some(before),
                })
            },
        )
    }

    /// Fetches the stills of the GIFs the picker can show: its results and
    /// the saved GIFs, from the disk cache when there (see
    /// [`super::gif_anim`]). Each is decoded and downscaled off the UI
    /// thread.
    pub(super) fn request_gif_thumbs(&mut self) -> Task<Message> {
        if self.cfg.mock {
            return Task::none();
        }
        let Some(p) = &self.gif_picker else {
            return Task::none();
        };
        let mut gifs: Vec<crate::net::model::Gif> = Vec::new();
        if !p.searching() && p.page == super::gifs::GifPage::Home {
            // The tile backgrounds.
            gifs.extend(self.gif_favorites.items().into_iter().take(1));
            gifs.extend(p.results.iter().take(1).cloned());
        } else {
            gifs = self.picker_grid();
        }
        let tasks: Vec<Task<Message>> = gifs
            .into_iter()
            .take(60)
            .filter(|g| self.gif_thumbs.want(&g.url))
            .map(|g| {
                let rest = self.rest.clone();
                let disk = self.gif_disk.clone();
                let sources = super::gifs::thumb_sources(&g);
                Task::perform(
                    async move {
                        for src in sources {
                            match super::gif_anim::picker_bytes(&rest, disk.as_ref(), &src).await {
                                Ok(bytes) => {
                                    let still = tokio::task::spawn_blocking(move || {
                                        super::gifs::thumbnail(&bytes)
                                    })
                                    .await
                                    .ok()
                                    .flatten();
                                    if still.is_some() {
                                        return (g.url, still);
                                    }
                                    tracing::warn!(host = %media_host(&src), "GIF still did not decode");
                                }
                                Err(e) => {
                                    tracing::warn!(host = %media_host(&src), error = %e, "GIF still download failed");
                                }
                            }
                        }
                        (g.url, None)
                    },
                    |(key, image)| Message::Done(Done::GifThumb { key, image }),
                )
            })
            .collect();
        Task::batch(tasks)
    }

    // -- stickers -------------------------------------------------------------------

    /// Sends a sticker-only message to the open channel (as a reply if one
    /// is being composed). Errors show as a toast.
    fn send_sticker(&mut self, id: Snowflake) -> Task<Message> {
        let Some(c) = self.open_chan() else {
            return Task::none();
        };
        let nonce = Snowflake::from_timestamp_ms(now_ms(), fastrand::u64(0..4096)).to_string();
        let reply = self.reply_target(c);
        self.reply = None;
        if self.cfg.mock {
            let (Some(me), Some(sticker)) = (
                self.chat.me.clone(),
                super::stickers::find(&self.chat.guilds, id).cloned(),
            ) else {
                return Task::none();
            };
            let mut m =
                crate::mock::text_message(c, Snowflake::from_timestamp_ms(now_ms(), 0), me, "");
            m.stickers = vec![sticker];
            return Task::done(Message::Net(crate::net::Event::MessageCreate(Box::new(m))));
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.send_sticker(c, id, &nonce, reply).await },
            |r| match r {
                Ok(m) => Message::Net(crate::net::Event::MessageCreate(Box::new(m))),
                Err(e) => Message::Done(Done::ActionFailed(format!(
                    "Could not send the sticker: {}",
                    e.user_message()
                ))),
            },
        )
    }

    /// What the user may use in the open channel (Nitro, roles, server).
    pub fn nitro_access(&self) -> super::nitro::Access<'_> {
        super::nitro::Access {
            here: self
                .open_chan()
                .and_then(|c| self.chat.channels.get(&c))
                .and_then(|c| c.guild_id),
            premium_type: self.chat.premium_type,
            my_roles: &self.chat.my_roles,
            me: self.chat.me_id(),
        }
    }

    /// Whether custom emoji `id` is Nitro-locked in the open channel.
    fn emoji_locked(&self, id: Snowflake) -> bool {
        let access = self.nitro_access();
        super::emoji::find_custom(&self.chat.guilds, id)
            .is_some_and(|(g, e)| access.emoji(g, e).locked())
    }

    /// "Send as image" for the locked emoji or sticker of the open picker.
    fn send_locked_as_image(&mut self) -> Task<Message> {
        use super::nitro::{EMOJI_IMAGE_SIZE, STICKER_IMAGE_SIZE};
        let cdn = self.cfg.cdn_url.clone();
        if let Some(open) = &self.emoji_picker
            && open.picker.target == PickerTarget::Composer
            && let Some(e) = open.picker.locked.clone()
        {
            self.emoji_picker = None;
            let Some(url) = super::nitro::emoji_image_url(&cdn, &e) else {
                return Task::none();
            };
            return self.send_image_copy(url, e.name, EMOJI_IMAGE_SIZE);
        }
        if let Some(id) = self.sticker_picker.as_ref().and_then(|p| p.locked) {
            let Some(s) = super::stickers::find(&self.chat.guilds, id).cloned() else {
                return Task::none();
            };
            let Some(url) = s.image_url(&cdn) else {
                // Lottie: the footer explains it; nothing to send.
                return Task::none();
            };
            self.sticker_picker = None;
            return self.send_image_copy(url, s.name, STICKER_IMAGE_SIZE);
        }
        Task::none()
    }

    /// Downloads `url`, scales it to fit `max` and uploads it to the open
    /// channel as an ordinary attachment (a reply if one is being
    /// composed), like sending a sticker.
    fn send_image_copy(&mut self, url: String, name: String, max: u32) -> Task<Message> {
        let Some(c) = self.open_chan() else {
            return Task::none();
        };
        let reply = self.reply_target(c);
        self.reply = None;
        let mock = self.cfg.mock;
        let rest = self.rest.clone();
        Task::perform(
            async move {
                let bytes = if mock {
                    // No network in mock mode: a drawn stand-in.
                    let (n, px) = crate::mock::mock_emoji([88, 101, 242]);
                    upload::encode_png(n as usize, n as usize, &px, usize::MAX)
                        .ok_or_else(|| "The image could not be converted.".to_owned())?
                } else {
                    rest.fetch_bytes(&url).await.map_err(|e| e.user_message())?
                };
                tokio::task::spawn_blocking(move || {
                    let img = super::nitro::scale_for_upload(&bytes, max)
                        .map_err(|e| e.user_message().to_owned())?;
                    let path = upload::save_temp_file(&img.bytes, "image", img.ext)
                        .map_err(|_| "Could not save the image.".to_owned())?;
                    Ok(UploadFile {
                        path,
                        name: super::nitro::image_file_name(&name, img.ext),
                        size: img.bytes.len() as u64,
                    })
                })
                .await
                .map_err(|_| "The image could not be converted.".to_owned())?
            },
            move |result| {
                Message::Done(Done::ImageCopy {
                    channel: c,
                    reply,
                    result,
                })
            },
        )
    }

    /// The scaled copy is ready: upload it now (deleted afterwards).
    pub(super) fn on_image_copy(
        &mut self,
        c: ChannelId,
        reply: Option<ReplyTarget>,
        result: Result<UploadFile, String>,
    ) -> Task<Message> {
        match result {
            Ok(file) => {
                let temp = vec![upload::DraftAttachment::from_file(file.clone(), true)];
                self.upload_now(c, String::new(), reply, vec![file], temp)
            }
            Err(e) => {
                self.toast = Some(format!("Could not send the image: {e}"));
                Task::none()
            }
        }
    }

    fn pick_emoji(&mut self, e: Emoji) -> Task<Message> {
        if let Some(id) = e.id
            && self.emoji_locked(id)
        {
            // Never inserted as `<:name:id>` where it would not work: the
            // picker stays open with the lock and the explicit choice.
            if let Some(open) = self.emoji_picker.as_mut() {
                open.picker.locked = Some(e);
            }
            return Task::none();
        }
        let Some(open) = self.emoji_picker.take() else {
            return Task::none();
        };
        match open.picker.target {
            PickerTarget::Composer => {
                self.composer.push_str(&format!("{} ", e.composer_text()));
                focus(composer_id())
            }
            PickerTarget::Reaction { channel, message } => {
                // Picking an emoji you already used removes it, like the
                // official picker does.
                self.react(channel, message, e)
            }
        }
    }

    // -- attachments and uploads ----------------------------------------------------

    /// Upload limit for the open channel.
    pub fn upload_limit(&self) -> u64 {
        let tier = self
            .open_chan()
            .and_then(|c| self.chat.channels.get(&c))
            .and_then(|c| c.guild_id)
            .and_then(|g| self.chat.guild(g))
            .map_or(0, |g| g.premium_tier);
        upload_limit(self.chat.premium_type, tier)
    }

    pub(super) fn on_files_ready(
        &mut self,
        files: Vec<Result<(UploadFile, bool), String>>,
    ) -> Task<Message> {
        let mut ok = Vec::new();
        let mut errors = Vec::new();
        for f in files {
            match f {
                Ok(f) => ok.push(f),
                Err(e) => errors.push(e),
            }
        }
        let before = self.draft.len();
        let limit = self.upload_limit();
        let outcome = upload::add_to_draft(&mut self.draft, ok, limit);
        errors.extend(outcome.rejected);
        if let Some(first) = errors.first() {
            self.toast = Some(first.clone());
        }
        // Thumbnails for newly added images.
        let mut tasks = vec![focus(composer_id())];
        for a in self.draft.iter().skip(before).filter(|a| a.is_image) {
            let key = a.key();
            let path = a.file.path.clone();
            let size = a.file.size;
            if size > MAX_PREVIEW_BYTES || !self.images.want(&key) {
                continue;
            }
            tasks.push(Task::perform(
                async move {
                    let bytes = tokio::task::spawn_blocking(move || {
                        let b = std::fs::read(&path).ok()?;
                        crate::net::rest::check_image_dimensions(&b).ok()?;
                        Some(b)
                    })
                    .await
                    .ok()
                    .flatten();
                    (key, bytes)
                },
                |(key, bytes)| Message::Done(Done::Image { key, bytes }),
            ));
        }
        Task::batch(tasks)
    }

    /// Starts uploading the draft with `content` as the caption.
    pub(super) fn start_upload(&mut self, c: ChannelId, content: String) -> Task<Message> {
        let files: Vec<UploadFile> = self.draft.iter().map(|d| d.file.clone()).collect();
        if let Err(e) = check_upload(&files, self.upload_limit()) {
            self.toast = Some(upload::upload_error_text(&e));
            return Task::none();
        }
        let reply = self.reply_target(c);
        let temp: Vec<upload::DraftAttachment> =
            self.draft.iter().filter(|d| d.temporary).cloned().collect();
        self.draft.clear();
        self.reply = None;
        self.composer.clear();
        self.upload_now(c, content, reply, files, temp)
    }

    /// Uploads `files` with `content` to `c`, showing a progress strip;
    /// `temp` files are deleted afterwards.
    fn upload_now(
        &mut self,
        c: ChannelId,
        content: String,
        reply: Option<ReplyTarget>,
        files: Vec<UploadFile>,
        temp: Vec<upload::DraftAttachment>,
    ) -> Task<Message> {
        if let Err(e) = check_upload(&files, self.upload_limit()) {
            self.toast = Some(upload::upload_error_text(&e));
            return Task::none();
        }
        self.next_upload_id += 1;
        let id = self.next_upload_id;
        let total: u64 = files.iter().map(|f| f.size).sum();
        self.uploads.push(ActiveUpload {
            id,
            channel: c,
            label: ActiveUpload::label_for(&files),
            sent: 0,
            total,
        });
        let nonce = Snowflake::from_timestamp_ms(now_ms(), fastrand::u64(0..4096)).to_string();
        let stream = if self.cfg.mock {
            mock_upload_stream(self.chat.me.clone(), c, content, files, total)
        } else {
            real_upload_stream(self.rest.clone(), c, content, nonce, reply, files, temp)
        };
        let (task, handle) = Task::stream(futures::StreamExt::map(stream, move |event| {
            Message::Done(Done::Upload { id, event })
        }))
        .abortable();
        self.upload_handles.insert(id, handle);
        task
    }

    pub(super) fn on_upload_event(&mut self, id: u64, event: UploadEvent) -> Task<Message> {
        match event {
            UploadEvent::Progress { sent, total } => {
                if let Some(u) = self.uploads.iter_mut().find(|u| u.id == id) {
                    u.sent = sent;
                    u.total = total.max(1);
                }
                Task::none()
            }
            UploadEvent::Done(result) => {
                self.uploads.retain(|u| u.id != id);
                self.upload_handles.remove(&id);
                match result {
                    Ok(m) => {
                        let ctx = self.ctx();
                        let ev = crate::net::Event::MessageCreate(m);
                        self.chat.apply_event(&ev, &ctx);
                        self.request_images()
                    }
                    Err(e) => {
                        self.toast = Some(e);
                        Task::none()
                    }
                }
            }
        }
    }

    /// Strips the pasted path text a file manager may also put on the
    /// clipboard as plain text.
    pub(super) fn strip_pasted_paths(&mut self, paths: &[PathBuf]) {
        for p in paths {
            let plain = p.display().to_string();
            for variant in [format!("file://{plain}"), plain] {
                let text = self.composer.text();
                if text.contains(&variant) {
                    self.composer.set_text(text.replace(&variant, "").trim());
                }
            }
        }
    }

    // -- member list and status -----------------------------------------------------

    /// Brings the member list subscription in line with the open view.
    pub(super) fn sync_members(&mut self) -> Task<Message> {
        let View::Guild {
            guild,
            channel: Some(channel),
        } = self.view
        else {
            self.chat.member_list = None;
            self.gateway.clear_members();
            return Task::none();
        };
        let visible = self.members_open && !self.search.panel_visible();
        if !visible {
            if !self.members_open {
                self.gateway.clear_members();
            }
            return Task::none();
        }
        if self.chat.member_list.as_ref().map(|l| l.guild) != Some(guild) {
            let mut list = MemberListState::new(guild);
            if self.cfg.mock && guild == crate::mock::GUILD_RUST {
                list.apply(&crate::mock::member_list());
            }
            self.chat.member_list = Some(list);
            self.member_rows_wanted = super::members::RANGE_SIZE;
        }
        if !self.cfg.mock {
            self.gateway.subscribe_members(MemberSubscription {
                guild,
                channel,
                ranges: MemberListState::ranges_for(self.member_rows_wanted),
            });
        }
        Task::none()
    }

    /// Status menu, tray and settings: applied at once (op 3) and saved
    /// in the user settings proto (`status.status`), like the official
    /// client.
    fn set_status(&mut self, p: Presence) -> Task<Message> {
        use crate::net::user_settings::{Edit, Synced, Value};
        self.status_menu = false;
        self.edit_synced(Edit::Set(Synced::Status, Value::Str(p.wire().into())))
    }

    // -- search and jumps ------------------------------------------------------------

    fn run_search(&mut self, offset: u32) -> Task<Message> {
        let scope = match self.view {
            View::Guild { guild, .. } => SearchScope::Guild(guild),
            View::Home(Some(c)) => SearchScope::Channel(c),
            View::Home(None) => return Task::none(),
        };
        let Some((generation, scope, query, offset)) = self.search.begin(scope, offset) else {
            return Task::none();
        };
        // `from:name`, `has:image`, `in:#channel` become API filters.
        let parsed = match self.parse_search(&query) {
            Ok(q) => q,
            Err(e) => {
                self.search.finish(generation, Err(e));
                return Task::none();
            }
        };
        if self.cfg.mock {
            let page = mock_search(&self.chat, scope, &parsed, offset);
            self.search.finish(generation, Ok(page));
            return Task::none();
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.search_query(scope, &parsed, offset).await },
            move |r| {
                Message::Done(Done::Search {
                    generation,
                    result: r.map_err(|e| e.user_message()),
                })
            },
        )
    }

    pub(super) fn on_search_done(
        &mut self,
        generation: u64,
        result: Result<SearchPage, String>,
    ) -> Task<Message> {
        if let Ok(p) = &result {
            for m in &p.messages {
                self.chat
                    .users
                    .entry(m.author.id)
                    .or_insert_with(|| m.author.clone());
            }
        }
        self.search.finish(generation, result);
        self.request_search_images()
    }

    /// Avatars for the search results.
    fn request_search_images(&mut self) -> Task<Message> {
        self.request_images()
    }

    pub(super) fn jump_to(&mut self, channel: ChannelId, message: MessageId) -> Task<Message> {
        // Popovers (pins, inbox) and the Discover page close on a jump.
        self.panels.open = None;
        let mut tasks = Vec::new();
        if self.open_channel() != Some(channel) {
            tasks.push(self.update_ui(Ui::SelectChannel(channel)));
            if self.open_channel() != Some(channel) {
                return Task::batch(tasks);
            }
        }
        self.highlight = Some(message);
        if self.chat.message(channel, message).is_some() {
            tasks.push(delayed(Done::ScrollToMessage(message), 60));
            tasks.push(delayed(Done::ClearHighlight(message), 3000));
            return Task::batch(tasks);
        }
        if self.cfg.mock {
            return Task::batch(tasks);
        }
        let rest = self.rest.clone();
        tasks.push(Task::perform(
            async move { rest.messages_around(channel, message, PAGE).await },
            move |r| {
                Message::Done(Done::Around {
                    channel,
                    target: message,
                    result: r.map_err(|e| e.user_message()),
                })
            },
        ));
        Task::batch(tasks)
    }

    pub(super) fn on_around(
        &mut self,
        channel: ChannelId,
        target: MessageId,
        result: Result<Vec<Msg>, String>,
    ) -> Task<Message> {
        match result {
            Ok(msgs) if msgs.iter().any(|m| m.id == target) => {
                let last = self
                    .chat
                    .channels
                    .get(&channel)
                    .and_then(|c| c.last_message_id);
                let detached = last.is_some_and(|l| msgs.last().is_none_or(|m| m.id < l));
                for m in &msgs {
                    self.chat
                        .users
                        .entry(m.author.id)
                        .or_insert_with(|| m.author.clone());
                }
                self.chat
                    .messages
                    .entry(channel)
                    .or_default()
                    .replace(msgs, detached);
                Task::batch([
                    self.request_images(),
                    delayed(Done::ScrollToMessage(target), 80),
                    delayed(Done::ClearHighlight(target), 3000),
                ])
            }
            Ok(_) => {
                self.highlight = None;
                self.toast = Some("That message no longer exists.".into());
                Task::none()
            }
            Err(e) => {
                self.highlight = None;
                self.toast = Some(e);
                Task::none()
            }
        }
    }

    fn jump_to_present(&mut self) -> Task<Message> {
        let Some(c) = self.open_chan() else {
            return Task::none();
        };
        if let Some(w) = self.chat.messages.get_mut(&c) {
            w.messages.clear();
            w.detached = false;
            w.synced = false;
            w.loading = false;
            w.reached_start = false;
        }
        self.load_channel(c)
    }

    /// Loads messages newer than the detached window.
    pub(super) fn load_newer(&mut self) -> Task<Message> {
        let Some(c) = self.open_chan() else {
            return Task::none();
        };
        if self.cfg.mock {
            return Task::none();
        }
        let Some(w) = self.chat.messages.get_mut(&c) else {
            return Task::none();
        };
        if !w.detached || w.loading {
            return Task::none();
        }
        let Some(after) = w.newest() else {
            return Task::none();
        };
        w.loading = true;
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.messages_after(c, after, PAGE).await },
            move |r| {
                Message::Done(Done::Newer {
                    channel: c,
                    result: r.map_err(|e| e.user_message()),
                })
            },
        )
    }

    pub(super) fn on_newer(
        &mut self,
        channel: ChannelId,
        result: Result<Vec<Msg>, String>,
    ) -> Task<Message> {
        let Some(w) = self.chat.messages.get_mut(&channel) else {
            return Task::none();
        };
        w.loading = false;
        match result {
            Ok(msgs) => {
                let full = msgs.len() >= PAGE as usize;
                for m in msgs {
                    w.upsert(m);
                }
                if !full {
                    w.detached = false;
                }
                self.request_images()
            }
            Err(e) => {
                self.toast = Some(e);
                Task::none()
            }
        }
    }

    /// A notification was clicked: open the chat and bring the window
    /// back, re-opening it if it was hidden to the tray.
    fn open_from_notification(&mut self, c: ChannelId) -> Task<Message> {
        let nav = self.update_ui(Ui::SelectChannel(c));
        // Wayland compositors may only flash the task bar when the app has
        // no activation token.
        let raise = self.show_window();
        Task::batch([nav, raise])
    }
}

/// A message after `ms` milliseconds.
pub(super) fn delayed(d: Done, ms: u64) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            d
        },
        Message::Done,
    )
}

/// Stats paths (blocking; call from `spawn_blocking`).
fn stat_paths(paths: Vec<PathBuf>, temporary: bool) -> Vec<Result<(UploadFile, bool), String>> {
    paths
        .iter()
        .map(|p| upload::file_from_path(p).map(|f| (f, temporary)))
        .collect()
}

fn remove_temp(files: Vec<upload::DraftAttachment>) -> Task<Message> {
    let paths: Vec<PathBuf> = files
        .into_iter()
        .filter(|f| f.temporary)
        .map(|f| f.file.path)
        .collect();
    if paths.is_empty() {
        return Task::none();
    }
    Task::perform(
        async move {
            for p in paths {
                let _ = tokio::fs::remove_file(p).await;
            }
        },
        |_| Message::Done(Done::Ignored),
    )
}

type UploadFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<Box<Msg>, String>> + Send>>;

type UploadStream = std::pin::Pin<Box<dyn futures::Stream<Item = UploadEvent> + Send>>;

/// Real upload: the request future and its progress channel are polled by
/// the stream itself, so dropping (cancelling) the stream cancels the
/// request.
fn real_upload_stream(
    rest: crate::net::rest::RestClient,
    channel: ChannelId,
    content: String,
    nonce: String,
    reply: Option<ReplyTarget>,
    files: Vec<UploadFile>,
    temp: Vec<upload::DraftAttachment>,
) -> UploadStream {
    let total: u64 = files.iter().map(|f| f.size).sum();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
    let fut: UploadFuture = Box::pin(async move {
        tracing::info!(channel = %channel, files = files.len(), "uploading");
        let r = rest
            .send_with_files(channel, &content, &nonce, reply, &files, tx)
            .await;
        for t in temp {
            let _ = tokio::fs::remove_file(t.file.path).await;
        }
        r.map(Box::new).map_err(|e| e.user_message())
    });
    struct St {
        fut: Option<UploadFuture>,
        rx: tokio::sync::mpsc::UnboundedReceiver<u64>,
        total: u64,
    }
    Box::pin(futures::stream::unfold(
        St {
            fut: Some(fut),
            rx,
            total,
        },
        |mut st| async move {
            let fut = st.fut.as_mut()?;
            tokio::select! {
                biased;
                Some(sent) = st.rx.recv() => {
                    let total = st.total;
                    Some((UploadEvent::Progress { sent, total }, st))
                }
                r = fut => {
                    st.fut = None;
                    Some((UploadEvent::Done(r), st))
                }
            }
        },
    ))
}

/// Mock upload: three progress steps, then a local message with the files
/// as attachments. Never touches the network.
fn mock_upload_stream(
    me: Option<User>,
    channel: ChannelId,
    content: String,
    files: Vec<UploadFile>,
    total: u64,
) -> UploadStream {
    let steps = [0.25f64, 0.6, 0.9];
    let stream = futures::stream::unfold(0usize, move |i| {
        let me = me.clone();
        let content = content.clone();
        let files = files.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if i < steps.len() {
                let sent = (total as f64 * steps[i]) as u64;
                return Some((UploadEvent::Progress { sent, total }, i + 1));
            }
            if i == steps.len() {
                let now = now_ms();
                let mut m = crate::mock::text_message(
                    channel,
                    Snowflake::from_timestamp_ms(now, 7),
                    me.unwrap_or_else(|| crate::mock::user(1)),
                    &content,
                );
                m.attachments = files
                    .iter()
                    .enumerate()
                    .map(|(n, f)| Attachment {
                        id: Snowflake(9_000 + n as u64),
                        filename: f.name.clone(),
                        size: f.size,
                        url: format!("mock://upload/{n}"),
                        proxy_url: None,
                        content_type: None,
                        width: None,
                        height: None,
                    })
                    .collect();
                return Some((UploadEvent::Done(Ok(Box::new(m))), i + 1));
            }
            None
        }
    });
    Box::pin(stream)
}

/// Mock search: substring match over the loaded message windows.
pub(super) fn mock_search(
    chat: &super::state::ChatState,
    scope: SearchScope,
    query: &crate::net::extras::SearchQuery,
    offset: u32,
) -> SearchPage {
    let q = query.content.to_lowercase();
    let mut hits: Vec<Msg> = chat
        .messages
        .iter()
        .filter(|(c, _)| match scope {
            SearchScope::Channel(id) => **c == id,
            SearchScope::Guild(g) => chat.channels.get(c).and_then(|ch| ch.guild_id) == Some(g),
        })
        .flat_map(|(_, w)| w.messages.iter())
        .filter(|m| !m.pending && m.content.to_lowercase().contains(&q))
        .filter(|m| query.authors.is_empty() || query.authors.contains(&m.author.id))
        .filter(|m| query.channels.is_empty() || query.channels.contains(&m.channel_id))
        .filter(|m| {
            query.has.iter().all(|h| match *h {
                "image" => m.attachments.iter().any(|a| a.is_image()),
                "file" | "video" | "sound" => !m.attachments.is_empty(),
                "link" => m.content.contains("http://") || m.content.contains("https://"),
                "embed" => !m.embeds.is_empty(),
                _ => false,
            })
        })
        .cloned()
        .collect();
    hits.sort_by_key(|m| std::cmp::Reverse(m.id));
    let total = hits.len() as u32;
    let messages = hits
        .into_iter()
        .skip(offset as usize)
        .take(SEARCH_PAGE as usize)
        .collect();
    SearchPage { total, messages }
}

/// The host of a media URL, for logs (never the path: it can name the GIF).
fn media_host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| "?".into())
}
