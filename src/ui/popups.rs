//! Popups drawn over the main view: message context menu, emoji, GIF and
//! sticker pickers, delete confirmation and the status menu.

use iced::widget::{
    Space, button, column, container, image, mouse_area, opaque, row, scrollable, text, text_input,
};
use iced::{Alignment, Color, Length, Padding, Point, Size};

use super::emoji_font::emoji_text;
use super::icons::{Icon, icon};
use super::theme::{self, INTER, INTER_MEDIUM, INTER_SEMIBOLD};
use super::{El, avatar, ui};
use crate::app::actions::OpenPicker;
use crate::app::emoji::{self, CATEGORIES, PickerTarget, Section};
use crate::app::{App, Message, Ui, emoji_url, input};
use crate::net::model::*;

const MENU_W: f32 = 220.0;
const PICKER_W: f32 = 408.0;
const PICKER_SIDEBAR_W: f32 = 44.0;
const PICKER_H: f32 = 400.0;
const COLUMNS: usize = 9;

/// Number of popup slots [`layers`] always returns.
pub const SLOTS: usize = 10;

/// The popup layers, bottom to top, always exactly [`SLOTS`] long: a closed
/// popup is an empty placeholder so the widget tree keeps its shape (see
/// `ui::view`).
pub fn layers(app: &App) -> Vec<El<'_>> {
    let window = input::window_size();
    let status = if app.status_menu {
        let size = Size::new(240.0, status_menu_height(app));
        let at = Point::new(
            8.0,
            window.height - app.theme.metrics.user_panel_height - size.height - 8.0,
        );
        popup_layer(app, status_menu(app), at, Ui::ToggleStatusMenu)
    } else {
        super::empty()
    };
    let menu = match &app.menu {
        Some(mn) => {
            let h = menu_height(app, mn.message);
            let at = input::clamp_popup(mn.at, Size::new(MENU_W, h), window);
            popup_layer(app, context_menu(app, mn.message), at, Ui::CloseMenu)
        }
        None => super::empty(),
    };
    let picker = match &app.emoji_picker {
        Some(open) => {
            let at = picker_position(open, window);
            popup_layer(app, emoji_picker(app, open), at, Ui::CloseEmojiPicker)
        }
        None => super::empty(),
    };
    let gif = match &app.gif_picker {
        Some(gp) => {
            let size = Size::new(GIF_PICKER_W, GIF_PICKER_H);
            // Above and to the left of the composer button, like the emoji one.
            let at = input::clamp_popup(
                Point::new(gp.at.x - size.width + 16.0, gp.at.y - size.height - 12.0),
                size,
                window,
            );
            popup_layer(app, gif_picker(app, gp), at, Ui::CloseGifPicker)
        }
        None => super::empty(),
    };
    let sticker = match &app.sticker_picker {
        Some(sp) => {
            let size = Size::new(STICKER_PICKER_W, STICKER_PICKER_H);
            // Above and to the left of the composer button, like the emoji one.
            let at = input::clamp_popup(
                Point::new(sp.at.x - size.width + 16.0, sp.at.y - size.height - 12.0),
                size,
                window,
            );
            popup_layer(app, sticker_picker(app, sp), at, Ui::CloseStickerPicker)
        }
        None => super::empty(),
    };
    let delete = match app.pending_delete {
        Some((c, m)) => delete_dialog(app, c, m),
        None => super::empty(),
    };
    let popover = match app.panels.open {
        Some(kind) => {
            let size = super::popovers::size_of(app, kind);
            let at = super::popovers::position(app, size);
            // The backdrop starts under the header so its buttons stay
            // clickable (switching from one popover to another).
            popup_layer_from(
                app,
                super::popovers::view(app, kind),
                at,
                Ui::ClosePopover,
                app.theme.metrics.header_height,
            )
        }
        None => super::empty(),
    };
    let rail_menu = match super::rail::menu(app) {
        Some((card, at)) => popup_layer(app, card, at, Ui::CloseRailMenu),
        None => super::empty(),
    };
    let ctx = match super::context_menu::view(app) {
        Some((card, at)) => popup_layer(app, card, at, Ui::ContextClose),
        None => super::empty(),
    };
    let profile = match super::profile::popout(app) {
        Some((card, at)) => popup_layer(app, card, at, Ui::CloseProfile),
        None => super::empty(),
    };
    vec![
        status, menu, picker, gif, sticker, delete, popover, rail_menu, ctx, profile,
    ]
}

const GIF_PICKER_W: f32 = crate::app::gif_anim::PICKER_W;
const GIF_PICKER_H: f32 = 440.0;

