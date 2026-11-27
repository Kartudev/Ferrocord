//! Paths, feature flags and environment overrides.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

/// Set by `main` as early as possible; used for `first_frame_ms`.
pub static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Stable application id: keyring service name, Wayland `app_id`,
/// notification app name.
pub const APP_ID: &str = "dev.ferro.ferrocord";
/// Human-readable application name.
pub const APP_NAME: &str = "Ferrocord";

/// Default endpoints. They can be overridden with environment variables so
/// the client can be pointed at a local mock server.
pub const DEFAULT_GATEWAY_URL: &str = "wss://gateway.discord.gg";
pub const DEFAULT_API_URL: &str = "https://discord.com/api/v9";
pub const DEFAULT_CDN_URL: &str = "https://cdn.discordapp.com";
pub const DEFAULT_REMOTE_AUTH_URL: &str = "wss://remote-auth-gateway.discord.gg/?v=2";

/// Which screen `--screenshot` renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotScreen {
    Login,
    /// QR login hit a captcha; Discord's page is open in its window.
    LoginCaptcha,
    /// "Log in with Discord's page" chosen; its window is open.
    LoginWeb,
    /// Discord's page could not open (no system webview).
    LoginWebFailed,
    Guild,
    Dms,
    Embeds,
    /// YouTube card and link preview.
    EmbedMedia,
    /// Bot embed with every decoration and a photo grid.
    EmbedRich,
    /// Twitter-style photo grid.
    EmbedGrid,
    Search,
    Compose,
    Settings,
    /// One User Settings page (`settings-<category>`).
    SettingsPage(crate::app::settings::Category),
    /// Long display name and status in the user panel.
    Panel,
    /// Right-click menu on a message.
    Menu,
    Bell,
    Pins,
    Inbox,
    AddServer,
    CreateServer,
    JoinServer,
    Discover,
    Folders,
    FolderMenu,
    FolderSettings,
    SearchFilters,
    /// Server dropdown of a server the mock user owns.
    ServerMenu,
    /// Server Settings (Overview) with an unsaved change.
    ServerSettings,
    /// Invite People dialog.
    ServerInvite,
    /// Leave Server confirmation.
    LeaveServer,
    /// Delete Server confirmation.
    DeleteServer,
    /// Right-click menu of a channel the mock user can manage.
    ChannelMenu,
    /// Create Channel dialog.
    CreateChannel,
    /// Edit Channel dialog.
    EditChannel,
    /// Server Settings → Roles, editing a role.
    ServerRoles,
    /// Right-click menu of a server member, with the moderation items.
    MemberMenu,
    /// A member's roles dialog.
    MemberRoles,
    /// Ban dialog for a member.
    BanMember,
    /// Joined voice channel with users, speaking ring and the
    /// "Voice Connected" panel.
    Voice,
    /// Active DM call.
    Call,
    /// Incoming call overlay.
    IncomingCall,
    /// The inline video player, playing.
    Video,
    /// The full-screen video view.
    VideoFullscreen,
    /// Profile popout with banner, theme colours, decoration and effect.
    Profile,
    /// The 1:1 DM profile panel.
    ProfileDm,
    /// Multi-line composer and multi-line / emoji messages in a DM.
    Multiline,
    /// GIF picker home (Favorites / Trending GIFs tiles).
    Gifs,
    /// GIF picker: trending GIFs with their save stars.
    GifTrending,
    /// GIF picker: the saved GIFs.
    GifFavorites,
    /// Settings → Plugins with the example plugins installed.
    Plugins,
    /// The permission prompt shown before a plugin is enabled.
    PluginPrompt,
    /// Plugin decorations (badges, a note line, a highlight) on messages.
    PluginDecorations,
    /// Emoji picker on another server's emoji, with the Nitro lock footer.
    EmojiLocked,
    /// Reaction picker on the "Recently Used" section.
    EmojiRecent,
    /// Sticker picker with a Nitro-locked sticker picked.
    StickersLocked,
    /// The "new version available" banner.
    UpdateBanner,
    /// The Reactions dialog (who reacted, a tab per emoji).
    Reactions,
    /// Member sidebar scrolled deep into a large list (skeleton rows).
    MembersLarge,
    /// Friends page: Online tab.
    Friends,
    /// Friends page: Pending tab (incoming + outgoing requests).
    FriendsPending,
    /// Friends page: Add Friend tab after a request was sent.
    FriendsAdd,
    /// Message Requests page.
    MessageRequests,
    /// "Find or start a conversation" with a query.
    Switcher,
}

