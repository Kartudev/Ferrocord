//! Right-click menus: users, channels / DMs (and the shared card the
//! server menu in `ui::rail` is drawn with).

use iced::widget::{button, column, container, row, rule, text};
use iced::{Alignment, Length, Padding, Point, Size};

use super::icons::{Icon, icon};
use super::theme::{self, INTER_MEDIUM};

use super::{El, ui};
use crate::app::channel_admin::{ChannelUi, reorder};
use crate::app::context::CtxTarget;
use crate::app::guild_admin::AdminUi;
use crate::app::permissions::{BAN_MEMBERS, KICK_MEMBERS, MODERATE_MEMBERS};
use crate::app::role_admin::{ModKind, RoleUi};
use crate::app::{App, Ui, input};
use crate::net::extras::MuteDuration;
use crate::net::model::*;

pub const MENU_W: f32 = 220.0;
const ROW_H: f32 = 32.0;
const SEP_H: f32 = 9.0;

/// One menu line.
pub enum Row {
    Item {
        icon: Icon,
        label: String,
        action: Ui,
        danger: bool,
    },
    Separator,
}

impl Row {
    pub fn item(icon: Icon, label: impl Into<String>, action: Ui) -> Self {
        Self::Item {
            icon,
            label: label.into(),
            action,
            danger: false,
        }
    }

    pub fn danger(icon: Icon, label: impl Into<String>, action: Ui) -> Self {
        Self::Item {
            icon,
            label: label.into(),
            action,
            danger: true,
        }
    }
}

/// The menu card for `rows`, placed at `at` (clamped to the window).
pub fn card(app: &App, rows: Vec<Row>, at: Point) -> (El<'_>, Point) {
    let p = app.theme.palette;
    let mut height = 16.0;
    let mut col = column![].spacing(2).padding(8);
    for r in rows {
        match r {
            Row::Separator => {
                height += SEP_H;
                col = col.push(
                    container(rule::horizontal(1).style(move |_| rule::Style {
                        color: p.bg_selected,
                        radius: 0.0.into(),
                        fill_mode: rule::FillMode::Full,
                        snap: true,
                    }))
                    .padding(Padding::from([3, 4])),
                );
            }
            Row::Item {
                icon: i,
                label,
                action,
                danger,
            } => {
                height += ROW_H;
                let fg = if danger { p.danger } else { p.text_secondary };
                col = col.push(
                    button(
                        row![
                            text(label).size(14).font(INTER_MEDIUM).width(Length::Fill),
                            icon(i, 18.0, fg),
                        ]
                        .align_y(Alignment::Center),
                    )
                    .padding(Padding::from([6, 8]))
                    .width(Length::Fill)
                    .on_press(ui(action))
                    .style(theme::menu_item(p, danger)),
                );
            }
        }
    }
    let at = input::clamp_popup(at, Size::new(MENU_W, height), input::window_size());
    (
        container(col).width(MENU_W).style(theme::popup(p)).into(),
        at,
    )
}

/// The open user / channel menu.
pub fn view(app: &App) -> Option<(El<'_>, Point)> {
    let m = app.ctx_menu?;
    let rows = match m.target {
        CtxTarget::User { user, guild } => user_rows(app, user, guild),
        CtxTarget::Channel(c) => channel_rows(app, c),
        CtxTarget::Guild(g) => {
            let mut rows = guild_rows(app, g);
            // `guild_rows` ends with a separator for the rail's folder items.
            if matches!(rows.last(), Some(Row::Separator)) {
                rows.pop();
            }
            rows
        }
    };
    if rows.is_empty() {
        return None;
    }
    Some(card(app, rows, m.at))
}

#[cfg(test)]
pub(super) fn user_rows_for_tests(app: &App, user: UserId) -> Vec<Row> {
    user_rows(app, user, None)
}