fn gif_picker<'a>(app: &'a App, gp: &'a crate::app::gifs::GifPicker) -> El<'a> {
    use crate::app::gifs::GifPage;
    let p = app.theme.palette;
    let search = text_input("Search Tenor", &gp.query)
        .id(iced::widget::Id::new("gif-search"))
        .on_input(|s| Message::Ui(Ui::GifQuery(s)))
        .size(14)
        .font(INTER)
        .padding(Padding::from([8, 10]))
        .style(theme::input(p, p.bg_app));
    let page = if gp.searching() { None } else { Some(gp.page) };
    // A page of the home screen has a back arrow, like the official picker.
    let header: El<'a> = match page {
        Some(GifPage::Favorites | GifPage::Trending) => row![
            button(icon(Icon::ArrowLeft, 20.0, p.text_secondary))
                .padding(6)
                .on_press(ui(Ui::GifPage(GifPage::Home)))
                .style(theme::icon_button(p)),
            search,
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into(),
        _ => search.into(),
    };
    let title = match page {
        None => Some("Search results"),
        Some(GifPage::Home) => None,
        Some(GifPage::Favorites) => Some("Favorites"),
        Some(GifPage::Trending) => Some("Trending GIFs"),
    };

    use crate::app::gif_anim::{GAP, GRID_ID, TILE_W};
    let tile_w = TILE_W;
    let scroll = |content: El<'a>| -> El<'a> {
        super::smooth_scroll::smooth(
            scrollable(container(content).padding(Padding::from([0, 8])))
                .id(iced::widget::Id::new(GRID_ID))
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(4).scroller_width(4),
                ))
                .on_scroll(|v| Message::Ui(Ui::GifScrolled(v.absolute_offset().y)))
                .style(theme::thin_scrollbar(p, false))
                .height(Length::Fill),
        )
        .into()
    };
    let favorites = app.gif_favorites.items();
    let body: El<'a> = match page {
        Some(GifPage::Home) => {
            let fav_bg = favorites.first().and_then(|g| app.gif_thumb(&g.url));
            let trend_bg = gp.results.first().and_then(|g| app.gif_thumb(&g.url));
            scroll(
                row![
                    gif_home_tile(
                        app,
                        tile_w,
                        fav_bg,
                        Icon::Star,
                        "Favorites",
                        GifPage::Favorites
                    ),
                    gif_home_tile(
                        app,
                        tile_w,
                        trend_bg,
                        Icon::Gif,
                        "Trending GIFs",
                        GifPage::Trending
                    ),
                ]
                .spacing(GAP)
                .into(),
            )
        }
        Some(GifPage::Favorites) if favorites.is_empty() => notice(
            p,
            if app.gif_favorites.list.is_none() {
                "Loading…"
            } else {
                "Click the star on a GIF to save it here."
            },
        ),
        Some(GifPage::Favorites) => scroll(gif_grid(app, &favorites, tile_w, GAP)),
        _ if gp.loading && gp.results.is_empty() => notice(p, "Loading…"),
        _ if gp.error.is_some() => notice(p, gp.error.as_deref().unwrap_or_default()),
        _ if gp.results.is_empty() => notice(p, "No GIFs found"),
        _ => scroll(gif_grid(app, &gp.results, tile_w, GAP)),
    };
    let title: El<'a> = match title {
        Some(t) => container(
            text(t.to_uppercase())
                .size(12)
                .font(INTER_SEMIBOLD)
                .color(p.text_muted),
        )
        .padding(Padding::from([0, 12]).bottom(4))
        .into(),
        None => Space::new().into(),
    };
    container(column![container(header).padding(8), title, body].spacing(4))
        .width(GIF_PICKER_W)
        .height(GIF_PICKER_H)
        .style(theme::popup(p))
        .into()
}

