//! Palette, typography, spacing and widget styles. The single source of
//! truth for the look: views never hard-code a colour or size.
//!
//! Values reproduce the layout and colour scheme of the official dark theme
//! (measured from public screenshots); see `docs/protocol.md` §UI reference.

use iced::gradient::Linear;
use iced::widget::{button, container, scrollable, slider, text_editor, text_input, toggler};
use iced::{Background, Border, Color, Font, Gradient, Radians, Shadow, Theme, font};

use crate::config::ThemeChoice;

const fn rgb(hex: u32) -> Color {
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(hex: u32, a: f32) -> Color {
    Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, a)
}

/// Converts a Discord integer colour (role, embed) to an iced colour.
pub fn from_int(c: u32) -> Color {
    rgb(c & 0xff_ffff)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg_app: Color,
    pub bg_sidebar: Color,
    pub bg_panel: Color,
    pub bg_elevated: Color,
    pub bg_hover: Color,
    pub bg_selected: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub danger: Color,
    pub success: Color,
    pub border: Color,
    /// Chat apps with bubbles only; Discord has none, so these are the
    /// message hover and mention highlight backgrounds.
    pub bubble_out: Color,
    pub bubble_in: Color,
    pub badge: Color,
    pub badge_muted: Color,
    // Discord-specific extras.
    pub text_normal: Color,
    pub header_text: Color,
    pub link: Color,
    pub mention_bg: Color,
    pub mention_text: Color,
    pub mention_bar: Color,
    pub composer_bg: Color,
    pub code_bg: Color,
    pub user_panel: Color,
    pub embed_bg: Color,
    pub spoiler: Color,
    pub channel_icon: Color,
    pub rail_pill: Color,
    /// Presence dots (online is `success`, do-not-disturb is `danger`).
    pub status_idle: Color,
    pub status_offline: Color,
}

impl Palette {
    pub const DARK: Palette = Palette {
        bg_app: rgb(0x1e1f22),
        bg_sidebar: rgb(0x2b2d31),
        bg_panel: rgb(0x313338),
        bg_elevated: rgb(0x111214),
        bg_hover: rgba(0x4e5058, 0.3),
        bg_selected: rgba(0x4e5058, 0.6),
        text_primary: rgb(0xf2f3f5),
        text_secondary: rgb(0xb5bac1),
        text_muted: rgb(0x949ba4),
        accent: rgb(0x5865f2),
        accent_text: rgb(0xffffff),
        danger: rgb(0xf23f43),
        success: rgb(0x23a559),
        border: rgb(0x1f2023),
        bubble_out: rgb(0x2e3035),
        bubble_in: rgba(0xf0b132, 0.08),
        badge: rgb(0xf23f43),
        badge_muted: rgb(0x80848e),
        text_normal: rgb(0xdbdee1),
        header_text: rgb(0xf2f3f5),
        link: rgb(0x00a8fc),
        mention_bg: rgba(0x5865f2, 0.3),
        mention_text: rgb(0xc9cdfb),
        mention_bar: rgb(0xf0b132),
        composer_bg: rgb(0x383a40),
        code_bg: rgb(0x2b2d31),
        user_panel: rgb(0x232428),
        embed_bg: rgb(0x2b2d31),
        spoiler: rgb(0x1e1f22),
        channel_icon: rgb(0x80848e),
        rail_pill: rgb(0xf2f3f5),
        status_idle: rgb(0xf0b232),
        status_offline: rgb(0x80848e),
    };

