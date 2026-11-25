//! "Add a Server" dialogs (create / join), the folder settings dialog and
//! the Discover page.

use iced::widget::{
    Space, button, column, container, mouse_area, opaque, row, scrollable, text, text_input,
};
use iced::{Alignment, Color, Length, Padding};

use super::icons::{Icon, icon};
use super::theme::{self, INTER, INTER_MEDIUM, INTER_SEMIBOLD};
use super::{El, initials_circle, ui};
use crate::app::folders::FOLDER_COLORS;
use crate::app::servers::{CreateForm, JoinForm, JoinStep, ServerDialog};
use crate::app::{App, Ui};
use crate::net::extras::{DiscoverGuild, InvitePreview};

pub fn create_name_id() -> iced::widget::Id {
    iced::widget::Id::new("create-server-name")
}

pub fn join_input_id() -> iced::widget::Id {
    iced::widget::Id::new("join-server-invite")
}

fn folder_name_id() -> iced::widget::Id {
    iced::widget::Id::new("folder-name")
}

pub fn folder_name_input_id() -> iced::widget::Id {
    folder_name_id()
}

pub fn discover_search_id() -> iced::widget::Id {
    iced::widget::Id::new("discover-search")
}

/// Dimmed full-window backdrop with `card` in the middle; a click outside
/// the card closes it.
pub(super) fn modal<'a>(card: El<'a>, close: Ui) -> El<'a> {
    opaque(
        mouse_area(
            container(opaque(card))
                .center(Length::Fill)
                .style(theme::bg(Color {
                    a: 0.7,
                    ..Color::BLACK
                })),
        )
        .on_press(ui(close)),
    )
}

pub(super) fn heading<'a>(app: &App, title: &str, sub: &str) -> El<'a> {
    let p = app.theme.palette;
    column![
        text(title.to_owned())
            .size(24)
            .font(INTER_SEMIBOLD)
            .color(p.header_text)
            .align_x(Alignment::Center),
        text(sub.to_owned())
            .size(15)
            .color(p.text_secondary)
            .align_x(Alignment::Center),
    ]
    .spacing(8)
    .align_x(Alignment::Center)
    .width(Length::Fill)
    .into()
}

pub(super) fn label<'a>(app: &App, s: &str) -> El<'a> {
    text(s.to_uppercase())
        .size(12)
        .font(INTER_SEMIBOLD)
        .color(app.theme.palette.text_secondary)
        .into()
}

pub(super) fn primary<'a>(app: &App, s: &str, msg: Option<Ui>) -> El<'a> {
    let p = app.theme.palette;
    let b = button(
        text(s.to_owned())
            .size(14)
            .font(INTER_MEDIUM)
            .color(p.accent_text),
    )
    .padding(Padding::from([10, 20]))
    .style(theme::primary_button(p));
    match msg {
        Some(m) => b.on_press(ui(m)),
        None => b,
    }
    .into()
}

pub(super) fn secondary<'a>(app: &App, s: &str, msg: Ui) -> El<'a> {
    let p = app.theme.palette;
    button(
        text(s.to_owned())
            .size(14)
            .font(INTER_MEDIUM)
            .color(p.text_normal),
    )
    .padding(Padding::from([10, 16]))
    .on_press(ui(msg))
    .style(theme::icon_button(p))
    .into()
}

pub(super) fn error_line<'a>(app: &App, e: &Option<String>) -> El<'a> {
    match e {
        Some(e) => text(e.clone())
            .size(13)
            .color(app.theme.palette.danger)
            .into(),
        None => Space::new().into(),
    }
}

pub(super) fn close_button<'a>(app: &App, msg: Ui) -> El<'a> {
    let p = app.theme.palette;
    button(icon(Icon::X, 20.0, p.text_secondary))
        .padding(4)
        .on_press(ui(msg))
        .style(theme::icon_button(p))
        .into()
}

pub(super) fn card<'a>(
    app: &App,
    body: iced::widget::Column<'a, crate::app::Message>,
    width: f32,
) -> El<'a> {
    container(body.spacing(16))
        .width(width)
        .padding(24)
        .style(theme::rounded_bg(app.theme.palette.bg_panel, 8.0))
        .into()
}