/// A home-screen tile: the newest GIF of the page as a dimmed background
/// with the page name on top.
fn gif_home_tile<'a>(
    app: &'a App,
    w: f32,
    bg: Option<&'a iced::widget::image::Handle>,
    glyph: Icon,
    label: &'static str,
    page: crate::app::gifs::GifPage,
) -> El<'a> {
    let p = app.theme.palette;
    const H: f32 = 110.0;
    let back: El<'a> = match bg {
        Some(h) => image(h.clone())
            .width(w)
            .height(H)
            .content_fit(iced::ContentFit::Cover)
            .into(),
        None => Space::new().width(w).height(H).into(),
    };
    let face = container(
        row![
            icon(glyph, 18.0, theme::video_fg()),
            text(label)
                .size(16)
                .font(INTER_SEMIBOLD)
                .color(theme::video_fg()),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .center_x(w)
    .center_y(H)
    .style(theme::rounded_bg(theme::gif_tile_scrim(), 4.0));
    button(iced::widget::stack![back, face])
        .padding(0)
        .on_press(ui(Ui::GifPage(page)))
        .style(theme::icon_button(p))
        .into()
}

/// Two columns of GIFs, each going to the shorter one (masonry), each with
/// a star that saves or unsaves it.
fn gif_grid<'a>(app: &'a App, gifs: &[Gif], tile_w: f32, gap: f32) -> El<'a> {
    let p = app.theme.palette;
    let mut cols: [Vec<El<'a>>; 2] = [Vec::new(), Vec::new()];
    let tiles = crate::app::gif_anim::masonry(gifs, tile_w, gap);
    for (g, t) in gifs.iter().zip(tiles) {
        let h = t.h;
        // The animation once decoded (frames made in `update`, played by
        // the widget itself), the still until then.
        let anim = app.gif_anim(&g.url);
        let face: El<'a> = match (anim, app.gif_thumb(&g.url)) {
            (Some(a), _) => super::anim_image::anim_image(
                a.clone(),
                app.anim.epoch,
                app.picker_playing(&g.url),
                tile_w,
                h,
            )
            .content_fit(iced::ContentFit::Cover)
            .into(),
            (None, Some(handle)) => image(handle.clone())
                .width(tile_w)
                .height(h)
                .content_fit(iced::ContentFit::Cover)
                .into(),
            (None, None) => container(icon(Icon::Image, 24.0, p.text_muted))
                .center_x(tile_w)
                .center_y(h)
                .style(theme::rounded_bg(p.bg_app, 4.0))
                .into(),
        };
        // Hover plays the tile when GIF autoplay is off.
        let tile = iced::widget::mouse_area(
            button(face)
                .padding(0)
                .on_press(ui(Ui::PickGif(g.url.clone())))
                .style(theme::icon_button(p)),
        )
        .on_enter(ui(Ui::GifHoverEnter(g.url.clone())))
        .on_exit(ui(Ui::GifHoverLeave(g.url.clone())));
        let saved = app.gif_favorites.contains(&g.url);
        let star = button(icon(
            if saved { Icon::StarFilled } else { Icon::Star },
            18.0,
            if saved {
                theme::gif_star_saved(p)
            } else {
                theme::video_fg()
            },
        ))
        .padding(5)
        .on_press(ui(Ui::ToggleGifFavorite(Box::new(g.clone()))))
        .style(theme::gif_star(p));
        let cell = iced::widget::stack![
            tile,
            container(star)
                .width(tile_w)
                .align_x(iced::alignment::Horizontal::Right)
                .padding(4),
        ];
        cols[t.col].push(cell.into());
    }
    let [left, right] = cols;
    row![
        iced::widget::Column::with_children(left)
            .spacing(gap)
            .width(tile_w),
        iced::widget::Column::with_children(right)
            .spacing(gap)
            .width(tile_w),
    ]
    .spacing(gap)
    .into()
}

fn notice<'a>(p: theme::Palette, msg: &str) -> El<'a> {
    container(text(msg.to_owned()).size(14).color(p.text_muted))
        .padding(24)
        .center_x(Length::Fill)
        .into()
}

const STICKER_PICKER_W: f32 = 424.0;
const STICKER_PICKER_H: f32 = 440.0;
const STICKER_COLUMNS: usize = 4;

