//! User Settings: the official full-window layout, a sidebar of categories
//! on the left and the page on the right, Esc (or the round X) to close.
//!
//! Flicker rules (see `LESSONS.md`): both scrollables have fixed ids, text
//! fields are bound to `SettingsState::forms` (filled once when the screen
//! opens, never rebuilt from the account each frame), and no image handle
//! is created here.

use iced::widget::{
    Space, button, column, container, opaque, pick_list, radio, row, scrollable, slider, stack,
    text, text_editor, text_input, toggler,
};
use iced::{Alignment, Color, Length, Padding};

use super::icons::{Icon, icon};
use super::theme::{self, INTER_SEMIBOLD};
use super::{El, ui};
use crate::app::settings::{
    self as st, CHAT_FONT_SIZES, Category, Expiry, LOCALES, LocalChange, SettingsUi, Spoilers,
    StickerAnim, ZOOM_LEVELS,
};
use crate::app::voice::VoiceUi;
use crate::app::window::TrayHost;
use crate::app::{App, Message, Ui};
use crate::config::APP_NAME;
use crate::net::model::Presence;
use crate::net::user_settings::{self as us, Synced, Theme, Value};
use crate::tray::autostart;

fn su(s: SettingsUi) -> Message {
    ui(Ui::Settings(s))
}

pub fn view(app: &App) -> El<'_> {
    let p = app.theme.palette;
    let sidebar = container(
        scrollable(
            container(sidebar(app))
                .width(Length::Fill)
                .align_right(Length::Fill)
                .padding(Padding::from([60, 6]).left(20)),
        )
        .id(st::sidebar_id())
        .style(theme::thin_scrollbar(p, false))
        .height(Length::Fill),
    )
    .width(Length::FillPortion(1))
    .height(Length::Fill)
    .style(theme::bg(p.bg_sidebar));
    let page = container(
        row![
            container(page(app))
                .max_width(theme::SETTINGS_CONTENT_MAX_W)
                .width(Length::Fill),
            close_button(app),
        ]
        .spacing(20),
    )
    .padding(Padding::from([60, 40]).bottom(80));
    let content = container(
        scrollable(page)
            .id(st::content_id())
            .style(theme::thin_scrollbar(p, false))
            .height(Length::Fill),
    )
    .width(Length::FillPortion(2))
    .height(Length::Fill)
    .style(theme::bg(p.bg_panel));
    opaque(
        container(row![sidebar, content])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::bg(p.bg_panel)),
    )
}

fn close_button(app: &App) -> El<'_> {
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
        .on_press(ui(Ui::CloseSettings))
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

fn sidebar(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let current = app.settings.category();
    let mut col = column![].spacing(2).width(theme::SETTINGS_SIDEBAR_W);
    for c in Category::ALL {
        if let Some(h) = c.heading() {
            if c != Category::MyAccount {
                col = col.push(divider(app));
            }
            col = col.push(
                container(
                    text(h)
                        .size(t.metrics.font_xs)
                        .font(INTER_SEMIBOLD)
                        .color(p.text_muted),
                )
                .padding(Padding::from([6, 10])),
            );
        }
        let selected = c == current;
        col = col.push(
            button(text(c.label()).size(t.metrics.font_m).color(if selected {
                p.header_text
            } else {
                p.text_secondary
            }))
            .width(Length::Fill)
            .padding(Padding::from([6, 10]))
            .on_press(su(SettingsUi::Select(c)))
            .style(theme::list_row(p, selected)),
        );
    }
    col = col.push(divider(app));
    col = col.push(
        button(
            row![
                text("Log Out")
                    .size(t.metrics.font_m)
                    .color(p.danger)
                    .width(Length::Fill),
                icon(Icon::LogOut, 16.0, p.danger),
            ]
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding::from([6, 10]))
        .on_press(ui(Ui::Logout))
        .style(theme::list_row(p, false)),
    );
    col.into()
}

fn divider(app: &App) -> El<'_> {
    container(Space::new().height(1).width(Length::Fill))
        .padding(Padding::from([8, 10]))
        .style(theme::bg(Color {
            a: 0.0,
            ..app.theme.palette.border
        }))
        .into()
}

pub(crate) fn rule(app: &App) -> El<'_> {
    container(Space::new().height(1).width(Length::Fill))
        .style(theme::bg(app.theme.palette.border))
        .into()
}

