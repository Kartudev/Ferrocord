//! Application state and update loop (the "Elm" side). No direct I/O:
//! everything that touches the network, disk or keyring runs in a `Task`.

pub mod acks;
pub mod actions;
pub mod anim;
mod background;
pub mod composer;
pub mod context;
pub mod cosmetics;
pub mod embeds;
pub mod emoji;
pub mod features;
pub mod folders;
pub mod gif_anim;
pub mod gifs;
pub mod images;
pub mod input;
pub mod jump;
pub mod lightbox;
pub mod markdown;
pub mod media_anim;
pub mod members;
pub mod message;
pub mod nitro;
pub mod notify;
pub mod panels;
pub mod plugins;
pub mod profile;
pub mod rail;
pub mod search;
pub mod servers;
pub mod settings;
pub mod state;
pub mod stickers;
mod updates;
pub mod upload;
pub mod video;
pub mod voice;
pub mod window;

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::{Subscription, Task};

use actions::{Editing, MessageMenu, OpenPicker, ReplyDraft};
pub use message::{Done, Message, Ui};
use state::{ChatState, Ctx, Effect};
use window::{Geometry, Prefs, WindowState};

use crate::auth::secrets::{self, KeyringStore, MemoryStore, Secret, SecretStore};
use crate::auth::web_login::{self, WebLoginEvent, WebLoginFailure};
use crate::auth::{AuthEvent, AuthState, QrState, WebLoginReason, WebLoginState, normalize_token};
use crate::config::{Config, ScreenshotScreen};
use crate::error::Redacted;
use crate::media::clip::Clip;
use crate::mock;
use crate::net::gateway::{self, GatewayConfig, GatewayHandle};
use crate::net::model::*;
use crate::net::remote_auth::{self, RaEvent};
use crate::net::rest::RestClient;
use crate::net::types::ClientProperties;
use crate::net::{ConnectionState, Event};
use crate::store::Store;
use crate::tray;
use crate::ui::theme::FerroTheme;

/// Minimum interval between typing notifications (the server shows the
/// indicator for ~10 s).
const TYPING_INTERVAL_MS: u64 = 8_000;
/// Messages fetched per history page.
const PAGE: u8 = 50;
/// How long an error toast stays in the top banner.
pub const TOAST_TTL: Duration = Duration::from_secs(5);

/// Parsed markdown keyed by (message id, content hash).
type MarkdownCache = HashMap<(MessageId, u64), std::rc::Rc<Vec<markdown::Block>>>;

/// What the main area shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Direct messages (with an optional open DM).
    Home(Option<ChannelId>),
    Guild {
        guild: GuildId,
        channel: Option<ChannelId>,
    },
}

/// Which top-level screen to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Splash,
    Login,
    Main,
}

pub struct App {
    pub cfg: Config,
    pub theme: FerroTheme,
    secrets: Arc<dyn SecretStore>,
    store: Store,
    rest: RestClient,
    pub auth: AuthState,
    token: Option<Secret>,
    pub token_input: String,
    pub qr: Option<QrState>,
    qr_generation: u64,
    /// "Log in with Discord's page" (helper window) state.
    pub web_login: Option<WebLoginState>,
    web_login_generation: u64,
    gateway_generation: u64,
    pub connection: ConnectionState,
    pub chat: ChatState,
    pub view: View,
    pub composer: composer::Composer,
    drafts: composer::Drafts,
    last_typing_sent: HashMap<ChannelId, u64>,
    /// Read acks of the focused channel, coalesced (`acks.rs`).
    acks: acks::AckQueue,
    pub collapsed: HashSet<ChannelId>,
    pub revealed_spoilers: HashSet<MessageId>,
    pub images: images::ImageCache,
    /// Parsed markdown per (message, content hash), so `view()` does not
    /// re-parse every message on every frame.
    md_cache: std::cell::RefCell<MarkdownCache>,
    /// QR image handle for the login screen. An `image::Handle` built in
    /// `view()` gets a new id each frame, so the renderer re-uploads the
    /// texture on every redraw (visible blinking); it is built once per
    /// code instead.
    qr_cache: std::cell::RefCell<Option<(Vec<bool>, iced::widget::image::Handle)>>,
    pub window_focused: bool,
    gateway: GatewayHandle,
    pub reply: Option<ReplyDraft>,
    pub editing: Option<Editing>,
    pub pending_delete: Option<(ChannelId, MessageId)>,
    pub menu: Option<MessageMenu>,
    /// Right-click menu of a user or channel.
    pub ctx_menu: Option<context::CtxMenu>,
    pub emoji_picker: Option<OpenPicker>,
    pub gif_picker: Option<gifs::GifPicker>,
    /// Decoded GIF picker stills, keyed by the GIF's page URL (own small
    /// LRU so they never push avatars and messages out).
    pub gif_thumbs: images::ImageCache,
    /// Saved GIFs (frecency settings).
    pub gif_favorites: gifs::Favorites,
    /// Decoded animations of the picker tiles near view (freed on close).
    pub gif_anims: gif_anim::PickerAnims,
    /// On-disk cache of picker media (`None` in mock mode / without a
    /// cache directory).
    pub gif_disk: Option<crate::media::disk_cache::DiskCache>,
    /// The saved GIFs were prefetched this session.
    pub gif_prefetched: bool,
    /// Serialises favourite saves (each is a read-modify-write).
    gif_favorites_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    pub sticker_picker: Option<stickers::StickerPicker>,
    pub draft: Vec<upload::DraftAttachment>,
    pub uploads: Vec<upload::ActiveUpload>,
    upload_handles: HashMap<u64, iced::task::Handle>,
    next_upload_id: u64,
    pub members_open: bool,
    member_rows_wanted: usize,
    pub status_menu: bool,
    pub search: search::SearchState,
    /// Channel-header popovers (bell, pins, inbox).
    pub panels: features::Panels,
    pub server_dialog: Option<servers::ServerDialog>,
    pub discover: Option<servers::DiscoverState>,
    /// Server rail interaction (folders, drag and drop, menus).
    pub rail: rail::RailUi,
    /// A server we created or joined, to open once the gateway has it.
    pending_select: Option<GuildId>,
    /// Gateway session id (sent when joining servers, like the official
    /// client).
    session_id: String,
    /// Message flashed after a jump.
    pub highlight: Option<MessageId>,
    pub toast: Option<String>,
    /// Bumped whenever `toast` changes, so that only the latest toast's
    /// timer clears it.
    toast_generation: u64,
    /// Masked link waiting for confirmation ("Leaving Ferrocord").
    pub pending_link: Option<String>,
    /// Image shown full-window (a click on a chat image or GIF).
    pub lightbox: Option<lightbox::Lightbox>,
    boot_at: Instant,
    first_frame_logged: bool,
    fonts_ready: bool,
    window_opened: bool,
    /// Main-window lifecycle (visible / hidden to the tray).
    pub win: WindowState,
    window_id: Option<iced::window::Id>,
    /// Size and position remembered while the window is closed.
    geometry: Option<Geometry>,
    pub prefs: Prefs,
    /// "Open at login" (the autostart file exists).
    pub autostart: bool,
    pub settings_open: bool,
    /// User Settings screen: categories, synced and local settings.
    pub settings: settings::SettingsState,
    /// Animation clock, hover target and decoded frames.
    pub anim: anim::AnimState,
    /// Profile popout, fetched profiles and effect configs.
    pub profiles: profile::ProfileState,
    /// Handle to the running tray, once it registered.
    tray_control: Option<tray::Control>,
    /// Voice channels, calls and voice settings.
    pub voice: voice::Voice,
    /// The one video that is downloading or playing.
    pub video: Option<video::Playback>,
    video_session: u32,
    /// Quit was chosen: subscriptions are dropped and the process ends as
    /// soon as the bounded shutdown work is done (`App::quit`).
    pub quitting: bool,
    /// Plugins: no engine or thread until one is enabled.
    pub plugins: crate::plugins::Plugins,
    /// In-app updater (banner, Settings → Updates).
    pub updater: crate::update::Updater,
}

pub fn now_ms() -> u64 {
    mock::now_ms()
}

