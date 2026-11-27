//! Home page without an open DM: Friends (Online / All / Implicit / Pending
//! / Add Friend), Message Requests, and the "Find or start a conversation"
//! switcher.

use iced::widget::{
    Space, button, column, container, row, rule, scrollable, text, text_input, tooltip,
};
use iced::{Alignment, Length, Padding};

use super::icons::{Icon, icon};
use super::theme::{self, INTER, INTER_MEDIUM, INTER_SEMIBOLD};
use super::{El, avatar_with_presence, initials_circle, mention_badge};
use crate::app::cosmetics::Phase;
use crate::app::friends::{
    AddStatus, FriendRow, FriendsTab, FriendsUi, HomePage, SWITCHER_LIMIT, SwitchTarget,
    add_friend_input_id, incoming_count, rows, switcher_hits, switcher_input_id,
};
use crate::app::{App, Message, Ui};
use crate::net::cosmetics::ActivityKind;
use crate::net::model::{Channel, ChannelKind, Presence, RelationshipKind, UserId};

fn fu(f: FriendsUi) -> Message {
    Message::Ui(Ui::Friends(f))
}

/// The Friends or Message Requests page.
pub fn view(app: &App) -> El<'_> {
    match app.friends.page {
        HomePage::Friends => friends(app),
        HomePage::Requests => requests(app),
    }
}

fn header_bar<'a>(app: &'a App, content: El<'a>) -> El<'a> {
    let t = &app.theme;
    container(content)
        .height(t.metrics.header_height)
        .width(Length::Fill)
        .padding(Padding::from([0.0, t.metrics.space_l]))
        .align_y(Alignment::Center)
        .style(theme::header(t.palette))
        .into()
}

fn title<'a>(app: &'a App, i: Icon, label: &'static str) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    row![
        icon(i, 22.0, p.channel_icon),
        text(label)
            .size(t.metrics.font_m)
            .font(INTER_SEMIBOLD)
            .color(p.header_text),
    ]
    .spacing(t.metrics.space_m)
    .align_y(Alignment::Center)
    .into()
}

fn dot<'a>(app: &'a App) -> El<'a> {
    let p = app.theme.palette;
    container(
        text("\u{2022}")
            .size(app.theme.metrics.font_s)
            .color(p.text_muted),
    )
    .padding(Padding::from([0, 4]))
    .into()
}

fn tab<'a>(
    app: &'a App,
    label: &'static str,
    selected: bool,
    badge: usize,
    msg: FriendsUi,
) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let mut r = row![text(label).size(t.metrics.font_m).font(INTER_MEDIUM)]
        .spacing(6)
        .align_y(Alignment::Center);
    if badge > 0 {
        r = r.push(mention_badge(&p, badge as u32, p.bg_panel));
    }
    button(r)
        .padding(Padding::from([4, 12]))
        .on_press(fu(msg))
        .style(theme::tab_button(p, selected))
        .into()
}

fn friends(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let f = &app.friends;
    let pending = incoming_count(&app.chat);
    let mut tabs = row![title(app, Icon::Users, "Friends"), dot(app)]
        .spacing(t.metrics.space_s)
        .align_y(Alignment::Center);
    for k in FriendsTab::LIST {
        let badge = if k == FriendsTab::Pending { pending } else { 0 };
        tabs = tabs.push(tab(app, k.label(), f.tab == k, badge, FriendsUi::Tab(k)));
    }
    let add_selected = f.tab == FriendsTab::AddFriend;
    tabs = tabs.push(
        button(text("Add Friend").size(t.metrics.font_m).font(INTER_MEDIUM))
            .padding(Padding::from([4, 12]))
            .on_press(fu(FriendsUi::Tab(FriendsTab::AddFriend)))
            .style(theme::add_friend_tab(p, add_selected)),
    );
    let header = header_bar(
        app,
        scrollable(tabs)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(0).scroller_width(0),
            ))
            .into(),
    );
    let body = if add_selected {
        add_friend(app)
    } else {
        list(app)
    };
    column![header, body].into()
}

fn search_box<'a>(app: &'a App) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    container(
        row![
            icon(Icon::Search, 18.0, p.text_muted),
            text_input("Search", &app.friends.query)
                .on_input(|s| fu(FriendsUi::Query(s)))
                .size(t.metrics.font_m)
                .font(INTER)
                .padding(Padding::from([6, 0]))
                .style(theme::input(p, p.bg_app)),
        ]
        .spacing(t.metrics.space_m)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([2.0, t.metrics.space_m + 4.0]))
    .width(Length::Fill)
    .style(theme::rounded_bg(p.bg_app, t.metrics.radius_large))
    .into()
}

