//! Views: pure functions `&App -> Element<Message>`.

pub mod anim_image;
pub mod channel_admin;
pub mod chat;
pub mod clock;
pub mod context_menu;
pub mod cosmetics;
pub mod embeds;
pub mod emoji_font;
pub mod frame_stats;
pub mod friends;
pub mod icons;
pub mod lightbox;
pub mod login;
pub mod markdown;
pub mod members;
pub mod plugins;
pub mod popovers;
pub mod popups;
pub mod profile;
pub mod rail;
pub mod reactions;
pub mod roles;
pub mod search;
pub mod server_settings;
pub mod servers;
pub mod settings;
pub mod sidebar;
pub mod smooth_scroll;
pub mod theme;
pub mod video;
pub mod voice;

use chrono::{DateTime, Datelike, Local, TimeZone};
use iced::widget::{column, container, row, stack, text};
use iced::{Alignment, Element, Length, Padding};

use crate::app::{App, Message, Screen, Ui};
use crate::net::ConnectionState;
use crate::net::model::User;
use theme::{FerroTheme, INTER_SEMIBOLD, Palette};

pub type El<'a> = Element<'a, Message>;

/// View of the (only) main window, in the shape `iced::daemon` expects.
pub fn view_window(app: &App, _window: iced::window::Id) -> El<'_> {
    frame_stats::view_built();
    view(app)
}

pub fn view(app: &App) -> El<'_> {
    let t = &app.theme;
    let body: El<'_> = match app.screen() {
        Screen::Splash => splash(t),
        Screen::Login => login::view(app),
        Screen::Main => {
            let mut r = row![rail::view(app), sidebar::view(app), chat::view(app)];
            if app.search.panel_visible() {
                r = r.push(search::view(app));
            } else if app.members_open {
                match app.view {
                    crate::app::View::Guild {
                        channel: Some(_), ..
                    } => r = r.push(members::view(app)),
                    crate::app::View::Home(Some(c)) => {
                        if let Some(c) = app.chat.channels.get(&c) {
                            r = r.push(members::dm_view(app, c));
                        }
                    }
                    _ => {}
                }
            }
            r.height(Length::Fill).into()
        }
    };
    // The widget tree must have the SAME shape whether or not a banner, a
    // popup, the settings dialog or the link dialog is showing. iced diffs
    // widget state by position and widget type: wrapping the body in a new
    // `stack` (or a `column`) only while a popup is open makes every
    // scrollable, hover and text-input state under it be thrown away, which
    // jumped the message list and caused the "flicker" on right-click. Each
    // optional layer therefore has a fixed slot that holds an empty `Space`
    // while it is closed.
    let banner: El<'_> = banner(app).unwrap_or_else(empty);
    // The update banner has its own fixed slot next to the connection
    // banner (same tree shape whether or not it shows).
    let update_banner: El<'_> = if app.screen() == Screen::Main {
        crate::update::view::banner(app).unwrap_or_else(empty)
    } else {
        empty()
    };
    let content: El<'_> = column![column![banner, update_banner], body].into();
    if app.screen() != Screen::Main {
        return container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::bg(t.palette.bg_app))
            .into();
    }
    let mut layers: Vec<El<'_>> = Vec::with_capacity(LAYER_SLOTS);
    layers.push(content);
    layers.push(servers::discover(app).unwrap_or_else(empty));
    layers.extend(popups::layers(app));
    layers.push(voice::user_menu(app).unwrap_or_else(empty));
    layers.push(voice::incoming_overlay(app).unwrap_or_else(empty));
    layers.push(servers::add_server(app).unwrap_or_else(empty));
    layers.push(servers::folder_dialog(app).unwrap_or_else(empty));
    layers.push(reactions::dialog(app).unwrap_or_else(empty));
    layers.push(friends::switcher(app).unwrap_or_else(empty));
    layers.push(if app.settings_open {
        settings::view(app)
    } else {
        empty()
    });
    layers.push(server_settings::screen(app).unwrap_or_else(empty));
    layers.push(server_settings::dialog(app).unwrap_or_else(empty));
    layers.push(channel_admin::view(app).unwrap_or_else(empty));
    layers.push(roles::dialog(app).unwrap_or_else(empty));
    layers.push(match &app.pending_link {
        Some(url) => leave_dialog(t, url),
        None => empty(),
    });
    layers.push(lightbox::view(app).unwrap_or_else(empty));
    layers.push(video::fullscreen(app).unwrap_or_else(empty));
    container(stack(layers))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::bg(t.palette.bg_app))
        .into()
}

