//! Main pane: channel header, grouped message list with embeds and
//! attachments, composer and typing indicator.

use iced::widget::{
    Space, button, column, container, hover, mouse_area, progress_bar, rich_text, row, rule,
    scrollable, span, text, text_editor, text_input, tooltip,
};
use iced::{Alignment, Background, Border, Color, Font, Length, Padding};

use super::emoji_font::emoji_text;
use super::icons::{Icon, icon};
use super::theme::{self, INTER, INTER_BOLD, INTER_MEDIUM, INTER_SEMIBOLD};
use super::video;
use super::{
    El, avatar, cosmetics, fit_size, format_day, format_time, format_timestamp, human_size,
    markdown, same_day, ui,
};
use crate::app::actions::{composer_id, edit_id, search_id};
use crate::app::composer;
use crate::app::emoji::{self, PickerTarget};
use crate::app::lightbox::{Lightbox, proxy_resized};
use crate::app::panels::Popover;
use crate::app::state::continues_group;
use crate::app::{App, Message, Ui, View, emoji_url, jump, now_ms, preview_key};
use crate::media::clip::Clip;
use crate::net::model::{self as model, Attachment, Channel, ChannelKind};
use crate::ui::anim_image::message_anim;
use crate::ui::lightbox::zoomable;

pub fn view(app: &App) -> El<'_> {
    let p = app.theme.palette;
    let channel = app.open_channel().and_then(|c| app.chat.channels.get(&c));
    let body: El<'_> = match channel {
        Some(c) => column![
            header(app, c),
            super::voice::dm_call_area(app, c).unwrap_or_else(super::empty),
            messages(app, c),
            detached_bar(app, c),
            composer(app, c)
        ]
        .into(),
        None if matches!(app.view, View::Home(None)) => super::friends::view(app),
        None => empty(app),
    };
    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::bg(p.bg_panel))
        .into()
}

fn empty(app: &App) -> El<'_> {
    let p = app.theme.palette;
    let msg = match app.view {
        View::Home(_) => "Pick a conversation on the left to start chatting.",
        View::Guild { .. } => "This server has no text channels you can see.",
    };
    container(
        column![
            icon(Icon::Users, 64.0, p.text_muted),
            text(msg).size(16).color(p.text_muted)
        ]
        .spacing(16)
        .align_x(Alignment::Center),
    )
    .center(Length::Fill)
    .into()
}

fn channel_title(app: &App, c: &Channel) -> String {
    match c.kind {
        ChannelKind::Dm | ChannelKind::GroupDm => c.label(|u| app.chat.user_name(u)),
        _ => c.name.clone().unwrap_or_default(),
    }
}

fn header<'a>(app: &'a App, c: &'a Channel) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let lead = match c.kind {
        ChannelKind::Dm => Icon::AtSign,
        ChannelKind::GroupDm => Icon::Users,
        ChannelKind::Announcement => Icon::Megaphone,
        _ => Icon::Hash,
    };
    let mut r = row![
        icon(lead, 24.0, p.channel_icon),
        emoji_text(
            channel_title(app, c),
            16.0,
            p.header_text,
            INTER_SEMIBOLD,
            false
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if let Some(topic) = c.topic.as_deref().filter(|t| !t.is_empty()) {
        r = r
            .push(
                container(rule::vertical(1).style(move |_| rule::Style {
                    color: p.bg_selected,
                    radius: 0.0.into(),
                    fill_mode: rule::FillMode::Full,
                    snap: true,
                }))
                .height(24)
                .padding(Padding::from([0, 4])),
            )
            .push(
                text(topic.to_owned())
                    .size(14)
                    .color(p.text_secondary)
                    .wrapping(text::Wrapping::None),
            );
    }
    // Header buttons with a tooltip; `active` lights the icon while its
    // popover or panel is open.
    let tip = |content: El<'a>, label: &'static str| -> El<'a> {
        tooltip(
            content,
            container(
                text(label)
                    .size(13)
                    .font(INTER_SEMIBOLD)
                    .color(p.header_text),
            )
            .padding(Padding::from([6, 10]))
            .style(theme::rounded_bg(p.bg_elevated, 5.0)),
            tooltip::Position::Bottom,
        )
        .gap(8)
        .into()
    };
    let tool = |i: Icon, label: &'static str, msg: Ui, active: bool| -> El<'a> {
        tip(
            button(icon(
                i,
                24.0,
                if active {
                    p.header_text
                } else {
                    p.text_secondary
                },
            ))
            .padding(0)
            .on_press(ui(msg))
            .style(theme::link_button(p))
            .into(),
            label,
        )
    };
    // Guilds show the member list; DMs show the participants (or, in a 1:1
    // DM, the other person's profile).
    let in_guild = matches!(app.view, View::Guild { .. });
    let members: El<'a> = tool(
        Icon::Users,
        match (in_guild, app.members_open) {
            (true, true) => "Hide Member List",
            (true, false) => "Show Member List",
            (false, true) => "Hide User Profile",
            (false, false) => "Show User Profile",
        },
        Ui::ToggleMembers,
        app.members_open,
    );
    let open = app.panels.open;
    let muted = app.chat.is_channel_muted(c.id);
    let searching = app.search.open || app.search.panel_visible();
    let search_input = text_input("Search", &app.search.input)
        .id(search_id())
        .on_input(|s| Message::Ui(Ui::SearchChanged(s)))
        .on_submit(Message::Ui(Ui::SubmitSearch))
        .size(14)
        .font(INTER)
        .padding(Padding::from([2, 6]))
        .style(theme::input(p, p.bg_app));
    let search_end: El<'a> = if searching {
        button(icon(Icon::X, 16.0, p.text_muted))
            .padding(0)
            .on_press(ui(Ui::CloseSearch))
            .style(theme::link_button(p))
            .into()
    } else {
        icon(Icon::Search, 16.0, p.text_muted)
    };
    let search = container(
        row![search_input, search_end]
            .spacing(4)
            .align_y(Alignment::Center),
    )
    .width(if searching { 240 } else { 144 })
    .height(28)
    .padding(Padding::from([0, 6]))
    .align_y(Alignment::Center)
    .style(theme::rounded_bg(p.bg_app, 4.0));
    let tools = row![
        tool(
            if muted { Icon::BellOff } else { Icon::Bell },
            "Notification Settings",
            Ui::TogglePopover(Popover::Bell),
            open == Some(Popover::Bell),
        ),
        tool(
            Icon::Pin,
            "Pinned Messages",
            Ui::TogglePopover(Popover::Pins),
            open == Some(Popover::Pins),
        ),
        members,
        search,
        tool(
            Icon::Inbox,
            "Inbox",
            Ui::TogglePopover(Popover::Inbox),
            open == Some(Popover::Inbox),
        ),
    ]
    .spacing(16)
    .align_y(Alignment::Center);
    // DMs and groups get the "Start Voice Call" phone first, like the
    // official header.
    let tools: El<'a> = if c.kind.is_private() {
        let in_call = app.voice.connected_channel() == Some(c.id);
        row![
            tool(
                Icon::Phone,
                if in_call {
                    "In Call"
                } else {
                    "Start Voice Call"
                },
                Ui::Voice(crate::app::voice::VoiceUi::StartCall(c.id)),
                in_call,
            ),
            tools
        ]
        .spacing(16)
        .align_y(Alignment::Center)
        .into()
    } else {
        tools.into()
    };
    container(
        row![container(r).width(Length::Fill).clip(true), tools]
            .spacing(16)
            .align_y(Alignment::Center),
    )
    .height(t.metrics.header_height)
    .width(Length::Fill)
    .padding(Padding::from([0, 16]))
    .align_y(Alignment::Center)
    .style(theme::header(p))
    .into()
}