// -- add a server ------------------------------------------------------------------

pub fn add_server(app: &App) -> Option<El<'_>> {
    let d = app.server_dialog.as_ref()?;
    let body = match d {
        ServerDialog::Choose => choose(app),
        ServerDialog::Create(f) => create(app, f),
        ServerDialog::Join(f) => join(app, f),
    };
    Some(modal(body, Ui::CloseAddServer))
}

fn choose(app: &App) -> El<'_> {
    let p = app.theme.palette;
    let big = |i: Icon, title: &str, sub: &str, msg: Ui| -> El<'_> {
        button(
            row![
                icon(i, 28.0, p.accent),
                column![
                    text(title.to_owned())
                        .size(16)
                        .font(INTER_SEMIBOLD)
                        .color(p.header_text),
                    text(sub.to_owned()).size(13).color(p.text_secondary),
                ]
                .spacing(2)
                .width(Length::Fill),
                icon(Icon::ChevronRight, 20.0, p.text_secondary),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        )
        .padding(16)
        .width(Length::Fill)
        .on_press(ui(msg))
        .style(move |t, s| {
            let mut st = theme::icon_button(p)(t, s);
            st.border = iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: p.border,
            };
            st
        })
        .into()
    };
    card(
        app,
        column![
            row![
                heading(
                    app,
                    "Create a Server",
                    "Your server is where you and your friends hang out. Make yours and start talking.",
                ),
                close_button(app, Ui::CloseAddServer),
            ]
            .align_y(Alignment::Start),
            big(
                Icon::Plus,
                "Create My Own",
                "Name it, give it an icon, invite friends.",
                Ui::AddServerCreate
            ),
            container(text("Have an invite already?").size(16).font(INTER_SEMIBOLD).color(p.header_text))
                .center_x(Length::Fill),
            big(
                Icon::Link,
                "Join a Server",
                "Paste an invite link or code.",
                Ui::AddServerJoin
            ),
        ],
        440.0,
    )
}

fn create<'a>(app: &'a App, f: &'a CreateForm) -> El<'a> {
    let p = app.theme.palette;
    let name = text_input("My server", &f.name)
        .id(create_name_id())
        .on_input(|s| crate::app::Message::Ui(Ui::CreateNameChanged(s)))
        .on_submit(crate::app::Message::Ui(Ui::SubmitCreate))
        .size(15)
        .font(INTER)
        .padding(10)
        .style(theme::input(p, p.bg_app));
    let picked = f
        .icon_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned());
    let icon_btn = button(
        container(
            column![
                icon(Icon::Upload, 26.0, p.text_secondary),
                text(if picked.is_some() { "CHANGE" } else { "UPLOAD" })
                    .size(10)
                    .font(INTER_SEMIBOLD)
                    .color(p.text_secondary),
            ]
            .spacing(2)
            .align_x(Alignment::Center),
        )
        .center(80),
    )
    .padding(0)
    .on_press(ui(Ui::PickServerIcon))
    .style(move |_, status| {
        let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(iced::Background::Color(if hot {
                p.bg_hover
            } else {
                p.bg_app
            })),
            border: iced::Border {
                radius: 40.0.into(),
                width: 2.0,
                color: p.text_muted,
            },
            ..Default::default()
        }
    });
    let mut icon_col = column![icon_btn].spacing(6).align_x(Alignment::Center);
    if let Some(n) = picked {
        icon_col = icon_col.push(
            row![
                text(super::ellipsize(&n, 120.0, 12.0, false))
                    .size(12)
                    .color(p.text_secondary),
                button(icon(Icon::X, 14.0, p.text_secondary))
                    .padding(2)
                    .on_press(ui(Ui::ClearServerIcon))
                    .style(theme::icon_button(p)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
    }
    let can = f.can_submit();
    card(
        app,
        column![
            row![
                heading(
                    app,
                    "Customize Your Server",
                    "Give your new server a personality with a name and an icon. You can always change it later.",
                ),
                close_button(app, Ui::CloseAddServer),
            ]
            .align_y(Alignment::Start),
            container(icon_col).center_x(Length::Fill),
            column![label(app, "Server Name"), name].spacing(8),
            text("By creating a server, you agree to Discord's Community Guidelines.")
                .size(12)
                .color(p.text_muted),
            error_line(app, &f.error),
            row![
                secondary(app, "Back", Ui::AddServerBack),
                Space::new().width(Length::Fill),
                primary(
                    app,
                    if f.busy { "Creating\u{2026}" } else { "Create" },
                    can.then_some(Ui::SubmitCreate)
                ),
            ]
            .align_y(Alignment::Center),
        ],
        440.0,
    )
}

fn invite_icon<'a>(app: &'a App, pv: &InvitePreview, size: f32) -> El<'a> {
    let p = app.theme.palette;
    match pv
        .icon_url(&app.cfg.cdn_url)
        .and_then(|u| app.images.get(&u).cloned())
    {
        Some(h) => iced::widget::image(h)
            .width(size)
            .height(size)
            .border_radius(size * 0.3)
            .into(),
        None => square_initials(&p, &pv.name, pv.guild_id.0, size),
    }
}