/// Base content + discover page + popup slots + voice user menu + incoming
/// call + add-server dialog + folder dialog + reactions dialog +
/// conversation switcher + settings + server settings + admin dialog +
/// channel dialog + role/member dialog + link dialog + image viewer +
/// full-screen video.
pub const LAYER_SLOTS: usize = 1 + 1 + popups::SLOTS + 2 + 10 + 1 + 1;

/// Approximate advance width of `c` in Inter, in em.
fn glyph_em(c: char) -> f32 {
    match c {
        'i' | 'l' | 'j' | 'I' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' => 0.30,
        ' ' | 't' | 'f' | 'r' | '(' | ')' | '[' | ']' | '-' | '/' => 0.40,
        'm' | 'w' | 'M' | 'W' | '@' => 0.92,
        c if c.is_ascii_uppercase() => 0.70,
        c if c.is_ascii() => 0.57,
        // Wide scripts and emoji.
        _ => 1.05,
    }
}

/// Shortens `s` with a trailing `…` so that it is estimated to fit in
/// `max_px` at `size` (semi-bold text is a little wider). iced has no text
/// ellipsis, so views use this inside fixed-width areas (and clip as a
/// safety net). The estimate errs on the short side.
pub fn ellipsize(s: &str, max_px: f32, size: f32, semibold: bool) -> String {
    let scale = size * if semibold { 1.06 } else { 1.0 };
    let width = |s: &str| s.chars().map(glyph_em).sum::<f32>() * scale;
    if width(s) <= max_px {
        return s.to_owned();
    }
    let ellipsis = glyph_em('\u{2026}').max(0.9) * scale;
    let mut out = String::new();
    let mut used = ellipsis;
    for c in s.chars() {
        used += glyph_em(c) * scale;
        if used > max_px {
            break;
        }
        out.push(c);
    }
    let out = out.trim_end().to_owned();
    format!("{out}\u{2026}")
}