impl ScreenshotScreen {
    /// Voice channel and call screens.
    pub fn is_voice(self) -> bool {
        matches!(self, Self::Voice | Self::Call | Self::IncomingCall)
    }

    /// Screens added with the header popovers, server dialogs and folders.
    pub fn is_round3(self) -> bool {
        matches!(
            self,
            Self::Bell
                | Self::Pins
                | Self::Inbox
                | Self::AddServer
                | Self::CreateServer
                | Self::JoinServer
                | Self::Discover
                | Self::Folders
                | Self::FolderMenu
                | Self::FolderSettings
                | Self::SearchFilters
                | Self::ServerMenu
                | Self::ServerSettings
                | Self::ServerInvite
                | Self::LeaveServer
                | Self::DeleteServer
                | Self::ChannelMenu
                | Self::CreateChannel
                | Self::EditChannel
                | Self::ServerRoles
                | Self::MemberMenu
                | Self::MemberRoles
                | Self::BanMember
        )
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "login" => Some(Self::Login),
            "login-captcha" => Some(Self::LoginCaptcha),
            "login-web" => Some(Self::LoginWeb),
            "login-web-failed" => Some(Self::LoginWebFailed),
            "guild" | "server" | "members" => Some(Self::Guild),
            "dms" | "dm" => Some(Self::Dms),
            "embeds" | "attachments" => Some(Self::Embeds),
            "embeds-media" => Some(Self::EmbedMedia),
            "embeds-rich" => Some(Self::EmbedRich),
            "embeds-grid" => Some(Self::EmbedGrid),
            "search" => Some(Self::Search),
            "compose" => Some(Self::Compose),
            "settings" => Some(Self::Settings),
            s if s.starts_with("settings-") => {
                crate::app::settings::Category::from_slug(&s["settings-".len()..])
                    .map(Self::SettingsPage)
            }
            "panel" | "userpanel" => Some(Self::Panel),
            "menu" | "context-menu" => Some(Self::Menu),
            "bell" | "notifications" => Some(Self::Bell),
            "pins" => Some(Self::Pins),
            "inbox" => Some(Self::Inbox),
            "add-server" => Some(Self::AddServer),
            "create-server" => Some(Self::CreateServer),
            "join-server" => Some(Self::JoinServer),
            "discover" => Some(Self::Discover),
            "folders" => Some(Self::Folders),
            "folder-menu" => Some(Self::FolderMenu),
            "folder-settings" => Some(Self::FolderSettings),
            "server-menu" => Some(Self::ServerMenu),
            "server-settings" => Some(Self::ServerSettings),
            "server-invite" => Some(Self::ServerInvite),
            "leave-server" => Some(Self::LeaveServer),
            "delete-server" => Some(Self::DeleteServer),
            "channel-menu" => Some(Self::ChannelMenu),
            "create-channel" => Some(Self::CreateChannel),
            "edit-channel" => Some(Self::EditChannel),
            "server-roles" => Some(Self::ServerRoles),
            "member-menu" => Some(Self::MemberMenu),
            "member-roles" => Some(Self::MemberRoles),
            "ban-member" => Some(Self::BanMember),
            "search-filters" => Some(Self::SearchFilters),
            "voice" => Some(Self::Voice),
            "call" => Some(Self::Call),
            "incoming-call" => Some(Self::IncomingCall),
            "video" => Some(Self::Video),
            "video-fullscreen" => Some(Self::VideoFullscreen),
            "profile" | "popout" => Some(Self::Profile),
            "profile-dm" => Some(Self::ProfileDm),
            "multiline" | "emoji" => Some(Self::Multiline),
            "gifs" | "gif-picker" => Some(Self::Gifs),
            "gif-trending" => Some(Self::GifTrending),
            "gif-favorites" => Some(Self::GifFavorites),
            "plugins" => Some(Self::Plugins),
            "plugin-prompt" => Some(Self::PluginPrompt),
            "plugin-decorations" => Some(Self::PluginDecorations),
            "emoji-locked" => Some(Self::EmojiLocked),
            "emoji-recent" => Some(Self::EmojiRecent),
            "stickers-locked" => Some(Self::StickersLocked),
            "update-banner" => Some(Self::UpdateBanner),
            "reactions" => Some(Self::Reactions),
            "members-large" => Some(Self::MembersLarge),
            "friends" => Some(Self::Friends),
            "friends-pending" => Some(Self::FriendsPending),
            "friends-add" => Some(Self::FriendsAdd),
            "message-requests" => Some(Self::MessageRequests),
            "switcher" => Some(Self::Switcher),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    Dark,
    Light,
}

/// Restricts `path` to the current user (0700 for directories, 0600 for
/// files). The cache and logs are not encrypted, so they must not be
/// readable by other local users. No-op on non-Unix platforms.
pub fn restrict_permissions(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// `https://` for any host, plain `http://` only for loopback (mock servers).
pub fn validate_http_url(url: &str) -> Result<(), String> {
    let u = url::Url::parse(url).map_err(|e| format!("invalid URL {url}: {e}"))?;
    let loopback = matches!(
        u.host(),
        Some(url::Host::Domain("localhost"))
            | Some(url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST))
            | Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
    );
    match u.scheme() {
        "https" if u.host().is_some() => Ok(()),
        "http" if loopback => Ok(()),
        _ => Err(format!("refusing insecure URL {url} (https required)")),
    }
}

/// Runtime configuration, built once in `main`.
#[derive(Debug, Clone)]
pub struct Config {
    /// `--mock`: use `mock.rs` data, an in-memory store and secret store,
    /// and never touch the network.
    pub mock: bool,
    /// `--screenshot <screen> <out.png>`.
    pub screenshot: Option<(ScreenshotScreen, PathBuf)>,
    /// `--minimized`: start with no window, only the tray icon.
    pub minimized: bool,
    /// System tray and background mode (off with `--no-tray`, and in
    /// screenshot mode).
    pub tray: bool,
    pub theme: ThemeChoice,
    pub gateway_url: String,
    pub api_url: String,
    pub cdn_url: String,
    pub remote_auth_url: String,
    /// Use `compress=zlib-stream` on the gateway.
    pub zlib: bool,
    /// Notification privacy: when false only the sender is shown.
    pub notification_preview: bool,
    /// Log `first_frame_ms`.
    pub trace_startup: bool,
    pub data_dir: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
    /// `<config dir>/plugins` (`FERROCORD_PLUGINS_DIR` overrides it; in
    /// mock mode only the override is used).
    pub plugins_dir: Option<PathBuf>,
    /// Disposable caches (GIF picker media).
    pub cache_dir: Option<PathBuf>,
    /// Quit arms [`crate::shutdown`]'s exit watchdog (the real process; off
    /// in tests, where it would end the test binary).
    pub exit_watchdog: bool,
    /// Mock only (`FERROCORD_MOCK_HANG=1`): simulate a stuck network call
    /// and a settings save that never answers, so tests can prove that
    /// Quit still leaves on time.
    pub mock_hang: bool,
}

impl Config {
    /// Builds the config from command-line arguments and the environment.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut mock = false;
        let mut screenshot = None;
        let mut minimized = false;
        let mut no_tray = false;
        let mut args = args.into_iter().skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--mock" => mock = true,
                "--minimized" => minimized = true,
                "--no-tray" => no_tray = true,
                "--screenshot" => {
                    let screen = args
                        .next()
                        .and_then(|s| ScreenshotScreen::parse(&s))
                        .ok_or(
                            "--screenshot needs one of: login, login-captcha, login-web, login-web-failed, guild, dms, embeds, search, compose, settings",
                        )?;
                    let out = args.next().ok_or("--screenshot needs an output path")?;
                    screenshot = Some((screen, PathBuf::from(out)));
                }
                "--help" | "-h" => {
                    return Err("usage: ferrocord [--mock] [--minimized] [--no-tray] [--screenshot <login|guild|dms|embeds|search|compose|settings> <out.png>]\n       ferrocord --version | -V".into());
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        if screenshot.is_some() {
            mock = true;
        }
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let theme = match env("FERRO_THEME").as_deref() {
            Some("light") => ThemeChoice::Light,
            _ => ThemeChoice::Dark,
        };
        let dirs = directories::ProjectDirs::from("dev", "ferro", "ferrocord");
        let gateway_url =
            env("FERROCORD_GATEWAY_URL").unwrap_or_else(|| DEFAULT_GATEWAY_URL.into());
        let remote_auth_url =
            env("FERROCORD_REMOTE_AUTH_URL").unwrap_or_else(|| DEFAULT_REMOTE_AUTH_URL.into());
        for u in [&gateway_url, &remote_auth_url] {
            crate::net::gateway::validate_url(u).map_err(|e| e.to_string())?;
        }
        let api_url = env("FERROCORD_API_URL").unwrap_or_else(|| DEFAULT_API_URL.into());
        let cdn_url = env("FERROCORD_CDN_URL").unwrap_or_else(|| DEFAULT_CDN_URL.into());
        for u in [&api_url, &cdn_url] {
            validate_http_url(u)?;
        }
        let tray = !no_tray && screenshot.is_none();
        Ok(Self {
            mock,
            screenshot,
            minimized,
            tray,
            theme,
            gateway_url,
            api_url,
            cdn_url,
            remote_auth_url,
            zlib: env("FERROCORD_ZLIB").as_deref() != Some("0"),
            notification_preview: env("FERROCORD_NOTIFY_PREVIEW").as_deref() != Some("0"),
            trace_startup: env("FERRO_TRACE_STARTUP").is_some(),
            data_dir: dirs.as_ref().map(|d| d.data_dir().to_path_buf()),
            state_dir: dirs
                .as_ref()
                .map(|d| d.state_dir().unwrap_or(d.data_dir()).to_path_buf()),
            plugins_dir: match env("FERROCORD_PLUGINS_DIR") {
                Some(d) => Some(PathBuf::from(d)),
                None if mock => None,
                None => dirs.as_ref().map(|d| d.config_dir().join("plugins")),
            },
            cache_dir: dirs.as_ref().map(|d| d.cache_dir().to_path_buf()),
            exit_watchdog: true,
            mock_hang: mock && env("FERROCORD_MOCK_HANG").as_deref() == Some("1"),
        })
    }

