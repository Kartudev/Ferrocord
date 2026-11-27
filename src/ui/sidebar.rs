//! Second column: guild channel list (categories, unread/mention state) or
//! the DM list, plus the user panel at the bottom.

use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Length, Padding};

use super::emoji_font::emoji_text;
use super::icons::{Icon, icon};
use super::theme::{self, INTER, INTER_MEDIUM, INTER_SEMIBOLD};
use super::{El, avatar_with_presence, avatar_with_status, initials_circle, mention_badge, ui};
use crate::app::anim::{Hover, Zone};
use crate::app::context::{CtxMenu, CtxTarget};
use crate::app::friends::{FriendsUi, HomePage};
use crate::app::guild_admin::AdminUi;
use crate::app::{App, Message, Ui, View};
use crate::net::model::*;

pub fn view(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let (header, list): (El<'_>, El<'_>) = match app.view {
        View::Guild { guild, channel } => (
            guild_header(app, guild),
            guild_channels(app, guild, channel),
        ),
        View::Home(open) => (dm_header(app), dm_list(app, open)),
    };
    container(column![
        container(header)
            .height(t.metrics.header_height)
            .width(Length::Fill)
            .style(theme::sidebar_header(p)),
        super::smooth_scroll::smooth(
            scrollable(list)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(4).scroller_width(4),
                ))
                .style(theme::thin_scrollbar(p, false))
                .height(Length::Fill)
        ),
        super::voice::connected_panel(app).unwrap_or_else(super::empty),
        user_panel(app),
    ])
    .width(t.metrics.sidebar_width)
    .height(Length::Fill)
    .style(theme::bg(p.bg_sidebar))
    .into()
}

fn guild_header(app: &App, g: GuildId) -> El<'_> {
    let p = app.theme.palette;
    let name = app
        .chat
        .guild(g)
        .map(|g| g.name.clone())
        .unwrap_or_default();
    // The chevron turns into an X while the dropdown is open.
    let open = matches!(
        app.ctx_menu,
        Some(CtxMenu {
            target: CtxTarget::Guild(x),
            ..
        }) if x == g
    );
    button(
        row![
            container(emoji_text(name, 16.0, p.header_text, INTER_SEMIBOLD, false))
                .width(Length::Fill),
            icon(
                if open { Icon::X } else { Icon::ChevronDown },
                18.0,
                p.text_normal
            ),
        ]
        .height(Length::Fill)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([0, 16]))
    .width(Length::Fill)
    .height(Length::Fill)
    .on_press(ui(Ui::Admin(AdminUi::ToggleMenu(g))))
    .style(theme::list_row(p, open))
    .into()
}

fn channel_icon(kind: ChannelKind) -> Icon {
    match kind {
        ChannelKind::Voice | ChannelKind::Stage => Icon::Volume2,
        ChannelKind::Announcement => Icon::Megaphone,
        ChannelKind::Forum => Icon::MessageSquareText,
        _ => Icon::Hash,
    }
}

/// Unread pill on the left edge of the sidebar.
fn edge_pill<'a>(app: &App, show: bool) -> El<'a> {
    let p = app.theme.palette;
    container(if show {
        container(text(""))
            .width(4)
            .height(8)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(p.rail_pill)),
                border: iced::Border {
                    radius: iced::border::Radius::default().right(4.0),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    } else {
        El::from(Space::new().width(4))
    })
    .width(8)
    .height(32)
    .align_y(Alignment::Center)
    .into()
}

fn is_voice(k: ChannelKind) -> bool {
    matches!(k, ChannelKind::Voice | ChannelKind::Stage)
}