/// Message text for one-line previews (popovers, search results): mention
/// and emoji tokens become readable names.
pub fn plain_text(app: &App, content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find('>') else {
            out.push_str(tail);
            return out;
        };
        let tok = &tail[1..end];
        let id = |s: &str| s.parse::<u64>().ok().map(crate::net::model::Snowflake);
        let rep = if let Some(r) = tok.strip_prefix("@&") {
            id(r).and_then(|id| {
                app.chat
                    .guilds
                    .iter()
                    .flat_map(|g| g.roles.iter())
                    .find(|ro| ro.id == id)
                    .map(|ro| format!("@{}", ro.name))
            })
        } else if let Some(r) = tok.strip_prefix("@!").or_else(|| tok.strip_prefix('@')) {
            id(r).map(|id| format!("@{}", app.chat.user_name(id).unwrap_or("unknown-user")))
        } else if let Some(r) = tok.strip_prefix('#') {
            id(r).map(|id| {
                format!(
                    "#{}",
                    app.chat
                        .channels
                        .get(&id)
                        .and_then(|c| c.name.as_deref())
                        .unwrap_or("deleted-channel")
                )
            })
        } else {
            let t2 = tok.strip_prefix("a:").or_else(|| tok.strip_prefix(':'));
            t2.and_then(|r| r.split(':').next())
                .filter(|n| !n.is_empty() && tok.matches(':').count() >= 2)
                .map(|n| format!(":{n}:"))
        };
        match rep {
            Some(r) => {
                out.push_str(&r);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Placeholder for a closed layer.
pub fn empty<'a>() -> El<'a> {
    iced::widget::Space::new().into()
}

/// "Leaving Ferrocord" confirmation for masked links: shows the real URL.
fn leave_dialog<'a>(t: &FerroTheme, url: &str) -> El<'a> {
    let p = t.palette;
    let card = container(
        column![
            text("Leaving Ferrocord")
                .size(t.metrics.font_l)
                .font(INTER_SEMIBOLD)
                .color(p.header_text),
            text("This link is taking you to the following website")
                .size(t.metrics.font_s)
                .color(p.text_secondary),
            container(
                text(url.to_owned())
                    .size(t.metrics.font_s)
                    .color(p.text_normal)
            )
            .width(Length::Fill)
            .padding(10)
            .style(theme::rounded_bg(p.bg_app, 4.0)),
            row![
                iced::widget::Space::new().width(Length::Fill),
                iced::widget::button(text("Go Back").size(14).color(p.text_normal))
                    .padding(Padding::from([8, 16]))
                    .on_press(ui(Ui::CancelLink))
                    .style(theme::icon_button(p)),
                iced::widget::button(text("Visit Site").size(14).color(p.accent_text))
                    .padding(Padding::from([8, 16]))
                    .on_press(ui(Ui::OpenUrl(url.to_owned())))
                    .style(theme::primary_button(p)),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .width(440)
    .padding(16)
    .style(theme::rounded_bg(p.bg_panel, 8.0));
    iced::widget::opaque(
        iced::widget::mouse_area(
            container(iced::widget::opaque(card))
                .center(Length::Fill)
                .style(theme::bg(iced::Color {
                    a: 0.7,
                    ..iced::Color::BLACK
                })),
        )
        .on_press(ui(Ui::CancelLink)),
    )
}

fn splash(t: &FerroTheme) -> El<'static> {
    let p = t.palette;
    container(
        column![
            icons::icon(icons::Icon::MessageCircle, 56.0, p.accent),
            text("Ferrocord")
                .size(t.metrics.font_xl)
                .font(INTER_SEMIBOLD)
                .color(p.header_text),
            text("Connecting…")
                .size(t.metrics.font_s)
                .color(p.text_muted),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .center(Length::Fill)
    .into()
}

/// Thin banner while not connected (or for an error toast).
fn banner(app: &App) -> Option<El<'_>> {
    let p = app.theme.palette;
    if app.screen() != Screen::Main {
        return None;
    }
    let (msg, color) = match (&app.toast, app.connection) {
        (_, ConnectionState::Connecting) => ("Connecting to Discord…".to_string(), p.accent),
        (_, ConnectionState::Reconnecting { attempt, delay_ms }) => (
            format!(
                "Connection lost. Reconnecting{}… (attempt {attempt})",
                if delay_ms > 0 {
                    format!(" in {}s", delay_ms.div_ceil(1000))
                } else {
                    String::new()
                }
            ),
            p.mention_bar,
        ),
        (Some(t), _) => (t.clone(), p.danger),
        (None, ConnectionState::Disconnected) if !app.cfg.mock => (
            "Offline — showing cached messages.".to_string(),
            p.badge_muted,
        ),
        _ => return None,
    };
    Some(
        container(
            row![
                icons::icon(icons::Icon::WifiOff, 14.0, p.accent_text),
                text(msg)
                    .size(app.theme.metrics.font_s)
                    .color(p.accent_text),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding::from([6, 16]))
        .align_x(Alignment::Center)
        .style(theme::bg(color))
        .into(),
    )
}

/// Round avatar with its decoration, as a still: like the official client,
/// animated avatars and decorations outside profiles play only while their
/// row is hovered ([`hover_avatar`]).
pub fn avatar<'a>(app: &'a App, user: &User, size: f32) -> El<'a> {
    cosmetics::avatar_decorated(app, user, size, crate::app::cosmetics::Phase::Still)
}

/// Round avatar with its decoration in a list row or message that animates
/// while `hover` (that row) is hovered (see `App::list_phase`).
pub fn hover_avatar<'a>(
    app: &'a App,
    user: &User,
    size: f32,
    hover: crate::app::anim::Hover,
) -> El<'a> {
    cosmetics::avatar_decorated(app, user, size, app.list_phase(hover))
}

/// The avatar circle alone: the downloaded image (the animated frame in
/// `phase`), or a coloured circle with the user's initial.
pub fn avatar_circle<'a>(
    app: &'a App,
    user: &User,
    size: f32,
    phase: crate::app::cosmetics::Phase,
) -> El<'a> {
    let cdn_size = if size > 64.0 {
        crate::app::cosmetics::AV_POP
    } else {
        crate::app::cosmetics::AV_LIST
    };
    if let Some(a) = app.avatar_anim(user, cdn_size, phase) {
        let (epoch, playing) = app.anim_clock(phase);
        return anim_image::anim_image(a.clone(), epoch, playing, size, size)
            .border_radius(size / 2.0)
            .into();
    }
    if let Some(h) = app.avatar_frame(user, cdn_size) {
        return iced::widget::image(h.clone())
            .width(size)
            .height(size)
            .border_radius(size / 2.0)
            .into();
    }
    initials_circle(
        &app.theme.palette,
        user.display_name(),
        theme::default_avatar_color(user.id.0),
        size,
    )
}

pub fn initials_circle<'a>(p: &Palette, name: &str, color: iced::Color, size: f32) -> El<'a> {
    let initial: String = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    container(
        text(initial)
            .size(size * 0.45)
            .font(INTER_SEMIBOLD)
            .color(p.accent_text),
    )
    .width(size)
    .height(size)
    .center_x(size)
    .center_y(size)
    .style(theme::rounded_bg(color, size / 2.0))
    .into()
}

/// Avatar with its presence dot (user panel), drawn in `phase`.
pub fn avatar_with_status<'a>(
    app: &'a App,
    user: &User,
    size: f32,
    phase: crate::app::cosmetics::Phase,
) -> El<'a> {
    avatar_with_presence(
        app,
        user,
        size,
        app.chat.presence_of(user.id),
        app.theme.palette.user_panel,
        phase,
    )
}