fn page(app: &App) -> El<'_> {
    match app.settings.category() {
        Category::MyAccount => my_account(app),
        Category::Profiles => profiles(app),
        Category::Privacy => privacy(app),
        Category::Status => status(app),
        Category::Appearance => appearance(app),
        Category::Accessibility => accessibility(app),
        Category::VoiceVideo => voice(app),
        Category::Chat => chat(app),
        Category::Notifications => notifications(app),
        Category::Keybinds => keybinds(app),
        Category::Language => language(app),
        Category::Advanced => advanced(app),
        Category::Updates => crate::update::view::page(app),
        Category::Plugins => super::plugins::page(app),
    }
}

// -- building blocks -----------------------------------------------------------

pub(crate) fn title<'a>(app: &'a App, s: &'a str) -> El<'a> {
    text(s)
        .size(app.theme.metrics.font_l)
        .font(INTER_SEMIBOLD)
        .color(app.theme.palette.header_text)
        .into()
}

pub(crate) fn heading<'a>(app: &'a App, s: &'a str) -> El<'a> {
    container(
        text(s)
            .size(app.theme.metrics.font_xs)
            .font(INTER_SEMIBOLD)
            .color(app.theme.palette.text_secondary),
    )
    .padding(Padding::ZERO.top(8))
    .into()
}

pub(crate) fn note<'a>(app: &'a App, s: impl Into<String>) -> El<'a> {
    text(s.into())
        .size(app.theme.metrics.font_s)
        .color(app.theme.palette.text_muted)
        .into()
}

pub(crate) fn toggle<'a>(
    app: &'a App,
    title: &'a str,
    hint: &'a str,
    on: bool,
    msg: Message,
) -> El<'a> {
    let t = &app.theme;
    let p = t.palette;
    let mut text_col = column![text(title).size(t.metrics.font_m).color(p.text_normal)].spacing(4);
    if !hint.is_empty() {
        text_col = text_col.push(text(hint).size(t.metrics.font_s).color(p.text_muted));
    }
    column![
        row![
            text_col.width(Length::Fill),
            toggler(on)
                .on_toggle(move |_| msg.clone())
                .size(24.0)
                .style(theme::switch(p)),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
        rule(app),
    ]
    .spacing(16)
    .into()
}

/// A toggle bound to a synced boolean setting.
fn synced_toggle<'a>(app: &'a App, s: Synced, title: &'a str, hint: &'a str) -> El<'a> {
    let on = app.settings.synced.bool(s);
    toggle(
        app,
        title,
        hint,
        on,
        su(SettingsUi::Set(s, Value::Bool(!on))),
    )
}

fn radios<'a, V: Copy + Eq + 'a>(
    app: &'a App,
    options: &[(V, &'a str)],
    selected: V,
    msg: impl Fn(V) -> Message + Copy + 'a,
) -> El<'a> {
    let p = app.theme.palette;
    let mut col = column![].spacing(8);
    for (v, label) in options {
        col = col.push(
            container(
                radio(*label, *v, Some(selected), msg)
                    .size(20)
                    .spacing(12)
                    .text_size(app.theme.metrics.font_m)
                    .style(theme::radio(p)),
            )
            .width(Length::Fill)
            .padding(Padding::from([10, 12]))
            .style(theme::rounded_bg(
                if *v == selected {
                    p.bg_selected
                } else {
                    p.bg_hover
                },
                app.theme.metrics.radius,
            )),
        );
    }
    col.into()
}

fn labelled_slider<'a>(
    app: &'a App,
    label: &'a str,
    value: String,
    s: impl Into<El<'a>>,
) -> El<'a> {
    let t = &app.theme;
    column![
        row![
            text(label)
                .size(t.metrics.font_xs)
                .font(INTER_SEMIBOLD)
                .color(t.palette.text_secondary)
                .width(Length::Fill),
            text(value)
                .size(t.metrics.font_xs)
                .color(t.palette.text_muted),
        ],
        s.into(),
    ]
    .spacing(8)
    .into()
}

pub(crate) fn page_col<'a>() -> iced::widget::Column<'a, Message> {
    column![].spacing(20).width(Length::Fill)
}

// -- pages -------------------------------------------------------------------------