fn sticker_picker<'a>(app: &'a App, sp: &'a crate::app::stickers::StickerPicker) -> El<'a> {
    let p = app.theme.palette;
    let search = text_input("Find the perfect sticker", &sp.query)
        .id(iced::widget::Id::new("sticker-search"))
        .on_input(|s| Message::Ui(Ui::StickerQuery(s)))
        .size(14)
        .font(INTER)
        .padding(Padding::from([8, 10]))
        .style(theme::input(p, p.bg_app));
    let current = app
        .open_channel()
        .and_then(|c| app.chat.channels.get(&c))
        .and_then(|c| c.guild_id);
    let sections = crate::app::stickers::sections(&app.chat.guilds, current, &sp.query);
    let access = app.nitro_access();
    // Each cell: the sticker plus 4px of button padding on both sides.
    let cell = (STICKER_PICKER_W - 16.0 - 8.0) / STICKER_COLUMNS as f32 - 8.0;
    let mut list = column![].spacing(8).padding(Padding::from([0, 8]));
    for (g, stickers) in &sections {
        list = list.push(
            text(g.name.to_uppercase())
                .size(12)
                .font(INTER_SEMIBOLD)
                .color(p.text_muted),
        );
        let mut grid = column![].spacing(4);
        for chunk in stickers.chunks(STICKER_COLUMNS) {
            let mut line = row![].spacing(4);
            for s in chunk {
                let face = super::chat::sticker(app, s, cell);
                // Nitro-locked here: a lock badge, like the official picker.
                let face: El<'a> = if access.sticker(g, s).locked() {
                    iced::widget::stack![
                        face,
                        container(
                            container(icon(Icon::Lock, 12.0, p.text_normal))
                                .padding(4)
                                .style(theme::popup(p))
                        )
                        .align_right(cell)
                        .align_bottom(cell),
                    ]
                    .into()
                } else {
                    face
                };
                line = line.push(
                    button(face)
                        .padding(4)
                        .on_press(ui(Ui::PickSticker(s.id)))
                        .style(theme::icon_button(p)),
                );
            }
            grid = grid.push(line);
        }
        list = list.push(grid);
    }
    let body: El<'a> = if sections.is_empty() {
        let msg = if sp.query.trim().is_empty() {
            "None of your servers have stickers yet."
        } else {
            "No stickers found"
        };
        container(text(msg).size(14).color(p.text_muted))
            .padding(24)
            .center_x(Length::Fill)
            .into()
    } else {
        super::smooth_scroll::smooth(
            scrollable(list)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(4).scroller_width(4),
                ))
                .style(theme::thin_scrollbar(p, false))
                .height(Length::Fill),
        )
        .into()
    };
    // Last child, so opening it keeps the list's scroll state.
    let footer: El<'a> = match sp
        .locked
        .and_then(|id| crate::app::stickers::find(&app.chat.guilds, id))
    {
        Some(s) => {
            let (why, send): (&str, El<'a>) = if s.format == StickerFormat::Lottie {
                (
                    "Stickers from other servers need Nitro, and animated vector (Lottie) stickers cannot be sent as an image.",
                    Space::new().into(),
                )
            } else {
                (
                    "Stickers from other servers need Nitro here. You can send it as an image instead.",
                    button(text("Send as image").size(14).font(INTER_MEDIUM))
                        .padding(Padding::from([6, 12]))
                        .on_press(ui(Ui::SendLockedAsImage))
                        .style(theme::primary_button(p))
                        .into(),
                )
            };
            lock_footer(
                app,
                super::chat::sticker(app, s, 40.0),
                s.name.clone(),
                why,
                send,
            )
        }
        None => Space::new().into(),
    };
    container(column![container(search).padding(8), body, footer].spacing(4))
        .width(STICKER_PICKER_W)
        .height(STICKER_PICKER_H)
        .style(theme::popup(p))
        .into()
}

fn picker_position(open: &OpenPicker, window: Size) -> Point {
    let size = Size::new(PICKER_W, PICKER_H);
    let at = match open.picker.target {
        // Opened from the composer: above and to the left of the button.
        PickerTarget::Composer => Point::new(
            open.at.x - size.width + 16.0,
            open.at.y - size.height - 12.0,
        ),
        // Opened from a message toolbar: below-left of the button.
        PickerTarget::Reaction { .. } => {
            Point::new(open.at.x - size.width + 16.0, open.at.y + 16.0)
        }
    };
    input::clamp_popup(at, size, window)
}

/// A click-away backdrop plus the popup at `at`.
pub(crate) fn popup_layer<'a>(app: &'a App, card: El<'a>, at: Point, close: Ui) -> El<'a> {
    popup_layer_from(app, card, at, close, 0.0)
}

/// Like [`popup_layer`], but the backdrop leaves the top `top` pixels
/// (the channel header) alone.
fn popup_layer_from<'a>(app: &'a App, card: El<'a>, at: Point, close: Ui, top: f32) -> El<'a> {
    let _ = app;
    // `Idle` makes the layers below see a "levitating" cursor while the
    // popup is open: no row under the backdrop reacts to hover. Without it
    // the message row under the pointer toggled its hover toolbar every
    // time the pointer crossed the menu's edge (the menu opens exactly at
    // the pointer), which showed as flicker.
    let backdrop = container(
        mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
            .on_press(ui(close.clone()))
            .on_right_press(ui(close))
            .interaction(iced::mouse::Interaction::Idle),
    )
    .padding(Padding::ZERO.top(top))
    .width(Length::Fill)
    .height(Length::Fill);
    let positioned = container(opaque(card))
        .padding(Padding::ZERO.top(at.y).left(at.x))
        .width(Length::Fill)
        .height(Length::Fill);
    iced::widget::stack![backdrop, positioned].into()
}

fn menu_height(app: &App, m: MessageId) -> f32 {
    let n = (menu_items(app, m).len() + super::plugins::menu_entries(app, m).len()) as f32;
    n * 32.0 + 16.0
}

struct Item {
    icon: Icon,
    label: &'static str,
    action: Ui,
    danger: bool,
}