fn welcome<'a>(app: &'a App, c: &'a Channel) -> El<'a> {
    let p = app.theme.palette;
    let (title, sub) = match c.kind {
        ChannelKind::Dm | ChannelKind::GroupDm => (
            channel_title(app, c),
            format!(
                "This is the beginning of your direct message history with {}.",
                channel_title(app, c)
            ),
        ),
        _ => (
            format!("Welcome to #{}!", c.name.as_deref().unwrap_or("")),
            format!(
                "This is the start of the #{} channel.",
                c.name.as_deref().unwrap_or("")
            ),
        ),
    };
    let badge: El<'a> = match c.kind {
        ChannelKind::Dm => match c.recipients.first().and_then(|u| app.chat.users.get(u)) {
            Some(u) => avatar(app, u, 80.0),
            None => Space::new().into(),
        },
        _ => container(icon(Icon::Hash, 42.0, p.header_text))
            .center(68)
            .style(theme::rounded_bg(p.bg_selected, 34.0))
            .into(),
    };
    column![
        badge,
        text(title).size(32).font(INTER_BOLD).color(p.header_text),
        text(sub).size(16).color(p.text_secondary),
    ]
    .spacing(8)
    .padding(Padding::from([16, 16]).bottom(8))
    .into()
}

fn day_divider<'a>(app: &App, ms: u64) -> El<'a> {
    let p = app.theme.palette;
    let line = move || {
        rule::horizontal(1).style(move |_| rule::Style {
            color: p.bg_selected,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
    };
    row![
        line(),
        text(format_day(ms))
            .size(12)
            .font(INTER_SEMIBOLD)
            .color(p.text_muted),
        line()
    ]
    .spacing(8)
    .padding(Padding::from([8, 16]))
    .align_y(Alignment::Center)
    .into()
}

fn messages<'a>(app: &'a App, c: &'a Channel) -> El<'a> {
    let p = app.theme.palette;
    let now = now_ms();
    let window = app.chat.messages.get(&c.id);
    let mut col = column![].width(Length::Fill);
    match window {
        Some(w) if w.reached_start => col = col.push(welcome(app, c)),
        Some(w) if w.loading => {
            col = col.push(
                container(text("Loading messages…").size(14).color(p.text_muted))
                    .padding(16)
                    .center_x(Length::Fill),
            );
        }
        _ => col = col.push(welcome(app, c)),
    }
    if let Some(w) = window {
        let mut prev: Option<&model::Message> = None;
        for m in &w.messages {
            let new_day = prev.is_none_or(|pm| !same_day(pm.timestamp_ms(), m.timestamp_ms()));
            if new_day && prev.is_some() {
                col = col.push(day_divider(app, m.timestamp_ms()));
            }
            let grouped = !new_day && prev.is_some_and(|pm| continues_group(pm, m));
            if m.kind == 3 {
                // Call service message.
                col = col.push(super::voice::call_message(app, m, now));
            } else {
                col = col.push(message(app, c, m, grouped, now));
            }
            prev = Some(m);
        }
    }
    col = col.push(Space::new().height(24));
    // `direction` resets the anchor, so it must come first.
    super::smooth_scroll::smooth(
        scrollable(col)
            .id(jump::list_id())
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new()
                    .width(8)
                    .scroller_width(8)
                    .margin(2),
            ))
            .anchor_bottom()
            .on_scroll(|vp| Message::Ui(Ui::Scrolled(vp)))
            .style(theme::thin_scrollbar(p, true))
            .height(Length::Fill),
    )
    .into()
}

