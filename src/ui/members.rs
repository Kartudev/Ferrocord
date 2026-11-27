//! Right-hand member list: role groups, presence dots.

use iced::widget::{Space, column, container, row, scrollable, text};
use iced::{Alignment, Color, Length, Padding};

use super::theme::{self, INTER_MEDIUM, INTER_SEMIBOLD};
use super::{El, avatar_with_presence, cosmetics};
use crate::app::anim::{Hover, Zone};
use crate::app::members::{MemberListState, RowMetrics};
use crate::app::{App, Message, Ui};
use crate::net::model::*;

pub const WIDTH: f32 = 240.0;

/// Fixed row heights: the window of rows to draw (and to subscribe to)
/// comes from the scroll offset, so a 100 000-row list costs as much as a
/// short one.
pub const ROWS: RowMetrics = RowMetrics {
    top_pad: 16.0,
    member: 44.0,
    header: 40.0,
};
/// Rows drawn above and below the visible ones.
pub const OVERSCAN: usize = 8;
const BOTTOM_PAD: f32 = 8.0;

/// Id of the member list scrollable.
pub fn scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("member-list")
}

pub fn view(app: &App) -> El<'_> {
    let p = app.theme.palette;
    let Some(list) = &app.chat.member_list else {
        return Space::new().width(0).into();
    };
    let guild = app.chat.guild(list.guild);
    let headers = list.header_rows();
    let window = app.member_window();
    let mut col = column![Space::new().height(ROWS.offset(&headers, window.start))]
        .padding(Padding::from([0, 8]));
    for i in window.clone() {
        let is_header = headers.binary_search(&i).is_ok();
        let height = if is_header { ROWS.header } else { ROWS.member };
        let row: El<'_> = match list.get(i) {
            Some(MemberItem::Group { id, count }) if !id.is_empty() => container(
                text(format!(
                    "{} \u{2014} {count}",
                    MemberListState::group_label(id, guild).to_uppercase()
                ))
                .size(12)
                .font(INTER_SEMIBOLD)
                .color(p.text_muted)
                .wrapping(text::Wrapping::None),
            )
            .padding(Padding::from([0, 8]).bottom(4))
            .align_y(iced::alignment::Vertical::Bottom)
            .into(),
            Some(MemberItem::Member(m)) => member_row(app, m, guild),
            // Not loaded yet (or an item we could not read): a skeleton of
            // the same height, like the official client.
            _ => placeholder(app, i, is_header),
        };
        col = col.push(container(row).width(Length::Fill).height(height).clip(true));
    }
    let below = ROWS.total(&headers, list.len()) - ROWS.offset(&headers, window.end);
    col = col.push(Space::new().height(below.max(0.0) + BOTTOM_PAD));
    if list.is_empty() {
        col = col.push(
            container(text("Loading members\u{2026}").size(14).color(p.text_muted))
                .padding(16)
                .center_x(Length::Fill),
        );
    }
    container(super::smooth_scroll::smooth(
        scrollable(col)
            .id(scroll_id())
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(4).scroller_width(4),
            ))
            .on_scroll(|vp| Message::Ui(Ui::MembersScrolled(vp)))
            .style(theme::thin_scrollbar(p, false))
            .height(Length::Fill),
    ))
    .width(WIDTH)
    .height(Length::Fill)
    .style(theme::bg(p.bg_sidebar))
    .into()
}

/// Grey avatar and name bar for a row that is not loaded.
fn placeholder<'a>(app: &App, index: usize, header: bool) -> El<'a> {
    let p = app.theme.palette;
    let bar = |w: f32, h: f32| {
        container(Space::new().width(w).height(h)).style(theme::rounded_bg(p.bg_hover, h / 2.0))
    };
    // Varying name widths, stable per row.
    let w = 60.0 + ((index * 37) % 70) as f32;
    if header {
        return container(bar(w, 10.0))
            .padding(Padding::from([0, 8]).bottom(6))
            .height(Length::Fill)
            .align_y(iced::alignment::Vertical::Bottom)
            .into();
    }
    container(
        row![bar(32.0, 32.0), bar(w, 12.0)]
            .spacing(12)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([6, 8]))
    .into()
}