fn my_account(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let mut col = page_col().push(title(app, "My Account"));
    if let Some(me) = app.chat.me.as_ref() {
        let field = |label: &'static str, value: String, edit: bool| -> El<'_> {
            let mut r = row![
                column![
                    text(label)
                        .size(t.metrics.font_xs)
                        .font(INTER_SEMIBOLD)
                        .color(p.text_secondary),
                    text(value).size(t.metrics.font_m).color(p.text_normal),
                ]
                .spacing(4)
                .width(Length::Fill),
            ]
            .align_y(Alignment::Center);
            if edit {
                r = r.push(
                    button(text("Edit").size(t.metrics.font_s).color(p.header_text))
                        .padding(Padding::from([6, 16]))
                        .on_press(su(SettingsUi::Select(Category::Profiles)))
                        .style(theme::secondary_button(p)),
                );
            }
            r.into()
        };
        let display = me
            .global_name
            .clone()
            .unwrap_or_else(|| me.username.clone());
        let card = container(
            column![
                container(Space::new().height(100).width(Length::Fill))
                    .style(theme::rounded_bg(p.accent, t.metrics.radius_large)),
                row![
                    super::avatar(app, me, 80.0),
                    column![
                        text(display.clone())
                            .size(t.metrics.font_l)
                            .font(INTER_SEMIBOLD)
                            .color(p.header_text),
                        text(me.username.clone())
                            .size(t.metrics.font_s)
                            .color(p.text_secondary),
                    ]
                    .spacing(2),
                ]
                .spacing(16)
                .align_y(Alignment::Center)
                .padding(Padding::from([0, 16])),
                container(
                    container(
                        column![
                            field("DISPLAY NAME", display, true),
                            field("USERNAME", me.username.clone(), false),
                        ]
                        .spacing(20)
                    )
                    .padding(16)
                    .width(Length::Fill)
                    .style(theme::rounded_bg(p.bg_app, t.metrics.radius_large))
                )
                .padding(Padding::from([0, 16])),
            ]
            .spacing(12),
        )
        .padding(Padding::ZERO.bottom(16))
        .style(theme::settings_card(p));
        col = col.push(card);
    }
    col = col
        .push(heading(app, "PASSWORD AND AUTHENTICATION"))
        .push(note(
            app,
            "Email, phone number, password, two-factor authentication, username changes \
             and disabling or deleting the account are managed on discord.com or in the \
             official app: Ferrocord does not change them.",
        ));
    col.into()
}

fn profiles(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let f = &app.settings.forms;
    let input = |placeholder: &'static str, value: &str, on: fn(String) -> SettingsUi| {
        text_input(placeholder, value)
            .on_input(move |s| su(on(s)))
            .padding(10)
            .size(t.metrics.font_m)
            .style(theme::input(p, p.bg_app))
    };
    let username = app.chat.me.as_ref().map_or("", |u| u.username.as_str());
    let bio_len = f.bio.text().trim_end_matches('\n').chars().count();
    let mut col = page_col()
        .push(title(app, "Profiles"))
        .push(heading(app, "DISPLAY NAME"))
        .push(
            text_input(username, &f.name)
                .on_input(|s| su(SettingsUi::ProfileName(s)))
                .padding(10)
                .size(t.metrics.font_m)
                .style(theme::input(p, p.bg_app)),
        )
        .push(heading(app, "PRONOUNS"))
        .push(input(
            "Add your pronouns",
            &f.pronouns,
            SettingsUi::ProfilePronouns,
        ))
        .push(heading(app, "ABOUT ME"))
        .push(note(
            app,
            "You can use markdown and links if you'd like.".to_owned(),
        ))
        .push(
            text_editor(&f.bio)
                .placeholder("Tell people about yourself")
                .on_action(|a| su(SettingsUi::BioAction(a)))
                .height(120)
                .padding(10)
                .size(t.metrics.font_m)
                .style(theme::editor(p, p.bg_app)),
        )
        .push(
            container(note(
                app,
                format!("{}", st::BIO_MAX.saturating_sub(bio_len)),
            ))
            .align_right(Length::Fill),
        );
    if f.profile_dirty() || f.saving {
        let save = button(
            text(if f.saving {
                "Saving…"
            } else {
                "Save Changes"
            })
            .size(t.metrics.font_s)
            .color(p.accent_text),
        )
        .padding(Padding::from([8, 16]))
        .style(theme::primary_button(p));
        let save = if f.saving {
            save
        } else {
            save.on_press(su(SettingsUi::SaveProfile))
        };
        col = col.push(
            container(
                row![
                    text("Careful — you have unsaved changes!")
                        .size(t.metrics.font_s)
                        .color(p.text_normal)
                        .width(Length::Fill),
                    button(text("Reset").size(t.metrics.font_s).color(p.text_normal))
                        .padding(Padding::from([8, 12]))
                        .on_press(su(SettingsUi::ResetProfile))
                        .style(theme::link_button(p)),
                    save,
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([10, 16]))
            .style(theme::rounded_bg(p.bg_elevated, t.metrics.radius_large)),
        );
    }
    col.push(note(
        app,
        "Avatar, banner, profile theme, decorations and effects are not edited here.",
    ))
    .into()
}

