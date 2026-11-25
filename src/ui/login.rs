//! Login screen: token login on the left, QR login on the right, on a
//! blurple backdrop (original artwork-free recreation).

use iced::widget::image::FilterMethod;
use iced::widget::{Space, button, column, container, row, rule, text, text_input};
use iced::{Alignment, Background, Border, Color, Gradient, Length, Padding, Radians, gradient};

use super::El;
use super::icons::{Icon, icon};
use super::theme::{self, INTER_BOLD, INTER_SEMIBOLD};
use crate::app::{App, Message, Ui};
use crate::auth::{AuthState, QrState, WebLoginReason, WebLoginState};

pub fn view(app: &App) -> El<'_> {
    let t = &app.theme;
    let p = t.palette;
    let validating = matches!(app.auth, AuthState::Validating);
    let error = match &app.auth {
        AuthState::LoggedOut { error: Some(e) } => Some(e.clone()),
        _ => None,
    };

    let label = row![
        text("TOKEN")
            .size(12)
            .font(INTER_BOLD)
            .color(if error.is_some() {
                p.danger
            } else {
                p.text_secondary
            }),
        text(" *").size(12).font(INTER_BOLD).color(p.danger),
    ];
    let input = text_input("", &app.token_input)
        .secure(true)
        .on_input(|s| Message::Ui(Ui::TokenInput(s)))
        .on_submit(Message::Ui(Ui::SubmitToken))
        .size(16)
        .padding(10)
        .style(theme::input(p, p.bg_app));
    let mut left = column![
        text("Welcome back!")
            .size(24)
            .font(INTER_SEMIBOLD)
            .color(p.header_text)
            .width(Length::Fill)
            .align_x(Alignment::Center),
        text("We're so excited to see you again!")
            .size(16)
            .color(p.text_secondary)
            .width(Length::Fill)
            .align_x(Alignment::Center),
    ]
    .spacing(8)
    .width(Length::Fill);
    if !app.accounts.is_empty() && !validating {
        // Saved accounts (account switcher): one click logs back in.
        let mut saved = column![
            text("CHOOSE AN ACCOUNT")
                .size(12)
                .font(INTER_BOLD)
                .color(p.text_secondary)
        ]
        .spacing(4);
        for a in &app.accounts.list {
            saved = saved.push(super::popups::account_row(app, a, 32.0));
        }
        left = left.push(Space::new().height(8)).push(saved);
        left = left.push(
            text("Or log in to another account:")
                .size(12)
                .color(p.text_muted),
        );
    } else {
        left = left.push(Space::new().height(12));
    }
    left = left.push(label).push(input);
    if let Some(e) = error {
        left = left.push(text(e).size(12).color(p.danger));
    }
    left = left.push(
        text("Your token is stored only in the system keyring (Secret Service).")
            .size(12)
            .color(p.text_muted),
    );
    let login = button(
        container(
            text(if validating {
                "Logging in…"
            } else {
                "Log In"
            })
            .size(16)
            .font(INTER_SEMIBOLD)
            .color(p.accent_text),
        )
        .center_x(Length::Fill),
    )
    .width(Length::Fill)
    .padding(Padding::from([11, 16]))
    .style(theme::primary_button(p));
    let login = if validating {
        login
    } else {
        login.on_press(Message::Ui(Ui::SubmitToken))
    };
    left = left
        .push(Space::new().height(8))
        .push(login)
        .push(web_login(app))
        .push(
        row![
            icon(Icon::TriangleAlert, 16.0, p.mention_bar),
            text("Unofficial client. Using third-party clients breaks Discord's Terms of Service and can get your account banned.")
                .size(12)
                .color(p.text_secondary),
        ]
        .spacing(8)
        .padding(Padding::ZERO.top(12)),
    );

    let right = qr_panel(app);
    let divider = rule::vertical(1).style(move |_| rule::Style {
        color: Color::TRANSPARENT,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    });

    let card = container(
        row![left, container(divider).width(64).center_x(64), right]
            .align_y(Alignment::Center)
            .height(Length::Shrink),
    )
    .width(784)
    .padding(32)
    .style(move |_| iced::widget::container::Style {
        background: Some(Background::Color(p.bg_panel)),
        border: Border {
            radius: 5.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    let accent = p.accent;
    container(card)
        .center(Length::Fill)
        .style(move |_| iced::widget::container::Style {
            background: Some(Background::Gradient(Gradient::Linear(
                gradient::Linear::new(Radians(2.4))
                    .add_stop(
                        0.0,
                        Color {
                            r: accent.r * 0.55,
                            g: accent.g * 0.55,
                            b: accent.b * 0.75,
                            a: 1.0,
                        },
                    )
                    .add_stop(1.0, accent),
            ))),
            ..Default::default()
        })
        .into()
}

fn qr_panel(app: &App) -> El<'_> {
    let p = app.theme.palette;
    let (code, title, sub): (El<'_>, &str, String) = match &app.qr {
        Some(QrState::Ready { size, modules, .. }) => (
            qr_image(app, *size, modules),
            "Log in with QR Code",
            "Scan this with the Discord mobile app to log in instantly.".into(),
        ),
        Some(QrState::Scanned { username }) => (
            icon(Icon::UserRound, 64.0, p.text_secondary),
            "Check your phone!",
            format!("Logging in as {username}. Confirm on the mobile app."),
        ),
        Some(QrState::CaptchaRequired)
            if app.web_login.as_ref().is_some_and(WebLoginState::is_active) =>
        {
            (
                icon(Icon::ShieldCheck, 64.0, Color::BLACK),
                "Discord wants a captcha",
                "Finish logging in on Discord's own page: it shows the captcha there. \
                 Close it to get a new QR code."
                    .into(),
            )
        }
        Some(QrState::CaptchaRequired) => (
            button(
                column![
                    icon(Icon::QrCode, 48.0, Color::BLACK),
                    text("New QR code").size(14).color(Color::BLACK),
                ]
                .spacing(4)
                .align_x(Alignment::Center),
            )
            .on_press(Message::Ui(Ui::RetryQr))
            .style(theme::link_button(p))
            .into(),
            "Discord wants a captcha",
            "Discord only shows it on its own page: use \u{201c}Log in with Discord's page\u{201d}, \
             or try a new QR code."
                .into(),
        ),
        Some(QrState::Failed(e)) => (
            button(
                column![
                    icon(Icon::QrCode, 48.0, Color::BLACK),
                    text("Retry").size(14).color(Color::BLACK),
                ]
                .spacing(4)
                .align_x(Alignment::Center),
            )
            .on_press(Message::Ui(Ui::RetryQr))
            .style(theme::link_button(p))
            .into(),
            "QR login unavailable",
            e.clone(),
        ),
        Some(QrState::Connecting) | None => (
            text("Generating…").size(14).color(Color::BLACK).into(),
            "Log in with QR Code",
            "Scan this with the Discord mobile app to log in instantly.".into(),
        ),
    };
    column![
        container(code)
            .center(176)
            .style(theme::rounded_bg(Color::WHITE, 4.0)),
        text(title)
            .size(24)
            .font(INTER_SEMIBOLD)
            .color(p.header_text)
            .align_x(Alignment::Center),
        text(sub)
            .size(16)
            .color(p.text_secondary)
            .width(Length::Fill)
            .align_x(Alignment::Center),
    ]
    .spacing(12)
    .width(240)
    .align_x(Alignment::Center)
    .into()
}

/// "Log in with Discord's page": the button, or what is happening with it.
fn web_login(app: &App) -> El<'_> {
    let p = app.theme.palette;
    let busy = matches!(app.auth, AuthState::Validating);
    let open_button = |label: &'static str| {
        let b = button(
            row![
                icon(Icon::ExternalLink, 16.0, p.header_text),
                text(label)
                    .size(14)
                    .font(INTER_SEMIBOLD)
                    .color(p.header_text),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding::from([9, 16]))
        .style(theme::secondary_button(p));
        if busy {
            b
        } else {
            b.on_press(Message::Ui(Ui::OpenWebLogin))
        }
    };
    let note = |s: String, c: Color| text(s).size(12).color(c).width(Length::Fill);
    let body: El<'_> = match &app.web_login {
        None => column![
            open_button("Log in with Discord's page"),
            note(
                "Opens discord.com/login in a Ferrocord window (captcha, 2FA and QR work there). \
                 When you have logged in, Ferrocord keeps that session in the system keyring \
                 and closes the window."
                    .into(),
                p.text_muted,
            ),
        ]
        .spacing(6)
        .into(),
        Some(WebLoginState::Opening(_) | WebLoginState::Open(_)) => {
            let opening = matches!(app.web_login, Some(WebLoginState::Opening(_)));
            let captcha = matches!(
                app.web_login,
                Some(
                    WebLoginState::Opening(WebLoginReason::Captcha)
                        | WebLoginState::Open(WebLoginReason::Captcha)
                )
            );
            let headline = if opening {
                "Opening Discord's login page…"
            } else {
                "Discord's login page is open in its own window."
            };
            let detail = if captcha {
                "Discord asked for a captcha. Log in there (scan the code again or use your \
                 email and password) and solve the captcha; Ferrocord continues on its own."
            } else {
                "Log in there; Ferrocord takes over the session and closes the window on its own."
            };
            container(
                column![
                    row![
                        icon(Icon::ExternalLink, 16.0, p.header_text),
                        text(headline)
                            .size(14)
                            .font(INTER_SEMIBOLD)
                            .color(p.header_text),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    note(detail.into(), p.text_secondary),
                    button(text("Cancel").size(13).color(p.link))
                        .padding(0)
                        .style(theme::link_button(p))
                        .on_press(Message::Ui(Ui::CancelWebLogin)),
                ]
                .spacing(6),
            )
            .padding(12)
            .width(Length::Fill)
            .style(theme::rounded_bg(p.bg_app, 4.0))
            .into()
        }
        Some(WebLoginState::Failed(e)) => column![
            open_button("Try Discord's page again"),
            note(e.clone(), p.danger),
        ]
        .spacing(6)
        .into(),
    };
    container(body).padding(Padding::ZERO.top(8)).into()
}

fn qr_image<'a>(app: &App, size: usize, modules: &[bool]) -> El<'a> {
    iced::widget::image(app.qr_handle(size, modules))
        .width(160)
        .height(160)
        .filter_method(FilterMethod::Nearest)
        .into()
}

/// RGBA pixels of a QR matrix (black modules on white).
pub fn qr_pixels(modules: &[bool]) -> Vec<u8> {
    let mut px = Vec::with_capacity(modules.len() * 4);
    for m in modules {
        let v = if *m { 0u8 } else { 255u8 };
        px.extend_from_slice(&[v, v, v, 255]);
    }
    px
}