fn square_initials<'a>(p: &theme::Palette, name: &str, id: u64, size: f32) -> El<'a> {
    let acr: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(3)
        .collect();
    let _ = initials_circle;
    container(
        text(if acr.is_empty() { "?".into() } else { acr })
            .size(size * 0.36)
            .font(INTER_SEMIBOLD)
            .color(p.accent_text),
    )
    .center(size)
    .style(theme::rounded_bg(
        theme::default_avatar_color(id),
        size * 0.3,
    ))
    .into()
}

fn counts<'a>(app: &App, online: Option<u32>, members: Option<u32>) -> El<'a> {
    let p = app.theme.palette;
    let dot = |c: Color| -> El<'a> {
        container(Space::new())
            .width(8)
            .height(8)
            .style(theme::rounded_bg(c, 4.0))
            .into()
    };
    let mut r = row![].spacing(6).align_y(Alignment::Center);
    if let Some(o) = online {
        r = r.push(dot(p.success)).push(
            text(format!("{} Online", group(o)))
                .size(13)
                .color(p.text_secondary),
        );
    }
    if let Some(m) = members {
        r = r.push(dot(p.text_muted)).push(
            text(format!("{} Members", group(m)))
                .size(13)
                .color(p.text_secondary),
        );
    }
    r.into()
}

/// 12345 -> "12,345".
pub fn group(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn join<'a>(app: &'a App, f: &'a JoinForm) -> El<'a> {
    let p = app.theme.palette;
    let input = text_input("https://discord.gg/hTKzmak", &f.input)
        .id(join_input_id())
        .on_input(|s| crate::app::Message::Ui(Ui::JoinInputChanged(s)))
        .on_submit(crate::app::Message::Ui(Ui::SubmitJoin))
        .size(15)
        .font(INTER)
        .padding(10)
        .style(theme::input(p, p.bg_app));
    let preview: El<'a> = match &f.step {
        JoinStep::Preview(pv) | JoinStep::Joining(pv) => container(
            row![
                invite_icon(app, pv, 56.0),
                column![
                    row![
                        text(pv.name.clone())
                            .size(17)
                            .font(INTER_SEMIBOLD)
                            .color(p.header_text),
                        if pv.verified {
                            El::from(icon(Icon::Check, 14.0, p.success))
                        } else {
                            Space::new().into()
                        },
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                    counts(app, pv.online, pv.members),
                    match &pv.inviter {
                        Some(n) => El::from(
                            text(format!("{n} invited you"))
                                .size(12)
                                .color(p.text_muted),
                        ),
                        None => Space::new().into(),
                    },
                ]
                .spacing(4)
                .width(Length::Fill),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        )
        .padding(14)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(p.bg_app)),
            border: iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: p.border,
            },
            ..Default::default()
        })
        .into(),
        _ => Space::new().into(),
    };
    let action: El<'a> = match &f.step {
        JoinStep::Idle => primary(app, "Continue", Some(Ui::SubmitJoin)),
        JoinStep::Resolving => primary(app, "Looking up\u{2026}", None),
        JoinStep::Preview(_) => primary(app, "Join Server", Some(Ui::ConfirmJoin)),
        JoinStep::Joining(_) => primary(app, "Joining\u{2026}", None),
    };
    card(
        app,
        column![
            row![
                heading(
                    app,
                    "Join a Server",
                    "Enter an invite below to join an existing server.",
                ),
                close_button(app, Ui::CloseAddServer),
            ]
            .align_y(Alignment::Start),
            column![label(app, "Invite link or code"), input].spacing(8),
            text("Invites look like hTKzmak, discord.gg/hTKzmak or https://discord.gg/hTKzmak")
                .size(12)
                .color(p.text_muted),
            preview,
            error_line(app, &f.error),
            row![
                secondary(app, "Back", Ui::AddServerBack),
                Space::new().width(Length::Fill),
                action,
            ]
            .align_y(Alignment::Center),
        ],
        440.0,
    )
}