    /// Config used by tests and mock mode helpers.
    pub fn for_tests() -> Self {
        Self {
            mock: true,
            screenshot: None,
            minimized: false,
            tray: false,
            theme: ThemeChoice::Dark,
            gateway_url: DEFAULT_GATEWAY_URL.into(),
            api_url: DEFAULT_API_URL.into(),
            cdn_url: DEFAULT_CDN_URL.into(),
            remote_auth_url: DEFAULT_REMOTE_AUTH_URL.into(),
            zlib: true,
            notification_preview: true,
            trace_startup: false,
            data_dir: None,
            state_dir: None,
            plugins_dir: None,
            cache_dir: None,
            exit_watchdog: false,
            mock_hang: false,
        }
    }

    /// `$XDG_DATA_HOME/ferrocord/cache.sqlite3`, or `None` in mock mode.
    pub fn database_path(&self) -> Option<PathBuf> {
        if self.mock {
            return None;
        }
        self.data_dir.as_ref().map(|d| d.join("cache.sqlite3"))
    }

    /// The plugins' key-value store, `None` (in memory) in mock mode.
    pub fn plugin_store_path(&self) -> Option<PathBuf> {
        if self.mock {
            return None;
        }
        self.data_dir.as_ref().map(|d| d.join("plugins.sqlite3"))
    }

