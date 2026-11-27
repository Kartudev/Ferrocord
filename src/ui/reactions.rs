//! Reaction pills: the "who reacted" tooltip and the Reactions dialog.

use std::time::Duration;

use iced::widget::{button, column, container, mouse_area, row, scrollable, text, tooltip};
use iced::{Alignment, Length, Padding};

use super::emoji_font::emoji_text;
use super::theme::{self, INTER, INTER_MEDIUM, INTER_SEMIBOLD};
use super::{El, avatar, ui};
use crate::app::reactions::{
    self, HOVER_DELAY_MS, ReactionKey, TOOLTIP_NAMES, emoji_label, pill_kind, tooltip_text,
};
use crate::app::{App, Ui, emoji_url};
use crate::net::model::{self as model, Emoji, Reaction};
use crate::ui::anim_image::message_anim;

const DIALOG_W: f32 = 520.0;
const DIALOG_H: f32 = 420.0;
const TABS_W: f32 = 132.0;
const TOOLTIP_W: f32 = 240.0;

/// The emoji of a reaction at `size` (custom emoji image, animated when
/// decoded and `animate`, or the Twemoji glyph).
pub fn face<'a>(app: &'a App, emoji: &Emoji, size: f32, animate: bool) -> El<'a> {
    let p = app.theme.palette;
    match emoji.id {
        Some(id) => match (
            app.emoji_anim(id, emoji.animated).filter(|_| animate),
            app.image(&emoji_url(&app.cfg.cdn_url, id)),
        ) {
            (Some(a), _) => message_anim(app, a, size, size)
                .content_fit(iced::ContentFit::Contain)
                .into(),
            (None, Some(h)) => iced::widget::image(h.clone())
                .width(size)
                .height(size)
                .into(),
            (None, None) => text(format!(":{}:", emoji.name))
                .size(12)
                .color(p.text_muted)
                .into(),
        },
        None => emoji_text(emoji.name.clone(), size - 2.0, p.text_normal, INTER, false),
    }
}

