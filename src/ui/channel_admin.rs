//! Channel dialogs: Create Channel / Create Category, Edit Channel and
//! Delete Channel. Text fields are bound to the form, filled once when
//! the dialog opens.

use iced::widget::{Space, button, column, container, pick_list, radio, row, text, text_input};
use iced::{Alignment, Length, Padding};

use super::icons::{Icon, icon};
use super::servers::{card, close_button, error_line, label, modal, primary, secondary};
use super::settings::{note, toggle};
use super::theme::{self, INTER, INTER_MEDIUM, INTER_SEMIBOLD};
use super::{El, ui};
use crate::app::channel_admin::{
    ChannelDialog, ChannelUi, CreateForm, DeleteConfirm, EditForm, parent_choices,
};
use crate::app::{App, Message, Ui};
use crate::net::model::*;
use crate::net::rest::guild::NewKind;

fn cu(c: ChannelUi) -> Message {
    ui(Ui::Channel(c))
}

pub fn create_name_id() -> iced::widget::Id {
    iced::widget::Id::new("create-channel-name")
}

pub fn view(app: &App) -> Option<El<'_>> {
    let d = app.channel_dialog.as_ref()?;
    let body = match d {
        ChannelDialog::Create(f) => create(app, f),
        ChannelDialog::Edit(f) => edit(app, f),
        ChannelDialog::Delete(d) => delete(app, d),
    };
    Some(modal(body, Ui::Channel(ChannelUi::Close)))
}

fn title_row<'a>(app: &'a App, title: String, sub: Option<String>) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let mut col = column![
        text(title)
            .size(t.metrics.font_l)
            .font(INTER_SEMIBOLD)
            .color(p.header_text)
    ]
    .spacing(4)
    .width(Length::Fill);
    if let Some(s) = sub {
        col = col.push(text(s).size(t.metrics.font_s).color(p.text_secondary));
    }
    row![col, close_button(app, Ui::Channel(ChannelUi::Close))]
        .align_y(Alignment::Start)
        .into()
}