// -- folder settings ------------------------------------------------------------------

pub fn folder_dialog(app: &App) -> Option<El<'_>> {
    let d = app.rail.folder_dialog.as_ref()?;
    let p = app.theme.palette;
    let input = text_input("Folder name", &d.name)
        .id(folder_name_id())
        .on_input(|s| crate::app::Message::Ui(Ui::FolderNameChanged(s)))
        .on_submit(crate::app::Message::Ui(Ui::SaveFolder))
        .size(15)
        .font(INTER)
        .padding(10)
        .style(theme::input(p, p.bg_app));
    let current = d.color.unwrap_or(crate::app::folders::DEFAULT_COLOR);
    let mut swatches = row![].spacing(10);
    for c in FOLDER_COLORS {
        let selected = c == current;
        let col = theme::from_int(c);
        swatches = swatches.push(
            button(
                container(if selected {
                    El::from(icon(Icon::Check, 16.0, p.accent_text))
                } else {
                    Space::new().into()
                })
                .center(32),
            )
            .padding(0)
            .on_press(ui(Ui::FolderColor(Some(c))))
            .style(move |_, _| button::Style {
                background: Some(iced::Background::Color(col)),
                border: iced::Border {
                    radius: 16.0.into(),
                    width: if selected { 2.0 } else { 0.0 },
                    color: p.header_text,
                },
                ..Default::default()
            }),
        );
    }
    let body = card(
        app,
        column![
            row![
                text("Folder Settings")
                    .size(20)
                    .font(INTER_SEMIBOLD)
                    .color(p.header_text)
                    .width(Length::Fill),
                close_button(app, Ui::CloseFolderSettings),
            ]
            .align_y(Alignment::Center),
            column![label(app, "Folder Name"), input].spacing(8),
            column![label(app, "Folder Color"), swatches].spacing(8),
            row![
                Space::new().width(Length::Fill),
                secondary(app, "Cancel", Ui::CloseFolderSettings),
                primary(app, "Save", Some(Ui::SaveFolder)),
            ]
            .spacing(8),
        ],
        420.0,
    );
    Some(modal(body, Ui::CloseFolderSettings))
}

// -- discover ---------------------------------------------------------------------------