impl App {
    pub fn boot(cfg: Config) -> (Self, Task<Message>) {
        let boot_at = crate::config::PROCESS_START
            .get()
            .copied()
            .unwrap_or_else(Instant::now);
        let theme = FerroTheme::new(cfg.theme);
        let secrets: Arc<dyn SecretStore> = if cfg.mock {
            Arc::new(MemoryStore::new())
        } else {
            Arc::new(KeyringStore)
        };
        let store = match Store::open(cfg.database_path().as_deref()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "cache unavailable; using in-memory cache");
                match Store::open(None) {
                    Ok(s) => s,
                    Err(e) => {
                        // In-memory SQLite failing means the process is broken.
                        tracing::error!(error = %e, "in-memory cache failed");
                        std::process::exit(1);
                    }
                }
            }
        };
        let rest = RestClient::new(&cfg.api_url, &cfg.cdn_url, None).unwrap_or_else(|e| {
            tracing::error!(error = %e, "HTTP client init failed");
            std::process::exit(1);
        });

        let prefs = Prefs::load(&store);
        let local_settings = settings::LocalSettings::load(&store);
        let voice = voice::Voice::with_prefs(App::load_voice_prefs(&store));
        let updater = updates::new_updater(&cfg, &store);
        let start_hidden = cfg.minimized || prefs.start_minimized;
        let (win, first_action) = WindowState::boot(cfg.tray, start_hidden, prefs.close_to_tray);
        let mut app = App {
            theme,
            secrets,
            store,
            rest,
            auth: AuthState::Restoring,
            token: None,
            token_input: String::new(),
            qr: None,
            qr_generation: 0,
            web_login: None,
            web_login_generation: 0,
            gateway_generation: 0,
            connection: ConnectionState::Disconnected,
            chat: ChatState::default(),
            view: View::Home(None),
            composer: composer::Composer::new(),
            drafts: composer::Drafts::default(),
            last_typing_sent: HashMap::new(),
            acks: acks::AckQueue::default(),
            collapsed: HashSet::new(),
            revealed_spoilers: HashSet::new(),
            images: images::ImageCache::default(),
            md_cache: std::cell::RefCell::new(HashMap::new()),
            qr_cache: std::cell::RefCell::new(None),
            window_focused: true,
            gateway: GatewayHandle::default(),
            reply: None,
            editing: None,
            pending_delete: None,
            menu: None,
            ctx_menu: None,
            emoji_picker: None,
            gif_picker: None,
            gif_thumbs: images::ImageCache::new(gifs::THUMB_BUDGET),
            gif_favorites: gifs::Favorites::default(),
            gif_anims: gif_anim::PickerAnims::default(),
            gif_disk: cfg.cache_dir.as_ref().filter(|_| !cfg.mock).map(|d| {
                crate::media::disk_cache::DiskCache::new(
                    d.join("gifs"),
                    crate::media::disk_cache::GIF_CACHE_CAP,
                )
            }),
            gif_prefetched: false,
            gif_favorites_lock: Default::default(),
            sticker_picker: None,
            draft: Vec::new(),
            uploads: Vec::new(),
            upload_handles: HashMap::new(),
            next_upload_id: 0,
            members_open: true,
            member_rows_wanted: members::RANGE_SIZE,
            status_menu: false,
            search: search::SearchState::default(),
            panels: features::Panels::default(),
            server_dialog: None,
            discover: None,
            rail: rail::RailUi::default(),
            pending_select: None,
            session_id: String::new(),
            highlight: None,
            toast: None,
            toast_generation: 0,
            pending_link: None,
            lightbox: None,
            boot_at,
            first_frame_logged: false,
            fonts_ready: false,
            window_opened: false,
            win,
            window_id: None,
            geometry: None,
            prefs,
            autostart: false,
            settings_open: false,
            settings: settings::SettingsState::new(local_settings),
            anim: anim::AnimState::default(),
            profiles: profile::ProfileState::default(),
            tray_control: None,
            voice,
            video: None,
            video_session: 0,
            quitting: false,
            plugins: crate::plugins::Plugins::new(
                cfg.plugins_dir.clone(),
                cfg.plugin_store_path(),
                !cfg.mock,
            ),
            updater,
            cfg,
        };
        app.window_focused = app.win.visible;
        // "Play GIFs automatically" is synced; the local copy holds until
        // Discord's value arrives.
        app.settings.synced.seed(
            crate::net::user_settings::Synced::GifAutoPlay,
            crate::net::user_settings::Value::Bool(app.prefs.autoplay_gifs),
        );
        app.apply_settings();
        let open_window = app.run_window_action(first_action);
        let autostart = if app.cfg.mock {
            Task::none()
        } else {
            Task::perform(
                async { tokio::task::spawn_blocking(tray::autostart::current).await },
                |r| Message::Done(Done::AutostartLoaded(r.unwrap_or(false))),
            )
        };

        let fonts = Task::batch(
            crate::ui::theme::FONT_FILES
                .iter()
                .map(|f| iced::font::load(*f).map(|_| Message::Done(Done::Ignored))),
        )
        .chain(Task::done(Message::Done(Done::FontsLoaded)));

        let restore = if app.cfg.mock {
            app.boot_mock();
            Task::none()
        } else {
            // First frame from the SQLite cache; session restore in the
            // background.
            match app.store.load_snapshot() {
                Ok(snap) => {
                    let ready = ReadyData {
                        me: snap.me.clone(),
                        guilds: snap.guilds,
                        channels: snap.channels,
                        users: snap.users,
                        read_states: snap.read_states,
                        ..Default::default()
                    };
                    app.chat.load_ready(&ready);
                    app.view = match (snap.selected_guild, snap.selected_channel) {
                        (Some(g), c) if app.chat.guild(g).is_some() => View::Guild {
                            guild: g,
                            channel: c,
                        },
                        (_, c) => View::Home(c.filter(|c| app.chat.channels.contains_key(c))),
                    };
                }
                Err(e) => tracing::warn!(error = %e, "could not read cache snapshot"),
            }
            let s = app.secrets.clone();
            Task::perform(secrets::load_token(s), |r| {
                Message::Done(Done::TokenRestored(r.map_err(|e| e.user_message())))
            })
        };
        let sweep = app.sweep_videos();
        let plugins = app.boot_plugins();
        let hang = if app.cfg.mock_hang {
            // `FERROCORD_MOCK_HANG=1`: a blocking call that never returns
            // (a stuck network read, plugin call or notification watch),
            // which iced's runtime would wait for at exit.
            Task::perform(
                async {
                    let _ = tokio::task::spawn_blocking(|| {
                        loop {
                            std::thread::park();
                        }
                    })
                    .await;
                },
                |()| Message::Done(Done::Ignored),
            )
        } else {
            Task::none()
        };
        let updater = app.boot_updater();
        (
            app,
            Task::batch([
                open_window,
                fonts,
                restore,
                autostart,
                sweep,
                plugins,
                updater,
                hang,
            ]),
        )
    }

    fn boot_mock(&mut self) {
        let ready = mock::ready();
        self.chat.load_ready(&ready);
        let me = ready.me.as_ref().map(|u| u.id);
        let _ = self.voice.apply(&Event::Ready(Box::new(ready.clone())), me);
        for c in ready.channels.iter().filter(|c| c.kind.is_textual()) {
            let mut h = mock::history(c.id);
            for m in &mut h {
                m.channel_id = c.id;
                m.guild_id = c.guild_id;
            }
            if let Some(last) = h.last() {
                // Keep read-state math consistent with the mock history.
                if let Some(ch) = self.chat.channels.get_mut(&c.id) {
                    ch.last_message_id = Some(last.id);
                }
                if let Some(rs) = self.chat.read_states.get_mut(&c.id)
                    && rs.last_read != Some(Snowflake(1))
                {
                    rs.last_read = Some(last.id);
                }
            }
            self.chat.insert_history(c.id, h, false, true);
        }
        for (i, (w, h)) in mock::MOCK_GIF_SIZES.iter().enumerate() {
            let (w, h, px) = mock::mock_image(*w, *h);
            let handle = iced::widget::image::Handle::from_rgba(w, h, px);
            let size = (w * h * 4) as usize;
            // The gifv embed still, and the picker tile of the same GIF.
            self.images
                .insert(format!("mock://gif/{}", i + 1), handle.clone(), size);
            self.gif_thumbs
                .insert(mock::gif_page_url(i + 1), handle, size);
        }
        self.gif_favorites.list = Some(mock::gif_favorites());
        let (w, h, px) = mock::mock_image(400, 225);
        self.images.insert(
            "mock://image/1".into(),
            iced::widget::image::Handle::from_rgba(w, h, px),
            (w * h * 4) as usize,
        );
        let (w, h, px) = mock::mock_image(400, 225);
        self.images.insert(
            "mock://video/poster".into(),
            iced::widget::image::Handle::from_rgba(w, h, px),
            (w * h * 4) as usize,
        );
        for (key, w, h, shift) in mock::MOCK_EMBED_IMAGES {
            let (w, h, px) = mock::mock_embed_image(w, h, shift);
            self.images.insert(
                key.into(),
                iced::widget::image::Handle::from_rgba(w, h, px),
                (w * h * 4) as usize,
            );
        }
        // Raster avatars for the users of the long mock DM.
        for (n, rgb) in [(1u64, [120u8, 200, 255]), (3, [250, 170, 40])] {
            let mut u = mock::user(n);
            u.avatar = Some(mock::MOCK_AVATAR.into());
            let (side, px) = mock::mock_emoji(rgb);
            for size in [64, 80] {
                if let Some(url) = self.rest.avatar_url(&u, size) {
                    self.images.insert(
                        url,
                        iced::widget::image::Handle::from_rgba(side, side, px.clone()),
                        (side * side * 4) as usize,
                    );
                }
            }
        }
        self.mock_load_cosmetics();
        self.mock_load_media_anims();
        // Mock sticker images, under the CDN URLs the views ask for.
        let iced_sticker = Sticker {
            id: Snowflake(1105),
            name: String::new(),
            format: StickerFormat::Png,
            available: true,
        };
        for (st, rgb) in mock::mock_stickers()
            .into_iter()
            .chain([iced_sticker])
            .zip([
                [255u8, 140, 60],
                [230, 70, 70],
                [90, 200, 120],
                [0, 0, 0],
                [120, 190, 250],
            ])
        {
            if let Some(url) = st.image_url(&self.cfg.cdn_url) {
                let (n, px) = mock::mock_emoji(rgb);
                self.images.insert(
                    url,
                    iced::widget::image::Handle::from_rgba(n, n, px),
                    (n * n * 4) as usize,
                );
            }
        }
        for (id, rgb) in [
            (1001u64, [247u8, 130, 52]),
            (1002, [250, 200, 60]),
            (1003, [120, 190, 250]),
            (1004, [170, 120, 250]),
        ] {
            let (n, px) = mock::mock_emoji(rgb);
            self.images.insert(
                emoji_url(&self.cfg.cdn_url, Snowflake(id)),
                iced::widget::image::Handle::from_rgba(n, n, px),
                (n * n * 4) as usize,
            );
        }
        self.chat.typing.insert(
            mock::CH_GENERAL,
            vec![state::Typing {
                user_id: Snowflake(2),
                until_ms: u64::MAX,
            }],
        );
        self.connection = ConnectionState::Connected;
        let screen = self.cfg.screenshot.as_ref().map(|(s, _)| *s);
        match screen {
            Some(ScreenshotScreen::Login) => {
                self.auth = AuthState::LoggedOut { error: None };
                self.qr = Some(qr_ready("https://discord.com/ra/mock-fingerprint-not-real"));
            }
            Some(ScreenshotScreen::LoginCaptcha) => {
                self.auth = AuthState::LoggedOut { error: None };
                self.qr = Some(QrState::CaptchaRequired);
                self.web_login = Some(WebLoginState::Open(WebLoginReason::Captcha));
            }
            Some(ScreenshotScreen::LoginWeb) => {
                self.auth = AuthState::LoggedOut { error: None };
                self.qr = Some(qr_ready("https://discord.com/ra/mock-fingerprint-not-real"));
                self.web_login = Some(WebLoginState::Open(WebLoginReason::Chosen));
            }
            Some(ScreenshotScreen::LoginWebFailed) => {
                self.auth = AuthState::LoggedOut { error: None };
                self.qr = Some(QrState::CaptchaRequired);
                self.web_login = Some(WebLoginState::Failed(
                    WebLoginFailure::NoWebEngine.user_message(),
                ));
            }
            Some(ScreenshotScreen::Multiline) => {
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.view = View::Home(Some(mock::DM_ALICE));
                self.chat.mark_read(mock::DM_ALICE);
            }
            Some(ScreenshotScreen::Dms) => {
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.view = View::Home(Some(mock::DM_ALICE));
                self.chat.mark_read(mock::DM_ALICE);
                self.chat.typing.insert(
                    mock::DM_ALICE,
                    vec![state::Typing {
                        user_id: Snowflake(2),
                        until_ms: u64::MAX,
                    }],
                );
            }
            Some(ScreenshotScreen::Embeds) => {
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.view = View::Guild {
                    guild: mock::GUILD_RUST,
                    channel: Some(mock::CH_SHOWCASE),
                };
                self.chat.mark_read(mock::CH_SHOWCASE);
            }
            Some(s @ (ScreenshotScreen::Video | ScreenshotScreen::VideoFullscreen)) => {
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.view = View::Guild {
                    guild: mock::GUILD_RUST,
                    channel: Some(mock::CH_SHOWCASE),
                };
                self.chat.mark_read(mock::CH_SHOWCASE);
                self.mock_video_screen(s == ScreenshotScreen::VideoFullscreen);
            }
            Some(
                s @ (ScreenshotScreen::EmbedMedia
                | ScreenshotScreen::EmbedRich
                | ScreenshotScreen::EmbedGrid),
            ) => {
                let channel = match s {
                    ScreenshotScreen::EmbedMedia => mock::CH_EMBED_MEDIA,
                    ScreenshotScreen::EmbedGrid => mock::CH_EMBED_GRID,
                    _ => mock::CH_EMBED_RICH,
                };
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.view = View::Guild {
                    guild: mock::GUILD_RUST,
                    channel: Some(channel),
                };
                self.chat.mark_read(channel);
            }
            Some(s @ (ScreenshotScreen::Search | ScreenshotScreen::Compose)) => {
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.view = View::Guild {
                    guild: mock::GUILD_RUST,
                    channel: Some(mock::CH_GENERAL),
                };
                self.chat.mark_read(mock::CH_GENERAL);
                if s == ScreenshotScreen::Search {
                    let _ = self.update_ui(Ui::SearchChanged("let".into()));
                    let _ = self.update_ui(Ui::SubmitSearch);
                } else {
                    let target = self
                        .chat
                        .messages
                        .get(&mock::CH_GENERAL)
                        .and_then(|w| w.messages.iter().rev().nth(1))
                        .map(|m| m.id);
                    if let Some(t) = target {
                        let _ = self.update_ui(Ui::StartReply(t));
                    }
                    self.draft.push(upload::DraftAttachment::from_file(
                        crate::net::rest::UploadFile {
                            path: "/tmp/design-notes.pdf".into(),
                            name: "design-notes.pdf".into(),
                            size: 482_113,
                        },
                        false,
                    ));
                    let guild = self.chat.guild(mock::GUILD_RUST);
                    self.emoji_picker = Some(OpenPicker {
                        picker: emoji::EmojiPicker::open(emoji::PickerTarget::Composer, guild),
                        at: iced::Point::new(1000.0, 725.0),
                    });
                }
            }
            Some(s @ (ScreenshotScreen::Panel | ScreenshotScreen::Menu)) => {
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.view = View::Guild {
                    guild: mock::GUILD_RUST,
                    channel: Some(mock::CH_GENERAL),
                };
                self.chat.mark_read(mock::CH_GENERAL);
                if s == ScreenshotScreen::Panel {
                    if let Some(me) = self.chat.me.as_mut() {
                        me.global_name = Some("bronze age romantic of the northern coast".into());
                    }
                } else if let Some(id) = self
                    .chat
                    .messages
                    .get(&mock::CH_GENERAL)
                    .and_then(|w| w.messages.iter().find(|m| m.author.id == Snowflake(2)))
                    .map(|m| m.id)
                {
                    let _ = self.update_ui(Ui::ContextMenu(id));
                    if let Some(mn) = self.menu.as_mut() {
                        mn.at = iced::Point::new(640.0, 300.0);
                    }
                }
            }
            Some(s) if s.is_voice() => {
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.mock_voice_screen(s);
            }
            Some(ScreenshotScreen::ProfileDm) => {
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.view = View::Home(Some(mock::DM_ALICE));
                self.chat.mark_read(mock::DM_ALICE);
            }
            Some(ScreenshotScreen::Profile) => {
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.view = View::Guild {
                    guild: mock::GUILD_RUST,
                    channel: Some(mock::CH_GENERAL),
                };
                self.chat.mark_read(mock::CH_GENERAL);
            }
            Some(s) if s.is_round3() => {
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.view = View::Guild {
                    guild: mock::GUILD_RUST,
                    channel: Some(mock::CH_GENERAL),
                };
                self.chat.mark_read(mock::CH_GENERAL);
                self.mock_round3_screen(s);
            }
            Some(_) | None => {
                self.auth = AuthState::LoggedIn {
                    user: mock::user(1),
                };
                self.view = View::Guild {
                    guild: mock::GUILD_RUST,
                    channel: Some(mock::CH_GENERAL),
                };
                self.chat.mark_read(mock::CH_GENERAL);
            }
        }
        self.settings_open = matches!(
            screen,
            Some(ScreenshotScreen::Settings | ScreenshotScreen::SettingsPage(_))
        );
        self.mock_settings(screen);
        self.mock_update_screen(screen);
        self.mock_plugin_screen(screen);
        let _ = self.sync_members();
        let ctx = self.ctx();
        self.chat
            .apply_event(&Event::MemberList(Box::new(mock::member_list())), &ctx);
        if screen == Some(ScreenshotScreen::Profile) {
            let _ = self.update_ui(Ui::OpenProfile {
                user: Snowflake(2),
                guild: Some(mock::GUILD_RUST),
            });
            if let Some(p) = self.profiles.popout.as_mut() {
                p.at = iced::Point::new(690.0, 70.0);
            }
        }
        if screen == Some(ScreenshotScreen::ProfileDm) {
            let _ = self.request_images();
        }
        let gif_page = match screen {
            Some(ScreenshotScreen::Gifs) => Some(gifs::GifPage::Home),
            Some(ScreenshotScreen::GifTrending) => Some(gifs::GifPage::Trending),
            Some(ScreenshotScreen::GifFavorites) => Some(gifs::GifPage::Favorites),
            _ => None,
        };
        if let Some(page) = gif_page {
            // Opened from the composer's GIF button.
            let (mut p, generation) = gifs::GifPicker::open(iced::Point::new(915.0, 752.0));
            p.finish(generation, Ok(mock::gifs("")));
            p.show(page);
            self.gif_picker = Some(p);
        }
        if screen == Some(ScreenshotScreen::EmojiLocked) {
            // Another server's emoji picked from the composer without Nitro.
            let mut picker = emoji::EmojiPicker::open(
                emoji::PickerTarget::Composer,
                self.chat.guild(mock::GUILD_RUST),
            );
            picker.section = emoji::Section::Server(mock::GUILD_ICED);
            picker.locked = emoji::find_custom(&self.chat.guilds, Snowflake(1003))
                .map(|(_, e)| Emoji::custom(e));
            self.emoji_picker = Some(actions::OpenPicker {
                picker,
                at: iced::Point::new(1000.0, 752.0),
            });
        }
        if screen == Some(ScreenshotScreen::StickersLocked) {
            self.sticker_picker = Some(stickers::StickerPicker {
                at: iced::Point::new(960.0, 752.0),
                query: String::new(),
                locked: Some(Snowflake(1105)),
            });
        }
        if screen == Some(ScreenshotScreen::Panel) {
            // After the member list: it carries our own (online) presence.
            self.chat.my_status = Presence::Dnd;
        }
    }

    /// Opens the popover / dialog / page a round-3 screenshot shows.
    fn mock_round3_screen(&mut self, s: ScreenshotScreen) {
        use ScreenshotScreen as S;
        use panels::Popover;
        let folders = |app: &mut App| {
            app.chat.folders.items = vec![
                crate::net::model::GuildFolder {
                    id: Some(11),
                    name: Some("Dev".into()),
                    color: Some(0x3BA55D),
                    guild_ids: vec![mock::GUILD_RUST, mock::GUILD_ICED],
                },
                crate::net::model::GuildFolder {
                    id: Some(12),
                    name: None,
                    color: Some(0xED4245),
                    guild_ids: vec![mock::GUILD_LINUX, mock::GUILD_GAMES],
                },
                crate::net::model::GuildFolder::single(mock::GUILD_CAFE),
            ];
            app.chat.resort_guilds();
            app.rail.expanded.insert(11);
        };
        match s {
            S::Bell => {
                let _ = self.update_ui(Ui::TogglePopover(Popover::Bell));
            }
            S::Pins => {
                let _ = self.update_ui(Ui::TogglePopover(Popover::Pins));
            }
            S::Inbox => {
                let _ = self.update_ui(Ui::TogglePopover(Popover::Inbox));
            }
            S::AddServer => {
                let _ = self.update_ui(Ui::OpenAddServer);
            }
            S::CreateServer => {
                let _ = self.update_ui(Ui::OpenAddServer);
                let _ = self.update_ui(Ui::AddServerCreate);
            }
            S::JoinServer => {
                let _ = self.update_ui(Ui::OpenAddServer);
                let _ = self.update_ui(Ui::AddServerJoin);
                let _ = self.update_ui(Ui::JoinInputChanged("https://discord.gg/rust-lang".into()));
                let _ = self.update_ui(Ui::SubmitJoin);
                if let (Some(servers::ServerDialog::Join(f)), Some(p)) = (
                    self.server_dialog.as_ref(),
                    mock::invite_preview("rust-lang"),
                ) {
                    let g = f.generation();
                    let _ = self.on_invite(g, Ok(p));
                }
            }
            S::Discover => {
                let _ = self.update_ui(Ui::OpenDiscover);
            }
            S::Folders => folders(self),
            S::FolderMenu => {
                folders(self);
                self.rail.menu = Some(rail::RailMenu {
                    item: rail::RailItem::Guild(mock::GUILD_LINUX),
                    at: iced::Point::new(76.0, 330.0),
                });
            }
            S::FolderSettings => {
                folders(self);
                let _ = self.update_ui(Ui::OpenFolderSettings(11));
            }
            S::SearchFilters => {
                let _ = self.update_ui(Ui::SearchChanged("from:alice has:link".into()));
                let _ = self.update_ui(Ui::SubmitSearch);
            }
            _ => {}
        }
    }

    pub fn screen(&self) -> Screen {
        match &self.auth {
            AuthState::LoggedOut { .. } => Screen::Login,
            AuthState::LoggedIn { .. } => Screen::Main,
            AuthState::Restoring | AuthState::Validating => {
                if self.chat.me.is_some() && !self.chat.guilds.is_empty() {
                    Screen::Main
                } else if matches!(self.auth, AuthState::Validating) && self.qr.is_some() {
                    Screen::Login
                } else {
                    Screen::Splash
                }
            }
        }
    }

    pub fn open_channel(&self) -> Option<ChannelId> {
        match self.view {
            View::Home(c) => c,
            View::Guild { channel, .. } => channel,
        }
    }

    pub fn title(&self) -> String {
        let ch = self
            .open_channel()
            .and_then(|c| self.chat.channels.get(&c))
            .map(|c| match c.kind {
                ChannelKind::Dm | ChannelKind::GroupDm => c.label(|u| self.chat.user_name(u)),
                _ => format!("#{}", c.name.as_deref().unwrap_or("")),
            });
        match ch {
            Some(c) => format!("{c} - Ferrocord"),
            None => "Ferrocord".into(),
        }
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            now_ms: now_ms(),
            open_channel: self.open_channel(),
            window_focused: self.window_focused,
            notification_preview: self.cfg.notification_preview
                && self.settings.local.notify_preview,
        }
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        let toast_before = self.toast.clone();
        let task = match msg {
            Message::Ui(u) => self.update_ui(u),
            Message::Net(ev) => self.on_net(ev),
            Message::Done(d) => self.on_done(d),
            Message::Tray(ev) => self.on_tray(ev),
            Message::Voice(ev) => self.on_voice_event(ev),
            Message::Plugin(m) => self.on_plugin(m),
            Message::Update(m) => self.on_update(m),
        };
        let task = match self.toast_timer(toast_before) {
            Some(t) => Task::batch([task, t]),
            None => task,
        };
        self.sync_tray();
        // However the picker closed, its tile frames go with it.
        if self.gif_picker.is_none()
            && (self.gif_anims.scroll_y != 0.0 || !self.gif_anims.cache.is_empty())
        {
            self.gif_anims.clear();
        }
        let task = if self.video.is_some() {
            Task::batch([task, self.stop_video_if_left()])
        } else {
            task
        };
        if !self.first_frame_logged
            && self.cfg.trace_startup
            && self.fonts_ready
            && self.window_opened
        {
            self.first_frame_logged = true;
            tracing::info!(
                first_frame_ms = self.boot_at.elapsed().as_millis() as u64,
                "startup"
            );
        }
        task
    }

    /// Starts the hide timer when the toast changed (or was set again)
    /// during this update. Every toast hides itself after [`TOAST_TTL`].
    fn toast_timer(&mut self, before: Option<String>) -> Option<Task<Message>> {
        if self.toast == before {
            return None;
        }
        self.toast_generation += 1;
        self.toast.as_ref()?;
        let generation = self.toast_generation;
        // `async` keeps the sleep lazy: it is created on the runtime, not
        // here (tests drive `update` without one).
        Some(Task::perform(
            async { tokio::time::sleep(TOAST_TTL).await },
            move |()| Message::Ui(Ui::ToastExpired(generation)),
        ))
    }

    // -- UI -------------------------------------------------------------------

    fn update_ui(&mut self, u: Ui) -> Task<Message> {
        match u {
            Ui::TokenInput(s) => {
                self.token_input = s;
                Task::none()
            }
            Ui::SubmitToken => match normalize_token(&self.token_input) {
                Err(e) => {
                    self.auth = AuthState::LoggedOut {
                        error: Some(e.into()),
                    };
                    Task::none()
                }
                Ok(t) => {
                    self.token_input.clear();
                    self.auth = self.auth.apply(AuthEvent::Submitted);
                    self.validate_token(Secret::new(t))
                }
            },
            Ui::RetryQr => {
                self.qr_generation += 1;
                self.qr = Some(QrState::Connecting);
                Task::none()
            }
            Ui::OpenWebLogin => {
                self.open_web_login(WebLoginReason::Chosen);
                Task::none()
            }
            Ui::CancelWebLogin => {
                // Dropping the subscription kills the helper and wipes its
                // profile directory.
                self.web_login_generation += 1;
                self.web_login = None;
                Task::none()
            }
            Ui::SelectHome => {
                // Like the official client, picking a destination leaves Discover.
                self.discover = None;
                let last_dm = self.chat.dm_list().first().map(|c| c.id);
                let target = match self.view {
                    View::Home(c) => c,
                    _ => None,
                }
                .or(last_dm);
                self.switch_view(View::Home(target))
            }
            Ui::SelectGuild(g) => {
                self.discover = None;
                if let View::Guild { guild, .. } = self.view
                    && guild == g
                {
                    return Task::none();
                }
                let remembered = self
                    .store
                    .meta(&format!("guild_channel:{g}"))
                    .ok()
                    .flatten()
                    .and_then(|s| s.parse().ok())
                    .filter(|c| self.chat.channels.contains_key(c));
                let channel = remembered.or_else(|| self.chat.default_channel(g));
                self.switch_view(View::Guild { guild: g, channel })
            }
            Ui::SelectChannel(c) => {
                self.discover = None;
                let view = match self.chat.channels.get(&c) {
                    Some(ch) if ch.kind.is_private() => View::Home(Some(c)),
                    Some(ch) => match ch.guild_id {
                        Some(g) => View::Guild {
                            guild: g,
                            channel: Some(c),
                        },
                        None => return Task::none(),
                    },
                    None => return Task::none(),
                };
                if !self
                    .chat
                    .channels
                    .get(&c)
                    .is_some_and(|c| c.kind.is_textual())
                {
                    return Task::none();
                }
                self.switch_view(view)
            }
            Ui::ToggleCategory(c) => {
                if !self.collapsed.remove(&c) {
                    self.collapsed.insert(c);
                }
                Task::none()
            }
            Ui::ComposerChanged(s) => {
                self.composer.set_text(&s);
                self.after_composer_edit()
            }
            Ui::ComposerAction(a) => {
                if self.composer.perform(a) {
                    self.after_composer_edit()
                } else {
                    Task::none()
                }
            }
            Ui::Send => self.send(),
            Ui::Scrolled(vp) => {
                let from_top = vp.absolute_offset_reversed().y;
                if from_top < 400.0 {
                    return self.load_older();
                }
                // `absolute_offset` of an anchor_bottom list is the
                // distance from the newest message.
                if vp.absolute_offset().y < 200.0 {
                    return self.load_newer();
                }
                Task::none()
            }
            Ui::RevealSpoiler(m) => {
                self.revealed_spoilers.insert(m);
                Task::none()
            }
            Ui::ConfirmLink(url) => {
                // Only http(s) ever reaches the dialog, let alone the browser.
                self.pending_link = embeds::web_url(&url).map(str::to_owned);
                Task::none()
            }
            Ui::SuppressEmbeds(m) => self.suppress_embeds(m),
            Ui::CancelLink => {
                self.pending_link = None;
                Task::none()
            }
            Ui::OpenLightbox(lb) => {
                self.lightbox = Some(lb);
                self.request_images()
            }
            Ui::CloseLightbox => {
                self.lightbox = None;
                Task::none()
            }
            Ui::OpenUrl(url) => {
                self.pending_link = None;
                if !(url.starts_with("https://") || url.starts_with("http://")) || self.cfg.mock {
                    return Task::none();
                }
                Task::perform(open_url(url), |_| Message::Done(Done::Ignored))
            }
            Ui::Logout => self.logout(),
            Ui::Voice(v) => self.update_voice(v),
            Ui::Settings(s) => self.update_settings(s),
            Ui::WindowFocus(f) => {
                self.window_focused = f;
                if f && let Some(c) = self.open_channel() {
                    return self.mark_read(c);
                }
                Task::none()
            }
            Ui::WindowOpened => {
                self.window_opened = true;
                Task::none()
            }
            Ui::CloseRequested
            | Ui::ToggleWindow
            | Ui::ShowWindow
            | Ui::HideWindow
            | Ui::Quit
            | Ui::OpenSettings
            | Ui::CloseSettings
            | Ui::ToggleCloseToTray
            | Ui::ToggleStartMinimized
            | Ui::ToggleMuted
            | Ui::ToggleAutostart => self.update_background(u),
            Ui::Tick => {
                self.chat.prune_typing(now_ms());
                Task::none()
            }
            Ui::ToastExpired(generation) => {
                if generation == self.toast_generation {
                    self.toast = None;
                }
                Task::none()
            }
            Ui::OpenProfile { .. }
            | Ui::CloseProfile
            | Ui::HoverEnter(_)
            | Ui::HoverLeave(_)
            | Ui::ToggleReduceMotion
            | Ui::ToggleAutoplayGifs => self.update_cosmetics(u),
            other => self.update_action(other),
        }
    }

    /// Typing indicator after the composer text changed.
    fn after_composer_edit(&mut self) -> Task<Message> {
        let Some(c) = self.open_channel() else {
            return Task::none();
        };
        let now = now_ms();
        let last = self.last_typing_sent.get(&c).copied().unwrap_or(0);
        if self.cfg.mock
            || self.composer.is_empty()
            || now.saturating_sub(last) < TYPING_INTERVAL_MS
        {
            return Task::none();
        }
        self.last_typing_sent.insert(c, now);
        let rest = self.rest.clone();
        Task::perform(async move { rest.trigger_typing(c).await }, |_| {
            Message::Done(Done::Ignored)
        })
    }

    fn switch_view(&mut self, view: View) -> Task<Message> {
        let prev = self.open_channel();
        if let Some(prev) = prev {
            self.chat.trim_inactive(prev);
            // Keep the unsent text of the channel we leave.
            self.drafts.save(prev, self.composer.text());
        }
        self.view = view;
        self.composer.clear();
        let next = match view {
            View::Home(c) => c,
            View::Guild { channel, .. } => channel,
        };
        if let Some(t) = next.and_then(|c| self.drafts.take(c)) {
            self.composer.set_text(&t);
        }
        self.reset_channel_ui();
        let members = self.sync_members();
        let (g, c) = match view {
            View::Home(c) => (None, c),
            View::Guild { guild, channel } => (Some(guild), channel),
        };
        let store = self.store.clone();
        let persist = Task::perform(
            async move {
                store
                    .blocking(move |s| {
                        s.set_meta(
                            "selected_guild",
                            &g.map(|g| g.to_string()).unwrap_or_default(),
                        )?;
                        s.set_meta(
                            "selected_channel",
                            &c.map(|c| c.to_string()).unwrap_or_default(),
                        )?;
                        if let (Some(g), Some(c)) = (g, c) {
                            s.set_meta(&format!("guild_channel:{g}"), &c.to_string())?;
                        }
                        Ok(())
                    })
                    .await
            },
            |_| Message::Done(Done::Ignored),
        );
        self.plugin_channel_switch();
        let Some(c) = c else {
            return Task::batch([persist, members]);
        };
        Task::batch([
            persist,
            members,
            self.mark_read(c),
            self.load_channel(c),
            self.request_images(),
        ])
    }

    fn mark_read(&mut self, c: ChannelId) -> Task<Message> {
        let Some(mid) = self.chat.mark_read(c) else {
            return Task::none();
        };
        let rs = self.chat.read_states.get(&c).copied().unwrap_or_default();
        let store = self.store.clone();
        let persist = Task::perform(
            async move { store.blocking(move |s| s.set_read_state(c, rs)).await },
            |_| Message::Done(Done::Ignored),
        );
        if self.cfg.mock {
            return persist;
        }
        self.acks.forget(c);
        let rest = self.rest.clone();
        Task::batch([
            persist,
            Task::perform(async move { rest.ack(c, mid).await }, |r| {
                if let Err(e) = r {
                    tracing::debug!(error = %e, "ack failed");
                }
                Message::Done(Done::Ignored)
            }),
        ])
    }

    /// Opens a channel's history: SQLite first, then the network.
    fn load_channel(&mut self, c: ChannelId) -> Task<Message> {
        if self.cfg.mock {
            return Task::none();
        }
        let w = self.chat.messages.entry(c).or_default();
        if w.synced || w.loading {
            return Task::none();
        }
        w.loading = true;
        let cached = if w.messages.is_empty() {
            let store = self.store.clone();
            Task::perform(
                async move {
                    store
                        .blocking(move |s| s.messages(c, None, state::MESSAGE_WINDOW))
                        .await
                },
                move |r| {
                    Message::Done(Done::CachedHistory {
                        channel: c,
                        messages: r.unwrap_or_default(),
                    })
                },
            )
        } else {
            Task::none()
        };
        let rest = self.rest.clone();
        let fetch = Task::perform(
            async move { rest.messages(c, None, PAGE).await },
            move |r| {
                Message::Done(Done::History {
                    channel: c,
                    older: false,
                    result: r.map_err(|e| e.user_message()),
                })
            },
        );
        Task::batch([cached, fetch])
    }

    fn load_older(&mut self) -> Task<Message> {
        let Some(c) = self.open_channel() else {
            return Task::none();
        };
        if self.cfg.mock {
            return Task::none();
        }
        let Some(w) = self.chat.messages.get_mut(&c) else {
            return Task::none();
        };
        if w.loading || w.reached_start || !w.synced {
            return Task::none();
        }
        let Some(before) = w.oldest() else {
            return Task::none();
        };
        w.loading = true;
        let store = self.store.clone();
        let rest = self.rest.clone();
        Task::perform(
            async move {
                // Older pages come from SQLite when available.
                let local = store
                    .blocking(move |s| s.messages(c, Some(before), PAGE as usize))
                    .await
                    .unwrap_or_default();
                if local.len() == PAGE as usize {
                    return Ok(local);
                }
                rest.messages(c, Some(before), PAGE).await
            },
            move |r| {
                Message::Done(Done::History {
                    channel: c,
                    older: true,
                    result: r.map_err(|e| e.user_message()),
                })
            },
        )
    }

    fn send(&mut self) -> Task<Message> {
        let mut content = self.composer.text().trim().to_owned();
        if self
            .settings
            .synced
            .bool(crate::net::user_settings::Synced::ConvertEmoticons)
        {
            content = settings::convert_emoticons(&content);
        }
        let Some(c) = self.open_channel() else {
            return Task::none();
        };
        if content.chars().count() > 2000 {
            self.toast = Some("Messages can be at most 2000 characters.".into());
            return Task::none();
        }
        if !self.draft.is_empty() {
            // Files go as a multipart upload; the text is the caption.
            return self.start_upload(c, content);
        }
        if content.is_empty() {
            return Task::none();
        }
        if let Some(t) = self.plugin_intercept_send(c, &content) {
            return t;
        }
        self.composer.clear();
        self.send_text(c, content)
    }

    /// Sends `content` to `c` as a text message (optimistic echo, then
    /// REST), as a reply if one is being composed.
    pub(crate) fn send_text(&mut self, c: ChannelId, content: String) -> Task<Message> {
        let now = now_ms();
        let nonce = Snowflake::from_timestamp_ms(now, fastrand::u64(0..4096)).to_string();
        let reply = self.reply_target(c);
        self.reply = None;
        let Some(pending) = self.chat.push_pending(
            c,
            content.clone(),
            nonce.clone(),
            now,
            reply.map(|r| r.message),
        ) else {
            return Task::none();
        };
        self.last_typing_sent.remove(&c);
        if self.cfg.mock {
            let mut echo = pending;
            echo.pending = false;
            echo.id = Snowflake::from_timestamp_ms(now + 1, 0);
            return Task::perform(
                async move {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    echo
                },
                |m| Message::Net(Event::MessageCreate(Box::new(m))),
            );
        }
        tracing::info!(channel = %c, body = %Redacted(&content), "sending message");
        let rest = self.rest.clone();
        let n2 = nonce.clone();
        Task::perform(
            async move { rest.send_message(c, &content, &nonce, reply).await },
            move |r| {
                Message::Done(Done::Sent {
                    channel: c,
                    nonce: n2.clone(),
                    result: r.map(Box::new).map_err(|e| e.user_message()),
                })
            },
        )
    }

    fn validate_token(&mut self, token: Secret) -> Task<Message> {
        let rest = self.rest.with_token(token.clone());
        self.token = Some(token);
        Task::perform(async move { rest.me().await }, |r| {
            let revoked = r.as_ref().err().is_some_and(|e| e.is_session_revoked());
            Message::Done(Done::TokenValidated {
                result: r.map_err(|e| e.user_message()),
                revoked,
            })
        })
    }

    fn start_gateway(&mut self) {
        self.gateway_generation += 1;
        if let Some(t) = &self.token {
            self.rest = self.rest.with_token(t.clone());
        }
    }

    fn logout(&mut self) -> Task<Message> {
        self.settings_open = false;
        // Account settings belong to the account; local ones stay.
        self.settings = settings::SettingsState::new(self.settings.local.clone());
        self.gateway.set_custom_status(None);
        self.apply_settings();
        let me = self.chat.me_id();
        let _ = self.voice.leave(me);
        self.voice = voice::Voice::with_prefs(self.voice.prefs.clone());
        self.token = None;
        self.acks.clear();
        self.gateway_generation += 1;
        self.auth = self.auth.apply(AuthEvent::LoggedOut);
        self.connection = ConnectionState::Disconnected;
        self.chat = ChatState::default();
        self.view = View::Home(None);
        self.qr = None;
        self.web_login = None;
        if self.cfg.mock {
            return Task::none();
        }
        if let Ok(r) = RestClient::new(&self.cfg.api_url, &self.cfg.cdn_url, None) {
            self.rest = r;
        }
        let s = self.secrets.clone();
        let store = self.store.clone();
        Task::batch([
            Task::perform(secrets::clear_token(s), |_| Message::Done(Done::Ignored)),
            Task::perform(async move { store.blocking(|s| s.clear()).await }, |_| {
                Message::Done(Done::Ignored)
            }),
        ])
    }

    // -- network events ---------------------------------------------------------

    fn on_net(&mut self, ev: Event) -> Task<Message> {
        let mut tasks = Vec::new();
        match &ev {
            Event::Connection(s) => self.connection = *s,
            Event::AuthFailed => {
                self.auth = self.auth.apply(AuthEvent::Revoked);
                return self.logout_revoked();
            }
            Event::Fatal(reason) => {
                self.toast = Some(format!("Disconnected: {reason}"));
            }
            Event::Ready(r) => {
                self.session_id.clone_from(&r.session_id);
                if let Some(me) = &r.me {
                    self.auth = self.auth.apply(AuthEvent::Validated(me.clone()));
                }
                let store = self.store.clone();
                let ready = (**r).clone();
                let order: Vec<GuildId> = if ready.guild_order.is_empty() {
                    ready.guilds.iter().map(|g| g.id).collect()
                } else {
                    ready.guild_order.clone()
                };
                tasks.push(Task::perform(
                    async move { store.blocking(move |s| s.save_ready(&ready, &order)).await },
                    |r| {
                        if let Err(e) = r {
                            tracing::warn!(error = %e, "could not cache READY");
                        }
                        Message::Done(Done::Ignored)
                    },
                ));
            }
            Event::MessageCreate(m) => {
                let store = self.store.clone();
                let m2 = (**m).clone();
                if !self.cfg.mock {
                    tasks.push(Task::perform(
                        async move {
                            store
                                .blocking(move |s| {
                                    s.upsert_messages(std::slice::from_ref(&m2))?;
                                    s.prune_channel(
                                        m2.channel_id,
                                        crate::store::MAX_MESSAGES_PER_CHANNEL,
                                    )
                                    .map(|_| ())
                                })
                                .await
                        },
                        |_| Message::Done(Done::Ignored),
                    ));
                }
            }
            Event::MessageUpdate(p) if !self.cfg.mock => {
                let store = self.store.clone();
                let p = (**p).clone();
                tasks.push(Task::perform(
                    async move { store.blocking(move |s| s.patch_message(&p)).await },
                    |_| Message::Done(Done::Ignored),
                ));
            }
            Event::MessageDelete { id, .. } if !self.cfg.mock => {
                let store = self.store.clone();
                let id = *id;
                tasks.push(Task::perform(
                    async move { store.blocking(move |s| s.delete_messages(&[id])).await },
                    |_| Message::Done(Done::Ignored),
                ));
            }
            Event::MessageDeleteBulk { ids, .. } if !self.cfg.mock => {
                let store = self.store.clone();
                let ids = ids.clone();
                tasks.push(Task::perform(
                    async move { store.blocking(move |s| s.delete_messages(&ids)).await },
                    |_| Message::Done(Done::Ignored),
                ));
            }
            Event::ChannelUpsert(c, _) if !self.cfg.mock => {
                let store = self.store.clone();
                let c = (**c).clone();
                tasks.push(Task::perform(
                    async move { store.blocking(move |s| s.upsert_channel(&c)).await },
                    |_| Message::Done(Done::Ignored),
                ));
            }
            Event::GuildUpsert {
                guild, channels, ..
            } if !self.cfg.mock => {
                let store = self.store.clone();
                let (g, cs) = ((**guild).clone(), channels.clone());
                tasks.push(Task::perform(
                    async move { store.blocking(move |s| s.upsert_guild(&g, &cs)).await },
                    |_| Message::Done(Done::Ignored),
                ));
            }
            _ => {}
        }
        let ctx = self.ctx();
        let effects = self.chat.apply_event(&ev, &ctx);
        self.plugin_observe(&ev);
        tasks.push(self.voice_on_gateway(&ev));
        if matches!(ev, Event::Ready(_) | Event::GuildFolders(_)) {
            self.cache_folders();
        }
        if let Event::GuildUpsert { guild, .. } = &ev
            && self.pending_select == Some(guild.id)
        {
            self.pending_select = None;
            tasks.push(self.update_ui(Ui::SelectGuild(guild.id)));
        }
        if matches!(ev, Event::Ready(_)) {
            // Re-sync the open channel after (re)connect.
            if let Some(c) = self.open_channel() {
                if !self.chat.channels.contains_key(&c) {
                    self.view = View::Home(None);
                } else {
                    tasks.push(self.load_channel(c));
                }
            }
            tasks.push(self.request_images());
            tasks.push(self.sync_members());
            tasks.push(self.load_user_settings());
        }
        if matches!(ev, Event::MessageCreate(_) | Event::MemberList(_)) {
            tasks.push(self.request_images());
        }
        for fx in effects {
            match fx {
                Effect::Notify(_) if self.prefs.muted => {}
                Effect::Notify(n) => {
                    tasks.push(notify(n, self.cfg.mock, self.settings.local.notify_sound));
                    tasks.push(self.flash_taskbar());
                }
                Effect::Ack(c, m) if !self.cfg.mock => {
                    // One ack per channel every few seconds, not one per
                    // incoming message.
                    if self.acks.push(c, m) {
                        tasks.push(actions::delayed(
                            Done::FlushAcks,
                            acks::ACK_DELAY.as_millis() as u64,
                        ));
                    }
                }
                Effect::Ack(..) => {}
            }
        }
        Task::batch(tasks)
    }

    fn logout_revoked(&mut self) -> Task<Message> {
        let auth = self.auth.clone();
        let t = self.logout();
        // Keep the "session revoked" message from `apply(Revoked)`.
        self.auth = auth;
        t
    }

    // -- task completions -------------------------------------------------------

    fn on_done(&mut self, d: Done) -> Task<Message> {
        match d {
            Done::Ignored => Task::none(),
            Done::FlushAcks => {
                let rest = self.rest.clone();
                Task::batch(self.acks.drain().into_iter().map(|(c, m)| {
                    let rest = rest.clone();
                    Task::perform(async move { rest.ack(c, m).await }, |_| {
                        Message::Done(Done::Ignored)
                    })
                }))
            }
            Done::QuitReady => iced::exit(),
            d @ (Done::Profile { .. } | Done::ProfileEffects(_) | Done::Anim { .. }) => {
                self.on_cosmetics_done(d)
            }
            Done::AutostartLoaded(on) => {
                self.autostart = on;
                Task::none()
            }
            Done::AutostartChanged(r) => {
                match r {
                    Ok(on) => self.autostart = on,
                    Err(e) => self.toast = Some(e),
                }
                Task::none()
            }
            Done::FontsLoaded => {
                self.fonts_ready = true;
                if let Some((screen, _)) = &self.cfg.screenshot {
                    // The editor buffer must be shaped once the fonts exist.
                    if *screen == ScreenshotScreen::Multiline {
                        self.composer.set_text(
                            "Dinner at 8? 🍕\nI booked the table by the window.\nShift+Enter breaks the line,\nEnter sends it 🦀",
                        );
                    }
                    return Task::perform(
                        async { tokio::time::sleep(Duration::from_millis(600)).await },
                        |_| Message::Done(Done::ScreenshotReady),
                    );
                }
                if matches!(self.auth, AuthState::LoggedOut { .. })
                    && self.qr.is_none()
                    && !self.cfg.mock
                {
                    self.qr = Some(QrState::Connecting);
                }
                Task::none()
            }
            Done::Voice(d) => self.on_voice_done(d),
            Done::Settings(d) => self.on_settings_done(*d),
            Done::ScreenshotReady => iced::window::latest()
                .and_then(iced::window::screenshot)
                .map(|s| Message::Done(Done::Screenshot(s))),
            Done::Screenshot(shot) => {
                if let Some((_, path)) = &self.cfg.screenshot {
                    match save_png(path, &shot) {
                        Ok(()) => tracing::info!(path = %path.display(), "screenshot saved"),
                        Err(e) => tracing::error!(error = %e, "screenshot failed"),
                    }
                }
                iced::exit()
            }
            Done::TokenRestored(Ok(Some(token))) => {
                self.auth = self.auth.apply(AuthEvent::Restored { found: true });
                self.start_gateway_with(token)
            }
            Done::TokenRestored(Ok(None)) => {
                self.auth = self.auth.apply(AuthEvent::Restored { found: false });
                if self.fonts_ready {
                    self.qr = Some(QrState::Connecting);
                }
                Task::none()
            }
            Done::TokenRestored(Err(e)) => {
                self.auth = self.auth.apply(AuthEvent::KeyringUnavailable(e));
                Task::none()
            }
            Done::TokenValidated { result, revoked } => match result {
                Ok(user) => {
                    let first_login = matches!(self.auth, AuthState::Validating);
                    self.auth = self.auth.apply(AuthEvent::Validated(user));
                    self.qr = None;
                    self.web_login = None;
                    if first_login && self.gateway_generation == 0
                        || self.connection == ConnectionState::Disconnected
                    {
                        self.start_gateway();
                    }
                    let (s, t) = (self.secrets.clone(), self.token.clone());
                    match t {
                        Some(t) => Task::perform(secrets::save_token(s, t), |r| {
                            Message::Done(Done::TokenSaved(r.map_err(|e| e.user_message())))
                        }),
                        None => Task::none(),
                    }
                }
                Err(e) if revoked => {
                    self.auth = self.auth.apply(AuthEvent::Revoked);
                    let _ = e;
                    self.logout_revoked()
                }
                Err(e) => {
                    if matches!(self.auth, AuthState::Validating) && self.gateway_generation == 0 {
                        self.auth = self.auth.apply(AuthEvent::Rejected(e));
                        self.token = None;
                    } else {
                        // Restored session and the network is down: keep the
                        // cached UI; the gateway keeps retrying.
                        self.toast = Some(e);
                    }
                    Task::none()
                }
            },
            Done::TokenSaved(r) => {
                if let Err(e) = r {
                    self.toast = Some(format!("Could not save your session: {e}"));
                }
                Task::none()
            }
            Done::Qr(ev) => self.on_qr(ev),
            Done::WebLogin(ev) => self.on_web_login(ev),
            Done::CachedHistory { channel, messages } => {
                if !messages.is_empty() {
                    let full = messages.len() >= state::MESSAGE_WINDOW;
                    self.chat.insert_history(channel, messages, false, full);
                    if let Some(w) = self.chat.messages.get_mut(&channel) {
                        // Still waiting for the network page.
                        w.loading = true;
                        w.reached_start = false;
                    }
                }
                self.request_images()
            }
            Done::History {
                channel,
                older,
                result,
            } => match result {
                Ok(msgs) => {
                    let full = msgs.len() >= PAGE as usize;
                    let store = self.store.clone();
                    let to_store = msgs.clone();
                    self.chat.insert_history(channel, msgs, older, full);
                    if let Some(w) = self.chat.messages.get_mut(&channel) {
                        w.synced = true;
                    }
                    self.plugin_history(channel);
                    Task::batch([
                        Task::perform(
                            async move { store.blocking(move |s| s.upsert_messages(&to_store)).await },
                            |_| Message::Done(Done::Ignored),
                        ),
                        self.request_images(),
                    ])
                }
                Err(e) => {
                    if let Some(w) = self.chat.messages.get_mut(&channel) {
                        w.loading = false;
                    }
                    self.toast = Some(e);
                    Task::none()
                }
            },
            Done::Sent {
                channel,
                nonce,
                result,
            } => {
                match result {
                    Ok(m) => {
                        let ctx = self.ctx();
                        self.chat.apply_event(&Event::MessageCreate(m), &ctx);
                    }
                    Err(e) => {
                        // Mark every pending message in this channel that
                        // matches (or all pending ones if the nonce is lost).
                        if nonce.is_empty() {
                            let pend: Vec<String> = self
                                .chat
                                .messages
                                .get(&channel)
                                .map(|w| {
                                    w.messages
                                        .iter()
                                        .filter(|m| m.pending)
                                        .filter_map(|m| m.nonce.clone())
                                        .collect()
                                })
                                .unwrap_or_default();
                            for n in pend {
                                self.chat.mark_failed(channel, &n);
                            }
                        } else {
                            self.chat.mark_failed(channel, &nonce);
                        }
                        self.toast = Some(e);
                    }
                }
                Task::none()
            }
            Done::Edited {
                channel,
                message,
                result,
            } => {
                match result {
                    Ok(m) => self.chat.set_content(channel, message, &m.content),
                    Err(e) => self.toast = Some(format!("Could not edit the message: {e}")),
                }
                Task::none()
            }
            Done::SuppressFailed {
                channel,
                message,
                embeds,
                flags,
                error,
            } => {
                self.chat.restore_embeds(channel, message, embeds, flags);
                self.toast = Some(format!("Could not remove the embeds: {error}"));
                Task::none()
            }
            Done::ActionFailed(e) => {
                self.toast = Some(e);
                Task::none()
            }
            Done::ReactionFailed {
                channel,
                message,
                emoji,
                change,
                error,
            } => {
                self.chat.revert_reaction(channel, message, &emoji, change);
                self.toast = Some(format!("Could not update the reaction: {error}"));
                Task::none()
            }
            Done::FilesReady(files) => self.on_files_ready(files),
            Done::Pasted { files, pasted } => {
                if !files.is_empty() {
                    self.strip_pasted_paths(&pasted);
                }
                self.on_files_ready(files)
            }
            Done::Upload { id, event } => self.on_upload_event(id, event),
            Done::ImageCopy {
                channel,
                reply,
                result,
            } => self.on_image_copy(channel, reply, result),
            Done::Around {
                channel,
                target,
                result,
            } => self.on_around(channel, target, result),
            Done::Newer { channel, result } => self.on_newer(channel, result),
            Done::Search { generation, result } => self.on_search_done(generation, result),
            Done::GifSearchDue(generation) => self.gif_search_due(generation),
            Done::Gifs { generation, result } => {
                if let Some(p) = self.gif_picker.as_mut() {
                    p.finish(generation, result);
                }
                // New results start at the top.
                self.gif_anims.scroll_y = 0.0;
                Task::batch([
                    iced::widget::operation::snap_to(
                        iced::widget::Id::new(gif_anim::GRID_ID),
                        iced::widget::operation::RelativeOffset::START,
                    ),
                    self.request_gif_thumbs(),
                    self.request_gif_anims(),
                ])
            }
            Done::GifAnim { key, anim } => self.on_gif_anim(key, anim),
            Done::GifPrefetched(n) => {
                tracing::debug!(files = n, "saved GIFs prefetched");
                Task::none()
            }
            Done::GifThumb { key, image } => {
                match image {
                    Some((w, h, px)) => {
                        let size = px.len();
                        let handle = iced::widget::image::Handle::from_rgba(w, h, px);
                        self.gif_thumbs.insert(key, handle, size);
                    }
                    None => self.gif_thumbs.mark_failed(&key),
                }
                Task::none()
            }
            Done::GifFavorites {
                generation,
                result,
                rollback,
            } => {
                if let Some(e) = self.gif_favorites.finish(generation, result, rollback) {
                    self.toast = Some(format!("Could not save your favorite GIFs: {e}"));
                }
                Task::batch([
                    self.request_gif_thumbs(),
                    self.request_gif_anims(),
                    self.prefetch_gif_favorites(),
                ])
            }
            Done::ScrollToMessage(m) => jump::scroll_to_message(m),
            Done::ClearHighlight(m) => {
                if self.highlight == Some(m) {
                    self.highlight = None;
                }
                Task::none()
            }
            Done::Pins { generation, result } => self.on_pins(generation, result),
            Done::Mentions { generation, result } => self.on_mentions(generation, result),
            Done::NotifySaved {
                channel,
                previous,
                result,
            } => self.on_notify_saved(channel, previous, result),
            Done::Unpinned {
                channel,
                message,
                result,
            } => self.on_unpinned(channel, message, result),
            Done::Invite { generation, result } => self.on_invite(generation, result),
            Done::Joined { generation, result } => self.on_joined(generation, result),
            Done::Created(r) => self.on_created(r),
            Done::IconPicked(r) => self.on_icon_picked(r),
            Done::DiscoverCategories(r) => {
                if let (Some(d), Ok(c)) = (self.discover.as_mut(), r) {
                    d.categories = c;
                }
                Task::none()
            }
            Done::DiscoverPage { generation, result } => self.on_discover_page(generation, result),
            Done::DiscoverJoined { guild, result } => self.on_discover_joined(guild, result),
            Done::FoldersSaved { previous, result } => self.on_folders_saved(previous, result),
            Done::StatusSaved(r) => {
                if let Err(e) = r {
                    self.toast = Some(format!("Could not save your status: {e}"));
                }
                Task::none()
            }
            Done::Video { session, progress } => self.on_video(session, progress),
            Done::Image { key, bytes } => {
                let t = match bytes {
                    Some(b) => {
                        let t = self.decode_media_anim(&key, &b);
                        self.images.insert_bytes(key, b);
                        t
                    }
                    None => {
                        self.images.mark_failed(&key);
                        Task::none()
                    }
                };
                // A batch is capped (`request_images`): once it is done, an
                // open emoji picker asks for the next one, so a server with
                // hundreds of emoji fills in instead of stopping at the cap.
                if self.emoji_picker.is_some() && self.images.pending_len() == 0 {
                    Task::batch([t, self.request_images()])
                } else {
                    t
                }
            }
        }
    }

    fn start_gateway_with(&mut self, token: Secret) -> Task<Message> {
        self.token = Some(token.clone());
        self.start_gateway();
        self.validate_token(token)
    }

    fn on_qr(&mut self, ev: RaEvent) -> Task<Message> {
        match ev {
            RaEvent::QrReady(url) => self.qr = Some(qr_ready(&url)),
            RaEvent::Scanned { username } => self.qr = Some(QrState::Scanned { username }),
            RaEvent::Token(t) => {
                self.qr = None;
                self.auth = self.auth.apply(AuthEvent::Submitted);
                return self.validate_token(t);
            }
            RaEvent::Cancelled => {
                self.qr_generation += 1;
                self.qr = Some(QrState::Connecting);
            }
            RaEvent::CaptchaRequired(_) => {
                // The ticket is bound to this QR session and Discord only
                // shows its captcha on its own pages: continue there.
                self.qr = Some(QrState::CaptchaRequired);
                self.open_web_login(WebLoginReason::Captcha);
            }
            RaEvent::Expired => self.qr = Some(QrState::Failed("The QR code expired.".into())),
            RaEvent::Error(e) => self.qr = Some(QrState::Failed(e)),
        }
        Task::none()
    }

    /// Opens Discord's login page in the helper window (no-op while open).
    fn open_web_login(&mut self, reason: WebLoginReason) {
        if self
            .web_login
            .as_ref()
            .is_some_and(WebLoginState::is_active)
            || !matches!(self.auth, AuthState::LoggedOut { .. })
        {
            return;
        }
        self.web_login_generation += 1;
        self.web_login = Some(if self.cfg.mock {
            // Mock mode never starts a browser engine.
            WebLoginState::Open(reason)
        } else {
            WebLoginState::Opening(reason)
        });
    }

    fn on_web_login(&mut self, ev: WebLoginEvent) -> Task<Message> {
        let Some(state) = &self.web_login else {
            return Task::none();
        };
        let next = state.apply(&ev);
        self.web_login = next;
        match ev {
            WebLoginEvent::Token(t) if matches!(self.auth, AuthState::LoggedOut { .. }) => {
                tracing::info!("logged in on Discord's page");
                self.qr = None;
                self.auth = self.auth.apply(AuthEvent::Submitted);
                self.validate_token(t)
            }
            WebLoginEvent::Closed if self.qr == Some(QrState::CaptchaRequired) => {
                // Back to a fresh QR code; the button stays available.
                self.qr_generation += 1;
                self.qr = Some(QrState::Connecting);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Fetches avatars / icons / attachment previews that are visible now.
    fn request_images(&mut self) -> Task<Message> {
        let profiles = self.request_profiles();
        if self.cfg.mock {
            return profiles;
        }
        let anims = self.request_anims();
        // The full-window viewer's larger copy goes first: it was asked for.
        let mut urls: Vec<String> = self
            .lightbox
            .as_ref()
            .and_then(|l| l.full.clone())
            .into_iter()
            .collect();
        urls.extend(self.cosmetic_urls());
        for g in &self.chat.guilds {
            if let Some(u) = self.rest.guild_icon_url(g, 96) {
                urls.push(u);
            }
        }
        // Discover cards and the invite preview.
        if let Some(d) = &self.discover {
            urls.extend(
                d.guilds
                    .iter()
                    .filter_map(|g| g.icon_url(&self.cfg.cdn_url)),
            );
        }
        if let Some(servers::ServerDialog::Join(f)) = &self.server_dialog
            && let servers::JoinStep::Preview(p) | servers::JoinStep::Joining(p) = &f.step
            && let Some(u) = p.icon_url(&self.cfg.cdn_url)
        {
            urls.push(u);
        }
        // Authors in the pins / inbox popovers.
        for m in self
            .panels
            .pins
            .items
            .iter()
            .chain(self.panels.inbox.items.iter())
        {
            if let Some(u) = self.rest.avatar_url(&m.author, 80) {
                urls.push(u);
            }
        }
        if let Some(c) = self.open_channel()
            && let Some(w) = self.chat.messages.get(&c)
        {
            for m in w.messages.iter().rev().take(60) {
                if let Some(u) = self.rest.avatar_url(&m.author, 80) {
                    urls.push(u);
                }
                for a in m.attachments.iter().filter(|a| a.is_image()) {
                    urls.push(attachment_preview_url(a));
                }
                urls.extend(
                    m.stickers
                        .iter()
                        .filter_map(|s| s.image_url(&self.cfg.cdn_url)),
                );
                if let Some(a) = m.referenced.as_ref().and_then(|r| r.author.as_ref())
                    && let Some(u) = self.rest.avatar_url(a, 80)
                {
                    urls.push(u);
                }
                for r in m.reactions.iter().filter_map(|r| r.emoji.id) {
                    urls.push(emoji_url(&self.cfg.cdn_url, r));
                }
                if m.content.contains("<:") || m.content.contains("<a:") {
                    let blocks = self.parsed_markdown(m.id, &m.content);
                    collect_emoji(&blocks, &self.cfg.cdn_url, &mut urls);
                }
                urls.extend(crate::ui::embeds::media_keys(m));
                urls.extend(
                    m.attachments
                        .iter()
                        .filter_map(Clip::from_attachment)
                        .filter_map(|c| c.poster),
                );
            }
        }
        if let Some(open) = &self.emoji_picker {
            let access = self.nitro_access();
            for (_, e, _) in emoji::shown_custom(&open.picker, &self.chat.guilds, &access) {
                urls.push(emoji_url(&self.cfg.cdn_url, e.id));
            }
            // Sidebar icons of the servers with emoji.
            for g in emoji::server_sections(&self.chat.guilds, &access) {
                if let Some(u) = self.rest.guild_icon_url(g, 96) {
                    urls.push(u);
                }
            }
        }
        if let Some(l) = &self.chat.member_list {
            for item in l.loaded().take(120) {
                if let MemberItem::Member(m) = item
                    && let Some(u) = self.rest.avatar_url(&m.user, 80)
                {
                    urls.push(u);
                }
            }
        }
        if self.sticker_picker.is_some() {
            for g in &self.chat.guilds {
                urls.extend(
                    g.stickers
                        .iter()
                        .filter_map(|s| s.image_url(&self.cfg.cdn_url)),
                );
            }
        }
        for m in self.search.results.iter() {
            if let Some(u) = self.rest.avatar_url(&m.author, 80) {
                urls.push(u);
            }
        }
        if let View::Home(_) = self.view {
            for c in self.chat.dm_list().into_iter().take(40) {
                if let Some(u) = c.recipients.first().and_then(|u| self.chat.users.get(u))
                    && let Some(url) = self.rest.avatar_url(u, 64)
                {
                    urls.push(url);
                }
            }
        }
        if let Some(me) = &self.chat.me
            && let Some(u) = self.rest.avatar_url(me, 64)
        {
            urls.push(u);
        }
        let tasks: Vec<Task<Message>> = urls
            .into_iter()
            .filter(|u| self.rest.is_allowed_media_url(u))
            .filter(|u| self.images.want(u))
            .take(96)
            .map(|url| {
                let rest = self.rest.clone();
                Task::perform(
                    async move {
                        let bytes = match rest.fetch_bytes(&url).await {
                            Ok(b) => tokio::task::spawn_blocking(move || {
                                crate::net::rest::check_image_dimensions(&b).map(|_| b)
                            })
                            .await
                            .ok()
                            .and_then(|r| r.ok()),
                            Err(_) => None,
                        };
                        (url, bytes)
                    },
                    |(key, bytes)| Message::Done(Done::Image { key, bytes }),
                )
            })
            .collect();
        Task::batch(tasks.into_iter().chain([profiles, anims]))
    }

    /// Parsed markdown for a message body (cached, bounded).
    pub fn parsed_markdown(
        &self,
        msg: MessageId,
        content: &str,
    ) -> std::rc::Rc<Vec<markdown::Block>> {
        const MAX_ENTRIES: usize = 4096;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut h);
        let key = (msg, h.finish());
        if let Some(b) = self.md_cache.borrow().get(&key) {
            return b.clone();
        }
        let blocks = std::rc::Rc::new(markdown::parse(content));
        let mut cache = self.md_cache.borrow_mut();
        if cache.len() >= MAX_ENTRIES {
            cache.clear();
        }
        cache.insert(key, blocks.clone());
        blocks
    }

    /// Whether `url` may be downloaded (Discord's CDN / media proxy only).
    pub fn is_allowed_media(&self, url: &str) -> bool {
        self.rest.is_allowed_media_url(url)
    }

    pub fn image(&self, key: &str) -> Option<&iced::widget::image::Handle> {
        self.images.get(key)
    }

    /// The picker still of the GIF with this page URL.
    pub fn gif_thumb(&self, url: &str) -> Option<&iced::widget::image::Handle> {
        self.gif_thumbs.get(url)
    }

    pub fn avatar_key(&self, user: &User) -> Option<String> {
        self.rest
            .avatar_url(user, 80)
            .or_else(|| self.rest.avatar_url(user, 64))
    }

    /// The login QR as an image handle, built once per distinct code.
    pub fn qr_handle(&self, size: usize, modules: &[bool]) -> iced::widget::image::Handle {
        let mut cache = self.qr_cache.borrow_mut();
        if let Some((m, h)) = cache.as_ref()
            && m == modules
        {
            return h.clone();
        }
        let h = iced::widget::image::Handle::from_rgba(
            size as u32,
            size as u32,
            crate::ui::login::qr_pixels(modules),
        );
        *cache = Some((modules.to_vec(), h.clone()));
        h
    }

    pub fn avatar_handle(&self, user: &User) -> Option<&iced::widget::image::Handle> {
        let a = self.rest.avatar_url(user, 80)?;
        self.images.get(&a).or_else(|| {
            self.rest
                .avatar_url(user, 64)
                .and_then(|b| self.images.get(&b))
        })
    }

    pub fn guild_icon(&self, g: &Guild) -> Option<&iced::widget::image::Handle> {
        self.images.get(&self.rest.guild_icon_url(g, 96)?)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if self.quitting {
            // Dropping them stops the gateway and voice connections, the
            // tray and the login helper (killed on drop).
            return Subscription::none();
        }
        let mut subs = vec![iced::event::listen_with(|e, status, _| {
            input::record(&e);
            input_message(e, status)
        })];
        if self.cfg.tray {
            subs.push(Subscription::run(tray::events).map(Message::Tray));
            subs.push(
                Subscription::run(tray::instance::show_requests)
                    .map(|()| Message::Ui(Ui::ShowWindow)),
            );
        }
        if let Some(p) = self.plugin_subscription() {
            subs.push(p);
        }
        if self.cfg.mock {
            return Subscription::batch(subs);
        }
        if let (Some(token), true) = (&self.token, self.gateway_generation > 0)
            && !matches!(self.auth, AuthState::LoggedOut { .. })
        {
            let key = Keyed {
                id: self.gateway_generation,
                value: GatewayConfig {
                    url: self.cfg.gateway_url.clone(),
                    token: token.clone(),
                    zlib: self.cfg.zlib,
                    properties: ClientProperties::default(),
                    handle: self.gateway.clone(),
                },
            };
            subs.push(
                Subscription::run_with(key, |k| gateway::spawn(k.value.clone())).map(Message::Net),
            );
        }
        let qr_active = matches!(
            self.qr,
            Some(QrState::Connecting | QrState::Ready { .. } | QrState::Scanned { .. })
        ) && matches!(self.auth, AuthState::LoggedOut { .. });
        if qr_active {
            let key = Keyed {
                id: self.qr_generation,
                value: (self.cfg.remote_auth_url.clone(), self.rest.clone()),
            };
            subs.push(
                Subscription::run_with(key, |k| {
                    remote_auth::spawn(k.value.0.clone(), k.value.1.clone())
                })
                .map(|e| Message::Done(Done::Qr(e))),
            );
        }
        if !self.cfg.mock
            && self
                .web_login
                .as_ref()
                .is_some_and(WebLoginState::is_active)
            && matches!(self.auth, AuthState::LoggedOut { .. })
        {
            subs.push(
                Subscription::run_with(
                    Keyed {
                        id: self.web_login_generation,
                        value: WebLoginSub,
                    },
                    |_| web_login::spawn(web_login::helper_path(), Vec::new()),
                )
                .map(|e| Message::Done(Done::WebLogin(e))),
            );
        }
        if !self.chat.typing.is_empty() {
            subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::Ui(Ui::Tick)));
        }
        if let Some(v) = self.voice_subscription() {
            subs.push(v);
        }
        // Live microphone meter in the voice settings while connected or
        // testing the microphone.
        if self.settings_open
            && (self.voice.conn.is_some() || self.settings.mic_test.is_some())
            && !self.cfg.mock
        {
            subs.push(iced::time::every(Duration::from_millis(100)).map(|_| Message::Ui(Ui::Tick)));
        }
        Subscription::batch(subs)
    }
}