    pub const LIGHT: Palette = Palette {
        bg_app: rgb(0xe3e5e8),
        bg_sidebar: rgb(0xf2f3f5),
        bg_panel: rgb(0xffffff),
        bg_elevated: rgb(0xffffff),
        bg_hover: rgba(0x747f8d, 0.16),
        bg_selected: rgba(0x747f8d, 0.3),
        text_primary: rgb(0x060607),
        text_secondary: rgb(0x4e5058),
        text_muted: rgb(0x5c5e66),
        accent: rgb(0x5865f2),
        accent_text: rgb(0xffffff),
        danger: rgb(0xda373c),
        success: rgb(0x23a559),
        border: rgb(0xe1e2e4),
        bubble_out: rgb(0xf2f3f5),
        bubble_in: rgba(0xf0b132, 0.1),
        badge: rgb(0xda373c),
        badge_muted: rgb(0x80848e),
        text_normal: rgb(0x313338),
        header_text: rgb(0x060607),
        link: rgb(0x006ce7),
        mention_bg: rgba(0x5865f2, 0.15),
        mention_text: rgb(0x505cdc),
        mention_bar: rgb(0xc38d0c),
        composer_bg: rgb(0xebedef),
        code_bg: rgb(0xf2f3f5),
        user_panel: rgb(0xebedef),
        embed_bg: rgb(0xf2f3f5),
        spoiler: rgb(0xb9bbbe),
        channel_icon: rgb(0x6d6f78),
        rail_pill: rgb(0x060607),
        status_idle: rgb(0xf0b232),
        status_offline: rgb(0x80848e),
    };
}

impl Palette {
    /// "Ash": the dark theme with lighter greys.
    pub const ASH: Palette = Palette {
        bg_app: rgb(0x26272b),
        bg_sidebar: rgb(0x303136),
        bg_panel: rgb(0x37383e),
        bg_elevated: rgb(0x1c1d20),
        border: rgb(0x26272b),
        bubble_out: rgb(0x3c3d43),
        composer_bg: rgb(0x414249),
        code_bg: rgb(0x303136),
        user_panel: rgb(0x2a2b30),
        embed_bg: rgb(0x303136),
        spoiler: rgb(0x26272b),
        ..Palette::DARK
    };

    /// "Onyx": near-black surfaces.
    pub const ONYX: Palette = Palette {
        bg_app: rgb(0x070709),
        bg_sidebar: rgb(0x0f0f12),
        bg_panel: rgb(0x141417),
        bg_elevated: rgb(0x000000),
        border: rgb(0x1f1f23),
        bubble_out: rgb(0x1a1a1e),
        composer_bg: rgb(0x1d1d21),
        code_bg: rgb(0x1a1a1e),
        user_panel: rgb(0x0b0b0d),
        embed_bg: rgb(0x1a1a1e),
        spoiler: rgb(0x070709),
        ..Palette::DARK
    };

    /// Every colour with its saturation scaled by `f` (0 = grey, 1 = as
    /// is): the Accessibility "Saturation" slider.
    pub fn saturated(self, f: f32) -> Palette {
        if f >= 1.0 {
            return self;
        }
        let f = f.clamp(0.0, 1.0);
        let s = |c: Color| {
            // Rec. 709 luma, then mix toward it.
            let y = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
            Color {
                r: y + (c.r - y) * f,
                g: y + (c.g - y) * f,
                b: y + (c.b - y) * f,
                a: c.a,
            }
        };
        Palette {
            bg_app: s(self.bg_app),
            bg_sidebar: s(self.bg_sidebar),
            bg_panel: s(self.bg_panel),
            bg_elevated: s(self.bg_elevated),
            bg_hover: s(self.bg_hover),
            bg_selected: s(self.bg_selected),
            text_primary: s(self.text_primary),
            text_secondary: s(self.text_secondary),
            text_muted: s(self.text_muted),
            accent: s(self.accent),
            accent_text: s(self.accent_text),
            danger: s(self.danger),
            success: s(self.success),
            border: s(self.border),
            bubble_out: s(self.bubble_out),
            bubble_in: s(self.bubble_in),
            badge: s(self.badge),
            badge_muted: s(self.badge_muted),
            text_normal: s(self.text_normal),
            header_text: s(self.header_text),
            link: s(self.link),
            mention_bg: s(self.mention_bg),
            mention_text: s(self.mention_text),
            mention_bar: s(self.mention_bar),
            composer_bg: s(self.composer_bg),
            code_bg: s(self.code_bg),
            user_panel: s(self.user_panel),
            embed_bg: s(self.embed_bg),
            spoiler: s(self.spoiler),
            channel_icon: s(self.channel_icon),
            rail_pill: s(self.rail_pill),
            status_idle: s(self.status_idle),
            status_offline: s(self.status_offline),
        }
    }
}