/// Avatar with a presence dot, drawn in `phase`; `ring` is the background
/// colour the dot is cut out of.
pub fn avatar_with_presence<'a>(
    app: &'a App,
    user: &User,
    size: f32,
    status: crate::net::model::Presence,
    ring_color: iced::Color,
    phase: crate::app::cosmetics::Phase,
) -> El<'a> {
    let base = cosmetics::avatar_decorated(app, user, size, phase);
    with_presence(app, base, size, status, ring_color)
}

/// Puts the presence dot on `base` (an avatar slot of `size`).
pub fn with_presence<'a>(
    app: &'a App,
    base: El<'a>,
    size: f32,
    status: crate::net::model::Presence,
    ring_color: iced::Color,
) -> El<'a> {
    let p = app.theme.palette;
    let dot = size * 0.3;
    let ring = container(
        container(text(""))
            .width(dot)
            .height(dot)
            .style(theme::rounded_bg(
                theme::presence_color(&p, status),
                dot / 2.0,
            )),
    )
    .padding(2)
    .style(theme::rounded_bg(ring_color, dot));
    stack![
        base,
        container(ring)
            .width(size + 2.0)
            .height(size + 2.0)
            .align_right(size + 2.0)
            .align_bottom(size + 2.0),
    ]
    .into()
}

/// Red pill with a mention count.
pub fn mention_badge<'a>(p: &Palette, count: u32, border: iced::Color) -> El<'a> {
    let p = *p;
    let label = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    container(
        text(label)
            .size(12)
            .font(theme::INTER_BOLD)
            .color(p.accent_text),
    )
    .padding(Padding::from([0, 5]))
    .height(16)
    .center_y(16)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(p.badge)),
        border: iced::Border {
            radius: 8.0.into(),
            width: 0.0,
            color: border,
        },
        ..Default::default()
    })
    .into()
}

/// Fits (w, h) into (max_w, max_h) preserving aspect ratio.
pub fn fit_size(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (max_w.min(400), max_h.min(300));
    }
    let scale = (max_w as f32 / w as f32)
        .min(max_h as f32 / h as f32)
        .min(1.0);
    (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    )
}

/// "Today at 2:03 PM", "Yesterday at 9:15 AM" or "11/18/2026 2:03 PM".
pub fn format_timestamp(ms: u64, now_ms: u64) -> String {
    let Some(t) = local(ms) else {
        return String::new();
    };
    let now = local(now_ms).unwrap_or(t);
    let time = t.format(clock::hm());
    let days = now.date_naive().num_days_from_ce() - t.date_naive().num_days_from_ce();
    match days {
        0 => format!("Today at {time}"),
        1 => format!("Yesterday at {time}"),
        _ => format!("{} {time}", t.format("%m/%d/%Y")),
    }
}

/// Short time for continuation messages ("2:03 PM").
pub fn format_time(ms: u64) -> String {
    local(ms)
        .map(|t| t.format(clock::hm()).to_string())
        .unwrap_or_default()
}

/// Day divider label ("November 18, 2026").
pub fn format_day(ms: u64) -> String {
    local(ms)
        .map(|t| t.format("%B %-d, %Y").to_string())
        .unwrap_or_default()
}

pub fn same_day(a_ms: u64, b_ms: u64) -> bool {
    match (local(a_ms), local(b_ms)) {
        (Some(a), Some(b)) => a.date_naive() == b.date_naive(),
        _ => true,
    }
}