/// Maps a runtime event to a message (non-capturing, as `listen_with`
/// requires).
fn input_message(e: iced::Event, status: iced::event::Status) -> Option<Message> {
    use iced::keyboard::{Event as Key, Key as K, key::Named};
    match e {
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
            Some(Message::Ui(Ui::DragEnd))
        }
        iced::Event::Window(iced::window::Event::Focused) => {
            Some(Message::Ui(Ui::WindowFocus(true)))
        }
        iced::Event::Window(iced::window::Event::Opened { .. }) => {
            Some(Message::Ui(Ui::WindowOpened))
        }
        iced::Event::Window(iced::window::Event::CloseRequested) => {
            Some(Message::Ui(Ui::CloseRequested))
        }
        iced::Event::Window(iced::window::Event::Unfocused) => {
            Some(Message::Ui(Ui::WindowFocus(false)))
        }
        iced::Event::Window(iced::window::Event::FileDropped(path)) => {
            Some(Message::Ui(Ui::FileDropped(path)))
        }
        // The text input captures Escape when focused; handle it anyway.
        iced::Event::Keyboard(Key::KeyPressed {
            key: K::Named(Named::Escape),
            ..
        }) => Some(Message::Ui(Ui::Escape)),
        iced::Event::Keyboard(Key::KeyPressed {
            key: K::Named(Named::ArrowUp),
            ..
        }) if status == iced::event::Status::Ignored => Some(Message::Ui(Ui::EditLast)),
        iced::Event::Keyboard(Key::KeyPressed {
            key: K::Character(c),
            modifiers,
            ..
        }) if modifiers.command() && c.as_str().eq_ignore_ascii_case("v") => {
            Some(Message::Ui(Ui::Paste))
        }
        // Ctrl+Q quits, like the official client on Linux and Windows.
        iced::Event::Keyboard(Key::KeyPressed {
            key: K::Character(c),
            modifiers,
            ..
        }) if modifiers.command() && !modifiers.shift() && c.as_str().eq_ignore_ascii_case("q") => {
            Some(Message::Ui(Ui::Quit))
        }
        _ => None,
    }
}