fn message<'a>(
    app: &'a App,
    c: &'a Channel,
    m: &'a model::Message,
    grouped: bool,
    now: u64,
) -> El<'a> {
    let p = app.theme.palette;
    let mentioned = app.chat.mentions_me(m) && !c.kind.is_private();
    let base = if m.failed {
        p.danger
    } else if m.pending {
        p.text_muted
    } else {
        p.text_normal
    };
    let editing = app.editing.as_ref().filter(|e| e.message == m.id);
    let mut body = column![].spacing(4).width(Length::Fill);
    if let Some(e) = editing {
        body = body.push(edit_box(app, e));
    } else if !m.content.is_empty() && (!m.content_is_gif_link() || !app.show_embeds()) {
        let content = markdown::render(app, m.id, &m.content, base, app.chat_font());
        if m.edited {
            body = body.push(column![
                content,
                text("(edited)").size(10).color(p.text_muted)
            ]);
        } else {
            body = body.push(content);
        }
    }
    for a in &m.attachments {
        body = body.push(attachment(app, a));
    }
    for s in &m.stickers {
        body = body.push(message_sticker(app, s, STICKER_SIZE, Some(m.id)));
    }
    // "Show embeds and preview website links".
    if app.show_embeds() {
        for card in super::embeds::message_embeds(app, m) {
            body = body.push(card);
        }
    }
    if let Some(d) = super::plugins::decorations(app, m.id) {
        body = body.push(d);
    }
    // "Show emoji reactions on messages".
    if app.show_reactions() && !m.reactions.is_empty() {
        body = body.push(reactions_row(app, c, m));
    }
    if m.failed {
        body = body.push(
            text("Message could not be delivered.")
                .size(12)
                .color(p.danger),
        );
    }

    let gutter = 72.0;
    let preview = if m.referenced.is_some() {
        app.chat.reply_preview(m)
    } else {
        None
    };
    let line: El<'a> = if app.compact() {
        compact_line(app, c, m, grouped, body, preview.as_ref())
    } else if grouped {
        row![
            container(
                text(format_time(m.timestamp_ms()))
                    .size(11)
                    .color(Color::TRANSPARENT)
            )
            .width(gutter)
            .padding(Padding::ZERO.left(16).top(4)),
            body
        ]
        .padding(Padding::from([2, 0]).right(48))
        .into()
    } else {
        let name_color = app
            .chat
            .author_color(m.guild_id.or(c.guild_id), m)
            .map(theme::from_int)
            .unwrap_or(p.header_text);
        let profile_guild = m.guild_id.or(c.guild_id);
        let mut name_row = row![cosmetics::clickable(
            cosmetics::name(
                app,
                &m.author,
                m.author.display_name(),
                app.chat_font(),
                INTER_MEDIUM,
                name_color
            ),
            &m.author,
            profile_guild
        )]
        .spacing(8)
        .align_y(Alignment::Center);
        if let Some(tag) = cosmetics::tag_chip(app, &m.author) {
            name_row = name_row.push(tag);
        }
        if m.author.bot {
            name_row = name_row.push(
                container(
                    text("BOT")
                        .size(10)
                        .font(INTER_SEMIBOLD)
                        .color(p.accent_text),
                )
                .padding(Padding::from([1, 4]))
                .style(theme::rounded_bg(p.accent, 3.0)),
            );
        }
        name_row = name_row.push(
            text(format_timestamp(m.timestamp_ms(), now))
                .size(12)
                .color(p.text_muted),
        );
        let main = row![
            container(cosmetics::clickable(
                super::hover_avatar(
                    app,
                    &m.author,
                    40.0,
                    crate::app::anim::Hover::message(m.author.id, m.id)
                ),
                &m.author,
                profile_guild
            ))
            .width(gutter)
            .padding(Padding::ZERO.left(16).top(2)),
            column![name_row, body].spacing(2).width(Length::Fill)
        ]
        .padding(Padding::from([2, 0]).right(48));
        match preview {
            Some(pv) => column![
                container(reply_header(app, c, m, &pv)).padding(Padding::ZERO.top(16)),
                main
            ]
            .into(),
            None => container(main).padding(Padding::ZERO.top(16)).into(),
        }
    };

    let jump_target = app.highlight == Some(m.id);
    let menu_target = app.menu.is_some_and(|mn| mn.message == m.id);
    let tint = if jump_target {
        Some(Color { a: 0.12, ..p.link })
    } else if mentioned {
        Some(p.bubble_in)
    } else if menu_target {
        Some(p.bg_hover)
    } else if app.plugins.decorations.highlighted(m.id.0) {
        Some(theme::plugin_highlight(p))
    } else {
        None
    };
    let bar: El<'a> = if mentioned {
        row![
            container(text(""))
                .width(2)
                .height(Length::Fill)
                .style(theme::bg(p.mention_bar)),
            container(line)
                .width(Length::Fill)
                .padding(Padding::ZERO.bottom(2)),
        ]
        .height(Length::Shrink)
        .into()
    } else {
        line
    };
    let base_el: El<'a> = match tint {
        Some(t) => container(bar)
            .width(Length::Fill)
            .style(theme::bg(t))
            .into(),
        None => container(bar).width(Length::Fill).into(),
    };
    let interactive = !m.pending && !m.failed && matches!(m.kind, 0 | 19) && editing.is_none();
    let el: El<'a> = if interactive {
        let overlay = container(action_bar(app, c, m))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::ZERO.right(16))
            .align_right(Length::Fill)
            .align_top(Length::Fill)
            .style(theme::bg(Color {
                a: 0.04,
                ..p.text_primary
            }));
        let h = crate::app::anim::Hover::message(m.author.id, m.id);
        mouse_area(hover(base_el, overlay))
            .on_right_press(ui(Ui::ContextMenu(m.id)))
            .on_enter(Message::Ui(Ui::HoverEnter(h)))
            .on_exit(Message::Ui(Ui::HoverLeave(h)))
            .into()
    } else {
        base_el
    };
    if jump_target {
        container(el).id(jump::row_id(m.id)).into()
    } else {
        el
    }
}