fn channel_row<'a>(app: &'a App, c: &'a Channel, selected: bool) -> El<'a> {
    let p = app.theme.palette;
    let joined = is_voice(c.kind) && app.voice.connected_channel() == Some(c.id);
    let unread =
        app.chat.is_unread(c.id) && !app.chat.is_channel_muted(c.id) && c.kind.is_textual();
    let mentions = app.chat.mention_count(c.id);
    let bright = selected || unread || mentions > 0;
    let color = if bright {
        p.header_text
    } else {
        p.channel_icon
    };
    let font = if unread && !selected {
        INTER_SEMIBOLD
    } else {
        INTER_MEDIUM
    };
    let mut r = row![
        icon(
            channel_icon(c.kind),
            20.0,
            if selected {
                p.text_secondary
            } else {
                p.channel_icon
            }
        ),
        container(emoji_text(
            c.name.clone().unwrap_or_default(),
            16.0,
            color,
            font,
            false
        ))
        .width(Length::Fill),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if mentions > 0 {
        r = r.push(mention_badge(&p, mentions, p.bg_sidebar));
    }
    let btn = button(r)
        .padding(Padding::from([6, 8]))
        .width(Length::Fill)
        .on_press(ui(if is_voice(c.kind) {
            Ui::Voice(crate::app::voice::VoiceUi::Join(c.id))
        } else {
            Ui::SelectChannel(c.id)
        }))
        .style(theme::list_row(p, selected || joined));
    row![
        edge_pill(app, unread && !selected),
        iced::widget::mouse_area(btn).on_right_press(ui(Ui::ContextOpen(
            crate::app::context::CtxTarget::Channel(c.id)
        ))),
    ]
    .padding(Padding::ZERO.right(8))
    .align_y(Alignment::Center)
    .into()
}

fn guild_channels(app: &App, g: GuildId, open: Option<ChannelId>) -> El<'_> {
    let p = app.theme.palette;
    let mut col = column![]
        .spacing(2)
        .padding(Padding::ZERO.top(12).bottom(8));
    for (cat, kids) in app.chat.channel_tree(g) {
        if let Some(cat) = cat {
            let collapsed = app.collapsed.contains(&cat.id);
            let header = button(
                row![
                    icon(
                        if collapsed {
                            Icon::ChevronRight
                        } else {
                            Icon::ChevronDown
                        },
                        12.0,
                        p.text_muted
                    ),
                    text(cat.name.clone().unwrap_or_default().to_uppercase())
                        .size(12)
                        .font(INTER_SEMIBOLD)
                        .color(p.text_muted),
                ]
                .spacing(2)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([0, 0]).top(16).bottom(4).left(10))
            .width(Length::Fill)
            .on_press(ui(Ui::ToggleCategory(cat.id)))
            .style(theme::link_button(p));
            let header = iced::widget::mouse_area(header).on_right_press(ui(Ui::ContextOpen(
                crate::app::context::CtxTarget::Channel(cat.id),
            )));
            // "+" (Create Channel in this category) for channel managers.
            let header: El<'_> = if app.can_manage_channels(g) {
                row![
                    header,
                    container(
                        button(icon(Icon::Plus, 14.0, p.text_muted))
                            .padding(2)
                            .on_press(ui(Ui::Channel(
                                crate::app::channel_admin::ChannelUi::OpenCreate {
                                    guild: g,
                                    parent: Some(cat.id),
                                    category: false,
                                }
                            )))
                            .style(theme::icon_button(p))
                    )
                    .padding(Padding::ZERO.top(14).right(8)),
                ]
                .align_y(Alignment::Center)
                .into()
            } else {
                header.into()
            };
            col = col.push(header);
            for c in kids {
                let selected = open == Some(c.id);
                // Collapsed categories still show unread and selected channels.
                if collapsed && !selected && !app.chat.is_unread(c.id) {
                    continue;
                }
                col = col.push(channel_row(app, c, selected));
                if let Some(m) = super::voice::channel_members(app, c.id) {
                    col = col.push(m);
                }
            }
        } else {
            for c in kids {
                col = col.push(channel_row(app, c, open == Some(c.id)));
                if let Some(m) = super::voice::channel_members(app, c.id) {
                    col = col.push(m);
                }
            }
        }
    }
    col.into()
}

fn dm_header(app: &App) -> El<'_> {
    let p = app.theme.palette;
    container(
        button(
            container(
                text("Find or start a conversation")
                    .size(14)
                    .font(INTER_MEDIUM)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
            )
            .center(Length::Fill),
        )
        .width(Length::Fill)
        .height(28)
        .padding(Padding::from([0, 6]))
        .on_press(Message::Ui(Ui::Friends(FriendsUi::OpenSwitcher)))
        .style(move |_, _| button::Style {
            background: Some(iced::Background::Color(p.bg_app)),
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }),
    )
    .padding(Padding::from([0, 10]))
    .height(Length::Fill)
    .align_y(Alignment::Center)
    .into()
}

