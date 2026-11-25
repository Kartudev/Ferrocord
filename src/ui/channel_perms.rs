//! The Channel Permissions dialog: overwrite targets on the left
//! (`@everyone`, roles, members, "Add a role"), the permissions of the
//! selected one on the right, each denied, left to the server (/) or
//! allowed. Bound to `app::channel_perms::PermsForm`.

use iced::widget::{Space, button, column, container, pick_list, row, scrollable, text};
use iced::{Alignment, Length, Padding};

use super::icons::{Icon, icon};
use super::servers::{card, close_button, error_line, label, modal, primary, secondary};
use super::settings::note;
use super::theme::{self, INTER_MEDIUM, INTER_SEMIBOLD};
use super::{El, ui};
use crate::app::channel_perms::{PermsForm, PermsUi, Tri};
use crate::app::permissions::channel_sections;
use crate::app::{App, Message, Ui};

fn pu(p: PermsUi) -> Message {
    ui(Ui::Perms(p))
}

const LIST_W: f32 = 200.0;
const TRI: f32 = 28.0;
const BODY_H: f32 = 400.0;

pub fn view(app: &App) -> Option<El<'_>> {
    let f = app.perms_dialog.as_ref()?;
    Some(modal(body(app, f), Ui::Perms(PermsUi::Close)))
}

fn body<'a>(app: &'a App, f: &'a PermsForm) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let name = app
        .chat
        .channels
        .get(&f.channel)
        .and_then(|c| c.name.clone())
        .unwrap_or_default();
    let prefix = if f.kind == crate::net::model::ChannelKind::Category {
        ""
    } else {
        "#"
    };
    // Targets.
    let mut list = column![label(app, "Roles / Members")]
        .spacing(2)
        .width(LIST_W);
    for (id, tname, color, member) in app.perms_targets(f) {
        let selected = id == f.selected;
        let mark: El<'_> = if member {
            icon(Icon::UserRound, 14.0, p.text_muted)
        } else {
            container(Space::new())
                .width(10)
                .height(10)
                .style(theme::rounded_bg(
                    if color == 0 {
                        p.text_muted
                    } else {
                        theme::from_int(color)
                    },
                    5.0,
                ))
                .into()
        };
        list = list.push(
            button(
                row![
                    mark,
                    text(super::ellipsize(
                        &tname,
                        LIST_W - 50.0,
                        t.metrics.font_s,
                        false
                    ))
                    .size(t.metrics.font_s)
                    .color(if selected {
                        p.header_text
                    } else {
                        p.text_normal
                    }),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(Padding::from([6, 8]))
            .on_press(pu(PermsUi::Select(id)))
            .style(theme::list_row(p, selected)),
        );
    }
    let addable = app.perms_addable(f);
    if !addable.is_empty() {
        list = list.push(Space::new().height(8));
        list = list.push(
            pick_list(
                addable,
                None::<crate::app::channel_perms::RoleChoice>,
                |r| pu(PermsUi::AddRole(r)),
            )
            .placeholder("Add a role\u{2026}")
            .text_size(t.metrics.font_s)
            .width(Length::Fill),
        );
    }
    // Permissions of the selected target.
    let grantable = app.channel_grantable(f.channel);
    let mut perms = column![].spacing(12).padding(Padding::ZERO.right(12));
    let everyone = f.selected == f.guild;
    perms = perms.push(
        row![
            text(
                app.perms_targets(f)
                    .into_iter()
                    .find(|x| x.0 == f.selected)
                    .map(|x| x.1)
                    .unwrap_or_default()
            )
            .size(t.metrics.font_m)
            .font(INTER_SEMIBOLD)
            .color(p.header_text)
            .width(Length::Fill),
            button(
                text(if everyone {
                    "Clear All"
                } else {
                    "Remove Override"
                })
                .size(t.metrics.font_s)
                .color(p.danger),
            )
            .padding(0)
            .on_press_maybe((!f.busy).then(|| pu(PermsUi::Remove)))
            .style(theme::link_button(p)),
        ]
        .align_y(Alignment::Center),
    );
    for (section, list_) in channel_sections(f.kind) {
        perms = perms.push(label(app, section));
        for (bit, pname, hint) in list_ {
            let can = grantable & bit == *bit && !f.busy;
            let state = f.tri(*bit);
            let tri = |ic: El<'a>, which: Tri, active: iced::Color| -> El<'a> {
                button(container(ic).center(Length::Fill))
                    .width(TRI)
                    .height(TRI)
                    .padding(0)
                    .on_press_maybe(can.then(|| pu(PermsUi::Set(*bit, which))))
                    .style(theme::tri_button(p, (state == which).then_some(active)))
                    .into()
            };
            let fg = |which: Tri| {
                if state == which {
                    p.accent_text
                } else {
                    p.text_muted
                }
            };
            perms = perms.push(
                row![
                    column![
                        text(*pname).size(t.metrics.font_m).color(if can {
                            p.text_normal
                        } else {
                            p.text_muted
                        }),
                        text(if grantable & bit == *bit {
                            (*hint).to_owned()
                        } else {
                            format!("{hint} You do not have this permission here.")
                        })
                        .size(t.metrics.font_xs)
                        .color(p.text_muted),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    row![
                        tri(icon(Icon::X, 14.0, fg(Tri::Deny)), Tri::Deny, p.danger),
                        tri(
                            text("/")
                                .size(t.metrics.font_m)
                                .font(INTER_MEDIUM)
                                .color(fg(Tri::Inherit))
                                .into(),
                            Tri::Inherit,
                            p.bg_selected,
                        ),
                        tri(
                            icon(Icon::Check, 14.0, fg(Tri::Allow)),
                            Tri::Allow,
                            p.success
                        ),
                    ]
                    .spacing(2),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            );
        }
    }
    let mut buttons = row![].spacing(8).align_y(Alignment::Center);
    if f.dirty() && !f.busy {
        buttons = buttons.push(
            button(text("Reset").size(t.metrics.font_s).color(p.text_secondary))
                .padding(Padding::from([10, 0]))
                .on_press(pu(PermsUi::Reset))
                .style(theme::link_button(p)),
        );
    }
    buttons = buttons.push(Space::new().width(Length::Fill));
    buttons = buttons.push(secondary(app, "Cancel", Ui::Perms(PermsUi::Close)));
    buttons = buttons.push(primary(
        app,
        if f.busy {
            "Saving\u{2026}"
        } else {
            "Save Changes"
        },
        (f.dirty() && !f.busy).then_some(Ui::Perms(PermsUi::Save)),
    ));
    card(
        app,
        column![
            row![
                text(format!("Permissions \u{2014} {prefix}{name}"))
                    .size(t.metrics.font_l)
                    .font(INTER_SEMIBOLD)
                    .color(p.header_text)
                    .width(Length::Fill),
                close_button(app, Ui::Perms(PermsUi::Close)),
            ]
            .align_y(Alignment::Center),
            note(
                app,
                "Overrides change what a role or member can do here: deny, keep the server's \
                 setting (/) or allow. A member's own override wins, then roles, then \
                 @everyone.",
            ),
            row![
                list,
                scrollable(perms)
                    .height(BODY_H)
                    .width(Length::Fill)
                    .style(theme::thin_scrollbar(p, false)),
            ]
            .spacing(20),
            error_line(app, &f.error),
            buttons,
        ],
        760.0,
    )
}