/// "╭─ (avatar) @name snippet": click jumps to the original message.
fn reply_header<'a>(
    app: &'a App,
    c: &'a Channel,
    m: &'a model::Message,
    pv: &model::ReplyPreview,
) -> El<'a> {
    let p = app.theme.palette;
    let spine = container(icon(Icon::ReplySpine, 24.0, p.text_muted))
        .width(Length::Shrink)
        .padding(Padding::ZERO.left(36).top(6));
    let mut r = row![spine].spacing(4).align_y(Alignment::Center);
    let target = pv.message_id.or(m.reply_to);
    let content: El<'a> = if pv.deleted {
        text("Original message was deleted")
            .size(14)
            .font(theme::INTER_ITALIC)
            .color(p.text_muted)
            .into()
    } else {
        let mut inner = row![].spacing(4).align_y(Alignment::Center);
        if let Some(a) = &pv.author {
            let color = app
                .chat
                .author_color(
                    m.guild_id.or(c.guild_id),
                    &model::Message {
                        author: a.clone(),
                        author_roles: vec![],
                        ..m.clone()
                    },
                )
                .map(theme::from_int)
                .unwrap_or(p.text_secondary);
            inner = inner.push(avatar(app, a, 16.0)).push(emoji_text(
                format!("@{}", a.display_name()),
                14.0,
                color,
                INTER_MEDIUM,
                false,
            ));
        }
        let snippet = pv.content.replace('\n', " ");
        let snippet = if snippet.is_empty() && pv.has_attachment {
            "Click to see attachment".to_owned()
        } else {
            snippet
        };
        inner = inner.push(
            text(snippet)
                .size(14)
                .color(p.text_secondary)
                .wrapping(text::Wrapping::None),
        );
        if pv.has_attachment {
            inner = inner.push(icon(Icon::Image, 16.0, p.text_secondary));
        }
        inner.into()
    };
    let pressable = container(content).max_width(640).clip(true);
    r = r.push(match target {
        Some(t) if !pv.deleted => button(pressable)
            .padding(0)
            .on_press(ui(Ui::JumpTo {
                channel: m.channel_id,
                message: t,
            }))
            .style(theme::link_button(p))
            .into(),
        _ => El::from(pressable),
    });
    r.into()
}

fn reactions_row<'a>(app: &'a App, c: &'a Channel, m: &'a model::Message) -> El<'a> {
    let pills: Vec<El<'a>> = m
        .reactions
        .iter()
        .map(|re| super::reactions::pill(app, c.id, m, re))
        .collect();
    row(pills)
        .spacing(4)
        .align_y(Alignment::Center)
        .wrap()
        .vertical_spacing(4)
        .into()
}

/// Floating toolbar shown while hovering a message.
fn action_bar<'a>(app: &'a App, c: &'a Channel, m: &'a model::Message) -> El<'a> {
    let p = app.theme.palette;
    let mine = Some(m.author.id) == app.chat.me_id();
    let tool = |i: Icon, msg: Ui| -> El<'a> {
        button(container(icon(i, 18.0, p.text_secondary)).center(28))
            .padding(0)
            .on_press(ui(msg))
            .style(theme::icon_button(p))
            .into()
    };
    // One-click reactions with the most recently used emoji, like the
    // official client's hover bar.
    let access = app.nitro_access();
    let quick = emoji::quick_reactions(&app.recent_emoji, &app.chat.guilds, &access);
    let has_quick = !quick.is_empty();
    let mut r = row![].spacing(0).align_y(Alignment::Center);
    for item in quick {
        let (face, e): (El<'a>, model::Emoji) = match item {
            emoji::RecentItem::Unicode(s) => (
                emoji_text(s, 18.0, p.text_normal, INTER, false),
                model::Emoji::unicode(s),
            ),
            emoji::RecentItem::Custom(_, ce, _) => (
                match app.image(&emoji_url(&app.cfg.cdn_url, ce.id)) {
                    Some(h) => iced::widget::image(h.clone()).width(20).height(20).into(),
                    None => text(":").size(14).color(p.text_muted).into(),
                },
                model::Emoji::custom(ce),
            ),
        };
        r = r.push(
            button(container(face).center(28))
                .padding(0)
                .on_press(ui(Ui::ToggleReaction(m.id, e)))
                .style(theme::icon_button(p)),
        );
    }
    if has_quick {
        r = r.push(
            container(container(Space::new().width(1).height(20)).style(theme::bg(p.border)))
                .padding(Padding::from([0, 4])),
        );
    }
    r = r.push(tool(
        Icon::SmilePlus,
        Ui::OpenEmojiPicker(PickerTarget::Reaction {
            channel: c.id,
            message: m.id,
        }),
    ));
    if mine && matches!(m.kind, 0 | 19) {
        r = r.push(tool(Icon::Pencil, Ui::StartEdit(m.id)));
    }
    r = r
        .push(tool(Icon::Reply, Ui::StartReply(m.id)))
        .push(tool(Icon::Ellipsis, Ui::ContextMenu(m.id)));
    container(r)
        .padding(2)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.bg_panel)),
            border: Border {
                radius: 6.0.into(),
                width: 1.0,
                color: p.border,
            },
            ..Default::default()
        })
        .into()
}