fn list(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let f = &app.friends;
    let people = rows(&app.chat, f.tab, &f.query);
    let label = match f.tab {
        FriendsTab::Online => "Online",
        FriendsTab::All => "All friends",
        FriendsTab::Implicit => "Implicit",
        FriendsTab::Pending => "Pending",
        FriendsTab::AddFriend => "",
    };
    let mut col = column![
        search_box(app),
        container(
            text(format!("{label} \u{2014} {}", people.len()))
                .size(t.metrics.font_s)
                .font(INTER_MEDIUM)
                .color(p.text_secondary),
        )
        .padding(
            Padding::ZERO
                .top(t.metrics.space_l + 4.0)
                .bottom(t.metrics.space_m)
        ),
    ];
    if people.is_empty() {
        let msg = if !f.query.trim().is_empty() {
            "Nobody here matches that search."
        } else {
            match f.tab {
                FriendsTab::Online => "No one's around to play with your friends.",
                FriendsTab::All => "No friends yet. Add someone with their username.",
                FriendsTab::Implicit => {
                    "No implicit friends. Discord lists people you talk to a lot here."
                }
                FriendsTab::Pending => "There are no pending friend requests.",
                FriendsTab::AddFriend => "",
            }
        };
        col = col.push(
            container(text(msg).size(t.metrics.font_m).color(p.text_muted))
                .padding(Padding::ZERO.top(40.0))
                .center_x(Length::Fill),
        );
    }
    for r in &people {
        col = col.push(friend_row(app, r));
    }
    super::smooth_scroll::smooth(
        scrollable(
            col.padding(Padding::from([t.metrics.space_l, t.metrics.space_l + 14.0]))
                .width(Length::Fill),
        )
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(4).scroller_width(4),
        ))
        .style(theme::thin_scrollbar(p, false))
        .height(Length::Fill),
    )
    .into()
}

fn action<'a>(app: &'a App, i: Icon, tip: &'static str, msg: Option<FriendsUi>) -> El<'a> {
    let p = app.theme.palette;
    let mut b = button(icon(i, 20.0, p.text_secondary))
        .padding(8)
        .style(theme::round_action(p));
    if let Some(m) = msg {
        b = b.on_press(fu(m));
    }
    tooltip(
        b,
        container(text(tip).size(13).font(INTER_SEMIBOLD).color(p.header_text))
            .padding(Padding::from([6, 10]))
            .style(theme::rounded_bg(p.bg_elevated, 5.0)),
        tooltip::Position::Top,
    )
    .gap(6)
    .into()
}

/// Second line of a friend row: custom status, else the presence.
fn status_line(app: &App, r: &FriendRow) -> String {
    match r.kind {
        RelationshipKind::Incoming => "Incoming Friend Request".into(),
        RelationshipKind::Outgoing => "Outgoing Friend Request".into(),
        RelationshipKind::Implicit | RelationshipKind::Blocked => r.username.clone(),
        RelationshipKind::Friend => {
            let custom = app.chat.activities.get(&r.user).and_then(|v| {
                v.iter()
                    .find(|a| a.kind == ActivityKind::Custom)
                    .and_then(|a| a.state.clone())
            });
            custom.unwrap_or_else(|| match app.chat.presence_of(r.user) {
                Presence::Offline => "Offline".into(),
                s => s.label().into(),
            })
        }
    }
}