/// Subscription identity: only the generation counter is hashed, so the
/// secret never feeds a hash and a new generation restarts the stream.
/// Subscription key marker for the login helper.
struct WebLoginSub;

struct Keyed<T> {
    id: u64,
    value: T,
}

impl<T> Hash for Keyed<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::any::type_name::<T>().hash(state);
        self.id.hash(state);
    }
}

/// Collects custom emoji image URLs from parsed markdown.
fn collect_emoji(blocks: &[markdown::Block], cdn: &str, out: &mut Vec<String>) {
    fn walk(v: &[markdown::Inline], cdn: &str, out: &mut Vec<String>) {
        use markdown::Inline as I;
        for i in v {
            match i {
                I::Emoji { id, .. } => out.push(emoji_url(cdn, *id)),
                I::Bold(c) | I::Italic(c) | I::Underline(c) | I::Strike(c) | I::Spoiler(c) => {
                    walk(c, cdn, out)
                }
                I::Link { label, .. } => walk(label, cdn, out),
                _ => {}
            }
        }
    }
    for b in blocks {
        match b {
            markdown::Block::Paragraph(v)
            | markdown::Block::Heading(_, v)
            | markdown::Block::Subtext(v) => walk(v, cdn, out),
            markdown::Block::Quote(inner) => collect_emoji(inner, cdn, out),
            markdown::Block::List(items) => items.iter().for_each(|v| walk(v, cdn, out)),
            markdown::Block::CodeBlock { .. } => {}
        }
    }
}