/// Maps a composer binding to the app message.
fn bind(b: text_editor::Binding<Ui>) -> text_editor::Binding<Message> {
    use text_editor::Binding as B;
    match b {
        B::Custom(u) => B::Custom(Message::Ui(u)),
        B::Sequence(v) => B::Sequence(v.into_iter().map(bind).collect()),
        B::Unfocus => B::Unfocus,
        B::Copy => B::Copy,
        B::Cut => B::Cut,
        B::Paste => B::Paste,
        B::Move(m) => B::Move(m),
        B::Select(m) => B::Select(m),
        B::SelectWord => B::SelectWord,
        B::SelectLine => B::SelectLine,
        B::SelectAll => B::SelectAll,
        B::Insert(c) => B::Insert(c),
        B::Enter => B::Enter,
        B::Backspace => B::Backspace,
        B::Delete => B::Delete,
    }
}

/// Inline editor shown in place of the message text.
fn edit_box<'a>(app: &'a App, e: &'a crate::app::actions::Editing) -> El<'a> {
    let p = app.theme.palette;
    let input = text_editor(e.text.content())
        .id(edit_id())
        .on_action(|a| Message::Ui(Ui::EditAction(a)))
        .key_binding(|kp| composer::key_binding(kp, false, Ui::SubmitEdit, None).map(bind))
        .size(16)
        .font(INTER)
        .line_height(iced::widget::text::LineHeight::Absolute(
            composer::LINE_HEIGHT.into(),
        ))
        .padding(Padding::from([composer::PADDING_Y, 12.0]))
        .max_height(composer::text_height_limits(crate::app::input::window_size().height).1)
        .style(theme::editor(p, p.composer_bg));
    let link = |label: &'static str, msg: Ui| -> El<'a> {
        button(text(label).size(12).color(p.link))
            .padding(0)
            .on_press(ui(msg))
            .style(theme::link_button(p))
            .into()
    };
    column![
        input,
        row![
            text("escape to ").size(12).color(p.text_muted),
            link("cancel", Ui::CancelEdit),
            text(" \u{2022} enter to ").size(12).color(p.text_muted),
            link("save", Ui::SubmitEdit),
        ]
    ]
    .spacing(4)
    .into()
}

fn detached_bar<'a>(app: &'a App, c: &Channel) -> El<'a> {
    let p = app.theme.palette;
    let detached = app.chat.messages.get(&c.id).is_some_and(|w| w.detached);
    if !detached {
        return Space::new().into();
    }
    container(
        row![
            text("You're viewing older messages")
                .size(14)
                .font(INTER_MEDIUM)
                .color(p.accent_text)
                .width(Length::Fill),
            button(
                row![
                    text("Jump To Present")
                        .size(14)
                        .font(INTER_SEMIBOLD)
                        .color(p.accent_text),
                    icon(Icon::ArrowDown, 16.0, p.accent_text),
                ]
                .spacing(4)
                .align_y(Alignment::Center)
            )
            .padding(0)
            .on_press(ui(Ui::JumpToPresent))
            .style(theme::link_button(p)),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([6, 16]))
    .width(Length::Fill)
    .style(theme::bg(p.accent))
    .into()
}

/// An image (attachment, GIF or image link) fitted into 400x300, with a
/// placeholder until it has loaded.
fn image_box<'a>(
    app: &'a App,
    key: &str,
    width: Option<u32>,
    height: Option<u32>,
    zoom: Lightbox,
) -> El<'a> {
    let p = app.theme.palette;
    let (w, h) = fit_size(width.unwrap_or(400), height.unwrap_or(300), 400, 300);
    if let Some(a) = app.media_anim(key) {
        return zoomable(
            message_anim(app, a, w as f32, h as f32).border_radius(8.0),
            zoom,
        );
    }
    match app.image(key) {
        Some(handle) => zoomable(
            iced::widget::image(handle.clone())
                .width(w as f32)
                .height(h as f32)
                .border_radius(8.0),
            zoom,
        ),
        None => container(icon(Icon::Image, 32.0, p.text_muted))
            .width(w as f32)
            .height(h as f32)
            .center_x(w as f32)
            .center_y(h as f32)
            .style(theme::rounded_bg(p.embed_bg, 8.0))
            .into(),
    }
}

/// Side of a sticker in a message (the official client's size).
const STICKER_SIZE: f32 = 160.0;

/// A sticker image, or a box with its name while loading and for Lottie
/// stickers (vector animations, not rendered).
pub(super) fn sticker<'a>(app: &'a App, s: &model::Sticker, size: f32) -> El<'a> {
    message_sticker(app, s, size, None)
}