fn name_input<'a>(
    app: &'a App,
    kind_icon: Icon,
    value: &'a str,
    on_input: fn(String) -> ChannelUi,
    submit: ChannelUi,
) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    row![
        icon(kind_icon, 18.0, p.text_muted),
        text_input("new-channel", value)
            .id(create_name_id())
            .on_input(move |s| cu(on_input(s)))
            .on_submit(cu(submit))
            .size(t.metrics.font_m)
            .font(INTER)
            .padding(10)
            .style(theme::input(p, p.bg_app)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn kind_icon(kind: ChannelKind) -> Icon {
    match kind {
        ChannelKind::Voice | ChannelKind::Stage => Icon::Volume2,
        ChannelKind::Announcement => Icon::Megaphone,
        ChannelKind::Category => Icon::Folder,
        _ => Icon::Hash,
    }
}

fn create<'a>(app: &'a App, f: &'a CreateForm) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let category = f.kind == NewKind::Category;
    let in_cat = f
        .parent
        .and_then(|c| app.chat.channels.get(&c))
        .and_then(|c| c.name.clone());
    let title = if category {
        "Create Category".to_owned()
    } else {
        "Create Channel".to_owned()
    };
    let sub = (!category).then(|| match in_cat {
        Some(n) => format!("in {n}"),
        None => "Not in a category".to_owned(),
    });
    let mut col = column![title_row(app, title, sub)];
    if !category {
        let choice = |k: NewKind, l: &'static str, i: Icon| -> El<'a> {
            container(
                row![
                    icon(i, 20.0, p.text_secondary),
                    radio(l, k, Some(f.kind), |k| cu(ChannelUi::CreateKind(k)))
                        .size(18)
                        .spacing(10)
                        .text_size(t.metrics.font_m)
                        .style(theme::radio(p)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(Padding::from([10, 12]))
            .style(theme::rounded_bg(
                if f.kind == k { p.bg_selected } else { p.bg_app },
                4.0,
            ))
            .into()
        };
        col = col.push(
            column![
                label(app, "Channel Type"),
                choice(NewKind::Text, "Text", Icon::Hash),
                choice(NewKind::Voice, "Voice", Icon::Volume2),
            ]
            .spacing(8),
        );
        let choices = parent_choices(&app.chat, f.guild);
        let selected = choices.iter().find(|c| c.id == f.parent).cloned();
        col = col.push(
            column![
                label(app, "Category"),
                pick_list(choices, selected, |c| cu(ChannelUi::CreateParent(c)))
                    .text_size(t.metrics.font_s)
                    .width(Length::Fill),
            ]
            .spacing(8),
        );
    }
    let kind = match f.kind {
        NewKind::Text => ChannelKind::Text,
        NewKind::Voice => ChannelKind::Voice,
        NewKind::Category => ChannelKind::Category,
    };
    col = col.push(
        column![
            label(
                app,
                if category {
                    "Category Name"
                } else {
                    "Channel Name"
                }
            ),
            name_input(
                app,
                kind_icon(kind),
                &f.name,
                ChannelUi::CreateName,
                ChannelUi::SubmitCreate
            ),
        ]
        .spacing(8),
    );
    col = col.push(error_line(app, &f.error));
    col = col.push(
        row![
            Space::new().width(Length::Fill),
            secondary(app, "Cancel", Ui::Channel(ChannelUi::Close)),
            primary(
                app,
                if f.busy {
                    "Creating\u{2026}"
                } else if category {
                    "Create Category"
                } else {
                    "Create Channel"
                },
                f.can_submit()
                    .then_some(Ui::Channel(ChannelUi::SubmitCreate)),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    card(app, col, 460.0)
}

fn edit<'a>(app: &'a App, f: &'a EditForm) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let what = if f.is_category() {
        "Category"
    } else {
        "Channel"
    };
    let guild = app.chat.channels.get(&f.channel).and_then(|c| c.guild_id);
    let mut col = column![
        title_row(app, format!("Edit {what}"), None),
        column![
            label(app, &format!("{what} Name")),
            name_input(
                app,
                kind_icon(f.kind),
                &f.name,
                ChannelUi::EditName,
                ChannelUi::SubmitEdit
            ),
        ]
        .spacing(8),
    ];
    if f.has_topic() {
        col = col.push(
            column![
                label(app, "Channel Topic"),
                text_input("Let everyone know how to use this channel!", &f.topic)
                    .on_input(|s| cu(ChannelUi::EditTopic(s)))
                    .on_submit(cu(ChannelUi::SubmitEdit))
                    .size(t.metrics.font_m)
                    .font(INTER)
                    .padding(10)
                    .style(theme::input(p, p.bg_app)),
            ]
            .spacing(8),
        );
    }
    if !f.is_category()
        && let Some(g) = guild
    {
        let choices = parent_choices(&app.chat, g);
        let selected = choices.iter().find(|c| c.id == f.parent).cloned();
        col = col.push(
            column![
                label(app, "Category"),
                pick_list(choices, selected, |c| cu(ChannelUi::EditParent(c)))
                    .text_size(t.metrics.font_s)
                    .width(Length::Fill),
            ]
            .spacing(8),
        );
    }
    if f.has_topic() {
        col = col.push(toggle(
            app,
            "Age-Restricted Channel",
            "Users will need to confirm they are of legal age to view the content in this channel.",
            f.nsfw,
            cu(ChannelUi::EditNsfw(!f.nsfw)),
        ));
    }
    col = col.push(error_line(app, &f.error));
    col = col.push(
        row![
            button(
                row![
                    icon(Icon::Trash, 16.0, p.danger),
                    text(format!("Delete {what}"))
                        .size(t.metrics.font_s)
                        .font(INTER_MEDIUM)
                        .color(p.danger),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([10, 8]))
            .on_press(cu(ChannelUi::AskDelete(f.channel)))
            .style(theme::link_button(p)),
            Space::new().width(Length::Fill),
            secondary(app, "Cancel", Ui::Channel(ChannelUi::Close)),
            primary(
                app,
                if f.busy {
                    "Saving\u{2026}"
                } else {
                    "Save Changes"
                },
                f.can_submit().then_some(Ui::Channel(ChannelUi::SubmitEdit)),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    card(app, col, 480.0)
}

fn delete<'a>(app: &'a App, d: &'a DeleteConfirm) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let ch = app.chat.channels.get(&d.channel);
    let category = ch.is_some_and(|c| c.kind == ChannelKind::Category);
    let name = ch.and_then(|c| c.name.clone()).unwrap_or_default();
    let shown = if category || ch.is_some_and(|c| !c.kind.is_textual()) {
        name
    } else {
        format!("#{name}")
    };
    let what = if category { "Category" } else { "Channel" };
    let mut body = format!("Are you sure you want to delete {shown}? This cannot be undone.");
    if category {
        body.push_str(" Its channels are kept, outside any category.");
    }
    card(
        app,
        column![
            title_row(app, format!("Delete {what}"), None),
            text(body).size(t.metrics.font_m).color(p.text_normal),
            note(app, "Messages in a deleted channel are gone for everyone."),
            error_line(app, &d.error),
            row![
                Space::new().width(Length::Fill),
                secondary(app, "Cancel", Ui::Channel(ChannelUi::Close)),
                {
                    let b = button(
                        text(if d.busy {
                            "Deleting\u{2026}".to_owned()
                        } else {
                            format!("Delete {what}")
                        })
                        .size(t.metrics.font_s)
                        .font(INTER_MEDIUM)
                        .color(p.accent_text),
                    )
                    .padding(Padding::from([10, 20]))
                    .style(theme::danger_button(p));
                    if d.busy {
                        b
                    } else {
                        b.on_press(cu(ChannelUi::ConfirmDelete))
                    }
                },
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ],
        440.0,
    )
}