pub fn discover(app: &App) -> Option<El<'_>> {
    let d = app.discover.as_ref()?;
    let p = app.theme.palette;
    let t = &app.theme;

    // Category sidebar.
    let mut cats = column![
        container(
            text("Discover")
                .size(18)
                .font(INTER_SEMIBOLD)
                .color(p.header_text)
        )
        .padding(Padding::from([14, 16])),
    ]
    .spacing(2)
    .padding(Padding::from([0, 8]));
    let cat_row = |name: String, id: Option<u32>, selected: bool| -> El<'_> {
        button(text(name).size(15).font(INTER_MEDIUM).color(if selected {
            p.header_text
        } else {
            p.text_secondary
        }))
        .padding(Padding::from([8, 10]))
        .width(Length::Fill)
        .on_press(ui(Ui::DiscoverCategory(id)))
        .style(theme::list_row(p, selected))
        .into()
    };
    cats = cats.push(cat_row("Featured".into(), None, d.category.is_none()));
    for c in d.categories.iter().filter(|c| c.id != 0) {
        cats = cats.push(cat_row(
            c.name.clone(),
            Some(c.id),
            d.category == Some(c.id),
        ));
    }
    let side = container(super::smooth_scroll::smooth(
        scrollable(cats).height(Length::Fill),
    ))
    .width(t.metrics.sidebar_width)
    .height(Length::Fill)
    .style(theme::bg(p.bg_sidebar));

    // Hero with the search box.
    let search = text_input("Explore communities", &d.input)
        .id(discover_search_id())
        .on_input(|s| crate::app::Message::Ui(Ui::DiscoverInput(s)))
        .on_submit(crate::app::Message::Ui(Ui::DiscoverSubmit))
        .size(16)
        .font(INTER)
        .padding(12)
        .style(theme::input(p, p.bg_app));
    let hero = container(
        column![
            text("Find your community on Ferrocord")
                .size(30)
                .font(INTER_SEMIBOLD)
                .color(p.header_text),
            text("From gaming, to music, to learning, there is a place for you.")
                .size(16)
                .color(p.text_secondary),
            container(search).max_width(480),
        ]
        .spacing(10),
    )
    .padding(Padding::from([28, 32]))
    .width(Length::Fill)
    .style(theme::bg(Color {
        a: 0.25,
        ..p.accent
    }));

    // Cards.
    let mut cards: Vec<El<'_>> = Vec::new();
    for g in &d.guilds {
        cards.push(guild_card(app, g, d.joining == Some(g.id)));
    }
    let mut body = column![
        text(if d.query.is_empty() {
            "Featured communities".to_owned()
        } else {
            format!("Results for \u{201C}{}\u{201D}", d.query)
        })
        .size(20)
        .font(INTER_SEMIBOLD)
        .color(p.header_text),
    ]
    .spacing(16)
    .padding(Padding::from([20, 32]));
    if let Some(e) = &d.error {
        body = body.push(text(e.clone()).size(14).color(p.danger));
    }
    if d.loading && d.guilds.is_empty() {
        body = body.push(text("Loading\u{2026}").size(14).color(p.text_muted));
    } else if d.guilds.is_empty() && d.error.is_none() {
        body = body.push(
            column![
                icon(Icon::Compass, 48.0, p.text_muted),
                text("No communities found")
                    .size(16)
                    .font(INTER_SEMIBOLD)
                    .color(p.header_text),
                text("Try a different search or category.")
                    .size(14)
                    .color(p.text_secondary),
            ]
            .spacing(8)
            .align_x(Alignment::Center)
            .width(Length::Fill),
        );
    } else {
        body = body.push(
            iced::widget::row(cards)
                .spacing(16)
                .wrap()
                .vertical_spacing(16),
        );
    }
    if d.has_more() {
        body = body.push(
            button(text("Show more").size(14).color(p.accent_text))
                .padding(Padding::from([8, 18]))
                .on_press(ui(Ui::DiscoverMore))
                .style(theme::primary_button(p)),
        );
    } else if d.loading {
        body = body.push(text("Loading\u{2026}").size(13).color(p.text_muted));
    }

    let top = row![
        container(
            text("Discover")
                .size(16)
                .font(INTER_SEMIBOLD)
                .color(p.header_text)
        )
        .padding(Padding::from([0, 8])),
        Space::new().width(Length::Fill),
        close_button(app, Ui::CloseDiscover),
    ]
    .align_y(Alignment::Center)
    .padding(Padding::from([0, 16]))
    .height(t.metrics.header_height);
    let main = column![
        container(top).width(Length::Fill).style(theme::header(p)),
        super::smooth_scroll::smooth(
            scrollable(column![hero, body])
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new()
                        .width(8)
                        .scroller_width(8)
                        .margin(2),
                ))
                .style(theme::thin_scrollbar(p, true))
                .height(Length::Fill)
        ),
    ]
    .width(Length::Fill);
    let page: El<'_> = container(row![
        side,
        container(main)
            .width(Length::Fill)
            .style(theme::bg(p.bg_panel))
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(Padding::ZERO.left(t.metrics.rail_width))
    .into();
    // The page swallows pointer events so the chat below stays inert.
    let mut layer: El<'_> = opaque(page);
    if let Some(g) = d.preview.and_then(|id| d.guild(id)) {
        layer = iced::widget::stack![layer, details(app, g, d.joining == Some(g.id))].into();
    }
    Some(layer)
}