fn friend_row<'a>(app: &'a App, r: &FriendRow) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let f = &app.friends;
    let busy = f.busy.contains(&r.user);
    let go = |m: FriendsUi| (!busy).then_some(m);
    let presence = app.chat.presence_of(r.user);
    let pic: El<'a> = match app.chat.users.get(&r.user) {
        Some(u) => avatar_with_presence(
            app,
            u,
            t.metrics.avatar_small,
            presence,
            p.bg_panel,
            Phase::Still,
        ),
        None => initials_circle(
            &p,
            &r.name,
            theme::default_avatar_color(r.user.0),
            t.metrics.avatar_small,
        ),
    };
    let mut name = row![
        text(r.name.clone())
            .size(t.metrics.font_m)
            .font(INTER_SEMIBOLD)
            .color(p.header_text)
            .wrapping(text::Wrapping::None)
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if r.username != r.name && !r.username.is_empty() && r.kind != RelationshipKind::Implicit {
        name = name.push(
            text(r.username.clone())
                .size(t.metrics.font_s)
                .color(p.text_muted)
                .wrapping(text::Wrapping::None),
        );
    }
    let info = column![
        name,
        text(status_line(app, r))
            .size(t.metrics.font_s)
            .color(p.text_secondary)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(1)
    .width(Length::Fill)
    .clip(true);
    let actions: El<'a> = if f.confirm_remove == Some(r.user) {
        row![
            button(text("Cancel").size(t.metrics.font_s).font(INTER_MEDIUM))
                .padding(Padding::from([6, 12]))
                .on_press(fu(FriendsUi::CancelRemove))
                .style(theme::secondary_button(p)),
            button(
                text("Remove Friend")
                    .size(t.metrics.font_s)
                    .font(INTER_MEDIUM)
            )
            .padding(Padding::from([6, 12]))
            .on_press_maybe(go(FriendsUi::Remove(r.user)).map(fu))
            .style(theme::danger_button(p)),
        ]
        .spacing(t.metrics.space_m)
        .into()
    } else {
        let u = r.user;
        let mut a = row![].spacing(10);
        match r.kind {
            RelationshipKind::Friend => {
                a = a
                    .push(action(
                        app,
                        Icon::MessageCircle,
                        "Message",
                        Some(FriendsUi::Message(u)),
                    ))
                    .push(action(
                        app,
                        Icon::X,
                        "Remove Friend",
                        go(FriendsUi::AskRemove(u)),
                    ));
            }
            RelationshipKind::Incoming => {
                a = a
                    .push(action(app, Icon::Check, "Accept", go(FriendsUi::Add(u))))
                    .push(action(app, Icon::X, "Ignore", go(FriendsUi::Remove(u))));
            }
            RelationshipKind::Outgoing => {
                a = a.push(action(app, Icon::X, "Cancel", go(FriendsUi::Remove(u))));
            }
            RelationshipKind::Implicit => {
                a = a
                    .push(action(
                        app,
                        Icon::MessageCircle,
                        "Message",
                        Some(FriendsUi::Message(u)),
                    ))
                    .push(action(
                        app,
                        Icon::UserPlus,
                        "Add Friend",
                        go(FriendsUi::Add(u)),
                    ));
            }
            RelationshipKind::Blocked => {}
        }
        a.into()
    };
    column![
        rule::horizontal(1).style(move |_| rule::Style {
            color: p.bg_hover,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        }),
        container(
            row![pic, info, actions]
                .spacing(12)
                .align_y(Alignment::Center)
        )
        .padding(Padding::from([10, 8]))
    ]
    .into()
}

fn add_friend(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let f = &app.friends;
    let sending = matches!(f.add_status, Some(AddStatus::Sending));
    let border = match &f.add_status {
        Some(AddStatus::Sent(_)) => p.success,
        Some(AddStatus::Failed(_)) => p.danger,
        _ => p.bg_app,
    };
    let can_send = !sending && !f.add_input.trim().is_empty();
    let input = container(
        row![
            text_input(
                "You can add friends with their Discord username.",
                &f.add_input
            )
            .id(add_friend_input_id())
            .on_input(|s| fu(FriendsUi::AddInput(s)))
            .on_submit(fu(FriendsUi::SubmitAdd))
            .size(t.metrics.font_m)
            .font(INTER)
            .padding(Padding::from([10, 0]))
            .style(theme::input(p, p.bg_app)),
            button(
                text(if sending {
                    "Sending..."
                } else {
                    "Send Friend Request"
                })
                .size(t.metrics.font_s)
                .font(INTER_MEDIUM)
            )
            .padding(Padding::from([8, 16]))
            .on_press_maybe(can_send.then(|| fu(FriendsUi::SubmitAdd)))
            .style(theme::primary_button(p)),
        ]
        .spacing(t.metrics.space_m)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([4.0, t.metrics.space_m + 4.0]).right(t.metrics.space_m))
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(p.bg_app)),
        border: iced::Border {
            radius: t.metrics.radius_large.into(),
            width: 1.0,
            color: border,
        },
        ..Default::default()
    });
    let status: El<'_> = match &f.add_status {
        Some(AddStatus::Sent(name)) => {
            text(format!("Success! Your friend request to {name} was sent."))
                .size(t.metrics.font_s)
                .color(p.success)
                .into()
        }
        Some(AddStatus::Failed(e)) => text(e.clone())
            .size(t.metrics.font_s)
            .color(p.danger)
            .into(),
        _ => Space::new().into(),
    };
    container(
        column![
            text("Add Friend")
                .size(t.metrics.font_l)
                .font(INTER_SEMIBOLD)
                .color(p.header_text),
            text("You can add friends with their Discord username.")
                .size(t.metrics.font_s)
                .color(p.text_secondary),
            Space::new().height(t.metrics.space_m),
            input,
            status,
        ]
        .spacing(t.metrics.space_m),
    )
    .padding(Padding::from([20.0, t.metrics.space_l + 14.0]))
    .width(Length::Fill)
    .into()
}