fn menu_items(app: &App, m: MessageId) -> Vec<Item> {
    let Some(c) = app.open_channel() else {
        return vec![];
    };
    let Some(msg) = app.chat.message(c, m) else {
        return vec![];
    };
    let mine = Some(msg.author.id) == app.chat.me_id();
    let mut v = vec![Item {
        icon: Icon::SmilePlus,
        label: "Add Reaction",
        action: Ui::OpenEmojiPicker(PickerTarget::Reaction {
            channel: c,
            message: m,
        }),
        danger: false,
    }];
    if mine && matches!(msg.kind, 0 | 19) {
        v.push(Item {
            icon: Icon::Pencil,
            label: "Edit Message",
            action: Ui::StartEdit(m),
            danger: false,
        });
    }
    v.push(Item {
        icon: Icon::Reply,
        label: "Reply",
        action: Ui::StartReply(m),
        danger: false,
    });
    if !msg.content.is_empty() {
        v.push(Item {
            icon: Icon::Copy,
            label: "Copy Text",
            action: Ui::CopyText(m),
            danger: false,
        });
    }
    v.push(Item {
        icon: Icon::Link,
        label: "Copy Message Link",
        action: Ui::CopyLink(m),
        danger: false,
    });
    if app.developer_mode() {
        v.push(Item {
            icon: Icon::Copy,
            label: "Copy Message ID",
            action: Ui::CopyToClipboard(m.to_string()),
            danger: false,
        });
    }
    if mine {
        v.push(Item {
            icon: Icon::Trash,
            label: "Delete Message",
            action: Ui::DeleteClicked(m),
            danger: true,
        });
    }
    v
}

fn context_menu(app: &App, m: MessageId) -> El<'_> {
    let p = app.theme.palette;
    let mut col = column![].spacing(2).padding(8);
    for it in menu_items(app, m) {
        let fg = if it.danger {
            p.danger
        } else {
            p.text_secondary
        };
        col = col.push(
            button(
                row![
                    text(it.label)
                        .size(14)
                        .font(INTER_MEDIUM)
                        .width(Length::Fill),
                    icon(it.icon, 18.0, fg),
                ]
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([6, 8]))
            .width(Length::Fill)
            .on_press(ui(it.action))
            .style(theme::menu_item(p, it.danger)),
        );
    }
    // Plugin actions (labels from their manifests), after the built-ins.
    for (label, action) in super::plugins::menu_entries(app, m) {
        col = col.push(
            button(text(label).size(14).font(INTER_MEDIUM).width(Length::Fill))
                .padding(Padding::from([6, 8]))
                .width(Length::Fill)
                .on_press(action)
                .style(theme::menu_item(p, false)),
        );
    }
    container(col).width(MENU_W).style(theme::popup(p)).into()
}

/// Height of the status menu: the four statuses, then the account
/// switcher (header, one row per saved account, "Add an account").
fn status_menu_height(app: &App) -> f32 {
    let accounts = app.accounts.len().max(1) as f32;
    188.0 + 41.0 + accounts * ACCOUNT_ROW_H + 36.0
}

const ACCOUNT_ROW_H: f32 = 42.0;

fn status_menu(app: &App) -> El<'_> {
    let p = app.theme.palette;
    let mut col = column![].spacing(2).padding(8);
    for s in [
        Presence::Online,
        Presence::Idle,
        Presence::Dnd,
        Presence::Offline,
    ] {
        let dot = container(Space::new())
            .width(10)
            .height(10)
            .style(theme::rounded_bg(theme::presence_color(&p, s), 5.0));
        let mut r = row![
            dot,
            text(s.label())
                .size(14)
                .font(INTER_MEDIUM)
                .width(Length::Fill)
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if app.chat.my_status == s {
            r = r.push(icon(Icon::Check, 16.0, p.text_secondary));
        }
        col = col.push(
            button(r)
                .padding(Padding::from([8, 10]))
                .width(Length::Fill)
                .on_press(ui(Ui::SetStatus(s)))
                .style(theme::menu_item(p, false)),
        );
    }
    col = col
        .push(
            container(
                container(Space::new().width(Length::Fill).height(1)).style(theme::bg(p.border)),
            )
            .padding(Padding::from([4, 4])),
        )
        .push(
            container(
                text("SWITCH ACCOUNTS")
                    .size(11)
                    .font(INTER_SEMIBOLD)
                    .color(p.text_muted),
            )
            .padding(Padding::from([6, 10])),
        );
    for a in &app.accounts.list {
        col = col.push(account_row(app, a, 24.0));
    }
    col = col.push(
        button(
            row![
                icon(Icon::Plus, 16.0, p.text_secondary),
                text("Add an account")
                    .size(14)
                    .font(INTER_MEDIUM)
                    .width(Length::Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([8, 10]))
        .width(Length::Fill)
        .on_press(ui(Ui::AddAccount))
        .style(theme::menu_item(p, false)),
    );
    container(col).width(240).style(theme::popup(p)).into()
}

/// One saved account: avatar, names, a check on the active one and an
/// "x" (remove) on the others. Also used by the login screen.
pub(crate) fn account_row<'a>(
    app: &'a App,
    a: &'a crate::auth::accounts::SavedAccount,
    size: f32,
) -> El<'a> {
    let p = app.theme.palette;
    let active = app.active_account == Some(a.id) && app.auth.is_logged_in();
    let user = a.user();
    let face = super::avatar_circle(app, &user, size, crate::app::cosmetics::Phase::Still);
    let name = a.global_name.as_deref().filter(|n| !n.is_empty());
    let mut names = column![
        text(name.unwrap_or(&a.username).to_owned())
            .size(14)
            .font(INTER_MEDIUM)
            .wrapping(text::Wrapping::None)
    ];
    if name.is_some() {
        names = names.push(
            text(a.username.as_str())
                .size(12)
                .color(p.text_muted)
                .wrapping(text::Wrapping::None),
        );
    }
    let main = button(
        row![face, container(names).width(Length::Fill).clip(true)]
            .spacing(10)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([4, 10]))
    .width(Length::Fill)
    .style(theme::menu_item(p, false));
    let main = if active {
        main
    } else {
        main.on_press(ui(Ui::SwitchAccount(a.id)))
    };
    let side: El<'a> = if active {
        container(icon(Icon::Check, 16.0, p.text_secondary))
            .padding(Padding::from([0, 10]))
            .into()
    } else {
        button(icon(Icon::X, 14.0, p.text_muted))
            .padding(6)
            .on_press(ui(Ui::RemoveAccount(a.id)))
            .style(theme::icon_button(p))
            .into()
    };
    row![main, side]
        .spacing(2)
        .height(ACCOUNT_ROW_H - 2.0)
        .align_y(Alignment::Center)
        .into()
}