/// A sticker of message `msg` (`None`: outside a message): it animates
/// as "Animate stickers" says (always, while hovered, never).
fn message_sticker<'a>(
    app: &'a App,
    s: &model::Sticker,
    size: f32,
    msg: Option<model::MessageId>,
) -> El<'a> {
    let p = app.theme.palette;
    if app.sticker_animates(msg)
        && let Some(a) = s
            .image_url(&app.cfg.cdn_url)
            .and_then(|url| app.media_anim(&url))
    {
        return message_anim(app, a, size, size)
            .content_fit(iced::ContentFit::Contain)
            .into();
    }
    if let Some(handle) = s
        .image_url(&app.cfg.cdn_url)
        .and_then(|url| app.image(&url))
    {
        return iced::widget::image(handle.clone())
            .width(size)
            .height(size)
            .content_fit(iced::ContentFit::Contain)
            .into();
    }
    container(
        column![
            icon(Icon::Sticker, size / 4.0, p.text_muted),
            text(s.name.clone())
                .size(12)
                .color(p.text_muted)
                .align_x(Alignment::Center),
        ]
        .spacing(4)
        .align_x(Alignment::Center),
    )
    .width(size)
    .height(size)
    .padding(8)
    .center_x(size)
    .center_y(size)
    .style(theme::rounded_bg(p.embed_bg, 8.0))
    .into()
}

/// A message in the "Compact" message display: time, name and text on one
/// line, no avatar; continuation messages show only their (faint) time.
fn compact_line<'a>(
    app: &'a App,
    c: &'a Channel,
    m: &'a model::Message,
    grouped: bool,
    body: iced::widget::Column<'a, Message>,
    preview: Option<&model::ReplyPreview>,
) -> El<'a> {
    let p = app.theme.palette;
    let t = &app.theme.metrics;
    let time = text(format_time(m.timestamp_ms()))
        .size(t.font_xs - 1.0)
        .color(if grouped {
            Color {
                a: 0.0,
                ..p.text_muted
            }
        } else {
            p.text_muted
        });
    let time = container(time)
        .width(COMPACT_TIME_WIDTH)
        .padding(Padding::ZERO.left(16).top(3));
    let row: El<'a> = if grouped {
        row![time, body].into()
    } else {
        let name_color = app
            .chat
            .author_color(m.guild_id.or(c.guild_id), m)
            .map(theme::from_int)
            .unwrap_or(p.header_text);
        let name = cosmetics::clickable(
            cosmetics::name(
                app,
                &m.author,
                m.author.display_name(),
                app.chat_font(),
                INTER_MEDIUM,
                name_color,
            ),
            &m.author,
            m.guild_id.or(c.guild_id),
        );
        row![time, container(name).padding(Padding::ZERO.right(6)), body].into()
    };
    let row = container(row).padding(Padding::from([1, 0]).right(48));
    match preview {
        Some(pv) if !grouped => column![
            container(reply_header(app, c, m, pv)).padding(Padding::ZERO.top(6)),
            row
        ]
        .into(),
        _ => container(row)
            .padding(Padding::ZERO.top(if grouped { 0 } else { 4 }))
            .into(),
    }
}

/// Time column of compact message rows.
const COMPACT_TIME_WIDTH: f32 = 72.0;

fn attachment<'a>(app: &'a App, a: &'a Attachment) -> El<'a> {
    // "Display images, videos and lolcats: when uploaded directly" off:
    // every attachment is a file card (a click opens it in the browser).
    if !app.inline_attachments() {
        return file_card(app, a);
    }
    if a.is_image() {
        let key = preview_key(a);
        let (nw, nh) = (a.width.unwrap_or(400), a.height.unwrap_or(300));
        let base = a.proxy_url.as_deref().unwrap_or(&a.url);
        // GIFs keep their chat-size animation; still images get a sharper copy.
        let full = (app.media_anim(&key).is_none() && !base.starts_with("mock://"))
            .then(|| proxy_resized(base, nw, nh, 1920, 1080));
        let zoom = Lightbox {
            key: key.clone(),
            full,
            width: nw,
            height: nh,
            url: Some(a.url.clone()),
        };
        return image_box(app, &key, a.width, a.height, zoom);
    }
    if let Some(clip) = Clip::from_attachment(a) {
        return video::inline(app, &clip);
    }
    file_card(app, a)
}

