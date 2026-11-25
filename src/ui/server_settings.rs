//! Server administration views: the Server Settings screen (the official
//! full-window layout: categories on the left, the page on the right) and
//! the Invite People / Leave Server / Delete Server dialogs.
//!
//! Like the User Settings, text fields are bound to the form filled once
//! when the screen opens, and no image handle is created here (the
//! current icon reuses the rail's handle; a newly picked file shows by
//! name).

use iced::widget::{Space, button, column, container, opaque, row, scrollable, text, text_input};
use iced::{Alignment, Color, Length, Padding};

use super::icons::{Icon, icon};
use super::servers::{card, close_button, error_line, heading, label, modal, primary, secondary};
use super::settings::{note, rule, title};
use super::theme::{self, INTER, INTER_MEDIUM, INTER_SEMIBOLD};
use super::{El, ui};
use crate::app::guild_admin::{AdminDialog, AdminUi, DeleteForm, IconChange, SettingsForm};
use crate::app::role_admin::{RoleUi, SettingsPage};
use crate::app::{App, Message, Ui};
use crate::net::mfa::MfaMethod;
use crate::net::rest::guild::invite_url;

fn au(a: AdminUi) -> Message {
    ui(Ui::Admin(a))
}

pub fn name_input_id() -> iced::widget::Id {
    iced::widget::Id::new("server-settings-name")
}

pub fn delete_input_id() -> iced::widget::Id {
    iced::widget::Id::new("server-delete-name")
}

pub fn mfa_input_id() -> iced::widget::Id {
    iced::widget::Id::new("server-delete-mfa")
}

fn content_id() -> iced::widget::Id {
    iced::widget::Id::new("server-settings-content")
}

// -- Server Settings screen ----------------------------------------------------

pub fn screen(app: &App) -> Option<El<'_>> {
    let f = app.admin.settings.as_ref()?;
    let p = app.theme.palette;
    let sidebar = container(
        container(sidebar(app, f))
            .width(Length::Fill)
            .align_right(Length::Fill)
            .padding(Padding::from([60, 6]).left(20)),
    )
    .width(Length::FillPortion(1))
    .height(Length::Fill)
    .style(theme::bg(p.bg_sidebar));
    let page = container(
        row![
            container(match app.admin.page {
                SettingsPage::Overview => overview(app, f),
                SettingsPage::Roles => super::roles::page(app, f.guild),
                SettingsPage::Bans => super::roles::bans_page(app),
            })
            .max_width(theme::SETTINGS_CONTENT_MAX_W)
            .width(Length::Fill),
            esc_button(app),
        ]
        .spacing(20),
    )
    .padding(Padding::from([60, 40]).bottom(80));
    let mut body = column![
        scrollable(page)
            .id(content_id())
            .style(theme::thin_scrollbar(p, false))
            .height(Length::Fill),
    ];
    match app.admin.page {
        SettingsPage::Overview if f.dirty() => body = body.push(unsaved_bar(app, f)),
        SettingsPage::Roles => {
            if let Some(bar) = super::roles::unsaved_bar(app) {
                body = body.push(bar);
            }
        }
        _ => {}
    }
    let content = container(body)
        .width(Length::FillPortion(2))
        .height(Length::Fill)
        .style(theme::bg(p.bg_panel));
    Some(opaque(
        container(row![sidebar, content])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::bg(p.bg_panel)),
    ))
}