/// Spacing scale, radii, widths and font sizes (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub rail_width: f32,
    pub rail_icon: f32,
    pub sidebar_width: f32,
    pub header_height: f32,
    pub user_panel_height: f32,
    pub avatar: f32,
    pub avatar_small: f32,
    pub radius_small: f32,
    pub radius: f32,
    pub radius_large: f32,
    pub space_xs: f32,
    pub space_s: f32,
    pub space_m: f32,
    pub space_l: f32,
    pub font_xs: f32,
    pub font_s: f32,
    pub font_m: f32,
    pub font_l: f32,
    pub font_xl: f32,
    pub font_xxl: f32,
    pub embed_max_width: f32,
}

impl Metrics {
    pub const DEFAULT: Metrics = Metrics {
        rail_width: 72.0,
        rail_icon: 48.0,
        sidebar_width: 240.0,
        header_height: 48.0,
        user_panel_height: 52.0,
        avatar: 40.0,
        avatar_small: 32.0,
        radius_small: 3.0,
        radius: 4.0,
        radius_large: 8.0,
        space_xs: 2.0,
        space_s: 4.0,
        space_m: 8.0,
        space_l: 16.0,
        font_xs: 12.0,
        font_s: 14.0,
        font_m: 16.0,
        font_l: 20.0,
        font_xl: 24.0,
        font_xxl: 32.0,
        embed_max_width: 520.0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FerroTheme {
    pub palette: Palette,
    pub metrics: Metrics,
    pub dark: bool,
}

impl FerroTheme {
    pub fn new(choice: ThemeChoice) -> Self {
        match choice {
            ThemeChoice::Dark => Self {
                palette: Palette::DARK,
                metrics: Metrics::DEFAULT,
                dark: true,
            },
            ThemeChoice::Light => Self {
                palette: Palette::LIGHT,
                metrics: Metrics::DEFAULT,
                dark: false,
            },
        }
    }

    /// The theme picked in Appearance, with the Accessibility saturation.
    pub fn from_settings(theme: crate::net::user_settings::Theme, saturation: f32) -> Self {
        use crate::net::user_settings::Theme as T;
        let (palette, dark) = match theme {
            T::Dark => (Palette::DARK, true),
            T::Light => (Palette::LIGHT, false),
            T::Darker => (Palette::ASH, true),
            T::Midnight => (Palette::ONYX, true),
        };
        Self {
            palette: palette.saturated(saturation),
            metrics: Metrics::DEFAULT,
            dark,
        }
    }

    /// iced base theme for widgets without a custom style.
    pub fn iced_theme(&self) -> Theme {
        let p = &self.palette;
        Theme::custom(
            if self.dark {
                "Ferrocord Dark"
            } else {
                "Ferrocord Light"
            },
            iced::theme::Palette {
                background: p.bg_panel,
                text: p.text_normal,
                primary: p.accent,
                success: p.success,
                warning: p.mention_bar,
                danger: p.danger,
            },
        )
    }
}

// -- typography ---------------------------------------------------------------

pub const INTER: Font = Font::with_name("Inter");
pub const INTER_MEDIUM: Font = Font {
    weight: font::Weight::Medium,
    ..INTER
};
pub const INTER_SEMIBOLD: Font = Font {
    weight: font::Weight::Semibold,
    ..INTER
};
pub const INTER_BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..INTER
};
pub const INTER_ITALIC: Font = Font {
    style: font::Style::Italic,
    ..INTER
};
pub const MONO: Font = Font::MONOSPACE;

/// Twemoji (graphics CC-BY 4.0 by Twitter and jdecked/twemoji, COLRv0 build
/// Apache-2.0 by Mozilla / Sable), renamed "Ferrocord Twemoji" so no system
/// face can shadow it. Loaded by `emoji_font::install`.
pub const TWEMOJI: &[u8] = include_bytes!("../../assets/fonts/Twemoji.ttf");

/// Embedded font files (SIL OFL 1.1).
pub const FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/Inter-Regular.otf"),
    include_bytes!("../../assets/fonts/Inter-Medium.otf"),
    include_bytes!("../../assets/fonts/Inter-SemiBold.otf"),
    include_bytes!("../../assets/fonts/Inter-Bold.otf"),
];

// -- styles -------------------------------------------------------------------

pub fn bg(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        ..Default::default()
    }
}