fn user_rows(app: &App, user: UserId, guild: Option<GuildId>) -> Vec<Row> {
    let mut rows = vec![Row::item(
        Icon::UserRound,
        "Profile",
        Ui::OpenProfile { user, guild },
    )];
    if app.open_channel().is_some() {
        rows.push(Row::item(Icon::AtSign, "Mention", Ui::MentionUser(user)));
    }
    if let Some(g) = guild {
        rows.extend(member_rows(app, g, user));
    }
    rows.push(Row::Separator);
    if let Some(u) = app.chat.users.get(&user) {
        rows.push(Row::item(
            Icon::Copy,
            "Copy Username",
            Ui::CopyToClipboard(u.username.clone()),
        ));
    }
    // "Copy ID" items exist only in Developer Mode, like the official
    // client.
    if app.developer_mode() {
        rows.push(Row::item(
            Icon::Copy,
            "Copy User ID",
            Ui::CopyToClipboard(user.to_string()),
        ));
    }
    rows
}

/// Roles / Timeout / Kick / Ban items for a server member, each shown
/// only with the permission and a higher rank than the member.
fn member_rows(app: &App, g: GuildId, user: UserId) -> Vec<Row> {
    let mut rows = Vec::new();
    // Like Discord, roles below one's own can be given to anyone.
    if !app.assignable_roles(g).is_empty() {
        rows.push(Row::item(
            Icon::ShieldCheck,
            "Roles",
            Ui::Roles(RoleUi::OpenMemberRoles { guild: g, user }),
        ));
    }
    let name = app.chat.user_name(user).unwrap_or("Member").to_string();
    let modr = |kind| {
        Ui::Roles(RoleUi::OpenModeration {
            guild: g,
            user,
            kind,
        })
    };
    if app.can_moderate(g, user, MODERATE_MEMBERS) {
        rows.push(Row::item(
            Icon::BellOff,
            format!("Timeout {name}"),
            modr(ModKind::Timeout),
        ));
    }
    if app.can_moderate(g, user, KICK_MEMBERS) {
        rows.push(Row::danger(
            Icon::LogOut,
            format!("Kick {name}"),
            modr(ModKind::Kick),
        ));
    }
    if app.can_moderate(g, user, BAN_MEMBERS) {
        rows.push(Row::danger(
            Icon::X,
            format!("Ban {name}"),
            modr(ModKind::Ban),
        ));
    }
    if !rows.is_empty() {
        rows.insert(0, Row::Separator);
    }
    rows
}

fn channel_rows(app: &App, c: ChannelId) -> Vec<Row> {
    let Some(ch) = app.chat.channels.get(&c) else {
        return vec![];
    };
    let mut rows = Vec::new();
    if ch.kind == ChannelKind::Dm
        && let Some(u) = ch.recipients.first()
    {
        rows.push(Row::item(
            Icon::UserRound,
            "Profile",
            Ui::OpenProfile {
                user: *u,
                guild: None,
            },
        ));
    }
    if ch.kind.is_textual() {
        if app.chat.is_unread(c) || app.chat.mention_count(c) > 0 {
            rows.push(Row::item(
                Icon::Check,
                "Mark As Read",
                Ui::MarkChannelRead(c),
            ));
        }
        let what = if ch.guild_id.is_some() {
            "Channel"
        } else {
            "Conversation"
        };
        if app.chat.is_channel_muted(c) {
            rows.push(Row::item(
                Icon::Bell,
                format!("Unmute {what}"),
                Ui::UnmuteChannel(c),
            ));
        } else {
            rows.push(Row::item(
                Icon::BellOff,
                format!("Mute {what}"),
                Ui::MuteChannel(c, MuteDuration::Forever),
            ));
        }
    }
    rows.extend(channel_admin_rows(app, ch));
    if matches!(rows.first(), Some(Row::Separator)) {
        rows.remove(0);
    }
    if !app.developer_mode() {
        return rows;
    }
    if !rows.is_empty() {
        rows.push(Row::Separator);
    }
    if ch.kind == ChannelKind::Dm
        && let Some(u) = ch.recipients.first()
    {
        rows.push(Row::item(
            Icon::Copy,
            "Copy User ID",
            Ui::CopyToClipboard(u.to_string()),
        ));
    }
    rows.push(Row::item(
        Icon::Copy,
        "Copy Channel ID",
        Ui::CopyToClipboard(c.to_string()),
    ));
    rows
}