// -- Message Requests ------------------------------------------------------------

fn requests(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let f = &app.friends;
    let normal = app.chat.message_requests(false).len();
    let header = header_bar(
        app,
        row![
            title(app, Icon::Mail, "Message Requests"),
            dot(app),
            tab(app, "Requests", !f.spam, 0, FriendsUi::SpamTab(false)),
            tab(app, "Spam", f.spam, 0, FriendsUi::SpamTab(true)),
        ]
        .spacing(t.metrics.space_s)
        .align_y(Alignment::Center)
        .into(),
    );
    let items = app.chat.message_requests(f.spam);
    let mut col = column![
        container(
            text(format!(
                "{} \u{2014} {}",
                if f.spam { "Spam" } else { "Message requests" },
                items.len()
            ))
            .size(t.metrics.font_s)
            .font(INTER_MEDIUM)
            .color(p.text_secondary),
        )
        .padding(Padding::ZERO.bottom(t.metrics.space_m)),
    ];
    if items.is_empty() {
        let msg = if f.spam {
            "No spam. Discord files suspicious message requests here."
        } else if normal == 0 {
            "No pending message requests. When someone who isn't your friend messages you, it shows up here."
        } else {
            ""
        };
        col = col.push(
            container(text(msg).size(t.metrics.font_m).color(p.text_muted))
                .padding(Padding::ZERO.top(40.0))
                .center_x(Length::Fill),
        );
    }
    for c in items {
        col = col.push(request_row(app, c));
    }
    column![
        header,
        super::smooth_scroll::smooth(
            scrollable(
                col.padding(Padding::from([
                    t.metrics.space_l + 4.0,
                    t.metrics.space_l + 14.0
                ]))
                .width(Length::Fill)
            )
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(4).scroller_width(4),
            ))
            .style(theme::thin_scrollbar(p, false))
            .height(Length::Fill),
        )
    ]
    .into()
}