/// CDN URL of a custom emoji (static PNG; animated ones show frame 1).
pub fn emoji_url(cdn: &str, id: Snowflake) -> String {
    format!("{}/emojis/{id}.png?size=96", cdn.trim_end_matches('/'))
}

fn qr_ready(url: &str) -> QrState {
    match remote_auth::qr_matrix(url) {
        Ok((size, modules)) => QrState::Ready {
            url: url.to_owned(),
            size,
            modules,
        },
        Err(e) => QrState::Failed(e.user_message()),
    }
}

/// Discord's media proxy can resize previews; ask for a bounded size.
fn attachment_preview_url(a: &Attachment) -> String {
    let base = a.proxy_url.clone().unwrap_or_else(|| a.url.clone());
    if base.starts_with("mock://") {
        return base;
    }
    let (w, h) = crate::ui::fit_size(a.width.unwrap_or(400), a.height.unwrap_or(300), 400, 300);
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}width={w}&height={h}")
}

pub fn preview_key(a: &Attachment) -> String {
    attachment_preview_url(a)
}

/// Notification servers (KDE, GNOME) interpret a subset of HTML in the
/// summary/body. Message text is untrusted, so `&`, `<`, `>` (and quotes)
/// are escaped to keep it literal.
pub fn escape_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn notify(n: state::Notification, mock: bool, sound: bool) -> Task<Message> {
    if mock {
        return Task::none();
    }
    let channel = n.channel_id;
    // Shown and watched on a thread of its own (see `app::notify`).
    let clicked = notify::show(escape_markup(&n.title), escape_markup(&n.body), sound);
    Task::perform(
        async move { clicked.await.unwrap_or(false) },
        move |clicked| {
            if clicked {
                Message::Ui(Ui::OpenChannel(channel))
            } else {
                Message::Done(Done::Ignored)
            }
        },
    )
}