/// "Friends" / "Message Requests" entry at the top of the DM list.
fn home_entry<'a>(
    app: &'a App,
    i: Icon,
    label: &'static str,
    selected: bool,
    badge: usize,
    msg: FriendsUi,
) -> El<'a> {
    let p = app.theme.palette;
    let color = if selected {
        p.header_text
    } else {
        p.text_secondary
    };
    let mut r = row![
        icon(i, 22.0, color),
        text(label)
            .size(16)
            .font(INTER_MEDIUM)
            .color(color)
            .wrapping(text::Wrapping::None)
            .width(Length::Fill),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    if badge > 0 {
        r = r.push(mention_badge(&p, badge as u32, p.bg_sidebar));
    }
    container(
        button(r)
            .padding(Padding::from([10, 12]))
            .width(Length::Fill)
            .on_press(Message::Ui(Ui::Friends(msg)))
            .style(theme::list_row(p, selected)),
    )
    .padding(Padding::from([0, 8]))
    .into()
}

fn dm_list(app: &App, open: Option<ChannelId>) -> El<'_> {
    let p = app.theme.palette;
    let page = if open.is_none() {
        app.home_page()
    } else {
        None
    };
    let mut col = column![
        home_entry(
            app,
            Icon::Users,
            "Friends",
            page == Some(HomePage::Friends),
            crate::app::friends::incoming_count(&app.chat),
            FriendsUi::OpenFriends,
        ),
        home_entry(
            app,
            Icon::Mail,
            "Message Requests",
            page == Some(HomePage::Requests),
            app.chat.message_requests(false).len(),
            FriendsUi::OpenRequests,
        ),
        row![
            text("DIRECT MESSAGES")
                .size(12)
                .font(INTER_SEMIBOLD)
                .color(p.text_muted)
                .width(Length::Fill),
            icon(Icon::Plus, 16.0, p.text_muted),
        ]
        .padding(Padding::from([0, 18]).top(18).bottom(4).left(18))
        .align_y(Alignment::Center),
    ]
    .spacing(2)
    .padding(Padding::ZERO.top(8));
    for c in app.chat.dm_list() {
        col = col.push(dm_row(app, c, open == Some(c.id)));
    }
    col.into()
}

fn dm_row<'a>(app: &'a App, c: &'a Channel, selected: bool) -> El<'a> {
    let p = app.theme.palette;
    let label = c.label(|u| app.chat.user_name(u));
    let unread = app.chat.mention_count(c.id) > 0;
    let mentions = app.chat.mention_count(c.id);
    let pic: El<'a> = match (
        c.kind,
        c.recipients.first().and_then(|u| app.chat.users.get(u)),
    ) {
        (ChannelKind::Dm, Some(u)) => avatar_with_presence(
            app,
            u,
            32.0,
            app.chat.presence_of(u.id),
            p.bg_sidebar,
            app.list_phase(Hover::row(u.id, Zone::DmList)),
        ),
        _ => initials_circle(&p, &label, theme::default_avatar_color(c.id.0), 32.0),
    };
    let mut names = column![
        text(label)
            .size(16)
            .font(if unread { INTER_SEMIBOLD } else { INTER_MEDIUM })
            .color(if selected || unread {
                p.header_text
            } else {
                p.channel_icon
            })
            .wrapping(text::Wrapping::None)
    ];
    if c.kind == ChannelKind::GroupDm {
        names = names.push(
            text(format!("{} Members", c.recipients.len() + 1))
                .size(12)
                .font(INTER)
                .color(p.text_muted),
        );
    }
    let mut r = row![pic, names.width(Length::Fill)]
        .spacing(12)
        .align_y(Alignment::Center);
    if mentions > 0 {
        r = r.push(mention_badge(&p, mentions, p.bg_sidebar));
    }
    let btn: El<'a> = button(r)
        .padding(Padding::from([6, 8]))
        .width(Length::Fill)
        .on_press(ui(Ui::SelectChannel(c.id)))
        .style(theme::list_row(p, selected))
        .into();
    let user = match (c.kind, c.recipients.first()) {
        (ChannelKind::Dm, Some(u)) => app.chat.users.get(u),
        _ => None,
    };
    let btn = match user {
        Some(u) => super::cosmetics::with_nameplate(app, u, btn),
        None => btn,
    };
    let hover = user.map(|u| Hover::row(u.id, Zone::DmList));
    let mut area = iced::widget::mouse_area(btn).on_right_press(ui(Ui::ContextOpen(
        crate::app::context::CtxTarget::Channel(c.id),
    )));
    if let Some(h) = hover {
        area = area
            .on_enter(Message::Ui(Ui::HoverEnter(h)))
            .on_exit(Message::Ui(Ui::HoverLeave(h)));
    }
    container(area).padding(Padding::from([0, 8])).into()
}