fn file_card<'a>(app: &'a App, a: &'a Attachment) -> El<'a> {
    let p = app.theme.palette;
    let link_spans = vec![
        span::<Ui, Font>(a.filename.clone())
            .color(p.link)
            .font(INTER)
            .link(Ui::OpenUrl(a.url.clone())),
    ];
    container(
        row![
            icon(Icon::File, 30.0, p.accent),
            column![
                rich_text(link_spans).size(16).on_link_click(Message::Ui),
                text(human_size(a.size)).size(12).color(p.text_muted),
            ]
            .spacing(2)
            .width(Length::Fill),
            button(icon(Icon::Download, 24.0, p.text_secondary))
                .padding(0)
                .on_press(ui(Ui::OpenUrl(a.url.clone())))
                .style(theme::link_button(p)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(432)
    .padding(16)
    .style(move |_| container::Style {
        background: Some(Background::Color(p.embed_bg)),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: p.border,
        },
        ..Default::default()
    })
    .into()
}

fn composer<'a>(app: &'a App, c: &'a Channel) -> El<'a> {
    let mut col = column![].padding(Padding::from([0, 16]));
    if let Some(r) = reply_bar(app, c) {
        col = col.push(r);
    }
    if !app.draft.is_empty() {
        col = col.push(draft_strip(app));
    }
    for u in app.uploads.iter().filter(|u| u.channel == c.id) {
        col = col.push(upload_strip(app, u));
    }
    col = col.push(composer_bar(app, c));
    column![col, typing(app, c)].into()
}

/// The message box row: attach button, text, GIF / sticker / emoji buttons.
fn composer_bar<'a>(app: &'a App, c: &'a Channel) -> El<'a> {
    let p = app.theme.palette;
    let placeholder = match c.kind {
        ChannelKind::Dm | ChannelKind::GroupDm => format!("Message @{}", channel_title(app, c)),
        _ => format!("Message #{}", c.name.as_deref().unwrap_or("")),
    };
    let tool = |i: Icon, msg: Option<Ui>| -> El<'a> {
        match msg {
            Some(m) => button(icon(i, 24.0, p.text_secondary))
                .padding(0)
                .on_press(ui(m))
                .style(theme::link_button(p))
                .into(),
            None => icon(i, 24.0, p.text_secondary),
        }
    };
    // Enter sends, Shift+Enter breaks the line; the box grows with its text
    // up to half of the window and then scrolls.
    let empty = app.composer.is_empty();
    let (min_h, max_h) = composer::text_height_limits(crate::app::input::window_size().height);
    let input = text_editor(app.composer.content())
        .id(composer_id())
        .placeholder(placeholder)
        .on_action(|a| Message::Ui(Ui::ComposerAction(a)))
        .key_binding(move |kp| {
            composer::key_binding(kp, empty, Ui::Send, Some(Ui::EditLast)).map(bind)
        })
        .size(16)
        .line_height(iced::widget::text::LineHeight::Absolute(
            composer::LINE_HEIGHT.into(),
        ))
        .font(INTER)
        .padding(Padding::from([composer::PADDING_Y, 0.0]))
        .min_height(min_h)
        .max_height(max_h)
        .style(theme::editor(p, p.composer_bg));
    // Tools are centred on the first line and stay level with the last
    // line of a tall box.
    let tools_end = |e: El<'a>| -> El<'a> {
        container(e)
            .padding(
                Padding::ZERO.bottom(composer::PADDING_Y + (composer::LINE_HEIGHT - 24.0) / 2.0),
            )
            .into()
    };
    container(
        row![
            tools_end(tool(Icon::CirclePlus, Some(Ui::PickFiles))),
            input,
            tools_end(tool(Icon::Gif, Some(Ui::OpenGifPicker))),
            tools_end(tool(Icon::Sticker, Some(Ui::OpenStickerPicker))),
            tools_end(tool(
                Icon::Smile,
                Some(Ui::OpenEmojiPicker(PickerTarget::Composer))
            ))
        ]
        .spacing(16)
        .align_y(Alignment::End),
    )
    .padding(Padding::from([0, 16]))
    .style(theme::rounded_bg(p.composer_bg, 8.0))
    .into()
}

/// "Replying to Name   @ ON  x" bar attached to the top of the composer.
fn reply_bar<'a>(app: &'a App, c: &Channel) -> Option<El<'a>> {
    let p = app.theme.palette;
    let r = app.reply.as_ref().filter(|r| r.channel == c.id)?;
    let mine = Some(r.author.id) == app.chat.me_id();
    let mut row_ = row![
        text("Replying to ").size(14).color(p.text_secondary),
        container(emoji_text(
            r.author.display_name(),
            14.0,
            p.text_primary,
            INTER_SEMIBOLD,
            false
        ))
        .width(Length::Fill),
    ]
    .align_y(Alignment::Center)
    .spacing(0);
    if !mine {
        // "@ ON" / "@ OFF": whether the reply pings the author.
        let label = if r.mention { "ON" } else { "OFF" };
        let toggle = button(
            row![
                text("@").size(14).font(INTER_BOLD).color(if r.mention {
                    p.accent
                } else {
                    p.text_muted
                }),
                text(label).size(12).font(INTER_BOLD).color(if r.mention {
                    p.accent
                } else {
                    p.text_muted
                }),
            ]
            .spacing(3)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([2, 6]))
        .on_press(ui(Ui::ToggleReplyMention))
        .style(theme::icon_button(p));
        row_ = row_.push(toggle);
    }
    row_ = row_.push(
        button(icon(Icon::X, 18.0, p.text_secondary))
            .padding(Padding::from([0, 4]))
            .on_press(ui(Ui::CancelReply))
            .style(theme::icon_button(p)),
    );
    Some(
        container(row_)
            .padding(Padding::from([8, 12]))
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(p.bg_sidebar)),
                border: Border {
                    radius: iced::border::Radius::default().top(8.0),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into(),
    )
}