fn emoji_picker<'a>(app: &'a App, open: &'a OpenPicker) -> El<'a> {
    let p = app.theme.palette;
    let pk = &open.picker;
    let access = app.nitro_access();

    let search = text_input("Find the perfect emoji", &pk.query)
        .id(iced::widget::Id::new("emoji-search"))
        .on_input(|s| Message::Ui(Ui::EmojiQuery(s)))
        .size(14)
        .font(INTER)
        .padding(Padding::from([8, 10]))
        .style(theme::input(p, p.bg_app));

    // Sidebar, like the official picker: the servers with emoji (the open
    // one first), then the Unicode categories.
    let tab = |label: El<'a>, section: Section| -> El<'a> {
        let active = pk.section == section && pk.query.is_empty();
        button(container(label).center(32))
            .padding(0)
            .on_press(ui(Ui::EmojiSection(section)))
            .style(move |t, s| {
                let mut st = theme::icon_button(p)(t, s);
                if active {
                    st.background = Some(iced::Background::Color(p.bg_selected));
                }
                st
            })
            .into()
    };
    let servers = emoji::server_sections(&app.chat.guilds, &access);
    let mut side = column![].spacing(2).align_x(Alignment::Center);
    for g in &servers {
        let face: El<'a> = match app.guild_icon(g) {
            Some(h) => image(h.clone())
                .width(24)
                .height(24)
                .border_radius(12.0)
                .into(),
            None => text(
                g.acronym()
                    .chars()
                    .next()
                    .map(String::from)
                    .unwrap_or_default(),
            )
            .size(14)
            .font(INTER_SEMIBOLD)
            .color(p.text_secondary)
            .into(),
        };
        side = side.push(tab(face, Section::Server(g.id)));
    }
    if !servers.is_empty() {
        side = side.push(container(Space::new().width(20).height(1)).style(theme::bg(p.border)));
    }
    const FACES: [&str; 9] = [
        "\u{1F600}",
        "\u{270B}",
        "\u{1F43B}",
        "\u{1F354}",
        "\u{26BD}",
        "\u{1F697}",
        "\u{1F4A1}",
        "\u{2764}",
        "\u{1F3C1}",
    ];
    for (i, face) in FACES.iter().enumerate() {
        side = side.push(tab(
            emoji_text(*face, 18.0, p.text_normal, INTER, false),
            Section::Unicode(i),
        ));
    }
    let sidebar = container(super::smooth_scroll::smooth(
        scrollable(container(side).padding(Padding::from([0, 4])))
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(0).scroller_width(0),
            ))
            .height(Length::Fill),
    ))
    .width(PICKER_SIDEBAR_W)
    .height(Length::Fill)
    .style(theme::bg(p.bg_app));

    // Grid.
    let mut cells: Vec<El<'a>> = Vec::new();
    for (_, ce, usable) in emoji::shown_custom(pk, &app.chat.guilds, &access) {
        let face: El<'a> = match app.image(&emoji_url(&app.cfg.cdn_url, ce.id)) {
            // Locked emoji are greyed out, as in the official picker.
            Some(h) => image(h.clone())
                .width(28)
                .height(28)
                .opacity(if usable.locked() { 0.35_f32 } else { 1.0_f32 })
                .into(),
            None => text(":").size(14).color(p.text_muted).into(),
        };
        cells.push(emoji_button(p, face, Emoji::custom(ce)));
    }
    let unicode: Vec<&'static emojis::Emoji> = if !pk.query.is_empty() {
        emoji::search(&pk.query)
    } else if let Section::Unicode(i) = pk.section {
        emoji::category(i)
    } else {
        vec![]
    };
    for e in unicode {
        cells.push(emoji_button(
            p,
            emoji_text(e.as_str(), 24.0, p.text_normal, INTER, false),
            Emoji::unicode(e.as_str()),
        ));
    }
    let empty = cells.is_empty();
    let mut grid = column![].spacing(2);
    let mut line = row![].spacing(2);
    let mut n = 0;
    for c in cells {
        line = line.push(c);
        n += 1;
        if n == COLUMNS {
            grid = grid.push(line);
            line = row![].spacing(2);
            n = 0;
        }
    }
    if n > 0 {
        grid = grid.push(line);
    }
    if empty {
        grid = grid.push(
            container(text("No emoji found").size(14).color(p.text_muted))
                .padding(24)
                .center_x(Length::Fill),
        );
    }
    let title = if !pk.query.is_empty() {
        "Search results".to_owned()
    } else {
        match pk.section {
            Section::Server(id) => app
                .chat
                .guild(id)
                .map(|g| g.name.clone())
                .unwrap_or_default(),
            Section::Unicode(i) => CATEGORIES
                .get(i)
                .map(|c| c.1.to_owned())
                .unwrap_or_default(),
        }
    };
    let main = column![
        container(
            text(title.to_uppercase())
                .size(12)
                .font(INTER_SEMIBOLD)
                .color(p.text_muted)
        )
        .padding(Padding::from([4, 8]).bottom(4)),
        super::smooth_scroll::smooth(
            scrollable(container(grid).padding(Padding::from([0, 4])))
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(4).scroller_width(4),
                ))
                .style(theme::thin_scrollbar(p, false))
                .height(Length::Fill)
        ),
    ]
    .spacing(4)
    .width(Length::Fill);

    // The Nitro lock footer stays the last child, so opening it never
    // rebuilds the scrollables above (`ui::view` tree-shape rule).
    let footer: El<'a> = match &pk.locked {
        Some(e) => emoji_lock_footer(app, e, pk.target),
        None => Space::new().into(),
    };
    container(
        column![
            container(search).padding(8),
            row![sidebar, main].spacing(4).height(Length::Fill),
            footer,
        ]
        .spacing(4),
    )
    .width(PICKER_W)
    .height(PICKER_H)
    .style(theme::popup(p))
    .into()
}