pub fn rounded_bg(color: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        border: Border {
            radius: radius.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

// Shadows are safe now: ferroiced's tiny-skia clips a quad's shadow to the
// damage region and counts it in damage tracking, so partial redraws no longer
// stack it up. No style here sets `container::Style::shadow` on purpose: the
// header keeps its gradient bottom edge as the house look, and the guard test
// below keeps it that way. Real shadows could replace that gradient later.

/// Background of a bar whose bottom edge darkens like the subtle drop shadow
/// of the official client's headers.
fn bar_with_bottom_edge(bg: Color) -> Background {
    let edge = Color {
        r: bg.r * 0.65,
        g: bg.g * 0.65,
        b: bg.b * 0.65,
        a: bg.a,
    };
    Background::Gradient(Gradient::Linear(
        // Pointing down: offset 0 is the top edge, 1 the bottom edge.
        Linear::new(Radians(std::f32::consts::PI))
            .add_stop(0.0, bg)
            .add_stop(0.97, bg)
            .add_stop(1.0, edge),
    ))
}

/// Header bar with the subtle bottom shadow of the official client.
pub fn header(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(bar_with_bottom_edge(p.bg_panel)),
        ..Default::default()
    }
}

pub fn sidebar_header(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(bar_with_bottom_edge(p.bg_sidebar)),
        ..Default::default()
    }
}