async fn open_url(url: String) {
    let _ = tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";
        #[cfg(target_os = "macos")]
        let cmd = "open";
        #[cfg(target_os = "windows")]
        let cmd = "explorer";
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        let cmd = "xdg-open";
        // Wait for the opener so it is reaped (xdg-open/open return as soon
        // as the browser has been handed the URL).
        match std::process::Command::new(cmd)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => tracing::warn!(error = %e, "could not launch URL opener"),
        }
    })
    .await;
}

fn save_png(path: &std::path::Path, shot: &iced::window::Screenshot) -> crate::error::Result<()> {
    let img = image::RgbaImage::from_raw(shot.size.width, shot.size.height, shot.rgba.to_vec())
        .ok_or_else(|| crate::error::Error::Io(std::io::Error::other("bad screenshot buffer")))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    img.save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| crate::error::Error::Io(std::io::Error::other(e.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_q_quits() {
        use iced::keyboard::{Event as Key, Key as K, Location, Modifiers};
        let press = |modifiers| {
            iced::Event::Keyboard(Key::KeyPressed {
                key: K::Character("q".into()),
                modified_key: K::Character("q".into()),
                physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyQ),
                location: Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            })
        };
        assert!(matches!(
            input_message(press(Modifiers::CTRL), iced::event::Status::Captured),
            Some(Message::Ui(Ui::Quit))
        ));
        assert!(input_message(press(Modifiers::empty()), iced::event::Status::Ignored).is_none());
    }

    #[test]
    fn notification_text_is_escaped() {
        assert_eq!(
            escape_markup(r#"<b>hi</b> & <a href="x">'y'</a>"#),
            "&lt;b&gt;hi&lt;/b&gt; &amp; &lt;a href=&quot;x&quot;&gt;&#39;y&#39;&lt;/a&gt;"
        );
        assert_eq!(escape_markup("plain 🦀"), "plain 🦀");
    }
}
