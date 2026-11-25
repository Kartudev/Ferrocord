//! The Roles page of Server Settings and the member dialogs (roles,
//! timeout, kick, ban). Text fields are bound to the forms in
//! `app::role_admin`; roles outside the user's reach show a lock.

use iced::widget::{Space, button, column, container, radio, row, text, text_input};
use iced::{Alignment, Length, Padding};

use super::icons::{Icon, icon};
use super::servers::{card, close_button, error_line, label, modal, primary, secondary};
use super::settings::{note, rule, title, toggle};
use super::theme::{self, INTER, INTER_MEDIUM, INTER_SEMIBOLD};
use super::{El, ui};
use crate::app::permissions::EDITABLE;
use crate::app::role_admin::{
    MemberDialog, ModForm, ModKind, ROLE_COLORS, RoleForm, RoleUi, sorted_roles,
};
use crate::app::{App, Message, Ui};
use crate::net::model::*;
use crate::net::rest::guild::{BAN_HISTORY, TIMEOUTS};

fn ru(r: RoleUi) -> Message {
    ui(Ui::Roles(r))
}

const LIST_W: f32 = 220.0;
const DOT: f32 = 12.0;
const SWATCH: f32 = 28.0;

fn dot<'a>(app: &App, color: u32) -> El<'a> {
    let c = if color == 0 {
        app.theme.palette.text_muted
    } else {
        theme::from_int(color)
    };
    container(Space::new())
        .width(DOT)
        .height(DOT)
        .style(theme::rounded_bg(c, DOT / 2.0))
        .into()
}

pub fn page(app: &App, g: GuildId) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let Some(guild) = app.chat.guild(g) else {
        return Space::new().into();
    };
    let selected = app.admin.roles.form.as_ref().map(|f| f.role);
    let mut list = column![].spacing(2).width(LIST_W);
    for r in sorted_roles(guild) {
        let editable = app.can_edit_role(g, r.id);
        let mut line = row![
            dot(app, r.color),
            text(super::ellipsize(
                &r.name,
                LIST_W - 70.0,
                t.metrics.font_m,
                false
            ))
            .size(t.metrics.font_m)
            .color(if selected == Some(r.id) {
                p.header_text
            } else {
                p.text_normal
            })
            .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if !editable {
            line = line.push(icon(Icon::Lock, 14.0, p.text_muted));
        }
        let b = button(line)
            .width(Length::Fill)
            .padding(Padding::from([6, 10]))
            .style(theme::list_row(p, selected == Some(r.id)));
        list = list.push(if editable {
            b.on_press(ru(RoleUi::Select(r.id)))
        } else {
            b
        });
    }
    let editor: El<'_> = match &app.admin.roles.form {
        Some(f) => editor(app, g, f),
        None => container(note(
            app,
            "Pick a role to edit its name, colour and permissions. Roles with a lock are at or \
             above your highest role.",
        ))
        .padding(Padding::from([8, 0]))
        .into(),
    };
    column![
        row![
            title(app, "Roles"),
            Space::new().width(Length::Fill),
            primary(
                app,
                if app.admin.roles.creating {
                    "Creating\u{2026}"
                } else {
                    "Create Role"
                },
                (!app.admin.roles.creating).then_some(Ui::Roles(RoleUi::Create)),
            ),
        ]
        .align_y(Alignment::Center),
        note(
            app,
            "Use roles to group members and give them permissions. Members get the highest \
             colour of their roles.",
        ),
        row![
            list,
            container(editor)
                .width(Length::Fill)
                .padding(Padding::ZERO.left(20)),
        ],
    ]
    .spacing(20)
    .into()
}