fn request_row<'a>(app: &'a App, c: &'a Channel) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let label = c.label(|u| app.chat.user_name(u));
    let user = c.recipients.first().and_then(|u| app.chat.users.get(u));
    let pic: El<'a> = match user {
        Some(u) => super::avatar(app, u, t.metrics.avatar),
        None => initials_circle(
            &p,
            &label,
            theme::default_avatar_color(c.id.0),
            t.metrics.avatar,
        ),
    };
    let preview = app
        .chat
        .messages
        .get(&c.id)
        .and_then(|w| w.messages.back())
        .map(|m| super::plain_text(app, &m.content))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Sent you a message".into());
    let busy = app.friends.busy_requests.contains(&c.id);
    let info = column![
        row![
            text(label)
                .size(t.metrics.font_m)
                .font(INTER_SEMIBOLD)
                .color(p.header_text)
                .wrapping(text::Wrapping::None),
            text(user.map(|u| u.username.clone()).unwrap_or_default())
                .size(t.metrics.font_s)
                .color(p.text_muted)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        text(super::ellipsize(&preview, 420.0, t.metrics.font_s, false))
            .size(t.metrics.font_s)
            .color(p.text_secondary)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(2)
    .width(Length::Fill)
    .clip(true);
    let id = c.id;
    let actions = row![
        button(text("Ignore").size(t.metrics.font_s).font(INTER_MEDIUM))
            .padding(Padding::from([6, 14]))
            .on_press_maybe((!busy).then(|| fu(FriendsUi::RejectRequest(id))))
            .style(theme::secondary_button(p)),
        button(text("Accept").size(t.metrics.font_s).font(INTER_MEDIUM))
            .padding(Padding::from([6, 14]))
            .on_press_maybe((!busy).then(|| fu(FriendsUi::AcceptRequest(id))))
            .style(theme::primary_button(p)),
    ]
    .spacing(t.metrics.space_m);
    column![
        rule::horizontal(1).style(move |_| rule::Style {
            color: p.bg_hover,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        }),
        container(
            row![pic, info, actions]
                .spacing(12)
                .align_y(Alignment::Center)
        )
        .padding(Padding::from([12, 8]))
    ]
    .into()
}

// -- "Find or start a conversation" ---------------------------------------------

/// The switcher dialog (Ctrl+K or the button above the DM list).
pub fn switcher(app: &App) -> Option<El<'_>> {
    let s = app.friends.switcher.as_ref()?;
    let t = &app.theme;
    let p = t.palette;
    let hits = switcher_hits(&app.chat, &s.query, SWITCHER_LIMIT);
    let input = text_input("Where would you like to go?", &s.query)
        .id(switcher_input_id())
        .on_input(|q| fu(FriendsUi::SwitcherQuery(q)))
        .on_submit(fu(FriendsUi::SwitcherSubmit))
        .size(t.metrics.font_l)
        .font(INTER)
        .padding(14)
        .style(theme::input(p, p.bg_app));
    let mut list = column![].spacing(t.metrics.space_xs);
    if hits.is_empty() {
        list = list.push(
            container(
                text("Can't find what you're looking for?")
                    .size(t.metrics.font_s)
                    .color(p.text_muted),
            )
            .padding(Padding::from([12, 8])),
        );
    } else {
        list = list.push(
            container(
                text(if s.query.trim().is_empty() {
                    "PREVIOUS CHANNELS"
                } else {
                    "RESULTS"
                })
                .size(t.metrics.font_xs)
                .font(INTER_SEMIBOLD)
                .color(p.text_muted),
            )
            .padding(Padding::from([6, 8])),
        );
    }
    for (i, h) in hits.iter().enumerate() {
        let lead: El<'_> = match h.target {
            SwitchTarget::Channel(c) => match app.chat.channels.get(&c) {
                Some(ch) if ch.kind == ChannelKind::Dm => {
                    person_icon(app, ch.recipients.first().copied(), &h.label)
                }
                Some(ch) if ch.kind == ChannelKind::GroupDm => {
                    icon(Icon::Users, 20.0, p.channel_icon)
                }
                _ => icon(Icon::Hash, 20.0, p.channel_icon),
            },
            SwitchTarget::User(u) => person_icon(app, Some(u), &h.label),
            SwitchTarget::Guild(g) => {
                initials_circle(&p, &h.label, theme::default_avatar_color(g.0), 20.0)
            }
        };
        let mut r = row![
            container(lead).width(24).center_x(24),
            text(h.label.clone())
                .size(t.metrics.font_m)
                .font(INTER_MEDIUM)
                .color(p.header_text)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if let Some(d) = &h.detail {
            r = r.push(
                text(d.clone())
                    .size(t.metrics.font_s)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
            );
        }
        if i == 0 {
            r = r.push(Space::new().width(Length::Fill)).push(
                text("Enter")
                    .size(t.metrics.font_xs)
                    .font(INTER_SEMIBOLD)
                    .color(p.text_muted),
            );
        }
        list = list.push(
            button(r)
                .padding(Padding::from([6, 8]))
                .width(Length::Fill)
                .on_press(fu(FriendsUi::SwitcherPick(h.target)))
                .style(theme::list_row(p, i == 0)),
        );
    }
    let card = container(
        column![
            input,
            container(
                scrollable(list)
                    .direction(scrollable::Direction::Vertical(
                        scrollable::Scrollbar::new()
                            .width(4)
                            .scroller_width(4)
                            .spacing(6),
                    ))
                    .style(theme::thin_scrollbar(p, false))
                    .height(Length::Shrink),
            )
            .max_height(400),
            text("Tip: press Ctrl + K anywhere to open this.")
                .size(t.metrics.font_xs)
                .color(p.text_muted),
        ]
        .spacing(t.metrics.space_m + 4.0),
    )
    .padding(20)
    .width(570)
    .style(theme::popup(p));
    Some(super::servers::modal(
        card.into(),
        Ui::Friends(FriendsUi::CloseSwitcher),
    ))
}

fn person_icon<'a>(app: &'a App, user: Option<UserId>, label: &str) -> El<'a> {
    let p = app.theme.palette;
    match user.and_then(|u| app.chat.users.get(&u)) {
        Some(u) => super::avatar(app, u, 20.0),
        None => initials_circle(&p, label, theme::default_avatar_color(0), 20.0),
    }
}