/// Footer shown when a Nitro-locked emoji was picked: why, and (from the
/// composer) the explicit "Send as image".
fn emoji_lock_footer<'a>(app: &'a App, e: &Emoji, target: PickerTarget) -> El<'a> {
    let p = app.theme.palette;
    let face: El<'a> = match e
        .id
        .and_then(|id| app.image(&emoji_url(&app.cfg.cdn_url, id)))
    {
        Some(h) => image(h.clone()).width(32).height(32).into(),
        None => icon(Icon::Lock, 20.0, p.text_muted),
    };
    let here = app.nitro_access().here;
    let same_server =
        e.id.and_then(|id| emoji::find_custom(&app.chat.guilds, id))
            .is_some_and(|(g, _)| Some(g.id) == here);
    let why = match (target, e.animated && same_server) {
        (PickerTarget::Reaction { .. }, _) => "Reacting with this emoji here needs Nitro.",
        (PickerTarget::Composer, true) => {
            "Animated emoji need Nitro. You can send it as an image instead."
        }
        (PickerTarget::Composer, false) => {
            "Emoji from other servers need Nitro here. You can send it as an image instead."
        }
    };
    let send: El<'a> = if target == PickerTarget::Composer {
        button(text("Send as image").size(14).font(INTER_MEDIUM))
            .padding(Padding::from([6, 12]))
            .on_press(ui(Ui::SendLockedAsImage))
            .style(theme::primary_button(p))
            .into()
    } else {
        Space::new().into()
    };
    lock_footer(app, face, format!(":{}:", e.name), why, send)
}