/// Files waiting to be sent: thumbnail or file icon, name, size, remove.
fn draft_strip<'a>(app: &'a App) -> El<'a> {
    let p = app.theme.palette;
    let mut r = row![].spacing(12);
    for (i, a) in app.draft.iter().enumerate() {
        let thumb: El<'a> = match app.image(&a.key()) {
            Some(h) if a.is_image => iced::widget::image(h.clone())
                .width(96)
                .height(96)
                .border_radius(4.0)
                .into(),
            _ => container(icon(
                if a.is_image { Icon::Image } else { Icon::File },
                40.0,
                p.accent,
            ))
            .center(96)
            .into(),
        };
        let remove = button(icon(Icon::Trash, 16.0, p.danger))
            .padding(4)
            .on_press(ui(Ui::RemoveAttachment(i)))
            .style(theme::icon_button(p));
        r = r.push(
            container(
                column![
                    container(thumb).center_x(Length::Fill),
                    row![
                        text(a.file.name.clone())
                            .size(12)
                            .color(p.text_normal)
                            .wrapping(text::Wrapping::None)
                            .width(Length::Fill),
                        remove,
                    ]
                    .align_y(Alignment::Center),
                    text(human_size(a.file.size)).size(11).color(p.text_muted),
                ]
                .spacing(2),
            )
            .width(128)
            .padding(8)
            .clip(true)
            .style(theme::rounded_bg(p.bg_sidebar, 6.0)),
        );
    }
    container(scrollable(r).direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::new().width(4).scroller_width(4),
    )))
    .padding(Padding::from([8, 12]))
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(p.bg_sidebar)),
        border: Border {
            radius: iced::border::Radius::default().top(8.0),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

fn upload_strip<'a>(app: &'a App, u: &'a crate::app::upload::ActiveUpload) -> El<'a> {
    let p = app.theme.palette;
    container(
        row![
            icon(Icon::File, 20.0, p.accent),
            column![
                text(format!("Uploading {}", u.label))
                    .size(13)
                    .color(p.text_normal)
                    .wrapping(text::Wrapping::None),
                progress_bar(0.0..=1.0, u.fraction())
                    .girth(4)
                    .style(move |_| progress_bar::Style {
                        background: Background::Color(p.bg_app),
                        bar: Background::Color(p.accent),
                        border: Border {
                            radius: 2.0.into(),
                            ..Default::default()
                        },
                    }),
            ]
            .spacing(4)
            .width(Length::Fill),
            text(format!("{:.0}%", u.fraction() * 100.0))
                .size(12)
                .color(p.text_muted),
            button(icon(Icon::X, 16.0, p.text_secondary))
                .padding(4)
                .on_press(ui(Ui::CancelUpload(u.id)))
                .style(theme::icon_button(p)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([8, 12]))
    .width(Length::Fill)
    .style(theme::bg(p.bg_sidebar))
    .into()
}

fn typing<'a>(app: &'a App, c: &'a Channel) -> El<'a> {
    let p = app.theme.palette;
    let names = app.chat.typing_names(c.id, now_ms());
    let mut spans: Vec<iced::widget::text::Span<'a, Ui, Font>> = Vec::new();
    let bold = |s: String| span(s).font(INTER_BOLD).color(p.text_normal).size(12);
    let reg = |s: &str| span(s.to_owned()).font(INTER).color(p.text_normal).size(12);
    match names.len() {
        0 => {}
        1 => {
            spans.push(bold(names[0].clone()));
            spans.push(reg(" is typing…"));
        }
        2 => {
            spans.push(bold(names[0].clone()));
            spans.push(reg(" and "));
            spans.push(bold(names[1].clone()));
            spans.push(reg(" are typing…"));
        }
        3 => {
            spans.push(bold(names[0].clone()));
            spans.push(reg(", "));
            spans.push(bold(names[1].clone()));
            spans.push(reg(" and "));
            spans.push(bold(names[2].clone()));
            spans.push(reg(" are typing…"));
        }
        _ => spans.push(reg("Several people are typing…")),
    }
    let content: El<'a> = if spans.is_empty() {
        Space::new().into()
    } else {
        row![
            text("•••").size(12).font(INTER_BOLD).color(p.text_normal),
            rich_text(spans).size(12)
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    };
    container(content)
        .height(24)
        .padding(Padding::from([0, 16]).left(24))
        .align_y(Alignment::Center)
        .into()
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use iced::advanced::layout::Limits;
    use iced::advanced::renderer::Headless;
    use iced::advanced::widget::Tree;
    use iced::{Pixels, Size};

    fn height_of(mut el: El<'_>) -> f32 {
        let renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(
            INTER,
            Pixels(16.0),
            Some("tiny-skia"),
        ))
        .expect("tiny-skia renderer");
        let mut tree = Tree::new(el.as_widget());
        el.as_widget_mut()
            .layout(
                &mut tree,
                &renderer,
                &Limits::new(Size::ZERO, Size::new(800.0, 5000.0)),
            )
            .size()
            .height
    }

    fn app_and_channel() -> (App, Channel) {
        let app = App::boot(crate::config::Config::for_tests()).0;
        let c = app
            .chat
            .channels
            .get(&crate::mock::CH_GENERAL)
            .cloned()
            .expect("mock channel");
        (app, c)
    }

    #[test]
    fn empty_composer_is_one_official_line_tall() {
        let (app, c) = app_and_channel();
        assert!(app.composer.is_empty());
        // 22 px line + 11 px above and below, like the official client;
        // it used to be 68 px (min height + padding counted twice).
        let h = height_of(composer_bar(&app, &c));
        assert!((h - 44.0).abs() < 0.5, "composer height {h}");
    }

    #[test]
    fn composer_grows_one_line_at_a_time() {
        let (mut app, c) = app_and_channel();
        app.composer.set_text("one\ntwo");
        let two = height_of(composer_bar(&app, &c));
        assert!((two - 66.0).abs() < 0.5, "two lines: {two}");
        app.composer.set_text("one\ntwo\nthree");
        let three = height_of(composer_bar(&app, &c));
        assert!((three - 88.0).abs() < 0.5, "three lines: {three}");
    }

    #[test]
    fn composer_limits_exclude_the_padding() {
        let (min, max) = composer::text_height_limits(800.0);
        assert_eq!(min, composer::LINE_HEIGHT);
        assert_eq!(max + 2.0 * composer::PADDING_Y, 400.0);
        // Tiny windows still get one line.
        assert!(composer::text_height_limits(10.0).1 >= composer::LINE_HEIGHT);
    }
}