fn sidebar<'a>(app: &'a App, f: &'a SettingsForm) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let name = app
        .chat
        .guild(f.guild)
        .map(|g| g.name.to_uppercase())
        .unwrap_or_default();
    let mut col = column![
        container(
            text(super::ellipsize(
                &name,
                theme::SETTINGS_SIDEBAR_W - 20.0,
                t.metrics.font_xs,
                true
            ))
            .size(t.metrics.font_xs)
            .font(INTER_SEMIBOLD)
            .color(p.text_muted),
        )
        .padding(Padding::from([6, 10])),
    ]
    .spacing(2)
    .width(theme::SETTINGS_SIDEBAR_W);
    let entry = |label: &'a str, page: SettingsPage| {
        let selected = app.admin.page == page;
        button(text(label).size(t.metrics.font_m).color(if selected {
            p.header_text
        } else {
            p.text_secondary
        }))
        .width(Length::Fill)
        .padding(Padding::from([6, 10]))
        .on_press(ui(Ui::Roles(RoleUi::Page(page))))
        .style(theme::list_row(p, selected))
    };
    if app.can_manage_guild(f.guild) {
        col = col.push(entry("Overview", SettingsPage::Overview));
    }
    if app.can_manage_roles(f.guild) {
        col = col.push(entry("Roles", SettingsPage::Roles));
    }
    if app.can_ban(f.guild) {
        col = col.push(entry("Bans", SettingsPage::Bans));
    }
    if app.chat.owns_guild(f.guild) {
        col = col.push(Space::new().height(16));
        col = col.push(
            button(
                row![
                    text("Delete Server")
                        .size(t.metrics.font_m)
                        .color(p.danger)
                        .width(Length::Fill),
                    icon(Icon::Trash, 16.0, p.danger),
                ]
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(Padding::from([6, 10]))
            .on_press(au(AdminUi::AskDelete))
            .style(theme::list_row(p, false)),
        );
    }
    col.into()
}

fn esc_button(app: &App) -> El<'_> {
    let p = app.theme.palette;
    column![
        button(
            container(icon(Icon::X, 18.0, p.text_secondary))
                .center_x(36)
                .center_y(36)
        )
        .padding(0)
        .width(36)
        .height(36)
        .on_press(au(AdminUi::CloseSettings))
        .style(theme::swatch(Color::TRANSPARENT, p.text_secondary, false)),
        text("ESC")
            .size(app.theme.metrics.font_xs)
            .font(INTER_SEMIBOLD)
            .color(p.text_muted),
    ]
    .spacing(6)
    .align_x(Alignment::Center)
    .into()
}

fn overview<'a>(app: &'a App, f: &'a SettingsForm) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let guild = app.chat.guild(f.guild);
    const ICON: f32 = 100.0;
    // Current icon (or its acronym), unless a change is pending.
    let face: El<'a> = match (&f.icon, guild.and_then(|g| app.guild_icon(g))) {
        (IconChange::Keep, Some(h)) => iced::widget::image(h.clone())
            .width(ICON)
            .height(ICON)
            .border_radius(ICON / 2.0)
            .into(),
        (IconChange::Set { .. }, _) => container(icon(Icon::Image, 36.0, p.text_secondary))
            .center(ICON)
            .style(theme::rounded_bg(p.bg_app, ICON / 2.0))
            .into(),
        _ => container(
            text(guild.map(|g| g.acronym()).unwrap_or_default())
                .size(t.metrics.font_xl)
                .font(INTER_MEDIUM)
                .color(p.header_text),
        )
        .center(ICON)
        .style(theme::rounded_bg(p.bg_app, ICON / 2.0))
        .into(),
    };
    let has_icon = match &f.icon {
        IconChange::Keep => guild.is_some_and(|g| g.icon.is_some()),
        IconChange::Remove => false,
        IconChange::Set { .. } => true,
    };
    let picked = match &f.icon {
        IconChange::Set { path, .. } => path.file_name().map(|n| n.to_string_lossy().into_owned()),
        _ => None,
    };
    let mut icon_side = column![
        note(
            app,
            "We recommend an image of at least 512x512 for the server."
        ),
        row![
            button(
                text("Upload Image")
                    .size(t.metrics.font_s)
                    .font(INTER_MEDIUM)
                    .color(p.text_normal)
            )
            .padding(Padding::from([8, 16]))
            .on_press(au(AdminUi::PickIcon))
            .style(theme::secondary_button(p)),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(12);
    if has_icon {
        icon_side = icon_side.push(
            button(
                text("Remove")
                    .size(t.metrics.font_s)
                    .color(p.text_secondary),
            )
            .padding(0)
            .on_press(au(AdminUi::RemoveIcon))
            .style(theme::link_button(p)),
        );
    }
    if let Some(n) = picked {
        icon_side = icon_side.push(note(app, format!("New icon: {n}")));
    }
    let name = text_input("Server name", &f.name)
        .id(name_input_id())
        .on_input(|s| au(AdminUi::NameChanged(s)))
        .on_submit(au(AdminUi::Save))
        .size(t.metrics.font_m)
        .font(INTER)
        .padding(10)
        .style(theme::input(p, p.bg_app));
    column![
        title(app, "Server Overview"),
        row![face, icon_side.width(Length::Fill)]
            .spacing(24)
            .align_y(Alignment::Center),
        column![label(app, "Server Name"), name].spacing(8),
        error_line(app, &f.error),
        rule(app),
        note(
            app,
            "Channels are managed from the channel list (right-click or the + of a \
             category) and members from the member list (right-click).",
        ),
    ]
    .spacing(20)
    .into()
}

/// "Careful — you have unsaved changes!" bar at the bottom of the page.
fn unsaved_bar<'a>(app: &'a App, f: &'a SettingsForm) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    container(
        container(
            row![
                text("Careful \u{2014} you have unsaved changes!")
                    .size(t.metrics.font_m)
                    .color(p.text_normal)
                    .width(Length::Fill),
                button(text("Reset").size(t.metrics.font_s).color(p.text_normal))
                    .padding(Padding::from([8, 12]))
                    .on_press(au(AdminUi::Reset))
                    .style(theme::link_button(p)),
                primary(
                    app,
                    if f.busy {
                        "Saving\u{2026}"
                    } else {
                        "Save Changes"
                    },
                    f.can_save().then_some(Ui::Admin(AdminUi::Save)),
                ),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .max_width(theme::SETTINGS_CONTENT_MAX_W)
        .padding(Padding::from([10, 16]))
        .style(theme::rounded_bg(p.bg_app, 6.0)),
    )
    .padding(Padding::from([0, 40]).bottom(20))
    .into()
}

// -- dialogs -------------------------------------------------------------------

pub fn dialog(app: &App) -> Option<El<'_>> {
    let d = app.admin.dialog.as_ref()?;
    let body = match d {
        AdminDialog::Invite(i) => invite(app, i),
        AdminDialog::Leave(c) => leave(app, c),
        AdminDialog::Delete(d) => delete(app, d),
    };
    Some(modal(body, Ui::Admin(AdminUi::CloseDialog)))
}