fn privacy(app: &App) -> El<'_> {
    let s = &app.settings.synced;
    let spam = s.u32(Synced::DmSpamFilter);
    let flags = s.u32(Synced::FriendSourceFlags);
    let friend = |bit: u32, label: &'static str, hint: &'static str| -> El<'_> {
        let on = flags & bit == bit;
        let next = st::toggle_friend_source(flags, bit);
        toggle(
            app,
            label,
            hint,
            on,
            su(SettingsUi::Set(Synced::FriendSourceFlags, Value::U32(next))),
        )
    };
    page_col()
        .push(title(app, "Privacy & Safety"))
        .push(heading(app, "DIRECT MESSAGE SPAM"))
        .push(note(
            app,
            "Discord moves messages it thinks are spam into a separate Spam inbox.",
        ))
        .push(radios(
            app,
            &[
                (3u32, "Filter all direct messages"),
                (2, "Filter direct messages from non-friends"),
                (1, "Do not filter direct messages"),
            ],
            // Unset: Discord's default filters non-friends.
            if spam == 0 { 2 } else { spam },
            |v| su(SettingsUi::Set(Synced::DmSpamFilter, Value::U32(v))),
        ))
        .push(heading(app, "SERVER PRIVACY DEFAULTS"))
        .push(toggle(
            app,
            "Allow DMs from other members in servers",
            "Applies to servers you join from now on.",
            !s.bool(Synced::DefaultGuildsRestricted),
            su(SettingsUi::Set(
                Synced::DefaultGuildsRestricted,
                Value::Bool(!s.bool(Synced::DefaultGuildsRestricted)),
            )),
        ))
        .push(heading(app, "WHO CAN SEND YOU A FRIEND REQUEST"))
        .push(friend(us::FRIENDS_NO_RELATION, "Everyone", ""))
        .push(friend(us::FRIENDS_MUTUAL_FRIENDS, "Friends of Friends", ""))
        .push(friend(us::FRIENDS_MUTUAL_GUILDS, "Server Members", ""))
        .into()
}