/// Width of every user-panel icon button.
const PANEL_BTN: f32 = 28.0;
const PANEL_GAP: f32 = 2.0;
const PANEL_PAD: f32 = 8.0;
/// Buttons in the panel: mic, deafen, settings.
const PANEL_BUTTONS: f32 = 3.0;

/// Width available to the name / status text next to the avatar.
pub fn panel_text_width(sidebar_width: f32) -> f32 {
    const AVATAR: f32 = 32.0;
    const IDENTITY_PAD: f32 = 4.0;
    const IDENTITY_GAP: f32 = 8.0;
    let buttons = PANEL_BUTTONS * PANEL_BTN + PANEL_BUTTONS * PANEL_GAP;
    (sidebar_width - 2.0 * PANEL_PAD - buttons - 2.0 * IDENTITY_PAD - AVATAR - IDENTITY_GAP)
        .max(0.0)
}

/// Bottom-left user panel like the official client: identity on the left
/// (name and status truncated with an ellipsis inside a fixed-width area,
/// so they can never run under the buttons), then fixed-size icon buttons.
/// Mic and deafen toggle self mute / deafen (remembered when not in voice,
/// like the official client); active states are drawn red.
fn user_panel(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let me = app.chat.me.clone().unwrap_or(User {
        id: Snowflake(0),
        username: String::new(),
        global_name: None,
        avatar: None,
        bot: false,
        ..Default::default()
    });
    let small = |i: Icon, on: Option<Ui>| {
        let enabled = on.is_some();
        let b = button(
            container(icon(
                i,
                18.0,
                if enabled {
                    p.text_secondary
                } else {
                    p.text_muted
                },
            ))
            .center(PANEL_BTN),
        )
        .width(PANEL_BTN)
        .height(PANEL_BTN)
        .padding(0)
        .style(theme::icon_button(p));
        match on {
            Some(u) => b.on_press(ui(u)),
            None => b,
        }
    };
    let toggle = |(i, tip, msg, active): (Icon, &'static str, Ui, bool)| -> El<'_> {
        let b = button(
            container(icon(
                i,
                18.0,
                if active { p.danger } else { p.text_secondary },
            ))
            .center(PANEL_BTN),
        )
        .width(PANEL_BTN)
        .height(PANEL_BTN)
        .padding(0)
        .style(theme::icon_button(p))
        .on_press(ui(msg));
        iced::widget::tooltip(
            b,
            container(text(tip).size(13).color(p.header_text))
                .padding(Padding::from([6, 10]))
                .style(theme::rounded_bg(p.bg_elevated, 5.0)),
            iced::widget::tooltip::Position::Top,
        )
        .gap(6)
        .into()
    };
    let [mic, deaf] = super::voice::panel_buttons(app);
    let avail = panel_text_width(t.metrics.sidebar_width);
    // Clicking the avatar or name opens the status menu, like the
    // official user panel. The avatar animates while the panel's identity
    // area is hovered, like every avatar outside a profile.
    let hover = Hover::row(me.id, Zone::Panel);
    let identity = button(
        row![
            avatar_with_status(app, &me, 32.0, app.list_phase(hover)),
            container(column![
                text(super::ellipsize(me.display_name(), avail, 14.0, true))
                    .size(14)
                    .font(INTER_SEMIBOLD)
                    .color(p.header_text)
                    .wrapping(text::Wrapping::None),
                text(super::ellipsize(
                    app.chat.my_status.label(),
                    avail,
                    12.0,
                    false
                ))
                .size(12)
                .color(p.text_secondary)
                .wrapping(text::Wrapping::None),
            ])
            .width(Length::Fill)
            .clip(true),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([0, 4]))
    .width(Length::Fill)
    .on_press(ui(Ui::ToggleStatusMenu))
    .style(theme::icon_button(p));
    let identity = iced::widget::mouse_area(identity)
        .on_enter(Message::Ui(Ui::HoverEnter(hover)))
        .on_exit(Message::Ui(Ui::HoverLeave(hover)));
    container(
        row![
            identity,
            toggle(mic),
            toggle(deaf),
            small(Icon::Settings, Some(Ui::OpenSettings)),
        ]
        .spacing(PANEL_GAP)
        .align_y(Alignment::Center),
    )
    .height(t.metrics.user_panel_height)
    .padding(Padding::from([0.0, PANEL_PAD]))
    .align_y(Alignment::Center)
    .style(theme::bg(p.user_panel))
    .into()
}