fn guild_name(app: &App, g: crate::net::model::GuildId) -> String {
    app.chat
        .guild(g)
        .map(|g| g.name.clone())
        .unwrap_or_default()
}

fn invite<'a>(app: &'a App, i: &'a crate::app::guild_admin::InviteState) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let channel = app
        .chat
        .channels
        .get(&i.channel)
        .and_then(|c| c.name.clone())
        .unwrap_or_default();
    let link: El<'a> = match (&i.code, &i.error) {
        (Some(code), _) => row![
            text(invite_url(code))
                .size(t.metrics.font_m)
                .color(p.text_normal)
                .width(Length::Fill),
            button(
                text(if i.copied { "Copied" } else { "Copy" })
                    .size(t.metrics.font_s)
                    .font(INTER_MEDIUM)
                    .color(p.accent_text)
            )
            .padding(Padding::from([8, 16]))
            .on_press(au(AdminUi::CopyInvite))
            .style(theme::primary_button(p)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
        (None, Some(_)) => Space::new().into(),
        (None, None) => text("Creating an invite\u{2026}")
            .size(t.metrics.font_m)
            .color(p.text_muted)
            .into(),
    };
    card(
        app,
        column![
            row![
                text(format!("Invite friends to {}", guild_name(app, i.guild)))
                    .size(t.metrics.font_l)
                    .font(INTER_SEMIBOLD)
                    .color(p.header_text)
                    .width(Length::Fill),
                close_button(app, Ui::Admin(AdminUi::CloseDialog)),
            ]
            .align_y(Alignment::Center),
            row![
                icon(Icon::Hash, 16.0, p.text_muted),
                text(channel).size(t.metrics.font_s).color(p.text_secondary),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
            label(app, "Send a server invite link to a friend"),
            container(link)
                .width(Length::Fill)
                .padding(Padding::from([6, 6]).left(12))
                .style(theme::rounded_bg(p.bg_app, 4.0)),
            error_line(app, &i.error),
            note(app, "Your invite link expires in 7 days."),
        ],
        460.0,
    )
}

fn leave<'a>(app: &'a App, c: &'a crate::app::guild_admin::Confirm) -> El<'a> {
    let p = app.theme.palette;
    let name = guild_name(app, c.guild);
    card(
        app,
        column![
            heading(app, &format!("Leave '{name}'"), ""),
            text(format!(
                "Are you sure you want to leave {name}? You won't be able to rejoin this \
                 server unless you are re-invited."
            ))
            .size(app.theme.metrics.font_m)
            .color(p.text_normal),
            error_line(app, &c.error),
            row![
                Space::new().width(Length::Fill),
                secondary(app, "Cancel", Ui::Admin(AdminUi::CloseDialog)),
                danger(
                    app,
                    if c.busy {
                        "Leaving\u{2026}"
                    } else {
                        "Leave Server"
                    },
                    (!c.busy).then_some(Ui::Admin(AdminUi::ConfirmLeave)),
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ],
        440.0,
    )
}

fn delete<'a>(app: &'a App, d: &'a DeleteForm) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let name = guild_name(app, d.guild);
    let input = text_input("", &d.typed)
        .id(delete_input_id())
        .on_input(|s| au(AdminUi::DeleteTyped(s)))
        .on_submit(au(AdminUi::ConfirmDelete))
        .size(t.metrics.font_m)
        .font(INTER)
        .padding(10)
        .style(theme::input(p, p.bg_app));
    let ok = d.matches(&name) && !d.busy;
    let mfa: El<'a> = match &d.mfa {
        None => Space::new().into(),
        Some(m) => {
            let backup = m.method == MfaMethod::Backup;
            let other = if backup {
                MfaMethod::Totp
            } else {
                MfaMethod::Backup
            };
            let mut c = column![
                label(
                    app,
                    if backup {
                        "Backup code"
                    } else {
                        "Two-factor code"
                    }
                ),
                text_input(
                    if backup {
                        "8-character code"
                    } else {
                        "6-digit code"
                    },
                    &m.code
                )
                .id(mfa_input_id())
                .on_input(|s| au(AdminUi::MfaCode(s)))
                .on_submit(au(AdminUi::ConfirmDelete))
                .size(t.metrics.font_m)
                .font(INTER)
                .padding(10)
                .style(theme::input(p, p.bg_app)),
            ]
            .spacing(8);
            if m.challenge.allows(other) {
                c = c.push(
                    button(
                        text(if backup {
                            "Use your authenticator app instead"
                        } else {
                            "Use a backup code instead"
                        })
                        .size(t.metrics.font_s)
                        .color(p.link),
                    )
                    .padding(0)
                    .on_press(au(AdminUi::MfaUse(other)))
                    .style(theme::link_button(p)),
                );
            }
            column![
                note(
                    app,
                    "Your account has two-factor authentication on. Enter a code to confirm."
                ),
                c
            ]
            .spacing(12)
            .into()
        }
    };
    card(
        app,
        column![
            heading(app, &format!("Delete '{name}'"), ""),
            container(
                text(format!(
                    "Are you sure you want to delete {name}? This action cannot be undone."
                ))
                .size(t.metrics.font_m)
                .color(p.text_normal),
            )
            .width(Length::Fill)
            .padding(12)
            .style(theme::rounded_bg(
                Color {
                    a: 0.15,
                    ..p.danger
                },
                4.0
            )),
            column![label(app, "Enter server name"), input].spacing(8),
            mfa,
            error_line(app, &d.error),
            row![
                Space::new().width(Length::Fill),
                secondary(app, "Cancel", Ui::Admin(AdminUi::CloseDialog)),
                danger(
                    app,
                    if d.busy {
                        "Deleting\u{2026}"
                    } else {
                        "Delete Server"
                    },
                    ok.then_some(Ui::Admin(AdminUi::ConfirmDelete)),
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ],
        440.0,
    )
}

fn danger<'a>(app: &App, s: &str, msg: Option<Ui>) -> El<'a> {
    let p = app.theme.palette;
    let b = button(
        text(s.to_owned())
            .size(app.theme.metrics.font_s)
            .font(INTER_MEDIUM)
            .color(p.accent_text),
    )
    .padding(Padding::from([10, 20]))
    .style(theme::danger_button(p));
    match msg {
        Some(m) => b.on_press(ui(m)),
        None => b,
    }
    .into()
}