fn status(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let f = &app.settings.forms;
    let current = app.chat.my_status;
    let mut presence = column![].spacing(8);
    for s in [
        Presence::Online,
        Presence::Idle,
        Presence::Dnd,
        Presence::Offline,
    ] {
        let dot = container(Space::new().width(10).height(10))
            .style(theme::rounded_bg(theme::presence_color(&p, s), 5.0));
        presence = presence.push(
            button(
                row![
                    dot,
                    text(s.label())
                        .size(t.metrics.font_m)
                        .color(p.text_normal)
                        .width(Length::Fill),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(Padding::from([10, 12]))
            .on_press(su(SettingsUi::SetStatus(s)))
            .style(theme::list_row(p, s == current)),
        );
    }
    let now = crate::app::now_ms();
    let active = app
        .settings
        .synced
        .custom
        .as_ref()
        .filter(|c| c.active(now));
    let current_custom: El<'_> = match active {
        Some(c) => note(
            app,
            format!(
                "Current: {} {}{}",
                c.emoji_name,
                c.text,
                if c.expires_at_ms > 0 {
                    format!(" (clears at {})", super::format_time(c.expires_at_ms))
                } else {
                    String::new()
                }
            ),
        ),
        None => note(app, "No custom status."),
    };
    page_col()
        .push(title(app, "Activity & Status"))
        .push(heading(app, "ONLINE STATUS"))
        .push(presence)
        .push(note(
            app,
            "Saved to your account, so every client and the tray show the same status.",
        ))
        .push(heading(app, "CUSTOM STATUS"))
        .push(current_custom)
        .push(
            row![
                text_input("🙂", &f.custom_emoji)
                    .on_input(|s| su(SettingsUi::CustomEmoji(s)))
                    .padding(10)
                    .width(64)
                    .size(t.metrics.font_m)
                    .style(theme::input(p, p.bg_app)),
                text_input("What's cookin', good lookin'?", &f.custom_text)
                    .on_input(|s| su(SettingsUi::CustomText(s)))
                    .on_submit(su(SettingsUi::SaveCustomStatus))
                    .padding(10)
                    .size(t.metrics.font_m)
                    .style(theme::input(p, p.bg_app)),
            ]
            .spacing(8),
        )
        .push(
            row![
                text("Clear after")
                    .size(t.metrics.font_s)
                    .color(p.text_secondary),
                pick_list(&Expiry::ALL[..], f.expiry.or(Some(Expiry::Never)), |e| su(
                    SettingsUi::CustomExpiry(e)
                ))
                .text_size(t.metrics.font_s),
                Space::new().width(Length::Fill),
                button(text("Clear").size(t.metrics.font_s).color(p.text_normal))
                    .padding(Padding::from([8, 16]))
                    .on_press(su(SettingsUi::ClearCustomStatus))
                    .style(theme::link_button(p)),
                button(text("Save").size(t.metrics.font_s).color(p.accent_text))
                    .padding(Padding::from([8, 16]))
                    .on_press(su(SettingsUi::SaveCustomStatus))
                    .style(theme::primary_button(p)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .into()
}

fn appearance(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let current = app.settings.synced.theme();
    let mut swatches = row![].spacing(16);
    for th in Theme::ALL {
        let fill = match th {
            Theme::Light => theme::Palette::LIGHT.bg_panel,
            Theme::Dark => theme::Palette::DARK.bg_panel,
            Theme::Darker => theme::Palette::ASH.bg_panel,
            Theme::Midnight => theme::Palette::ONYX.bg_panel,
        };
        swatches = swatches.push(
            column![
                button(Space::new())
                    .width(theme::SETTINGS_SWATCH)
                    .height(theme::SETTINGS_SWATCH)
                    .on_press(su(SettingsUi::Set(Synced::Theme, Value::U32(th.wire()))))
                    .style(theme::swatch(fill, p.accent, th == current)),
                text(th.label())
                    .size(t.metrics.font_s)
                    .color(p.text_secondary),
            ]
            .spacing(6)
            .align_x(Alignment::Center),
        );
    }
    let local = &app.settings.local;
    let font_i = CHAT_FONT_SIZES
        .iter()
        .position(|s| *s >= local.chat_font)
        .unwrap_or(3) as u8;
    let zoom_i = ZOOM_LEVELS
        .iter()
        .position(|z| *z >= local.zoom)
        .unwrap_or(5) as u8;
    let last_font = (CHAT_FONT_SIZES.len() - 1) as u8;
    let last_zoom = (ZOOM_LEVELS.len() - 1) as u8;
    let hour = app.settings.synced.u32(Synced::HourCycle);
    page_col()
        .push(title(app, "Appearance"))
        .push(heading(app, "THEME"))
        .push(swatches)
        .push(heading(app, "MESSAGE DISPLAY"))
        .push(radios(
            app,
            &[
                (false, "Cozy: modern, beautiful, and easy on your eyes."),
                (true, "Compact: fit more messages on screen at one time."),
            ],
            app.compact(),
            |v| {
                su(SettingsUi::Set(
                    Synced::MessageDisplayCompact,
                    Value::Bool(v),
                ))
            },
        ))
        .push(labelled_slider(
            app,
            "CHAT FONT SCALING",
            format!("{}px", local.chat_font),
            slider(0..=last_font, font_i, move |i| {
                su(SettingsUi::LocalPreview(LocalChange::ChatFont(
                    CHAT_FONT_SIZES[usize::from(i.min(last_font))],
                )))
            })
            .on_release(su(SettingsUi::SaveLocal)),
        ))
        .push(labelled_slider(
            app,
            "ZOOM LEVEL",
            format!("{}%", local.zoom),
            slider(0..=last_zoom, zoom_i, move |i| {
                su(SettingsUi::LocalPreview(LocalChange::Zoom(
                    ZOOM_LEVELS[usize::from(i.min(last_zoom))],
                )))
            })
            .on_release(su(SettingsUi::SaveLocal)),
        ))
        .push(heading(app, "TIME FORMAT"))
        .push(radios(
            app,
            &[
                (0u32, "Automatic (from your system language)"),
                (1, "12-hour (2:03 PM)"),
                (2, "24-hour (14:03)"),
            ],
            hour,
            |v| su(SettingsUi::Set(Synced::HourCycle, Value::U32(v))),
        ))
        .into()
}

fn accessibility(app: &App) -> El<'_> {
    let local = &app.settings.local;
    let sticker = app.sticker_anim();
    page_col()
        .push(title(app, "Accessibility"))
        .push(heading(app, "SATURATION"))
        .push(labelled_slider(
            app,
            "SATURATION",
            format!("{}%", local.saturation),
            slider(0..=100u8, local.saturation, |v| {
                su(SettingsUi::LocalPreview(LocalChange::Saturation(v)))
            })
            .step(5u8)
            .on_release(su(SettingsUi::SaveLocal)),
        ))
        .push(heading(app, "MOTION"))
        .push(toggle(
            app,
            "Enable Reduced Motion",
            "Keeps animated avatars, decorations, emoji, GIFs, banners and profile effects still.",
            app.prefs.reduce_motion,
            ui(Ui::ToggleReduceMotion),
        ))
        .push(synced_toggle(
            app,
            Synced::GifAutoPlay,
            "Automatically play GIFs when Ferrocord is focused.",
            "Off: GIFs play while you hover their message.",
        ))
        .push(synced_toggle(
            app,
            Synced::AnimateEmoji,
            "Play animated emoji",
            "Animated custom emoji in messages and reactions move.",
        ))
        .push(heading(app, "STICKERS"))
        .push(note(app, "Animate stickers"))
        .push(radios(
            app,
            &[
                (StickerAnim::Always, "Always animate"),
                (
                    StickerAnim::OnInteraction,
                    "Animate on interaction (while you hover the message)",
                ),
                (StickerAnim::Never, "Never animate"),
            ],
            sticker,
            |v| {
                su(SettingsUi::Set(
                    Synced::AnimateStickers,
                    Value::U32(v.wire()),
                ))
            },
        ))
        .into()
}

fn voice(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let v = &app.voice;
    let default = "Default".to_owned();
    let with_default = |list: &[String]| -> Vec<String> {
        std::iter::once(default.clone())
            .chain(list.iter().cloned())
            .collect()
    };
    let pick = |s: String| if s == "Default" { None } else { Some(s) };
    let sel_in = v.prefs.input.clone().unwrap_or_else(|| default.clone());
    let sel_out = v.prefs.output.clone().unwrap_or_else(|| default.clone());
    let testing = app.settings.mic_test.is_some();
    let live = v.conn.is_some() || app.settings.mic_test.is_some() || app.cfg.mock;
    let level = if live {
        v.control.input_level()
    } else {
        crate::net::voice::mixer::FLOOR_DB
    };
    let threshold = v.prefs.threshold_db;
    let frac = ((level + 100.0) / 100.0).clamp(0.0, 1.0);
    let filled = ((frac * 1000.0) as u16).max(1);
    let meter_color = if level >= threshold {
        p.success
    } else {
        p.status_idle
    };
    let meter = stack![
        container(Space::new().width(Length::Fill).height(8))
            .style(theme::rounded_bg(p.bg_app, 4.0)),
        row![
            container(Space::new().width(Length::Fill).height(8))
                .width(Length::FillPortion(filled))
                .style(theme::rounded_bg(meter_color, 4.0)),
            Space::new()
                .width(Length::FillPortion(1000u16.saturating_sub(filled).max(1)))
                .height(8),
        ],
    ];
    let vui = |x: VoiceUi| ui(Ui::Voice(x));
    let in_call = v.conn.is_some();
    let test_button = button(
        text(if testing {
            "Stop Testing"
        } else {
            "Let's Check"
        })
        .size(t.metrics.font_s)
        .color(p.accent_text),
    )
    .padding(Padding::from([8, 16]))
    .style(theme::primary_button(p));
    let test_button = if in_call {
        test_button
    } else {
        test_button.on_press(su(SettingsUi::ToggleMicTest))
    };
    page_col()
        .push(title(app, "Voice & Video"))
        .push(
            row![
                column![
                    heading(app, "INPUT DEVICE"),
                    pick_list(with_default(&v.devices.0), Some(sel_in), move |s| vui(
                        VoiceUi::InputDevice(pick(s))
                    ))
                    .width(Length::Fill),
                ]
                .spacing(8)
                .width(Length::Fill),
                column![
                    heading(app, "OUTPUT DEVICE"),
                    pick_list(with_default(&v.devices.1), Some(sel_out), move |s| vui(
                        VoiceUi::OutputDevice(pick(s))
                    ))
                    .width(Length::Fill),
                ]
                .spacing(8)
                .width(Length::Fill),
            ]
            .spacing(16),
        )
        .push(
            row![
                container(labelled_slider(
                    app,
                    "INPUT VOLUME",
                    format!("{:.0}%", v.prefs.input_volume),
                    slider(0.0..=200.0, v.prefs.input_volume, move |x| vui(
                        VoiceUi::InputVolume(x)
                    ))
                    .step(1.0_f32),
                ))
                .width(Length::Fill),
                container(labelled_slider(
                    app,
                    "OUTPUT VOLUME",
                    format!("{:.0}%", v.prefs.output_volume),
                    slider(0.0..=200.0, v.prefs.output_volume, move |x| vui(
                        VoiceUi::OutputVolume(x)
                    ))
                    .step(1.0_f32),
                ))
                .width(Length::Fill),
            ]
            .spacing(16),
        )
        .push(heading(app, "MIC TEST"))
        .push(note(
            app,
            if in_call {
                "You are in a call: the meter shows your microphone live."
            } else {
                "Having mic issues? Start a test and say something fun: we'll play your voice back to you."
            },
        ))
        .push(
            row![test_button, container(meter).width(Length::Fill)]
                .spacing(16)
                .align_y(Alignment::Center),
        )
        .push(labelled_slider(
            app,
            "INPUT SENSITIVITY",
            format!("{threshold:.0} dB"),
            slider(-100.0..=0.0, threshold, move |x| vui(VoiceUi::Threshold(x))).step(1.0_f32),
        ))
        .push(note(app, "Your voice is sent when the bar is green."))
        .push(note(
            app,
            "Push to Talk needs a global keyboard shortcut, which Ferrocord does not have yet: \
             voice activity is used.",
        ))
        .into()
}

fn chat(app: &App) -> El<'_> {
    let spoilers = app.spoilers();
    page_col()
        .push(title(app, "Chat"))
        .push(heading(app, "DISPLAY IMAGES, VIDEOS, AND LOLCATS"))
        .push(synced_toggle(
            app,
            Synced::InlineEmbedMedia,
            "When posted as links to chat",
            "",
        ))
        .push(synced_toggle(
            app,
            Synced::InlineAttachmentMedia,
            "When uploaded directly to Discord",
            "Off: attachments show as files you can open.",
        ))
        .push(heading(app, "EMBEDS AND LINK PREVIEWS"))
        .push(synced_toggle(
            app,
            Synced::RenderEmbeds,
            "Show embeds and preview website links pasted into chat",
            "",
        ))
        .push(heading(app, "EMOJI"))
        .push(synced_toggle(
            app,
            Synced::RenderReactions,
            "Show emoji reactions on messages",
            "",
        ))
        .push(synced_toggle(
            app,
            Synced::ConvertEmoticons,
            "Automatically convert emoticons in your messages to emoji",
            "For example, when you type :-) Ferrocord sends 🙂",
        ))
        .push(heading(app, "SHOW SPOILER CONTENT"))
        .push(radios(
            app,
            &[
                (Spoilers::OnClick, "On click"),
                (Spoilers::IfModerator, "On servers I moderate"),
                (Spoilers::Always, "Always"),
            ],
            spoilers,
            |v| {
                su(SettingsUi::Set(
                    Synced::RenderSpoilers,
                    Value::Str(v.wire().into()),
                ))
            },
        ))
        .into()
}

fn notifications(app: &App) -> El<'_> {
    let local = &app.settings.local;
    let mut col = page_col()
        .push(title(app, "Notifications"))
        .push(toggle(
            app,
            "Enable Desktop Notifications",
            "Mentions and direct messages while you are elsewhere. The tray menu has the same switch.",
            !app.prefs.muted,
            ui(Ui::ToggleMuted),
        ))
        .push(toggle(
            app,
            "Show message content",
            "Off: notifications show only who wrote, not what.",
            local.notify_preview,
            su(SettingsUi::Local(LocalChange::NotifyPreview(!local.notify_preview))),
        ))
        .push(toggle(
            app,
            "Enable Notification Sounds",
            "Desktop notifications play your system's message sound.",
            local.notify_sound,
            su(SettingsUi::Local(LocalChange::NotifySound(!local.notify_sound))),
        ))
        .push(toggle(
            app,
            "Enable Unread Message Badge",
            "The tray icon shows how many mentions are waiting.",
            local.unread_badge,
            su(SettingsUi::Local(LocalChange::UnreadBadge(!local.unread_badge))),
        ))
        .push(toggle(
            app,
            "Enable Taskbar Flashing",
            "Flash the taskbar entry when you get a mention while Ferrocord is in the background.",
            local.taskbar_flash,
            su(SettingsUi::Local(LocalChange::TaskbarFlash(!local.taskbar_flash))),
        ));
    if !app.cfg.notification_preview {
        col = col.push(note(
            app,
            "FERROCORD_NOTIFY_PREVIEW=0 is set: message content stays hidden.",
        ));
    }
    col.push(note(
        app,
        "Per-server and per-channel levels are in each server's and channel's bell menu.",
    ))
    .into()
}

/// The shortcuts Ferrocord handles (not editable).
pub const KEYBINDS: [(&str, &str); 7] = [
    ("Enter", "Send the message"),
    ("Shift + Enter", "New line in the message"),
    ("Up", "Edit your last message (empty message box)"),
    (
        "Escape",
        "Close a menu, dialog or these settings; cancel a reply or edit",
    ),
    ("Ctrl + V", "Paste text, images or files"),
    ("Ctrl + K", "Find or start a conversation"),
    (
        "Right click",
        "Context menu of a message, user, channel or server",
    ),
];

fn keybinds(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let mut table = column![].spacing(12);
    for (keys, action) in KEYBINDS {
        table = table.push(
            row![
                text(action)
                    .size(t.metrics.font_m)
                    .color(p.text_normal)
                    .width(Length::Fill),
                container(
                    text(keys)
                        .size(t.metrics.font_s)
                        .font(INTER_SEMIBOLD)
                        .color(p.header_text)
                )
                .padding(Padding::from([4, 8]))
                .style(theme::rounded_bg(p.bg_app, t.metrics.radius)),
            ]
            .align_y(Alignment::Center),
        );
    }
    page_col()
        .push(title(app, "Keybinds"))
        .push(note(
            app,
            "Shortcuts work while the Ferrocord window is focused. They cannot be changed yet.",
        ))
        .push(
            container(table)
                .padding(16)
                .width(Length::Fill)
                .style(theme::settings_card(p)),
        )
        .into()
}

fn language(app: &App) -> El<'_> {
    let current = app.settings.synced.get(Synced::Locale);
    let current = current.as_str();
    let options: Vec<(usize, &'static str)> = LOCALES
        .iter()
        .enumerate()
        .map(|(i, (_, name))| (i, *name))
        .collect();
    let selected = LOCALES
        .iter()
        .position(|(code, _)| *code == current)
        .unwrap_or(4);
    page_col()
        .push(title(app, "Language"))
        .push(note(
            app,
            "Sets your Discord account language (emails, system messages and the official \
             apps). Ferrocord's own interface is English only.",
        ))
        .push(heading(app, "SELECT A LANGUAGE"))
        .push(radios(app, &options, selected, |i| {
            let code = LOCALES.get(i).map_or("en-US", |(c, _)| *c);
            su(SettingsUi::Set(Synced::Locale, Value::Str(code.into())))
        }))
        .into()
}

fn advanced(app: &App) -> El<'_> {
    let tray_missing = app.cfg.tray && app.win.host == TrayHost::Unavailable;
    let tray_note: Option<&'static str> = if !app.cfg.tray {
        Some("The system tray is turned off (--no-tray): closing the window quits.")
    } else if tray_missing {
        Some("No system tray was found on this desktop: closing the window quits.")
    } else {
        None
    };
    let mut col = page_col()
        .push(title(app, "Advanced"))
        .push(synced_toggle(
            app,
            Synced::DeveloperMode,
            "Developer Mode",
            "Adds \"Copy ID\" to the context menus of messages, users, channels and servers.",
        ))
        .push(heading(app, "SYSTEM TRAY AND STARTUP"))
        .push(toggle(
            app,
            "Minimize to Tray",
            "The close button keeps Ferrocord running in the tray so notifications keep arriving.",
            app.prefs.close_to_tray,
            ui(Ui::ToggleCloseToTray),
        ))
        .push(toggle(
            app,
            "Start Minimized",
            "Launch without opening a window; click the tray icon to show it.",
            app.prefs.start_minimized,
            ui(Ui::ToggleStartMinimized),
        ));
    if autostart::SUPPORTED {
        col = col.push(toggle(
            app,
            open_at_login(),
            "Adds an autostart entry that starts minimized to the tray.",
            app.autostart,
            ui(Ui::ToggleAutostart),
        ));
    }
    if let Some(n) = tray_note {
        col = col.push(note(app, n));
    }
    col.into()
}

fn open_at_login() -> &'static str {
    static LABEL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LABEL.get_or_init(|| format!("Open {APP_NAME} when I log in"))
}