/// A reaction pill with its hover tooltip ("Alice, Bob, and 3 others
/// reacted with :x:") and right-click to the Reactions dialog. The
/// reactors are fetched only once the pointer rests on the pill.
pub fn pill<'a>(
    app: &'a App,
    channel: model::ChannelId,
    m: &'a model::Message,
    re: &'a Reaction,
) -> El<'a> {
    let p = app.theme.palette;
    let button = button(
        row![
            face(app, &re.emoji, 18.0, true),
            text(re.count.to_string())
                .size(14)
                .font(INTER_MEDIUM)
                .color(if re.me { p.link } else { p.text_secondary }),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([2, 6]))
    .on_press(ui(Ui::ToggleReaction(m.id, re.emoji.clone())))
    .style(theme::reaction_pill(p, re.me));
    let (burst, count) = pill_kind(re);
    let key = ReactionKey::new(channel, m.id, &re.emoji, burst);
    let tip = container(
        row![
            face(app, &re.emoji, 32.0, false),
            text(tooltip_line(app, &key, &re.emoji, count))
                .size(14)
                .font(INTER_MEDIUM)
                .color(p.header_text)
                .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(TOOLTIP_W)
    .padding(Padding::from([8, 12]))
    .style(theme::popup(p));
    let tip = tooltip(button, tip, tooltip::Position::Top)
        .gap(6)
        .delay(Duration::from_millis(HOVER_DELAY_MS));
    mouse_area(tip)
        .on_enter(ui(Ui::ReactionHover(key.clone())))
        .on_exit(ui(Ui::ReactionUnhover(key)))
        .on_right_press(ui(Ui::OpenReactors(m.id, Some(re.emoji.clone()))))
        .into()
}

/// The tooltip text from the cache: the emoji's name until the reactors
/// arrive.
fn tooltip_line(app: &App, key: &ReactionKey, emoji: &Emoji, count: u32) -> String {
    let label = emoji_label(emoji);
    match app.reactions.cache.get(key) {
        Some(e) if !e.users.is_empty() => {
            let names: Vec<&str> = e
                .users
                .iter()
                .take(TOOLTIP_NAMES)
                .map(model::User::display_name)
                .collect();
            tooltip_text(&names, count, &label)
        }
        Some(e) if e.loading => format!("{label}\n\u{2026}"),
        _ => label,
    }
}

/// The Reactions dialog: a tab per emoji (and kind) on the left, the
/// people who reacted with it on the right.
pub fn dialog(app: &App) -> Option<El<'_>> {
    let d = app.reactions.dialog.as_ref()?;
    let p = app.theme.palette;
    let m = app.chat.message(d.channel, d.message)?;
    let mut tabs = column![].spacing(2).padding(8);
    for (emoji, burst, count) in reactions::tabs(&m.reactions) {
        let selected = emoji == d.emoji && burst == d.burst;
        let mut label = row![face(app, &emoji, 20.0, false)]
            .spacing(8)
            .align_y(Alignment::Center);
        if burst {
            label = label.push(text("\u{2728}").size(12).color(p.text_muted));
        }
        label = label.push(text(count.to_string()).size(14).font(INTER_SEMIBOLD).color(
            if selected {
                p.header_text
            } else {
                p.text_secondary
            },
        ));
        tabs = tabs.push(
            button(label)
                .padding(Padding::from([6, 10]))
                .width(Length::Fill)
                .on_press(ui(Ui::ReactorsTab(emoji.clone(), burst)))
                .style(theme::list_row(p, selected)),
        );
    }
    let key = ReactionKey::new(d.channel, d.message, &d.emoji, d.burst);
    let entry = app.reactions.cache.get(&key);
    let mut people = column![].spacing(2).padding(Padding::from([8, 12]));
    if let Some(e) = entry {
        for u in &e.users {
            people = people.push(
                row![
                    avatar(app, u, 24.0),
                    text(u.display_name().to_owned())
                        .size(15)
                        .font(INTER_MEDIUM)
                        .color(p.header_text)
                        .wrapping(text::Wrapping::None),
                    text(u.username.clone())
                        .size(13)
                        .color(p.text_muted)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .padding(Padding::from([6, 4])),
            );
        }
    }
    let status: Option<&str> = match entry {
        None => Some("Loading\u{2026}"),
        Some(e) if e.loading => Some("Loading\u{2026}"),
        Some(e) if e.failed => Some("Could not load who reacted."),
        _ => None,
    };
    if let Some(s) = status {
        people = people.push(container(text(s).size(14).color(p.text_muted)).padding(8));
    } else if let Some(e) = entry
        && !e.complete
        && e.users.len() < reactions::MAX_USERS
    {
        people = people.push(
            container(
                button(text("Load more").size(14).color(p.text_normal))
                    .padding(Padding::from([6, 12]))
                    .on_press(ui(Ui::ReactorsMore))
                    .style(theme::icon_button(p)),
            )
            .center_x(Length::Fill)
            .padding(8),
        );
    }
    let body = row![
        container(scrollable(tabs).height(Length::Fill))
            .width(TABS_W)
            .height(Length::Fill)
            .style(theme::bg(p.bg_sidebar)),
        scrollable(people)
            .style(theme::thin_scrollbar(p, false))
            .width(Length::Fill)
            .height(Length::Fill),
    ];
    let card = container(
        column![
            row![
                text("Reactions")
                    .size(16)
                    .font(INTER_SEMIBOLD)
                    .color(p.header_text)
                    .width(Length::Fill),
                button(super::icons::icon(
                    super::icons::Icon::X,
                    18.0,
                    p.text_secondary
                ))
                .padding(4)
                .on_press(ui(Ui::CloseReactors))
                .style(theme::icon_button(p)),
            ]
            .align_y(Alignment::Center)
            .padding(Padding::from([12, 16])),
            body,
        ]
        .height(Length::Fill),
    )
    .width(DIALOG_W)
    .height(DIALOG_H)
    .clip(true)
    .style(theme::rounded_bg(p.bg_panel, 8.0));
    Some(super::servers::modal(card.into(), Ui::CloseReactors))
}