    pub fn log_path(&self) -> Option<PathBuf> {
        self.state_dir.as_ref().map(|d| d.join("log"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        std::iter::once("ferrocord")
            .chain(a.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn parses_mock_and_screenshot() {
        let c = Config::from_args(args(&["--screenshot", "dms", "/tmp/x.png"])).unwrap();
        assert!(c.mock);
        assert_eq!(c.screenshot.as_ref().unwrap().0, ScreenshotScreen::Dms);
        assert!(c.database_path().is_none());
    }

    #[test]
    fn url_validation() {
        assert!(validate_http_url("https://discord.com/api/v9").is_ok());
        assert!(validate_http_url("http://127.0.0.1:8080").is_ok());
        assert!(validate_http_url("http://example.com").is_err());
        assert!(validate_http_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn parses_tray_flags() {
        let c = Config::from_args(args(&["--mock", "--minimized"])).unwrap();
        assert!(c.minimized && c.tray);
        let c = Config::from_args(args(&["--no-tray"])).unwrap();
        assert!(!c.tray && !c.minimized);
        // Screenshots never use the tray.
        let c = Config::from_args(args(&["--screenshot", "settings", "/tmp/x.png"])).unwrap();
        assert!(!c.tray);
        assert_eq!(c.screenshot.unwrap().0, ScreenshotScreen::Settings);
        let c = Config::from_args(args(&["--screenshot", "settings-voice", "/tmp/x.png"])).unwrap();
        assert_eq!(
            c.screenshot.unwrap().0,
            ScreenshotScreen::SettingsPage(crate::app::settings::Category::VoiceVideo)
        );
        assert!(Config::from_args(args(&["--screenshot", "settings-nope", "x"])).is_err());
    }

    #[test]
    fn rejects_unknown() {
        assert!(Config::from_args(args(&["--nope"])).is_err());
        assert!(Config::from_args(args(&["--screenshot", "bogus", "x"])).is_err());
    }
}