fn editor<'a>(app: &'a App, g: GuildId, f: &'a RoleForm) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let mut col = column![].spacing(16);
    if !f.everyone {
        col = col.push(
            column![
                label(app, "Role Name"),
                text_input("Role name", &f.name)
                    .on_input(|s| ru(RoleUi::Name(s)))
                    .on_submit(ru(RoleUi::Save))
                    .size(t.metrics.font_m)
                    .font(INTER)
                    .padding(10)
                    .style(theme::input(p, p.bg_app)),
            ]
            .spacing(8),
        );
        let swatch = |c: u32| -> El<'a> {
            let fill = if c == 0 {
                p.text_muted
            } else {
                theme::from_int(c)
            };
            button(Space::new())
                .width(SWATCH)
                .height(SWATCH)
                .on_press(ru(RoleUi::Color(c)))
                .style(theme::swatch(fill, p.header_text, f.color == c))
                .into()
        };
        let mut first = row![swatch(0)].spacing(6);
        let mut second = row![Space::new().width(SWATCH)].spacing(6);
        for (i, c) in ROLE_COLORS.iter().enumerate() {
            if i < ROLE_COLORS.len() / 2 {
                first = first.push(swatch(*c));
            } else {
                second = second.push(swatch(*c));
            }
        }
        col = col.push(column![label(app, "Role Colour"), first, second].spacing(8));
    } else {
        col = col.push(note(
            app,
            "@everyone: the default permissions of every member of the server.",
        ));
    }
    let grantable = app.grantable(g);
    for (section, perms) in EDITABLE {
        col = col.push(label(app, section));
        for (bit, name, hint) in perms {
            let on = f.permissions & bit != 0;
            if grantable & bit == *bit {
                col = col.push(toggle(
                    app,
                    name,
                    hint,
                    on,
                    ru(RoleUi::Permission(*bit, !on)),
                ));
            } else {
                col = col.push(
                    column![
                        row![
                            column![
                                text(*name).size(t.metrics.font_m).color(p.text_muted),
                                text(format!("{hint} You do not have this permission."))
                                    .size(t.metrics.font_s)
                                    .color(p.text_muted),
                            ]
                            .spacing(4)
                            .width(Length::Fill),
                            icon(
                                if on { Icon::Check } else { Icon::Lock },
                                16.0,
                                p.text_muted
                            ),
                        ]
                        .spacing(16)
                        .align_y(Alignment::Center),
                        rule(app),
                    ]
                    .spacing(16),
                );
            }
        }
    }
    col = col.push(error_line(app, &f.error));
    if !f.everyone {
        col = col.push(
            button(
                row![
                    icon(Icon::Trash, 16.0, p.danger),
                    text("Delete Role")
                        .size(t.metrics.font_s)
                        .font(INTER_MEDIUM)
                        .color(p.danger),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([8, 0]))
            .on_press(ru(RoleUi::AskDelete))
            .style(theme::link_button(p)),
        );
    }
    col.into()
}