/// Colour of the highest role with a colour.
fn role_color(guild: Option<&Guild>, roles: &[RoleId]) -> Option<u32> {
    guild?
        .roles
        .iter()
        .filter(|r| r.color != 0 && roles.contains(&r.id))
        .max_by_key(|r| r.position)
        .map(|r| r.color)
}

fn member_row<'a>(app: &'a App, m: &'a MemberEntry, guild: Option<&Guild>) -> El<'a> {
    let p = app.theme.palette;
    let offline = m.presence == Presence::Offline;
    let base = role_color(guild, &m.roles)
        .map(theme::from_int)
        .unwrap_or(p.text_secondary);
    let color = if offline {
        Color { a: 0.4, ..base }
    } else {
        base
    };
    person_row(
        app,
        &m.user,
        m.display_name(),
        color,
        m.presence,
        guild.map(|g| g.id),
    )
}

/// Avatar with presence, name and (for bots) a BOT tag: one list row.
fn person_row<'a>(
    app: &'a App,
    user: &User,
    name: &str,
    color: Color,
    presence: Presence,
    guild: Option<GuildId>,
) -> El<'a> {
    let p = app.theme.palette;
    let mut name_row = row![cosmetics::name(app, user, name, 16.0, INTER_MEDIUM, color)]
        .spacing(6)
        .align_y(Alignment::Center);
    if let Some(tag) = cosmetics::tag_chip(app, user) {
        name_row = name_row.push(tag);
    }
    if user.bot {
        name_row = name_row.push(bot_tag(app));
    }
    let h = Hover::row(user.id, Zone::Members);
    let content: El<'a> = container(
        row![
            avatar_with_presence(app, user, 32.0, presence, p.bg_sidebar, app.list_phase(h),),
            name_row,
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([6, 8]))
    .width(Length::Fill)
    .into();
    let content = cosmetics::with_nameplate(app, user, content);
    iced::widget::mouse_area(cosmetics::clickable(content, user, guild))
        .on_enter(Message::Ui(Ui::HoverEnter(h)))
        .on_exit(Message::Ui(Ui::HoverLeave(h)))
        .into()
}

fn bot_tag<'a>(app: &App) -> El<'a> {
    let p = app.theme.palette;
    container(
        text("BOT")
            .size(10)
            .font(INTER_SEMIBOLD)
            .color(p.accent_text),
    )
    .padding(Padding::from([1, 4]))
    .style(theme::rounded_bg(p.accent, 3.0))
    .into()
}

/// Right-hand panel of an open DM: the other person's profile in a 1:1 DM,
/// the participant list in a group DM.
pub fn dm_view<'a>(app: &'a App, c: &Channel) -> El<'a> {
    let p = app.theme.palette;
    let people = app.chat.dm_participants(c);
    let body: El<'a> = match (c.kind, people.as_slice()) {
        (ChannelKind::Dm, [other]) => profile(app, other),
        _ => {
            let mut col = column![
                container(
                    text(format!("MEMBERS \u{2014} {}", people.len()))
                        .size(12)
                        .font(INTER_SEMIBOLD)
                        .color(p.text_muted)
                        .wrapping(text::Wrapping::None),
                )
                .padding(Padding::from([0, 8]).top(4).bottom(4))
            ]
            .padding(Padding::from([8, 8]).top(16));
            for u in people {
                let presence = app.chat.presence_of(u.id);
                let color = if presence == Presence::Offline {
                    Color {
                        a: 0.4,
                        ..p.text_secondary
                    }
                } else {
                    p.text_secondary
                };
                col = col.push(person_row(app, u, u.display_name(), color, presence, None));
            }
            super::smooth_scroll::smooth(
                scrollable(col)
                    .direction(scrollable::Direction::Vertical(
                        scrollable::Scrollbar::new().width(4).scroller_width(4),
                    ))
                    .style(theme::thin_scrollbar(p, false))
                    .height(Length::Fill),
            )
            .into()
        }
    };
    container(body)
        .width(WIDTH)
        .height(Length::Fill)
        .style(theme::bg(p.bg_sidebar))
        .into()
}

/// Profile card of the other person in a 1:1 DM.
fn profile<'a>(app: &'a App, u: &User) -> El<'a> {
    super::profile::panel(app, u)
}