fn guild_icon<'a>(app: &'a App, g: &DiscoverGuild, size: f32) -> El<'a> {
    let p = app.theme.palette;
    match g
        .icon_url(&app.cfg.cdn_url)
        .and_then(|u| app.images.get(&u).cloned())
    {
        Some(h) => iced::widget::image(h)
            .width(size)
            .height(size)
            .border_radius(size * 0.3)
            .into(),
        None => square_initials(&p, &g.name, g.id.0, size),
    }
}

fn guild_card<'a>(app: &'a App, g: &'a DiscoverGuild, joining: bool) -> El<'a> {
    let p = app.theme.palette;
    let member = app.chat.guild(g.id).is_some();
    let banner = container(Space::new())
        .width(Length::Fill)
        .height(72)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(Color {
                a: 0.55,
                ..theme::default_avatar_color(g.id.0)
            })),
            border: iced::Border {
                radius: iced::border::Radius::default().top(8.0),
                ..Default::default()
            },
            ..Default::default()
        });
    let desc = g
        .description
        .as_deref()
        .map(|d| crate::net::model::truncate_chars(d, 110))
        .unwrap_or_default();
    let join_label = if member {
        "Open"
    } else if joining {
        "Joining\u{2026}"
    } else {
        "Join"
    };
    let join_btn = {
        let b = button(
            text(join_label)
                .size(13)
                .font(INTER_MEDIUM)
                .color(p.accent_text),
        )
        .padding(Padding::from([6, 16]))
        .style(theme::primary_button(p));
        if joining {
            b
        } else {
            b.on_press(ui(Ui::DiscoverJoin(g.id)))
        }
    };
    let body = column![
        row![
            text(g.name.clone())
                .size(16)
                .font(INTER_SEMIBOLD)
                .color(p.header_text),
            if g.verified || g.partnered {
                El::from(icon(Icon::Check, 14.0, p.success))
            } else {
                Space::new().into()
            },
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        text(desc).size(13).color(p.text_secondary),
        counts(app, g.online, g.members),
        row![
            button(
                text("Details")
                    .size(13)
                    .font(INTER_MEDIUM)
                    .color(p.text_normal)
            )
            .padding(Padding::from([6, 12]))
            .on_press(ui(Ui::DiscoverDetails(Some(g.id))))
            .style(theme::icon_button(p)),
            Space::new().width(Length::Fill),
            join_btn,
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(8)
    .padding(Padding::from([0, 14]).top(30).bottom(14));
    let icon_row = container(guild_icon(app, g, 48.0)).padding(Padding::ZERO.left(14).top(48));
    container(iced::widget::stack![column![banner, body], icon_row,])
        .width(280)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(p.bg_sidebar)),
            border: iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: p.border,
            },
            ..Default::default()
        })
        .into()
}

fn details<'a>(app: &'a App, g: &'a DiscoverGuild, joining: bool) -> El<'a> {
    let p = app.theme.palette;
    let member = app.chat.guild(g.id).is_some();
    let body = card(
        app,
        column![
            row![
                guild_icon(app, g, 64.0),
                column![
                    text(g.name.clone())
                        .size(22)
                        .font(INTER_SEMIBOLD)
                        .color(p.header_text),
                    counts(app, g.online, g.members),
                ]
                .spacing(6)
                .width(Length::Fill),
                close_button(app, Ui::DiscoverDetails(None)),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
            text(
                g.description
                    .clone()
                    .unwrap_or_else(|| "This community has no description yet.".into())
            )
            .size(15)
            .color(p.text_normal),
            row![
                Space::new().width(Length::Fill),
                secondary(app, "Close", Ui::DiscoverDetails(None)),
                primary(
                    app,
                    if member {
                        "Open Server"
                    } else if joining {
                        "Joining\u{2026}"
                    } else {
                        "Join Server"
                    },
                    (!joining).then_some(Ui::DiscoverJoin(g.id)),
                ),
            ]
            .spacing(8),
        ],
        460.0,
    );
    modal(body, Ui::DiscoverDetails(None))
}