/// "Careful — you have unsaved changes!" for the role being edited.
pub fn unsaved_bar(app: &App) -> Option<El<'_>> {
    let f = app.admin.roles.form.as_ref().filter(|f| f.dirty())?;
    let t = &app.theme;
    let p = t.palette;
    Some(
        container(
            container(
                row![
                    text("Careful \u{2014} you have unsaved changes!")
                        .size(t.metrics.font_m)
                        .color(p.text_normal)
                        .width(Length::Fill),
                    button(text("Reset").size(t.metrics.font_s).color(p.text_normal))
                        .padding(Padding::from([8, 12]))
                        .on_press(ru(RoleUi::Reset))
                        .style(theme::link_button(p)),
                    primary(
                        app,
                        if f.busy {
                            "Saving\u{2026}"
                        } else {
                            "Save Changes"
                        },
                        f.can_save().then_some(Ui::Roles(RoleUi::Save)),
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
        .into(),
    )
}

// -- dialogs -------------------------------------------------------------------

pub fn dialog(app: &App) -> Option<El<'_>> {
    let body = if app.admin.roles.confirm_delete {
        delete_role(app)?
    } else {
        match app.member_dialog.as_ref()? {
            MemberDialog::Roles {
                guild,
                user,
                pending,
                error,
            } => member_roles(app, *guild, *user, pending, error),
            MemberDialog::Moderate(f) => moderate(app, f),
        }
    };
    Some(modal(body, Ui::Roles(RoleUi::CloseDialog)))
}

fn header<'a>(app: &'a App, title: String) -> El<'a> {
    let t = &app.theme;
    row![
        text(title)
            .size(t.metrics.font_l)
            .font(INTER_SEMIBOLD)
            .color(t.palette.header_text)
            .width(Length::Fill),
        close_button(app, Ui::Roles(RoleUi::CloseDialog)),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn danger<'a>(app: &App, s: String, msg: Option<Message>) -> El<'a> {
    let p = app.theme.palette;
    let b = button(
        text(s)
            .size(app.theme.metrics.font_s)
            .font(INTER_MEDIUM)
            .color(p.accent_text),
    )
    .padding(Padding::from([10, 20]))
    .style(theme::danger_button(p));
    match msg {
        Some(m) => b.on_press(m),
        None => b,
    }
    .into()
}

fn delete_role(app: &App) -> Option<El<'_>> {
    let f = app.admin.roles.form.as_ref()?;
    let busy = app.admin.roles.deleting;
    Some(card(
        app,
        column![
            header(app, "Delete Role".into()),
            text(format!(
                "Are you sure you want to delete the role {}? This cannot be undone.",
                f.name
            ))
            .size(app.theme.metrics.font_m)
            .color(app.theme.palette.text_normal),
            row![
                Space::new().width(Length::Fill),
                secondary(app, "Cancel", Ui::Roles(RoleUi::CloseDialog)),
                danger(
                    app,
                    if busy {
                        "Deleting\u{2026}".into()
                    } else {
                        "Delete Role".into()
                    },
                    (!busy).then(|| ru(RoleUi::ConfirmDelete)),
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ],
        440.0,
    ))
}

fn user_name(app: &App, u: UserId) -> String {
    app.chat
        .users
        .get(&u)
        .map(|u| u.display_name().to_owned())
        .unwrap_or_else(|| "this member".into())
}

fn member_roles<'a>(
    app: &'a App,
    g: GuildId,
    user: UserId,
    pending: &'a [RoleId],
    error: &'a Option<String>,
) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let has = app
        .chat
        .member_roles
        .get(&(g, user))
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut list = column![].spacing(4);
    if let Some(guild) = app.chat.guild(g) {
        for r in sorted_roles(guild).into_iter().filter(|r| r.id != g) {
            let on = has.contains(&r.id);
            let editable = app.can_edit_role(g, r.id) && !pending.contains(&r.id);
            let mut line = row![
                dot(app, r.color),
                text(r.name.clone())
                    .size(t.metrics.font_m)
                    .color(if editable {
                        p.text_normal
                    } else {
                        p.text_muted
                    })
                    .width(Length::Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            line = line.push(icon(
                match (on, app.can_edit_role(g, r.id)) {
                    (_, false) => Icon::Lock,
                    (true, true) => Icon::Check,
                    (false, true) => Icon::Plus,
                },
                16.0,
                if on { p.accent } else { p.text_muted },
            ));
            let b = button(line)
                .width(Length::Fill)
                .padding(Padding::from([8, 10]))
                .style(theme::list_row(p, on));
            list = list.push(if editable {
                b.on_press(ru(RoleUi::MemberRole(r.id, !on)))
            } else {
                b
            });
        }
    }
    card(
        app,
        column![
            header(app, format!("Roles \u{2014} {}", user_name(app, user))),
            note(
                app,
                "Click a role to give it or take it away. Changes apply at once."
            ),
            list,
            error_line(app, error),
        ],
        420.0,
    )
}

fn moderate<'a>(app: &'a App, f: &'a ModForm) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let name = user_name(app, f.user);
    let (title, what, verb) = match f.kind {
        ModKind::Timeout => (
            format!("Timeout {name}"),
            "Timed out members cannot chat, react, join voice or video until it ends.".to_owned(),
            "Timeout",
        ),
        ModKind::Kick => (
            format!("Kick {name} from the server"),
            format!("{name} can rejoin with a new invite."),
            "Kick",
        ),
        ModKind::Ban => (
            format!("Ban {name}"),
            format!("{name} will be removed and cannot rejoin until unbanned."),
            "Ban",
        ),
    };
    let mut col = column![
        header(app, title),
        text(what).size(t.metrics.font_m).color(p.text_normal),
    ];
    let choice = |selected: bool, l: &'static str, msg: Message| -> El<'a> {
        container(
            radio(l, true, Some(selected), move |_| msg.clone())
                .size(18)
                .spacing(10)
                .text_size(t.metrics.font_m)
                .style(theme::radio(p)),
        )
        .width(Length::Fill)
        .padding(Padding::from([8, 12]))
        .style(theme::rounded_bg(
            if selected { p.bg_selected } else { p.bg_app },
            4.0,
        ))
        .into()
    };
    match f.kind {
        ModKind::Timeout => {
            let mut c = column![label(app, "Duration")].spacing(6);
            for (secs, l) in TIMEOUTS {
                c = c.push(choice(
                    f.timeout == secs,
                    l,
                    ru(RoleUi::TimeoutLength(secs)),
                ));
            }
            col = col.push(c);
        }
        ModKind::Ban => {
            let mut c = column![label(app, "Delete message history")].spacing(6);
            for (secs, l) in BAN_HISTORY {
                c = c.push(choice(
                    f.ban_history == secs,
                    l,
                    ru(RoleUi::BanHistory(secs)),
                ));
            }
            col = col.push(c);
        }
        ModKind::Kick => {}
    }
    col = col.push(
        column![
            label(app, "Reason (optional)"),
            text_input("Shown in the audit log", &f.reason)
                .on_input(|s| ru(RoleUi::Reason(s)))
                .size(t.metrics.font_m)
                .font(INTER)
                .padding(10)
                .style(theme::input(p, p.bg_app)),
        ]
        .spacing(8),
    );
    col = col.push(error_line(app, &f.error));
    let mut buttons = row![].spacing(8).align_y(Alignment::Center);
    if f.kind == ModKind::Timeout {
        buttons = buttons.push(
            button(
                text("Remove Timeout")
                    .size(t.metrics.font_s)
                    .color(p.text_secondary),
            )
            .padding(Padding::from([10, 0]))
            .on_press_maybe((!f.busy).then(|| ru(RoleUi::ConfirmModeration(true))))
            .style(theme::link_button(p)),
        );
    }
    buttons = buttons.push(Space::new().width(Length::Fill));
    buttons = buttons.push(secondary(app, "Cancel", Ui::Roles(RoleUi::CloseDialog)));
    buttons = buttons.push(danger(
        app,
        if f.busy {
            "Working\u{2026}".into()
        } else {
            verb.into()
        },
        (!f.busy).then(|| ru(RoleUi::ConfirmModeration(false))),
    ));
    col = col.push(buttons);
    card(app, col, 460.0)
}