/// A transparent button that shows `hover` on hover and `selected` when
/// selected (channel rows, DM rows).
pub fn list_row(p: Palette, selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match (selected, status) {
            (true, _) => Some(Background::Color(p.bg_selected)),
            (false, button::Status::Hovered | button::Status::Pressed) => {
                Some(Background::Color(p.bg_hover))
            }
            _ => None,
        };
        button::Style {
            background,
            text_color: p.text_muted,
            border: Border {
                radius: Metrics::DEFAULT.radius.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Server icon on the rail: a circle that morphs into a rounded square on
/// hover / selection, and takes the accent colour.
pub fn rail_icon(
    p: Palette,
    selected: bool,
    has_image: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let active =
            selected || matches!(status, button::Status::Hovered | button::Status::Pressed);
        let size = Metrics::DEFAULT.rail_icon;
        let radius = if active { 16.0 } else { size / 2.0 };
        let background = if has_image {
            None
        } else if active {
            Some(Background::Color(p.accent))
        } else {
            Some(Background::Color(p.bg_panel))
        };
        button::Style {
            background,
            text_color: if active { p.accent_text } else { p.text_normal },
            border: Border {
                radius: radius.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

pub fn rail_add(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(if hovered {
                p.success
            } else {
                p.bg_panel
            })),
            text_color: if hovered { p.accent_text } else { p.success },
            border: Border {
                radius: (if hovered { 16.0 } else { 24.0 }).into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

pub fn primary_button(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let base = p.accent;
        let bg = match status {
            button::Status::Hovered => Color {
                r: base.r * 0.85,
                g: base.g * 0.85,
                b: base.b * 0.85,
                a: 1.0,
            },
            button::Status::Pressed => Color {
                r: base.r * 0.75,
                g: base.g * 0.75,
                b: base.b * 0.75,
                a: 1.0,
            },
            button::Status::Disabled => Color { a: 0.5, ..base },
            button::Status::Active => base,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.accent_text,
            border: Border {
                radius: Metrics::DEFAULT.radius.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Grey filled button (secondary actions, like Discord's "secondary").
pub fn secondary_button(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => p.bg_selected,
            button::Status::Disabled => Color {
                a: 0.5,
                ..p.bg_hover
            },
            button::Status::Active => p.bg_hover,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.header_text,
            border: Border {
                radius: Metrics::DEFAULT.radius.into(),
                width: 1.0,
                color: p.border,
            },
            ..Default::default()
        }
    }
}

/// Red filled button (destructive actions).
pub fn danger_button(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let k = if hovered { 0.85 } else { 1.0 };
        button::Style {
            background: Some(Background::Color(Color {
                r: p.danger.r * k,
                g: p.danger.g * k,
                b: p.danger.b * k,
                a: 1.0,
            })),
            text_color: p.accent_text,
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Borderless icon button (header, user panel).
pub fn icon_button(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then_some(Background::Color(p.bg_hover)),
        text_color: p.text_secondary,
        border: Border {
            radius: Metrics::DEFAULT.radius.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Switch used by the settings dialog: green when on, like the official
/// client.
pub fn switch(p: Palette) -> impl Fn(&Theme, toggler::Status) -> toggler::Style {
    move |_, status| {
        let (on, hovered) = match status {
            toggler::Status::Active { is_toggled } => (is_toggled, false),
            toggler::Status::Hovered { is_toggled } => (is_toggled, true),
            toggler::Status::Disabled { is_toggled } => (is_toggled, false),
        };
        let track = if on { p.success } else { p.status_offline };
        toggler::Style {
            background: Background::Color(if hovered {
                Color { a: 0.9, ..track }
            } else {
                track
            }),
            background_border_width: 0.0,
            background_border_color: Color::TRANSPARENT,
            foreground: Background::Color(Color::WHITE),
            foreground_border_width: 0.0,
            foreground_border_color: Color::TRANSPARENT,
            text_color: None,
            border_radius: None,
            padding_ratio: 0.2,
        }
    }
}

/// Settings screen layout (logical px), as in the official client.
pub const SETTINGS_SIDEBAR_W: f32 = 218.0;
pub const SETTINGS_CONTENT_MAX_W: f32 = 740.0;
pub const SETTINGS_SWATCH: f32 = 60.0;

/// Radio buttons of the settings screen.
pub fn radio(
    p: Palette,
) -> impl Fn(&Theme, iced::widget::radio::Status) -> iced::widget::radio::Style {
    use iced::widget::radio::Status;
    move |_, status| {
        let (selected, hovered) = match status {
            Status::Active { is_selected } => (is_selected, false),
            Status::Hovered { is_selected } => (is_selected, true),
        };
        iced::widget::radio::Style {
            background: Background::Color(Color::TRANSPARENT),
            dot_color: p.accent,
            border_width: 2.0,
            border_color: if selected {
                p.accent
            } else if hovered {
                p.text_normal
            } else {
                p.text_muted
            },
            text_color: Some(p.text_normal),
        }
    }
}

/// A theme swatch in Appearance: a circle of `fill`, ringed when picked.
pub fn swatch(
    fill: Color,
    ring: Color,
    selected: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: Some(Background::Color(fill)),
        border: Border {
            radius: (SETTINGS_SWATCH / 2.0).into(),
            width: if selected {
                3.0
            } else if matches!(status, button::Status::Hovered) {
                2.0
            } else {
                1.0
            },
            color: if selected {
                ring
            } else {
                Color { a: 0.4, ..ring }
            },
        },
        ..Default::default()
    }
}

/// One of the deny / neutral / allow buttons of a channel permission:
/// filled with `active` when chosen.
pub fn tri_button(
    p: Palette,
    active: Option<Color>,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: Some(Background::Color(match (active, status) {
            (Some(c), _) => c,
            (None, button::Status::Hovered) => p.bg_hover,
            (None, _) => p.bg_app,
        })),
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Bordered panel inside a settings page (account card, keybind table).
pub fn settings_card(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.bg_sidebar)),
        border: Border {
            radius: Metrics::DEFAULT.radius_large.into(),
            width: 1.0,
            color: p.border,
        },
        ..Default::default()
    }
}

/// Colour of a presence dot.
pub fn presence_color(p: &Palette, status: crate::net::model::Presence) -> Color {
    use crate::net::model::Presence::*;
    match status {
        Online => p.success,
        Idle => p.status_idle,
        Dnd => p.danger,
        Offline => p.status_offline,
    }
}

/// Floating menu / popover (context menu, emoji picker, status menu).
pub fn popup(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.bg_elevated)),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: p.border,
        },
        ..Default::default()
    }
}

/// Row in a popup menu: accent fill on hover; `danger` rows turn red.
pub fn menu_item(p: Palette, danger: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, fg) = match (hovered, danger) {
            (true, true) => (Some(Background::Color(p.danger)), p.accent_text),
            (true, false) => (Some(Background::Color(p.accent)), p.accent_text),
            (false, true) => (None, p.danger),
            (false, false) => (None, p.text_normal),
        };
        button::Style {
            background: bg,
            text_color: fg,
            border: Border {
                radius: Metrics::DEFAULT.radius.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Reaction pill: tinted with the accent when the user reacted.
pub fn reaction_pill(p: Palette, me: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, border) = if me {
            (
                Color {
                    a: 0.15,
                    ..p.accent
                },
                Color { a: 1.0, ..p.accent },
            )
        } else if hovered {
            (p.bg_selected, p.text_muted)
        } else {
            (p.bg_sidebar, Color::TRANSPARENT)
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.text_normal,
            border: Border {
                radius: 8.0.into(),
                width: 1.0,
                color: border,
            },
            ..Default::default()
        }
    }
}

/// Text-like button (links, spoiler reveal).
/// Outlined button on a coloured notice bar (the update banner), like the
/// official client's notice buttons.
pub fn notice_button(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(Color {
                a: 0.15,
                ..p.accent_text
            })),
            text_color: p.accent_text,
            border: Border {
                radius: Metrics::DEFAULT.radius_small.into(),
                width: 1.0,
                color: p.accent_text,
            },
            ..Default::default()
        }
    }
}

/// Height of the update banner's buttons and of the bar itself.
pub const NOTICE_BUTTON_PAD: [u16; 2] = [2, 10];

pub fn link_button(_p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style {
        background: None,
        ..Default::default()
    }
}

pub fn input(p: Palette, bg: Color) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, _| text_input::Style {
        background: Background::Color(bg),
        border: Border {
            radius: Metrics::DEFAULT.radius.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        icon: p.text_muted,
        placeholder: p.text_muted,
        value: p.text_normal,
        selection: Color { a: 0.4, ..p.accent },
    }
}

/// Multi-line editor with the same look as [`input`].
pub fn editor(p: Palette, bg: Color) -> impl Fn(&Theme, text_editor::Status) -> text_editor::Style {
    move |_, _| text_editor::Style {
        background: Background::Color(bg),
        border: Border {
            radius: Metrics::DEFAULT.radius.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        placeholder: p.text_muted,
        value: p.text_normal,
        selection: Color { a: 0.4, ..p.accent },
    }
}

pub fn thin_scrollbar(
    p: Palette,
    track: bool,
) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |_, _| {
        let rail = scrollable::Rail {
            background: track.then_some(Background::Color(p.bg_sidebar)),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            scroller: scrollable::Scroller {
                background: Background::Color(p.bg_app),
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
            },
        };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable::AutoScroll {
                background: Background::Color(p.bg_elevated),
                border: Border::default(),
                shadow: Shadow::default(),
                icon: p.text_normal,
            },
        }
    }
}

/// Avatar background colours for users without an avatar image (original
/// palette, chosen by user id).
pub fn default_avatar_color(id: u64) -> Color {
    const COLORS: [u32; 6] = [0x5865f2, 0x757e8a, 0x3ba55c, 0xfaa61a, 0xed4245, 0xeb459f];
    rgb(COLORS[((id >> 22) % 6) as usize].max(COLORS[(id % 6) as usize]) & 0xffffff)
}

// -- Video player ------------------------------------------------------------
//
// The player chrome is dark in both themes (it sits on top of a picture), so
// these do not depend on the palette.

/// Behind the picture of a video (letterbox bars).
pub fn video_bg() -> Color {
    rgb(0x000000)
}

/// Translucent black under the controls and over a paused picture.
pub fn video_scrim() -> Color {
    rgba(0x000000, 0.62)
}

/// Dark margin around the full-window image viewer.
pub fn lightbox_scrim() -> Color {
    rgba(0x000000, 0.85)
}

/// Text and icons on the player chrome.
pub fn video_fg() -> Color {
    rgb(0xffffff)
}

/// Dimmed text on the player chrome.
pub fn video_fg_dim() -> Color {
    rgba(0xffffff, 0.72)
}

/// The round play button over a poster.
pub fn video_play_button(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(if hovered {
                p.accent
            } else {
                video_scrim()
            })),
            text_color: video_fg(),
            border: Border {
                radius: 32.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// A button that is invisible until hovered: the click target over a picture.
pub fn video_surface() -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: matches!(status, button::Status::Hovered)
            .then_some(Background::Color(rgba(0x000000, 0.12))),
        text_color: video_fg(),
        ..Default::default()
    }
}

/// Small icon buttons of the control bar.
pub fn video_control(_p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then_some(Background::Color(rgba(0xffffff, 0.16))),
        text_color: video_fg(),
        border: Border {
            radius: Metrics::DEFAULT.radius_small.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Seek and volume sliders: accent for the played part, a round handle.
pub fn video_slider(p: Palette) -> impl Fn(&Theme, slider::Status) -> slider::Style {
    move |_, status| {
        let handle = match status {
            slider::Status::Hovered | slider::Status::Dragged => 7.0,
            slider::Status::Active => 5.0,
        };
        slider::Style {
            rail: slider::Rail {
                backgrounds: (
                    Background::Color(p.accent),
                    Background::Color(rgba(0xffffff, 0.3)),
                ),
                width: 4.0,
                border: Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: handle },
                background: Background::Color(video_fg()),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        }
    }
}

/// The save star over a GIF picker tile: a dark round chip.
pub fn gif_star(_p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(rgba(
                0x000000,
                if hovered { 0.8 } else { 0.55 },
            ))),
            text_color: video_fg(),
            border: Border {
                radius: Metrics::DEFAULT.radius_small.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Star colour of a saved GIF (Discord's gold).
pub fn gif_star_saved(p: Palette) -> Color {
    p.mention_bar
}

/// The dim layer under a GIF picker home tile's label.
pub fn gif_tile_scrim() -> Color {
    rgba(0x000000, 0.5)
}

/// Row tint of a message a plugin highlighted (lighter than a mention).
pub fn plugin_highlight(p: Palette) -> Color {
    Color {
        a: 0.10,
        ..p.accent
    }
}

/// Small pill a plugin attached to a message.
pub fn plugin_badge(p: Palette, accent: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(if accent {
            Color {
                a: 0.25,
                ..p.accent
            }
        } else {
            p.bg_hover
        })),
        text_color: Some(if accent {
            p.header_text
        } else {
            p.text_secondary
        }),
        border: Border {
            radius: 4.0.into(),
            width: 1.0,
            color: p.border,
        },
        ..Default::default()
    }
}

/// Bordered card of one plugin on the Plugins page.
pub fn plugin_card(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.bg_sidebar)),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: p.border,
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_styles_have_no_shadow() {
        // Shadows are clipped by ferroiced now, but these styles stay
        // shadowless by design: see the note above `bar_with_bottom_edge`.
        let p = Palette::DARK;
        let t = Theme::Dark;
        for style in [header(p)(&t), sidebar_header(p)(&t), popup(p)(&t)] {
            assert_eq!(style.shadow, Shadow::default());
        }
        assert!(matches!(
            header(p)(&t).background,
            Some(Background::Gradient(_))
        ));
    }

    #[test]
    fn presence_colours_are_distinct() {
        use crate::net::model::Presence::*;
        let p = Palette::DARK;
        let cs = [Online, Idle, Dnd, Offline].map(|s| presence_color(&p, s));
        for i in 0..cs.len() {
            for j in i + 1..cs.len() {
                assert_ne!(cs[i], cs[j]);
            }
        }
    }

    #[test]
    fn int_colours() {
        assert_eq!(from_int(0xff0000), Color::from_rgb8(255, 0, 0));
    }

    #[test]
    fn settings_themes_and_saturation() {
        use crate::net::user_settings::Theme as T;
        let all = T::ALL.map(|t| FerroTheme::from_settings(t, 1.0));
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i].palette.bg_panel, all[j].palette.bg_panel);
            }
        }
        assert!(!FerroTheme::from_settings(T::Light, 1.0).dark);
        let grey = FerroTheme::from_settings(T::Dark, 0.0).palette.accent;
        assert!((grey.r - grey.g).abs() < 1e-6 && (grey.g - grey.b).abs() < 1e-6);
        assert_eq!(
            FerroTheme::from_settings(T::Dark, 1.0).palette,
            Palette::DARK
        );
    }

    #[test]
    fn themes_differ() {
        let d = FerroTheme::new(ThemeChoice::Dark);
        let l = FerroTheme::new(ThemeChoice::Light);
        assert!(d.dark && !l.dark);
        assert_ne!(d.palette.bg_panel, l.palette.bg_panel);
    }
}