fn lock_footer<'a>(app: &'a App, face: El<'a>, name: String, why: &'a str, send: El<'a>) -> El<'a> {
    let p = app.theme.palette;
    container(
        row![
            container(face).center(40),
            column![
                row![
                    icon(Icon::Lock, 12.0, p.text_muted),
                    text(name)
                        .size(14)
                        .font(INTER_SEMIBOLD)
                        .color(p.header_text),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
                text(why).size(12).color(p.text_secondary),
            ]
            .spacing(2)
            .width(Length::Fill),
            send,
            button(icon(Icon::X, 14.0, p.text_muted))
                .padding(4)
                .on_press(ui(Ui::DismissLocked))
                .style(theme::icon_button(p)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding(8)
    .width(Length::Fill)
    .style(theme::bg(p.bg_app))
    .into()
}

fn emoji_button<'a>(p: theme::Palette, face: El<'a>, emoji: Emoji) -> El<'a> {
    button(container(face).center(34))
        .padding(0)
        .on_press(ui(Ui::PickEmoji(emoji)))
        .style(theme::icon_button(p))
        .into()
}

/// "Delete Message" confirmation with a preview of the message.
fn delete_dialog(app: &App, c: ChannelId, m: MessageId) -> El<'_> {
    let p = app.theme.palette;
    let preview: El<'_> = match app.chat.message(c, m) {
        Some(msg) => container(
            row![
                avatar(app, &msg.author, 40.0),
                column![
                    text(msg.author.display_name().to_owned())
                        .size(15)
                        .font(INTER_MEDIUM)
                        .color(p.header_text),
                    text(truncate_chars(&msg.content.replace('\n', " "), 140))
                        .size(14)
                        .color(p.text_normal),
                ]
                .spacing(2)
                .width(Length::Fill),
            ]
            .spacing(12),
        )
        .padding(12)
        .width(Length::Fill)
        .style(move |_| container::Style {
            border: iced::Border {
                radius: 6.0.into(),
                width: 1.0,
                color: p.border,
            },
            ..Default::default()
        })
        .into(),
        None => Space::new().into(),
    };
    let card = container(
        column![
            text("Delete Message")
                .size(20)
                .font(INTER_SEMIBOLD)
                .color(p.header_text),
            text("Are you sure you want to delete this message?")
                .size(14)
                .color(p.text_secondary),
            preview,
            text("PROTIP: You can hold down shift when clicking delete message to bypass this confirmation entirely.")
                .size(12)
                .color(p.text_muted),
            row![
                Space::new().width(Length::Fill),
                button(text("Cancel").size(14).color(p.text_normal))
                    .padding(Padding::from([8, 16]))
                    .on_press(ui(Ui::CancelDelete))
                    .style(theme::icon_button(p)),
                button(text("Delete").size(14).color(p.accent_text))
                    .padding(Padding::from([8, 20]))
                    .on_press(ui(Ui::ConfirmDelete))
                    .style(move |_, status| {
                        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                        button::Style {
                            background: Some(iced::Background::Color(if hovered {
                                Color { r: p.danger.r * 0.85, g: p.danger.g * 0.85, b: p.danger.b * 0.85, a: 1.0 }
                            } else {
                                p.danger
                            })),
                            text_color: p.accent_text,
                            border: iced::Border { radius: 4.0.into(), ..Default::default() },
                            ..Default::default()
                        }
                    }),
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .width(440)
    .padding(16)
    .style(theme::rounded_bg(p.bg_panel, 8.0));
    opaque(
        mouse_area(
            container(opaque(card))
                .center(Length::Fill)
                .style(theme::bg(Color {
                    a: 0.7,
                    ..Color::BLACK
                })),
        )
        .on_press(ui(Ui::CancelDelete)),
    )
}

#[cfg(test)]
mod dev_mode_tests {
    use super::*;
    use crate::net::user_settings::{Synced, Value};

    fn labels(app: &App) -> Vec<String> {
        let m = app
            .open_channel()
            .and_then(|c| app.chat.messages.get(&c))
            .and_then(|w| w.messages.back())
            .map(|m| m.id)
            .unwrap();
        let mut out: Vec<String> = menu_items(app, m)
            .into_iter()
            .map(|i| i.label.to_owned())
            .collect();
        let user = app.chat.me_id().unwrap();
        for r in super::super::context_menu::user_rows_for_tests(app, user) {
            if let super::super::context_menu::Row::Item { label, .. } = r {
                out.push(label);
            }
        }
        out
    }

    #[test]
    fn developer_mode_adds_copy_id_items() {
        let mut app = App::boot(crate::config::Config::for_tests()).0;
        let off = labels(&app);
        assert!(!off.iter().any(|l| l.contains("ID")), "{off:?}");
        let _ = app.update(crate::app::Message::Ui(Ui::Settings(
            crate::app::settings::SettingsUi::Set(Synced::DeveloperMode, Value::Bool(true)),
        )));
        let on = labels(&app);
        assert!(on.contains(&"Copy Message ID".to_owned()), "{on:?}");
        assert!(on.contains(&"Copy User ID".to_owned()), "{on:?}");
    }
}