/// `<t:unix:style>` formatting (Discord styles t T d D f F R).
pub fn format_discord_timestamp(unix: i64, style: char, now_ms: u64) -> String {
    let Some(t) = Local.timestamp_opt(unix, 0).single() else {
        return format!("<t:{unix}>");
    };
    match style {
        't' => t.format(clock::hm()).to_string(),
        'T' => t.format(clock::hms()).to_string(),
        'd' => t.format("%m/%d/%Y").to_string(),
        'D' => t.format("%B %-d, %Y").to_string(),
        'F' => format!("{} {}", t.format("%A, %B %-d, %Y"), t.format(clock::hm())),
        'R' => relative(unix * 1000 - now_ms as i64),
        _ => format!("{} {}", t.format("%B %-d, %Y"), t.format(clock::hm())),
    }
}

fn relative(delta_ms: i64) -> String {
    let s = delta_ms.abs() / 1000;
    let (n, unit) = match s {
        0..=59 => (s, "second"),
        60..=3599 => (s / 60, "minute"),
        3600..=86_399 => (s / 3600, "hour"),
        86_400..=2_591_999 => (s / 86_400, "day"),
        2_592_000..=31_535_999 => (s / 2_592_000, "month"),
        _ => (s / 31_536_000, "year"),
    };
    let plural = if n == 1 { "" } else { "s" };
    if delta_ms >= 0 {
        format!("in {n} {unit}{plural}")
    } else {
        format!("{n} {unit}{plural} ago")
    }
}

fn local(ms: u64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms as i64).single()
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}

/// Message shortcut for buttons.
pub fn ui(u: Ui) -> Message {
    Message::Ui(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit() {
        assert_eq!(fit_size(800, 450, 400, 300), (400, 225));
        assert_eq!(fit_size(100, 100, 400, 300), (100, 100));
        assert_eq!(fit_size(300, 1200, 400, 300), (75, 300));
    }

    #[test]
    fn timestamps() {
        let now = 1_700_000_000_000;
        assert!(format_timestamp(now, now).starts_with("Today at "));
        assert!(format_timestamp(now - 86_400_000, now).starts_with("Yesterday at "));
        assert!(format_timestamp(now - 10 * 86_400_000, now).contains('/'));
        assert_eq!(relative(-5 * 60_000), "5 minutes ago");
        assert_eq!(relative(3_600_000), "in 1 hour");
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(500), "500 bytes");
        assert_eq!(human_size(1_532_211), "1.46 MB");
    }

    fn est(s: &str, size: f32, semibold: bool) -> f32 {
        s.chars().map(glyph_em).sum::<f32>() * size * if semibold { 1.06 } else { 1.0 }
    }

    #[test]
    fn plain_text_resolves_mentions_and_emoji() {
        let a = crate::app::App::boot(crate::config::Config::for_tests()).0;
        let s = plain_text(
            &a,
            "hi <@2> in <#132> <:ferris:1001> <a:x:5> <not a token> a < b",
        );
        assert!(s.starts_with("hi @"), "{s}");
        assert!(!s.contains("<@2>") && !s.contains("<#132>"), "{s}");
        assert!(s.contains(":ferris:") && s.contains(":x:"), "{s}");
        assert!(s.contains("<not a token>") && s.ends_with("a < b"), "{s}");
        assert_eq!(plain_text(&a, "no tokens"), "no tokens");
        assert_eq!(plain_text(&a, "unterminated <@2"), "unterminated <@2");
    }

    #[test]
    fn short_text_is_left_alone() {
        assert_eq!(ellipsize("Ferris", 120.0, 14.0, true), "Ferris");
        assert_eq!(ellipsize("", 10.0, 14.0, false), "");
    }

    #[test]
    fn long_text_gets_an_ellipsis_and_fits() {
        for (s, w) in [
            ("bronze age romantic", 88.0),
            ("Do Not Disturb", 60.0),
            ("an extremely long display name that goes on and on", 88.0),
            ("日本語の非常に長い表示名です", 88.0),
        ] {
            let out = ellipsize(s, w, 14.0, true);
            assert!(out.ends_with('\u{2026}'), "{out}");
            assert!(out.chars().count() < s.chars().count());
            assert!(est(&out, 14.0, true) <= w + 1.0, "{out} too wide for {w}");
        }
    }

    #[test]
    fn degenerate_widths_do_not_panic() {
        assert_eq!(ellipsize("abc", 0.0, 14.0, false), "\u{2026}");
        assert_eq!(ellipsize("abc", -5.0, 14.0, false), "\u{2026}");
    }

    #[test]
    fn user_panel_text_area_never_goes_negative() {
        assert!(sidebar::panel_text_width(240.0) > 60.0);
        assert_eq!(sidebar::panel_text_width(10.0), 0.0);
    }
}