/// Edit / move / delete items of a server channel or category, for users
/// with Manage Channels there.
fn channel_admin_rows(app: &App, ch: &Channel) -> Vec<Row> {
    let Some(g) = ch.guild_id else {
        return vec![];
    };
    if !app.can_manage_channel(ch.id) {
        return vec![];
    }
    let c = |u: ChannelUi| Ui::Channel(u);
    let category = ch.kind == ChannelKind::Category;
    let what = if category { "Category" } else { "Channel" };
    let mut rows = vec![Row::Separator];
    if category && app.can_manage_channels(g) {
        rows.push(Row::item(
            Icon::Plus,
            "Create Channel",
            c(ChannelUi::OpenCreate {
                guild: g,
                parent: Some(ch.id),
                category: false,
            }),
        ));
    }
    rows.push(Row::item(
        Icon::Pencil,
        format!("Edit {what}"),
        c(ChannelUi::OpenEdit(ch.id)),
    ));
    if reorder(&app.chat, ch.id, -1).is_some() {
        rows.push(Row::item(
            Icon::ArrowUp,
            "Move Up",
            c(ChannelUi::Move(ch.id, -1)),
        ));
    }
    if reorder(&app.chat, ch.id, 1).is_some() {
        rows.push(Row::item(
            Icon::ArrowDown,
            "Move Down",
            c(ChannelUi::Move(ch.id, 1)),
        ));
    }
    if app.can_manage_permissions(ch.id) {
        rows.push(Row::item(
            Icon::ShieldCheck,
            "Edit Permissions",
            Ui::Perms(crate::app::channel_perms::PermsUi::Open(ch.id)),
        ));
    }
    rows.push(Row::danger(
        Icon::Trash,
        format!("Delete {what}"),
        c(ChannelUi::AskDelete(ch.id)),
    ));
    rows
}

/// Server actions at the top of the rail menu.
pub fn guild_rows(app: &App, g: GuildId) -> Vec<Row> {
    let mut rows = Vec::new();
    let admin = |a: AdminUi| Ui::Admin(a);
    if app.can_invite(g) {
        rows.push(Row::item(
            Icon::Users,
            "Invite People",
            admin(AdminUi::OpenInvite(g)),
        ));
    }
    if app.can_open_server_settings(g) {
        rows.push(Row::item(
            Icon::Settings,
            "Server Settings",
            admin(AdminUi::OpenSettings(g)),
        ));
    }
    if app.can_manage_channels(g) {
        rows.push(Row::item(
            Icon::Plus,
            "Create Channel",
            Ui::Channel(ChannelUi::OpenCreate {
                guild: g,
                parent: None,
                category: false,
            }),
        ));
        rows.push(Row::item(
            Icon::Folder,
            "Create Category",
            Ui::Channel(ChannelUi::OpenCreate {
                guild: g,
                parent: None,
                category: true,
            }),
        ));
    }
    if !rows.is_empty() {
        rows.push(Row::Separator);
    }
    let b = app.chat.guild_badge(g);
    if b.unread || b.mentions > 0 {
        rows.push(Row::item(Icon::Check, "Mark As Read", Ui::MarkGuildRead(g)));
    }
    if app.chat.muted_guilds.contains(&g) {
        rows.push(Row::item(
            Icon::Bell,
            "Unmute Server",
            Ui::MuteGuild(g, false),
        ));
    } else {
        rows.push(Row::item(
            Icon::BellOff,
            "Mute Server",
            Ui::MuteGuild(g, true),
        ));
    }
    let suppressed = app.chat.suppress_everyone.contains(&g);
    rows.push(Row::item(
        if suppressed {
            Icon::Check
        } else {
            Icon::AtSign
        },
        "Suppress @everyone",
        Ui::SuppressEveryone(g, !suppressed),
    ));
    if app.developer_mode() {
        rows.push(Row::Separator);
        rows.push(Row::item(
            Icon::Copy,
            "Copy Server ID",
            Ui::CopyToClipboard(g.to_string()),
        ));
    }
    // The owner cannot leave (only transfer or delete the server).
    if app.chat.guild(g).is_some() && !app.chat.owns_guild(g) {
        rows.push(Row::Separator);
        rows.push(Row::danger(
            Icon::LogOut,
            "Leave Server",
            admin(AdminUi::AskLeave(g)),
        ));
    }
    rows.push(Row::Separator);
    rows
}
