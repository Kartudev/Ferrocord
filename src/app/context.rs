//! Right-click menus for users (member list, message authors, DM list)
//! and channels (channel list, DMs), plus the server actions of the rail
//! menu (mark as read, mute, @everyone suppression, copy ID).

use iced::{Point, Task};

use super::{App, Done, Message, Ui, input};
use crate::net::extras::{self, MuteDuration};
use crate::net::model::*;

/// What a context menu is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtxTarget {
    User {
        user: UserId,
        guild: Option<GuildId>,
    },
    Channel(ChannelId),
    /// The server dropdown (header chevron).
    Guild(GuildId),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CtxMenu {
    pub target: CtxTarget,
    pub at: Point,
}

impl App {
    pub(super) fn update_context(&mut self, u: Ui) -> Option<Task<Message>> {
        if matches!(u, Ui::MuteChannel(..) | Ui::UnmuteChannel(_)) {
            // Picked from a context menu: close it, the usual handler runs.
            self.ctx_menu = None;
            return None;
        }
        Some(match u {
            Ui::ContextOpen(target) => {
                self.close_overlays();
                self.profiles.popout = None;
                self.ctx_menu = Some(CtxMenu {
                    target,
                    at: input::cursor(),
                });
                Task::none()
            }
            Ui::ContextClose => {
                self.ctx_menu = None;
                Task::none()
            }
            Ui::CopyToClipboard(s) => {
                self.ctx_menu = None;
                self.rail.menu = None;
                iced::clipboard::write(s)
            }
            Ui::MentionUser(user) => {
                self.ctx_menu = None;
                let mut text = self.composer.text();
                if !text.is_empty() && !text.ends_with(char::is_whitespace) {
                    text.push(' ');
                }
                text.push_str(&format!("<@{user}> "));
                self.composer.set_text(&text);
                iced::widget::operation::focus(super::actions::composer_id())
            }
            Ui::MarkChannelRead(c) => {
                self.ctx_menu = None;
                self.mark_read(c)
            }
            Ui::MarkGuildRead(g) => {
                self.rail.menu = None;
                self.ctx_menu = None;
                let ids: Vec<ChannelId> = self
                    .chat
                    .channels
                    .values()
                    .filter(|c| c.guild_id == Some(g) && c.kind.is_textual())
                    .filter(|c| self.chat.is_unread(c.id) || self.chat.mention_count(c.id) > 0)
                    .map(|c| c.id)
                    .collect();
                Task::batch(
                    ids.into_iter()
                        .map(|c| self.mark_read(c))
                        .collect::<Vec<_>>(),
                )
            }
            Ui::MuteGuild(g, mute) => {
                self.rail.menu = None;
                self.ctx_menu = None;
                if mute {
                    self.chat.muted_guilds.insert(g);
                } else {
                    self.chat.muted_guilds.remove(&g);
                }
                let body = if mute {
                    extras::mute_override(MuteDuration::Forever, super::now_ms())
                } else {
                    extras::unmute_override()
                };
                self.save_guild_settings(g, body)
            }
            Ui::SuppressEveryone(g, on) => {
                self.rail.menu = None;
                self.ctx_menu = None;
                if on {
                    self.chat.suppress_everyone.insert(g);
                } else {
                    self.chat.suppress_everyone.remove(&g);
                }
                self.save_guild_settings(g, serde_json::json!({ "suppress_everyone": on }))
            }
            _ => return None,
        })
    }

    fn save_guild_settings(&mut self, g: GuildId, body: serde_json::Value) -> Task<Message> {
        if self.cfg.mock {
            return Task::none();
        }
        let rest = self.rest.clone();
        Task::perform(
            async move { rest.patch_guild_settings(Some(g), body).await },
            |r| match r {
                Ok(()) => Message::Done(Done::Ignored),
                Err(e) => Message::Done(Done::ActionFailed(format!(
                    "Could not save the server settings: {}",
                    e.user_message()
                ))),
            },
        )
    }
}
